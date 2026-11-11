//! The open documents of one window, as tabs, and the decisions that span
//! them: what opening a path does, what closing or quitting must ask, the
//! order of saves before quitting, and a species analysis started from one
//! document that creates another. Qt-free; the desktop app shows it.

/// What a tab holds. `Canvas` is a sequence, alignment or tree still being
/// opened on a worker; its tab already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Canvas,
    Sequence,
    Alignment,
    Tree,
    Trace,
    Delimitation,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Sequence => "sequence",
            Self::Alignment => "alignment",
            Self::Tree => "tree",
            Self::Trace => "trace",
            Self::Delimitation => "delimitation",
        }
    }
    /// The kind a file name gives; anything unknown is a sequence.
    pub fn parse(name: &str) -> Self {
        match name {
            "canvas" => Self::Canvas,
            "alignment" => Self::Alignment,
            "tree" => Self::Tree,
            "trace" => Self::Trace,
            "delimitation" => Self::Delimitation,
            _ => Self::Sequence,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub token: u64,
    pub path: String,
    pub kind: Kind,
    pub dirty: bool,
    /// A trace review busy preparing or calling.
    pub working: bool,
    /// A species workspace created for an analysis; removed on close if the
    /// analysis left it empty.
    pub draft: bool,
}

/// A tab the user agreed to save or discard: the tab and the path it showed.
/// The agreement lapses when the tab closes or is saved under another name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub token: u64,
    pub path: String,
}

/// Opening a path.
#[derive(Debug, PartialEq, Eq)]
pub enum Open {
    /// The specimen table or a submission: shown in its own dialog.
    Project,
    /// Already open in this tab.
    Switch(usize),
    /// Needs a new tab.
    New,
}

/// Where a new trace review of a read goes.
#[derive(Debug, PartialEq, Eq)]
pub enum TraceReview {
    /// That read's review is already working in this tab.
    Switch(usize),
    /// A tab holds unsaved changes to that read (or a new tab was asked
    /// for): start in a new tab and keep the old one.
    NewTab,
    /// Open the read the usual way, then start the review there.
    Open,
}

/// What saving a tab does.
#[derive(Debug, PartialEq, Eq)]
pub enum Save {
    /// Ask where to save first (Save As, or a never-saved document).
    Destination,
    /// Write to the tab's file.
    Write,
    /// A trace review saves through its own review dialog.
    TraceReview,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Close {
    /// Unsaved changes: ask to save, discard or cancel.
    Ask(Target),
    /// Close now; `draft` asks the view to delete an empty species workspace.
    Remove { draft: bool },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Quit {
    /// Methods drafts in the document record are unsaved.
    Drafts,
    /// These tabs have unsaved changes; `error` says why the question repeats.
    Ask {
        targets: Vec<Target>,
        error: &'static str,
    },
    /// Save this tab next.
    Save(Target),
    /// A save failed; its own notice explains. Stay open.
    Stay,
    /// Close the window, deleting these empty species workspaces first.
    Leave { drafts: Vec<u64> },
}

pub const CHANGED: &str = "The open documents changed. Review them before closing.";
pub const NOT_SAVED: &str = "Some files could not be saved. Review their notifications and retry.";

/// A species analysis asked for on an alignment or a tree. It runs in a new
/// workspace document once the source has created it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Analysis {
    pub method: String,
    pub input: String,
    pub rooted: bool,
    pub settings: String,
    pub display: String,
    /// The input is the open alignment itself; the new workspace then reads
    /// its own linked copy instead. Set from the source when requested.
    #[serde(default)]
    pub uses_open_alignment: bool,
}

#[derive(Debug, PartialEq)]
pub enum Delimit {
    /// Nothing to do: another request is running, the source closed, changed
    /// or is busy.
    Refuse,
    /// The source is a species workspace: run the analysis in it.
    InWorkspace(u64),
    /// Ask the source to create a workspace; `workspace_created` follows.
    Create,
}

struct Request {
    source: Target,
    analysis: Analysis,
}

#[derive(Default)]
pub struct Workspace {
    documents: Vec<Document>,
    /// Numbers new, never-saved sequences.
    untitled: u64,
    /// The document the species dialog was opened for.
    source: Option<Target>,
    request: Option<Request>,
    /// Tabs still to save before quitting, and the consent they came from.
    saving: Vec<Target>,
}

impl Workspace {
    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn index(&self, token: u64) -> Option<usize> {
        self.documents.iter().position(|d| d.token == token)
    }

    fn by_path(&self, path: &str) -> Option<usize> {
        self.documents.iter().position(|d| d.path == path)
    }

    /// The tab is still open and still shows the path it was agreed for.
    pub fn target_index(&self, target: &Target) -> Option<usize> {
        self.index(target.token)
            .filter(|&i| self.documents[i].path == target.path)
    }

    pub fn target(&self, index: usize) -> Option<Target> {
        self.documents.get(index).map(|d| Target {
            token: d.token,
            path: d.path.clone(),
        })
    }

    /// Append a tab; returns its index.
    pub fn add(&mut self, token: u64, path: String, kind: Kind) -> usize {
        self.documents.push(Document {
            token,
            path,
            kind,
            dirty: false,
            working: false,
            draft: false,
        });
        self.documents.len() - 1
    }

    /// Remove a tab and every request that depended on it.
    pub fn remove(&mut self, token: u64) -> Option<usize> {
        let index = self.index(token)?;
        self.documents.remove(index);
        if self.source.as_ref().is_some_and(|s| s.token == token) {
            self.source = None;
        }
        if self
            .request
            .as_ref()
            .is_some_and(|r| r.source.token == token)
        {
            self.request = None;
        }
        Some(index)
    }

    pub fn set_dirty(&mut self, index: usize, dirty: bool) -> bool {
        match self.documents.get_mut(index) {
            Some(d) if d.dirty != dirty => {
                d.dirty = dirty;
                true
            }
            _ => false,
        }
    }

    pub fn set_working(&mut self, index: usize, working: bool) {
        if let Some(d) = self.documents.get_mut(index) {
            d.working = working;
        }
    }

    /// Where a review of the read at `path` starts.
    pub fn trace_review(&self, path: &str, new_tab: bool) -> TraceReview {
        let mut keep = new_tab;
        for (index, d) in self
            .documents
            .iter()
            .enumerate()
            .filter(|(_, d)| d.path == path)
        {
            if d.kind == Kind::Trace && d.working {
                return TraceReview::Switch(index);
            }
            keep |= d.dirty;
        }
        if keep {
            TraceReview::NewTab
        } else {
            TraceReview::Open
        }
    }

    /// The tab now shows `path` (Save As, or a review saved to a file).
    pub fn set_path(&mut self, token: u64, path: String) -> Option<usize> {
        let index = self.index(token)?;
        self.documents[index].path = path;
        Some(index)
    }

    /// Opening `path`. `kind` is what the file holds: "metadata",
    /// "submission", "trace", "delimitation", "sequence", or "canvas" for a
    /// file opened on a worker. A path open in a tab, also one still being
    /// opened, switches to that tab.
    pub fn open(&self, path: &str, kind: &str) -> Open {
        if kind == "metadata" || kind == "submission" {
            return Open::Project;
        }
        self.by_path(path).map_or(Open::New, Open::Switch)
    }

    /// Saving the tab at `index`; `choose_name` for Save As.
    pub fn save(&self, index: usize, choose_name: bool) -> Option<Save> {
        let d = self.documents.get(index)?;
        Some(if d.kind == Kind::Trace && choose_name {
            Save::TraceReview
        } else if choose_name || d.path.starts_with("untitled:") {
            Save::Destination
        } else {
            Save::Write
        })
    }

    /// The path of a new, never-saved sequence named `name`.
    pub fn untitled_path(&mut self, name: &str) -> String {
        self.untitled += 1;
        format!("untitled:{}/{}.gb", self.untitled, name.trim())
    }

    pub fn close(&self, index: usize) -> Option<Close> {
        let d = self.documents.get(index)?;
        Some(if d.dirty {
            Close::Ask(self.target(index)?)
        } else {
            Close::Remove { draft: d.draft }
        })
    }

    fn unsaved(&self) -> Vec<Target> {
        (0..self.documents.len())
            .filter(|&i| self.documents[i].dirty)
            .filter_map(|i| self.target(i))
            .collect()
    }

    fn leave(&self) -> Quit {
        Quit::Leave {
            drafts: self
                .documents
                .iter()
                .filter(|d| d.draft)
                .map(|d| d.token)
                .collect(),
        }
    }

    /// The window is closing. `drafts` is true when the document record holds
    /// unsaved methods drafts the user has not agreed to drop.
    pub fn quit(&mut self, drafts: bool) -> Quit {
        self.saving.clear();
        if drafts {
            return Quit::Drafts;
        }
        let targets = self.unsaved();
        if targets.is_empty() {
            self.leave()
        } else {
            Quit::Ask { targets, error: "" }
        }
    }

    /// The user chose Discard for `consent`. Leave only if that is still
    /// exactly the set of unsaved tabs.
    pub fn discard_all(&self, consent: &[Target]) -> Quit {
        let current = self.unsaved();
        let same = consent.iter().all(|t| self.target_index(t).is_some())
            && current.iter().all(|t| consent.contains(t));
        if same {
            self.leave()
        } else {
            Quit::Ask {
                targets: current,
                error: CHANGED,
            }
        }
    }

    /// The user chose Save for `consent`: save each in turn.
    pub fn save_all(&mut self, consent: Vec<Target>) -> Quit {
        self.saving = consent;
        self.saving.reverse();
        self.next_save()
    }

    /// The last save finished.
    pub fn quit_saved(&mut self, success: bool) -> Quit {
        if success {
            self.next_save()
        } else {
            self.saving.clear();
            Quit::Stay
        }
    }

    fn next_save(&mut self) -> Quit {
        let Some(target) = self.saving.pop() else {
            let left = self.unsaved();
            return if left.is_empty() {
                self.leave()
            } else {
                Quit::Ask {
                    targets: left,
                    error: NOT_SAVED,
                }
            };
        };
        if self.target_index(&target).is_none() {
            self.saving.clear();
            return Quit::Ask {
                targets: self.unsaved(),
                error: CHANGED,
            };
        }
        Quit::Save(target)
    }

    /// The species dialog opened for the tab at `index`.
    pub fn begin_delimitation(&mut self, index: usize) {
        self.source = self.target(index);
    }

    /// The species dialog asked for an analysis. `source_path` is the file
    /// the source controller has open now; `busy` whether it is working.
    pub fn request_delimitation(
        &mut self,
        source_path: &str,
        busy: bool,
        analysis: Analysis,
    ) -> Delimit {
        let Some(source) = self.source.clone() else {
            return Delimit::Refuse;
        };
        let Some(index) = self.target_index(&source) else {
            return Delimit::Refuse;
        };
        if self.request.is_some() || busy || source_path != source.path {
            return Delimit::Refuse;
        }
        match self.documents[index].kind {
            Kind::Delimitation => Delimit::InWorkspace(source.token),
            Kind::Canvas | Kind::Alignment | Kind::Tree => {
                let mut analysis = analysis;
                analysis.uses_open_alignment = analysis.input == source.path;
                self.request = Some(Request { source, analysis });
                Delimit::Create
            }
            Kind::Sequence | Kind::Trace => Delimit::Refuse,
        }
    }

    /// The source could not start creating the workspace.
    pub fn creation_refused(&mut self) {
        self.request = None;
    }

    /// The source finished creating a workspace at `path` (empty: failed).
    /// Returns the analysis to run in a new tab for it, unless the source
    /// closed or changed meanwhile, or that file is already open (an open
    /// result is never reused or overwritten).
    pub fn workspace_created(&mut self, source_path: &str, path: &str) -> Option<Analysis> {
        let request = self.request.take()?;
        let open = self
            .target_index(&request.source)
            .is_some_and(|_| source_path == request.source.path);
        if path.is_empty() || !open || self.by_path(path).is_some() {
            return None;
        }
        Some(request.analysis)
    }

    /// Mark the tab for a new species workspace.
    pub fn mark_draft(&mut self, token: u64) {
        if let Some(i) = self.index(token) {
            self.documents[i].draft = true;
        }
    }
}

/// Whether the species dialog may open for method `method` on the active
/// document of `kind`: an idle alignment, tree or species workspace; ASAP
/// needs an alignment, open or linked to the workspace.
pub fn species_request_allowed(
    kind: &str,
    busy: bool,
    method: &str,
    linked_alignment: bool,
) -> bool {
    let fits = matches!(kind, "alignment" | "tree" | "delimitation");
    let asap =
        method != "asap" || kind == "alignment" || (kind == "delimitation" && linked_alignment);
    fits && !busy && asap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(rows: &[(&str, Kind, bool)]) -> Workspace {
        let mut w = Workspace::default();
        for (token, (path, kind, dirty)) in (1..).zip(rows) {
            let i = w.add(token, (*path).into(), *kind);
            w.set_dirty(i, *dirty);
        }
        w
    }
    fn target(token: u64, path: &str) -> Target {
        Target {
            token,
            path: path.into(),
        }
    }
    fn analysis() -> Analysis {
        Analysis {
            method: "asap".into(),
            input: "/a.afa".into(),
            rooted: false,
            settings: "{}".into(),
            display: "existing".into(),
            uses_open_alignment: true,
        }
    }

    #[test]
    fn opening_switches_to_an_open_path_and_sends_project_files_to_their_dialogs() {
        let w = workspace(&[("/a.ab1", Kind::Trace, false)]);
        assert_eq!(w.open("/a.ab1", "trace"), Open::Switch(0));
        assert_eq!(w.open("/b.spart", "delimitation"), Open::New);
        assert_eq!(w.open("/m.csv", "metadata"), Open::Project);
        assert_eq!(w.open("/s.json", "submission"), Open::Project);
    }

    #[test]
    fn a_path_opening_or_open_on_a_worker_switches_to_its_tab() {
        let mut w = Workspace::default();
        assert_eq!(w.open("/a.afa", "canvas"), Open::New);
        // Requested again while its worker still runs.
        w.add(7, "/a.afa".into(), Kind::Canvas);
        assert_eq!(w.open("/a.afa", "canvas"), Open::Switch(0));
        // Saved under another name after the worker finished.
        w.set_path(7, "/b.afa".into());
        assert_eq!(w.open("/b.afa", "canvas"), Open::Switch(0));
        assert_eq!(w.open("/a.afa", "canvas"), Open::New);
    }

    #[test]
    fn saving_asks_for_a_name_when_needed_and_traces_use_their_review() {
        let mut w = workspace(&[("/a.gb", Kind::Canvas, true), ("/r.ab1", Kind::Trace, true)]);
        let untitled = w.untitled_path(" ITS ");
        assert_eq!(untitled, "untitled:1/ITS.gb");
        assert_eq!(w.untitled_path("rbcL"), "untitled:2/rbcL.gb");
        w.add(3, untitled, Kind::Sequence);
        assert_eq!(w.save(0, false), Some(Save::Write));
        assert_eq!(w.save(0, true), Some(Save::Destination));
        assert_eq!(w.save(1, true), Some(Save::TraceReview));
        assert_eq!(w.save(1, false), Some(Save::Write));
        assert_eq!(w.save(2, false), Some(Save::Destination));
        assert_eq!(w.save(9, false), None);
    }

    #[test]
    fn a_trace_review_joins_a_working_tab_and_never_replaces_unsaved_work() {
        let mut w = workspace(&[
            ("/r.ab1", Kind::Trace, false),
            ("/o.ab1", Kind::Trace, true),
        ]);
        assert_eq!(w.trace_review("/r.ab1", false), TraceReview::Open);
        assert_eq!(w.trace_review("/r.ab1", true), TraceReview::NewTab);
        assert_eq!(w.trace_review("/o.ab1", false), TraceReview::NewTab);
        assert_eq!(w.trace_review("/new.ab1", false), TraceReview::Open);
        w.set_working(0, true);
        assert_eq!(w.trace_review("/r.ab1", true), TraceReview::Switch(0));
    }

    #[test]
    fn species_requests_need_an_idle_alignment_tree_or_workspace() {
        assert!(species_request_allowed("alignment", false, "asap", false));
        assert!(species_request_allowed("tree", false, "gmyc", false));
        assert!(!species_request_allowed("tree", false, "asap", false));
        assert!(species_request_allowed("delimitation", false, "asap", true));
        assert!(!species_request_allowed(
            "delimitation",
            false,
            "asap",
            false
        ));
        assert!(!species_request_allowed("alignment", true, "ptp", false));
        assert!(!species_request_allowed("sequence", false, "ptp", false));
    }

    #[test]
    fn closing_asks_only_for_unsaved_tabs_and_deletes_empty_drafts() {
        let mut w = workspace(&[
            ("/a.afa", Kind::Canvas, true),
            ("/w.spart", Kind::Delimitation, false),
        ]);
        assert_eq!(w.close(0), Some(Close::Ask(target(1, "/a.afa"))));
        assert_eq!(w.close(1), Some(Close::Remove { draft: false }));
        w.mark_draft(2);
        assert_eq!(w.close(1), Some(Close::Remove { draft: true }));
        assert_eq!(w.close(5), None);
    }

    #[test]
    fn quitting_asks_about_drafts_then_unsaved_tabs_then_leaves() {
        let mut w = workspace(&[
            ("/a.gb", Kind::Canvas, true),
            ("/b.gb", Kind::Canvas, false),
        ]);
        assert_eq!(w.quit(true), Quit::Drafts);
        assert_eq!(
            w.quit(false),
            Quit::Ask {
                targets: vec![target(1, "/a.gb")],
                error: ""
            }
        );
        w.set_dirty(0, false);
        w.add(3, "/w.spart".into(), Kind::Delimitation);
        w.mark_draft(3);
        assert_eq!(w.quit(false), Quit::Leave { drafts: vec![3] });
    }

    #[test]
    fn discarding_on_quit_needs_consent_for_exactly_the_unsaved_tabs() {
        let mut w = workspace(&[
            ("/a.gb", Kind::Canvas, true),
            ("/b.gb", Kind::Canvas, false),
        ]);
        let consent = vec![target(1, "/a.gb")];
        assert_eq!(w.discard_all(&consent), Quit::Leave { drafts: vec![] });
        // Another tab gained changes after the question.
        w.set_dirty(1, true);
        assert_eq!(
            w.discard_all(&consent),
            Quit::Ask {
                targets: vec![target(1, "/a.gb"), target(2, "/b.gb")],
                error: CHANGED
            }
        );
        // The agreed tab was saved under another name.
        w.set_dirty(1, false);
        w.set_path(1, "/renamed.gb".into());
        assert!(matches!(
            w.discard_all(&consent),
            Quit::Ask { error: CHANGED, .. }
        ));
    }

    #[test]
    fn saving_before_quit_goes_in_order_and_stops_on_failure_or_change() {
        let mut w = workspace(&[("/a.gb", Kind::Canvas, true), ("/b.gb", Kind::Canvas, true)]);
        let consent = vec![target(1, "/a.gb"), target(2, "/b.gb")];
        assert_eq!(w.save_all(consent.clone()), Quit::Save(target(1, "/a.gb")));
        w.set_dirty(0, false);
        assert_eq!(w.quit_saved(true), Quit::Save(target(2, "/b.gb")));
        w.set_dirty(1, false);
        assert_eq!(w.quit_saved(true), Quit::Leave { drafts: vec![] });

        w.set_dirty(0, true);
        w.set_dirty(1, true);
        assert_eq!(w.save_all(consent.clone()), Quit::Save(target(1, "/a.gb")));
        assert_eq!(w.quit_saved(false), Quit::Stay);
        // A save that reports success but leaves changes asks again.
        assert_eq!(
            w.save_all(vec![target(1, "/a.gb")]),
            Quit::Save(target(1, "/a.gb"))
        );
        assert_eq!(
            w.quit_saved(true),
            Quit::Ask {
                targets: consent.clone(),
                error: NOT_SAVED
            }
        );
        // A tab closed while saving.
        assert_eq!(w.save_all(consent), Quit::Save(target(1, "/a.gb")));
        w.remove(2);
        assert_eq!(
            w.quit_saved(true),
            Quit::Ask {
                targets: vec![target(1, "/a.gb")],
                error: CHANGED
            }
        );
    }

    #[test]
    fn a_species_analysis_on_an_alignment_opens_its_new_workspace_once() {
        let mut w = workspace(&[("/a.afa", Kind::Canvas, false)]);
        w.begin_delimitation(0);
        assert_eq!(
            w.request_delimitation("/a.afa", true, analysis()),
            Delimit::Refuse
        );
        assert_eq!(
            w.request_delimitation("/other.afa", false, analysis()),
            Delimit::Refuse
        );
        assert_eq!(
            w.request_delimitation("/a.afa", false, analysis()),
            Delimit::Create
        );
        // One request at a time.
        assert_eq!(
            w.request_delimitation("/a.afa", false, analysis()),
            Delimit::Refuse
        );
        assert_eq!(w.workspace_created("/a.afa", "/a.spart"), Some(analysis()));
        assert_eq!(
            w.workspace_created("/a.afa", "/a.spart"),
            None,
            "delivered once"
        );
    }

    #[test]
    fn a_workspace_is_not_opened_for_a_closed_source_failure_or_an_open_file() {
        let mut w = workspace(&[
            ("/a.afa", Kind::Canvas, false),
            ("/old.spart", Kind::Delimitation, false),
        ]);
        w.begin_delimitation(0);
        assert_eq!(
            w.request_delimitation("/a.afa", false, analysis()),
            Delimit::Create
        );
        assert_eq!(w.workspace_created("/a.afa", ""), None);
        assert_eq!(
            w.request_delimitation("/a.afa", false, analysis()),
            Delimit::Create
        );
        assert_eq!(w.workspace_created("/a.afa", "/old.spart"), None);
        assert_eq!(
            w.request_delimitation("/a.afa", false, analysis()),
            Delimit::Create
        );
        assert_eq!(
            w.workspace_created("/b.afa", "/a.spart"),
            None,
            "source changed"
        );
        assert_eq!(
            w.request_delimitation("/a.afa", false, analysis()),
            Delimit::Create
        );
        w.remove(1);
        assert_eq!(
            w.workspace_created("/a.afa", "/a.spart"),
            None,
            "source closed"
        );
        assert_eq!(
            w.request_delimitation("/a.afa", false, analysis()),
            Delimit::Refuse
        );
        w.creation_refused();
    }

    #[test]
    fn an_analysis_asked_for_in_a_workspace_runs_there() {
        let mut w = workspace(&[
            ("/w.spart", Kind::Delimitation, false),
            ("/a.ab1", Kind::Trace, false),
        ]);
        w.begin_delimitation(0);
        assert_eq!(
            w.request_delimitation("/w.spart", false, analysis()),
            Delimit::InWorkspace(1)
        );
        w.begin_delimitation(1);
        assert_eq!(
            w.request_delimitation("/a.ab1", false, analysis()),
            Delimit::Refuse
        );
    }
}
