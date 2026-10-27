//! Read-only chromatogram inspection backed by immutable ABIF evidence.

#[cxx_qt::bridge]
pub mod qobject {
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
        type SangerController = super::SangerRust;

        #[qinvokable]
        fn poll_calls(self: Pin<&mut SangerController>);
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
        ) -> QString;
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
        fn open_draft(self: Pin<&mut SangerController>, path: &QString) -> bool;
        #[qinvokable]
        fn draft_consensus(
            self: Pin<&mut SangerController>,
            additional_paths_json: &QString,
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
        fn hscroll_to(self: Pin<&mut SangerController>, fraction: f64);
        #[qinvokable]
        fn check_theme(self: Pin<&mut SangerController>);
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
        );
        #[qinvokable]
        fn save(self: Pin<&mut SangerController>);
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;

use basecraft_docs::AlignmentDoc;
use basecraft_docs::sanger::{SangerDraft, SangerPlacement};
use basecraft_io::abif::{self, Trace};
use basecraft_ops::sanger_review::{
    MaskSettings, ReviewChoice, ReviewOp, SangerReviewSession, SangerRun,
};
use basecraft_plugin_host::{Manifest, PluginProcess};
use basecraft_viewmodel::sanger::SangerView;
use basecraft_viewmodel::sanger_review::SangerReviewView;
use basecraft_viewmodel::{Frame, Theme, svg};
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString};

use crate::bridge::{css, load_theme, monospace_font, omarchy_theme_path, plugin_dir};

pub struct SangerRust {
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
    caller: basecraft_plugin_host::sanger_calls::SangerCaller,
    trace: Option<Trace>,
    review: Option<SangerReviewView>,
    review_session: Option<SangerReviewSession>,
    review_path: Option<PathBuf>,
    view: SangerView,
    theme: Theme,
    theme_stamp: Option<std::time::SystemTime>,
    scratch: Frame,
}

impl Default for SangerRust {
    fn default() -> Self {
        let (theme, theme_stamp) = load_theme().unwrap_or_else(|| (Theme::default(), None));
        Self {
            frame: QByteArray::default(),
            status: QString::default(),
            message: QString::default(),
            message_kind: QString::default(),
            ok_color: QString::from(&css(theme.nearest_hue(120.0))),
            error_color: QString::from(&css(theme.nearest_hue(0.0))),
            background: QString::from(&css(theme.background)),
            foreground: QString::from(&css(theme.foreground)),
            accent: QString::from(&css(theme.accent)),
            mono_font: QString::from(&monospace_font()),
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
            caller: basecraft_plugin_host::sanger_calls::SangerCaller::default(),
            trace: None,
            review: None,
            review_session: None,
            review_path: None,
            view: SangerView::default(),
            theme,
            theme_stamp,
            scratch: Frame::new(),
        }
    }
}

fn verified_traces(draft: &SangerDraft) -> Result<Vec<Trace>, String> {
    let mut traces = Vec::with_capacity(draft.inputs.len());
    for read in &draft.inputs {
        let actual = basecraft_io::sha256_file(&read.path).map_err(|e| e.to_string())?;
        if !actual.eq_ignore_ascii_case(&read.sha256) {
            return Err(format!(
                "Source AB1 changed since draft: {}",
                read.path.display()
            ));
        }
        traces.push(abif::open(&read.path).map_err(|e| e.to_string())?);
    }
    Ok(traces)
}

fn review_view(draft: &SangerDraft) -> Result<SangerReviewView, String> {
    // Opening saved evidence must never re-run a caller or create job files.
    // Older drafts without coordinates need explicit regeneration; instrument
    // calls cannot silently substitute for the recorded Tracy call mapping.
    let traces = verified_traces(draft)?;
    SangerReviewView::new(draft.clone(), traces)
}

// All explicit Sanger tool invocations share the same durable reservation and
// failure record. Returned paths must stay inside that owned run.
fn sanger_tool(
    anchor: &Path,
    plugin: &str,
    operation: &str,
    sources: &[PathBuf],
    dependencies: &[PathBuf],
    output_name: &str,
    mut parameters: serde_json::Value,
) -> Result<(SangerRun, PathBuf, Vec<PathBuf>), String> {
    let run = SangerRun::prepare_with_dependencies(anchor, operation, sources, dependencies)
        .map_err(|e| e.to_string())?;
    let output = run.artifact(output_name).map_err(|e| e.to_string())?;
    parameters["output"] = serde_json::json!(output);
    run.parameters(&parameters).map_err(|e| e.to_string())?;
    let outcome = (|| {
        let manifest = Manifest::load(&plugin_dir(plugin)).map_err(|e| e.to_string())?;
        let mut process = PluginProcess::spawn(&manifest).map_err(|e| e.to_string())?;
        process
            .invoke(operation, parameters, None)
            .map_err(|e| e.to_string())
    })();
    match outcome {
        Ok(result) => Ok((
            run,
            output,
            result.outputs.into_iter().map(PathBuf::from).collect(),
        )),
        Err(error) => {
            if let Err(record) = run.failed(&error) {
                return Err(format!(
                    "{error}; failure record could not be saved: {record}"
                ));
            }
            Err(error)
        }
    }
}

fn session_view(session: &SangerReviewSession) -> Result<SangerReviewView, String> {
    let draft = session.working_draft()?;
    let mut view = review_view(&draft)?;
    if let Some(placement) = session.placement()? {
        let indexed = Arc::new(
            basecraft_io::cache::open_fasta(&placement.source_path).map_err(|e| e.to_string())?,
        );
        let alignment =
            AlignmentDoc::from_fasta(placement.source_path.display().to_string(), &indexed);
        view.attach_alignment(placement, alignment)?;
    }
    Ok(view)
}

impl qobject::SangerController {
    fn call_error(mut self: std::pin::Pin<&mut Self>, error: &str) {
        self.as_mut().set_calling(false);
        self.as_mut().set_message_kind(QString::from("fail"));
        self.set_message(QString::from(format!(
            "Alternative calls: {error}. Instrument review remains available."
        )));
    }

    pub fn cancel_calls(mut self: std::pin::Pin<&mut Self>) {
        self.as_mut().rust_mut().pending_calls = false;
        let result = self.as_mut().rust_mut().caller.cancel();
        self.as_mut().set_calling(false);
        match result {
            Ok(()) => {
                self.as_mut().set_message_kind(QString::from("notify"));
                self.set_message(QString::from("Alternative calling cancelled"));
            }
            Err(error) => self.call_error(&error),
        }
    }

    fn retain_calls(
        mut self: std::pin::Pin<&mut Self>,
        called: basecraft_plugin_host::sanger_calls::CalledDraft,
    ) {
        let result = (|| -> Result<(), String> {
            let source = called
                .draft
                .inputs
                .first()
                .ok_or("Caller returned no source")?
                .path
                .clone();
            let mut alternate = SangerReviewSession::from_draft(called.draft, &source)?;
            alternate.attach_operation_run(&called.run)?;
            let mut r = self.as_mut().rust_mut();
            let session = r.review_session.as_mut().ok_or("Trace was closed")?;
            session.retain_call_set(alternate)?;
            session.start_recovery()
        })();
        self.as_mut().set_dirty(true);
        self.as_mut().refresh();
        match result {
            Ok(()) => {
                self.as_mut().set_calling(false);
                self.as_mut().set_message_kind(QString::from("done"));
                self.set_message(QString::from(
                    "Computed calls retained. Choose a call set to compare and review.",
                ));
            }
            Err(error) => self.call_error(&error),
        }
    }

    pub fn compute_calls(self: std::pin::Pin<&mut Self>) {
        self.single_review(20);
    }

    pub fn poll_calls(mut self: std::pin::Pin<&mut Self>) {
        if self.rust().pending_calls {
            self.as_mut().rust_mut().pending_calls = false;
            self.compute_calls();
            return;
        }
        let result = self.as_mut().rust_mut().caller.poll();
        match result {
            Ok(Some(called)) => self.retain_calls(called),
            Ok(None) => {}
            Err(error) => self.call_error(&error),
        }
    }

    pub fn select_calls(mut self: std::pin::Pin<&mut Self>, id: &QString) -> bool {
        let result = (|| -> Result<SangerReviewView, String> {
            let mut r = self.as_mut().rust_mut();
            let session = r.review_session.as_mut().ok_or("No review is open")?;
            session.select_call_set(&id.to_string())?;
            session_view(session)
        })();
        match result {
            Ok(mut view) => {
                let mut r = self.as_mut().rust_mut();
                view.resize(r.view.width, r.view.height);
                r.review = Some(view);
                drop(r);
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
            format!(
                "{}:{}:{}",
                self.rust().file_path,
                self.rust()
                    .review_session
                    .as_ref()
                    .map_or(0, |r| r.events().len()),
                self.rust().dirty
            )
            .as_str(),
        )
    }
    pub fn document_record(&self) -> QString {
        let result = if let Some(review) = &self.rust().review_session {
            review.document_report(self.rust().dirty)
        } else {
            basecraft_ops::document_record::DocumentRecord::empty(
                &self.rust().file_path.to_string(),
            )
            .report()
            .map_err(|e| e.to_string())
        };
        crate::provenance_bridge::report_json(
            result.and_then(|r| serde_json::to_string(&r).map_err(|e| e.to_string())),
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
        let (bytes, status, zoom, review_mode, sequence, decision, unresolved, context) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(review) = &mut r.review {
                if let Some(session) = &r.review_session {
                    review.review_settings = session.settings.clone();
                    review.reference_evidence = session.reference.clone();
                    for i in 0..review.consensus.len() {
                        review.consensus[i] = match session.decision(i) {
                            Some(ReviewChoice::Edit(base) | ReviewChoice::Auto(base)) => {
                                *base as u8
                            }
                            _ => review.draft.columns[i].consensus as u8,
                        };
                        review.masked[i] = session.masked(i);
                        review.unresolved[i] = !session.masked(i)
                            && matches!(
                                session.decision(i),
                                Some(ReviewChoice::Pending | ReviewChoice::Reject)
                            );
                    }
                }
                review.build(&r.theme, &mut r.scratch);
                let zoom = basecraft_viewmodel::zoom::percent_text(
                    f64::from(review.column_width) / 12.0 * 100.0,
                );
                let decision = r
                    .review_session
                    .as_ref()
                    .and_then(|session| {
                        session
                            .decision(review.selected)
                            .map(|value| format!("{value:?}"))
                    })
                    .unwrap_or_default();
                let unresolved = r.review_session.as_ref().map_or(0, |session| {
                    i32::try_from(session.unresolved()).unwrap_or(i32::MAX)
                });
                (
                    r.scratch.to_bytes(),
                    format!(
                        "{unresolved} unresolved · {} masked · {} reads",
                        review.masked.iter().filter(|&&m| m).count(),
                        review.lanes.len()
                    ),
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
                r.view.build_wrapped(trace, &r.theme, &mut r.scratch);
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
        let names: Vec<_> = self.rust().review_session.as_ref().map_or_else(
            || paths.clone(),
            |s| s.draft.inputs.iter().map(|r| r.path.clone()).collect(),
        );
        let name = basecraft_viewmodel::sanger_review::suggested_name(&names);
        let anchor = self
            .rust()
            .review_session
            .as_ref()
            .map(|s| s.storage_anchor().to_path_buf());
        let export = anchor
            .as_deref()
            .and_then(Path::parent)
            .or_else(|| paths.first().and_then(|p| p.parent()))
            .unwrap_or(Path::new("/tmp"))
            .join(format!("{name}_consensus_from_basecraft.fasta"));
        self.as_mut().set_proposed_name(QString::from(&name));
        self.as_mut()
            .set_export_path(QString::from(export.to_string_lossy().as_ref()));
        self.as_mut().set_review_height(height);
        self.as_mut()
            .set_trace_lanes(QString::from(serde_json::to_string(&lanes).unwrap()));
        let mask = self
            .rust()
            .review_session
            .as_ref()
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
            .rust()
            .review_session
            .as_ref()
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
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
        self.as_mut().set_status(QString::from(&status));
        self.as_mut().set_zoom_text(QString::from(&zoom));
        self.as_mut().set_review_mode(review_mode);
        self.as_mut().set_review_sequence(QString::from(&sequence));
        self.as_mut().set_review_decision(QString::from(&decision));
        self.as_mut().set_unresolved(unresolved);
        self.as_mut().set_alignment_context(QString::from(&context));
        let (sets, selected) = self.rust().review_session.as_ref().map_or_else(
            || (serde_json::json!([]), String::new()), |session| {
                (serde_json::json!(session.call_sets().iter().enumerate().map(|(i, (id, engine))|
                    serde_json::json!({"id":id,"label":format!("{} calls · {}", if *engine == "ABI" { "Instrument" } else { "Computed" }, i + 1)})).collect::<Vec<_>>()),
                session.draft_sha256.clone())
            });
        self.as_mut().set_call_sets(QString::from(sets.to_string()));
        self.as_mut().set_selected_call_set(QString::from(selected));
    }

    fn install_draft_run(mut self: std::pin::Pin<&mut Self>, run: &SangerRun, path: &Path) -> bool {
        if !self
            .as_mut()
            .open_draft(&QString::from(path.to_string_lossy().as_ref()))
        {
            return false;
        }
        self.as_mut().set_dirty(true);
        let result = self
            .as_mut()
            .rust_mut()
            .review_session
            .as_mut()
            .ok_or_else(|| "No review is open".to_string())
            .and_then(|session| session.attach_run(run));
        if let Err(error) = result {
            self.as_mut().set_message_kind(QString::from("fail"));
            self.as_mut().set_message(QString::from(error));
            return false;
        }
        self.refresh();
        true
    }

    pub fn orient_review(mut self: std::pin::Pin<&mut Self>, alignment: &QString) -> bool {
        let result = (|| -> Result<(SangerRun, Option<PathBuf>), String> {
            let session = self
                .rust()
                .review_session
                .as_ref()
                .ok_or("Open a draft first")?;
            let draft = session.working_draft()?;
            let mut sources: Vec<_> = draft.inputs.iter().map(|r| r.path.clone()).collect();
            sources.push(PathBuf::from(alignment.to_string()));
            let (run, output, mut outputs) = sanger_tool(
                session.storage_anchor(),
                "mafft",
                "add_sequences",
                &sources,
                session.evidence_runs(),
                "orientation.json",
                serde_json::json!({"input":alignment.to_string(),"rows":[["probe",draft.consensus.replace('-',"")]],"orientation":true,"keep":true}),
            )?;
            outputs.push(run.review_snapshot(session).map_err(|e| e.to_string())?);
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&output).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let evidence = if value["reversed"].as_array().is_none_or(Vec::is_empty) {
                None
            } else {
                if !session.automatic_only() {
                    return Err(
                    "This review already has decisions; open a fresh draft to change orientation"
                        .into(),
                );
                }
                // Reverse the immutable original, not its transient resolved paths.
                let mut reversed = session.draft.reverse_complemented();
                reversed.limitations.push(format!(
                    "Draft reverse-complemented for alignment context; source draft SHA-256 {}",
                    session.draft_sha256
                ));
                let evidence = run.artifact("evidence.json").map_err(|e| e.to_string())?;
                basecraft_ops::input_evidence::write_json(&evidence, &reversed)
                    .map_err(|e| e.to_string())?;
                outputs.push(evidence.clone());
                Some(evidence)
            };
            Ok((run.finish(&outputs).map_err(|e| e.to_string())?, evidence))
        })();
        match result {
            Ok((run, Some(path))) => self.install_draft_run(&run, &path),
            Ok((run, None)) => {
                let result = self
                    .as_mut()
                    .rust_mut()
                    .review_session
                    .as_mut()
                    .ok_or_else(|| "No review is open".to_string())
                    .and_then(|session| session.include_run(&run));
                if let Err(error) = result {
                    self.as_mut().set_message(QString::from(error));
                    self.as_mut().set_message_kind(QString::from("fail"));
                    return false;
                }
                self.as_mut().set_dirty(true);
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("fail"));
                self.as_mut()
                    .set_message(QString::from(crate::bridge::error_summary(&error)));
                false
            }
        }
    }

    pub fn single_review(mut self: std::pin::Pin<&mut Self>, signal_threshold: i32) -> bool {
        self.as_mut().rust_mut().pending_calls = false;
        let Ok(threshold) = u32::try_from(signal_threshold) else {
            return false;
        };
        let source = self.rust().review_session.as_ref().and_then(|session| {
            (session.draft.inputs.len() == 1).then(|| session.draft.inputs[0].clone())
        });
        let Some(source) = source else {
            return false;
        };
        let result = self
            .as_mut()
            .rust_mut()
            .caller
            .start(&source.path, &source.sha256, threshold);
        match result {
            Ok(Some(called)) => self.retain_calls(called),
            Ok(None) => {
                self.as_mut().set_calling(true);
                self.as_mut().set_message_kind(QString::from("notify"));
                self.as_mut().set_message(QString::from(
                    "Computing alternative calls in background; current calls remain selected",
                ));
            }
            Err(error) => {
                self.call_error(&error);
                return false;
            }
        }
        true
    }

    pub fn reviewed_evidence(mut self: std::pin::Pin<&mut Self>) -> QString {
        let result = self
            .as_mut()
            .rust_mut()
            .review_session
            .as_mut()
            .ok_or_else(|| "No review is open".to_string())
            .and_then(SangerReviewSession::addition_evidence);
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
            .rust()
            .review_session
            .as_ref()
            .and_then(SangerReviewSession::resolved_sequence)
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
            .rust()
            .review_session
            .as_ref()
            .and_then(SangerReviewSession::resolved_sequence);
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
        match basecraft_ops::sanger_review::evidence_kind(Path::new(&path.to_string())) {
            Some(basecraft_ops::sanger_review::EvidenceKind::Review) => {
                return self.open_review(path);
            }
            Some(basecraft_ops::sanger_review::EvidenceKind::Draft) => {
                return self.open_draft(path);
            }
            None => {}
        }
        let path = PathBuf::from(path.to_string());
        let loaded = basecraft_ops::sanger_review::instrument_review(&path)
            .and_then(|session| session_view(&session).map(|view| (session, view)));
        match loaded {
            Ok((session, mut view)) => {
                self.as_mut().cancel_calls();
                {
                    let mut r = self.as_mut().rust_mut();
                    view.resize(r.view.width, r.view.height);
                    // Keep the raw trace for the existing multi-read draft action.
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

    pub fn open_draft(mut self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        let path = PathBuf::from(path.to_string());
        let result = (|| -> Result<(SangerReviewView, SangerReviewSession), String> {
            let mut session = SangerReviewSession::new(&path)?;
            // Opening evidence is inspection. Re-running Tracy to enrich older
            // evidence must be an explicit analysis, never a passive-open write.
            let mut view = session_view(&session)?;
            let calls = (0..view.draft.columns.len())
                .map(|i| view.assessment(i).call)
                .collect();
            session.apply(ReviewOp::Evaluate {
                version: 2,
                settings: view.review_settings.clone(),
                calls,
                reference: None,
            })?;
            if let Some(i) = (0..view.draft.columns.len()).find(|&i| view.requires_review(i)) {
                view.select_column(i);
            }
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
                    r.review_path = None;

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

    pub fn open_review(mut self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        let path = PathBuf::from(path.to_string());
        let result = (|| -> Result<(SangerReviewView, SangerReviewSession), String> {
            let session = SangerReviewSession::load(&path)?;
            let view = session_view(&session)?;
            Ok((view, session))
        })();
        match result {
            Ok((mut review, session)) => {
                self.as_mut().cancel_calls();
                let recovered = session.recovery_path().is_some();
                {
                    let mut r = self.as_mut().rust_mut();
                    review.resize(r.view.width, r.view.height);
                    r.review = Some(review);
                    r.review_session = Some(session);
                    r.review_path = if recovered { None } else { Some(path.clone()) };

                    r.trace = None;
                }
                self.as_mut().set_dirty(recovered);
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
        let result = (|| -> Result<(SangerRun, PathBuf), String> {
            if !(1..=90).contains(&signal_threshold) {
                return Err("Signal cutoff must be between 1 and 90 percent".into());
            }
            let mut paths = vec![self.rust().file_path.to_string()];
            if self.rust().trace.is_none() {
                return Err("Open an AB1 trace before drafting a consensus".into());
            }
            let additional: Vec<String> = serde_json::from_str(&additional_paths_json.to_string())
                .map_err(|e| e.to_string())?;
            paths.extend(additional);
            if !(2..=3).contains(&paths.len()) {
                return Err("Exactly two or three AB1 reads are required".into());
            }
            let sources: Vec<_> = paths.iter().map(PathBuf::from).collect();
            let (run, output, outputs) = sanger_tool(
                &sources[0],
                "tracy-sanger",
                "consensus",
                &sources,
                &[],
                "draft",
                serde_json::json!({"reads":paths,"signal_threshold":signal_threshold}),
            )?;
            Ok((
                run.finish(&outputs).map_err(|e| e.to_string())?,
                output.join("evidence.json"),
            ))
        })();
        match result {
            Ok((run, evidence)) => {
                let loaded = self.as_mut().install_draft_run(&run, &evidence);
                if loaded {
                    self.as_mut()
                        .set_message(QString::from("Draft review retained; resume from Tasks"));
                    self.as_mut().set_message_kind(QString::from("done"));
                }
                loaded
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                false
            }
        }
    }

    pub fn save_review(mut self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
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
        let column = self.rust().review.as_ref().map(|review| review.selected);
        let events_before = self
            .rust()
            .review_session
            .as_ref()
            .map_or(0, |s| s.events().len());
        let result = match (self.as_mut().rust_mut().review_session.as_mut(), column) {
            (Some(session), Some(column)) => session
                .apply(ReviewOp::Set {
                    columns: vec![column],
                    choice,
                })
                .and_then(|()| session.start_recovery()),
            _ => Err("No Sanger review is open".into()),
        };
        if self
            .rust()
            .review_session
            .as_ref()
            .is_some_and(|s| s.events().len() > events_before)
        {
            self.as_mut().set_dirty(true);
        }
        match result {
            Ok(()) => {
                self.as_mut().set_dirty(true);
                self.as_mut().set_message(QString::default());
                self.as_mut().set_message_kind(QString::default());
                self.refresh();
                true
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                self.refresh();
                false
            }
        }
    }

    fn review_history(mut self: std::pin::Pin<&mut Self>, op: ReviewOp) {
        let events_before = self
            .rust()
            .review_session
            .as_ref()
            .map_or(0, |s| s.events().len());
        let result = self
            .as_mut()
            .rust_mut()
            .review_session
            .as_mut()
            .ok_or_else(|| "No Sanger review is open".to_string())
            .and_then(|session| session.apply(op).and_then(|()| session.start_recovery()));
        if self
            .rust()
            .review_session
            .as_ref()
            .is_some_and(|s| s.events().len() > events_before)
        {
            self.as_mut().set_dirty(true);
        }
        match result {
            Ok(()) => {
                self.as_mut().set_dirty(true);
                self.as_mut().set_message(QString::default());
                self.as_mut().set_message_kind(QString::default());
                self.refresh();
            }
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                self.refresh();
            }
        }
    }

    pub fn apply_review_settings(
        mut self: std::pin::Pin<&mut Self>,
        settings_json: &QString,
    ) -> bool {
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
        let Some(session) = &self.rust().review_session else {
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
                if let (Some(view), Some(session)) = (&mut r.review, &r.review_session) {
                    if session.masked(view.selected) {
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
            }
            self.refresh();
        }
        ok
    }
    pub fn mask_reference(mut self: std::pin::Pin<&mut Self>) -> bool {
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
        mut self: std::pin::Pin<&mut Self>,
        forward: &QString,
        reverse: &QString,
        overlap: i32,
        errors: i32,
    ) -> QString {
        let result = (|| -> Result<(String, SangerRun), String> {
            let review = self.rust().review.as_ref().ok_or("No review open")?;
            let sequence: String = review
                .consensus
                .iter()
                .filter(|&&b| b != b'-')
                .map(|&b| char::from(b))
                .collect();
            let session = self
                .rust()
                .review_session
                .as_ref()
                .ok_or("No review open")?;
            let sources: Vec<_> = session
                .working_draft()?
                .inputs
                .iter()
                .map(|r| r.path.clone())
                .collect();
            let (run, output, mut outputs) = sanger_tool(
                session.storage_anchor(),
                "tracy-sanger",
                "primers",
                &sources,
                session.evidence_runs(),
                "primers.json",
                serde_json::json!({"sequence":sequence,"forward":forward.to_string(),"reverse":reverse.to_string(),"minimum_overlap":overlap,"error_percent":errors}),
            )?;
            outputs.push(run.review_snapshot(session).map_err(|e| e.to_string())?);
            let text = std::fs::read_to_string(&output).map_err(|e| e.to_string())?;
            Ok((text, run.finish(&outputs).map_err(|e| e.to_string())?))
        })();
        match result {
            Ok((text, run)) => {
                let result = self
                    .as_mut()
                    .rust_mut()
                    .review_session
                    .as_mut()
                    .ok_or_else(|| "No review open".to_string())
                    .and_then(|session| session.include_run(&run));
                if let Err(error) = result {
                    self.as_mut().set_message(QString::from(error));
                    return QString::default();
                }
                self.as_mut().set_dirty(true);
                self.as_mut().set_message(QString::default());
                QString::from(text)
            }
            Err(e) => {
                self.as_mut().set_message(QString::from(e));
                QString::default()
            }
        }
    }
    pub fn apply_primers(mut self: std::pin::Pin<&mut Self>, proposal: &QString) -> bool {
        let result = (|| -> Result<MaskSettings, String> {
            let value: serde_json::Value =
                serde_json::from_str(&proposal.to_string()).map_err(|e| e.to_string())?;
            let review = self.rust().review.as_ref().ok_or("No review open")?;
            let session = self
                .rust()
                .review_session
                .as_ref()
                .ok_or("No review open")?;
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
        self.review_history(ReviewOp::Undo);
    }
    pub fn review_redo(self: std::pin::Pin<&mut Self>) {
        self.review_history(ReviewOp::Redo);
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
        let review_path = self.rust().review_path.clone();
        let output = PathBuf::from(path.to_string());
        let events_before = self
            .rust()
            .review_session
            .as_ref()
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
                    .rust()
                    .review_session
                    .as_ref()
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

    pub fn place_alignment(mut self: std::pin::Pin<&mut Self>, path: &QString) -> bool {
        let source = PathBuf::from(path.to_string());
        let result = (|| -> Result<(SangerRun, PathBuf, SangerPlacement, AlignmentDoc), String> {
            let fragment = self
                .rust()
                .review
                .as_ref()
                .ok_or("Open a Sanger draft before placing it")?
                .draft
                .consensus
                .replace('-', "");
            let session = self
                .rust()
                .review_session
                .as_ref()
                .ok_or("No review open")?;
            let (run, output, mut outputs) = sanger_tool(
                session.storage_anchor(),
                "mafft",
                "place_sanger",
                std::slice::from_ref(&source),
                session.evidence_runs(),
                "placement.json",
                serde_json::json!({"input":source,"fragment":fragment}),
            )?;
            outputs.push(run.review_snapshot(session).map_err(|e| e.to_string())?);
            let placement = SangerPlacement::read(&output)?;
            let hash = basecraft_io::sha256_file(&source).map_err(|e| e.to_string())?;
            if !hash.eq_ignore_ascii_case(&placement.source_sha256) {
                return Err("Source alignment changed during placement".into());
            }
            let indexed =
                Arc::new(basecraft_io::cache::open_fasta(&source).map_err(|e| e.to_string())?);
            let alignment = AlignmentDoc::from_fasta(source.display().to_string(), &indexed);
            Ok((
                run.finish(&outputs).map_err(|e| e.to_string())?,
                output,
                placement,
                alignment,
            ))
        })();
        match result {
            Ok((run, output, placement, alignment)) => {
                let result = self
                    .as_mut()
                    .rust_mut()
                    .review
                    .as_mut()
                    .ok_or_else(|| "No Sanger draft is open".to_string())
                    .and_then(|review| review.attach_alignment(placement, alignment));
                match result {
                    Ok(()) => {
                        self.as_mut().set_dirty(true);
                        let attached = self
                            .as_mut()
                            .rust_mut()
                            .review_session
                            .as_mut()
                            .ok_or_else(|| "No review is open".to_string())
                            .and_then(|session| session.attach_placement(&run, &output));
                        if let Err(error) = attached {
                            self.as_mut().set_message(QString::from(error));
                            self.as_mut().set_message_kind(QString::from("fail"));
                            return false;
                        }
                        let automatic =
                            self.rust().review_session.as_ref().is_some_and(|session| {
                                session.events().iter().all(|op| {
                                    matches!(
                                        op,
                                        ReviewOp::AutoAccept { .. } | ReviewOp::Evaluate { .. }
                                    )
                                })
                            });
                        let settings = self.rust().review_settings.clone();
                        self.as_mut().apply_review_settings(&settings);
                        if automatic {
                            self.as_mut().mask_reference();
                        }
                        self.as_mut().set_message_kind(QString::from("done"));
                        self.as_mut()
                            .set_message(QString::from("Alignment context loaded"));
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
            Err(error) => {
                self.as_mut().set_message_kind(QString::from("error"));
                self.as_mut().set_message(QString::from(&error));
                false
            }
        }
    }

    pub fn next_issue(mut self: std::pin::Pin<&mut Self>, forward: bool) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let (Some(review), Some(session)) = (&mut r.review, &r.review_session) {
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

    pub fn check_theme(mut self: std::pin::Pin<&mut Self>) {
        let font = QString::from(&monospace_font());
        if font != self.rust().mono_font {
            self.as_mut().set_mono_font(font);
        }
        let Some(path) = omarchy_theme_path() else {
            return;
        };
        let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if stamp == self.rust().theme_stamp {
            return;
        }
        if let Some((theme, stamp)) = load_theme() {
            self.as_mut()
                .set_background(QString::from(&css(theme.background)));
            self.as_mut()
                .set_foreground(QString::from(&css(theme.foreground)));
            self.as_mut().set_accent(QString::from(&css(theme.accent)));
            self.as_mut()
                .set_ok_color(QString::from(&css(theme.nearest_hue(120.0))));
            self.as_mut()
                .set_error_color(QString::from(&css(theme.nearest_hue(0.0))));
            {
                let mut r = self.as_mut().rust_mut();
                r.theme = theme;
                r.theme_stamp = stamp;
            }
            self.refresh();
        }
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
        self.export_figure_with(path, scope, publication, false);
    }

    pub fn export_figure_png(
        self: std::pin::Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
    ) {
        self.export_figure_with(path, scope, publication, true);
    }

    fn export_figure_with(
        mut self: std::pin::Pin<&mut Self>,
        path: &QString,
        scope: i32,
        publication: bool,
        png: bool,
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
                Theme::publication()
            } else {
                r.theme.clone()
            };
            if let Some(review) = &mut r.review {
                if let Some(session) = &r.review_session {
                    review.review_settings = session.settings.clone();
                    review.reference_evidence = session.reference.clone();
                    for i in 0..review.consensus.len() {
                        review.consensus[i] = match session.decision(i) {
                            Some(ReviewChoice::Edit(base) | ReviewChoice::Auto(base)) => {
                                *base as u8
                            }
                            _ => review.draft.columns[i].consensus as u8,
                        };
                        review.masked[i] = session.masked(i);
                        review.unresolved[i] = !session.masked(i)
                            && matches!(
                                session.decision(i),
                                Some(ReviewChoice::Pending | ReviewChoice::Reject)
                            );
                    }
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
            if png {
                let bytes = crate::bridge::figure_png(&xml)?;
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
            Ok(()) => self.as_mut().set_message(QString::from(if png {
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
