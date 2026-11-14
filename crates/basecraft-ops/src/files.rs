//! Opening and saving documents by file format.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use basecraft_docs::{AlignmentDoc, SequenceDoc, TreeDoc};
use basecraft_io::{Source, fasta, genbank, gff3, newick};
use basecraft_protocol::{Feature, Location, Qualifier, Segment};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    SangerReview,
    GenBank,
    Delimitation,
    /// The project's specimen metadata: a manifest with TSV tables beside
    /// it. See [`crate::metadata`].
    Metadata,
    /// A GenBank marker submission: one JSON document. See
    /// [`crate::submission`].
    Submission,
    /// FASTA, with annotations in a GFF3 sidecar of the same stem.
    Fasta,
    /// Newick or NEXUS (told apart by content), with annotations and style
    /// in its companion: see [`companion_path`].
    Tree,
}

impl Format {
    pub fn from_path(path: &Path) -> Result<Self> {
        if crate::metadata::is_manifest(path) {
            return Ok(Format::Metadata);
        }
        if crate::submission::is_submission(path) {
            return Ok(Format::Submission);
        }
        if crate::sanger_review::evidence_kind(path)
            == Some(crate::sanger_review::EvidenceKind::Review)
        {
            return Ok(Format::SangerReview);
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ext.as_str() {
            "spart" | "spartxml" | "spart-xml" | "xml" => Ok(Format::Delimitation),
            "gb" | "gbk" | "genbank" | "gbff" => Ok(Format::GenBank),
            "fa" | "fasta" | "fna" | "fas" | "ffn" | "faa" | "afa" => Ok(Format::Fasta),
            "nwk" | "newick" | "tre" | "tree" | "trees" | "treefile" | "contree" | "nex"
            | "nexus" | "nxs" => Ok(Format::Tree),
            _ => Err(Error::UnknownFormat(path.to_path_buf())),
        }
    }
}

/// What a file opens as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SangerReview,
    Sequence,
    Delimitation,
    Metadata,
    Submission,
    Alignment,
    Tree,
}

/// Whether a file opens as a sequence or an alignment. A FASTA file is an
/// alignment when it's named `*.afa` or `*.aln.<fasta extension>`, or when
/// it holds two or more records and none is longer than
/// [`LONGEST_ALIGNMENT_ROW`]: aligned or not, a set of genes or organelle
/// genomes reads best as rows. A GFF3 sidecar makes it a sequence: its
/// annotations belong to sequences (explicit alignment names take precedence).
/// The standalone probe streams the entire file without constructing an index.
pub fn kind(path: &Path) -> Result<Kind> {
    match Format::from_path(path)? {
        Format::SangerReview => {
            crate::sanger_review::SangerReview::load(path).map_err(|detail| {
                crate::Error::Damaged {
                    path: path.into(),
                    detail,
                }
            })?;
            return Ok(Kind::SangerReview);
        }
        Format::Delimitation => return Ok(Kind::Delimitation),
        Format::Metadata => return Ok(Kind::Metadata),
        Format::Submission => return Ok(Kind::Submission),
        Format::Tree => return Ok(Kind::Tree),
        Format::GenBank => return Ok(Kind::Sequence),
        Format::Fasta => {}
    }
    let summary = fasta::summary(path)?;
    Ok(fasta_kind(path, summary, gff_sidecar(path).exists()))
}

fn fasta_kind(path: &Path, summary: fasta::Summary, gff_present: bool) -> Kind {
    let named = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("afa"))
        || path
            .file_stem()
            .and_then(|s| Path::new(s).extension())
            .is_some_and(|e| e.eq_ignore_ascii_case("aln"));
    let rows = summary.records >= 2 && summary.longest <= LONGEST_ALIGNMENT_ROW && !gff_present;
    if named || rows {
        Kind::Alignment
    } else {
        Kind::Sequence
    }
}

/// Multi-record FASTA files with a longer record, such as a genome's
/// chromosomes, open as sequences.
pub const LONGEST_ALIGNMENT_ROW: u64 = 1_000_000;

/// An open alignment plus what's needed to write its file back.
pub struct OpenedAlignment {
    pub doc: AlignmentDoc,
    /// Why the alignment's companion could not be used, if it couldn't.
    pub companion_warning: Option<String>,
    /// Rows the GFF3 companion annotates, with their residue counts at open.
    pub annotated: Vec<(String, u64)>,
    pub format: Format,
}

/// An open tree plus what's needed to write its file back.
pub struct OpenedTree {
    pub doc: TreeDoc,
    pub nexus: bool,
    /// Missing bound metadata is reported without changing the exchange.
    pub metadata_warning: Option<String>,
    /// Other forest members are preserved when the first tree is edited.
    pub remaining: Vec<newick::ParsedTree>,
}

pub enum Loaded {
    SangerReview(Box<crate::sanger_review::SangerReview>),
    Sequence(Opened),
    Delimitation(basecraft_protocol::delimitation::DelimitationWorkspace),
    Metadata(crate::metadata::OpenedMetadata),
    Submission(basecraft_protocol::submission::Submission),
    Alignment(OpenedAlignment),
    Tree(OpenedTree),
}

/// `Some(payload)` when the loaded document is of that kind.
macro_rules! payload_of_kind {
    ($($name:ident: $variant:ident => $payload:ty),* $(,)?) => {
        $(pub(crate) fn $name(&self) -> Option<&$payload> {
            if let Self::$variant(payload) = self {
                Some(payload)
            } else {
                None
            }
        })*
    };
}

impl Loaded {
    payload_of_kind! {
        sanger_review: SangerReview => crate::sanger_review::SangerReview,
        sequence: Sequence => Opened,
        alignment: Alignment => OpenedAlignment,
        tree: Tree => OpenedTree,
        metadata: Metadata => crate::metadata::OpenedMetadata,
        submission: Submission => basecraft_protocol::submission::Submission,
        delimitation: Delimitation => basecraft_protocol::delimitation::DelimitationWorkspace,
    }

    pub(crate) fn sanger_review_mut(&mut self) -> Option<&mut crate::sanger_review::SangerReview> {
        if let Self::SangerReview(payload) = self {
            Some(payload)
        } else {
            None
        }
    }

    /// Classify the payload that was loaded, without probing its source again.
    pub(crate) fn kind(&self) -> Kind {
        self.as_document().kind()
    }

    /// Borrow the shared behavior of the existing document payloads.
    pub(crate) fn as_document(&self) -> &dyn crate::documents::Document {
        match self {
            Self::SangerReview(review) => review.as_ref(),
            Self::Submission(sub) => sub,
            Self::Metadata(opened) => opened,
            Self::Alignment(opened) => opened,
            Self::Tree(opened) => opened,
            Self::Sequence(opened) => opened,
            Self::Delimitation(work) => work,
        }
    }

    pub(crate) fn as_document_mut(&mut self) -> &mut dyn crate::documents::Document {
        match self {
            Self::SangerReview(review) => review.as_mut(),
            Self::Submission(sub) => sub,
            Self::Metadata(opened) => opened,
            Self::Alignment(opened) => opened,
            Self::Tree(opened) => opened,
            Self::Sequence(opened) => opened,
            Self::Delimitation(work) => work,
        }
    }
}

/// Open a loose file as whatever it holds: see [`kind`].
pub fn load(path: &Path) -> Result<Loaded> {
    load_in(None, path)
}

/// Open a file of `project` (or a loose file) as whatever it holds, with
/// its companion from where [`companion_path`] keeps it.
pub fn load_in(project: Option<&crate::project::Project>, path: &Path) -> Result<Loaded> {
    let format = Format::from_path(path)?;
    match format {
        Format::SangerReview => crate::sanger_review::SangerReview::load(path)
            .map(|review| Loaded::SangerReview(Box::new(review)))
            .map_err(|detail| crate::Error::Damaged {
                path: path.into(),
                detail,
            }),
        Format::GenBank => Ok(Loaded::Sequence(opened_genbank(
            genbank::open_hashed(path)?.0,
        ))),
        Format::Metadata => crate::metadata::load(path).map(Loaded::Metadata),
        Format::Submission => crate::submission::load(path).map(Loaded::Submission),
        Format::Delimitation => load_delimitation_in(project, path).map(Loaded::Delimitation),
        Format::Fasta | Format::Tree => load_source(project, path, format, Source::open(path)?),
    }
}

/// Load FASTA, GenBank or a tree from the supplied main-file snapshot.
/// Companion files are still read from disk; callers needing their exact hashes
/// must retain the companion validation used by `Session`.
/// Other formats require their existing loaders and are rejected here.
pub fn load_from(path: &Path, source: Source) -> Result<Loaded> {
    load_source(None, path, Format::from_path(path)?, source)
}

fn load_source(
    project: Option<&crate::project::Project>,
    path: &Path,
    format: Format,
    source: Source,
) -> Result<Loaded> {
    match format {
        Format::Tree => load_tree_from(project, path, &source).map(Loaded::Tree),
        Format::Fasta => {
            let gff = read_optional(&gff_sidecar(path))?;
            load_fasta_from(
                path,
                fasta::IndexedFasta::from_source(source)?,
                gff.as_ref(),
            )
        }
        Format::GenBank => Ok(Loaded::Sequence(opened_genbank(genbank::parse_first(
            source.bytes(),
        )?))),
        _ => Err(crate::error::invalid(
            "this format does not support loading an owned source",
        )),
    }
}

/// Exact inputs retained until admission and recovery preparation finish.
pub(crate) struct CapturedLoad {
    pub content: Loaded,
    /// Why a companion was ignored, for the opening notice.
    pub warning: Option<String>,
    pub files: Vec<(PathBuf, String)>,
    pub stamps: Vec<(PathBuf, Option<basecraft_protocol::provenance::FileStamp>)>,
}

pub(crate) fn capture_load(
    project: Option<&crate::project::Project>,
    path: &Path,
    format: Format,
) -> Result<CapturedLoad> {
    if format == Format::GenBank {
        let stamps = vec![(
            path.to_path_buf(),
            Some(crate::records::input_evidence::stamp(path)?),
        )];
        let (first, hash) = genbank::open_hashed(path)?;
        return finish_capture(
            project,
            Loaded::Sequence(opened_genbank(first)),
            vec![(path.to_path_buf(), hash)],
            stamps,
        );
    }
    let companions = match format {
        Format::Metadata => crate::metadata::files(path).split_off(1),
        Format::Tree | Format::Delimitation => vec![companion_path(project, path)],
        Format::Fasta => vec![gff_sidecar(path)],
        Format::GenBank => unreachable!("streamed above"),
        Format::Submission => crate::documents::submission_companions(path),
        Format::SangerReview => Vec::new(),
    };
    let mut stamps = Vec::new();
    let mut files = Vec::new();
    let mut fasta_index = None;
    let mut tree_issue = None;
    for file in std::iter::once(path.to_path_buf()).chain(companions) {
        let metadata = match std::fs::metadata(&file) {
            Ok(metadata) if !metadata.is_file() && format == Format::Tree && file != path => {
                // Unreadable tree metadata leaves the exchange tree open with a warning.
                tree_issue = Some(format!("{} is not an ordinary file", file.display()));
                None
            }
            Ok(metadata) if !metadata.is_file() => {
                let error = io::Error::new(io::ErrorKind::InvalidInput, "not an ordinary file");
                return Err(basecraft_io::Error::Io(error).into());
            }
            Ok(metadata) => Some(crate::records::input_evidence::stamp_metadata(
                &file, &metadata,
            )?),
            Err(error) if error.kind() == io::ErrorKind::NotFound && file != path => None,
            Err(error) => return Err(error.into()),
        };
        if metadata.is_some() {
            let (source, hash) = if format == Format::Fasta && file == path {
                let (source, index, hash) = Source::open_fasta(&file)?;
                fasta_index = Some(index);
                (source, hash)
            } else {
                Source::open_hashed(&file)?
            };
            files.push((file.clone(), source, hash));
        }
        stamps.push((file, metadata));
    }
    let main = files[0].1.clone();
    let companion = files.get(1).map(|(_, source, _)| source);
    let mut warning = None;
    let content = match format {
        Format::Fasta => load_fasta_from(
            path,
            fasta::IndexedFasta {
                source: main,
                index: fasta_index.expect("FASTA indexed during capture"),
            },
            companion,
        )?,
        Format::GenBank => unreachable!("streamed above"),
        Format::SangerReview => {
            if main.bytes().len() > 64 * 1024 * 1024 {
                return Err(Error::Limit {
                    what: "Sanger review bytes".into(),
                    limit: 64 * 1024 * 1024,
                });
            }
            let review = crate::sanger_review::SangerReview::decode(main.bytes(), path).map_err(
                |detail| Error::Damaged {
                    path: path.into(),
                    detail,
                },
            )?;
            Loaded::SangerReview(Box::new(review))
        }
        Format::Tree => Loaded::Tree(load_tree_bytes(
            project,
            path,
            &main,
            &files[0].2,
            (&companion_path(project, path), companion),
            tree_issue,
        )?),
        Format::Delimitation => {
            let (work, ignored) = load_delimitation_bytes(
                project,
                path,
                main.bytes(),
                &files[0].2,
                (&companion_path(project, path), companion),
            )?;
            warning = ignored;
            Loaded::Delimitation(work)
        }
        Format::Submission => Loaded::Submission(crate::submission::load_from(path, main.bytes())?),
        Format::Metadata => Loaded::Metadata(crate::metadata::load_with(path, &mut |file| {
            files
                .iter()
                .find(|(p, _, _)| p == file)
                .map(|(_, source, hash)| (source.clone(), hash.clone()))
                .ok_or_else(|| Error::NotFound {
                    path: file.to_path_buf(),
                })
        })?),
    };
    let mut captured = finish_capture(
        project,
        content,
        files
            .into_iter()
            .map(|(path, _, hash)| (path, hash))
            .collect(),
        stamps,
    )?;
    captured.warning = warning;
    Ok(captured)
}

fn finish_capture(
    project: Option<&crate::project::Project>,
    mut content: Loaded,
    files: Vec<(PathBuf, String)>,
    stamps: Vec<(PathBuf, Option<basecraft_protocol::provenance::FileStamp>)>,
) -> Result<CapturedLoad> {
    restore_alphabet(project, &mut content, &files[0].0, &files[0].1);
    for (file, before) in &stamps {
        let after = match std::fs::metadata(file) {
            Ok(metadata) if !metadata.is_file() => None,
            Ok(metadata) => Some(crate::records::input_evidence::stamp_metadata(
                file, &metadata,
            )?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if *before != after {
            return Err(Error::ChangedOnDisk { path: file.clone() });
        }
    }
    Ok(CapturedLoad {
        content,
        warning: None,
        files,
        stamps,
    })
}

fn read_optional(path: &Path) -> Result<Option<Source>> {
    match Source::open(path) {
        Ok(source) => Ok(Some(source)),
        Err(basecraft_io::Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn load_fasta_from(
    path: &Path,
    file: fasta::IndexedFasta,
    captured_gff: Option<&Source>,
) -> Result<Loaded> {
    let file = Arc::new(file);
    if file.index.records.is_empty() {
        return Err(Error::Empty(path.to_path_buf()));
    }
    let summary = fasta::Summary {
        records: file.index.records.len() as u64,
        longest: file.index.records.iter().map(|r| r.len).max().unwrap_or(0),
    };
    let gff_present = captured_gff.is_some() || gff_sidecar(path).exists();
    if fasta_kind(path, summary, gff_present) == Kind::Alignment {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let doc = AlignmentDoc::from_fasta(name, &file);
        Ok(Loaded::Alignment(OpenedAlignment {
            companion_warning: None,
            annotated: annotated_rows(&doc, captured_gff),
            doc,
            format: Format::Fasta,
        }))
    } else {
        open_fasta_with_gff(path, file, captured_gff.map(Source::bytes)).map(Loaded::Sequence)
    }
}

/// Residues of an alignment row, gaps excluded.
/// Reads the row in chunks, so a long row is never held whole.
pub(crate) fn ungapped_len(doc: &AlignmentDoc, row: usize) -> u64 {
    let chunk = 60 * 16_384;
    let mut residues = Vec::new();
    let mut count = 0;
    let mut pos = 0;
    while pos < doc.width() {
        let end = (pos + chunk).min(doc.width());
        residues.clear();
        doc.read_into(row, pos, end, &mut residues);
        count += residues
            .iter()
            .filter(|&&byte| !basecraft_docs::alignment::is_gap(byte))
            .count() as u64;
        pos = end;
    }
    count
}

/// The rows a companion annotates. An unreadable companion counts every row.
fn annotated_rows(doc: &AlignmentDoc, gff: Option<&Source>) -> Vec<(String, u64)> {
    let Some(gff) = gff else {
        return Vec::new();
    };
    let named: Option<std::collections::BTreeSet<String>> =
        gff3::parse(gff.bytes()).ok().map(|gff| {
            gff.sequences
                .into_iter()
                .map(|seq| seq.seqid)
                .chain(gff.regions.into_iter().map(|region| region.seqid))
                .collect()
        });
    doc.rows()
        .enumerate()
        .filter(|(_, row)| named.as_ref().is_none_or(|named| named.contains(&row.name)))
        .map(|(index, row)| (row.name.clone(), ungapped_len(doc, index)))
        .collect()
}

/// Write explicit alignment contents as FASTA, 60 columns a line.
/// Synthetic suffix padding used to display ragged input is never materialised.
pub fn write_alignment(doc: &AlignmentDoc, w: &mut dyn Write) -> io::Result<()> {
    basecraft_io::counters::record_serialization("alignment");
    let mut buf = Vec::new();
    let chunk = 60 * 16_384;
    for (i, row) in doc.rows().enumerate() {
        w.write_all(b">")?;
        w.write_all(row.name.as_bytes())?;
        if !row.description.is_empty() {
            w.write_all(b" ")?;
            w.write_all(row.description.as_bytes())?;
        }
        w.write_all(b"\n")?;
        let mut pos = 0;
        let len = doc.saved_row_len(i);
        while pos < len {
            let end = (pos + chunk).min(len);
            buf.clear();
            doc.read_into(i, pos, end, &mut buf);
            for line in buf.chunks(60) {
                w.write_all(line)?;
                w.write_all(b"\n")?;
            }
            pos = end;
        }
    }
    Ok(())
}

/// The alignment's rows without gaps, as FASTA: the input for an aligner.
pub fn write_ungapped(doc: &AlignmentDoc, w: &mut dyn Write) -> io::Result<()> {
    let mut buf = Vec::new();
    for (i, row) in doc.rows().enumerate() {
        buf.clear();
        doc.read_into(i, 0, doc.width(), &mut buf);
        buf.retain(|&b| !basecraft_docs::alignment::is_gap(b));
        fasta::write_record(w, row.name.as_bytes(), &buf, 60)?;
    }
    Ok(())
}

pub fn gff_sidecar(path: &Path) -> PathBuf {
    path.with_extension("gff3")
}

/// Where a document's companion lives: the one place that knows (D23). A
/// result bound to an analysis run keeps it in that run; any other document
/// in the `.basecraft` of `project` or of its own folder, keyed by its
/// relative path ([`crate::project::Project::document_record`]).
pub fn companion_path(project: Option<&crate::project::Project>, path: &Path) -> PathBuf {
    if let Ok(Some(run)) = crate::records::analysis_store::ArtifactRun::for_result(project, path) {
        return run_companion(&run, path);
    }
    crate::project::Project::document_record(project, path)
}

/// The name a result's companion has in the analysis run it is bound to.
pub fn run_companion_name(file: &Path) -> String {
    format!(
        "{}.basecraft.json",
        file.file_name().unwrap_or_default().to_string_lossy()
    )
}

/// The companion of `file` inside run folder `run`.
pub fn run_companion(run: &Path, file: &Path) -> PathBuf {
    run.join(run_companion_name(file))
}

/// Copy a document to a new `target` with its companion and, for a tree,
/// its alignment links, each going where `target`'s records are kept. The
/// copy is a new document: it starts its own saved history. An existing
/// target is refused and never replaced; a record left at the target's
/// place by a file that is gone is replaced. If a record can't be copied,
/// the target is removed again.
pub fn copy_document(
    project: Option<&crate::project::Project>,
    source: &Path,
    target: &Path,
) -> Result<()> {
    let (from, to) = (
        companion_path(project, source),
        companion_path(project, target),
    );
    let folder = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staged = tempfile::NamedTempFile::new_in(folder)?;
    std::fs::copy(source, staged.path())?;
    if let Err(error) = basecraft_platform::atomic::rename_noreplace(staged.path(), target) {
        return Err(if error.kind() == io::ErrorKind::AlreadyExists {
            crate::error::invalid(format!(
                "{} already exists; choose a new name",
                target.display()
            ))
        } else {
            error.into()
        });
    }
    let companion = (|| -> Result<()> {
        if to.exists() {
            // The target did not exist: this record is an orphan.
            std::fs::remove_file(&to)?;
        }
        if from.is_file() {
            companion_folder(&to)?;
            basecraft_io::write_atomic(&to, |w| {
                io::copy(&mut std::fs::File::open(&from)?, w)?;
                Ok(())
            })?;
        }
        let linked = crate::records::links::metadata_present(&crate::records::links::sidecar_in(
            project, source,
        ))? || crate::records::links::metadata_present(
            &crate::records::links::view_sidecar_in(project, source),
        )?;
        if linked {
            crate::records::links::PreparedLinks::prepare(project, source, target)?
                .publish(|| Ok(()))?;
        }
        Ok(())
    })();
    if companion.is_err() {
        let _ = std::fs::remove_file(target);
    }
    companion
}

/// Move the companion a document got as a loose file to where `project`
/// keeps it, when that is somewhere else: for a document published as part
/// of a folder, such as a recovery, into a project. Folders the move
/// leaves empty go too, so nothing remains beside the document.
pub(crate) fn settle_companion(
    project: Option<&crate::project::Project>,
    document: &Path,
) -> Result<()> {
    let loose = crate::project::Project::document_record(None, document);
    let kept = companion_path(project, document);
    // Only the companion is settled. A loose alignment link would need its
    // snapshot path rewritten; none is made in a stage today, so one is an
    // error to report, never a record to strand.
    if loose != kept {
        for link in [
            crate::records::links::sidecar_in(None, document),
            crate::records::links::view_sidecar_in(None, document),
        ] {
            if link.exists() {
                return Err(crate::error::invalid(format!(
                    "the recovered {} has an alignment link in {}; move it with the tree before reopening it",
                    document.display(),
                    link.display()
                )));
            }
        }
    }
    if loose == kept || !loose.is_file() {
        return Ok(());
    }
    if crate::records::analysis_store::ArtifactRun::for_result(project, document)?.is_some() {
        // The save wrote the companion into the result's run; the loose one
        // is the input recovery replayed from.
        std::fs::remove_file(&loose)?;
    } else {
        // Replaces a record an earlier file at this path left behind.
        companion_folder(&kept)?;
        std::fs::rename(&loose, &kept)?;
    }
    remove_empty_record_folders(&loose);
    Ok(())
}

/// Remove the folders between `record` and its `.basecraft`, that folder
/// included, while they are empty.
fn remove_empty_record_folders(record: &Path) {
    for folder in record.ancestors().skip(1) {
        let last = folder.file_name().is_some_and(|name| name == ".basecraft");
        if std::fs::remove_dir(folder).is_err() || last {
            break;
        }
    }
}

/// Whether a recorded file `key` is the companion of document `main`:
/// either in a `.basecraft/documents` folder, or, for a species workspace,
/// in the analysis run it is bound to.
pub(crate) fn is_companion_key(key: &Path, main: &Path) -> bool {
    let Some(name) = main.file_name() else {
        return false;
    };
    let has = |part: &str| key.components().any(|c| c.as_os_str() == part);
    let mut record = name.to_os_string();
    record.push(".json");
    let bound = run_companion_name(main);
    (has(".basecraft") && has("documents") && key.file_name() == Some(record.as_os_str()))
        || (Format::from_path(main).ok() == Some(Format::Delimitation)
            && has("analyses-v1")
            && key
                .file_name()
                .is_some_and(|file| file.to_string_lossy() == bound))
}

/// Make the folder a companion is written into. A `.basecraft` that is a
/// symbolic link or another program's file is refused, not followed.
pub(crate) fn companion_folder(companion: &Path) -> Result<()> {
    let Some(folder) = companion.parent() else {
        return Ok(());
    };
    let base = companion
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == ".basecraft"))
        .and_then(Path::parent);
    if let Some(reason) = base.and_then(crate::records::input_evidence::foreign_basecraft) {
        return Err(crate::error::invalid(format!(
            "Basecraft can't keep this document's facts in {}: {reason}",
            base.unwrap_or(folder).display()
        )));
    }
    std::fs::create_dir_all(folder)?;
    Ok(())
}

use basecraft_protocol::companion_records::{
    COMPANION_VERSION, Companion, DelimitationCompanion, SupportName, TreeCompanion,
};
use basecraft_protocol::facts::{Alphabet, AlphabetCounts, Fact, FactSource};

/// Read a document's companion. Refuses another layout, a companion written
/// for different bytes than the document's, and for a tree, a companion that
/// describes a different tree.
fn read_companion(
    path: &Path,
    bytes: &[u8],
    exchange_hash: &str,
    tree: Option<&TreeDoc>,
) -> Result<Companion> {
    let (companion, writer) = basecraft_protocol::records::decode_written::<Companion>(bytes)?;
    if companion.version != COMPANION_VERSION {
        return Err(Error::Unsupported(format!(
            "{}: {}",
            path.display(),
            basecraft_protocol::records::other_format(
                &writer,
                companion.version,
                COMPANION_VERSION
            )
        )));
    }
    if companion.exchange_hash != exchange_hash {
        return Err(Error::Damaged {
            path: path.into(),
            detail: "the file it describes was changed outside basecraft".into(),
        });
    }
    if let Some(tree) = tree
        && companion.tree.as_ref().map(|t| &t.tree_hash) != Some(&tree_hash(tree))
    {
        return Err(Error::Damaged {
            path: path.into(),
            detail: "Tree metadata describes a different tree".into(),
        });
    }
    Ok(companion)
}

/// Read the first tree of a loose Newick or NEXUS file, with its companion.
pub fn load_tree(path: &Path) -> Result<OpenedTree> {
    load_tree_in(None, path)
}

/// Read the first tree of a Newick or NEXUS file of `project`, with its
/// companion.
pub fn load_tree_in(project: Option<&crate::project::Project>, path: &Path) -> Result<OpenedTree> {
    load_tree_from(project, path, &open_source(path, Format::Tree)?)
}

pub(crate) fn load_tree_from(
    project: Option<&crate::project::Project>,
    path: &Path,
    source: &Source,
) -> Result<OpenedTree> {
    let companion = companion_path(project, path);
    let (side, issue) = match read_optional(&companion) {
        Ok(side) => (side, None),
        Err(error) => (None, Some(error.to_string())),
    };
    load_tree_bytes(
        project,
        path,
        source,
        &basecraft_io::hash_bytes(source.bytes()),
        (&companion, side.as_ref()),
        issue,
    )
}

/// `captured_side` is the companion and its bytes, read with the main
/// file; `side_issue` says why the companion could not be read.
fn load_tree_bytes(
    project: Option<&crate::project::Project>,
    path: &Path,
    source: &Source,
    exchange_hash: &str,
    (companion, captured_side): (&Path, Option<&Source>),
    side_issue: Option<String>,
) -> Result<OpenedTree> {
    let text = std::str::from_utf8(source.bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let nexus = newick::is_nexus(text);
    let mut parsed_trees = newick::parse_trees(text)?.into_iter();
    let parsed = parsed_trees
        .next()
        .ok_or_else(|| Error::Empty(path.to_path_buf()))?;
    let remaining = parsed_trees.collect();
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut doc = TreeDoc::from_parsed(name, &parsed);
    drop(parsed);
    let mut metadata_warning = None;
    let metadata = (|| -> Result<()> {
        if let Some(issue) = side_issue {
            return Err(Error::Io(io::Error::other(issue)));
        }
        // A broken hidden binding never falls back to the companion beside the file.
        let run = crate::records::analysis_store::ArtifactRun::for_result_with_hash(
            project,
            path,
            exchange_hash,
        )?;
        let bound = run.is_some();
        let (sidecar, side) = match run {
            Some(run) => {
                let sidecar = run_companion(&run, path);
                let side = read_optional(&sidecar)?;
                (sidecar, side)
            }
            None => (companion.to_path_buf(), captured_side.cloned()),
        };
        if let Some(bytes) = &side {
            let side = read_companion(&sidecar, bytes.bytes(), exchange_hash, Some(&doc))?
                .tree
                .expect("the reader refuses a tree companion without a tree section");
            let kinds: Vec<_> = side
                .supports
                .iter()
                .map(|s| (s.name.clone(), s.kind))
                .collect();
            let names: Vec<(String, String)> = side
                .supports
                .into_iter()
                .map(|s| (s.name, s.source))
                .collect();
            doc.restore(&names, side.annotations, side.style);
            doc.restore_branch_lengths(side.branch_lengths);
            for (name, kind) in kinds {
                doc.set_support_kind(&name, kind);
            }
        }
        if bound && side.is_none() {
            metadata_warning = Some("Bound tree metadata is unavailable; exchange values remain open with unknown support meaning and default presentation.".into());
        }
        Ok(())
    })();
    if let Err(error) = metadata {
        metadata_warning = Some(format!("Tree metadata was ignored: {error}"));
    }
    Ok(OpenedTree {
        doc,
        nexus,
        metadata_warning,
        remaining,
    })
}

/// Write a tree as Newick, or as NEXUS if it was read from NEXUS.
pub fn write_tree(tree: &OpenedTree, w: &mut dyn Write) -> io::Result<()> {
    if tree.nexus {
        writeln!(w, "#NEXUS\nbegin trees;")?;
    }
    if tree.nexus {
        let name = if tree.doc.exchange_name.is_empty() {
            "tree1"
        } else {
            &tree.doc.exchange_name
        };
        write!(w, "\ttree {} = ", newick::quote_name(name))?;
    }
    writeln!(w, "{}", tree.doc.to_newick_with_rooting())?;
    for member in &tree.remaining {
        if tree.nexus {
            write!(w, "\ttree {} = ", newick::quote_name(&member.name))?;
        }
        newick::write_parsed(member, w)?;
        writeln!(w)?;
    }
    if tree.nexus {
        writeln!(w, "end;")?;
    }
    Ok(())
}

fn tree_hash(doc: &TreeDoc) -> String {
    let mut writer = HashingWriter::new(io::sink());
    doc.write_newick(&mut writer)
        .expect("hashing writer cannot fail");
    writer.hash()
}

/// The companion of an alignment or sequence that holds a stated fact, and
/// nothing for one that holds none: a plain open and save writes no file.
pub fn facts_companion(
    alphabet: Option<&Fact<Alphabet>>,
    exchange_hash: &str,
) -> Option<Companion> {
    alphabet.map(|alphabet| Companion {
        alphabet: Some(alphabet.clone()),
        ..Companion::new(exchange_hash)
    })
}

/// Put back the alphabet an alignment's or sequence's companion keeps. A
/// companion that no longer matches the file is ignored, with a warning.
fn restore_alphabet(
    project: Option<&crate::project::Project>,
    content: &mut Loaded,
    path: &Path,
    exchange_hash: &str,
) {
    // Trees and species files read their side file in their own loader.
    if !matches!(content, Loaded::Sequence(_) | Loaded::Alignment(_)) {
        return;
    }
    let companion = companion_path(project, path);
    let found = read_optional(&companion).and_then(|bytes| {
        bytes
            .map(|bytes| read_companion(&companion, bytes.bytes(), exchange_hash, None))
            .transpose()
    });
    let (stated, warning) = match found {
        Ok(found) => (found.and_then(|c| c.alphabet), None),
        Err(error) => (
            None,
            Some(format!("The companion file was ignored: {error}")),
        ),
    };
    match content {
        Loaded::Sequence(opened) => {
            opened.doc.set_alphabet(stated);
            opened.companion_warning = warning;
        }
        Loaded::Alignment(opened) => {
            opened.doc.set_alphabet(stated);
            opened.companion_warning = warning;
        }
        _ => {}
    }
}

/// A tree's companion, for the saved file whose bytes hash to `exchange_hash`.
pub fn tree_companion(doc: &TreeDoc, exchange_hash: &str) -> Companion {
    Companion {
        tree: Some(TreeCompanion {
            tree_hash: tree_hash(doc),
            supports: doc
                .supports()
                .iter()
                .map(|s| SupportName {
                    name: s.name.clone(),
                    source: s.source.clone(),
                    kind: s.kind,
                })
                .collect(),
            annotations: doc.annotations().clone(),
            style: doc.style().clone(),
            branch_lengths: doc.branch_lengths().cloned(),
        }),
        ..Companion::new(exchange_hash)
    }
}

/// Replace the companion of a tree file already on disk.
pub fn save_tree_companion(
    project: Option<&crate::project::Project>,
    tree: &Path,
    doc: &TreeDoc,
) -> Result<PathBuf> {
    let companion = tree_companion(doc, &basecraft_io::hash_file(tree)?);
    let path = companion_path(project, tree);
    companion_folder(&path)?;
    basecraft_io::write_atomic(&path, |w| companion.write(w))?;
    Ok(path)
}

/// An open document plus what's needed to write its file back.
pub struct Opened {
    pub doc: SequenceDoc,
    /// Why the sequence's companion could not be used, if it couldn't.
    pub companion_warning: Option<String>,
    pub format: Format,
    /// For FASTA: the original file, so records other than the open one are
    /// copied through on save.
    pub fasta: Option<Arc<fasta::IndexedFasta>>,
    pub others: OtherRecords,
}

/// The records a sequence document does not show, written back unchanged on save.
#[derive(Clone, Default)]
pub struct OtherRecords {
    /// Records in the file, the open one included.
    pub count: usize,
    /// Record names that more than one record in the file uses. Edits name
    /// their record, so they are refused while this is not empty.
    pub repeated_names: Vec<String>,
    /// The alphabet of each record after the open one (D12.5), stated by
    /// the file or guessed, read once at open.
    pub alphabets: Vec<(String, Fact<Alphabet>)>,
    /// GenBank: the exact bytes after the first record, held once.
    pub genbank: Arc<Vec<u8>>,
    /// FASTA: the GFF3 companion without the open record's lines.
    pub gff: Option<Arc<gff3::Kept>>,
}

fn open_source(path: &Path, format: Format) -> Result<Source> {
    Source::open(path).map_err(|error| match (format, error) {
        (Format::GenBank | Format::Tree, basecraft_io::Error::Io(error)) => Error::Io(error),
        (_, error) => Error::Format(error),
    })
}

/// Open the first record of a sequence file.
pub fn open(path: &Path) -> Result<Opened> {
    match Format::from_path(path)? {
        Format::GenBank => Ok(opened_genbank(genbank::open_hashed(path)?.0)),
        Format::Fasta => open_fasta_from(
            path,
            Arc::new(fasta::IndexedFasta::from_source(Source::open(path)?)?),
        ),
        Format::SangerReview => Err(crate::error::invalid("this is a Sanger review")),
        Format::Delimitation => Err(crate::error::invalid("this is a delimitation workspace")),
        Format::Metadata => Err(crate::error::invalid(
            "this is the project's specimen metadata",
        )),
        Format::Submission => Err(crate::error::invalid("this is a GenBank submission")),
        Format::Tree => Err(crate::error::invalid(format!(
            "{} is a tree, not a sequence",
            path.display()
        ))),
    }
}

fn opened_genbank(first: genbank::First) -> Opened {
    let names: Vec<String> = std::iter::once(first.record.name.clone())
        .chain(locus_names(&first.rest).map(|(_, name)| String::from_utf8_lossy(name).into_owned()))
        .collect();
    Opened {
        companion_warning: None,
        doc: SequenceDoc::from_genbank(first.record),
        format: Format::GenBank,
        fasta: None,
        others: OtherRecords {
            count: first.records,
            repeated_names: repeated(&names),
            alphabets: genbank_alphabets(&first.rest),
            genbank: Arc::new(first.rest),
            gff: None,
        },
    }
}

fn open_fasta_from(path: &Path, file: Arc<fasta::IndexedFasta>) -> Result<Opened> {
    if file.index.records.is_empty() {
        return Err(Error::Empty(path.to_path_buf()));
    }
    let sidecar = gff_sidecar(path);
    let gff = if sidecar.exists() {
        let bytes = std::fs::read(&sidecar)?;
        basecraft_io::counters::record_read(&sidecar, bytes.len() as u64);
        Some(bytes)
    } else {
        None
    };
    open_fasta_with_gff(path, file, gff.as_deref())
}

fn open_fasta_with_gff(
    path: &Path,
    file: Arc<fasta::IndexedFasta>,
    gff_bytes: Option<&[u8]>,
) -> Result<Opened> {
    if file.index.records.is_empty() {
        return Err(Error::Empty(path.to_path_buf()));
    }
    let gff = gff_bytes.map(gff3::parse).transpose()?;
    let name = file.index.records[0].name.clone();
    let features = gff.and_then(|g| g.sequences.into_iter().find(|s| s.seqid == name));
    let doc = SequenceDoc::from_fasta(Arc::clone(&file), 0, features);
    Ok(Opened {
        companion_warning: None,
        doc,
        format: Format::Fasta,
        others: OtherRecords {
            count: file.index.records.len(),
            repeated_names: repeated(
                &file
                    .index
                    .records
                    .iter()
                    .map(|r| r.name.clone())
                    .collect::<Vec<_>>(),
            ),
            alphabets: fasta_alphabets(&file),
            genbank: Arc::default(),
            gff: gff_bytes.map(|bytes| Arc::new(gff3::keep_others(bytes, &name))),
        },
        fasta: Some(file),
    })
}

/// Each FASTA record after the first, with its guessed alphabet.
fn fasta_alphabets(file: &fasta::IndexedFasta) -> Vec<(String, Fact<Alphabet>)> {
    const PIECE: u64 = 1 << 16;
    let mut buf = Vec::new();
    let records = file.index.records.iter().skip(1);
    records
        .map(|record| {
            let mut counts = AlphabetCounts::default();
            let mut start = 0;
            while start < record.len && !counts.is_full() {
                buf.clear();
                let end = (start + PIECE).min(record.len);
                if record
                    .read_into(file.bytes(), start, end, &mut buf)
                    .is_err()
                {
                    break;
                }
                counts.read(&buf);
                start = end;
            }
            (record.name.clone(), Alphabet::guessed(counts.alphabet()))
        })
        .collect()
}

/// Each GenBank record in `rest`, with the alphabet its LOCUS line states
/// ("aa", or a DNA or RNA molecule type), else guessed from its ORIGIN.
fn genbank_alphabets(rest: &[u8]) -> Vec<(String, Fact<Alphabet>)> {
    let finish = |(name, stated, counts): (String, Option<Alphabet>, AlphabetCounts)| {
        let fact = stated.map_or_else(
            || Alphabet::guessed(counts.alphabet()),
            |value| Fact {
                value,
                source: FactSource::Stated,
            },
        );
        (name, fact)
    };
    let mut out = Vec::new();
    let mut record: Option<(String, Option<Alphabet>, AlphabetCounts)> = None;
    let mut origin = false;
    for line in rest.split(|&b| b == b'\n') {
        if line.starts_with(b"LOCUS") {
            out.extend(record.take().map(finish));
            let text = String::from_utf8_lossy(line);
            let stated = genbank::locus_alphabet(&text);
            let name = text
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            record = Some((name, stated, AlphabetCounts::default()));
            origin = false;
        } else if line.starts_with(b"ORIGIN") {
            origin = true;
        } else if line.starts_with(b"//") {
            origin = false;
        } else if origin && let Some((_, _, counts)) = &mut record {
            counts.read(line);
        }
    }
    out.extend(record.map(finish));
    out
}

fn repeated(names: &[String]) -> Vec<String> {
    crate::renames::duplicate_names(names).map_or_else(Vec::new, |d| d.names)
}

/// Each GenBank record's name, the first word after LOCUS, with the byte
/// offset where it is written.
pub(crate) fn locus_names(bytes: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let mut at = 0;
    bytes
        .split_inclusive(|&b| b == b'\n')
        .filter_map(move |line| {
            let offset = at;
            at += line.len();
            let rest = line.strip_prefix(b"LOCUS")?;
            if !rest.first().is_some_and(u8::is_ascii_whitespace) {
                return None;
            }
            let start = 5 + rest.iter().take_while(|b| b.is_ascii_whitespace()).count();
            let len = line[start..]
                .iter()
                .take_while(|b| !b.is_ascii_whitespace())
                .count();
            Some((offset + start, &line[start..start + len]))
        })
}

/// Serialise the main file of a document.
pub fn write_main(opened: &Opened, w: &mut dyn Write) -> io::Result<()> {
    basecraft_io::counters::record_serialization("sequence");
    let doc = &opened.doc;
    match opened.format {
        Format::GenBank => {
            genbank::write_stream(
                w,
                &doc.genbank_metadata(),
                doc.len(),
                doc.features().map(|(_, feature)| feature),
                (0..doc.len())
                    .step_by(60 * 1024)
                    .map(|start| doc.read(start, (start + 60 * 1024).min(doc.len()))),
            )?;
            w.write_all(&opened.others.genbank)
        }
        Format::SangerReview => Err(io::Error::other(
            "a sequence cannot be written as a Sanger review",
        )),
        Format::Delimitation => Err(io::Error::other("this is a delimitation workspace")),
        Format::Metadata => Err(io::Error::other("a sequence can't be written as metadata")),
        Format::Submission => Err(io::Error::other(
            "a sequence can't be written as a submission",
        )),
        Format::Tree => Err(io::Error::other("a sequence can't be written as a tree")),
        Format::Fasta => {
            let Some(file) = &opened.fasta else {
                return write_fasta_record(doc, None, b"\n", w);
            };
            let (bytes, records) = (file.bytes(), &file.index.records);
            #[allow(clippy::cast_possible_truncation)]
            let start = records[0].header_offset as usize;
            // Text before the first record (blank lines, comments) stays.
            w.write_all(&bytes[..start])?;
            let header = records[0].header(bytes);
            let crlf = bytes.get(start + 1 + header.len()) == Some(&b'\r');
            let eol: &[u8] = if crlf { b"\r\n" } else { b"\n" };
            write_fasta_record(doc, Some(header), eol, w)?;
            // The other records, byte for byte.
            if let Some(next) = records.get(1) {
                #[allow(clippy::cast_possible_truncation)]
                w.write_all(&file.bytes()[next.header_offset as usize..])?;
            }
            Ok(())
        }
    }
}

/// `eol` keeps the line ending the file already uses.
fn write_fasta_record(
    doc: &SequenceDoc,
    header: Option<&[u8]>,
    eol: &[u8],
    w: &mut dyn Write,
) -> io::Result<()> {
    let header = header.map_or_else(|| doc.name.as_bytes().to_vec(), <[u8]>::to_vec);
    // Stream in chunks so a huge record never sits in memory twice.
    w.write_all(b">")?;
    w.write_all(&header)?;
    w.write_all(eol)?;
    let mut buf = Vec::new();
    let chunk = 60 * 16_384;
    let mut pos = 0;
    while pos < doc.len() {
        let end = (pos + chunk).min(doc.len());
        buf.clear();
        doc.read_into(pos, end, &mut buf);
        for line in buf.chunks(60) {
            w.write_all(line)?;
            w.write_all(eol)?;
        }
        pos = end;
    }
    Ok(())
}

/// Whether a save writes the GFF3 file beside a FASTA (D27): when there
/// are features, the sequence is circular, or a GFF3 was read with it (or
/// this session wrote one for it). Such a GFF3 is written again, empty if
/// every feature went; basecraft never deletes it.
pub fn writes_gff(opened: &Opened) -> bool {
    opened.format == Format::Fasta
        && (opened.others.gff.is_some()
            || opened.doc.circular
            || opened.doc.features().next().is_some())
}

/// The GFF3 file beside a FASTA, when [`writes_gff`].
pub fn write_sidecar(opened: &Opened, w: &mut dyn Write) -> Option<io::Result<()>> {
    writes_gff(opened).then(|| {
        let feats = opened.doc.to_gff();
        match &opened.others.gff {
            Some(kept) => gff3::write_kept(w, kept, &feats, opened.doc.len()),
            None => gff3::write(w, &[(&feats, opened.doc.len())]),
        }
    })
}

/// A writer that hashes what passes through it.
pub struct HashingWriter<W> {
    pub inner: W,
    pub hasher: basecraft_io::Hasher,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: basecraft_io::Hasher::new(),
        }
    }

    pub fn hash(&self) -> String {
        self.hasher.finish()
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// A new GenBank record for an extract of `doc`:
/// - `segments` empty: bases `start..end`, with every annotation that
///   overlaps them, clipped and shifted; an annotation reaching past the
///   range is marked incomplete at that end (GenBank `<`/`>`).
/// - `segments` given (5' to 3'): those parts spliced together, carrying
///   the annotations whose parts are exactly these, as one continuous
///   annotation. This is how a spliced CDS is extracted.
///
/// With `reverse`, the record is the reverse complement, so a reverse-
/// strand annotation reads 5' to 3' and lies on the forward strand.
/// Biological sequence of an annotation, in its stored segment order.
/// Bounds are checked before the document's range reader is called.
pub fn feature_sequence(
    doc: &SequenceDoc,
    feature: &basecraft_protocol::Feature,
) -> Result<Vec<u8>> {
    if feature.location.segments.is_empty()
        || feature
            .location
            .segments
            .iter()
            .any(|s| s.start >= s.end || s.end > doc.len())
    {
        return Err(crate::error::invalid(
            "Annotation is empty or outside the sequence",
        ));
    }
    let mut sequence = Vec::new();
    for segment in &feature.location.segments {
        let part = extract_record(
            doc,
            segment.start,
            segment.end,
            &[],
            segment.strand == basecraft_protocol::Strand::Reverse,
        );
        sequence.extend(part.sequence);
    }
    Ok(sequence)
}

pub fn extract_record(
    doc: &SequenceDoc,
    start: u64,
    end: u64,
    segments: &[(u64, u64)],
    reverse: bool,
) -> genbank::Record {
    if segments.len() > 1 {
        return extract_spliced(doc, segments, reverse);
    }
    let mut rec = extract_range(doc, start, end);
    if reverse {
        reverse_complement_record(&mut rec);
        rec.name.push_str("_rc");
    }
    rec
}

/// Reverse-complement a record's sequence and mirror its annotations onto
/// the other strand. Segment order (5' to 3') carries over unchanged.
fn reverse_complement_record(rec: &mut genbank::Record) {
    use basecraft_docs::sequence::complement;
    let n = rec.sequence.len() as u64;
    rec.sequence.reverse();
    for b in &mut rec.sequence {
        *b = complement(*b);
    }
    for f in &mut rec.features {
        if f.kind == "source" {
            f.location = Location::simple(0, n, basecraft_protocol::Strand::Forward);
            continue;
        }
        for s in &mut f.location.segments {
            let (a, b) = (n - s.end, n - s.start);
            s.start = a;
            s.end = b;
            s.strand = match s.strand {
                basecraft_protocol::Strand::Forward => basecraft_protocol::Strand::Reverse,
                basecraft_protocol::Strand::Reverse => basecraft_protocol::Strand::Forward,
                basecraft_protocol::Strand::Unstranded => basecraft_protocol::Strand::Unstranded,
            };
            std::mem::swap(&mut s.partial_start, &mut s.partial_end);
        }
    }
    rec.header = vec![format!(
        "{} Reverse complement.",
        rec.header.join(" ").trim_end()
    )];
}

fn extract_spliced(doc: &SequenceDoc, segments: &[(u64, u64)], reverse: bool) -> genbank::Record {
    let mut sequence = Vec::new();
    for &(a, b) in segments {
        let mut part = doc.read(a, b);
        if reverse {
            part.reverse();
            for x in &mut part {
                *x = basecraft_docs::sequence::complement(*x);
            }
        }
        sequence.extend(part);
    }
    let n = sequence.len() as u64;
    let lo = segments.iter().map(|s| s.0).min().unwrap_or(0);
    let hi = segments.iter().map(|s| s.1).max().unwrap_or(0);
    // The annotations made of exactly these parts (a CDS, and perhaps its
    // mRNA): one continuous annotation over the spliced sequence.
    let mut features = vec![Feature {
        kind: "source".into(),
        location: Location::simple(0, n, basecraft_protocol::Strand::Forward),
        qualifiers: vec![Qualifier::new(
            "note",
            format!(
                "{} spliced parts of {} between bases {}..{}",
                segments.len(),
                doc.name,
                lo + 1,
                hi
            ),
        )],
    }];
    let mut label = None;
    for id in doc.features_in(lo, hi) {
        let Some(f) = doc.feature(id) else { continue };
        let same: Vec<(u64, u64)> = f
            .location
            .segments
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        if same != segments {
            continue;
        }
        label = label.or_else(|| f.label().map(str::to_string));
        let mut seg = Segment::new(0, n, basecraft_protocol::Strand::Forward);
        seg.partial_start = f.location.partial_5();
        seg.partial_end = f.location.partial_3();
        features.push(Feature {
            kind: f.kind.clone(),
            location: Location {
                segments: vec![seg],
                unordered: false,
            },
            qualifiers: f.qualifiers.clone(),
        });
    }
    let name = match &label {
        Some(l) => format!("{}_{l}", doc.name),
        None => format!("{}_{}-{}_spliced", doc.name, lo + 1, hi),
    };
    let definition = format!(
        "{}{} parts of {} spliced together",
        label.map(|l| format!("{l}: ")).unwrap_or_default(),
        segments.len(),
        doc.name
    );
    extracted(doc, name, &definition, features, sequence)
}

/// An extracted record, with the source's molecule, division and date.
fn extracted(
    doc: &SequenceDoc,
    name: String,
    definition: &str,
    features: Vec<Feature>,
    sequence: Vec<u8>,
) -> genbank::Record {
    let meta = doc.genbank.clone().unwrap_or_default();
    let or = |value: String, default: &str| {
        if value.is_empty() {
            default.to_owned()
        } else {
            value
        }
    };
    genbank::Record {
        name,
        molecule: or(meta.molecule, "DNA"),
        alphabet: None,
        circular: false,
        division: or(meta.division, "UNK"),
        date: or(meta.date, "01-JAN-1970"),
        header: vec![format!(
            "DEFINITION  {definition}, extracted with Basecraft."
        )],
        features,
        sequence,
    }
}

/// Bases `start..end` with their annotations, clipped and shifted.
fn extract_range(doc: &SequenceDoc, start: u64, end: u64) -> genbank::Record {
    let name = format!("{}_{}-{}", doc.name, start + 1, end);
    let mut features = Vec::new();
    for id in doc.features_in(start, end) {
        let Some(f) = doc.feature(id) else { continue };
        let mut segments = Vec::new();
        for s in &f.location.segments {
            let (a, b) = (s.start.max(start), s.end.min(end));
            let inside = if s.is_empty() {
                s.start >= start && s.start <= end
            } else {
                b > a
            };
            if !inside {
                continue;
            }
            segments.push(Segment {
                start: a - start,
                end: b.max(a) - start,
                strand: s.strand,
                partial_start: s.partial_start || s.start < start,
                partial_end: s.partial_end || s.end > end,
            });
        }
        if segments.is_empty() {
            continue;
        }
        // Parts dropped entirely count too: an annotation reaching past the
        // range is incomplete at that end.
        if let Some((lo, hi)) = f.location.bounds() {
            if lo < start
                && let Some(s) = segments.iter_mut().min_by_key(|s| s.start)
            {
                s.partial_start = true;
            }
            if hi > end
                && let Some(s) = segments.iter_mut().max_by_key(|s| s.end)
            {
                s.partial_end = true;
            }
        }
        features.push(Feature {
            kind: f.kind.clone(),
            location: Location {
                segments,
                unordered: f.location.unordered,
            },
            qualifiers: f.qualifiers.clone(),
        });
    }
    features.insert(
        0,
        Feature {
            kind: "source".into(),
            location: Location::simple(0, end - start, basecraft_protocol::Strand::Forward),
            qualifiers: vec![Qualifier::new(
                "note",
                format!("bases {}..{} of {}", start + 1, end, doc.name),
            )],
        },
    );
    // A source feature from the original no longer describes the extract.
    features.retain(|f| {
        !(f.kind == "source" && f.qualifier("note").is_none_or(|n| !n.starts_with("bases ")))
    });
    let definition = format!("Bases {}..{} of {}", start + 1, end, doc.name);
    extracted(doc, name, &definition, features, doc.read(start, end))
}

/// Read a loose species workspace with its companion.
pub fn load_delimitation(
    path: &Path,
) -> Result<basecraft_protocol::delimitation::DelimitationWorkspace> {
    load_delimitation_in(None, path)
}

/// Read a species workspace of `project` with its companion.
pub fn load_delimitation_in(
    project: Option<&crate::project::Project>,
    path: &Path,
) -> Result<basecraft_protocol::delimitation::DelimitationWorkspace> {
    let (source, hash) = Source::open_hashed(path)?;
    let companion = companion_path(project, path);
    let side = read_optional(&companion)?;
    load_delimitation_bytes(
        project,
        path,
        source.bytes(),
        &hash,
        (&companion, side.as_ref()),
    )
    .map(|(work, _)| work)
}

fn load_delimitation_bytes(
    project: Option<&crate::project::Project>,
    path: &Path,
    bytes: &[u8],
    exchange_hash: &str,
    (companion, captured_side): (&Path, Option<&Source>),
) -> Result<(
    basecraft_protocol::delimitation::DelimitationWorkspace,
    Option<String>,
)> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut work = if text.trim_start().starts_with('<') {
        basecraft_io::spart::parse_xml(text)?
    } else {
        basecraft_io::spart::parse_text(text)?
    };
    // A broken hidden binding never falls back to the companion beside the file.
    let hidden = crate::records::analysis_store::ArtifactRun::for_result(project, path)?;
    if hidden.is_some() && captured_side.is_none() {
        return Err(crate::error::invalid(
            "Hidden species workspace metadata is missing; restore its folder context or explicitly open a standalone SPART copy",
        ));
    }
    let mut warning = None;
    if let Some(source) = captured_side {
        let side = read_companion(companion, source.bytes(), exchange_hash, None).and_then(|c| {
            c.delimitation.ok_or_else(|| Error::Damaged {
                path: companion.to_path_buf(),
                detail: "the companion holds no species workspace".into(),
            })
        });
        match side {
            Ok(side) => {
                work = side.workspace;
                if let Some(old) = side.path_base {
                    let new = path.parent().unwrap_or(Path::new(".")).canonicalize()?;
                    rebase_delimitation_paths(&mut work, &old, &new);
                }
            }
            // A run's own record must hold; a document's record that no
            // longer fits it is ignored, as for trees and sequences.
            Err(error) if hidden.is_some() => return Err(error),
            Err(error) => {
                warning = Some(format!("The companion file was ignored: {error}"));
            }
        }
    }
    basecraft_docs::delimitation::validate(&work).map_err(|e| crate::error::invalid(e.0))?;
    Ok((work, warning))
}

pub fn write_delimitation(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    xml: bool,
    w: &mut dyn Write,
) -> io::Result<()> {
    let text = if xml {
        basecraft_io::spart::write_xml(work)
    } else {
        basecraft_io::spart::write_text(work).0
    };
    w.write_all(text.as_bytes())
}

/// A species workspace's companion. Its paths are recorded under the folder
/// `target` is saved in, so they follow the SPART file when it moves.
pub fn delimitation_companion(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    exchange_hash: &str,
    target: &Path,
) -> io::Result<Companion> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let base = parent
        .canonicalize()
        .or_else(|_| std::path::absolute(parent))?;
    Ok(Companion {
        delimitation: Some(DelimitationCompanion {
            path_base: Some(base),
            workspace: work.clone(),
        }),
        ..Companion::new(exchange_hash)
    })
}

/// Rebase only declared workspace path fields; authored text and scientific
/// parameters remain untouched. Historical reports retain their original bytes.
pub(crate) fn rebase_delimitation_paths(
    work: &mut basecraft_protocol::delimitation::DelimitationWorkspace,
    old: &Path,
    new: &Path,
) {
    map_delimitation_paths(work, |s| {
        Path::new(s).strip_prefix(old).map_or_else(
            |_| s.to_owned(),
            |p| new.join(p).to_string_lossy().into_owned(),
        )
    });
}

pub(crate) fn map_delimitation_paths(
    work: &mut basecraft_protocol::delimitation::DelimitationWorkspace,
    rebase: impl Fn(&str) -> String,
) {
    for specimen in &mut work.specimens {
        specimen.sources = std::mem::take(&mut specimen.sources)
            .into_iter()
            .map(|(key, value)| (rebase(&key), value))
            .collect();
    }
    if let Some(tree) = &mut work.view.display_tree {
        *tree = rebase(tree);
    }
    for name in [
        "basecraft_alignment_snapshot",
        "basecraft_alignment_evidence_run",
        "basecraft_display_evidence_run",
    ] {
        if let Some(path) = work.exchange_fields.get_mut(name) {
            *path = rebase(path);
        }
    }
    for hypothesis in &mut work.hypotheses {
        hypothesis.provenance.inputs = std::mem::take(&mut hypothesis.provenance.inputs)
            .into_iter()
            .map(|(key, value)| (rebase(&key), value))
            .collect();
        for name in ["alignment_snapshot", "mapping", "original_source", "cache"] {
            if let Some(value) = hypothesis.provenance.parameters.get_mut(name)
                && let Some(s) = value.as_str()
            {
                *value = serde_json::json!(rebase(s));
            }
        }
        for name in [
            "analysis_report",
            "combined_source",
            "alignment_evidence",
            "input_evidence",
        ] {
            if let Some(value) = hypothesis
                .provenance
                .parameters
                .get_mut(name)
                .and_then(|v| v.get_mut("path"))
                && let Some(s) = value.as_str()
            {
                *value = serde_json::json!(rebase(s));
            }
        }
        for name in ["combined_source", "alignment_evidence", "input_evidence"] {
            if let Some(value) = hypothesis.provenance.parameters.get_mut(name) {
                for key in ["run_directory", "evidence_run"] {
                    if let Some(field) = value.get_mut(key)
                        && let Some(s) = field.as_str()
                    {
                        *field = serde_json::json!(rebase(s));
                    }
                }
            }
        }
    }
}

pub fn delimitation_xml(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        e.eq_ignore_ascii_case("xml")
            || e.eq_ignore_ascii_case("spartxml")
            || e.eq_ignore_ascii_case("spart-xml")
    })
}

#[cfg(test)]
mod companion_tests {
    use super::*;

    /// A recovered loose record replaces one an earlier file at the same
    /// project path left behind, and the emptied loose folders go.
    #[test]
    fn settling_replaces_an_orphan_record_and_removes_empty_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let project = crate::project::Project::new(&root).unwrap();
        let document = root.join("sub/a.nwk");
        std::fs::write(&document, "(a,b);").unwrap();
        let kept = companion_path(Some(&project), &document);
        std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
        std::fs::write(&kept, "stale").unwrap();
        let loose = crate::project::Project::document_record(None, &document);
        std::fs::create_dir_all(loose.parent().unwrap()).unwrap();
        std::fs::write(&loose, "recovered").unwrap();
        settle_companion(Some(&project), &document).unwrap();
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "recovered");
        assert!(!root.join("sub/.basecraft").exists());

        // A loose alignment link can't be settled: it is reported and kept.
        let link = crate::records::links::sidecar_in(None, &document);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::fs::write(&link, "{}").unwrap();
        std::fs::write(&loose, "again").unwrap();
        let error = settle_companion(Some(&project), &document).unwrap_err();
        assert!(error.to_string().contains("alignment link"), "{error}");
        assert!(link.is_file() && loose.is_file());
    }
}
