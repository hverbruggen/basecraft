//! Chromatogram inspection and asynchronous registered Sanger operations.

#[path = "sanger_work.rs"]
mod work;

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for SangerController {}
    impl cxx_qt::Initialize for SangerController {}
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
        #[qproperty(QString, message_kind)]
        #[qproperty(QString, ok_color)]
        #[qproperty(QString, error_color)]
        #[qproperty(QString, background)]
        #[qproperty(QString, foreground)]
        #[qproperty(QString, accent)]
        #[qproperty(QString, mono_font)]
        #[qproperty(QString, file_path)]
        #[qproperty(QString, zoom_text)]
        #[qproperty(f64, trace_scroll_pos)]
        #[qproperty(f64, trace_scroll_size)]
        #[qproperty(f64, hscroll_pos)]
        #[qproperty(f64, hscroll_size)]
        #[qproperty(bool, dirty)]
        #[qproperty(bool, busy)]
        #[qproperty(bool, review_mode)]
        #[qproperty(f64, review_height)]
        #[qproperty(QString, proposed_name)]
        #[qproperty(QString, export_path)]
        #[qproperty(QString, trace_lanes)]
        #[qproperty(QString, review_sequence)]
        #[qproperty(QString, review_decision)]
        #[qproperty(QString, alignment_context)]
        #[qproperty(QString, review_details)]
        #[qproperty(QString, mask_settings)]
        #[qproperty(i32, selected_column)]
        #[qproperty(i32, consensus_columns)]
        #[qproperty(i32, unresolved)]
        #[qproperty(i32, signal_threshold)]
        #[qproperty(QString, input_paths)]
        #[qproperty(QString, call_sets)]
        #[qproperty(QString, selected_call_set)]
        #[qproperty(bool, calling)]
        #[qproperty(QString, review_settings)]
        /// A review is being prepared from reads (see `sanger_flow`).
        #[qproperty(bool, preparing)]
        #[qproperty(QString, preparation_error)]
        /// The reads of a failed preparation, one per line, or "".
        #[qproperty(QString, failed_reads)]
        type SangerController = super::SangerRust;

        /// Prepare a review from `paths` (JSON list), oriented against
        /// `reference` when it is not empty. False when one is already running.
        #[qinvokable]
        fn start_review(
            self: Pin<&mut SangerController>,
            paths: &QString,
            reference: &QString,
            cutoff: i32,
        ) -> bool;
        /// Orient the open review against another reference alignment.
        #[qinvokable]
        fn review_context(self: Pin<&mut SangerController>, reference: &QString);
        #[qinvokable]
        fn retry_review(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn dismiss_failure(self: Pin<&mut SangerController>);
        /// Stop the running stage and any background calling.
        #[qinvokable]
        fn cancel_work(self: Pin<&mut SangerController>);
        #[qsignal]
        #[cxx_name = "reviewReady"]
        fn review_ready(self: Pin<&mut SangerController>, reference: QString);

        #[qinvokable]
        fn find_query(self: Pin<&mut SangerController>, query: &QString, reverse: bool);
        #[qinvokable]
        fn find_step(self: Pin<&mut SangerController>, direction: i32);

        #[qinvokable]
        fn cancel_stage(self: Pin<&mut SangerController>);
        #[qsignal]
        #[cxx_name = "stageFinished"]
        fn stage_finished(
            self: Pin<&mut SangerController>,
            stage: QString,
            success: bool,
            payload: QString,
        );
        #[qinvokable]
        fn cancel_calls(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn compute_calls(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn select_calls(self: Pin<&mut SangerController>, id: &QString) -> bool;
        #[qinvokable]
        fn document_record(self: &SangerController) -> QString;
        #[qinvokable]
        fn record_revision(self: &SangerController) -> QString;
        #[qinvokable]
        fn accessible_page(self: &SangerController, page: i32) -> QString;
        #[qinvokable]
        fn trace_height(self: Pin<&mut SangerController>, read: i32, height: f64);
        #[qinvokable]
        fn trace_gain(self: Pin<&mut SangerController>, read: i32, gain: f64);
        #[qinvokable]
        fn mask_ends(self: Pin<&mut SangerController>, start: i32, end: i32) -> bool;
        #[qinvokable]
        fn mask_reference(self: Pin<&mut SangerController>) -> bool;
        #[qinvokable]
        fn find_primers(
            self: Pin<&mut SangerController>,
            forward: &QString,
            reverse: &QString,
            overlap: i32,
            errors: i32,
        ) -> bool;
        #[qinvokable]
        fn apply_primers(self: Pin<&mut SangerController>, proposal: &QString) -> bool;
        #[qinvokable]
        fn apply_review_settings(self: Pin<&mut SangerController>, settings_json: &QString)
        -> bool;
        #[qinvokable]
        fn open_file(self: Pin<&mut SangerController>, path: &QString) -> bool;
        #[qinvokable]
        fn orient_review(self: Pin<&mut SangerController>, alignment: &QString) -> bool;
        #[qinvokable]
        fn single_review(self: Pin<&mut SangerController>, signal_threshold: i32) -> bool;
        #[qinvokable]
        fn reviewed_sequence(self: Pin<&mut SangerController>) -> QString;
        #[qinvokable]
        fn reviewed_evidence(self: Pin<&mut SangerController>) -> QString;
        #[qinvokable]
        fn reviewed_fasta(self: Pin<&mut SangerController>, name: &QString) -> QString;
        #[qinvokable]
        fn draft_consensus(
            self: Pin<&mut SangerController>,
            additional_paths_json: &QString,
            signal_threshold: i32,
        ) -> bool;
        /// Retain the request reference before starting any asynchronous stage.
        #[qinvokable]
        fn retain_reference(
            self: Pin<&mut SangerController>,
            anchor: &QString,
            source: &QString,
        ) -> QString;
        /// Prepare a complete multi-read selection without first installing a single read.
        #[qinvokable]
        fn prepare_reads(
            self: Pin<&mut SangerController>,
            paths_json: &QString,
            signal_threshold: i32,
        ) -> bool;
        #[qinvokable]
        fn open_review(self: Pin<&mut SangerController>, path: &QString) -> bool;
        #[qinvokable]
        fn save_review(self: Pin<&mut SangerController>, path: &QString) -> bool;
        #[qinvokable]
        fn review_set(self: Pin<&mut SangerController>, choice: &QString, base: &QString) -> bool;
        #[qinvokable]
        fn review_undo(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn review_redo(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn review_accept_unflagged(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn export_consensus(
            self: Pin<&mut SangerController>,
            path: &QString,
            label: &QString,
        ) -> bool;
        #[qinvokable]
        fn place_alignment(self: Pin<&mut SangerController>, path: &QString) -> bool;
        #[qinvokable]
        fn next_issue(self: Pin<&mut SangerController>, forward: bool);
        #[qinvokable]
        fn go_to_column(
            self: Pin<&mut SangerController>,
            reference: bool,
            column: &QString,
        ) -> bool;
        #[qinvokable]
        fn hscroll_to(self: Pin<&mut SangerController>, fraction: f64);
        #[qinvokable]
        fn hover(self: Pin<&mut SangerController>, x: f64, y: f64) -> QString;
        #[qinvokable]
        fn resize(self: Pin<&mut SangerController>, width: f64, height: f64);
        #[qinvokable]
        fn wheel(self: Pin<&mut SangerController>, x: f64, delta: f64);
        #[qinvokable]
        fn press(self: Pin<&mut SangerController>, x: f64, y: f64);
        #[qinvokable]
        fn context_column(self: Pin<&mut SangerController>, x: f64, y: f64) -> i32;
        #[qinvokable]
        fn pan(self: Pin<&mut SangerController>, pixels: f64);
        #[qinvokable]
        fn next_call(self: Pin<&mut SangerController>, forward: bool);
        #[qinvokable]
        fn zoom_in(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn set_zoom_percent(self: Pin<&mut SangerController>, percent: f64);
        #[qinvokable]
        fn zoom_out(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn scroll_trace(self: Pin<&mut SangerController>, fraction: f64);
        #[qinvokable]
        fn fit(self: Pin<&mut SangerController>);
        #[qinvokable]
        fn export_figure(
            self: Pin<&mut SangerController>,
            path: &QString,
            scope: i32,
            publication: bool,
        );
        #[qinvokable]
        fn export_figure_png(
            self: Pin<&mut SangerController>,
            path: &QString,
            scope: i32,
            publication: bool,
            width: f64,
        );
        #[qinvokable]
        fn save(self: Pin<&mut SangerController>);
    }
}

use basecraft_viewmodel::sanger_flow::{Stage, Step};
use std::path::{Path, PathBuf};

use basecraft_io::abif::Trace;
use basecraft_ops::sanger_review::{MaskSettings, ReviewChoice, ReviewOp, SangerReview};
use basecraft_viewmodel::sanger::SangerView;
use basecraft_viewmodel::sanger_review::SangerReviewView;
use basecraft_viewmodel::{Frame, Theme, svg};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QString};

use work::{load_draft_review, session_view, sync_review_view};

// Independent generated QML properties describe document state and caller activity.
#[allow(clippy::struct_excessive_bools)]
pub struct SangerRust {
    find: basecraft_viewmodel::find::Session,
    find_status: QString,
    find_count: i32,
    find_row: i32,
    find_hit_y: f64,
    find_nucleotide: bool,
    find_rows: QString,
    find_reveals: i32,
    history_revision: QString,
    find_source_key: String,
    overview_content: QString,
    overview_matches: QString,
    overview_match_cache: basecraft_viewmodel::overview::Cache,
    overview_cache: basecraft_viewmodel::overview::Cache,
    frame: QByteArray,
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
    zoom_text: QString,
    trace_scroll_pos: f64,
    trace_scroll_size: f64,
    hscroll_pos: f64,
    hscroll_size: f64,
    dirty: bool,
    busy: bool,
    review_mode: bool,
    review_height: f64,
    proposed_name: QString,
    export_path: QString,
    trace_lanes: QString,
    review_sequence: QString,
    review_decision: QString,
    alignment_context: QString,
    review_details: QString,
    mask_settings: QString,
    selected_column: i32,
    consensus_columns: i32,
    unresolved: i32,
    signal_threshold: i32,
    input_paths: QString,
    review_settings: QString,
    call_sets: QString,
    selected_call_set: QString,
    calling: bool,
    pending_calls: bool,
    stage: Option<work::Stage>,
    trace: Option<Trace>,
    review: Option<SangerReviewView>,
    review_session: Option<work::ReviewDocument>,
    review_path: Option<PathBuf>,
    view: SangerView,
    theme: Theme,
    scratch: Frame,
    flow: basecraft_viewmodel::sanger_flow::Flow,
    preparing: bool,
    preparation_error: QString,
    failed_reads: QString,
}

impl Default for SangerRust {
    fn default() -> Self {
        Self {
            find: basecraft_viewmodel::find::Session::default(),
            find_status: QString::default(),
            find_count: 0,
            find_row: -1,
            find_hit_y: 0.0,
            find_nucleotide: false,
            find_rows: QString::from("[]"),
            find_reveals: 0,
            history_revision: QString::default(),
            find_source_key: String::new(),
            overview_content: QString::from("{\"shapes\":[]}"),
            overview_matches: QString::from("[]"),
            overview_match_cache: basecraft_viewmodel::overview::Cache::default(),
            overview_cache: basecraft_viewmodel::overview::Cache::default(),
            frame: QByteArray::default(),
            status: QString::default(),
            message: QString::default(),
            message_kind: QString::default(),
            ok_color: QString::default(),
            error_color: QString::default(),
            background: QString::default(),
            foreground: QString::default(),
            accent: QString::default(),
            mono_font: QString::default(),
            file_path: QString::default(),
            zoom_text: QString::default(),
            trace_scroll_pos: 0.0,
            trace_scroll_size: 1.0,
            hscroll_pos: 0.0,
            hscroll_size: 1.0,
            dirty: false,
            busy: false,
            review_mode: false,
            review_height: 0.0,
            proposed_name: QString::default(),
            export_path: QString::default(),
            trace_lanes: QString::from("[]"),
            review_sequence: QString::default(),
            review_decision: QString::default(),
            alignment_context: QString::default(),
            review_details: QString::default(),
            mask_settings: QString::from("{}"),
            selected_column: 1,
            consensus_columns: 1,
            unresolved: 0,
            signal_threshold: 20,
            input_paths: QString::default(),
            review_settings: QString::from(
                serde_json::to_string(&basecraft_docs::sanger::ReviewSettings::default()).unwrap(),
            ),
            call_sets: QString::from("[]"),
            selected_call_set: QString::default(),
            calling: false,
            pending_calls: false,
            stage: None,
            trace: None,
            review: None,
            review_session: None,
            review_path: None,
            view: SangerView::default(),
            theme: Theme::default(),
            scratch: Frame::new(),
            flow: basecraft_viewmodel::sanger_flow::Flow::default(),
            preparing: false,
            preparation_error: QString::default(),
            failed_reads: QString::default(),
        }
    }
}

impl qobject::SangerController {
    fn open_draft(self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        let path = PathBuf::from(path.to_string());
        let result = load_draft_review(&path);
        self.install_review_result(&path, result, false)
    }

    fn review_payload(&self) -> Option<&SangerReview> {
        self.rust()
            .review_session
            .as_ref()
            .and_then(work::ReviewDocument::review)
    }

    fn call_error(mut self: std::pin::Pin<&mut Self>, error: &str) {
        self.as_mut().set_calling(false);
        self.as_mut().set_message_kind(QString::from("fail"));
        self.set_message(QString::from(format!(
            "Alternative calls: {error}. Instrument review remains available."
        )));
    }

    pub fn cancel_calls(mut self: std::pin::Pin<&mut Self>) {
        self.as_mut().rust_mut().pending_calls = false;
        if self.rust().stage.is_none() {
            self.as_mut().set_calling(false);
        }
        self.cancel_stage();
    }

    pub fn cancel_stage(mut self: std::pin::Pin<&mut Self>) {
        if let Some(stage) = &self.rust().stage {
            stage.work.cancel();
            self.as_mut().set_message_kind(QString::from("notify"));
            self.set_message(QString::from("Cancelling Sanger operation"));
        }
    }

    fn submit_stage(mut self: std::pin::Pin<&mut Self>, request: work::Request) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        self.as_mut().rust_mut().pending_calls = false;
        let single = request.name() == "single";
        let message = request.message();
        let session = self.review_payload().map(SangerReview::fork_for_operation);
        match work::start(self.qt_thread(), request, session) {
            Ok(stage) => {
                self.as_mut().rust_mut().stage = Some(stage);
                self.as_mut().set_busy(true);
                self.as_mut().set_calling(single);
                self.as_mut().set_message_kind(QString::from("notify"));
                self.set_message(QString::from(message));
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.set_message(QString::from(error));
                false
            }
        }
    }

    fn stage_done(
        mut self: std::pin::Pin<&mut Self>,
        id: u64,
        result: Result<work::Completion, String>,
    ) {
        let Some(stage) = self.rust().stage.as_ref().filter(|stage| stage.id == id) else {
            return;
        };
        let name = stage.name;
        let cancelled = stage.work.cancelled();
        self.as_mut().rust_mut().stage = None;
        self.as_mut().set_busy(false);
        self.as_mut().set_calling(false);
        let result = if cancelled {
            Err("Sanger operation cancelled; previous review preserved".into())
        } else {
            result
        };
        let result = result.and_then(|completed| {
            let mut r = self.as_mut().rust_mut();
            if name != "prepare"
                && let Some(document) = r.review_session.as_mut()
            {
                document.update(&completed.session)?;
            } else {
                r.review_session = Some(work::ReviewDocument::new(&completed.session)?);
            }
            Ok(completed)
        });
        let (success, payload) = match result {
            Ok(mut completed) => {
                {
                    let mut r = self.as_mut().rust_mut();
                    completed.view.resize(r.view.width, r.view.height);
                    r.overview_cache = basecraft_viewmodel::overview::Cache::default();
                    r.review = Some(completed.view);
                    if name == "prepare" {
                        r.review_path = None;
                        r.trace = None;
                    }
                }
                if name == "prepare" {
                    self.as_mut()
                        .set_file_path(QString::from(completed.path.to_string_lossy().as_ref()));
                }
                self.as_mut().set_dirty(true);
                self.as_mut().refresh();
                self.as_mut().set_message_kind(QString::from("done"));
                self.as_mut()
                    .set_message(QString::from(if name == "single" {
                        "Computed calls retained. Choose a call set to compare and review."
                    } else {
                        "Sanger stage completed"
                    }));
                (true, completed.payload)
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from(if cancelled {
                    "notify"
                } else {
                    "fail"
                }));
                self.as_mut().set_message(QString::from(error));
                (false, String::new())
            }
        };
        self.as_mut()
            .stage_finished(QString::from(name), success, QString::from(payload));
        if let Some(step) = self.as_mut().rust_mut().flow.finished(name, success) {
            self.run_step(step);
        }
    }

    pub fn start_review(
        mut self: std::pin::Pin<&mut Self>,
        paths: &QString,
        reference: &QString,
        cutoff: i32,
    ) -> bool {
        let paths: Vec<String> = serde_json::from_str(&paths.to_string()).unwrap_or_default();
        let first = paths.first().cloned().unwrap_or_default();
        if self.rust().busy {
            return false;
        }
        let Some(start) = self
            .as_mut()
            .rust_mut()
            .flow
            .start(paths, reference.to_string(), cutoff)
        else {
            return false;
        };
        if self.rust().calling {
            self.as_mut().cancel_calls();
        }
        // The reference is frozen before any stage runs, so a retry reads
        // the same bytes.
        if !reference.is_empty() {
            let retained = self
                .as_mut()
                .retain_reference(&QString::from(&first), reference);
            if retained.is_empty() {
                self.run_step(Step::Fail);
                return true;
            }
            self.as_mut().rust_mut().flow.retain(retained.to_string());
        }
        if let Some(path) = start.open {
            if !self.as_mut().open_file(&QString::from(&path)) {
                self.run_step(Step::Fail);
                return true;
            }
            if self.rust().calling {
                self.as_mut().cancel_calls();
            }
        }
        self.run_step(start.then);
        true
    }

    pub fn review_context(mut self: std::pin::Pin<&mut Self>, reference: &QString) {
        if self.rust().busy || self.rust().calling {
            return;
        }
        let paths = serde_json::from_str(&self.rust().input_paths.to_string()).unwrap_or_default();
        let cutoff = self.rust().signal_threshold;
        if let Some(step) =
            self.as_mut()
                .rust_mut()
                .flow
                .context(paths, reference.to_string(), cutoff)
        {
            self.run_step(step);
        }
    }

    pub fn retry_review(mut self: std::pin::Pin<&mut Self>) {
        if let Some((paths, reference, cutoff)) = self.as_mut().rust_mut().flow.retry() {
            let paths = serde_json::to_string(&paths).unwrap_or_default();
            self.start_review(&QString::from(&paths), &QString::from(&reference), cutoff);
        }
    }

    pub fn dismiss_failure(mut self: std::pin::Pin<&mut Self>) {
        self.as_mut().rust_mut().flow.finish();
        self.publish_flow();
    }

    pub fn cancel_work(mut self: std::pin::Pin<&mut Self>) {
        self.as_mut().rust_mut().flow.cancel();
        if self.rust().busy {
            self.as_mut().cancel_stage();
        }
        if self.rust().calling {
            self.cancel_calls();
        }
    }

    fn run_step(mut self: std::pin::Pin<&mut Self>, step: Step) {
        match step {
            Step::Submit(stage) => {
                let Some(request) = self.as_mut().rust_mut().flow.submitting(stage).cloned() else {
                    return;
                };
                let reference = QString::from(&request.reference);
                let accepted = match stage {
                    Stage::Prepare => {
                        let paths = serde_json::to_string(&request.paths).unwrap_or_default();
                        self.as_mut()
                            .prepare_reads(&QString::from(&paths), request.cutoff)
                    }
                    Stage::Single => self.as_mut().single_review(request.cutoff),
                    Stage::Orient => self.as_mut().orient_review(&reference),
                    Stage::Place => self.as_mut().place_alignment(&reference),
                };
                if !accepted {
                    self.as_mut().run_step(Step::Fail);
                }
            }
            Step::Finish { reference } => {
                self.as_mut().rust_mut().flow.finish();
                self.as_mut().review_ready(QString::from(&reference));
            }
            Step::Fail => {
                let message = self.rust().message.to_string();
                self.as_mut().rust_mut().flow.fail(message);
            }
        }
        self.publish_flow();
    }

    fn publish_flow(mut self: std::pin::Pin<&mut Self>) {
        let preparing = self.rust().flow.pending();
        let failed = QString::from(&self.rust().flow.failed_paths().join("\n"));
        let error = QString::from(&self.rust().flow.error);
        self.as_mut().set_preparing(preparing);
        self.as_mut().set_failed_reads(failed);
        self.set_preparation_error(error);
    }

    pub fn compute_calls(self: std::pin::Pin<&mut Self>) {
        self.single_review(20);
    }

    /// Start the alternative calls queued when a trace opened, unless the
    /// user cancelled them or another stage started first.
    fn start_queued_calls(mut self: std::pin::Pin<&mut Self>) {
        if self.rust().pending_calls && self.rust().stage.is_none() {
            self.as_mut().rust_mut().pending_calls = false;
            self.compute_calls();
        }
    }

    pub fn select_calls(mut self: std::pin::Pin<&mut Self>, id: &QString) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let result = (|| -> Result<SangerReviewView, String> {
            let mut r = self.as_mut().rust_mut();
            let session = r.review_session.as_mut().ok_or("No review is open")?;
            session.select_call_set(&id.to_string())?;
            session_view(session.review().ok_or("No review is open")?)
        })();
        match result {
            Ok(mut view) => {
                {
                    let mut r = self.as_mut().rust_mut();
                    view.resize(r.view.width, r.view.height);
                    r.overview_cache = basecraft_viewmodel::overview::Cache::default();
                    r.review = Some(view);
                }
                self.as_mut().set_dirty(true);
                self.refresh();
                true
            }
            Err(error) => {
                self.call_error(&error);
                false
            }
        }
    }

    pub fn record_revision(&self) -> QString {
        QString::from(
            self.rust()
                .review_session
                .as_ref()
                .and_then(|document| document.session.record_revision(document.id).ok())
                .unwrap_or_default(),
        )
    }
    pub fn document_record(&self) -> QString {
        let result = self.rust().review_session.as_ref().map_or_else(
            || {
                basecraft_ops::records::DocumentRecord::empty(&self.rust().file_path.to_string())
                    .report()
                    .map_err(|e| e.to_string())
            },
            |document| document.report(self.rust().dirty),
        );
        crate::provenance_bridge::report_json(
            result.and_then(|report| serde_json::to_string(&report).map_err(|e| e.to_string())),
        )
    }

    pub fn accessible_page(&self, page: i32) -> QString {
        let result = {
            let r = self.rust();
            if let Some(review) = &r.review {
                Some(basecraft_viewmodel::accessible::review(
                    &review.consensus,
                    &review.masked,
                    &review.unresolved,
                    page,
                ))
            } else {
                r.trace
                    .as_ref()
                    .map(|trace| basecraft_viewmodel::accessible::trace(trace, page))
            }
        };
        let value = result.map_or_else(
            || serde_json::json!({"page": 0, "pages": 1, "text": "No document loaded"}),
            |p| serde_json::json!({"page": p.page, "pages": p.pages, "text": p.text}),
        );
        QString::from(&value.to_string())
    }

    fn refresh(mut self: std::pin::Pin<&mut Self>) {
        self.as_mut().find_update(false);
        let history = self.record_revision();
        self.as_mut().set_history_revision(history);
        let (bytes, status, zoom, review_mode, sequence, decision, unresolved, context) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &mut r.review {
                if let Some(session) = r
                    .review_session
                    .as_ref()
                    .and_then(work::ReviewDocument::review)
                {
                    sync_review_view(review, session);
                }
                if r.find.active() {
                    r.find_source_key = format!("{}:{}", r.file_path, review.find_key());
                    r.find.invalidate(&r.find_source_key);
                }
                review.build(&r.theme, &mut r.scratch);
                review.draw_find(&r.find, &r.theme, &mut r.scratch);
                let zoom = basecraft_viewmodel::zoom::percent_text(
                    f64::from(review.column_width) / 12.0 * 100.0,
                );
                let decision = r
                    .review_session
                    .as_ref()
                    .and_then(work::ReviewDocument::review)
                    .and_then(|session| {
                        session
                            .decision(review.selected)
                            .map(|value| format!("{value:?}"))
                    })
                    .unwrap_or_default();
                let unresolved = r
                    .review_session
                    .as_ref()
                    .and_then(work::ReviewDocument::review)
                    .map_or(0, |session| {
                        i32::try_from(session.unresolved()).unwrap_or(i32::MAX)
                    });
                (
                    r.scratch.to_bytes(),
                    review.review_counts(),
                    zoom,
                    true,
                    String::from_utf8_lossy(&review.consensus).into_owned(),
                    decision,
                    unresolved,
                    review.alignment_text(),
                )
            } else {
                let Some(trace) = &r.trace else {
                    return;
                };
                if r.find.active() {
                    let (key, _) = r.find_source();
                    r.find_source_key = key;
                    r.find.invalidate(&r.find_source_key);
                }
                r.view.build_wrapped(trace, &r.theme, &mut r.scratch);
                r.view.draw_find(trace, &r.find, &r.theme, &mut r.scratch);
                let calls = trace.display_calls();
                let status = format!("Instrument calls · {} bases", calls.bases.len());
                let zoom =
                    basecraft_viewmodel::zoom::percent_text(100.0 / r.view.samples_per_pixel);
                (
                    r.scratch.to_bytes(),
                    status,
                    zoom,
                    false,
                    String::new(),
                    String::new(),
                    0,
                    String::new(),
                )
            }
        };
        let (position, size) = self.rust().trace.as_ref().map_or((0.0, 1.0), |trace| {
            (
                self.rust().view.start / display_count(trace.samples().max(1)),
                self.rust().view.wrapped_scroll_size(trace),
            )
        });
        let (hpos, hsize) = self.rust().review.as_ref().map_or((0.0, 1.0), |review| {
            (review.scroll_position(), review.scroll_size())
        });
        let lanes = self.rust().review.as_ref().map_or_else(Vec::new, |review| {
            review
                .lanes
                .iter()
                .enumerate()
                .map(|(i, lane)| {
                    serde_json::json!({
                        "top": review.lane_top(i), "gain": lane.gain, "height": lane.height
                    })
                })
                .collect::<Vec<_>>()
        });
        let height = self
            .rust()
            .review
            .as_ref()
            .map_or(0.0, |v| f64::from(v.content_height()));
        let paths: Vec<PathBuf> = self.rust().review.as_ref().map_or_else(
            || vec![PathBuf::from(self.rust().file_path.to_string())],
            |v| v.draft.inputs.iter().map(|r| r.path.clone()).collect(),
        );
        let threshold = self
            .rust()
            .review
            .as_ref()
            .and_then(|v| v.draft.signal_threshold)
            .unwrap_or(20);
        self.as_mut()
            .set_signal_threshold(i32::try_from(threshold).unwrap_or(20));
        self.as_mut()
            .set_input_paths(QString::from(serde_json::to_string(&paths).unwrap()));
        let names: Vec<_> = self.review_payload().map_or_else(
            || paths.clone(),
            |s| s.draft.inputs.iter().map(|r| r.path.clone()).collect(),
        );
        let name = basecraft_viewmodel::sanger_review::suggested_name(&names);
        let anchor = self
            .review_payload()
            .map(|s| s.storage_anchor().to_path_buf());
        let export = anchor
            .as_deref()
            .and_then(Path::parent)
            .or_else(|| paths.first().and_then(|p| p.parent()))
            .map(|folder| folder.join(format!("{name}_consensus_from_basecraft.fasta")))
            .unwrap_or_default();
        self.as_mut().set_proposed_name(QString::from(&name));
        self.as_mut()
            .set_export_path(QString::from(export.to_string_lossy().as_ref()));
        self.as_mut().set_review_height(height);
        self.as_mut()
            .set_trace_lanes(QString::from(serde_json::to_string(&lanes).unwrap()));
        let mask = self
            .review_payload()
            .map(|s| &s.mask)
            .map_or_else(|| "{}".into(), |m| serde_json::to_string(m).unwrap());
        let selected = self
            .rust()
            .review
            .as_ref()
            .map_or(1, |v| i32::try_from(v.selected + 1).unwrap_or(i32::MAX));
        let columns = self
            .rust()
            .review
            .as_ref()
            .map_or(1, |v| i32::try_from(v.consensus.len()).unwrap_or(i32::MAX));
        let settings = self
            .review_payload()
            .map_or_else(basecraft_docs::sanger::ReviewSettings::default, |s| {
                s.settings.clone()
            });
        self.as_mut()
            .set_review_settings(QString::from(serde_json::to_string(&settings).unwrap()));
        self.as_mut().set_mask_settings(QString::from(mask));
        self.as_mut().set_selected_column(selected);
        self.as_mut().set_consensus_columns(columns);
        self.as_mut().set_hscroll_pos(hpos);
        self.as_mut().set_hscroll_size(hsize);
        self.as_mut().set_trace_scroll_pos(position);
        self.as_mut().set_trace_scroll_size(size);
        let overview = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &r.review {
                r.overview_cache
                    .get(review.overview_key(&r.theme), || review.overview(&r.theme))
            } else if let Some(trace) = &r.trace {
                let key = format!("{}:{:?}", r.file_path, r.theme);
                r.overview_cache.get(key, || {
                    basecraft_viewmodel::overview::trace(trace, &r.theme)
                })
            } else {
                "{\"shapes\":[]}".into()
            }
        };
        let matches = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let geometry = r
                .review
                .as_ref()
                .map(|v| (v.find_key(), v.content_height().to_bits()));
            let key = format!("{}:{}:{geometry:?}", r.find_source_key, r.find.revision());
            r.overview_match_cache.get_json(key, || {
                basecraft_viewmodel::find::overview_marks(&r.find, |hit| {
                    if let Some(review) = &r.review {
                        review.overview_match_at(
                            hit.start,
                            hit.end,
                            review.find_row_y(hit.row),
                            review.find_row_height(hit.row),
                        )
                    } else {
                        r.trace.as_ref().and_then(|trace| {
                            basecraft_viewmodel::overview::trace_match(trace, hit.start, hit.end)
                        })
                    }
                })
            })
        };
        self.as_mut().set_overview_content(QString::from(&overview));
        self.as_mut().set_overview_matches(QString::from(&matches));
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
        self.as_mut().set_status(QString::from(&status));
        self.as_mut().set_zoom_text(QString::from(&zoom));
        self.as_mut().set_review_mode(review_mode);
        self.as_mut().set_review_sequence(QString::from(&sequence));
        self.as_mut().set_review_decision(QString::from(&decision));
        self.as_mut().set_unresolved(unresolved);
        self.as_mut().set_alignment_context(QString::from(&context));
        let details = self
            .rust()
            .review
            .as_ref()
            .map_or_else(String::new, SangerReviewView::selected_text);
        self.as_mut().set_review_details(QString::from(details));
        let (sets, selected) = self.review_payload().map_or_else(
            || (serde_json::json!([]), String::new()), |session| {
                (serde_json::json!(session.call_sets().iter().enumerate().map(|(i, (id, engine))|
                    serde_json::json!({"id":id,"label":format!("{} calls · {}", if *engine == "ABI" { "Instrument" } else { "Computed" }, i + 1)})).collect::<Vec<_>>()),
                session.draft_sha256.clone())
            });
        self.as_mut().set_call_sets(QString::from(sets.to_string()));
        self.as_mut().set_selected_call_set(QString::from(selected));
    }

    pub fn orient_review(self: std::pin::Pin<&mut Self>, alignment: &QString) -> bool {
        self.submit_stage(work::Request::Orient(PathBuf::from(alignment.to_string())))
    }

    pub fn single_review(mut self: std::pin::Pin<&mut Self>, signal_threshold: i32) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        self.as_mut().rust_mut().pending_calls = false;
        if !(1..=90).contains(&signal_threshold) {
            self.call_error("Signal cutoff must be between 1 and 90");
            return false;
        }
        self.submit_stage(work::Request::Single(signal_threshold.cast_unsigned()))
    }

    pub fn reviewed_evidence(mut self: std::pin::Pin<&mut Self>) -> QString {
        let result = self
            .as_mut()
            .rust_mut()
            .review_session
            .as_mut()
            .ok_or_else(|| "No review is open".to_string())
            .and_then(work::ReviewDocument::addition_evidence);
        match result {
            Ok(value) => QString::from(value),
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.as_mut().set_message(QString::from(error));
                QString::default()
            }
        }
    }

    pub fn reviewed_sequence(mut self: std::pin::Pin<&mut Self>) -> QString {
        match self
            .review_payload()
            .and_then(SangerReview::resolved_sequence)
        {
            Some(sequence) if !sequence.is_empty() => QString::from(sequence),
            _ => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.as_mut()
                    .set_message(QString::from("Resolve every review site first"));
                QString::default()
            }
        }
    }

    pub fn reviewed_fasta(mut self: std::pin::Pin<&mut Self>, name: &QString) -> QString {
        let name = name.to_string();
        let name = name.trim();
        let sequence = self
            .review_payload()
            .and_then(SangerReview::resolved_sequence);
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            self.as_mut()
                .set_message(QString::from("Enter a specimen ID without spaces"));
            return QString::default();
        }
        match sequence {
            Some(sequence) if !sequence.is_empty() => {
                QString::from(format!(">{name}\n{sequence}\n"))
            }
            _ => {
                self.as_mut()
                    .set_message(QString::from("Resolve every review site first"));
                QString::default()
            }
        }
    }

    pub fn open_file(mut self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        self.as_mut().cancel_stage();
        self.as_mut().rust_mut().stage = None;
        self.as_mut().set_busy(false);
        self.as_mut().set_calling(false);
        let input = PathBuf::from(path.to_string());
        let instrument = input
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ab1") || ext.eq_ignore_ascii_case("abi"));
        match if instrument {
            None
        } else {
            basecraft_ops::sanger_review::evidence_kind(&input)
        } {
            Some(basecraft_ops::sanger_review::EvidenceKind::Review) => {
                return self.open_review(path);
            }
            Some(basecraft_ops::sanger_review::EvidenceKind::Draft) => {
                return self.open_draft(path);
            }
            None => {}
        }
        let path = input;
        let loaded = basecraft_ops::sanger_review::instrument_review(&path).and_then(|review| {
            let view = session_view(&review)?;
            Ok((work::ReviewDocument::new(&review)?, view))
        });
        match loaded {
            Ok((session, mut view)) => {
                self.as_mut().cancel_calls();
                {
                    let mut r = self.as_mut().rust_mut();
                    view.resize(r.view.width, r.view.height);
                    // Keep the raw trace for the existing multi-read draft action.
                    r.overview_cache = basecraft_viewmodel::overview::Cache::default();
                    r.trace = view.lanes.first().map(|lane| lane.trace.clone());
                    r.review = Some(view);
                    r.review_session = Some(session);
                    r.review_path = None;
                }
                self.as_mut()
                    .set_file_path(QString::from(path.to_string_lossy().as_ref()));
                self.as_mut().set_dirty(false);
                self.as_mut().refresh();
                self.as_mut().rust_mut().pending_calls = true;
                let _ = self.qt_thread().queue(Self::start_queued_calls);
                self.as_mut().set_calling(true);
                self.as_mut().set_message_kind(QString::from("notify"));
                self.set_message(QString::from(
                    "Alternative calling queued; reviewing instrument calls",
                ));
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.as_mut().set_message(QString::from(error));
                false
            }
        }
    }

    fn install_review_result(
        mut self: std::pin::Pin<&mut Self>,
        path: &Path,
        result: Result<(SangerReviewView, SangerReview), String>,
        dirty: bool,
    ) -> bool {
        self.as_mut().cancel_stage();
        self.as_mut().rust_mut().stage = None;
        self.as_mut().set_busy(false);
        self.as_mut().set_calling(false);
        let result =
            result.and_then(|(view, review)| Ok((view, work::ReviewDocument::new(&review)?)));
        match result {
            Ok((mut review, session)) => {
                self.as_mut().cancel_calls();
                {
                    let mut r = self.as_mut().rust_mut();
                    review.resize(r.view.width, r.view.height);
                    r.review = Some(review);
                    r.review_session = Some(session);
                    r.review_path = None;

                    r.overview_cache = basecraft_viewmodel::overview::Cache::default();
                    r.trace = None;
                }
                self.as_mut().set_dirty(dirty);
                self.as_mut()
                    .set_file_path(QString::from(path.to_string_lossy().as_ref()));
                self.as_mut().set_message(QString::default());
                self.as_mut().set_message_kind(QString::default());
                self.refresh();
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                false
            }
        }
    }

    pub fn open_review(mut self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        self.as_mut().cancel_stage();
        self.as_mut().rust_mut().stage = None;
        self.as_mut().set_busy(false);
        self.as_mut().set_calling(false);
        let path = PathBuf::from(path.to_string());
        let result = (|| -> Result<_, String> {
            let session = work::ReviewDocument::open(&path)?;
            let view = session_view(session.review().ok_or("No review is open")?)?;
            Ok((view, session))
        })();
        match result {
            Ok((mut review, session)) => {
                self.as_mut().cancel_calls();
                {
                    let mut r = self.as_mut().rust_mut();
                    review.resize(r.view.width, r.view.height);
                    r.review = Some(review);
                    r.review_session = Some(session);
                    r.review_path = Some(path.clone());

                    r.overview_cache = basecraft_viewmodel::overview::Cache::default();
                    r.trace = None;
                }
                self.as_mut().set_dirty(false);
                self.as_mut()
                    .set_file_path(QString::from(path.to_string_lossy().as_ref()));
                self.as_mut().set_message(QString::default());
                self.as_mut().set_message_kind(QString::default());
                self.refresh();
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                false
            }
        }
    }

    pub fn draft_consensus(
        mut self: std::pin::Pin<&mut Self>,
        additional_paths_json: &QString,
        signal_threshold: i32,
    ) -> bool {
        // Keep the existing additional-read API, sharing the full-selection path.
        // Detach earlier calls before source resolution or JSON validation can
        // fail, so late results cannot replace this request's error.
        self.as_mut().cancel_calls();
        if self
            .rust()
            .stage
            .as_ref()
            .is_some_and(|stage| stage.name == "single")
        {
            self.as_mut().rust_mut().stage = None;
            self.as_mut().set_calling(false);
            self.as_mut().set_busy(false);
        }
        let paths = (|| -> Result<Vec<String>, String> {
            let source = self
                .review_payload()
                .filter(|session| session.draft.inputs.len() == 1)
                .ok_or("Open one AB1 trace before drafting a consensus")?
                .working_draft()?
                .inputs[0]
                .path
                .to_string_lossy()
                .into_owned();
            let mut paths = vec![source];
            let additional: Vec<String> = serde_json::from_str(&additional_paths_json.to_string())
                .map_err(|e| e.to_string())?;
            paths.extend(additional);
            Ok(paths)
        })();
        match paths {
            Ok(paths) => self.prepare_reads(
                &QString::from(serde_json::json!(paths).to_string()),
                signal_threshold,
            ),
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.as_mut().set_message(QString::from(error));
                false
            }
        }
    }

    pub fn retain_reference(
        mut self: std::pin::Pin<&mut Self>,
        anchor: &QString,
        source: &QString,
    ) -> QString {
        if self.rust().stage.is_some() {
            return QString::default();
        }
        match work::retain_reference(
            Path::new(&anchor.to_string()),
            Path::new(&source.to_string()),
        ) {
            Ok(path) => QString::from(path.to_string_lossy().as_ref()),
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.set_message(QString::from(format!(
                    "Cannot retain review reference: {error}"
                )));
                QString::default()
            }
        }
    }

    pub fn prepare_reads(
        mut self: std::pin::Pin<&mut Self>,
        paths_json: &QString,
        signal_threshold: i32,
    ) -> bool {
        // Invalid paired requests also supersede queued automatic calling, so
        // a later timer cannot replace their failure with single-read success.
        self.as_mut().rust_mut().pending_calls = false;
        if self.rust().stage.is_none() {
            self.as_mut().set_calling(false);
        }
        // A selected pair supersedes automatic single-read calling. Detaching
        // its receiver prevents a late single completion masking pair failure.
        if self
            .rust()
            .stage
            .as_ref()
            .is_some_and(|stage| stage.name == "single")
        {
            self.as_mut().cancel_stage();
            self.as_mut().rust_mut().stage = None;
            self.as_mut().set_busy(false);
            self.as_mut().set_calling(false);
        }
        let paths = serde_json::from_str::<Vec<PathBuf>>(&paths_json.to_string());
        match paths {
            Ok(paths) if (2..=3).contains(&paths.len()) && (1..=90).contains(&signal_threshold) => {
                self.submit_stage(work::Request::Prepare(
                    paths,
                    signal_threshold.cast_unsigned(),
                ))
            }
            _ => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.set_message(QString::from(
                    "Choose two or three AB1 reads and a signal cutoff between 1 and 90",
                ));
                false
            }
        }
    }

    pub fn save_review(mut self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let path = PathBuf::from(path.to_string());
        let result = self
            .as_mut()
            .rust_mut()
            .review_session
            .as_mut()
            .ok_or_else(|| "No Sanger review is open".to_string())
            .and_then(|session| session.save(&path));
        match result {
            Ok(()) => {
                self.as_mut().rust_mut().review_path = Some(path.clone());
                self.as_mut()
                    .set_file_path(QString::from(path.to_string_lossy().as_ref()));
                self.as_mut().set_dirty(false);
                self.as_mut().set_message_kind(QString::from("done"));
                self.as_mut().set_message(QString::from("Review saved"));
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                false
            }
        }
    }

    pub fn review_set(
        mut self: std::pin::Pin<&mut Self>,
        choice: &QString,
        base: &QString,
    ) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let choice = match choice.to_string().as_str() {
            "accept" => self
                .rust()
                .review
                .as_ref()
                .map_or(ReviewChoice::Accept, |v| {
                    let base = v.consensus[v.selected] as char;
                    if base == v.draft.columns[v.selected].consensus {
                        ReviewChoice::Accept
                    } else {
                        ReviewChoice::Edit(base)
                    }
                }),
            "reject" => ReviewChoice::Reject,
            "pending" => ReviewChoice::Pending,
            "edit" => {
                let text = base.to_string().to_ascii_uppercase();
                let mut chars = text.chars();
                if let (Some(symbol), None) = (chars.next(), chars.next()) {
                    ReviewChoice::Edit(symbol)
                } else {
                    self.as_mut().set_message_kind(QString::from("error"));
                    self.as_mut()
                        .set_message(QString::from("Enter one DNA base"));
                    return false;
                }
            }
            _ => return false,
        };
        let Some(column) = self.rust().review.as_ref().map(|review| review.selected) else {
            return false;
        };
        self.review_history(ReviewOp::Set {
            columns: vec![column],
            choice,
        })
    }

    fn review_history(self: std::pin::Pin<&mut Self>, op: ReviewOp) -> bool {
        self.review_change(|document| document.apply(op))
    }

    fn review_change(
        mut self: std::pin::Pin<&mut Self>,
        change: impl FnOnce(&mut work::ReviewDocument) -> Result<(), String>,
    ) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let before_view = self
            .review_payload()
            .map(|review| (review.draft_sha256.clone(), review.evidence_runs().to_vec()));
        let result = self
            .as_mut()
            .rust_mut()
            .review_session
            .as_mut()
            .ok_or_else(|| "No Sanger review is open".to_string())
            .and_then(change);
        if result.is_ok() {
            self.as_mut().set_dirty(true);
        }
        let result = result.and_then(|()| {
            let after_view = self
                .review_payload()
                .map(|review| (review.draft_sha256.clone(), review.evidence_runs().to_vec()));
            if before_view != after_view {
                let mut r = self.as_mut().rust_mut();
                let view = session_view(
                    r.review_session
                        .as_ref()
                        .and_then(work::ReviewDocument::review)
                        .ok_or("No review is open")?,
                );
                let mut view = match view {
                    Ok(view) => view,
                    Err(error) => {
                        r.review = None;
                        return Err(error);
                    }
                };
                view.resize(r.view.width, r.view.height);
                if let Some(old) = &r.review {
                    view.selected = old.selected.min(view.draft.columns.len().saturating_sub(1));
                    view.left = old.left;
                    view.column_width = old.column_width;
                    view.pan(0.0);
                }
                r.review = Some(view);
                r.overview_cache = basecraft_viewmodel::overview::Cache::default();
            }
            Ok(())
        });
        let success = result.is_ok();
        match result {
            Ok(()) => {
                self.as_mut().set_dirty(true);
                self.as_mut().set_message(QString::default());
                self.as_mut().set_message_kind(QString::default());
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
            }
        }
        self.refresh();
        success
    }

    pub fn apply_review_settings(
        mut self: std::pin::Pin<&mut Self>,
        settings_json: &QString,
    ) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let result = (|| -> Result<ReviewOp, String> {
            let settings: basecraft_docs::sanger::ReviewSettings =
                serde_json::from_str(&settings_json.to_string()).map_err(|e| e.to_string())?;
            settings.validate()?;
            let mut r = self.as_mut().rust_mut();
            let view = r.review.as_mut().ok_or("Open a trace review first")?;
            view.review_settings = settings.clone();
            let reference = view.frozen_reference();
            let calls = (0..view.draft.columns.len())
                .map(|i| view.assessment(i).call)
                .collect();
            Ok(ReviewOp::Evaluate {
                version: 2,
                settings,
                calls,
                reference,
            })
        })();
        match result {
            Ok(op) => {
                self.as_mut().review_history(op);
                self.rust().message.to_string().is_empty()
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(error));
                false
            }
        }
    }

    pub fn mask_ends(mut self: std::pin::Pin<&mut Self>, start: i32, end: i32) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let Some(session) = self.review_payload() else {
            return false;
        };
        let mut settings = session.mask.clone();
        let (Some(start), Ok(end)) = (
            start.checked_sub(1).and_then(|v| usize::try_from(v).ok()),
            usize::try_from(end),
        ) else {
            return false;
        };
        settings.start = start;
        settings.end = end;
        self.as_mut().review_history(ReviewOp::Mask { settings });
        let ok = self.rust().message.to_string().is_empty();
        if ok {
            {
                let mut r = self.as_mut().rust_mut();
                let r = &mut *r;
                if let (Some(view), Some(session)) = (
                    &mut r.review,
                    r.review_session
                        .as_ref()
                        .and_then(work::ReviewDocument::review),
                ) && session.masked(view.selected)
                {
                    let target = (session.mask.start..session.mask.end)
                        .find(|&i| {
                            matches!(
                                session.decision(i),
                                Some(ReviewChoice::Pending | ReviewChoice::Reject)
                            )
                        })
                        .unwrap_or(session.mask.start);
                    view.select_column(target);
                }
            }
            self.refresh();
        }
        ok
    }
    pub fn mask_reference(mut self: std::pin::Pin<&mut Self>) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let Some((start, end)) = self
            .rust()
            .review
            .as_ref()
            .and_then(SangerReviewView::reference_mask_bounds)
        else {
            self.as_mut()
                .set_message(QString::from("Load an alignment context first"));
            return false;
        };
        self.mask_ends(
            i32::try_from(start + 1).unwrap_or(i32::MAX),
            i32::try_from(end).unwrap_or(i32::MAX),
        )
    }
    pub fn find_primers(
        self: std::pin::Pin<&mut Self>,
        forward: &QString,
        reverse: &QString,
        overlap: i32,
        errors: i32,
    ) -> bool {
        self.submit_stage(work::Request::Primers(serde_json::json!({"forward":forward.to_string(),"reverse":reverse.to_string(),"minimum_overlap":overlap,"error_percent":errors})))
    }
    pub fn apply_primers(mut self: std::pin::Pin<&mut Self>, proposal: &QString) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let result = (|| -> Result<MaskSettings, String> {
            let value: serde_json::Value =
                serde_json::from_str(&proposal.to_string()).map_err(|e| e.to_string())?;
            let review = self.rust().review.as_ref().ok_or("No review open")?;
            let session = self.review_payload().ok_or("No review open")?;
            let kept: Vec<_> = review
                .consensus
                .iter()
                .enumerate()
                .filter(|(_, b)| **b != b'-')
                .map(|(i, _)| i)
                .collect();
            let start = value["keep_start"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("Invalid primer bounds")?;
            let end = value["keep_end"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("Invalid primer bounds")?;
            if start >= end || end > kept.len() {
                return Err("Invalid primer bounds".into());
            }
            let mut mask = session.mask.clone();
            mask.start = mask.start.max(if start == 0 { 0 } else { kept[start] });
            mask.end = mask.end.min(if end == kept.len() {
                review.consensus.len()
            } else {
                kept[end]
            });
            mask.forward = value["forward"].as_str().ok_or("Missing primers")?.into();
            mask.reverse = value["reverse"].as_str().ok_or("Missing primers")?.into();
            mask.minimum_overlap =
                usize::try_from(value["minimum_overlap"].as_u64().ok_or("Missing overlap")?)
                    .map_err(|_| "Overlap is too large")?;
            mask.error_percent = usize::try_from(
                value["error_percent"]
                    .as_u64()
                    .ok_or("Missing error threshold")?,
            )
            .map_err(|_| "Error threshold is too large")?;
            mask.evidence = proposal.to_string();
            Ok(mask)
        })();
        match result {
            Ok(settings) => {
                self.as_mut().review_history(ReviewOp::Mask { settings });
                self.rust().message.to_string().is_empty()
            }
            Err(e) => {
                self.as_mut().set_message(QString::from(e));
                false
            }
        }
    }

    #[allow(clippy::cast_possible_truncation)] // bounded pixel height
    pub fn trace_height(mut self: std::pin::Pin<&mut Self>, read: i32, height: f64) {
        if height.is_finite() {
            if let Some(lane) = self.as_mut().rust_mut().review.as_mut().and_then(|view| {
                usize::try_from(read)
                    .ok()
                    .and_then(|read| view.lanes.get_mut(read))
            }) {
                lane.height = height.clamp(80.0, 1200.0) as f32;
            }
            self.refresh();
        }
    }

    #[allow(clippy::cast_possible_truncation)] // finite gain is clamped to 0.25–20
    pub fn trace_gain(mut self: std::pin::Pin<&mut Self>, read: i32, gain: f64) {
        if gain.is_finite() {
            if let Some(lane) = self.as_mut().rust_mut().review.as_mut().and_then(|view| {
                usize::try_from(read)
                    .ok()
                    .and_then(|read| view.lanes.get_mut(read))
            }) {
                lane.gain = gain.clamp(0.25, 20.0) as f32;
            }
            self.refresh();
        }
    }

    pub fn review_undo(self: std::pin::Pin<&mut Self>) {
        self.review_change(|document| {
            document
                .session
                .undo(document.id)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }
    pub fn review_redo(self: std::pin::Pin<&mut Self>) {
        self.review_change(|document| {
            document
                .session
                .redo(document.id)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }

    pub fn review_accept_unflagged(mut self: std::pin::Pin<&mut Self>) {
        let settings = self.rust().review_settings.clone();
        self.as_mut().apply_review_settings(&settings);
    }

    pub fn export_consensus(
        mut self: std::pin::Pin<&mut Self>,
        path: &QString,
        label: &QString,
    ) -> bool {
        if self.rust().stage.is_some() {
            return false;
        }
        let review_path = self.rust().review_path.clone();
        let output = PathBuf::from(path.to_string());
        let events_before = self
            .review_payload()
            .map_or(0, |session| session.events().len());
        let result = self
            .as_mut()
            .rust_mut()
            .review_session
            .as_mut()
            .ok_or_else(|| "No Sanger review is open".to_string())
            .and_then(|session| {
                session.export_consensus(&output, &label.to_string())?;
                if let Some(path) = &review_path {
                    session.save(path)?;
                }
                Ok(())
            });
        match result {
            Ok(()) => {
                self.as_mut().set_dirty(review_path.is_none());
                self.as_mut().set_message_kind(QString::from("done"));
                self.as_mut()
                    .set_message(QString::from("Consensus exported"));
                true
            }
            Err(error) => {
                let export_was_recorded = self
                    .review_payload()
                    .is_some_and(|session| session.events().len() > events_before);
                if export_was_recorded {
                    self.as_mut().set_dirty(true);
                }
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                false
            }
        }
    }

    pub fn place_alignment(self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        self.submit_stage(work::Request::Place(PathBuf::from(path.to_string())))
    }

    pub fn go_to_column(
        mut self: std::pin::Pin<&mut Self>,
        reference: bool,
        column: &QString,
    ) -> bool {
        if self.rust().busy {
            return false;
        }
        let result = self
            .as_mut()
            .rust_mut()
            .review
            .as_mut()
            .ok_or_else(|| "No draft review is open".to_string())
            .and_then(|review| review.go_to_column(reference, &column.to_string()));
        match result {
            Ok(()) => {
                self.as_mut().set_message(QString::default());
                self.refresh();
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.as_mut().set_message(QString::from(error));
                false
            }
        }
    }

    pub fn next_issue(mut self: std::pin::Pin<&mut Self>, forward: bool) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let (Some(review), Some(session)) = (
                &mut r.review,
                r.review_session
                    .as_ref()
                    .and_then(work::ReviewDocument::review),
            ) {
                let len = review.draft.columns.len();
                let pending = (1..=len)
                    .map(|offset| {
                        if forward {
                            (review.selected + offset) % len
                        } else {
                            (review.selected + len - offset % len) % len
                        }
                    })
                    .find(|&column| {
                        !session.masked(column)
                            && matches!(
                                session.decision(column),
                                Some(ReviewChoice::Pending | ReviewChoice::Reject)
                            )
                    });
                if let Some(column) = pending {
                    review.select_column(column);
                }
            }
        }
        self.refresh();
    }

    pub fn hscroll_to(mut self: std::pin::Pin<&mut Self>, fraction: f64) {
        if let Some(review) = &mut self.as_mut().rust_mut().review {
            review.scroll_to(fraction);
        }
        self.refresh();
    }

    pub fn hover(mut self: std::pin::Pin<&mut Self>, x: f64, y: f64) -> QString {
        let mut changed = false;
        let text = {
            let mut r = self.as_mut().rust_mut();
            if let Some(review) = &mut r.review {
                let display = if y >= 0.0 {
                    review.display_at(display_coordinate(x))
                } else {
                    None
                };
                changed = review.hovered != display;
                review.hovered = display;
                display.map_or_else(String::new, |column| review.site_status(column))
            } else {
                String::new()
            }
        };
        if changed {
            self.refresh();
        }
        QString::from(text)
    }

    pub fn resize(mut self: std::pin::Pin<&mut Self>, width: f64, height: f64) {
        {
            let mut r = self.as_mut().rust_mut();
            r.view.width = display_coordinate(width.max(1.0));
            r.view.height = display_coordinate(height.max(1.0));
            if let Some(review) = &mut r.review {
                review.resize(display_coordinate(width), display_coordinate(height));
            }
        }
        self.refresh();
    }

    pub fn wheel(mut self: std::pin::Pin<&mut Self>, x: f64, delta: f64) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let factor = if delta > 0.0 { 1.3 } else { 1.0 / 1.3 };
            if let Some(review) = &mut r.review {
                review.zoom(display_coordinate(x), factor);
            } else if let Some(trace) = &r.trace {
                r.view.zoom(trace, display_coordinate(x), factor);
                r.view
                    .scroll_wrapped(trace, r.view.start / display_count(trace.samples().max(1)));
            }
        }
        self.refresh();
    }

    pub fn press(mut self: std::pin::Pin<&mut Self>, x: f64, y: f64) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &mut r.review {
                review.select_at(display_coordinate(x), display_coordinate(y));
            } else if let Some(trace) = &r.trace {
                r.view
                    .select_wrapped(trace, display_coordinate(x), display_coordinate(y));
            }
        }
        self.refresh();
    }

    pub fn context_column(mut self: std::pin::Pin<&mut Self>, x: f64, y: f64) -> i32 {
        let column = self.rust().review.as_ref().and_then(|review| {
            review
                .display_at(display_coordinate(x))
                .and_then(|display| review.columns[display].draft)
        });
        let Some(column) = column.and_then(|i| i32::try_from(i + 1).ok()) else {
            return 0;
        };
        self.as_mut().press(x, y);
        column
    }

    pub fn pan(mut self: std::pin::Pin<&mut Self>, pixels: f64) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &mut r.review {
                review.pan(display_coordinate(pixels));
            } else if let Some(trace) = &r.trace {
                r.view.pan(trace, display_coordinate(pixels));
                r.view
                    .scroll_wrapped(trace, r.view.start / display_count(trace.samples().max(1)));
            }
        }
        self.refresh();
    }

    pub fn next_call(mut self: std::pin::Pin<&mut Self>, forward: bool) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &mut r.review {
                let next = if forward {
                    review.selected.saturating_add(1)
                } else {
                    review.selected.saturating_sub(1)
                };
                review.select_column(next);
            } else if let Some(trace) = &r.trace {
                let next = if forward {
                    r.view.selected.saturating_add(1)
                } else {
                    r.view.selected.saturating_sub(1)
                };
                r.view.center_call(trace, next);
            }
        }
        self.refresh();
    }

    pub fn set_zoom_percent(mut self: std::pin::Pin<&mut Self>, percent: f64) {
        if !percent.is_finite() || !(0.000_001..=20_000.0).contains(&percent) {
            return;
        }
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &mut r.review {
                let factor = percent / (f64::from(review.column_width) / 12.0 * 100.0);
                review.zoom(review.width / 2.0, factor);
            } else if let Some(trace) = &r.trace {
                let factor = percent * r.view.samples_per_pixel / 100.0;
                r.view.zoom(trace, r.view.width / 2.0, factor);
                r.view
                    .scroll_wrapped(trace, r.view.start / display_count(trace.samples().max(1)));
            }
        }
        self.refresh();
    }

    pub fn zoom_in(self: std::pin::Pin<&mut Self>) {
        self.wheel(400.0, 1.0);
    }
    pub fn zoom_out(self: std::pin::Pin<&mut Self>) {
        self.wheel(400.0, -1.0);
    }

    pub fn scroll_trace(mut self: std::pin::Pin<&mut Self>, fraction: f64) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(trace) = &r.trace {
                r.view.scroll_wrapped(trace, fraction);
            }
        }
        self.refresh();
    }

    pub fn fit(mut self: std::pin::Pin<&mut Self>) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &mut r.review {
                review.fit();
            } else if let Some(trace) = &r.trace {
                r.view.fit_wrapped(trace);
            }
        }
        self.refresh();
    }

    pub fn export_figure(
        self: std::pin::Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
    ) {
        self.export_figure_with(path, scope, publication, None);
    }

    pub fn export_figure_png(
        self: std::pin::Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
        width: f64,
    ) {
        self.export_figure_with(path, scope, publication, Some(width));
    }

    fn export_figure_with(
        mut self: std::pin::Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
        png_width: Option<f64>,
    ) {
        if !(0..=2).contains(&scope) {
            self.as_mut().set_message_kind(QString::from("error"));
            self.as_mut()
                .set_message(QString::from("Unknown figure scope"));
            return;
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let theme = if publication {
                crate::appearance::publication_theme()
            } else {
                r.theme.clone()
            };
            if let Some(review) = &mut r.review {
                if let Some(session) = r
                    .review_session
                    .as_ref()
                    .and_then(work::ReviewDocument::review)
                {
                    sync_review_view(review, session);
                }
                let previous = (review.left, review.column_width, review.hovered);
                if scope == 2 {
                    review.fit();
                } else if scope == 1 {
                    // A selected call with neighbouring calls gives the signal
                    // enough context to read in a standalone figure.
                    #[allow(clippy::cast_precision_loss)]
                    let start = review.selected.saturating_sub(5) as f64;
                    review.left = start;
                    review.column_width = (review.width / 11.0).clamp(1.0, 100.0);
                }
                review.build(&theme, &mut r.scratch);
                (review.left, review.column_width, review.hovered) = previous;
            } else if let Some(trace) = &r.trace {
                let mut view = r.view.clone();
                if scope == 2 {
                    view.fit_wrapped(trace);
                } else if scope == 1 {
                    view.center_call(trace, view.selected);
                }
                view.build_wrapped(trace, &theme, &mut r.scratch);
            } else {
                return Err("No trace open".into());
            }
            let xml = svg::frame_svg(
                &r.scratch,
                r.view.width,
                r.view.height,
                &r.mono_font.to_string(),
                theme.background,
            );
            if let Some(width) = png_width {
                let bytes = crate::figure_png::render(&xml, width)?;
                basecraft_io::write_atomic(Path::new(&path.to_string()), |out| {
                    out.write_all(&bytes)
                })?;
            } else {
                basecraft_io::write_atomic(Path::new(&path.to_string()), |out| {
                    out.write_all(xml.as_bytes())
                })?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => self
                .as_mut()
                .set_message(QString::from(if png_width.is_some() {
                    "Exported PNG"
                } else {
                    "Exported SVG"
                })),
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error.to_string()));
            }
        }
    }

    pub fn save(mut self: std::pin::Pin<&mut Self>) {
        if let Some(path) = self.rust().review_path.clone() {
            self.as_mut()
                .save_review(&QString::from(path.to_string_lossy().as_ref()));
        } else if self.rust().review_session.is_some() {
            self.as_mut().set_message_kind(QString::from("error"));
            self.as_mut()
                .set_message(QString::from("Choose Save review as… first"));
        }
    }
}

// Qt supplies doubles; rendering uses finite single-precision screen coordinates.
#[allow(clippy::cast_possible_truncation)]
fn display_coordinate(value: f64) -> f32 {
    if value.is_finite() {
        value.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32
    } else {
        0.0
    }
}
// Sample counts are used only for approximate viewport ratios here.
#[allow(clippy::cast_precision_loss)]
fn display_count(value: usize) -> f64 {
    value as f64
}

crate::find_bridge::controller_find!(qobject::SangerController);

impl SangerRust {
    fn find_source(&self) -> (String, basecraft_viewmodel::find::Input) {
        if let Some(review) = &self.review {
            return (
                format!("{}:{}", self.file_path, review.find_key()),
                review.find_input(),
            );
        }
        let rows = {
            self.trace
                .as_ref()
                .map(|trace| {
                    vec![basecraft_viewmodel::find::Row {
                        id: 0,
                        name: "Instrument calls".into(),
                        bases: trace.display_calls().bases.clone(),
                    }]
                })
                .unwrap_or_default()
        };
        let key = basecraft_viewmodel::find::rows_key(&rows, &self.file_path.to_string());
        (key, basecraft_viewmodel::find::Input::Rows(rows))
    }
}
impl qobject::SangerController {
    fn find_update(mut self: std::pin::Pin<&mut Self>, first: bool) {
        let source = &self.rust().find_source_key;
        if !self.rust().find.needs_update(source) || !(first || self.rust().find.stale(source)) {
            return;
        }
        let (key, input) = self.rust().find_source();
        let scan = self.as_mut().rust_mut().find.update(key, || input);
        self.find_start(scan);
    }
    fn find_reveal(mut self: std::pin::Pin<&mut Self>) {
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        let Some(hit) = r.find.hit() else {
            return;
        };
        if let Some(review) = &mut r.review {
            review.reveal_find(hit.start);
        } else if let Some(trace) = &r.trace {
            r.view.reveal_find(trace, hit.start);
        }
    }
    fn find_y(&self) -> f32 {
        let r = self.rust();
        r.review
            .as_ref()
            .zip(r.find.hit())
            .map_or(0.0, |(review, hit)| review.find_row_y(hit.row))
    }
}

crate::appearance::editor_follows_appearance!(qobject::SangerController);
