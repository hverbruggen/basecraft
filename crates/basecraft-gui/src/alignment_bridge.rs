//! The Rust object QML talks to for one alignment editor.
//!
//! As with the genome editor, QML forwards raw input; the view model turns
//! it into viewport changes, selections and operations, and every edit goes
//! through the ops session.

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for AlignmentController {}
    impl cxx_qt::Initialize for AlignmentController {}

    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, overview_content)]
        #[qproperty(QString, overview_matches)]
        #[qproperty(QByteArray, frame)]
        #[qproperty(QString, status)]
        #[qproperty(QString, find_status)]
        #[qproperty(i32, find_count)]
        #[qproperty(i32, find_row)]
        #[qproperty(f64, find_hit_y)]
        #[qproperty(bool, find_nucleotide)]
        #[qproperty(QString, find_rows)]
        #[qproperty(i32, find_reveals)]
        #[qproperty(QString, history_revision)]
        #[qproperty(QString, message)]
        /// "ok", "error", or "" for a neutral hint.
        #[qproperty(QString, message_kind)]
        #[qproperty(QString, ok_color)]
        #[qproperty(QString, error_color)]
        #[qproperty(QString, background)]
        #[qproperty(QString, foreground)]
        #[qproperty(QString, accent)]
        #[qproperty(QString, mono_font)]
        #[qproperty(QString, file_path)]
        #[qproperty(bool, dirty)]
        /// The copy offer for an edit a read-only folder refused (D13), or "".
        #[qproperty(QString, refused_edit)]
        /// The offer to number names several entries share (D29), or "".
        #[qproperty(QString, duplicate_names)]
        /// Asks what the document is when basecraft cannot tell (D12), or "".
        #[qproperty(QString, alphabet_question)]
        #[qproperty(QString, zoom_text)]
        #[qproperty(f64, hscroll_pos)]
        #[qproperty(f64, hscroll_size)]
        #[qproperty(f64, vscroll_pos)]
        #[qproperty(f64, vscroll_size)]
        /// Width of the row names column, in pixels.
        #[qproperty(f64, names_width)]
        /// Typing inserts residues rather than replacing them.
        #[qproperty(bool, insert_mode)]
        /// A tool is running on this alignment.
        #[qproperty(bool, busy)]
        #[qproperty(bool, bootstrap_running)]
        #[qproperty(bool, bootstrap_cancelling)]
        #[qproperty(f64, bootstrap_progress)]
        #[qproperty(QString, bootstrap_status)]
        #[qproperty(f64, gap_column_count)]
        type AlignmentController = super::AlignmentRust;

        /// A tool wrote a new file, to open in a tab.
        #[qsignal]
        #[cxx_name = "fileCreated"]
        fn file_created(self: Pin<&mut AlignmentController>, path: QString);

        /// End the closed tab's session now (Main.qml `removeDocument`).
        #[qinvokable]
        fn close_session(self: Pin<&mut AlignmentController>);
        /// Number the names the duplicate-name offer named; whether it worked.
        #[qinvokable]
        fn number_duplicates(self: Pin<&mut AlignmentController>) -> bool;
        /// Where records of work on this document go (its session's folder).
        #[qinvokable]
        fn record_folder(self: &AlignmentController) -> QString;
        #[qinvokable]
        fn find_query(self: Pin<&mut AlignmentController>, query: &QString, reverse: bool);
        #[qinvokable]
        fn find_step(self: Pin<&mut AlignmentController>, direction: i32);

        #[qinvokable]
        fn document_record(self: &AlignmentController) -> QString;
        #[qinvokable]
        fn record_revision(self: &AlignmentController) -> QString;
        #[qinvokable]
        fn accessible_page(self: &AlignmentController, page: i32) -> QString;
        #[qinvokable]
        fn add_sequences(
            self: Pin<&mut AlignmentController>,
            text: &QString,
            name: &QString,
            mafft: bool,
            orientation: bool,
            keep: bool,
        );
        #[qinvokable]
        fn add_reviewed_sequences(
            self: Pin<&mut AlignmentController>,
            text: &QString,
            name: &QString,
            mafft: bool,
            orientation: bool,
            keep: bool,
            evidence: &QString,
        );
        #[qinvokable]
        fn read_sequence_file(self: Pin<&mut AlignmentController>, path: &QString) -> QString;
        #[qinvokable]
        fn review_snapshot(self: Pin<&mut AlignmentController>) -> QString;
        #[qsignal]
        #[cxx_name = "additionNeedsGaps"]
        fn addition_needs_gaps(self: Pin<&mut AlignmentController>, count: i32);
        #[qsignal]
        #[cxx_name = "sequencesAdded"]
        fn sequences_added(self: Pin<&mut AlignmentController>);
        #[qsignal]
        #[cxx_name = "additionFailed"]
        fn addition_failed(self: Pin<&mut AlignmentController>, message: QString);

        /// Realign the rows with MAFFT's default settings, in the
        /// background, into a new file beside this one.
        #[qinvokable]
        fn align_mafft(self: Pin<&mut AlignmentController>);

        #[qinvokable]
        fn trim_alignment(self: Pin<&mut AlignmentController>);
        /// A plugin's settings form, as JSON, or `{"error": …}`.
        #[qinvokable]
        fn tool_form(self: &AlignmentController, plugin: &QString, form: &QString) -> QString;
        /// Run a plugin with the values of its settings form (JSON) in the
        /// background, into a new file beside this one. Missing values take
        /// the form's defaults.
        #[qinvokable]
        fn run_form(
            self: Pin<&mut AlignmentController>,
            plugin: &QString,
            form: &QString,
            values: &QString,
        );
        #[qinvokable]
        fn cancel_bootstrap(self: Pin<&mut AlignmentController>);

        #[qinvokable]
        fn create_delimitation(self: Pin<&mut AlignmentController>, method: &QString) -> bool;
        #[qsignal]
        #[cxx_name = "workspaceCreated"]
        fn workspace_created(self: Pin<&mut AlignmentController>, path: QString);
        #[qinvokable]
        fn open_file(self: Pin<&mut AlignmentController>, path: &QString) -> bool;
        #[qsignal]
        fn opened(self: Pin<&mut AlignmentController>, success: bool);
        #[qinvokable]
        fn highlight_taxa(self: Pin<&mut AlignmentController>, names: &QString);
        #[qsignal]
        #[cxx_name = "taxaSelected"]
        fn taxa_selected(self: Pin<&mut AlignmentController>, names: QString);
        #[qinvokable]
        fn export_svg(self: Pin<&mut AlignmentController>, path: &QString);
        #[qinvokable]
        fn export_figure(
            self: Pin<&mut AlignmentController>,
            path: &QString,
            scope: i32,
            publication: bool,
        );
        #[qinvokable]
        fn export_figure_png(
            self: Pin<&mut AlignmentController>,
            path: &QString,
            scope: i32,
            publication: bool,
            width: f64,
        );
        #[qinvokable]
        fn export_figure_selection(
            self: Pin<&mut AlignmentController>,
            path: &QString,
            scope: i32,
            publication: bool,
            include_selection: bool,
        );
        #[qinvokable]
        fn export_figure_png_selection(
            self: Pin<&mut AlignmentController>,
            path: &QString,
            scope: i32,
            publication: bool,
            width: f64,
            include_selection: bool,
        );
        #[qinvokable]
        fn resize(self: Pin<&mut AlignmentController>, width: f64, height: f64);
        #[qinvokable]
        fn wheel(
            self: Pin<&mut AlignmentController>,
            x: f64,
            y: f64,
            dy: f64,
            dx: f64,
            modifiers: i32,
        );
        #[qinvokable]
        fn press(self: Pin<&mut AlignmentController>, x: f64, y: f64, modifiers: i32);
        #[qinvokable]
        fn drag(self: Pin<&mut AlignmentController>, x: f64, y: f64);
        #[qinvokable]
        fn release(self: Pin<&mut AlignmentController>);
        /// Start panning (middle button).
        #[qinvokable]
        fn pan_press(self: Pin<&mut AlignmentController>, x: f64, y: f64);
        /// Called repeatedly while a selection drag is near or past an edge;
        /// returns whether the view moved.
        #[qinvokable]
        fn edge_scroll(self: Pin<&mut AlignmentController>, x: f64, y: f64) -> bool;
        /// Returns whether the key was handled.
        #[qinvokable]
        fn key(
            self: Pin<&mut AlignmentController>,
            key: i32,
            text: &QString,
            modifiers: i32,
        ) -> bool;
        #[qinvokable]
        fn double_click(self: Pin<&mut AlignmentController>, x: f64, y: f64);
        /// Zoom by `factor` around a point (pinch gestures).
        #[qinvokable]
        fn zoom_by(self: Pin<&mut AlignmentController>, x: f64, y: f64, factor: f64);
        #[qinvokable]
        fn zoom_in(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn set_zoom_percent(self: Pin<&mut AlignmentController>, percent: f64);
        #[qinvokable]
        fn zoom_out(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn fit(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn zoom_to_selection(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn hscroll_to(self: Pin<&mut AlignmentController>, fraction: f64);
        #[qinvokable]
        fn vscroll_to(self: Pin<&mut AlignmentController>, fraction: f64);
        #[qinvokable]
        fn undo(self: Pin<&mut AlignmentController>);
        /// State what the document is: "dna", "rna" or "protein" (D12).
        #[qinvokable]
        fn state_alphabet(self: Pin<&mut AlignmentController>, answer: &QString) -> bool;
        #[qinvokable]
        fn redo(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn save(self: Pin<&mut AlignmentController>);
        /// Save, or the question to answer first (editor_save.rs).
        #[qinvokable]
        fn request_save(self: Pin<&mut AlignmentController>, path: &QString) -> QString;
        #[qinvokable]
        fn check_save(self: &AlignmentController, path: &QString) -> QString;
        #[qinvokable]
        fn save_confirmed(
            self: Pin<&mut AlignmentController>,
            path: &QString,
            token: &QString,
            policy: &QString,
            acknowledge: bool,
        ) -> bool;
        /// Save to a new file and carry on editing that one.
        #[qinvokable]
        fn save_as(self: Pin<&mut AlignmentController>, path: &QString) -> bool;
        /// Reload the Omarchy theme if it changed.
        #[qinvokable]
        fn memory_report(self: &AlignmentController) -> QString;

        /// Right-click at a point. Returns JSON for the context menu:
        /// `{"region": "grid" | "names" | "header" | "corner", "rows",
        /// "columns", "rowName"}`.
        #[qinvokable]
        fn context_press(self: Pin<&mut AlignmentController>, x: f64, y: f64) -> QString;
        /// What's under the pointer, or an empty string.
        #[qinvokable]
        fn hover(self: &AlignmentController, x: f64, y: f64) -> QString;
        /// The selected block as FASTA, for the clipboard.
        #[qinvokable]
        fn selection_text(self: &AlignmentController) -> QString;
        #[qinvokable]
        fn blast_queries(self: &AlignmentController, source: &QString, feature: i32) -> QString;
        #[qinvokable]
        fn select_all(self: Pin<&mut AlignmentController>);
        /// Select rows `r0..r1` by columns `c0..c1`.
        #[qinvokable]
        fn select_block(self: Pin<&mut AlignmentController>, r0: i32, c0: i32, r1: i32, c1: i32);

        // Edits on the selection (or the cursor's cell).
        #[qinvokable]
        fn insert_gap(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn remove_gap(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn delete_selection(self: Pin<&mut AlignmentController>);
        /// Slide the selection `by` columns, one column at a time, as far as
        /// the gaps allow.
        #[qinvokable]
        fn slide(self: Pin<&mut AlignmentController>, by: i32);
        #[qinvokable]
        fn insert_column(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn delete_columns(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn remove_gap_columns(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn move_rows(self: Pin<&mut AlignmentController>, up: bool);
        #[qinvokable]
        fn rows_to_top(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn remove_rows(self: Pin<&mut AlignmentController>);
        /// The name of the cursor's row.
        #[qinvokable]
        fn row_name(self: &AlignmentController) -> QString;
        /// Rename the cursor's row. Returns an error message, or empty.
        #[qinvokable]
        fn rename_row(self: Pin<&mut AlignmentController>, name: &QString) -> QString;
    }
}

use std::path::PathBuf;
use std::pin::Pin;
use std::time::Instant;

use basecraft_docs::AlignmentDoc;
use basecraft_ops::Session;
use basecraft_plugin_host::file_tool::output_beside;

use crate::document_jobs::{DocumentJob, Finished, bundled, failure, primary_member};
use basecraft_protocol::Op;
use basecraft_protocol::operation::OperationOutcome;
use basecraft_viewmodel::alignment::Region;
use basecraft_viewmodel::{AlignmentView, Frame};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QString};
use serde_json::json;

use crate::bridge::{
    ALT, CTRL, KEY_0, KEY_BACKSPACE, KEY_DELETE, KEY_DOWN, KEY_END, KEY_EQUAL, KEY_ESCAPE,
    KEY_HOME, KEY_LEFT, KEY_MINUS, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_PLUS, KEY_RIGHT, KEY_UP, KEY_Y,
    KEY_Z, SHIFT, WHEEL_ZOOM, ZOOM_STEP, file_name, took,
};

const KEY_A: i32 = 0x41;

/// A plugin that reads this alignment and writes a new file beside it.
struct Tool {
    name: String,
    plugin: String,
    operation: String,
    /// The new file's suffix and extension: `NAME.<suffix>.<ext>`.
    output: (String, String),
    min_rows: usize,
    working: String,
    params: serde_json::Map<String, serde_json::Value>,
}
const KEY_SPACE: i32 = 0x20;

// Independent Qt properties plus the background scan state.
crate::editor::editor_state!(AlignmentRust [drawn documents] {
    last_taxa: String = String::new(),
    alphabet_question: QString = QString::default(),
    hscroll_pos: f64 = 0.0,
    hscroll_size: f64 = 1.0,
    vscroll_pos: f64 = 0.0,
    vscroll_size: f64 = 1.0,
    names_width: f64 = 0.0,
    insert_mode: bool = false,
    bootstrap_running: bool = false,
    bootstrap_cancelling: bool = false,
    bootstrap_progress: f64 = 0.,
    bootstrap_status: QString = QString::default(),
    tool_job: Option<ActiveTool> = None,
    gap_column_count: f64 = -1.0,
    gap_scan_running: bool = false,
    gap_runs: Vec<(u64, u64)> = Vec::new(),
    gap_scan_key: Option<(String, u64)> = None,
    review_snapshot_dir: Option<tempfile::TempDir> = None,
    duplicate_names: QString = QString::default(),
    save_state: crate::editor_save::SaveState = crate::editor_save::SaveState::default(),
    view: AlignmentView = AlignmentView::new(800.0, 600.0),
});

// The worker and monitor retain this directory until the engine has finished.
/// A registered operation running on this alignment. Dropping it cancels the
/// run.
struct ActiveTool {
    work: basecraft_ops::work::Work,
    started: Instant,
    name: String,
    output: PathBuf,
    kind: ToolKind,
    generation: u64,
}

enum ToolKind {
    /// Open the new file, or its component trees when there are several.
    Open,
    /// Report the saved table; nothing to open.
    Matrix,
    /// Apply the placement MAFFT computed for added sequences; a reviewed
    /// consensus is recorded after its row is placed.
    Addition(Option<Reviewed>),
}

/// A reviewed consensus waiting for its row: the accepted review's evidence
/// and the sequence as reviewed.
struct Reviewed {
    evidence: String,
    sequence: String,
}

impl Drop for ActiveTool {
    fn drop(&mut self) {
        self.work.cancel();
    }
}

/// The names of the rows the selection covers.
fn selected_names(view: &AlignmentView, alignment: &basecraft_docs::AlignmentDoc) -> Vec<String> {
    view.selection.map_or_else(Vec::new, |block| {
        (block.rows.0..block.rows.1)
            .map(|i| alignment.row(i).name.clone())
            .collect()
    })
}

#[allow(clippy::cast_possible_truncation)]
fn f(v: f64) -> f32 {
    v as f32
}

crate::editor::document_editor!(qobject::AlignmentController, alignment [fresh fail]);
crate::editor::number_names!(qobject::AlignmentController);
crate::editor::taxa_editor!(
    qobject::AlignmentController,
    alignment,
    basecraft_docs::AlignmentDoc
);
crate::editor::figure_export!(qobject::AlignmentController, alignment);

fn has_figure_selection(view: &AlignmentView) -> bool {
    view.selection.is_some()
}

// Every editor's figure goes through the shared export, which takes a Result.
#[allow(clippy::unnecessary_wraps)]
fn figure_svg(
    view: &mut AlignmentView,
    alignment: &basecraft_docs::AlignmentDoc,
    scope: i32,
    theme: &basecraft_viewmodel::Theme,
    font: &str,
    include_selection: bool,
) -> basecraft_ops::Result<String> {
    let mut view = if scope == 0 {
        view.visible_figure_view(alignment, include_selection)
    } else {
        view.figure_view_with_selection(alignment, scope == 1, include_selection)
    };
    let mut frame = Frame::new();
    view.build(alignment, theme, &mut frame);
    Ok(basecraft_viewmodel::svg::frame_svg(
        &frame,
        view.width,
        view.height,
        font,
        theme.background,
    ))
}

impl qobject::AlignmentController {
    fn before_history(self: Pin<&mut Self>) -> bool {
        true
    }

    fn after_history(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        if let (Some(session), Some(doc)) = (&r.session, r.doc) {
            r.view.sync(crate::bridge::checked_document!(
                access_thread,
                session.alignment(doc)
            ));
        }
    }
}

impl crate::document_open::DocumentEditor for qobject::AlignmentController {
    fn apply_edit(mut self: Pin<&mut Self>, op: Op) -> bool {
        let a = self.as_mut().edit(op);
        self.refresh();
        a
    }
    fn set_refused(self: Pin<&mut Self>, refused: QString) {
        self.set_refused_edit(refused);
    }
    fn open_document(&self) -> Option<(&Session, basecraft_ops::DocId)> {
        self.rust().session.as_ref().zip(self.rust().doc)
    }
    fn offer_numbering(self: Pin<&mut Self>, offer: QString) {
        self.set_duplicate_names(offer);
    }
    fn alphabet_question(&self) -> Option<String> {
        let (session, doc) = self.rust().session.as_ref().zip(self.rust().doc)?;
        basecraft_viewmodel::alphabet::alignment_question(session.alignment(doc).ok()?)
    }
    fn set_question(self: Pin<&mut Self>, question: QString) {
        self.set_alphabet_question(question);
    }
}

impl qobject::AlignmentController {
    pub fn record_folder(&self) -> QString {
        crate::document_open::record_folder(self.rust().session.as_ref())
    }

    /// Rebuild the frame, status line and scroll bars.
    fn refresh(mut self: Pin<&mut Self>) {
        if !self.as_mut().refresh_start() {
            return;
        }
        let access_thread = self.qt_thread();
        let (bytes, status, zoom, h, v, names) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.alignment(doc));
            r.find
                .invalidate(&crate::find_bridge::document_key(session, doc));
            r.view.build(d, &r.theme, &mut r.scratch);
            r.view.draw_find(&r.find, &r.theme, &mut r.scratch);
            (
                r.scratch.to_bytes(),
                r.view.status(d),
                r.view.zoom_text(),
                r.view.hscroll(),
                r.view.vscroll(),
                f64::from(r.view.name_width),
            )
        };
        self.as_mut().set_names_width(names);
        let (overview, matches) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.alignment(doc));
            let key = format!("{}:{}:{:?}", session.id(), d.version(), r.theme);
            let content = r.overview_cache.get(key.clone(), || {
                basecraft_viewmodel::overview::alignment(d, &r.theme)
            });
            let matches =
                r.overview_match_cache
                    .get_json(format!("{key}:{}", r.find.revision()), || {
                        basecraft_viewmodel::find::overview_marks(&r.find, |hit| {
                            r.view.overview_match(
                                hit.row,
                                hit.start,
                                hit.end,
                                hit.kind == basecraft_viewmodel::find::Kind::Name,
                            )
                        })
                    });
            (content, matches)
        };
        self.as_mut().set_overview_content(QString::from(&overview));
        self.as_mut().set_overview_matches(QString::from(&matches));
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
        self.as_mut().set_status(QString::from(&status));
        self.as_mut().set_zoom_text(QString::from(&zoom));
        self.as_mut().set_hscroll_size(h.1);
        self.as_mut().set_hscroll_pos(h.0);
        self.as_mut().set_vscroll_size(v.1);
        self.as_mut().set_vscroll_pos(v.0);
        self.as_mut().update_gap_count();
        self.emit_taxa();
    }

    // Only one scan at a time; discard old versions and scan the newest snapshot.
    fn update_gap_count(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.alignment(doc));
        }
        let work = {
            let mut r = self.as_mut().rust_mut();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.alignment(doc));
            let key = (session.id().to_owned(), d.version());
            if r.gap_scan_key.as_ref() == Some(&key) {
                return;
            }
            if r.gap_scan_running {
                self.as_mut().set_gap_column_count(-1.0);
                return;
            }
            let snapshot = d.snapshot();
            r.gap_scan_running = true;
            (key, snapshot)
        };
        self.as_mut().set_gap_column_count(-1.0);
        let (key, snapshot) = work;
        let started = crate::jobs::start(
            self.qt_thread(),
            move |_| Ok(snapshot.gap_column_runs()),
            move |mut this, runs: Result<Vec<(u64, u64)>, String>| {
                let Ok(runs) = runs else {
                    this.as_mut().rust_mut().gap_scan_running = false;
                    return;
                };
                let count: u64 = runs.iter().map(|(a, b)| b - a).sum();
                let current = {
                    let mut r = this.as_mut().rust_mut();
                    r.gap_scan_running = false;
                    let current = r.session.as_ref().zip(r.doc).is_some_and(|(s, d)| {
                        s.id() == key.0
                            && crate::bridge::checked_document!(access_thread, s.alignment(d))
                                .version()
                                == key.1
                    });
                    if current {
                        r.gap_scan_key = Some(key);
                        r.gap_runs = runs;
                    }
                    current
                };
                if current {
                    #[allow(clippy::cast_precision_loss)]
                    this.as_mut().set_gap_column_count(count as f64);
                } else {
                    this.update_gap_count();
                }
            },
        );
        if started.is_err() {
            self.as_mut().rust_mut().gap_scan_running = false;
        }
    }

    fn fail_addition(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().fail("Cannot add sequences");
        self.as_mut().addition_failed(QString::from(msg));
    }

    /// Apply an edit through the session; on failure, say why.
    fn edit(mut self: Pin<&mut Self>, op: Op) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.alignment(doc));
        }
        let (result, refused_now) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return false;
            };
            let (result, refused) =
                crate::document_open::applying(session, doc, op, basecraft_ops::renames::apply_op);
            r.view.sync(crate::bridge::checked_document!(
                access_thread,
                session.alignment(doc)
            ));
            (result, refused)
        };
        crate::document_open::offer_copy(self.as_mut(), refused_now);
        match result {
            Ok(()) => {
                let dirty =
                    crate::document_open::unsaved(self.rust().session.as_ref(), self.rust().doc);
                self.as_mut().set_dirty(dirty);
                self.as_mut().notify("");
                true
            }
            Err(e) => {
                self.fail(&e);
                false
            }
        }
    }

    pub fn export_svg(self: Pin<&mut Self>, path: &QString) {
        self.export_figure(path, 0, false);
    }

    pub fn export_figure_selection(
        self: Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
        include_selection: bool,
    ) {
        self.export_figure_with(path, scope, publication, None, include_selection);
    }

    pub fn export_figure_png_selection(
        self: Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
        width: f64,
        include_selection: bool,
    ) {
        self.export_figure_with(path, scope, publication, Some(width), include_selection);
    }

    pub(crate) fn install_opened(
        mut self: Pin<&mut Self>,
        candidate: crate::document_open::Candidate,
    ) -> Result<(), crate::document_open::OpenError> {
        candidate.check(basecraft_ops::files::Kind::Alignment)?;
        let crate::document_open::Candidate {
            session,
            doc,
            path,
            root,
            ..
        } = candidate;
        let _ = root;
        let alignment = session.alignment(doc)?;
        {
            let mut r = self.as_mut().rust_mut();
            r.view.sync(alignment);
            r.view.reset_zoom();
            r.session = Some(session);
            r.doc = Some(doc);
        }
        self.as_mut()
            .set_file_path(QString::from(path.to_string_lossy().as_ref()));
        self.as_mut().set_dirty(false);
        self.as_mut().notify("");
        crate::document_open::installed(self.as_mut(), &path);
        self.refresh();
        Ok(())
    }

    pub fn wheel(mut self: Pin<&mut Self>, x: f64, y: f64, dy: f64, dx: f64, modifiers: i32) {
        {
            let mut r = self.as_mut().rust_mut();
            if modifiers & CTRL != 0 {
                r.view.zoom_at(f(x), f(y), WHEEL_ZOOM.powf(dy));
            } else if modifiers & SHIFT != 0 || dx.abs() > dy.abs() {
                let d = if dx.abs() > dy.abs() { dx } else { dy };
                r.view.pan(f(-d / 2.0));
            } else {
                r.view.scroll_by(f(-dy / 2.0));
            }
        }
        self.refresh();
    }

    pub fn press(mut self: Pin<&mut Self>, x: f64, y: f64, modifiers: i32) {
        crate::bridge::checked_document!(sync self, self.check_document());
        self.as_mut()
            .rust_mut()
            .view
            .press(f(x), f(y), modifiers & SHIFT != 0);
        self.refresh();
    }

    pub fn drag(mut self: Pin<&mut Self>, x: f64, y: f64) {
        let slide = self.as_mut().rust_mut().view.drag_to(f(x), f(y));
        if let Some(by) = slide {
            self.as_mut().slide_steps(by);
        }
        self.refresh();
    }

    pub fn release(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.release();
        self.refresh();
    }

    pub fn pan_press(mut self: Pin<&mut Self>, x: f64, y: f64) {
        self.as_mut().rust_mut().view.pan_press(f(x), f(y));
    }

    pub fn edge_scroll(mut self: Pin<&mut Self>, x: f64, y: f64) -> bool {
        let moved = self.as_mut().rust_mut().view.edge_scroll(f(x), f(y));
        if moved {
            self.refresh();
        }
        moved
    }

    /// Slide the selection by `by`, a column at a time, stopping at the
    /// first step that has residues in the way. Steps merge into one log
    /// entry.
    fn slide_steps(mut self: Pin<&mut Self>, by: i64) {
        let step = by.signum();
        for _ in 0..by.unsigned_abs() {
            let op = self.rust().view.shift_op(step);
            if !self.as_mut().edit(op) {
                break;
            }
            self.as_mut().rust_mut().view.offset_columns(step);
        }
    }

    pub fn key(mut self: Pin<&mut Self>, key: i32, text: &QString, modifiers: i32) -> bool {
        crate::bridge::checked_document!(sync self, self.check_document());
        let ctrl = modifiers & CTRL != 0;
        let shift = modifiers & SHIFT != 0;
        let alt = modifiers & ALT != 0;
        let handled = match key {
            KEY_Z if ctrl && shift => {
                self.as_mut().history(true);
                true
            }
            KEY_Z if ctrl => {
                self.as_mut().history(false);
                true
            }
            KEY_Y if ctrl => {
                self.as_mut().history(true);
                true
            }
            KEY_0 if ctrl => {
                self.as_mut().rust_mut().view.fit();
                true
            }
            KEY_A if ctrl => {
                self.as_mut().rust_mut().view.select_all();
                true
            }
            KEY_PLUS | KEY_EQUAL | KEY_MINUS if ctrl => {
                let factor = if key == KEY_MINUS {
                    1.0 / ZOOM_STEP
                } else {
                    ZOOM_STEP
                };
                self.as_mut().rust_mut().view.zoom_step(factor);
                true
            }
            KEY_LEFT | KEY_RIGHT if alt => {
                self.as_mut().slide(if key == KEY_LEFT { -1 } else { 1 });
                true
            }
            KEY_UP | KEY_DOWN if alt => {
                self.as_mut().move_rows(key == KEY_UP);
                true
            }
            KEY_LEFT | KEY_RIGHT | KEY_UP | KEY_DOWN => {
                let step = if ctrl { 10 } else { 1 };
                let (rows, cols) = match key {
                    KEY_LEFT => (0, -step),
                    KEY_RIGHT => (0, step),
                    KEY_UP => (-step, 0),
                    _ => (step, 0),
                };
                self.as_mut().rust_mut().view.move_cursor(rows, cols, shift);
                true
            }
            KEY_HOME | KEY_END => {
                let mut r = self.as_mut().rust_mut();
                let far = i64::MAX / 4;
                let cols = if key == KEY_HOME { -far } else { far };
                let rows = if ctrl {
                    if key == KEY_HOME { -far } else { far }
                } else {
                    0
                };
                r.view.move_cursor(rows, cols, shift);
                true
            }
            KEY_PAGE_UP | KEY_PAGE_DOWN => {
                let mut r = self.as_mut().rust_mut();
                let page = r.view.page_rows();
                r.view
                    .move_cursor(if key == KEY_PAGE_UP { -page } else { page }, 0, shift);
                true
            }
            KEY_ESCAPE => {
                self.as_mut().rust_mut().view.collapse();
                true
            }
            KEY_SPACE if !ctrl && !alt => {
                self.as_mut().insert_gap();
                true
            }
            KEY_BACKSPACE => {
                self.as_mut().remove_gap();
                true
            }
            KEY_DELETE => {
                self.as_mut().delete_selection();
                true
            }
            _ => {
                let t = text.to_string();
                let residue = t.len() == 1
                    && t.bytes()
                        .all(|b| b.is_ascii_alphabetic() || b == b'-' || b == b'*');
                if residue && !ctrl && !alt {
                    let ch = t.to_ascii_uppercase().chars().next().unwrap_or('N');
                    let op = self.rust().view.type_op(ch, self.rust().insert_mode);
                    if self.as_mut().edit(op) {
                        self.as_mut().rust_mut().view.advance();
                    }
                    true
                } else {
                    false
                }
            }
        };
        if handled {
            self.refresh();
        }
        handled
    }

    pub fn state_alphabet(self: Pin<&mut Self>, answer: &QString) -> bool {
        crate::document_open::state_alphabet(self, &answer.to_string())
    }

    pub fn double_click(mut self: Pin<&mut Self>, x: f64, y: f64) {
        self.as_mut().rust_mut().view.zoom_at(f(x), f(y), 2.0);
        self.refresh();
    }

    pub fn zoom_by(mut self: Pin<&mut Self>, x: f64, y: f64, factor: f64) {
        if factor > 0.0 {
            self.as_mut().rust_mut().view.zoom_at(f(x), f(y), factor);
        }
        self.refresh();
    }

    pub fn set_zoom_percent(mut self: Pin<&mut Self>, percent: f64) {
        if !percent.is_finite() || !(0.000_001..=20_000.0).contains(&percent) {
            return;
        }
        let factor = percent / self.rust().view.zoom_percent();
        self.as_mut().rust_mut().view.zoom_step(factor);
        self.refresh();
    }

    pub fn zoom_in(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.zoom_step(ZOOM_STEP);
        self.refresh();
    }

    pub fn zoom_out(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.zoom_step(1.0 / ZOOM_STEP);
        self.refresh();
    }

    pub fn zoom_to_selection(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.zoom_to_selection();
        self.refresh();
    }

    pub fn hscroll_to(mut self: Pin<&mut Self>, fraction: f64) {
        self.as_mut().rust_mut().view.hscroll_to(fraction);
        self.refresh();
    }

    pub fn vscroll_to(mut self: Pin<&mut Self>, fraction: f64) {
        self.as_mut().rust_mut().view.vscroll_to(fraction);
        self.refresh();
    }

    pub fn context_press(mut self: Pin<&mut Self>, x: f64, y: f64) -> QString {
        crate::bridge::checked_document!(sync self, self.check_document());
        let region = self.as_mut().rust_mut().view.context_press(f(x), f(y));
        self.as_mut().refresh();
        let b = self.rust().view.block();
        let name = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| d.row(self.rust().view.cursor.0).name.clone())
        )
        .unwrap_or_default();
        let info = json!({
            "region": match region {
                Region::Grid => "grid",
                Region::Names => "names",
                Region::Header => "header",
                Region::Corner => "corner",
            },
            "rows": b.row_count(),
            "columns": b.col_count(),
            "rowName": name,
        });
        QString::from(&info.to_string())
    }

    pub fn hover(&self, x: f64, y: f64) -> QString {
        let text = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| self.rust().view.hover_text(f(x), f(y), d))
        )
        .flatten()
        .unwrap_or_default();
        QString::from(&text)
    }

    pub fn blast_queries(&self, source: &QString, _feature: i32) -> QString {
        let result = self
            .with_doc(|doc| {
                let source = source.to_string();
                let block = self.rust().view.block();
                let rows = if source == "all" {
                    (0, doc.row_count())
                } else {
                    block.rows
                };
                let columns = if source == "selection" {
                    block.cols
                } else {
                    (0, doc.width())
                };
                basecraft_ops::blast::from_alignment(doc, rows, columns)
            })
            .and_then(|value| {
                value.unwrap_or_else(|| {
                    Err(basecraft_ops::Error::Invalid("Alignment closed".into()))
                })
            });
        QString::from(&match result {
            Ok(queries) => serde_json::to_string(&queries).unwrap_or_default(),
            Err(error) => serde_json::json!({"error":error.to_string()}).to_string(),
        })
    }

    pub fn selection_text(&self) -> QString {
        let text = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| self.rust().view.selection_fasta(d))
        )
        .unwrap_or_default();
        QString::from(&text)
    }

    pub fn select_all(mut self: Pin<&mut Self>) {
        crate::bridge::checked_document!(sync self, self.check_document());
        self.as_mut().rust_mut().view.select_all();
        self.refresh();
    }

    pub fn read_sequence_file(mut self: Pin<&mut Self>, path: &QString) -> QString {
        match std::fs::read_to_string(path.to_string()) {
            Ok(text) if text.trim_start().starts_with('>') => QString::from(text),
            Ok(_) => {
                self.as_mut().fail_addition("Choose a FASTA file");
                QString::default()
            }
            Err(e) => {
                self.as_mut().fail_addition(&e.to_string());
                QString::default()
            }
        }
    }

    pub fn review_snapshot(mut self: Pin<&mut Self>) -> QString {
        crate::bridge::checked_document!(sync self, self.check_document());
        let result = (|| -> Result<(tempfile::TempDir, PathBuf), String> {
            let dir = tempfile::Builder::new()
                .prefix("basecraft-review-reference-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            let path = dir.path().join("reference.afa");
            let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            self.with_doc(|doc| basecraft_ops::files::write_alignment(doc, &mut file))
                .map_err(|e| e.to_string())?
                .ok_or("No alignment open")?
                .map_err(|e| e.to_string())?;
            Ok((dir, path))
        })();
        match result {
            Ok((dir, path)) => {
                self.as_mut().rust_mut().review_snapshot_dir = Some(dir);
                QString::from(path.to_string_lossy().as_ref())
            }
            Err(e) => {
                self.as_mut().fail(&e);
                QString::default()
            }
        }
    }

    pub fn add_reviewed_sequences(
        self: Pin<&mut Self>,
        text: &QString,
        name: &QString,
        mafft: bool,
        orientation: bool,
        keep: bool,
        evidence: &QString,
    ) {
        self.add_sequences_impl(
            text,
            name,
            mafft,
            orientation,
            keep,
            Some(evidence.to_string()),
        );
    }

    pub fn add_sequences(
        self: Pin<&mut Self>,
        text: &QString,
        name: &QString,
        mafft: bool,
        orientation: bool,
        keep: bool,
    ) {
        self.add_sequences_impl(text, name, mafft, orientation, keep, None);
    }

    fn add_sequences_impl(
        mut self: Pin<&mut Self>,
        text: &QString,
        name: &QString,
        mafft: bool,
        orientation: bool,
        keep: bool,
        evidence: Option<String>,
    ) {
        if self.rust().busy {
            return;
        }
        if crate::bridge::checked_document!(self.qt_thread(), self.with_doc(|_| ())).is_none() {
            self.fail_addition("No alignment open");
            return;
        }
        let (text, name) = (text.to_string(), name.to_string());
        // A reviewed consensus is checked by the same call as `basecraft
        // sanger add`, and its review recorded only once its row is in.
        let rows = {
            let r = self.rust();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            match &evidence {
                Some(evidence) => basecraft_ops::sanger_commands::consensus_rows(
                    session, doc, evidence, &text, &name,
                ),
                None => session.alignment(doc).and_then(|alignment| {
                    Ok(basecraft_ops::alignment_add::addition_rows(
                        alignment, &text, &name,
                    )?)
                }),
            }
            .map_err(|e| e.to_string())
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => {
                self.fail_addition(&error);
                return;
            }
        };
        let reviewed = evidence.map(|evidence| Reviewed {
            evidence,
            sequence: rows[0].1.clone(),
        });
        if !mafft {
            if self.as_mut().add_rows(
                Op::AlnAddSequences {
                    rows,
                    reference: None,
                },
                reviewed.as_ref(),
            ) {
                self.as_mut().done("Added sequences");
                self.as_mut().sequences_added();
            } else {
                let message = self.rust().message.to_string();
                self.as_mut().fail_addition(&message);
            }
            self.refresh();
            return;
        }
        let mut params = serde_json::Map::new();
        params.insert("rows".into(), json!(rows));
        params.insert("orientation".into(), json!(orientation));
        params.insert("keep".into(), json!(keep));
        let tool = Tool {
            name: "MAFFT".into(),
            plugin: "mafft".into(),
            operation: "add_sequences".into(),
            output: (String::new(), String::new()),
            min_rows: 0,
            working: "Adding sequences with MAFFT…".into(),
            params,
        };
        self.start_tool(&tool, ToolKind::Addition(reviewed));
    }

    /// Apply an addition. A reviewed consensus is recorded once its row is
    /// in, and the row is undone if that fails, so neither stays alone.
    fn add_rows(mut self: Pin<&mut Self>, op: Op, reviewed: Option<&Reviewed>) -> bool {
        if !self.as_mut().edit(op) {
            return false;
        }
        let Some(reviewed) = reviewed else {
            return true;
        };
        let recorded = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            match (&mut r.session, r.doc) {
                (Some(session), Some(doc)) => basecraft_ops::sanger_commands::record_consensus(
                    session,
                    doc,
                    &reviewed.evidence,
                    &reviewed.sequence,
                )
                .map_err(|e| e.to_string()),
                _ => Err("No alignment open".into()),
            }
        };
        if let Err(error) = recorded {
            self.as_mut().undo();
            self.as_mut().set_message(QString::from(&error));
            return false;
        }
        true
    }

    /// Apply MAFFT's placement of added sequences, read from the run's result.
    fn finish_addition(
        mut self: Pin<&mut Self>,
        outcome: &OperationOutcome,
        generation: u64,
        reviewed: Option<&Reviewed>,
    ) {
        let result = (|| -> Result<serde_json::Value, String> {
            {
                let r = self.rust();
                let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                    return Err("Alignment was closed".into());
                };
                if session.generation(doc).map_err(|e| e.to_string())? != generation {
                    return Err("Alignment changed during addition; try again".into());
                }
            }
            let path = outcome
                .artifacts
                .iter()
                .find(|artifact| artifact.path.ends_with("addition.json"))
                .ok_or("MAFFT returned no placement")?;
            let bytes = std::fs::read(&path.path).map_err(|e| e.to_string())?;
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())
        })();
        let value = match result {
            Ok(value) => value,
            Err(e) => {
                self.fail_addition(&e);
                return;
            }
        };
        let count = value["insertions"].as_i64().unwrap_or(0);
        if count > 0 {
            self.as_mut().notify("New bases need reference gaps");
            self.as_mut()
                .addition_needs_gaps(i32::try_from(count).unwrap_or(i32::MAX));
            return;
        }
        let parsed = serde_json::from_value::<Vec<(String, String)>>(value["rows"].clone())
            .and_then(|rows| {
                serde_json::from_value::<Vec<String>>(value["reference"].clone())
                    .map(|reference| (rows, reference))
            });
        match parsed {
            Ok((rows, reference)) => {
                if self.as_mut().add_rows(
                    Op::AlnAddSequences {
                        rows,
                        reference: Some(reference),
                    },
                    reviewed,
                ) {
                    self.as_mut().done("Added sequences");
                    self.as_mut().sequences_added();
                } else {
                    let message = self.rust().message.to_string();
                    self.as_mut().fail_addition(&message);
                }
                self.as_mut().refresh();
            }
            Err(e) => self.as_mut().fail_addition(&e.to_string()),
        }
    }

    pub fn align_mafft(self: Pin<&mut Self>) {
        self.run_tool(&Tool {
            name: "MAFFT".into(),
            plugin: "mafft".into(),
            operation: "align".into(),
            output: ("mafft".into(), "fasta".into()),
            min_rows: 2,
            working: "Aligning with MAFFT…".into(),
            params: serde_json::Map::new(),
        });
    }

    pub fn trim_alignment(self: Pin<&mut Self>) {
        let mut params = serde_json::Map::new();
        params.insert("method".into(), serde_json::json!("automated1"));
        self.run_tool(&Tool {
            name: "trimAl".into(),
            plugin: "trimal".into(),
            operation: "trim".into(),
            output: ("trimmed".into(), "fasta".into()),
            min_rows: 2,
            working: "Trimming with trimAl…".into(),
            params,
        });
    }

    /// What a settings form may depend on: the alignment's sequence type.
    fn form_context(&self) -> basecraft_ops::Result<serde_json::Map<String, serde_json::Value>> {
        let protein = self.with_doc(AlignmentDoc::is_protein)?.unwrap_or(false);
        Ok([(
            "sequence".to_string(),
            serde_json::json!(if protein { "protein" } else { "dna" }),
        )]
        .into_iter()
        .collect())
    }

    pub fn tool_form(&self, plugin: &QString, form: &QString) -> QString {
        let context = match self.form_context() {
            Ok(context) => context,
            Err(error) => return QString::from(&json!({"error": error.to_string()}).to_string()),
        };
        let value = crate::plugins::bundled_manifest(&plugin.to_string()).and_then(|m| {
            m.form(&form.to_string())
                .map(|f| serde_json::to_value(f.for_context(&context)).unwrap_or_default())
                .ok_or_else(|| format!("{plugin} has no {form} settings"))
        });
        QString::from(
            &value
                .unwrap_or_else(|error| serde_json::json!({"error": error}))
                .to_string(),
        )
    }

    pub fn run_form(self: Pin<&mut Self>, plugin: &QString, form: &QString, values: &QString) {
        let tool = (|| -> Result<Tool, String> {
            let plugin = plugin.to_string();
            let manifest = crate::plugins::bundled_manifest(&plugin)?;
            let form = manifest
                .form(&form.to_string())
                .ok_or_else(|| format!("{} has no {form} settings", manifest.name))?;
            let given: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&values.to_string()).map_err(|e| e.to_string())?;
            let mut params = form
                .for_context(&self.form_context().map_err(|e| e.to_string())?)
                .parameters(&given)?;
            if form.threads {
                params.insert(
                    "threads".into(),
                    serde_json::json!(basecraft_plugin_host::compute::allocation()),
                );
            }
            Ok(Tool {
                name: form.title.clone(),
                plugin,
                operation: form.operation.clone(),
                output: form.output.clone(),
                min_rows: form.min_rows,
                working: form.working.clone(),
                params,
            })
        })();
        match tool {
            Ok(tool) => self.run_tool(&tool),
            Err(error) => self.fail(&error),
        }
    }

    pub fn cancel_bootstrap(mut self: Pin<&mut Self>) {
        let Some(tool) = &self.rust().tool_job else {
            return;
        };
        tool.work.cancel();
        self.as_mut().set_bootstrap_cancelling(true);
        self.set_bootstrap_status(QString::from("Cancelling after the current replicate…"));
    }

    /// Run a plugin on this alignment in the background, writing a new file
    /// beside it; the run is recorded in this alignment's history.
    fn run_tool(self: Pin<&mut Self>, tool: &Tool) {
        let kind = if tool.operation == "matrix" {
            ToolKind::Matrix
        } else {
            ToolKind::Open
        };
        self.start_tool(tool, kind);
    }

    fn start_tool(mut self: Pin<&mut Self>, tool: &Tool, kind: ToolKind) {
        if self.rust().busy {
            return;
        }
        let name = tool.name.clone();
        if crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(AlignmentDoc::row_count)
        )
        .unwrap_or(0)
            < tool.min_rows
        {
            self.fail(&format!(
                "{name} needs at least {} sequences",
                tool.min_rows
            ));
            return;
        }
        let operation = bundled(&tool.plugin, &tool.operation);
        let bootstrap = tool.plugin == "distance"
            && tool.operation == "tree"
            && tool
                .params
                .get("bootstrap")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|n| n > 0);
        let thread = self.qt_thread();
        let submitted = (|| -> Result<ActiveTool, String> {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return Err("No alignment open".into());
            };
            let (output, exports) = if matches!(kind, ToolKind::Addition(_)) {
                (PathBuf::new(), Vec::new())
            } else {
                let (suffix, ext) = (&tool.output.0, &tool.output.1);
                let output = output_beside(&PathBuf::from(r.file_path.to_string()), suffix, ext);
                let exports = vec![(primary_member(&operation)?, output.clone())];
                (output, exports)
            };
            let generation = session.generation(doc).map_err(|e| e.to_string())?;
            let parameters = serde_json::Value::Object(tool.params.clone());
            let listing = crate::jobs::Listing {
                operation: name.clone(),
                source: r.file_path.to_string(),
            };
            let work = crate::document_jobs::run_operation(
                thread,
                listing,
                &mut r.session,
                move |session| DocumentJob::submit(session, doc, &operation, parameters, &exports),
                qobject::AlignmentController::show_bootstrap_progress,
                qobject::AlignmentController::tool_finished,
            )?;
            Ok(ActiveTool {
                work,
                started: Instant::now(),
                name: name.clone(),
                output,
                kind,
                generation,
            })
        })();
        let active = match submitted {
            Ok(active) => active,
            Err(e) => {
                self.fail(&format!("{name}: {e}"));
                return;
            }
        };
        self.as_mut().rust_mut().tool_job = Some(active);
        if bootstrap {
            self.as_mut().set_bootstrap_running(true);
            self.as_mut().set_bootstrap_cancelling(false);
            self.as_mut().set_bootstrap_progress(0.);
            self.as_mut()
                .set_bootstrap_status(QString::from("Preparing bootstrap…"));
        }
        self.as_mut().set_busy(true);
        self.as_mut().notify(&tool.working);
    }

    /// The run finished and the session has recorded it.
    fn tool_finished(
        mut self: Pin<&mut Self>,
        session: Session,
        finished: Result<Finished, String>,
    ) {
        self.as_mut().rust_mut().session = Some(session);
        let outcome = match finished {
            Ok(finished) => finished.outcome,
            Err(e) => {
                self.as_mut().rust_mut().tool_job = None;
                self.as_mut().set_busy(false);
                self.as_mut().set_bootstrap_running(false);
                self.fail(&e);
                return;
            }
        };
        let Some(tool) = self.as_mut().rust_mut().tool_job.take() else {
            return;
        };
        self.as_mut().set_busy(false);
        self.as_mut().set_bootstrap_running(false);
        self.as_mut().set_bootstrap_cancelling(false);
        let name = &tool.name;
        if outcome.state != basecraft_protocol::operation::RuntimeState::Completed {
            let message = format!("{name}: {}", failure(&outcome));
            if matches!(tool.kind, ToolKind::Addition(_)) {
                self.fail_addition(&message);
            } else {
                self.fail(&message);
            }
            return;
        }
        match &tool.kind {
            ToolKind::Addition(reviewed) => {
                self.finish_addition(&outcome, tool.generation, reviewed.as_ref());
            }
            ToolKind::Matrix => self.as_mut().done(&format!(
                "Done with {name}{}; saved {}",
                took(tool.started),
                tool.output.display()
            )),
            ToolKind::Open => {
                self.as_mut()
                    .done(&format!("Done with {name}{}", took(tool.started)));
                let components = component_trees(&tool, &outcome);
                if components.is_empty() {
                    let path = tool.output.to_string_lossy().into_owned();
                    self.as_mut().file_created(QString::from(&path));
                } else {
                    for path in components {
                        self.as_mut()
                            .file_created(QString::from(&path.to_string_lossy().into_owned()));
                    }
                }
            }
        }
    }

    fn show_bootstrap_progress(mut self: Pin<&mut Self>, progress: &serde_json::Value) {
        if !self.rust().bootstrap_running || self.rust().bootstrap_cancelling {
            return;
        }
        let attempted = progress["attempted"].as_f64().unwrap_or(0.);
        let requested = progress["requested"].as_f64().unwrap_or(1.).max(1.);
        let completed = progress["completed"].as_u64().unwrap_or(0);
        let failures = progress["failed_replicates"].as_u64().unwrap_or(0);
        self.as_mut().set_bootstrap_progress(attempted / requested);
        self.as_mut().set_bootstrap_status(QString::from(&format!(
            "Bootstrap {attempted:.0}/{requested:.0}; {completed} complete, {failures} failed"
        )));
    }

    pub fn select_block(mut self: Pin<&mut Self>, r0: i32, c0: i32, r1: i32, c1: i32) {
        let u = |v: i32| usize::try_from(v).unwrap_or(0);
        let w = |v: i32| u64::try_from(v).unwrap_or(0);
        self.as_mut()
            .rust_mut()
            .view
            .select_block((u(r0), u(r1)), (w(c0), w(c1)));
        self.refresh();
    }

    pub fn insert_gap(mut self: Pin<&mut Self>) {
        let op = self.rust().view.insert_gap_op();
        if self.as_mut().edit(op) {
            self.as_mut().rust_mut().view.offset_columns(1);
        }
        self.refresh();
    }

    pub fn remove_gap(mut self: Pin<&mut Self>) {
        let op = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| self.rust().view.remove_left_op(d, self.rust().insert_mode))
        )
        .unwrap_or_else(|| Err(String::new()));
        match op {
            Ok(op) => {
                if self.as_mut().edit(op) {
                    self.as_mut().rust_mut().view.offset_columns(-1);
                }
            }
            Err(msg) => self.as_mut().fail(&msg),
        }
        self.refresh();
    }

    pub fn delete_selection(mut self: Pin<&mut Self>) {
        let op = self.rust().view.delete_op();
        if self.as_mut().edit(op) {
            let mut r = self.as_mut().rust_mut();
            let b = r.view.block();
            r.view.cursor = (b.rows.0, b.cols.0);
            r.view.collapse();
        }
        self.refresh();
    }

    pub fn slide(mut self: Pin<&mut Self>, by: i32) {
        self.as_mut().slide_steps(i64::from(by));
        self.refresh();
    }

    pub fn insert_column(mut self: Pin<&mut Self>) {
        let op = self.rust().view.insert_column_op();
        if self.as_mut().edit(op) {
            self.as_mut().rust_mut().view.offset_columns(1);
        }
        self.refresh();
    }

    pub fn delete_columns(mut self: Pin<&mut Self>) {
        let op = self.rust().view.delete_columns_op();
        if self.as_mut().edit(op) {
            self.as_mut().rust_mut().view.collapse();
        }
        self.refresh();
    }

    pub fn remove_gap_columns(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.alignment(doc));
        }
        let started = Instant::now();
        let before =
            crate::bridge::checked_document!(self.qt_thread(), self.with_doc(AlignmentDoc::width))
                .unwrap_or(0);
        let runs = if self.rust().gap_column_count >= 0.0 {
            self.rust().gap_runs.clone()
        } else {
            crate::bridge::checked_document!(
                self.qt_thread(),
                self.with_doc(AlignmentDoc::gap_column_runs)
            )
            .unwrap_or_default()
        };
        let mut view = self.rust().view.clone();
        view.map_deleted_columns(&runs);
        if self.as_mut().edit(Op::AlnRemoveGapColumns {}) {
            {
                let mut r = self.as_mut().rust_mut();
                if let (Some(session), Some(doc)) = (&r.session, r.doc) {
                    view.sync(crate::bridge::checked_document!(
                        access_thread,
                        session.alignment(doc)
                    ));
                }
                r.view = view;
            }
            let after = crate::bridge::checked_document!(
                self.qt_thread(),
                self.with_doc(AlignmentDoc::width)
            )
            .unwrap_or(0);
            if before == after {
                self.as_mut().notify("No gap-only columns");
            } else {
                self.as_mut().done(&format!(
                    "Removed {} gap-only columns{}",
                    before - after,
                    took(started)
                ));
            }
        }
        self.refresh();
    }

    pub fn move_rows(mut self: Pin<&mut Self>, up: bool) {
        if let Some(op) = self.rust().view.move_rows_op(up)
            && self.as_mut().edit(op)
        {
            self.as_mut()
                .rust_mut()
                .view
                .offset_rows(if up { -1 } else { 1 });
        }
        self.refresh();
    }

    pub fn rows_to_top(mut self: Pin<&mut Self>) {
        let op = self.rust().view.rows_to_top_op();
        let b = self.rust().view.block();
        if self.as_mut().edit(op) {
            self.as_mut().rust_mut().view.select_rows(0, b.row_count());
        }
        self.refresh();
    }

    pub fn remove_rows(mut self: Pin<&mut Self>) {
        let op = self.rust().view.remove_rows_op();
        if self.as_mut().edit(op) {
            self.as_mut().rust_mut().view.collapse();
        }
        self.refresh();
    }

    pub fn row_name(&self) -> QString {
        let name = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| d.row(self.rust().view.cursor.0).name.clone())
        )
        .unwrap_or_default();
        QString::from(&name)
    }

    pub fn rename_row(mut self: Pin<&mut Self>, name: &QString) -> QString {
        let row = self.rust().view.cursor.0;
        let op = Op::AlnRenameRow {
            row: u32::try_from(row).unwrap_or(u32::MAX),
            name: name.to_string().trim().to_string(),
        };
        let ok = self.as_mut().edit(op);
        let msg = if ok {
            String::new()
        } else {
            self.rust().message.to_string()
        };
        self.refresh();
        QString::from(&msg)
    }
}

/// The component trees a forest run exported beside its main output.
fn component_trees(tool: &ActiveTool, outcome: &OperationOutcome) -> Vec<PathBuf> {
    let stem = tool
        .output
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let prefix = format!("{stem}.component-");
    outcome
        .artifacts
        .iter()
        .map(|artifact| PathBuf::from(&artifact.path))
        .filter(|path| {
            !path
                .components()
                .any(|part| part.as_os_str() == ".basecraft")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
        })
        .collect()
}

crate::find_bridge::controller_find!(qobject::AlignmentController, alignment, |d| {
    basecraft_viewmodel::find::Input::Alignment(d.snapshot())
});

impl qobject::AlignmentController {
    fn find_reveal(mut self: std::pin::Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.alignment(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        let (Some(session), Some(doc), Some(hit)) = (&r.session, r.doc, r.find.hit()) else {
            return;
        };
        let d = crate::bridge::checked_document!(access_thread, session.alignment(doc));
        r.view.sync(d);
        let start = r.view.start;
        r.view.reveal((hit.row, hit.start));
        if hit.kind == basecraft_viewmodel::find::Kind::Name {
            r.view.start = start;
        }
    }
}

#[cfg(test)]
#[allow(dead_code)] // Invoked by the windowless application_bootstrap harness.
#[path = "../tests/controller_refusal/alignment.rs"]
pub(crate) mod refusal_tests;

crate::editor_save::ordinary_save!(qobject::AlignmentController);

crate::editor_save::confirmed_save!(qobject::AlignmentController);

crate::appearance::editor_follows_appearance!(qobject::AlignmentController);
