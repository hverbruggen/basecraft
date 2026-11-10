//! A partition workspace owns its edits; `TreeController` remains a presentation canvas.
#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for DelimitationController {}
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, workspace_json)]
        #[qproperty(QString, selection_json)]
        #[qproperty(QString, preview_json)]
        #[qproperty(QString, sampling_summary)]
        #[qproperty(QString, statistics_json)]
        #[qproperty(QString, display_path)]
        #[qproperty(QString, active_hypothesis)]
        #[qproperty(QString, linked_alignment)]
        #[qproperty(QString, file_path)]
        #[qproperty(QString, status)]
        #[qproperty(QString, message)]
        #[qproperty(QString, message_kind)]
        #[qproperty(QString, message_details)]
        #[qproperty(bool, dirty)]
        #[qproperty(bool, busy)]
        #[qproperty(bool, can_cut)]
        #[qproperty(f64, root_height)]
        #[qproperty(f64, threshold)]
        #[qproperty(i32, theme_revision)]
        /// How the display tree is drawn: its drawing edits, as JSON.
        #[qproperty(QString, tree_edits)]
        /// Counts attached display trees, so even one at the same path is
        /// drawn afresh.
        #[qproperty(i32, display_revision)]
        /// The visible hypotheses, and a cut's preview, as bar columns for
        /// the tree canvas, as JSON.
        #[qproperty(QString, bar_columns)]
        type DelimitationController = super::DelimitationRust;
        #[qinvokable]
        fn find_query(self: Pin<&mut DelimitationController>, query: &QString, reverse: bool);
        #[qinvokable]
        fn find_poll(self: Pin<&mut DelimitationController>, direction: i32) -> QString;

        #[qinvokable]
        fn accessible_page(self: &DelimitationController, page: i32) -> QString;
        #[qinvokable]
        fn document_record(self: &DelimitationController) -> QString;
        #[qinvokable]
        fn record_revision(self: &DelimitationController) -> QString;
        #[qinvokable]
        fn open_file(self: Pin<&mut DelimitationController>, path: &QString) -> bool;
        #[qinvokable]
        fn combine_files(
            self: Pin<&mut DelimitationController>,
            paths: &QString,
            output: &QString,
            selection: &QString,
        ) -> bool;
        #[qinvokable]
        fn combine_candidates(self: Pin<&mut DelimitationController>, paths: &QString) -> QString;
        #[qinvokable]
        fn combine_name(self: &DelimitationController, paths: &QString) -> QString;
        /// Root the display tree at its midpoint ("midpoint") or by minimum
        /// variance ("mrttv") into a new tree file beside it, and attach that.
        /// For analyses such as PTP that need a rooted tree.
        #[qinvokable]
        fn root_display_tree(self: Pin<&mut DelimitationController>, method: &QString) -> bool;
        #[qinvokable]
        fn attach_tree(
            self: Pin<&mut DelimitationController>,
            path: &QString,
            rooted: bool,
        ) -> bool;
        #[qinvokable]
        fn activate(self: Pin<&mut DelimitationController>, id: &QString);
        #[qinvokable]
        fn analysis_json(self: &DelimitationController) -> QString;
        #[qinvokable]
        fn action(self: Pin<&mut DelimitationController>, name: &QString, args: &QString);
        #[qinvokable]
        fn select_group(
            self: Pin<&mut DelimitationController>,
            hypothesis: &QString,
            group: &QString,
            toggle: bool,
        );
        #[qinvokable]
        fn highlight_taxa(self: Pin<&mut DelimitationController>, names: &QString);
        #[qinvokable]
        fn tree_taxa(self: Pin<&mut DelimitationController>, names: &QString);
        #[qinvokable]
        fn coverage_json(self: &DelimitationController, geometry: &QString) -> QString;
        #[qsignal]
        #[cxx_name = "taxaSelected"]
        fn taxa_selected(self: Pin<&mut DelimitationController>, names: QString);
        #[qsignal]
        #[cxx_name = "displayTaxaSelected"]
        fn display_taxa_selected(self: Pin<&mut DelimitationController>, names: QString);
        #[qinvokable]
        fn columns_json(self: &DelimitationController, left: f64) -> QString;
        #[qinvokable]
        fn export_figure(
            self: Pin<&mut DelimitationController>,
            path: &QString,
            scope: i32,
            publication: bool,
        );
        #[qinvokable]
        fn export_figure_png(
            self: Pin<&mut DelimitationController>,
            path: &QString,
            scope: i32,
            publication: bool,
            width: f64,
        );
        #[qinvokable]
        fn preview_cut(self: Pin<&mut DelimitationController>, threshold: f64);
        #[qinvokable]
        fn preview_position(self: Pin<&mut DelimitationController>, geometry: &QString, x: f64);
        #[qinvokable]
        fn cancel_cut(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn save_cut(self: Pin<&mut DelimitationController>, name: &QString);
        #[qinvokable]
        fn undo(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn redo(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn save(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn save_as(self: Pin<&mut DelimitationController>, path: &QString) -> bool;
        #[qinvokable]
        fn export_svg(self: Pin<&mut DelimitationController>, path: &QString);
        #[qinvokable]
        fn export_statistics(self: Pin<&mut DelimitationController>, path: &QString);
        #[qinvokable]
        fn export_analysis(self: Pin<&mut DelimitationController>, path: &QString);
        #[qinvokable]
        fn run_analysis(
            self: Pin<&mut DelimitationController>,
            method: &QString,
            input: &QString,
            rooted: bool,
            settings: &QString,
        );
        #[qsignal]
        #[cxx_name = "displayTreeReady"]
        fn display_tree_ready(self: Pin<&mut DelimitationController>, success: bool);
        #[qsignal]
        #[cxx_name = "analysisFinished"]
        fn analysis_finished(self: Pin<&mut DelimitationController>, success: bool);
        #[qinvokable]
        fn discard_empty_workspace(self: Pin<&mut DelimitationController>) -> bool;
        #[qinvokable]
        fn cancel_job(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn poll_job(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn flush(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn check_theme(self: Pin<&mut DelimitationController>);
        #[qinvokable]
        fn memory_report(self: &DelimitationController) -> QString;
    }
}
use basecraft_docs::TreeDoc;
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{
    Op,
    delimitation::{
        DelimitationEdit, Group, Hypothesis, HypothesisOrigin, Provenance, TreeDrawingEdit,
    },
};
use basecraft_viewmodel::delimitation::{self, Selection};

use crate::document_jobs::{
    BackgroundDocumentJob as DocumentJob, bundled, declared_parameters, failure, input_role,
    primary_member, result_file,
};

/// A registered analysis running for this workspace; dropping it cancels it.
struct RunningAnalysis {
    job: DocumentJob,
    display_job: bool,
    display_rooted: bool,
    display_output: PathBuf,
    generation: u64,
    identity: (String, String),
}
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    pin::Pin,
};

pub struct DelimitationRust {
    find: basecraft_viewmodel::find::Session,
    find_status_revision: u64,
    workspace_json: QString,
    selection_json: QString,
    preview_json: QString,
    statistics_json: QString,
    sampling_summary: QString,
    display_path: QString,
    active_hypothesis: QString,
    linked_alignment: QString,
    file_path: QString,
    status: QString,
    message: QString,
    message_kind: QString,
    message_details: QString,
    dirty: bool,
    busy: bool,
    can_cut: bool,
    root_height: f64,
    threshold: f64,
    theme_revision: i32,
    tree_edits: QString,
    display_revision: i32,
    bar_columns: QString,
    palette: Vec<String>,
    // Modification time of the theme file last applied; empty before the first read.
    theme_stamp: crate::bridge::ThemeStamp,
    session: Option<Session>,
    doc: Option<DocId>,
    selection: Selection,
    display_tree: Option<TreeDoc>,
    time_tree: Option<(TreeDoc, basecraft_delimitation::time_tree::TimeTree, String)>,
    preview: Vec<BTreeSet<String>>,
    analysis: Option<RunningAnalysis>,
}
impl Default for DelimitationRust {
    fn default() -> Self {
        Self {
            find: basecraft_viewmodel::find::Session::default(),
            find_status_revision: u64::MAX,
            workspace_json: QString::from("{}"),
            selection_json: QString::from("[]"),
            preview_json: QString::from("[]"),
            statistics_json: QString::from("{}"),
            sampling_summary: QString::default(),
            display_path: QString::default(),
            active_hypothesis: QString::default(),
            linked_alignment: QString::default(),
            file_path: QString::default(),
            status: QString::default(),
            message: QString::default(),
            message_kind: QString::default(),
            message_details: QString::default(),
            dirty: false,
            busy: false,
            can_cut: false,
            root_height: 0.0,
            threshold: 0.0,
            theme_revision: 0,
            tree_edits: QString::from("[]"),
            display_revision: 0,
            bar_columns: QString::from("[]"),
            palette: delimitation::palette(&basecraft_viewmodel::Theme::default()),
            theme_stamp: crate::bridge::ThemeStamp::default(),
            session: None,
            doc: None,
            selection: Selection::default(),
            display_tree: None,
            time_tree: None,
            preview: vec![],
            analysis: None,
        }
    }
}
fn rows(geometry: &str) -> Vec<(String, f32, f32)> {
    serde_json::from_str::<Value>(geometry)
        .ok()
        .and_then(|v| v.get("rows").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            Some((
                v.get("name")?.as_str()?.into(),
                number(v.get("y")?.as_f64()?),
                number(v.get("height")?.as_f64()?),
            ))
        })
        .collect()
}
#[allow(clippy::cast_possible_truncation)]
fn number(v: f64) -> f32 {
    v as f32
}
fn group(members: BTreeSet<String>, label: String) -> Group {
    Group {
        id: ulid::Ulid::generate().to_string(),
        label,
        color: String::new(),
        members,
        scores: vec![],
        individual_scores: BTreeMap::new(),
        attributes: BTreeMap::new(),
    }
}
impl qobject::DelimitationController {
    pub fn accessible_page(&self, page: i32) -> QString {
        let access_thread = self.qt_thread();
        let state = self.rust();
        let result = match state.session.as_ref().zip(state.doc) {
            Some((s, d)) => {
                let workspace = crate::bridge::checked_document!(access_thread, s.delimitation(d));
                Some(basecraft_viewmodel::accessible::delimitation(
                    workspace, page,
                ))
            }
            None => None,
        };
        QString::from(
            result
                .map_or_else(
                    || json!({"page":0,"pages":1,"text":"No workspace loaded"}),
                    |p| json!({"page":p.page,"pages":p.pages,"text":p.text}),
                )
                .to_string(),
        )
    }

    pub fn document_record(&self) -> QString {
        crate::provenance_bridge::session_report(self.rust().session.as_ref(), self.rust().doc)
    }
    pub fn record_revision(&self) -> QString {
        crate::provenance_bridge::session_revision(self.rust().session.as_ref(), self.rust().doc)
    }

    fn fail(mut self: Pin<&mut Self>, text: &str) {
        let summary = crate::bridge::error_summary(text);
        let details = if summary == text.trim() {
            ""
        } else {
            text.trim()
        };
        self.as_mut().set_message_details(QString::from(details));
        self.as_mut().set_message_kind(QString::from("error"));
        self.set_message(QString::from(&summary));
    }
    fn refresh(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let Some((data, status)) = ({
            let r = self.rust();
            r.session.as_ref().zip(r.doc).map(|(s, d)| {
                let w = crate::bridge::checked_document!(access_thread, s.delimitation(d));
                (
                    serde_json::to_string(w).unwrap(),
                    format!(
                        "{} specimens · {} hypotheses · {} selected",
                        w.specimens.len(),
                        w.hypotheses.len(),
                        r.selection.specimens.len()
                    ),
                )
            })
        }) else {
            return;
        };
        self.as_mut().set_workspace_json(QString::from(&data));
        self.as_mut().set_status(QString::from(&status));
        self.as_mut().update_tree_edits();
        self.update_bar_columns();
    }
    fn update_tree_edits(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let edits = {
            let r = self.rust();
            let Some((s, d)) = r.session.as_ref().zip(r.doc) else {
                return;
            };
            let workspace = crate::bridge::checked_document!(access_thread, s.delimitation(d));
            serde_json::to_string(&workspace.view.tree_edits).unwrap_or_else(|_| "[]".into())
        };
        if self.rust().tree_edits.to_string() != edits {
            self.set_tree_edits(QString::from(&edits));
        }
    }
    // BackgroundDocumentJob exclusively owns the Session until poll_job restores it.
    fn workspace_available(mut self: Pin<&mut Self>) -> bool {
        let error = if self.rust().busy {
            Some("Species analysis is running; wait for completion or cancel it first")
        } else if self.rust().session.is_none() || self.rust().doc.is_none() {
            Some("No species workspace is open")
        } else {
            None
        };
        if let Some(error) = error {
            self.as_mut().fail(error);
            return false;
        }
        true
    }
    fn edit(mut self: Pin<&mut Self>, edit: DelimitationEdit) -> Result<(), String> {
        if !self.as_mut().workspace_available() {
            return Err(self.rust().message.to_string());
        }
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let s = r.session.as_mut().ok_or("No workspace open")?;
            s.apply(
                r.doc.ok_or("No workspace open")?,
                Op::DelimitationEdit {
                    edit: Box::new(edit),
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        };
        match &result {
            Ok(()) => {
                self.as_mut().set_statistics_json(QString::from("{}"));
                self.as_mut().set_dirty(true);
                self.as_mut().set_message(QString::default());
                self.refresh();
            }
            Err(e) => self.fail(e),
        }
        result
    }
    #[allow(clippy::many_single_char_names)]
    pub fn open_file(mut self: Pin<&mut Self>, path: &QString) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        if self.rust().busy {
            self.fail("Wait for the running operation before replacing this document");
            return false;
        }
        let p = PathBuf::from(path.to_string());
        let result = (|| -> basecraft_ops::Result<_> {
            let mut s = Session::recoverable(crate::bridge::project_root(&p))?;
            let d = s.open(&p)?;
            if !s.is_delimitation(d) {
                return Err(basecraft_ops::Error::Invalid(
                    "This file is not a species workspace".into(),
                ));
            }
            let linked = basecraft_ops::delimitation::linked_alignment(&s, d)?.map(str::to_owned);
            Ok((s, d, linked))
        })();
        match result {
            Ok((s, d, linked)) => {
                let w = crate::bridge::checked_document!(access_thread, s.delimitation(d));
                let display = w.view.display_tree.clone();
                self.as_mut()
                    .set_linked_alignment(QString::from(linked.unwrap_or_default()));
                // Edit a hypothesis that can be edited, if there is one.
                let active = w
                    .hypotheses
                    .iter()
                    .find(|h| {
                        !matches!(
                            h.origin,
                            HypothesisOrigin::Imported | HypothesisOrigin::Inferred
                        )
                    })
                    .or_else(|| w.hypotheses.first())
                    .map(|h| h.id.clone())
                    .unwrap_or_default();
                {
                    let mut r = self.as_mut().rust_mut();
                    r.session = Some(s);
                    r.doc = Some(d);
                }
                self.as_mut().set_file_path(path.clone());
                self.as_mut().set_active_hypothesis(QString::from(&active));
                self.as_mut().refresh();
                if let Some(display) = display {
                    self.as_mut().attach_tree(&QString::from(&display), false);
                }
                true
            }
            Err(e) => {
                self.fail(&e.to_string());
                false
            }
        }
    }
    /// A free `<common root>.combinedNN.spart` beside the last file.
    pub fn combine_name(&self, paths: &QString) -> QString {
        let paths: Vec<PathBuf> = serde_json::from_str(&paths.to_string()).unwrap_or_default();
        QString::from(
            basecraft_ops::delimitation::combined_path(&paths)
                .to_string_lossy()
                .as_ref(),
        )
    }
    /// Hypotheses in each file, best analysis results first, for the combine dialog.
    pub fn combine_candidates(mut self: Pin<&mut Self>, paths: &QString) -> QString {
        let result = (|| -> Result<Vec<Value>, String> {
            let paths: Vec<PathBuf> =
                serde_json::from_str(&paths.to_string()).map_err(|e| e.to_string())?;
            basecraft_ops::delimitation::combine_candidates(&paths)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|file| {
                    let hypotheses: Vec<Value> = file.hypotheses.into_iter().map(|entry| {
                        let h = entry.hypothesis;
                        let (code, method) = delimitation::origin(&h);
                        json!({"id": h.id, "name": h.name, "code": code, "method": method,
                            "species": h.groups.len(), "unassigned": h.unassigned.len(),
                            "best": entry.best, "parent": h.parent,
                            "scores": h.scores.iter().map(|s| json!({"value": s.value, "scale": s.scale})).collect::<Vec<_>>()})
                    }).collect();
                    Ok(json!({"path": file.path, "file": file.file,
                        "specimens": file.specimens, "hypotheses": hypotheses}))
                }).collect()
        })();
        match result {
            Ok(files) => QString::from(&Value::Array(files).to_string()),
            Err(error) => {
                self.as_mut().fail(&error);
                QString::from("[]")
            }
        }
    }
    pub fn combine_files(
        mut self: Pin<&mut Self>,
        paths: &QString,
        output: &QString,
        selection: &QString,
    ) -> bool {
        let result = (|| -> Result<_, String> {
            let paths: Vec<PathBuf> =
                serde_json::from_str(&paths.to_string()).map_err(|e| e.to_string())?;
            let output = PathBuf::from(output.to_string());
            let keep: Option<basecraft_ops::delimitation::CombineSelection> =
                Some(selection.to_string())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| serde_json::from_str(&s))
                    .transpose()
                    .map_err(|e| e.to_string())?;
            let mut session = Session::recoverable(crate::bridge::project_root(&output))
                .map_err(|e| e.to_string())?;
            basecraft_ops::delimitation::combine_to(&mut session, &paths, &output, keep.as_ref())
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        match result {
            Ok(()) => true,
            Err(error) => {
                self.as_mut().fail(&error);
                false
            }
        }
    }
    pub fn root_display_tree(mut self: Pin<&mut Self>, method: &QString) -> bool {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        if !self.as_mut().workspace_available() {
            return false;
        }
        let result = (|| -> Result<PathBuf, String> {
            let state = self.rust();
            let session = state.session.as_ref().ok_or("No workspace")?;
            let work = session
                .delimitation(state.doc.ok_or("No workspace")?)
                .map_err(|e| e.to_string())?;
            let display = basecraft_ops::delimitation::checked_display_tree(work)
                .map_err(|e| e.to_string())?
                .ok_or("No display tree")?;
            let mut rooting_session = Session::recoverable(crate::bridge::project_root(&display))
                .map_err(|e| e.to_string())?;
            root_tree_copy(&mut rooting_session, &display, &method.to_string())
        })();
        match result {
            Ok(path) => self
                .as_mut()
                .attach_tree(&QString::from(path.to_string_lossy().as_ref()), false),
            Err(e) => {
                self.fail(&format!("Rooting the tree: {e}"));
                false
            }
        }
    }
    pub fn attach_tree(mut self: Pin<&mut Self>, path: &QString, rooted: bool) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        if !self.as_mut().workspace_available() {
            return false;
        }
        let result = (|| -> Result<_, Box<dyn std::error::Error>> {
            let mut p = PathBuf::from(path.to_string());
            if let Some((session, doc)) = self.rust().session.as_ref().zip(self.rust().doc) {
                let work = session.delimitation(doc)?;
                if work.view.display_tree.as_deref() == Some(path.to_string().as_str()) {
                    p = basecraft_ops::delimitation::checked_display_tree(work)?
                        .ok_or("No display tree")?;
                }
            }
            let bytes = std::fs::read(&p)?;
            let trees = basecraft_io::newick::parse_trees(std::str::from_utf8(&bytes)?)?;
            if trees.len() != 1 {
                return Err("Choose a file containing exactly one display tree".into());
            }
            let tree = TreeDoc::from_parsed(&trees[0].name, &trees[0]);
            // A manual cut works on the tree as drawn, so it needs ultrametric
            // ages but not a rooting assertion or a bifurcating tree. The
            // tolerance allows Newick lengths printed to six decimals.
            let time = basecraft_delimitation::time_tree::validate(&tree, true, 1e-4, false);
            let hash = basecraft_io::hash_bytes(&bytes);
            Ok((p, tree, time, hash, bytes))
        })();
        match result {
            Ok((source, tree, time, hash, _bytes)) => {
                let retained = source;
                self.as_mut().rust_mut().display_tree = Some(tree.clone());
                let warning = time.as_ref().err().map(ToString::to_string);
                if let Ok(time) = time {
                    let height = time.root_height;
                    self.as_mut().rust_mut().time_tree = Some((tree, time, hash.clone()));
                    self.as_mut().set_can_cut(true);
                    self.as_mut().set_root_height(height);
                } else {
                    self.as_mut().rust_mut().time_tree = None;
                    self.as_mut().set_can_cut(false);
                }
                self.as_mut()
                    .set_display_path(QString::from(retained.to_string_lossy().as_ref()));
                let changed = {
                    let state = self.rust();
                    let Some((session, doc)) = state.session.as_ref().zip(state.doc) else {
                        self.fail("No species workspace is open");
                        return false;
                    };
                    crate::bridge::checked_document!(access_thread, session.delimitation(doc))
                        .view
                        .display_tree
                        .as_deref()
                        != Some(path.to_string().as_str())
                };
                if changed {
                    let evidence = {
                        let state = self.rust();
                        let Some((session, doc)) = state.session.as_ref().zip(state.doc) else {
                            self.fail("No species workspace is open");
                            return false;
                        };
                        basecraft_ops::delimitation::reference_input(
                            &session.root().join(session.doc_key(doc)),
                            &retained,
                            &hash,
                        )
                    };
                    let run = match evidence {
                        Ok(run) => run,
                        Err(error) => {
                            self.fail(&error.to_string());
                            return false;
                        }
                    };
                    if self
                        .as_mut()
                        .edit(DelimitationEdit::SetDisplayTree {
                            path: retained.to_string_lossy().into_owned(),
                            hash: hash.clone(),
                            evidence_run: run.directory().display().to_string(),
                        })
                        .is_err()
                    {
                        return false;
                    }
                }
                // A newly attached tree is drawn afresh even at the same path.
                let revision = self.rust().display_revision + 1;
                self.as_mut().set_display_revision(revision);
                self.as_mut().update_tree_edits();
                self.as_mut().update_bar_columns();
                if rooted && let Some(warning) = warning {
                    self.fail(&warning);
                }
                true
            }
            Err(e) => {
                self.fail(&format!("Cannot attach tree: {e}"));
                false
            }
        }
    }
    pub fn activate(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut().set_active_hypothesis(id.clone());
        self.refresh();
    }
    pub fn analysis_json(&self) -> QString {
        let access_thread = self.qt_thread();
        let value = (|| -> Option<Value> {
            let state = self.rust();
            let (session, doc) = state.session.as_ref().zip(state.doc)?;
            let hypothesis =
                crate::bridge::checked_document!(access_thread, session.delimitation(doc))
                    .hypotheses
                    .iter()
                    .find(|h| h.id == state.active_hypothesis.to_string())?;
            if hypothesis.origin != HypothesisOrigin::Inferred {
                return None;
            }
            let report = hypothesis.provenance.parameters.get("analysis_report")?;
            let hash = report["hash"].as_str()?;
            let digest = hash.strip_prefix("b3:")?;
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            let retained =
                basecraft_ops::session::objects_dir(session.recovery_root()).join(digest);
            let bytes = std::fs::read(retained)
                .or_else(|_| std::fs::read(report["path"].as_str().unwrap_or("")))
                .ok()?;
            if basecraft_io::hash_bytes(&bytes) != hash {
                return None;
            }
            let result: Value = serde_json::from_slice(&bytes).ok()?;
            Some(result["metrics"]["gmyc"].clone())
        })()
        .unwrap_or_else(|| json!({}));
        QString::from(value.to_string())
    }
    pub fn action(mut self: Pin<&mut Self>, name: &QString, args: &QString) {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        if !self.as_mut().workspace_available() {
            return;
        }
        let result = (|| -> Result<DelimitationEdit, String> {
            let r = self.rust();
            let s = r.session.as_ref().ok_or("No workspace open")?;
            let w = s
                .delimitation(r.doc.ok_or("No workspace open")?)
                .map_err(|e| e.to_string())?;
            let args: Value = serde_json::from_str(&args.to_string()).map_err(|e| e.to_string())?;
            let text = |key: &str| {
                args.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            // Context menus name their hypothesis; toolbar actions use the active one.
            let id = args
                .get("hypothesis")
                .and_then(Value::as_str)
                .map_or_else(|| r.active_hypothesis.to_string(), str::to_string);
            let members = r.selection.specimens.clone();
            Ok(match name.to_string().as_str() {
                "roster_links" => DelimitationEdit::SetRosterLinks {
                    links: serde_json::from_value(args["links"].clone())
                        .map_err(|e| e.to_string())?,
                    metadata: serde_json::from_value(args["metadata"].clone())
                        .map_err(|e| e.to_string())?,
                },
                "create" if args.get("source").and_then(Value::as_str).is_some() => {
                    DelimitationEdit::Derive {
                        source: text("source"),
                        id: ulid::Ulid::generate().to_string(),
                        name: text("name"),
                    }
                }
                "create" => DelimitationEdit::Add {
                    hypothesis: Hypothesis {
                        id: ulid::Ulid::generate().to_string(),
                        name: text("name"),
                        origin: HypothesisOrigin::Manual,
                        parent: None,
                        groups: vec![],
                        unassigned: w.specimens.iter().map(|s| s.id.clone()).collect(),
                        scores: vec![],
                        provenance: Provenance::default(),
                        attributes: BTreeMap::new(),
                    },
                },
                "derive" => DelimitationEdit::Derive {
                    source: id,
                    id: ulid::Ulid::generate().to_string(),
                    name: text("name"),
                },
                "rename" => DelimitationEdit::Rename {
                    hypothesis: id,
                    name: text("name"),
                },
                "rename_group" => DelimitationEdit::RenameGroup {
                    hypothesis: id,
                    group: text("group"),
                    label: text("label"),
                },
                "group" => {
                    if members.is_empty() {
                        return Err("Select specimens first".into());
                    }
                    DelimitationEdit::Split {
                        hypothesis: id,
                        group: group(members, text("label")),
                    }
                }
                "merge" => DelimitationEdit::Merge {
                    hypothesis: id,
                    groups: serde_json::from_value(
                        args.get("groups").cloned().unwrap_or(json!([])),
                    )
                    .map_err(|e| e.to_string())?,
                    id: ulid::Ulid::generate().to_string(),
                    label: text("label"),
                },
                "reassign" => DelimitationEdit::Reassign {
                    hypothesis: id,
                    specimens: members,
                    group: Some(text("group")),
                },
                "unassign" => DelimitationEdit::Reassign {
                    hypothesis: id,
                    specimens: members,
                    group: None,
                },
                "move_visible" => {
                    let mut view = w.view.clone();
                    let index = view
                        .visible
                        .iter()
                        .position(|h| h == &text("id"))
                        .ok_or("Hypothesis is not visible")?;
                    let next = if args["direction"].as_i64().unwrap_or(1) < 0 {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(view.visible.len() - 1)
                    };
                    view.visible.swap(index, next);
                    DelimitationEdit::SetView { view }
                }
                // A drawing edit from the tree canvas, kept with this view. A
                // style replaces the last; an order replaces earlier orders and
                // flips; flipping a clade again undoes its flip.
                "tree_edit" => {
                    let edit: TreeDrawingEdit =
                        serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
                    let mut view = w.view.clone();
                    let edits = &mut view.tree_edits;
                    match &edit {
                        TreeDrawingEdit::Style { .. } => {
                            edits.retain(|e| !matches!(e, TreeDrawingEdit::Style { .. }));
                            edits.push(edit);
                        }
                        TreeDrawingEdit::Ladderize { .. } => {
                            edits.retain(|e| matches!(e, TreeDrawingEdit::Style { .. }));
                            edits.push(edit);
                        }
                        TreeDrawingEdit::Rotate { clade } => {
                            let tree = r
                                .display_tree
                                .as_ref()
                                .ok_or("Attach a display tree first")?;
                            let node = tree.resolve(clade).ok();
                            // Flips commute, so look back over the flips only.
                            let earlier = edits
                                .iter()
                                .rposition(|e| match e {
                                    TreeDrawingEdit::Rotate { clade } => {
                                        node.is_some() && tree.resolve(clade).ok() == node
                                    }
                                    TreeDrawingEdit::Style { .. } => false,
                                    TreeDrawingEdit::Ladderize { .. } => true,
                                })
                                .filter(|&k| matches!(edits[k], TreeDrawingEdit::Rotate { .. }));
                            match earlier {
                                Some(k) => {
                                    edits.remove(k);
                                }
                                None => edits.push(edit),
                            }
                        }
                    }
                    DelimitationEdit::SetView { view }
                }
                "hide" => {
                    let mut view = w.view.clone();
                    view.visible.retain(|h| h != &id);
                    DelimitationEdit::SetView { view }
                }
                // Style one hypothesis's column, or with `hypotheses` several
                // at once as one edit. A null gap or spacing scales with the
                // bar width again.
                "style" => {
                    let mut view = w.view.clone();
                    let ids: Vec<String> = match args.get("hypotheses") {
                        Some(list) => {
                            serde_json::from_value(list.clone()).map_err(|e| e.to_string())?
                        }
                        None => vec![id.clone()],
                    };
                    let pixels = |key: &str| -> Option<Option<u32>> {
                        let value = args.get(key)?;
                        Some(value.as_u64().map(|v| {
                            u32::try_from(v)
                                .unwrap_or(u32::MAX)
                                .min(delimitation::MAX_GAP)
                        }))
                    };
                    for id in ids {
                        let style = view.columns.entry(id.clone()).or_default();
                        if let Some(width) = args.get("width").and_then(Value::as_u64) {
                            style.width =
                                Some(u32::try_from(width).unwrap_or(u32::MAX).clamp(
                                    delimitation::MIN_BAR_WIDTH,
                                    delimitation::MAX_BAR_WIDTH,
                                ));
                        }
                        if let Some(gap) = pixels("gap") {
                            style.gap = gap;
                        }
                        if let Some(spacing) = pixels("spacing") {
                            style.spacing = spacing;
                        }
                        if let Some(names) = args.get("names").and_then(Value::as_bool) {
                            style.names = names;
                        }
                        if let Some(colors) = args.get("colors").and_then(Value::as_object) {
                            for (group, color) in colors {
                                let color = color.as_str().unwrap_or("");
                                if color.is_empty() {
                                    style.colors.remove(group);
                                } else if color.len() == 7
                                    && color.starts_with('#')
                                    && color[1..].bytes().all(|b| b.is_ascii_hexdigit())
                                {
                                    style
                                        .colors
                                        .insert(group.clone(), color.to_ascii_lowercase());
                                } else {
                                    return Err(format!("{color} is not a #rrggbb colour"));
                                }
                            }
                        }
                        if *style == basecraft_protocol::delimitation::ColumnStyle::default() {
                            view.columns.remove(&id);
                        }
                    }
                    DelimitationEdit::SetView { view }
                }
                "visible" => {
                    let mut view = w.view.clone();
                    view.visible =
                        serde_json::from_value(args.get("ids").cloned().unwrap_or(json!([])))
                            .map_err(|e| e.to_string())?;
                    DelimitationEdit::SetView { view }
                }
                _ => return Err("Unknown grouping action".into()),
            })
        })();
        match result {
            Ok(edit) => {
                let created = match &edit {
                    DelimitationEdit::Add { hypothesis } => Some(hypothesis.id.clone()),
                    DelimitationEdit::Derive { id, .. } => Some(id.clone()),
                    _ => None,
                };
                let mut this = self;
                // A new hypothesis and its column undo as one step.
                this.as_mut().set_group(created.is_some());
                if this.as_mut().edit(edit).is_ok()
                    && let Some(id) = created
                {
                    this.as_mut().activate(&QString::from(&id));
                    this.as_mut().show_created(id);
                }
                this.set_group(false);
            }
            Err(e) => self.fail(&e),
        }
    }
    fn emit_selection(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let text = serde_json::to_string(&self.rust().selection.specimens).unwrap();
        self.as_mut().set_selection_json(QString::from(&text));
        self.as_mut().taxa_selected(QString::from(&text));
        let display = (|| -> Option<_> {
            let state = self.rust();
            let (session, doc) = state.session.as_ref().zip(state.doc)?;
            let tree = state.display_tree.as_ref()?;
            let mapping = delimitation::tree_mapping(
                crate::bridge::checked_document!(access_thread, session.delimitation(doc)),
                &tree
                    .tips_in_order()
                    .iter()
                    .map(|&i| tree.node(i).name.clone())
                    .collect(),
            );
            Some(
                mapping
                    .iter()
                    .filter(|(_, members)| !members.is_disjoint(&state.selection.specimens))
                    .map(|(tip, _)| tip.clone())
                    .collect::<Vec<_>>(),
            )
        })();
        if let Some(display) = display {
            self.as_mut()
                .display_taxa_selected(QString::from(serde_json::to_string(&display).unwrap()));
        }
        self.refresh();
    }
    pub fn select_group(
        mut self: Pin<&mut Self>,
        hypothesis: &QString,
        group: &QString,
        toggle: bool,
    ) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let h = r.session.as_ref().zip(r.doc).and_then(|(s, d)| {
                crate::bridge::checked_document!(access_thread, s.delimitation(d))
                    .hypotheses
                    .iter()
                    .find(|h| h.id == hypothesis.to_string())
            });
            h.ok_or_else(|| "Unknown hypothesis".to_string())
                .and_then(|h| r.selection.group(h, &group.to_string(), toggle))
        };
        match result {
            Ok(()) => self.emit_selection(),
            Err(e) => self.fail(&e),
        }
    }
    pub fn tree_taxa(mut self: Pin<&mut Self>, names: &QString) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let mapped = (|| -> Option<_> {
            let r = self.rust();
            let (session, doc) = r.session.as_ref().zip(r.doc)?;
            let display = r.display_tree.as_ref()?;
            let mapping = delimitation::tree_mapping(
                crate::bridge::checked_document!(access_thread, session.delimitation(doc)),
                &display
                    .tips_in_order()
                    .iter()
                    .map(|&i| display.node(i).name.clone())
                    .collect(),
            );
            let names: BTreeSet<String> = serde_json::from_str(&names.to_string()).ok()?;
            Some(
                names
                    .iter()
                    .flat_map(|name| mapping.get(name).into_iter().flatten().cloned())
                    .collect(),
            )
        })();
        if let Some(mapped) = mapped
            && self.as_mut().rust_mut().selection.receive(mapped, "tree")
        {
            self.emit_selection();
        }
    }
    pub fn coverage_json(&self, geometry: &QString) -> QString {
        let access_thread = self.qt_thread();
        let r = self.rust();
        let Some((session, doc)) = r.session.as_ref().zip(r.doc) else {
            return QString::from("{}");
        };
        let work = crate::bridge::checked_document!(access_thread, session.delimitation(doc));
        let names = rows(&geometry.to_string())
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        let mapping = delimitation::tree_mapping(work, &names);
        let represented: BTreeSet<_> = mapping.values().flatten().cloned().collect();
        QString::from(json!({"unmatched_tips":names.difference(&mapping.keys().cloned().collect()).collect::<Vec<_>>(),"absent_specimens":work.specimens.iter().filter(|s|!represented.contains(&s.id)).map(|s|s.id.clone()).collect::<Vec<_>>()}).to_string())
    }
    pub fn highlight_taxa(mut self: Pin<&mut Self>, names: &QString) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let ids = serde_json::from_str::<BTreeSet<String>>(&names.to_string());
        if let Ok(names) = ids {
            let mapped = {
                let r = self.rust();
                let Some((s, d)) = r.session.as_ref().zip(r.doc) else {
                    return;
                };
                names
                    .into_iter()
                    .filter(|name| {
                        crate::bridge::checked_document!(access_thread, s.delimitation(d))
                            .specimens
                            .iter()
                            .any(|specimen| &specimen.id == name)
                    })
                    .collect()
            };
            if self.as_mut().rust_mut().selection.receive(mapped, "linked") {
                self.emit_selection();
            }
        }
    }
    /// Recompute `bar_columns` after the workspace, display tree, preview
    /// or palette changed.
    fn update_bar_columns(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let json = {
            let r = self.rust();
            match (r.session.as_ref().zip(r.doc), r.display_tree.as_ref()) {
                (Some((s, d)), Some(tree)) => {
                    let work = with_preview(
                        crate::bridge::checked_document!(access_thread, s.delimitation(d)),
                        tree,
                        &r.preview,
                    );
                    let columns = delimitation::tip_columns(&work, &tip_names(tree), &r.palette);
                    serde_json::to_string(&columns).unwrap_or_else(|_| "[]".into())
                }
                _ => "[]".into(),
            }
        };
        if self.rust().bar_columns.to_string() != json {
            self.as_mut().set_bar_columns(QString::from(&json));
        }
    }
    /// Visible columns with their letter codes, plus a legend of the codes used.
    pub fn columns_json(&self, left: f64) -> QString {
        let access_thread = self.qt_thread();
        let r = self.rust();
        let Some((s, d)) = r.session.as_ref().zip(r.doc) else {
            return QString::from("{\"columns\":[],\"legend\":[]}");
        };
        let work = crate::bridge::checked_document!(access_thread, s.delimitation(d));
        let mut legend = Vec::new();
        let columns: Vec<Value> = delimitation::columns(work, number(left))
            .into_iter()
            .filter_map(|c| {
                let h = work.hypotheses.iter().find(|h| h.id == c.hypothesis)?;
                let (code, method) = delimitation::origin(h);
                if !legend.contains(&(code, method)) {
                    legend.push((code, method));
                }
                let style = work.view.columns.get(&h.id);
                Some(json!({"id": h.id, "x": c.x, "width": c.width, "code": code,
                    "gap": c.gap, "spacing": c.spacing,
                    "gap_set": style.is_some_and(|s| s.gap.is_some()),
                    "spacing_set": style.is_some_and(|s| s.spacing.is_some()),
                    "names": work.view.columns.get(&h.id).is_some_and(|s| s.names),
                    "method": method, "name": h.name, "editable": !matches!(h.origin,
                    HypothesisOrigin::Imported | HypothesisOrigin::Inferred),
                    "groups": h.groups.iter().enumerate().map(|(i, g)| json!({"id": g.id,
                        "label": g.label, "members": g.members.len(),
                        "color": work.view.columns.get(&h.id).and_then(|s| s.colors.get(&g.id)).cloned()
                            .unwrap_or_else(|| r.palette[i % r.palette.len()].clone()),
                        "theme": r.palette[i % r.palette.len()]})).collect::<Vec<_>>()}))
            })
            .collect();
        legend.sort_unstable();
        QString::from(
            &json!({"columns": columns, "legend": legend.iter().map(|(c, m)| json!({"code": c, "method": m})).collect::<Vec<_>>()})
                .to_string(),
        )
    }
    pub fn export_figure(self: Pin<&mut Self>, path: &QString, _scope: i32, publication: bool) {
        // The comparison figure is always complete: every visible column and tip.
        let theme = if publication {
            crate::bridge::publication_theme()
        } else {
            crate::bridge::load_theme()
                .map(|(theme, _)| theme)
                .unwrap_or_default()
        };
        self.export_with(path, &theme, None);
    }
    pub fn export_figure_png(
        self: Pin<&mut Self>,
        path: &QString,
        _scope: i32,
        publication: bool,
        width: f64,
    ) {
        let theme = if publication {
            crate::bridge::publication_theme()
        } else {
            crate::bridge::load_theme()
                .map(|(theme, _)| theme)
                .unwrap_or_default()
        };
        self.export_with(path, &theme, Some(width));
    }
    pub fn preview_cut(mut self: Pin<&mut Self>, threshold: f64) {
        if !self.as_mut().workspace_available() {
            return;
        }
        let result = {
            let r = self.rust();
            r.time_tree
                .as_ref()
                .ok_or("Validate a rooted time tree first".into())
                .and_then(|(tree, time, _)| {
                    basecraft_delimitation::time_tree::cut(tree, time, threshold)
                        .map_err(|e| e.to_string())
                })
        };
        match result {
            Ok(groups) => {
                let json = serde_json::to_string(&groups).unwrap();
                self.as_mut().rust_mut().preview = groups;
                self.as_mut().set_threshold(threshold);
                self.as_mut().set_preview_json(QString::from(&json));
                self.update_bar_columns();
            }
            Err(e) => self.fail(&e),
        }
    }
    pub fn preview_position(self: Pin<&mut Self>, geometry: &QString, x: f64) {
        let value = serde_json::from_str::<Value>(&geometry.to_string()).ok();
        if let Some(value) = value
            && let Some((root, tips)) = value["root"][0].as_f64().zip(value["tips_x"].as_f64())
            && tips > root
        {
            let threshold = self.rust().root_height * ((tips - x) / (tips - root)).clamp(0.0, 1.0);
            self.preview_cut(threshold);
        }
    }
    pub fn cancel_cut(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().preview.clear();
        self.as_mut().set_preview_json(QString::from("[]"));
        self.update_bar_columns();
    }
    pub fn save_cut(mut self: Pin<&mut Self>, name: &QString) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        if !self.as_mut().workspace_available() {
            return;
        }
        let edit = {
            let r = self.rust();
            if r.preview.is_empty() {
                return;
            }
            let Some((tree, _, hash)) = r.time_tree.as_ref() else {
                self.fail("Attach a valid time tree before saving a cut");
                return;
            };
            let Some((session, doc)) = r.session.as_ref().zip(r.doc) else {
                self.fail("No species workspace is open");
                return;
            };
            let w = crate::bridge::checked_document!(access_thread, session.delimitation(doc));
            let mapping = delimitation::tree_mapping(
                w,
                &tree
                    .tips_in_order()
                    .iter()
                    .map(|&i| tree.node(i).name.clone())
                    .collect(),
            );
            let groups: Vec<_> = r
                .preview
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    group(
                        g.iter()
                            .flat_map(|name| mapping.get(name).into_iter().flatten().cloned())
                            .collect(),
                        (i + 1).to_string(),
                    )
                })
                .collect();
            let assigned: BTreeSet<_> = groups
                .iter()
                .flat_map(|g| g.members.iter().cloned())
                .collect();
            let h = Hypothesis {
                id: ulid::Ulid::generate().to_string(),
                name: name.to_string(),
                origin: HypothesisOrigin::ManualCut,
                parent: None,
                groups,
                unassigned: w
                    .specimens
                    .iter()
                    .map(|s| s.id.clone())
                    .filter(|id| !assigned.contains(id))
                    .collect(),
                scores: vec![],
                provenance: Provenance {
                    method: "manual_time_cut".into(),
                    analysis_tree: Some(hash.clone()),
                    parameters: [
                        ("tree_name".into(), json!(tree.name)),
                        ("rooted".into(), json!(true)),
                    ]
                    .into(),
                    ..Provenance::default()
                },
                attributes: BTreeMap::new(),
            };
            DelimitationEdit::SaveCut {
                hypothesis: h,
                threshold: r.threshold,
                boundary: "node_age_le_threshold".into(),
            }
        };
        let created = match &edit {
            DelimitationEdit::SaveCut { hypothesis, .. } => hypothesis.id.clone(),
            _ => return,
        };
        self.as_mut().set_group(true);
        if self.as_mut().edit(edit).is_ok() {
            self.as_mut().cancel_cut();
            self.as_mut().activate(&QString::from(&created));
            self.as_mut().show_created(created);
        }
        self.set_group(false);
    }
    /// Show a hypothesis just created, within the caller's undo group.
    fn show_created(mut self: Pin<&mut Self>, id: String) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let state = self.rust();
        if let Some((session, doc)) = state.session.as_ref().zip(state.doc) {
            let mut view =
                crate::bridge::checked_document!(access_thread, session.delimitation(doc))
                    .view
                    .clone();
            view.visible.push(id);
            let _ = self.edit(DelimitationEdit::SetView { view });
        }
    }
    fn set_group(mut self: Pin<&mut Self>, open: bool) {
        if let Some(session) = self.as_mut().rust_mut().session.as_mut() {
            if open {
                session.begin_group();
            } else {
                session.end_group();
            }
        }
    }
    fn history(mut self: Pin<&mut Self>, redo: bool) {
        if !self.as_mut().workspace_available() {
            return;
        }
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            r.session
                .as_mut()
                .zip(r.doc)
                .map(|(s, d)| if redo { s.redo(d) } else { s.undo(d) })
        };
        if let Some(Err(e)) = result {
            self.fail(&e.to_string());
        } else {
            self.as_mut().set_statistics_json(QString::from("{}"));
            self.as_mut().set_message(QString::default());
            self.as_mut().set_dirty(true);
            self.as_mut().cancel_cut();
            self.refresh();
        }
    }
    pub fn undo(self: Pin<&mut Self>) {
        self.history(false);
    }
    pub fn redo(self: Pin<&mut Self>) {
        self.history(true);
    }
    pub fn save(mut self: Pin<&mut Self>) {
        let path = self.rust().file_path.clone();
        self.as_mut().save_as(&path);
    }
    pub fn save_as(mut self: Pin<&mut Self>, path: &QString) -> bool {
        if !self.as_mut().workspace_available() {
            return false;
        }
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            // Saving to the opened path is an ordinary save, so a workspace
            // opened through a symbolic link updates its file, not the link.
            let target = (path != &r.file_path).then(|| PathBuf::from(path.to_string()));
            r.session
                .as_mut()
                .zip(r.doc)
                .map(|(s, d)| s.save(d, target.as_deref()))
        };
        match result {
            Some(Ok(())) => {
                if path != &self.rust().file_path {
                    let adopted = (|| -> basecraft_ops::Result<_> {
                        let target = PathBuf::from(path.to_string());
                        let mut session =
                            Session::recoverable(crate::bridge::project_root(&target))?;
                        let doc = session.open(&target)?;
                        Ok((session, doc))
                    })();
                    match adopted {
                        Ok((session, doc)) => {
                            let mut state = self.as_mut().rust_mut();
                            state.session = Some(session);
                            state.doc = Some(doc);
                        }
                        Err(error) => {
                            self.fail(&format!(
                                "Saved, but could not reopen the new workspace: {error}"
                            ));
                            return false;
                        }
                    }
                }
                self.as_mut().set_file_path(path.clone());
                self.as_mut().set_dirty(false);
                self.as_mut().set_message_kind(QString::from("ok"));
                self.set_message(QString::from("Saved"));
                true
            }
            Some(Err(e)) => {
                self.fail(&e.to_string());
                false
            }
            None => false,
        }
    }
    pub fn export_svg(self: Pin<&mut Self>, path: &QString) {
        let theme = crate::bridge::load_theme()
            .map(|(theme, _)| theme)
            .unwrap_or_default();
        self.export_with(path, &theme, None);
    }
    fn export_with(
        mut self: Pin<&mut Self>,
        path: &QString,
        theme: &basecraft_viewmodel::Theme,
        png_width: Option<f64>,
    ) {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (s, d) = r.session.as_mut().zip(r.doc).ok_or("No workspace open")?;
            // The tree as drawn on screen, with its bar columns.
            let mut tree = r
                .display_tree
                .clone()
                .ok_or("Attach a display tree first")?;
            let work = s.delimitation(d)?;
            basecraft_ops::session::apply_drawing_edits(&mut tree, &work.view.tree_edits);
            let font = crate::bridge::monospace_font();
            let columns =
                delimitation::tip_columns(work, &tip_names(&tree), &delimitation::palette(theme));
            let figure = basecraft_viewmodel::tree::figure_with_columns(
                &tree, &columns, 900.0, 20.0, theme, &font,
            );
            let svg = delimitation::comparison_svg(work, &figure, &font);
            if let Some(width) = png_width {
                let bytes = crate::figure_png::render(&svg, width)?;
                s.export(d, Path::new(&path.to_string()), "png", |w| {
                    w.write_all(&bytes)
                })?;
            } else {
                s.export(d, Path::new(&path.to_string()), "svg", |w| {
                    w.write_all(svg.as_bytes())
                })?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.as_mut().set_message_kind(QString::from("ok"));
                self.set_message(QString::from("Exported comparison"));
            }
            Err(e) => self.fail(&e.to_string()),
        }
    }
    pub fn export_analysis(mut self: Pin<&mut Self>, path: &QString) {
        let bytes = self.analysis_json().to_string().into_bytes();
        let result = (|| -> basecraft_ops::Result<()> {
            if bytes == b"{}" {
                return Err(basecraft_ops::Error::Invalid(
                    "Choose a fitted GMYC hypothesis".into(),
                ));
            }
            let mut state = self.as_mut().rust_mut();
            let state = &mut *state;
            let (session, doc) = state
                .session
                .as_mut()
                .zip(state.doc)
                .ok_or_else(|| basecraft_ops::Error::Invalid("No workspace open".into()))?;
            session.export(doc, Path::new(&path.to_string()), "gmyc_report", |writer| {
                writer.write_all(&bytes)
            })?;
            Ok(())
        })();
        match result {
            Ok(()) => self
                .as_mut()
                .set_message(QString::from("Exported GMYC report")),
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub fn export_statistics(mut self: Pin<&mut Self>, path: &QString) {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let value: Value = serde_json::from_str(&self.rust().statistics_json.to_string())?;
            let groups = value["groups"]
                .as_array()
                .ok_or("Run distance summaries first")?;
            let fields = [
                "group",
                "specimens",
                "mean_within",
                "max_within",
                "min_between",
                "nearest_groups",
                "barcode_gap",
                "observed_within",
                "missing_within",
                "observed_between",
                "missing_between",
            ];
            let mut csv = fields.join(",") + "\n";
            for group in groups {
                let row: Vec<_> = fields
                    .iter()
                    .map(|field| {
                        let value = &group[field];
                        let text = if value.is_null() {
                            String::new()
                        } else if let Some(values) = value.as_array() {
                            values
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("; ")
                        } else if let Some(text) = value.as_str() {
                            text.to_string()
                        } else {
                            value.to_string()
                        };
                        format!("\"{}\"", text.replace('"', "\"\""))
                    })
                    .collect();
                csv.push_str(&row.join(","));
                csv.push('\n');
            }
            let mut state = self.as_mut().rust_mut();
            let doc = state.doc.ok_or("No workspace open")?;
            let session = state.session.as_mut().ok_or("No workspace open")?;
            session.export(doc, Path::new(&path.to_string()), "csv", |writer| {
                writer.write_all(csv.as_bytes())
            })?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.as_mut().set_message_kind(QString::from("ok"));
                self.set_message(QString::from("Exported distance summaries"));
            }
            Err(error) => self.fail(&error.to_string()),
        }
    }
    pub fn run_analysis(
        mut self: Pin<&mut Self>,
        method: &QString,
        input: &QString,
        rooted: bool,
        settings: &QString,
    ) {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        if self.rust().busy {
            return;
        }
        let display_job = method.to_string().starts_with("display_");
        let display_rooted = method.to_string() == "display_upgma";
        let result = (|| -> Result<_, String> {
            let r = self.rust();
            let (session, doc) = r.session.as_ref().zip(r.doc).ok_or("No workspace open")?;
            let workspace = session.delimitation(doc).map_err(|e| e.to_string())?;
            let expected_alignment = if workspace
                .exchange_fields
                .get("basecraft_alignment_snapshot")
                .is_some_and(|p| p == &input.to_string())
            {
                Some(
                    workspace
                        .exchange_fields
                        .get("basecraft_alignment_hash")
                        .ok_or("Retained alignment hash is missing")?,
                )
            } else {
                None
            };
            let method = method.to_string();
            if ![
                "display_upgma",
                "display_nj",
                "display_iqtree",
                "gmyc_single",
                "gmyc_multiple",
                "gmyc_alignment_single",
                "gmyc_alignment_multiple",
                "ptp",
                "mptp",
                "asap",
                "relative_time",
                "haplotypes",
                "group_statistics",
            ]
            .contains(&method.as_str())
            {
                return Err("Unknown analysis".into());
            }
            let display_job = method.starts_with("display_");
            let workspace_path = session.root().join(session.doc_key(doc));
            let output = workspace_path
                .parent()
                .unwrap_or(session.root())
                .join(format!(
                    "analysis-{}.{}",
                    ulid::Ulid::generate(),
                    if display_job { "nwk" } else { "json" }
                ));
            let alignment_pipeline = method.starts_with("gmyc_alignment_") || method == "asap";
            let requested_input = PathBuf::from(input.to_string());
            let analysis_input = if expected_alignment.is_some() {
                basecraft_ops::delimitation::checked_alignment(workspace, &workspace_path)
                    .map_err(|e| e.to_string())?
                    .ok_or("No linked alignment available")?
            } else if !display_job
                && (workspace.view.display_tree.as_deref() == Some(input.to_string().as_str())
                    || r.display_path == *input)
            {
                basecraft_ops::delimitation::checked_display_tree(workspace)
                    .map_err(|e| e.to_string())?
                    .ok_or("No analysis tree available")?
            } else {
                requested_input
            };
            if let Some(expected) = expected_alignment {
                let actual = basecraft_io::hash_file(&analysis_input).map_err(|e| e.to_string())?;
                if &actual != expected {
                    return Err(
                        "The retained alignment changed; select an intact input explicitly".into(),
                    );
                }
            }
            let operation = if method == "display_iqtree" {
                bundled("iqtree", "tree")
            } else if display_job {
                bundled("distance", "tree")
            } else if alignment_pipeline {
                format!(
                    "basecraft.host/{}@1",
                    if method == "asap" {
                        "asap_alignment"
                    } else {
                        &method
                    }
                )
            } else {
                bundled("delimitation", &method)
            };
            let declared = declared_parameters(&operation)?;
            let mut params = serde_json::Map::new();
            params.insert("rooted".into(), json!(rooted));
            params.insert("workers".into(), json!(1));
            params.insert("seed".into(), json!(1));
            let settings: serde_json::Map<String, Value> =
                serde_json::from_str(&settings.to_string()).map_err(|e| e.to_string())?;
            for (key, value) in settings {
                if [
                    "mapping",
                    "workers",
                    "seed",
                    "replicates",
                    "model",
                    "reference_version",
                    "reference_age_clamp",
                    "unique_haplotypes",
                    "deduplicate",
                    "iqtree_model",
                    "tree_index",
                    "min_overlap",
                    "candidate_budget",
                ]
                .contains(&key.as_str())
                    && !value.as_str().is_some_and(str::is_empty)
                {
                    params.insert(key, value);
                }
            }
            if !alignment_pipeline
                && !display_job
                && params.get("deduplicate").and_then(Value::as_bool) == Some(true)
            {
                return Err("Exact sequence deduplication requires a verified alignment and representative tree. Tree-only preprocessing is unavailable; use alignment GMYC/ASAP, or prepare and root a representative tree with its mapping before fitting. No tips were removed.".into());
            }
            if alignment_pipeline {
                let unique = params
                    .get("deduplicate")
                    .or_else(|| params.get("unique_haplotypes"))
                    .and_then(Value::as_bool)
                    .unwrap_or(method.starts_with("gmyc_alignment_"));
                params.insert("unique_haplotypes".into(), json!(unique));
            }
            if display_job {
                params.insert(
                    "tree_role".into(),
                    json!("display only; does not change species assignments"),
                );
                params.insert(
                    "method".into(),
                    json!(if method == "display_upgma" {
                        "upgma"
                    } else {
                        "njstar"
                    }),
                );
            }
            if method == "display_iqtree" {
                let model = params.remove("iqtree_model").unwrap_or(json!("MFP"));
                params.insert("model".into(), model);
                params.insert("keep_identical".into(), json!(true));
            }
            if method == "group_statistics" {
                let h = session
                    .delimitation(doc)
                    .map_err(|e| e.to_string())?
                    .hypotheses
                    .iter()
                    .find(|h| h.id == r.active_hypothesis.to_string())
                    .ok_or("Choose a hypothesis")?;
                params.insert(
                    "groups_data".into(),
                    json!(
                        h.groups
                            .iter()
                            .flat_map(|g| g
                                .members
                                .iter()
                                .map(move |id| (id.clone(), g.id.clone())))
                            .collect::<BTreeMap<_, _>>()
                    ),
                );
            }
            // Only declared settings reach the plugin; the descriptor decides.
            params.retain(|key, _| declared.contains(key));
            let role = input_role(&operation, &analysis_input)?;
            let exports = if display_job {
                vec![(primary_member(&operation)?, output.clone())]
            } else {
                Vec::new()
            };
            Ok((
                operation,
                role,
                analysis_input,
                Value::Object(params),
                exports,
                output,
                session.generation(doc),
                (session.id().to_owned(), session.doc_key(doc).to_owned()),
            ))
        })();
        let (operation, role, analysis_input, params, exports, output, generation, identity) =
            match result {
                Ok(job) => job,
                Err(e) => {
                    self.as_mut().fail(&e);
                    if display_job {
                        self.display_tree_ready(false);
                    } else {
                        self.analysis_finished(false);
                    }
                    return;
                }
            };
        let submitted = {
            let mut state = self.as_mut().rust_mut();
            let state = &mut *state;
            match state.doc {
                Some(doc) => DocumentJob::submit_files(
                    &mut state.session,
                    doc,
                    &operation,
                    [(role, vec![analysis_input])].into(),
                    params,
                    &exports,
                ),
                None => Err("No workspace open".into()),
            }
        };
        let job = match submitted {
            Ok(job) => job,
            Err(e) => {
                self.as_mut().fail(&e);
                if display_job {
                    self.display_tree_ready(false);
                } else {
                    self.analysis_finished(false);
                }
                return;
            }
        };
        self.as_mut().rust_mut().analysis = Some(RunningAnalysis {
            job,
            display_job,
            display_rooted,
            display_output: output,
            generation,
            identity,
        });
        self.as_mut().set_busy(true);
        self.as_mut()
            .set_message(QString::from("Running analysis…"));
    }

    /// Called from the poll timer: records a finished run in the workspace's
    /// history, then adds its hypotheses or attaches its display tree.
    fn finish_analysis(mut self: Pin<&mut Self>) {
        let polled = {
            let mut state = self.as_mut().rust_mut();
            let state = &mut *state;
            let Some(analysis) = &state.analysis else {
                return;
            };
            analysis.job.poll(&mut state.session)
        };
        let outcome = match polled {
            Ok(Some(outcome)) => outcome,
            Ok(None) => return,
            Err(error) => {
                let display_job = self
                    .as_mut()
                    .rust_mut()
                    .analysis
                    .take()
                    .is_some_and(|analysis| analysis.display_job);
                self.as_mut().set_busy(false);
                self.as_mut().fail(&error);
                if display_job {
                    self.display_tree_ready(false);
                } else {
                    self.analysis_finished(false);
                }
                return;
            }
        };
        let Some(analysis) = self.as_mut().rust_mut().analysis.take() else {
            return;
        };
        let display_job = analysis.display_job;
        let applied = (|| -> Result<(Value, Vec<String>), String> {
            if outcome.state != basecraft_protocol::operation::RuntimeState::Completed {
                return Err(failure(&outcome));
            }
            let mut state = self.as_mut().rust_mut();
            let state = &mut *state;
            let (session, doc) = state
                .session
                .as_mut()
                .zip(state.doc)
                .ok_or("Workspace closed")?;
            if session.id() != analysis.identity.0 || session.doc_key(doc) != analysis.identity.1 {
                return Err("Analysis completed for its original workspace; its result remains available for review".into());
            }
            if session.generation(doc)
                != analysis
                    .job
                    .finalized_generation()
                    .unwrap_or(analysis.generation)
            {
                return Err("Analysis completed for its saved input; workspace changed, so result remains an artifact".into());
            }
            if display_job {
                return Ok((json!({"display_tree":analysis.display_output}), Vec::new()));
            }
            let name = if outcome.operation.starts_with("basecraft.host/") {
                "fit.json"
            } else {
                "result.json"
            };
            let result = result_file(&outcome, name).ok_or("Analysis returned no result")?;
            // The session already added its hypotheses when it recorded the run.
            let value: Value = std::fs::read(&result)
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))?;
            let relative = outcome
                .artifacts
                .iter()
                .filter(|artifact| artifact.path.ends_with(".relative.nwk"))
                .map(|artifact| artifact.path.clone())
                .collect();
            Ok((value, relative))
        })();
        self.as_mut().set_busy(false);
        match applied {
            Ok((value, relative)) => {
                if display_job {
                    if let Some(path) = value["display_tree"].as_str() {
                        let success = self
                            .as_mut()
                            .attach_tree(&QString::from(path), analysis.display_rooted);
                        self.as_mut().display_tree_ready(success);
                    }
                    return;
                }
                if let Some(hypotheses) = value["workspace"]["hypotheses"].as_array() {
                    if let Some(first) = hypotheses.first().and_then(|h| h["id"].as_str()) {
                        self.as_mut().activate(&QString::from(first));
                    }
                    let inferred_alignment = hypotheses.first().is_some_and(|h| {
                        h["provenance"]["parameters"]
                            .get("tree_inference")
                            .is_some()
                    });
                    if (self.rust().display_path.to_string().is_empty() || inferred_alignment)
                        && let Some(path) = value["workspace"]["view"]["display_tree"].as_str()
                    {
                        self.as_mut().attach_tree(
                            &QString::from(path),
                            value["metrics"].get("gmyc").is_some(),
                        );
                    }
                }
                if let Some(path) = relative.first() {
                    self.as_mut().attach_tree(&QString::from(path), true);
                }
                if value.get("groups").is_some() {
                    self.as_mut()
                        .set_statistics_json(QString::from(value.to_string()));
                } else {
                    self.as_mut().set_dirty(true);
                }
                if let Some(counts) = value["metrics"]["haplotypes"].as_object() {
                    let original = counts
                        .get("original_count")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let unique = counts
                        .get("unique")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    self.as_mut().set_sampling_summary(QString::from(format!(
                        "{original} specimens · {unique} exact haplotypes"
                    )));
                } else if let Some(hypothesis) = value["workspace"]["hypotheses"]
                    .as_array()
                    .and_then(|h| h.first())
                {
                    let counts = &hypothesis["provenance"]["parameters"];
                    if let Some(original) = counts["original_count"].as_u64() {
                        self.as_mut().set_sampling_summary(QString::from(format!(
                            "{original} specimens · {} retained tree tips",
                            counts["retained_count"]
                        )));
                    }
                }
                self.as_mut().set_message_kind(QString::from("ok"));
                self.as_mut()
                    .set_message(QString::from("Analysis completed"));
                self.as_mut().refresh();
                self.analysis_finished(true);
            }
            Err(error) => {
                self.as_mut().fail(&error);
                if display_job {
                    self.display_tree_ready(false);
                } else {
                    self.analysis_finished(false);
                }
            }
        }
    }
    /// Remove a generated workspace whose first analysis produced nothing, so a
    /// failed run does not leave an empty SPART beside the user's data. Run
    /// evidence stays in the hidden store. Anything with hypotheses, unsaved
    /// edits or a user-chosen name is kept.
    pub fn discard_empty_workspace(mut self: Pin<&mut Self>) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let path = {
            let r = self.rust();
            let Some((session, doc)) = r.session.as_ref().zip(r.doc) else {
                return false;
            };
            if r.dirty
                || r.busy
                || !crate::bridge::checked_document!(access_thread, session.delimitation(doc))
                    .hypotheses
                    .is_empty()
            {
                return false;
            }
            session.root().join(session.doc_key(doc))
        };
        let generated = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(generated_workspace);
        if !generated || std::fs::remove_file(&path).is_err() {
            return false;
        }
        let mut r = self.as_mut().rust_mut();
        r.session = None;
        r.doc = None;
        true
    }
    pub fn cancel_job(self: Pin<&mut Self>) {
        if let Some(analysis) = &self.rust().analysis {
            analysis.job.cancel();
        }
    }
    pub fn poll_job(mut self: Pin<&mut Self>) {
        let progress = self.rust().analysis.as_ref().and_then(|analysis| {
            let path = analysis.job.progress_file()?;
            std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .map(|v| {
                    format!(
                        "{} · {} / {}",
                        v["phase"].as_str().unwrap_or("Running"),
                        v["completed"],
                        v["total"]
                    )
                })
        });
        if let Some(text) = progress {
            self.as_mut().set_message(QString::from(&text));
        }
        self.finish_analysis();
    }
    pub fn flush(mut self: Pin<&mut Self>) {
        let result = self
            .as_mut()
            .rust_mut()
            .session
            .as_mut()
            .map(Session::flush);
        if let Some(Err(e)) = result {
            self.fail(&e.to_string());
        }
    }
    pub fn check_theme(mut self: Pin<&mut Self>) {
        let stamp = crate::bridge::theme_stamp();
        if self.rust().theme_stamp == stamp {
            return;
        }
        let theme = crate::bridge::load_theme()
            .map(|(theme, _)| theme)
            .unwrap_or_default();
        let palette = delimitation::palette(&theme);
        let changed = palette != self.rust().palette;
        {
            let mut r = self.as_mut().rust_mut();
            r.theme_stamp = stamp;
            r.palette = palette;
        }
        if changed {
            let next = self.rust().theme_revision + 1;
            self.as_mut().set_theme_revision(next);
            self.update_bar_columns();
        }
    }
    pub fn memory_report(&self) -> QString {
        QString::from(&crate::bridge::memory_text())
    }
}

const WORKSPACE_TAGS: [&str; 5] = ["gmyc", "asap", "ptp", "manual", "species"];

fn workspace_tag(method: &str) -> &'static str {
    match method {
        m if m.starts_with("gmyc") => "gmyc",
        "asap" => "asap",
        "ptp" | "mptp" => "ptp",
        "manual" => "manual",
        _ => "species",
    }
}

/// `specimens.afa` and ASAP give `specimens.afa.asap01.spart`, numbered past
/// any existing file.
fn workspace_path(source: &Path, method: &str) -> PathBuf {
    let directory = source.parent().unwrap_or(Path::new("."));
    let base = source
        .file_name()
        .map_or_else(|| "species".into(), |n| n.to_string_lossy().into_owned());
    let tag = workspace_tag(method);
    (1..=u32::MAX)
        .map(|n| directory.join(format!("{base}.{tag}{n:02}.spart")))
        .find(|path| !path.exists())
        .unwrap_or_default()
}

/// Names `create_workspace` gives, including the earlier `species-<id>.spart`.
fn generated_workspace(name: &str) -> bool {
    let Some(stem) = name
        .len()
        .checked_sub(6)
        .filter(|&i| name.is_char_boundary(i) && name[i..].eq_ignore_ascii_case(".spart"))
        .map(|i| &name[..i])
    else {
        return false;
    };
    if stem.starts_with("species-") {
        return true;
    }
    stem.rsplit_once('.').is_some_and(|(_, last)| {
        let digits = last.trim_start_matches(|c: char| c.is_ascii_lowercase());
        let tag = &last[..last.len() - digits.len()];
        digits.len() >= 2
            && digits.bytes().all(|b| b.is_ascii_digit())
            && WORKSPACE_TAGS.contains(&tag)
    })
}

/// Entry actions run the same registered operation as the CLI, Python and MCP.
pub(crate) fn create_workspace_in_session(
    session: &mut Session,
    doc: DocId,
    method: &str,
) -> Result<String, String> {
    let source_path = session.root().join(session.doc_key(doc));
    let output = workspace_path(&source_path, method);
    let operation = bundled(
        "delimitation",
        if session.is_tree(doc) {
            "workspace_tree"
        } else {
            "workspace_alignment"
        },
    );
    let job = crate::document_jobs::DocumentJob::submit(
        session,
        doc,
        &operation,
        json!({}),
        &[("workspace".into(), output.clone())],
    )?;
    let outcome = loop {
        if let Some(outcome) = job.poll(session)? {
            break outcome;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    if outcome.state != basecraft_protocol::operation::RuntimeState::Completed {
        return Err(failure(&outcome));
    }
    Ok(output.display().to_string())
}

fn tip_names(tree: &TreeDoc) -> BTreeSet<String> {
    tree.tips_in_order()
        .iter()
        .map(|&i| tree.node(i).name.clone())
        .collect()
}

/// The workspace with a manual cut's preview as an extra, last column.
fn with_preview(
    work: &basecraft_protocol::delimitation::DelimitationWorkspace,
    tree: &TreeDoc,
    preview: &[BTreeSet<String>],
) -> basecraft_protocol::delimitation::DelimitationWorkspace {
    let mut work = work.clone();
    if preview.is_empty() {
        return work;
    }
    let mapping = delimitation::tree_mapping(&work, &tip_names(tree));
    let mut h = Hypothesis {
        id: "cut-preview".into(),
        name: "Cut preview".into(),
        origin: HypothesisOrigin::ManualCut,
        parent: None,
        groups: vec![],
        unassigned: BTreeSet::new(),
        scores: vec![],
        provenance: Provenance::default(),
        attributes: BTreeMap::new(),
    };
    for (i, members) in preview.iter().enumerate() {
        h.groups.push(group(
            members
                .iter()
                .flat_map(|id| mapping.get(id).into_iter().flatten().cloned())
                .collect(),
            format!("Preview {}", i + 1),
        ));
    }
    work.hypotheses.push(h);
    work.view.visible.push("cut-preview".into());
    work
}

/// Use the shared typed edit/save path, retaining source identity and rooting
/// provenance. Reserve a whole output directory atomically: neither a previous
/// result nor an unrelated tree, sidecar or provenance bundle can be replaced.
fn root_tree_copy(
    session: &mut Session,
    display: &std::path::Path,
    method: &str,
) -> Result<PathBuf, String> {
    let op = match method {
        "midpoint" => Op::TreeMidpointRoot {},
        "mrttv" => Op::TreeMinimumVarianceRoot {},
        other => return Err(format!("Unknown rooting {other:?}")),
    };
    let doc = session.open(display).map_err(|e| e.to_string())?;
    session.apply(doc, op).map_err(|e| e.to_string())?;
    let prefix = format!(
        "{}.{}-rooted-",
        display.file_stem().unwrap_or_default().to_string_lossy(),
        method
    );
    let output = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir_in(display.parent().unwrap_or(std::path::Path::new(".")))
        .map_err(|e| e.to_string())?;
    let target = output.path().join("tree.nwk");
    session
        .save(doc, Some(&target))
        .map_err(|e| e.to_string())?;
    // Keep only complete saves; a failed save cannot be fitted or attached.
    let _ = output.keep();
    Ok(target)
}

crate::find_bridge::controller_find!(qobject::DelimitationController, delimitation, |d| {
    basecraft_viewmodel::find::Input::Names(
        d.specimens
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.label.clone()))
            .collect(),
    )
});

impl qobject::DelimitationController {
    fn find_reveal(mut self: std::pin::Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.delimitation(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        let (Some(session), Some(doc), Some(hit)) = (&r.session, r.doc, r.find.hit()) else {
            return;
        };
        let d = crate::bridge::checked_document!(access_thread, session.delimitation(doc));
        let _ = (d, hit);
    }
    fn find_y(&self) -> f32 {
        0.0
    }
}

#[cfg(test)]
mod naming_tests {
    use super::*;
    fn rooting_session(root: &Path) -> Session {
        let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
            (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
        })
        .unwrap();
        Session::recoverable_with_paths(root, &paths).unwrap()
    }

    #[test]
    fn rooted_copies_preserve_source_and_prior_results_with_provenance() {
        for (method, operation, expected_depths) in [
            ("midpoint", "tree.midpoint_root", [2.5, 4.5, 4.5]),
            ("mrttv", "tree.minimum_variance_root", [3.0, 5.0, 4.0]),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("supplied.nwk");
            let original = b"(A:1,B:3,C:6);\n";
            std::fs::write(&source, original).unwrap();
            // The previous predictable filename may belong to the user.
            let unrelated = source.with_extension(format!("{method}-rooted.nwk"));
            std::fs::write(&unrelated, "unrelated tree").unwrap();
            let mut session = rooting_session(dir.path());
            let first = root_tree_copy(&mut session, &source, method).unwrap();
            let first_bytes = std::fs::read(&first).unwrap();
            let sidecar = basecraft_ops::files::tree_sidecar(&first);
            let sidecar_bytes = std::fs::read(&sidecar).unwrap();
            let mut session = rooting_session(dir.path());
            let second = root_tree_copy(&mut session, &source, method).unwrap();
            assert_ne!(first, second);
            assert_eq!(std::fs::read(&source).unwrap(), original);
            assert!(!basecraft_ops::files::tree_sidecar(&source).exists());
            assert_eq!(
                std::fs::read_to_string(unrelated).unwrap(),
                "unrelated tree"
            );
            assert_eq!(std::fs::read(&first).unwrap(), first_bytes);
            assert_eq!(std::fs::read(&sidecar).unwrap(), sidecar_bytes);
            assert_eq!(std::fs::read(&second).unwrap(), first_bytes);
            let tree = basecraft_ops::files::load_tree(&second).unwrap().doc;
            for (name, expected) in ["A", "B", "C"].into_iter().zip(expected_depths) {
                let mut node = tree
                    .tips_in_order()
                    .into_iter()
                    .find(|&i| tree.node(i).name == name)
                    .unwrap();
                let mut depth = 0.0;
                while let Some(parent) = tree.node(node).parent {
                    depth += tree.node(node).length.unwrap();
                    node = parent;
                }
                assert!(
                    (depth - expected).abs() < 1e-9,
                    "{method}: {name} depth {depth}"
                );
            }
            for result in [&first, &second] {
                let publication = basecraft_ops::provenance::inspect(result).unwrap();
                assert!(
                    publication
                        .inputs
                        .iter()
                        .any(|input| input.hash == basecraft_io::hash_bytes(original))
                );
                assert!(
                    publication
                        .effective_edits
                        .iter()
                        .any(|edit| edit.operation.name() == operation)
                );
                assert_eq!(
                    basecraft_ops::provenance::verify(result).unwrap(),
                    Vec::<String>::new()
                );
            }
            // A supplied tree can itself have ancestry (including IQ-TREE's
            // inference). Save As must carry its contributing publication.
            let parent = basecraft_ops::provenance::inspect(&first).unwrap();
            let mut session = rooting_session(dir.path());
            let descendant = root_tree_copy(&mut session, &first, method).unwrap();
            let publication = basecraft_ops::provenance::inspect(&descendant).unwrap();
            assert!(publication.parents.contains(&parent.id));
            assert_eq!(
                basecraft_ops::provenance::verify(&descendant).unwrap(),
                Vec::<String>::new()
            );
        }
    }

    #[test]
    fn rooting_errors_publish_no_tree_and_preserve_source() {
        for method in ["midpoint", "mrttv"] {
            for text in [None, Some("not a tree"), Some("(A:1,(B:10,C:1)90:1);")] {
                let dir = tempfile::tempdir().unwrap();
                let source = dir.path().join("input.nwk");
                if let Some(text) = text {
                    std::fs::write(&source, text).unwrap();
                }
                let mut session = rooting_session(dir.path());
                assert!(root_tree_copy(&mut session, &source, method).is_err());
                if let Some(text) = text {
                    assert_eq!(std::fs::read_to_string(&source).unwrap(), text);
                }
                assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .contains("-rooted-")
                }));
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rooting_output_write_failure_returns_error_without_replacing_input() {
        let dir = tempfile::tempdir().unwrap();
        // Valid source name, but the output prefix exceeds Linux NAME_MAX.
        // This gives a deterministic write failure even under a privileged test user.
        let source = dir.path().join(format!("{}.nwk", "x".repeat(240)));
        let original = "(A:1,B:3,C:6);";
        std::fs::write(&source, original).unwrap();
        let mut session = rooting_session(dir.path());
        assert!(root_tree_copy(&mut session, &source, "mrttv").is_err());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
        assert!(!basecraft_ops::files::tree_sidecar(&source).exists());
    }

    #[test]
    fn workspaces_are_named_after_source_and_method() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("specimens.afa");
        let first = workspace_path(&source, "gmyc_alignment_single");
        assert_eq!(first.file_name().unwrap(), "specimens.afa.gmyc01.spart");
        std::fs::write(&first, "").unwrap();
        let second = workspace_path(&source, "gmyc_single");
        assert_eq!(second.file_name().unwrap(), "specimens.afa.gmyc02.spart");
        assert_eq!(
            workspace_path(&source, "mptp").file_name().unwrap(),
            "specimens.afa.ptp01.spart"
        );
        for name in [
            "specimens.afa.gmyc01.spart",
            "x.asap12.SPART",
            "species-01ABC.spart",
        ] {
            assert!(generated_workspace(name), "{name}");
        }
        let combined = |names: &[&str]| {
            basecraft_ops::delimitation::combined_path(
                &names
                    .iter()
                    .map(|n| directory.path().join(n))
                    .collect::<Vec<_>>(),
            )
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
        };
        assert_eq!(
            combined(&["specimens.afa.asap01.spart", "specimens.afa.gmyc01.spart"]),
            "specimens.afa.combined01.spart"
        );
        assert_eq!(
            combined(&["x.asap01.spart", "x.asap02.spart", "x.spart"]),
            "x.combined01.spart"
        );
        assert_eq!(
            combined(&["hypotheses.spart", "specimens.afa.gmyc01.spart"]),
            "combined01.spart"
        );
        std::fs::write(directory.path().join("combined01.spart"), "").unwrap();
        assert_eq!(combined(&["a.spart", "b.spart"]), "combined02.spart");
        for name in [
            "hypotheses.spart",
            "x.gmyc1.spart",
            "x.trees01.spart",
            "x.asap01.txt",
        ] {
            assert!(!generated_workspace(name), "{name}");
        }
    }
}

#[cfg(test)]
#[allow(dead_code)] // Called by the real application_bootstrap controller harness.
pub(crate) mod inspection_tests {
    use super::*;

    fn state(controller: &qobject::DelimitationController, original: DocId) -> Value {
        let r = controller.rust();
        let session = r.session.as_ref().unwrap();
        json!({
            "doc": r.doc.map(|d| d.0), "session": session.id(),
            "key": session.doc_key(original), "revision": session.revision(original),
            "record": session.document_record(original).unwrap(),
            "undo": session.can_undo(original), "redo": session.can_redo(original),
            "workspace": r.workspace_json.to_string(), "path": r.file_path.to_string(),
            "linked": r.linked_alignment.to_string(), "active": r.active_hypothesis.to_string(),
            "selection": r.selection.specimens, "origin": r.selection.origin,
            "selection_json": r.selection_json.to_string(), "preview": r.preview,
            "display": r.display_path.to_string(), "tree_edits": r.tree_edits.to_string(),
            "find": crate::find_bridge::status(&r.find, 0.0),
            "find_revision": r.find.revision(), "find_status_revision": r.find_status_revision,
            "find_pending": r.find.busy(), "busy": r.busy, "dirty": r.dirty,
            "analysis_pending": r.analysis.is_some(),
        })
    }

    pub(crate) fn run(mut controller: Pin<&mut qobject::DelimitationController>) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("linked.spart");
        let plain = folder.path().join("plain.spart");
        let source = include_str!("../../../testdata/fixtures/delimitation/manual.spart");
        for path in [&path, &plain] {
            std::fs::write(path, source).unwrap();
        }
        let mut work = basecraft_ops::files::load_delimitation(&path).unwrap();
        work.exchange_fields.insert(
            "basecraft_alignment_snapshot".into(),
            "Missing/Original.FA".into(),
        );
        basecraft_ops::files::write_delimitation_sidecar(
            &work,
            &basecraft_io::hash_file(&path).unwrap(),
            &mut std::fs::File::create(basecraft_ops::files::tree_sidecar(&path)).unwrap(),
        )
        .unwrap();
        let linked = QString::from(path.to_str().unwrap());
        let plain = QString::from(plain.to_str().unwrap());
        assert!(controller.as_mut().open_file(&linked));
        assert_eq!(
            controller.rust().linked_alignment.to_string(),
            "Missing/Original.FA"
        );
        assert!(controller.as_mut().open_file(&plain));
        assert!(controller.rust().linked_alignment.is_empty());
        assert!(controller.as_mut().open_file(&linked));
        let original = controller.rust().doc.unwrap();
        let mut view = work.view;
        view.visible.reverse();
        controller
            .as_mut()
            .edit(DelimitationEdit::SetView { view })
            .unwrap();
        controller
            .as_mut()
            .rust_mut()
            .selection
            .specimens
            .insert("a".into());
        controller
            .as_mut()
            .rust_mut()
            .find
            .set_query("a".into(), false);
        let wrong_path = folder.path().join("wrong.fa");
        std::fs::write(&wrong_path, ">a\nACGT\n").unwrap();
        let wrong = controller
            .as_mut()
            .rust_mut()
            .session
            .as_mut()
            .unwrap()
            .open(&wrong_path)
            .unwrap();
        for invalid in [wrong, DocId(usize::MAX)] {
            controller.as_mut().rust_mut().doc = Some(invalid);
            let before = state(&controller, original);
            assert!(!controller.as_mut().open_file(&plain));
            assert_eq!(state(&controller, original), before);
        }
        controller.as_mut().rust_mut().doc = Some(original);
        for busy in [false, true] {
            controller.as_mut().rust_mut().busy = busy;
            let before = state(&controller, original);
            assert!(
                !controller
                    .as_mut()
                    .open_file(&QString::from(wrong_path.to_str().unwrap()))
            );
            assert_eq!(state(&controller, original), before);
            assert!(!controller.as_mut().open_file(&QString::from(
                folder.path().join("missing.spart").to_str().unwrap()
            )));
            assert_eq!(state(&controller, original), before);
        }
        controller.as_mut().rust_mut().busy = false;
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
        println!("Delimitation real-controller linked hint/reopen/refusal passed");
    }
}
