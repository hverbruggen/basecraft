//! Opening and saving documents by file format.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::collections::BTreeMap;

use basecraft_docs::{AlignmentDoc, SequenceDoc, TreeDoc};
use basecraft_io::{Source, fasta, genbank, gff3, newick};
use basecraft_protocol::{CladeAnnotation, Feature, Location, Qualifier, Segment, TreeStyle};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
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
    /// in a JSON sidecar: see [`tree_sidecar`].
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
    pub format: Format,
}

/// An open tree plus what's needed to write its file back.
pub struct OpenedTree {
    pub doc: TreeDoc,
    pub nexus: bool,
    /// Unverifiable legacy metadata remains on disk and was not applied.
    pub metadata_warning: Option<String>,
    /// Other forest members are preserved when the first tree is edited.
    pub remaining: Vec<newick::ParsedTree>,
}

pub enum Loaded {
    Sequence(Opened),
    Delimitation(basecraft_protocol::delimitation::DelimitationWorkspace),
    Metadata(crate::metadata::OpenedMetadata),
    Submission(basecraft_protocol::submission::Submission),
    Alignment(OpenedAlignment),
    Tree(OpenedTree),
}

impl Loaded {
    pub(crate) fn sequence(&self) -> Option<&Opened> {
        if let Self::Sequence(payload) = self {
            Some(payload)
        } else {
            None
        }
    }
    pub(crate) fn alignment(&self) -> Option<&OpenedAlignment> {
        if let Self::Alignment(payload) = self {
            Some(payload)
        } else {
            None
        }
    }
    pub(crate) fn tree(&self) -> Option<&OpenedTree> {
        if let Self::Tree(payload) = self {
            Some(payload)
        } else {
            None
        }
    }
    pub(crate) fn metadata(&self) -> Option<&crate::metadata::OpenedMetadata> {
        if let Self::Metadata(payload) = self {
            Some(payload)
        } else {
            None
        }
    }
    pub(crate) fn submission(&self) -> Option<&basecraft_protocol::submission::Submission> {
        if let Self::Submission(payload) = self {
            Some(payload)
        } else {
            None
        }
    }
    pub(crate) fn delimitation(
        &self,
    ) -> Option<&basecraft_protocol::delimitation::DelimitationWorkspace> {
        if let Self::Delimitation(payload) = self {
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
            Self::Submission(sub) => sub,
            Self::Metadata(opened) => opened,
            Self::Alignment(opened) => opened,
            Self::Tree(opened) => opened,
            Self::Sequence(opened) => opened,
            Self::Delimitation(work) => work,
        }
    }
}

/// Open a file as whatever it holds: see [`kind`].
pub fn load(path: &Path) -> Result<Loaded> {
    let format = Format::from_path(path)?;
    if format == Format::Metadata {
        return crate::metadata::load(path).map(Loaded::Metadata);
    }
    if format == Format::Submission {
        return crate::submission::load(path).map(Loaded::Submission);
    }
    if format == Format::Delimitation {
        return load_delimitation(path).map(Loaded::Delimitation);
    }
    load_from(path, open_source(path, format)?)
}

/// Load FASTA, GenBank or a tree from the supplied main-file snapshot.
/// Companion files are still read from disk; callers needing their exact hashes
/// must retain the companion validation used by `Session`.
/// Other formats require their existing loaders and are rejected here.
pub fn load_from(path: &Path, source: Source) -> Result<Loaded> {
    match Format::from_path(path)? {
        Format::Tree => load_tree_from(path, &source, false).map(Loaded::Tree),
        Format::Fasta => {
            let gff = read_optional(&gff_sidecar(path))?;
            load_fasta_from(path, source, gff.as_ref())
        }
        Format::GenBank => open_from(path, source).map(Loaded::Sequence),
        _ => Err(crate::error::invalid(
            "this format does not support loading an owned source",
        )),
    }
}

/// Exact inputs retained until admission and recovery preparation finish.
pub(crate) struct CapturedLoad {
    pub content: Loaded,
    pub files: Vec<(PathBuf, Source, String)>,
    pub stamps: Vec<(PathBuf, Option<std::fs::Metadata>)>,
}

pub(crate) fn capture_load(
    path: &Path,
    format: Format,
    verified: bool,
) -> Result<CapturedLoad> {
    let companions = match format {
        Format::Metadata => crate::metadata::files(path).split_off(1),
        Format::Tree | Format::Delimitation => vec![tree_sidecar(path)],
        Format::GenBank | Format::Fasta => vec![gff_sidecar(path)],
        Format::Submission => crate::documents::submission_companions(path),
    };
    let mut stamps = Vec::new();
    let mut files = Vec::new();
    for file in std::iter::once(path.to_path_buf()).chain(companions) {
        let metadata = match std::fs::metadata(&file) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound && file != path => None,
            Err(error) => return Err(error.into()),
        };
        if metadata.is_some() {
            let (source, hash) = Source::open_hashed(&file)?;
            files.push((file.clone(), source, hash));
        }
        stamps.push((file, metadata));
    }
    let main = files[0].1.clone();
    let companion = files.get(1).map(|(_, source, _)| source);
    let content = match format {
        Format::Fasta => load_fasta_from(path, main, companion)?,
        Format::GenBank => Loaded::Sequence(open_from(path, main)?),
        Format::Tree => Loaded::Tree(load_tree_bytes(
            path,
            &main,
            &files[0].2,
            verified,
            companion,
        )?),
        Format::Delimitation => Loaded::Delimitation(load_delimitation_bytes(
            path,
            main.bytes(),
            &files[0].2,
            companion,
        )?),
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
    for (file, before) in &stamps {
        let after = match std::fs::metadata(file) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if !same_metadata(before.as_ref(), after.as_ref()) {
            return Err(Error::ChangedOnDisk { path: file.clone() });
        }
    }
    Ok(CapturedLoad {
        content,
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

pub(crate) fn same_metadata(a: Option<&std::fs::Metadata>, b: Option<&std::fs::Metadata>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let same = a.is_file()
                && b.is_file()
                && a.len() == b.len()
                && a.modified().ok() == b.modified().ok();
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                same && a.dev() == b.dev()
                    && a.ino() == b.ino()
                    && a.ctime() == b.ctime()
                    && a.ctime_nsec() == b.ctime_nsec()
            }
            #[cfg(not(unix))]
            {
                same
            }
        }
        _ => false,
    }
}

fn load_fasta_from(
    path: &Path,
    source: Source,
    captured_gff: Option<&Source>,
) -> Result<Loaded> {
    let file = Arc::new(fasta::IndexedFasta::from_source(source)?);
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
        Ok(Loaded::Alignment(OpenedAlignment {
            doc: AlignmentDoc::from_fasta(name, &file),
            format: Format::Fasta,
        }))
    } else {
        open_fasta_with_gff(path, file, captured_gff.map(Source::bytes)).map(Loaded::Sequence)
    }
}

/// Write explicit alignment contents as FASTA, 60 columns a line.
/// Synthetic suffix padding used to display ragged input is never materialised.
pub fn write_alignment(doc: &AlignmentDoc, w: &mut dyn Write) -> io::Result<()> {
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

/// A tree file's sidecar: `NAME.nwk.basecraft.json` beside `NAME.nwk`.
pub fn tree_sidecar(path: &Path) -> PathBuf {
    if let Ok(Some(run)) = crate::analysis_store::ArtifactRun::for_result(path) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        return run.join(format!("{name}.basecraft.json"));
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".basecraft.json");
    path.with_file_name(name)
}

/// What a tree's Newick can't hold.
#[derive(Default, Serialize, Deserialize)]
struct TreeSidecar {
    #[serde(default)]
    schema_version: u32,
    /// Canonical topology, raw support and node ordering of the first tree.
    #[serde(default)]
    tree_hash: String,
    /// Exact exchange bytes, including every forest component, when file-backed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exchange_hash: Option<String>,
    /// Support set names and sources, in the order their values are joined
    /// in the Newick labels.
    #[serde(default)]
    supports: Vec<SupportName>,
    #[serde(default)]
    annotations: BTreeMap<u32, CladeAnnotation>,
    #[serde(default)]
    style: TreeStyle,
}

#[derive(Serialize, Deserialize)]
struct SupportName {
    name: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    kind: basecraft_protocol::SupportKind,
}

/// Read the first tree of a Newick or NEXUS file, with its sidecar.
pub fn load_tree(path: &Path) -> Result<OpenedTree> {
    load_tree_metadata(path, false)
}

fn load_tree_metadata(path: &Path, verified_snapshot: bool) -> Result<OpenedTree> {
    load_tree_from(path, &open_source(path, Format::Tree)?, verified_snapshot)
}

pub(crate) fn load_tree_from(
    path: &Path,
    source: &Source,
    verified_snapshot: bool,
) -> Result<OpenedTree> {
    let side = read_optional(&tree_sidecar(path))?;
    load_tree_bytes(
        path,
        source,
        &basecraft_io::hash_bytes(source.bytes()),
        verified_snapshot,
        side.as_ref(),
    )
}

fn load_tree_bytes(
    path: &Path,
    source: &Source,
    exchange_hash: &str,
    verified_snapshot: bool,
    captured_side: Option<&Source>,
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
    let mut metadata_warning = None;
    // Errors in a hidden association must never select an adjacent legacy file.
    let run = crate::analysis_store::ArtifactRun::for_result_with_hash(path, exchange_hash)?;
    let bound = run.is_some();
    let sidecar = run.map_or_else(
        || tree_sidecar(path),
        |run| {
            run.join(format!(
                "{}.basecraft.json",
                path.file_name().unwrap_or_default().to_string_lossy()
            ))
        },
    );
    if let Some(source) = captured_side {
        let side: TreeSidecar = serde_json::from_slice(source.bytes())?;
        if side.schema_version == 0 && !verified_snapshot {
            metadata_warning = Some(format!(
                "Unverified legacy tree metadata at {} was left unapplied; the exchange tree is open and the original metadata bytes are preserved.",
                sidecar.display()
            ));
        } else if side.schema_version > 1 {
            return Err(crate::error::invalid(
                "Unsupported future tree metadata version; bytes were preserved",
            ));
        } else {
            if side.schema_version == 1
                && (side.tree_hash != tree_hash(&doc)
                    || side
                        .exchange_hash
                        .as_ref()
                        .is_some_and(|hash| hash != exchange_hash))
            {
                return Err(crate::error::invalid(
                    "Tree metadata describes different exchange contents; bytes were preserved and metadata was not applied",
                ));
            }
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
            for (name, kind) in kinds {
                doc.set_support_kind(&name, kind);
            }
        }
    }
    if bound && captured_side.is_none() {
        metadata_warning = Some("Bound tree metadata is unavailable; exchange values remain open with unknown support meaning and default presentation.".into());
    }
    // Legacy Basecraft distance runs carry explicit bootstrap metadata. Require
    // an exact matching result, so stale adjacent reports cannot relabel imports.
    let report_path = PathBuf::from(format!("{}.run.json", path.display()));
    if let Ok(report) = std::fs::read(&report_path)
        && let Ok(report) = serde_json::from_slice::<serde_json::Value>(&report)
        && report["tool"] == "basecraft-distance"
        && report["bootstrap"]["support_withheld"] == false
        && report["bootstrap"]["completed"]
            .as_u64()
            .is_some_and(|n| n > 0)
        && report["tree"]["trees"].as_array().is_some_and(|trees| {
            trees.iter().any(|tree| {
                tree.as_str()
                    .and_then(|text| TreeDoc::from_newick("", text).ok())
                    .is_some_and(|tree| tree_hash(&tree) == tree_hash(&doc))
            })
        })
        && doc.supports().len() == 1
        && doc.supports()[0].kind == basecraft_protocol::SupportKind::Unknown
    {
        let name = doc.supports()[0].name.clone();
        if name == "Support" {
            doc.rename_support(&name, "Bootstrap");
        }
        let name = doc.supports()[0].name.clone();
        doc.set_support_kind(&name, basecraft_protocol::SupportKind::Bootstrap);
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


pub fn write_tree_sidecar(doc: &TreeDoc, w: &mut dyn Write) -> io::Result<()> {
    write_tree_metadata(doc, None, w)
}

/// File-backed metadata binds the entire exchange, including forests and NEXUS.
pub fn write_tree_sidecar_at(
    doc: &TreeDoc,
    exchange_hash: &str,
    w: &mut dyn Write,
) -> io::Result<()> {
    write_tree_metadata(doc, Some(exchange_hash.to_owned()), w)
}

fn write_tree_metadata(
    doc: &TreeDoc,
    exchange_hash: Option<String>,
    w: &mut dyn Write,
) -> io::Result<()> {
    let side = tree_metadata(doc, exchange_hash);
    serde_json::to_writer_pretty(&mut *w, &side)?;
    writeln!(w)
}


fn tree_hash(doc: &TreeDoc) -> String {
    let mut writer = HashingWriter::new(io::sink());
    doc.write_newick(&mut writer)
        .expect("hashing writer cannot fail");
    writer.hash()
}

fn tree_metadata(doc: &TreeDoc, exchange_hash: Option<String>) -> TreeSidecar {
    TreeSidecar {
        schema_version: 1,
        tree_hash: tree_hash(doc),
        exchange_hash,
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
    }
}

/// An open document plus what's needed to write its file back.
pub struct Opened {
    pub doc: SequenceDoc,
    pub format: Format,
    /// For FASTA: the original file, so records other than the open one are
    /// copied through on save.
    pub fasta: Option<Arc<fasta::IndexedFasta>>,
}

// Preserve the public loaders' historical filesystem error variants: FASTA
// used the IO crate's wrapper; GenBank and trees used std filesystem reads.
fn open_source(path: &Path, format: Format) -> Result<Source> {
    Source::open(path).map_err(|error| match (format, error) {
        (Format::GenBank | Format::Tree, basecraft_io::Error::Io(error)) => Error::Io(error),
        (_, error) => Error::Format(error),
    })
}

/// Open the first record of a sequence file.
pub fn open(path: &Path) -> Result<Opened> {
    let format = Format::from_path(path)?;
    if matches!(format, Format::Fasta | Format::GenBank) {
        return open_from(path, open_source(path, format)?);
    }
    open_from(path, Source::from_vec(Vec::new()))
}

/// Open a sequence from already-read main-file bytes using the shared parsers.
pub fn open_from(path: &Path, source: Source) -> Result<Opened> {
    let format = Format::from_path(path)?;
    match format {
        Format::Delimitation => Err(crate::error::invalid("this is a delimitation workspace")),
        Format::Metadata => Err(crate::error::invalid(
            "this is the project's specimen metadata",
        )),
        Format::Submission => Err(crate::error::invalid("this is a GenBank submission")),
        Format::Tree => Err(crate::error::invalid(format!(
            "{} is a tree, not a sequence",
            path.display()
        ))),
        Format::GenBank => {
            let rec = genbank::parse(source.bytes())?
                .into_iter()
                .next()
                .ok_or_else(|| Error::Empty(path.to_path_buf()))?;
            Ok(Opened {
                doc: SequenceDoc::from_genbank(rec),
                format,
                fasta: None,
            })
        }
        Format::Fasta => open_fasta_from(path, Arc::new(fasta::IndexedFasta::from_source(source)?)),
    }
}

fn open_fasta_from(path: &Path, file: Arc<fasta::IndexedFasta>) -> Result<Opened> {
    if file.index.records.is_empty() {
        return Err(Error::Empty(path.to_path_buf()));
    }
    let sidecar = gff_sidecar(path);
    let gff = if sidecar.exists() {
        let bytes = std::fs::read(&sidecar)?;
        #[cfg(feature = "test-counters")]
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
        doc,
        format: Format::Fasta,
        fasta: Some(file),
    })
}

/// Serialise the main file of a document.
pub fn write_main(opened: &Opened, w: &mut dyn Write) -> io::Result<()> {
    let doc = &opened.doc;
    match opened.format {
        Format::GenBank => genbank::write(w, &doc.to_genbank()),
        Format::Delimitation => Err(io::Error::other("this is a delimitation workspace")),
        Format::Metadata => Err(io::Error::other("a sequence can't be written as metadata")),
        Format::Submission => Err(io::Error::other(
            "a sequence can't be written as a submission",
        )),
        Format::Tree => Err(io::Error::other("a sequence can't be written as a tree")),
        Format::Fasta => {
            let Some(file) = &opened.fasta else {
                return write_fasta_record(doc, None, w);
            };
            for (i, rec) in file.index.records.iter().enumerate() {
                if i == 0 {
                    write_fasta_record(doc, Some(rec.header(file.bytes())), w)?;
                } else {
                    let seq = rec
                        .read(file.bytes(), 0, rec.len)
                        .map_err(io::Error::other)?;
                    fasta::write_record(w, rec.header(file.bytes()), &seq, 60)?;
                }
            }
            Ok(())
        }
    }
}

fn write_fasta_record(
    doc: &SequenceDoc,
    header: Option<&[u8]>,
    w: &mut dyn Write,
) -> io::Result<()> {
    let header = header.map_or_else(|| doc.name.as_bytes().to_vec(), <[u8]>::to_vec);
    // Stream in chunks so a huge record never sits in memory twice.
    w.write_all(b">")?;
    w.write_all(&header)?;
    w.write_all(b"\n")?;
    let mut buf = Vec::new();
    let chunk = 60 * 16_384;
    let mut pos = 0;
    while pos < doc.len() {
        let end = (pos + chunk).min(doc.len());
        buf.clear();
        doc.read_into(pos, end, &mut buf);
        for line in buf.chunks(60) {
            w.write_all(line)?;
            w.write_all(b"\n")?;
        }
        pos = end;
    }
    Ok(())
}

/// Serialise the GFF3 sidecar, if the format has one.
pub fn write_sidecar(opened: &Opened, w: &mut dyn Write) -> Option<io::Result<()>> {
    (opened.format == Format::Fasta).then(|| {
        let feats = opened.doc.to_gff();
        gff3::write(w, &[(&feats, opened.doc.len())])
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
    let mut rec = if segments.len() > 1 {
        extract_spliced(doc, segments, reverse)
    } else {
        let mut r = extract_range(doc, start, end);
        if reverse {
            reverse_complement_record(&mut r);
        }
        r
    };
    rec.name = if reverse && segments.len() <= 1 {
        format!("{}_rc", rec.name)
    } else {
        rec.name
    };
    rec
}

fn complement(b: u8) -> u8 {
    match b {
        b'A' => b'T',
        b'T' | b'U' => b'A',
        b'C' => b'G',
        b'G' => b'C',
        b'a' => b't',
        b't' | b'u' => b'a',
        b'c' => b'g',
        b'g' => b'c',
        b'R' => b'Y',
        b'Y' => b'R',
        b'K' => b'M',
        b'M' => b'K',
        b'B' => b'V',
        b'V' => b'B',
        b'D' => b'H',
        b'H' => b'D',
        other => other,
    }
}

/// Reverse-complement a record's sequence and mirror its annotations onto
/// the other strand. Segment order (5' to 3') carries over unchanged.
fn reverse_complement_record(rec: &mut genbank::Record) {
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
                *x = complement(*x);
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
    let meta = doc.genbank.clone().unwrap_or_default();
    let name = match &label {
        Some(l) => format!("{}_{l}", doc.name),
        None => format!("{}_{}-{}_spliced", doc.name, lo + 1, hi),
    };
    genbank::Record {
        name,
        molecule: if meta.molecule.is_empty() {
            "DNA".into()
        } else {
            meta.molecule
        },
        circular: false,
        division: if meta.division.is_empty() {
            "UNK".into()
        } else {
            meta.division
        },
        date: if meta.date.is_empty() {
            "01-JAN-1970".into()
        } else {
            meta.date
        },
        header: vec![format!(
            "DEFINITION  {}{} parts of {} spliced together, extracted with Basecraft.",
            label.map(|l| format!("{l}: ")).unwrap_or_default(),
            segments.len(),
            doc.name
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
    let meta = doc.genbank.clone().unwrap_or_default();
    genbank::Record {
        name,
        molecule: if meta.molecule.is_empty() {
            "DNA".into()
        } else {
            meta.molecule
        },
        circular: false,
        division: if meta.division.is_empty() {
            "UNK".into()
        } else {
            meta.division
        },
        date: if meta.date.is_empty() {
            "01-JAN-1970".into()
        } else {
            meta.date
        },
        header: vec![format!(
            "DEFINITION  Bases {}..{} of {}, extracted with Basecraft.",
            start + 1,
            end,
            doc.name
        )],
        features,
        sequence: doc.read(start, end),
    }
}

/// A portable SPART file plus its versioned Basecraft workspace sidecar.
#[derive(Serialize, Deserialize)]
struct DelimitationSidecar {
    version: u32,
    exchange_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path_base: Option<PathBuf>,
    workspace: basecraft_protocol::delimitation::DelimitationWorkspace,
}

pub fn load_delimitation(
    path: &Path,
) -> Result<basecraft_protocol::delimitation::DelimitationWorkspace> {
    let (source, hash) = Source::open_hashed(path)?;
    let side = read_optional(&tree_sidecar(path))?;
    load_delimitation_bytes(path, source.bytes(), &hash, side.as_ref())
}

fn load_delimitation_bytes(
    path: &Path,
    bytes: &[u8],
    exchange_hash: &str,
    captured_side: Option<&Source>,
) -> Result<basecraft_protocol::delimitation::DelimitationWorkspace> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut work = if text.trim_start().starts_with('<') {
        basecraft_io::spart::parse_xml(text)?
    } else {
        basecraft_io::spart::parse_text(text)?
    };
    // A damaged hidden binding must never silently select stale legacy data.
    let hidden = crate::analysis_store::ArtifactRun::for_result(path)?;
    if hidden.is_some() && captured_side.is_none() {
        return Err(crate::error::invalid(
            "Hidden species workspace metadata is missing; restore its folder context or explicitly open a standalone SPART copy",
        ));
    }
    if let Some(source) = captured_side {
        let side: DelimitationSidecar = serde_json::from_slice(source.bytes())?;
        if !matches!(side.version, 1 | 2) || side.exchange_hash != exchange_hash {
            return Err(crate::error::invalid(
                "SPART sidecar version or exchange hash differs",
            ));
        }
        work = side.workspace;
        if side.version == 2 {
            let old = side
                .path_base
                .ok_or_else(|| crate::error::invalid("Species metadata has no path base"))?;
            let new = path.parent().unwrap_or(Path::new(".")).canonicalize()?;
            rebase_delimitation_paths(&mut work, &old, &new);
        }
    }
    basecraft_docs::delimitation::validate(&work).map_err(|e| crate::error::invalid(e.0))?;
    Ok(work)
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

pub fn write_delimitation_sidecar(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    hash: &str,
    w: &mut dyn Write,
) -> io::Result<()> {
    let side = DelimitationSidecar {
        version: 1,
        exchange_hash: hash.into(),
        path_base: None,
        workspace: work.clone(),
    };
    serde_json::to_writer_pretty(&mut *w, &side)?;
    writeln!(w)
}

pub fn write_delimitation_sidecar_at(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    hash: &str,
    target: &Path,
    w: &mut dyn Write,
) -> io::Result<()> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let base = parent
        .canonicalize()
        .or_else(|_| std::path::absolute(parent))?;
    write_delimitation_sidecar_in(work, hash, &base, w)
}

pub(crate) fn write_delimitation_sidecar_in(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    hash: &str,
    base: &Path,
    w: &mut dyn Write,
) -> io::Result<()> {
    let side = DelimitationSidecar {
        version: 2,
        exchange_hash: hash.into(),
        path_base: Some(base.to_path_buf()),
        workspace: work.clone(),
    };
    serde_json::to_writer_pretty(&mut *w, &side)?;
    writeln!(w)
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
