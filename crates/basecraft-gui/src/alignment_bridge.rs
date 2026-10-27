//! The Rust object QML talks to for one alignment editor.
//!
//! As with the genome editor, QML forwards raw input; the view model turns
//! it into viewport changes, selections and operations, and every edit goes
//! through the ops session.

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for AlignmentController {}

    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QByteArray, frame)]
        #[qproperty(QString, status)]
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
        fn create_delimitation(self: Pin<&mut AlignmentController>, method: &QString) -> QString;
        #[qinvokable]
        fn open_file(self: Pin<&mut AlignmentController>, path: &QString) -> bool;
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
        #[qinvokable]
        fn redo(self: Pin<&mut AlignmentController>);
        #[qinvokable]
        fn save(self: Pin<&mut AlignmentController>);
        /// Save to a new file and carry on editing that one.
        #[qinvokable]
        fn save_as(self: Pin<&mut AlignmentController>, path: &QString) -> bool;
        /// Reload the Omarchy theme if it changed.
        #[qinvokable]
        fn check_theme(self: Pin<&mut AlignmentController>);
        /// Write a pending burst of edits to the log.
        #[qinvokable]
        fn flush(self: Pin<&mut AlignmentController>);
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
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Instant, SystemTime};

use basecraft_docs::AlignmentDoc;
use basecraft_ops::{DocId, Session};
use basecraft_plugin_host::FileJob;
use basecraft_plugin_host::file_tool::{AlignmentInput, output_beside};
use basecraft_protocol::Op;
use basecraft_viewmodel::alignment::Region;
use basecraft_viewmodel::{AlignmentView, Frame, Theme};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QString};
use serde_json::json;

use crate::bridge::{
    ALT, CTRL, KEY_0, KEY_BACKSPACE, KEY_DELETE, KEY_DOWN, KEY_END, KEY_EQUAL, KEY_ESCAPE,
    KEY_HOME, KEY_LEFT, KEY_MINUS, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_PLUS, KEY_RIGHT, KEY_UP, KEY_Y,
    KEY_Z, SHIFT, WHEEL_ZOOM, ZOOM_STEP, css, file_name, load_theme, memory_text, monospace_font,
    omarchy_theme_path, plugin_dir, project_root, took,
};

const KEY_A: i32 = 0x41;

/// A plugin that reads this alignment and writes a new file beside it.
struct Tool {
    name: String,
    plugin: String,
    operation: String,
    input: AlignmentInput,
    /// The new file's suffix and extension: `NAME.<suffix>.<ext>`.
    output: (String, String),
    min_rows: usize,
    working: String,
    params: serde_json::Map<String, serde_json::Value>,
}
const KEY_SPACE: i32 = 0x20;

// Independent Qt properties plus the background scan state.
#[allow(clippy::struct_excessive_bools)]
pub struct AlignmentRust {
    frame: QByteArray,
    last_taxa: String,
    status: QString,
    message: QString,
    message_kind: QString,
    ok_color: QString,
    error_color: QString,
    background: QString,
    foreground: QString,
    accent: QString,
    mono_font: QString,
    file_path: QString,
    dirty: bool,
    zoom_text: QString,
    hscroll_pos: f64,
    hscroll_size: f64,
    vscroll_pos: f64,
    vscroll_size: f64,
    names_width: f64,
    insert_mode: bool,
    busy: bool,
    bootstrap_running: bool,
    bootstrap_cancelling: bool,
    bootstrap_progress: f64,
    bootstrap_status: QString,
    bootstrap_control: Option<Arc<BootstrapControl>>,
    gap_column_count: f64,
    gap_scan_running: bool,
    gap_runs: Vec<(u64, u64)>,
    gap_scan_key: Option<(String, u64)>,

    review_snapshot_dir: Option<tempfile::TempDir>,
    session: Option<Session>,
    doc: Option<DocId>,
    view: AlignmentView,
    theme: Theme,
    theme_stamp: Option<SystemTime>,
    scratch: Frame,
}

// The worker and monitor retain this directory until the engine has finished.
struct BootstrapControl {
    directory: tempfile::TempDir,
    finished: AtomicBool,
}
impl Drop for AlignmentRust {
    fn drop(&mut self) {
        if let Some(control) = self.bootstrap_control.take() {
            std::thread::spawn(move || {
                let _ = std::fs::write(control.directory.path().join("cancel"), b"");
            });
        }
    }
}

impl Default for AlignmentRust {
    fn default() -> Self {
        let mut c = Self {
            frame: QByteArray::default(),
            last_taxa: String::new(),
            status: QString::default(),
            message: QString::default(),
            message_kind: QString::default(),
            ok_color: QString::default(),
            error_color: QString::default(),
            background: QString::default(),
            foreground: QString::default(),
            accent: QString::default(),
            mono_font: QString::from(&monospace_font()),
            file_path: QString::default(),
            dirty: false,
            zoom_text: QString::default(),
            hscroll_pos: 0.0,
            hscroll_size: 1.0,
            vscroll_pos: 0.0,
            vscroll_size: 1.0,
            names_width: 0.0,
            insert_mode: false,
            busy: false,
            bootstrap_running: false,
            bootstrap_cancelling: false,
            bootstrap_progress: 0.,
            bootstrap_status: QString::default(),
            bootstrap_control: None,
            gap_column_count: -1.0,
            gap_scan_running: false,
            gap_runs: Vec::new(),
            gap_scan_key: None,
            review_snapshot_dir: None,
            session: None,
            doc: None,
            view: AlignmentView::new(800.0, 600.0),
            theme: Theme::default(),
            theme_stamp: None,
            scratch: Frame::new(),
        };
        if let Some((theme, stamp)) = load_theme() {
            c.theme = theme;
            c.theme_stamp = stamp;
        }
        c.background = QString::from(&css(c.theme.background));
        c.foreground = QString::from(&css(c.theme.foreground));
        c.accent = QString::from(&css(c.theme.accent));
        c.ok_color = QString::from(&css(c.theme.nearest_hue(120.0)));
        c.error_color = QString::from(&css(c.theme.nearest_hue(0.0)));
        c
    }
}

#[allow(clippy::cast_possible_truncation)]
fn f(v: f64) -> f32 {
    v as f32
}

impl qobject::AlignmentController {
    pub fn document_record(&self) -> QString {
        crate::provenance_bridge::session_report(self.rust().session.as_ref(), self.rust().doc)
    }
    pub fn record_revision(&self) -> QString {
        crate::provenance_bridge::session_revision(self.rust().session.as_ref(), self.rust().doc)
    }

    pub fn accessible_page(&self, page: i32) -> QString {
        let result = self.with_doc(|d| basecraft_viewmodel::accessible::alignment(d, page));
        let value = result.map_or_else(
            || serde_json::json!({"page": 0, "pages": 1, "text": "No document loaded"}),
            |p| serde_json::json!({"page": p.page, "pages": p.pages, "text": p.text}),
        );
        QString::from(&value.to_string())
    }

    fn with_doc<T>(&self, f: impl FnOnce(&AlignmentDoc) -> T) -> Option<T> {
        let r = self.rust();
        match (&r.session, r.doc) {
            (Some(s), Some(d)) => Some(f(s.alignment(d))),
            _ => None,
        }
    }

    /// Rebuild the frame, status line and scroll bars.
    fn refresh(mut self: Pin<&mut Self>) {
        let (bytes, status, zoom, h, v, names) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = session.alignment(doc);
            r.view.build(d, &r.theme, &mut r.scratch);
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
        let work = {
            let mut r = self.as_mut().rust_mut();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = session.alignment(doc);
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
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let runs = work.1.gap_column_runs();
            let count: u64 = runs.iter().map(|(a, b)| b - a).sum();
            let _ = qt_thread.queue(move |mut this| {
                let current = {
                    let mut r = this.as_mut().rust_mut();
                    r.gap_scan_running = false;
                    let current = r.session.as_ref().zip(r.doc).is_some_and(|(s, d)| {
                        s.id() == work.0.0 && s.alignment(d).version() == work.0.1
                    });
                    if current {
                        r.gap_scan_key = Some(work.0);
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
            });
        });
    }

    /// A neutral hint. Messages stay brief: say what happened.
    fn notify(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().set_message_kind(QString::default());
        self.as_mut().set_message(QString::from(msg));
    }

    /// Something completed.
    fn done(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().set_message_kind(QString::from("ok"));
        self.as_mut().set_message(QString::from(msg));
    }

    /// Something failed.
    fn fail(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().set_message_kind(QString::from("error"));
        self.as_mut().set_message(QString::from(msg));
    }

    fn fail_addition(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().fail("Cannot add sequences");
        self.as_mut().addition_failed(QString::from(msg));
    }

    /// Apply an edit through the session; on failure, say why.
    fn edit(mut self: Pin<&mut Self>, op: Op) -> bool {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return false;
            };
            let result = session.apply(doc, op);
            r.view.sync(session.alignment(doc));
            result
        };
        match result {
            Ok(_) => {
                self.as_mut().set_dirty(true);
                self.as_mut().notify("");
                true
            }
            Err(e) => {
                self.fail(&e.to_string());
                false
            }
        }
    }

    pub fn export_svg(self: Pin<&mut Self>, path: &QString) {
        self.export_figure(path, 0, false);
    }

    pub fn export_figure(self: Pin<&mut Self>, path: &QString, scope: i32, publication: bool) {
        self.export_figure_with(path, scope, publication, None);
    }

    pub fn export_figure_png(
        self: Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
        width: f64,
    ) {
        self.export_figure_with(path, scope, publication, Some(width));
    }

    fn export_figure_with(
        mut self: Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
        png_width: Option<f64>,
    ) {
        if !(0..=2).contains(&scope) {
            self.fail("Unknown figure scope");
            return;
        }
        if scope == 1 && self.rust().view.selection.is_none() {
            self.fail("Select a region or clade first");
            return;
        }
        let target = PathBuf::from(path.to_string());
        let result = (|| -> basecraft_ops::Result<()> {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return Ok(());
            };
            let theme = if publication {
                Theme::publication()
            } else {
                r.theme.clone()
            };
            let mut view = if scope == 0 {
                r.view.clone()
            } else {
                r.view.figure_view(session.alignment(doc), scope == 1)
            };
            let mut frame = Frame::new();
            view.build(session.alignment(doc), &theme, &mut frame);
            let svg = basecraft_viewmodel::svg::frame_svg(
                &frame,
                view.width,
                view.height,
                &r.mono_font.to_string(),
                theme.background,
            );
            if let Some(width) = png_width {
                let bytes = crate::figure_png::render(&svg, width)
                    .map_err(basecraft_ops::Error::Invalid)?;
                session.export(doc, &target, "png", |w| w.write_all(&bytes))
            } else {
                session.export(doc, &target, "svg", |w| w.write_all(svg.as_bytes()))
            }
        })();
        match result {
            Ok(()) => self.done(&format!("Exported {}", file_name(&target))),
            Err(e) => self.fail(&format!("Export failed: {e}")),
        }
    }

    pub fn highlight_taxa(mut self: Pin<&mut Self>, names: &QString) {
        if let Ok(taxa) =
            serde_json::from_str::<std::collections::HashSet<String>>(&names.to_string())
        {
            self.as_mut().rust_mut().view.linked_taxa = taxa;
            self.refresh();
        }
    }

    fn emit_taxa(mut self: Pin<&mut Self>) {
        let taxa = {
            let r = self.rust();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let alignment = session.alignment(doc);
            let names: Vec<_> = r.view.selection.map_or_else(Vec::new, |block| {
                (block.rows.0..block.rows.1)
                    .map(|i| alignment.row(i).name.clone())
                    .collect()
            });
            serde_json::to_string(&names).unwrap_or_default()
        };
        if taxa != self.rust().last_taxa {
            self.as_mut().rust_mut().last_taxa.clone_from(&taxa);
            self.taxa_selected(QString::from(&taxa));
        }
    }

    pub fn create_delimitation(mut self: Pin<&mut Self>, method: &QString) -> QString {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            r.session
                .as_mut()
                .zip(r.doc)
                .ok_or_else(|| "No source open".to_string())
                .and_then(|(s, d)| {
                    crate::delimitation_bridge::create_workspace(s, d, &method.to_string())
                })
        };
        match result {
            Ok(path) => QString::from(path),
            Err(error) => {
                self.fail(&error);
                QString::default()
            }
        }
    }
    pub fn open_file(mut self: Pin<&mut Self>, path: &QString) -> bool {
        if self.rust().busy {
            self.fail("Wait for the running tool before replacing this document");
            return false;
        }
        let path = PathBuf::from(path.to_string());
        let mut session = match Session::recoverable(project_root(&path)) {
            Ok(session) => session,
            Err(error) => {
                self.fail(&error.to_string());
                return false;
            }
        };
        match session.open(&path) {
            Ok(doc) if session.is_alignment(doc) => {
                {
                    let mut r = self.as_mut().rust_mut();
                    let r = &mut *r;
                    r.view.sync(session.alignment(doc));
                    r.view.reset_zoom();
                    r.session = Some(session);
                    r.doc = Some(doc);
                }
                let abs = std::fs::canonicalize(&path).unwrap_or(path);
                self.as_mut()
                    .set_file_path(QString::from(&abs.to_string_lossy().into_owned()));
                self.as_mut().set_dirty(false);
                self.as_mut().notify("");
                self.refresh();
                true
            }
            Ok(_) => {
                self.fail(&format!("{} is not an alignment", path.display()));
                false
            }
            Err(e) => {
                self.fail(&format!("Can't open {}: {e}", path.display()));
                false
            }
        }
    }

    pub fn resize(mut self: Pin<&mut Self>, width: f64, height: f64) {
        self.as_mut().rust_mut().view.resize(f(width), f(height));
        self.refresh();
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

    fn history(mut self: Pin<&mut Self>, redo: bool) {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return;
            };
            let result = if redo {
                session.redo(doc)
            } else {
                session.undo(doc)
            };
            r.view.sync(session.alignment(doc));
            result
        };
        match result {
            Ok(_) => {
                self.as_mut().set_dirty(true);
                // Undoing or redoing makes the last message stale.
                self.as_mut().notify("");
            }
            Err(e) => self.as_mut().fail(&e.to_string()),
        }
        self.refresh();
    }

    pub fn undo(self: Pin<&mut Self>) {
        self.history(false);
    }

    pub fn redo(self: Pin<&mut Self>) {
        self.history(true);
    }

    pub fn save(mut self: Pin<&mut Self>) {
        let started = Instant::now();
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return;
            };
            session.save(doc, None)
        };
        match result {
            Ok(()) => {
                self.as_mut().set_dirty(false);
                self.done(&format!("Saved{}", took(started)));
            }
            Err(e) => self.fail(&format!("Save failed: {e}")),
        }
    }

    pub fn save_as(mut self: Pin<&mut Self>, path: &QString) -> bool {
        let started = Instant::now();
        let target = PathBuf::from(path.to_string());
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return false;
            };
            session.save(doc, Some(&target))
        };
        if let Err(e) = result {
            self.fail(&format!("Save failed: {e}"));
            return false;
        }
        // Carry on in the new file; its log starts with opening it.
        if !self.as_mut().open_file(path) {
            return false;
        }
        self.done(&format!("Saved as {}{}", file_name(&target), took(started)));
        true
    }

    pub fn check_theme(mut self: Pin<&mut Self>) {
        let font = QString::from(&monospace_font());
        if font != self.rust().mono_font {
            self.as_mut().set_mono_font(font);
        }

        let Some(path) = omarchy_theme_path() else {
            return;
        };
        let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if stamp == self.rust().theme_stamp {
            return;
        }
        if let Some((theme, stamp)) = load_theme() {
            let (bg, fg, ac) = (
                css(theme.background),
                css(theme.foreground),
                css(theme.accent),
            );
            {
                let mut r = self.as_mut().rust_mut();
                r.theme = theme;
                r.theme_stamp = stamp;
            }
            self.as_mut().set_background(QString::from(&bg));
            self.as_mut().set_foreground(QString::from(&fg));
            self.as_mut().set_accent(QString::from(&ac));
            let (ok, err) = {
                let t = &self.rust().theme;
                (css(t.nearest_hue(120.0)), css(t.nearest_hue(0.0)))
            };
            self.as_mut().set_ok_color(QString::from(&ok));
            self.as_mut().set_error_color(QString::from(&err));
            self.refresh();
        }
    }

    pub fn flush(mut self: Pin<&mut Self>) {
        if let Some(s) = &mut self.as_mut().rust_mut().session {
            let _ = s.flush();
        }
    }

    pub fn memory_report(&self) -> QString {
        QString::from(&memory_text())
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

    pub fn fit(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.fit();
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
        let region = self.as_mut().rust_mut().view.context_press(f(x), f(y));
        self.as_mut().refresh();
        let b = self.rust().view.block();
        let name = self
            .with_doc(|d| d.row(self.rust().view.cursor.0).name.clone())
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
        let text = self
            .with_doc(|d| self.rust().view.hover_text(f(x), f(y), d))
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
            .unwrap_or_else(|| Err(basecraft_ops::Error::Invalid("Alignment closed".into())));
        QString::from(&match result {
            Ok(queries) => serde_json::to_string(&queries).unwrap_or_default(),
            Err(error) => serde_json::json!({"error":error.to_string()}).to_string(),
        })
    }

    pub fn selection_text(&self) -> QString {
        let text = self
            .with_doc(|d| self.rust().view.selection_fasta(d))
            .unwrap_or_default();
        QString::from(&text)
    }

    pub fn select_all(mut self: Pin<&mut Self>) {
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
        let result = (|| -> Result<(tempfile::TempDir, PathBuf), String> {
            let dir = tempfile::Builder::new()
                .prefix("basecraft-review-reference-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            let path = dir.path().join("reference.afa");
            let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            self.with_doc(|doc| basecraft_ops::files::write_alignment(doc, &mut file))
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
        let rows = match basecraft_ops::alignment_add::parse_sequences(
            &text.to_string(),
            &name.to_string(),
        ) {
            Ok(rows) => rows,
            Err(e) => {
                self.fail_addition(&e);
                return;
            }
        };
        match self.with_doc(|doc| doc.validate_additions(&rows)) {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                self.fail_addition(&error.to_string());
                return;
            }
            None => {
                self.fail_addition("No alignment open");
                return;
            }
        }
        let duplicates = self
            .with_doc(|doc| doc.duplicate_sequence_names(&rows))
            .unwrap_or_default();
        if !duplicates.is_empty() {
            self.fail_addition(&format!(
                "These sequence IDs are already in the alignment:\n\n{}",
                duplicates
                    .iter()
                    .map(|n| format!("• {n}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
            return;
        }
        if let Some(evidence) = evidence {
            let result = (|| -> Result<(), String> {
                if rows.len() != 1 {
                    return Err("Select one reviewed specimen at a time".into());
                }
                let mut r = self.as_mut().rust_mut();
                let r = &mut *r;
                let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                    return Err("No alignment is open".into());
                };
                basecraft_ops::sanger_review::record_addition(session, doc, &evidence, &rows[0].1)
                    .map_err(|e| e.to_string())
            })();
            if let Err(error) = result {
                self.fail_addition(&error);
                return;
            }
        }
        if !mafft {
            if self.as_mut().edit(Op::AlnAddSequences {
                rows,
                reference: None,
            }) {
                self.as_mut().done("Added sequences");
                self.as_mut().sequences_added();
            } else {
                let message = self.rust().message.to_string();
                self.as_mut().fail_addition(&message);
            }
            self.refresh();
            return;
        }
        let result = (|| -> Result<(FileJob, tempfile::TempDir), String> {
            let dir = tempfile::Builder::new()
                .prefix("basecraft-add-sequences-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            let r = self.rust();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return Err("No alignment open".into());
            };
            let mut job = FileJob::from_alignment(
                session,
                doc,
                (&plugin_dir("mafft"), "add_sequences"),
                AlignmentInput::Aligned,
                dir.path().join("addition.json"),
            )
            .map_err(|e| e.to_string())?;
            job.params.insert("rows".into(), json!(rows));
            job.params.insert("orientation".into(), json!(orientation));
            job.params.insert("keep".into(), json!(keep));
            Ok((job, dir))
        })();
        let (job, dir) = match result {
            Ok(value) => value,
            Err(e) => {
                self.fail_addition(&e);
                return;
            }
        };
        let generation = self
            .rust()
            .session
            .as_ref()
            .zip(self.rust().doc)
            .map(|(session, doc)| session.generation(doc));
        self.as_mut().set_busy(true);
        self.as_mut().notify("Adding sequences with MAFFT…");
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = job.run();
            let _ = qt_thread.queue(move |mut this| {
                this.as_mut().set_busy(false);
                let result = (|| -> Result<serde_json::Value, String> {
                    let res = result.map_err(|e| e.to_string())?;
                    {
                        let mut r = this.as_mut().rust_mut();
                        let r = &mut *r;
                        let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                            return Err("Alignment was closed".into());
                        };
                        if Some(session.generation(doc)) != generation {
                            return Err("Alignment changed during addition; try again".into());
                        }
                        job.record(session, doc, &res).map_err(|e| e.to_string())?;
                    }
                    let bytes = std::fs::read(&job.output).map_err(|e| e.to_string())?;
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
                })();
                match result {
                    Ok(value) => {
                        let count = value["insertions"].as_i64().unwrap_or(0);
                        if count > 0 {
                            this.as_mut().notify("New bases need reference gaps");
                            this.as_mut()
                                .addition_needs_gaps(i32::try_from(count).unwrap_or(i32::MAX));
                        } else {
                            let parsed = serde_json::from_value::<Vec<(String, String)>>(
                                value["rows"].clone(),
                            )
                            .and_then(|rows| {
                                serde_json::from_value::<Vec<String>>(value["reference"].clone())
                                    .map(|reference| (rows, reference))
                            });
                            match parsed {
                                Ok((rows, reference)) => {
                                    if this.as_mut().edit(Op::AlnAddSequences {
                                        rows,
                                        reference: Some(reference),
                                    }) {
                                        this.as_mut().done("Added sequences");
                                        this.as_mut().sequences_added();
                                    } else {
                                        let message = this.rust().message.to_string();
                                        this.as_mut().fail_addition(&message);
                                    }
                                    this.as_mut().refresh();
                                }
                                Err(e) => this.as_mut().fail_addition(&e.to_string()),
                            }
                        }
                    }
                    Err(e) => this.as_mut().fail_addition(&e),
                }
                drop(dir);
            });
        });
    }

    pub fn align_mafft(self: Pin<&mut Self>) {
        self.run_tool(&Tool {
            name: "MAFFT".into(),
            plugin: "mafft".into(),
            operation: "align".into(),
            input: AlignmentInput::Ungapped,
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
            input: AlignmentInput::Aligned,
            output: ("trimmed".into(), "fasta".into()),
            min_rows: 2,
            working: "Trimming with trimAl…".into(),
            params,
        });
    }

    /// What a settings form may depend on: the alignment's sequence type.
    fn form_context(&self) -> serde_json::Map<String, serde_json::Value> {
        let protein = self.with_doc(AlignmentDoc::is_protein).unwrap_or(false);
        [(
            "sequence".to_string(),
            serde_json::json!(if protein { "protein" } else { "dna" }),
        )]
        .into_iter()
        .collect()
    }

    pub fn tool_form(&self, plugin: &QString, form: &QString) -> QString {
        let context = self.form_context();
        let value =
            basecraft_plugin_host::manifest::Manifest::load(&plugin_dir(&plugin.to_string()))
                .map_err(|e| e.to_string())
                .and_then(|m| {
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
            let manifest = basecraft_plugin_host::manifest::Manifest::load(&plugin_dir(&plugin))
                .map_err(|e| e.to_string())?;
            let form = manifest
                .form(&form.to_string())
                .ok_or_else(|| format!("{} has no {form} settings", manifest.name))?;
            let given: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&values.to_string()).map_err(|e| e.to_string())?;
            let mut params = form.for_context(&self.form_context()).parameters(&given)?;
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
                input: AlignmentInput::Aligned,
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
        if let Some(control) = self.rust().bootstrap_control.clone() {
            self.as_mut().set_bootstrap_cancelling(true);
            self.as_mut()
                .set_bootstrap_status(QString::from("Cancelling after the current replicate…"));
            let qt_thread = self.qt_thread();
            std::thread::spawn(move || {
                if let Err(error) = std::fs::write(control.directory.path().join("cancel"), b"") {
                    let _ = qt_thread.queue(move |mut this| {
                        if this
                            .rust()
                            .bootstrap_control
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &control))
                        {
                            this.as_mut().set_bootstrap_cancelling(false);
                            this.fail(&format!("Could not request cancellation: {error}"));
                        }
                    });
                }
            });
        }
    }

    /// Run a plugin on this alignment in the background, writing a new file
    /// beside it, then log the run and ask for the file to be opened.
    fn run_tool(mut self: Pin<&mut Self>, tool: &Tool) {
        if self.rust().busy {
            return;
        }
        let name = tool.name.clone();
        if self.with_doc(AlignmentDoc::row_count).unwrap_or(0) < tool.min_rows {
            self.fail(&format!(
                "{name} needs at least {} sequences",
                tool.min_rows
            ));
            return;
        }
        let job = {
            let r = self.rust();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let (suffix, ext) = (&tool.output.0, &tool.output.1);
            let output = output_beside(&PathBuf::from(r.file_path.to_string()), suffix, ext);
            FileJob::from_alignment(
                session,
                doc,
                (&plugin_dir(&tool.plugin), &tool.operation),
                tool.input,
                output,
            )
        };
        let mut job = match job {
            Ok(job) => job,
            Err(e) => {
                self.fail(&format!("{name}: {e}"));
                return;
            }
        };
        job.params.clone_from(&tool.params);
        let bootstrap_control = if tool.plugin == "distance"
            && tool.operation == "tree"
            && tool
                .params
                .get("bootstrap")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|n| n > 0)
        {
            let directory = match tempfile::Builder::new()
                .prefix("basecraft-bootstrap-")
                .tempdir()
            {
                Ok(directory) => directory,
                Err(error) => {
                    self.fail(&format!("Bootstrap progress: {error}"));
                    return;
                }
            };
            let control = Arc::new(BootstrapControl {
                directory,
                finished: AtomicBool::new(false),
            });
            job.params.insert(
                "progress".into(),
                json!(control.directory.path().join("progress.json")),
            );
            job.params.insert(
                "cancel_file".into(),
                json!(control.directory.path().join("cancel")),
            );
            self.as_mut().rust_mut().bootstrap_control = Some(Arc::clone(&control));
            self.as_mut().set_bootstrap_running(true);
            self.as_mut().set_bootstrap_cancelling(false);
            self.as_mut().set_bootstrap_progress(0.);
            self.as_mut()
                .set_bootstrap_status(QString::from("Preparing bootstrap…"));
            let qt_thread = self.qt_thread();
            let monitor = Arc::clone(&control);
            std::thread::spawn(move || {
                let mut previous = String::new();
                while !monitor.finished.load(Ordering::Acquire) {
                    if let Ok(text) =
                        std::fs::read_to_string(monitor.directory.path().join("progress.json"))
                    {
                        if text != previous {
                            if let Ok(progress) = serde_json::from_str::<serde_json::Value>(&text) {
                                let attempted = progress["attempted"].as_f64().unwrap_or(0.);
                                let requested =
                                    progress["requested"].as_f64().unwrap_or(1.).max(1.);
                                let completed = progress["completed"].as_u64().unwrap_or(0);
                                let failures = progress["failed_replicates"].as_u64().unwrap_or(0);
                                let label = format!(
                                    "Bootstrap {attempted:.0}/{requested:.0}; {completed} complete, {failures} failed"
                                );
                                let generation = Arc::clone(&monitor);
                                let _ =
                                    qt_thread.queue(move |mut this| {
                                        if this.rust().bootstrap_control.as_ref().is_some_and(
                                            |current| Arc::ptr_eq(current, &generation),
                                        ) {
                                            this.as_mut()
                                                .set_bootstrap_progress(attempted / requested);
                                            if !this.rust().bootstrap_cancelling {
                                                this.as_mut()
                                                    .set_bootstrap_status(QString::from(&label));
                                            }
                                        }
                                    });
                            }
                            previous = text;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            Some(control)
        } else {
            None
        };
        self.as_mut().set_busy(true);
        self.as_mut().notify(&tool.working);
        let started = Instant::now();
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = job.run();
            if let Some(control) = &bootstrap_control {
                control.finished.store(true, Ordering::Release);
            }
            let _ = qt_thread.queue(move |mut this| {
                if let Some(control) = &bootstrap_control {
                    if !this
                        .rust()
                        .bootstrap_control
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, control))
                    {
                        return;
                    }
                    this.as_mut().rust_mut().bootstrap_control = None;
                    this.as_mut().set_bootstrap_running(false);
                    this.as_mut().set_bootstrap_cancelling(false);
                }
                this.as_mut().set_busy(false);
                let terminal = {
                    let mut r = this.as_mut().rust_mut();
                    let r = &mut *r;
                    if let (Some(session), Some(doc)) = (&mut r.session, r.doc) {
                        job.record_terminal(session, doc, &result, started.elapsed().as_secs_f64())
                    } else {
                        Ok(())
                    }
                };
                let result = terminal.and(result).and_then(|res| {
                    let mut r = this.as_mut().rust_mut();
                    let r = &mut *r;
                    if let (Some(session), Some(doc)) = (&mut r.session, r.doc) {
                        job.record(session, doc, &res)?;
                    }
                    Ok(res)
                });
                match result {
                    Ok(res) => {
                        let msg = res.message.unwrap_or_else(|| "Done".into());
                        this.as_mut()
                            .done(&format!("{msg} with {name}{}", took(started)));
                        if job.operation == "matrix" {
                            this.as_mut()
                                .done(&format!("{msg}; saved {}", job.output.display()));
                            return;
                        }
                        let components: Vec<_> = res
                            .outputs
                            .iter()
                            .filter(|p| {
                                std::path::Path::new(p)
                                    .extension()
                                    .is_some_and(|e| e.eq_ignore_ascii_case("nwk"))
                                    && p.contains(".component-")
                            })
                            .collect();
                        if job.operation == "tree" && !components.is_empty() {
                            for path in components {
                                this.as_mut().file_created(QString::from(path));
                            }
                        } else {
                            let path = job.output.to_string_lossy().into_owned();
                            this.as_mut().file_created(QString::from(&path));
                        }
                    }
                    Err(e) => this.as_mut().fail(&format!("{name}: {}", short_error(&e))),
                }
            });
        });
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
        let op = self
            .with_doc(|d| self.rust().view.remove_left_op(d, self.rust().insert_mode))
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
        let started = Instant::now();
        let before = self.with_doc(AlignmentDoc::width).unwrap_or(0);
        let runs = if self.rust().gap_column_count >= 0.0 {
            self.rust().gap_runs.clone()
        } else {
            self.with_doc(AlignmentDoc::gap_column_runs)
                .unwrap_or_default()
        };
        let mut view = self.rust().view.clone();
        view.map_deleted_columns(&runs);
        if self.as_mut().edit(Op::AlnRemoveGapColumns {}) {
            {
                let mut r = self.as_mut().rust_mut();
                if let (Some(session), Some(doc)) = (&r.session, r.doc) {
                    view.sync(session.alignment(doc));
                }
                r.view = view;
            }
            let after = self.with_doc(AlignmentDoc::width).unwrap_or(0);
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
        let name = self
            .with_doc(|d| d.row(self.rust().view.cursor.0).name.clone())
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

/// A plugin error without the transport's wrapping: "MAFFT failed: …".
fn short_error(e: &basecraft_plugin_host::Error) -> String {
    match e {
        basecraft_plugin_host::Error::Remote { message, .. } => crate::bridge::error_summary(
            message
                .split_once(": ")
                .map_or(message.as_str(), |(_, m)| m),
        ),
        other => other.to_string(),
    }
}
