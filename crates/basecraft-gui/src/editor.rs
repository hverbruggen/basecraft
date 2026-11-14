//! What the five editor bridges share: their state and the methods that are
//! the same in each. The Qt declarations stay in each bridge, because
//! cxx-qt reads them from the source; these macros supply the Rust side.

/// The Rust state of one editor bridge, with its `Default`. Every editor gets
/// the find, status, message and file fields. `drawn` adds the frame, theme
/// colours and overview of an editor that paints its document; `documents`
/// adds the ops session and the open document. The bridge lists its own
/// fields in the braces as `name: Type = default,`, each ending in a comma.
/// They come before the shared ones, so they are dropped first: a running
/// job is cancelled before the session goes.
macro_rules! editor_state {
    ($name:ident [drawn $($flag:ident)*] { $($rest:tt)* }) => {
        crate::editor::editor_state!($name [$($flag)*] {
            $($rest)*
            frame: cxx_qt_lib::QByteArray = cxx_qt_lib::QByteArray::default(),
            ok_color: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            error_color: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            background: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            foreground: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            accent: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            mono_font: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            zoom_text: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            overview_content: cxx_qt_lib::QString = cxx_qt_lib::QString::from("{\"shapes\":[]}"),
            overview_matches: cxx_qt_lib::QString = cxx_qt_lib::QString::from("[]"),
            overview_cache: basecraft_viewmodel::overview::Cache = Default::default(),
            overview_match_cache: basecraft_viewmodel::overview::Cache = Default::default(),
            theme: basecraft_viewmodel::Theme = basecraft_viewmodel::Theme::default(),
            scratch: basecraft_viewmodel::Frame = basecraft_viewmodel::Frame::new(),
        });
    };
    ($name:ident [documents $($flag:ident)*] { $($rest:tt)* }) => {
        crate::editor::editor_state!($name [$($flag)*] {
            $($rest)*
            session: Option<basecraft_ops::Session> = None,
            doc: Option<basecraft_ops::DocId> = None,
        });
    };
    ($name:ident [] { $($(#[$meta:meta])* $field:ident: $ty:ty = $default:expr),* $(,)? }) => {
        #[allow(clippy::struct_excessive_bools)]
        pub struct $name {
            find: basecraft_viewmodel::find::Session,
            find_status: cxx_qt_lib::QString,
            find_count: i32,
            find_row: i32,
            find_hit_y: f64,
            find_nucleotide: bool,
            find_rows: cxx_qt_lib::QString,
            find_reveals: i32,
            history_revision: cxx_qt_lib::QString,
            status: cxx_qt_lib::QString,
            message: cxx_qt_lib::QString,
            /// "ok", "error", or "" for a neutral hint.
            message_kind: cxx_qt_lib::QString,
            file_path: cxx_qt_lib::QString,
            dirty: bool,
            busy: bool,
            /// The copy offer for an edit a read-only folder refused (D13).
            refused_edit: cxx_qt_lib::QString,
            $($(#[$meta])* $field: $ty,)*
        }
        impl Default for $name {
            fn default() -> Self {
                Self {
                    find: basecraft_viewmodel::find::Session::default(),
                    find_status: cxx_qt_lib::QString::default(),
                    find_count: 0,
                    find_row: -1,
                    find_hit_y: 0.0,
                    find_nucleotide: false,
                    find_rows: cxx_qt_lib::QString::from("[]"),
                    find_reveals: 0,
                    history_revision: cxx_qt_lib::QString::default(),
                    status: cxx_qt_lib::QString::default(),
                    message: cxx_qt_lib::QString::default(),
                    message_kind: cxx_qt_lib::QString::default(),
                    file_path: cxx_qt_lib::QString::default(),
                    dirty: false,
                    busy: false,
                    refused_edit: cxx_qt_lib::QString::default(),
                    $($field: $default,)*
                }
            }
        }
    };
}
pub(crate) use editor_state;

/// The methods every document editor (sequence, alignment, tree, species
/// workspace) has in the same form. `$getter` is the `Session` method that
/// returns the editor's document. Flags: `fresh` adds `install_fresh` for
/// editors the application opens into new tabs; `fail` adds the plain error
/// message (the species editor writes its own).
///
/// The bridge supplies `install_opened`, `refresh`, and two undo steps:
/// `before_history`, which may refuse undo and redo, and `after_history`,
/// which brings the view up to date after the document changed.
macro_rules! document_editor {
    ($controller:ty, $getter:ident [fresh $($flag:ident)*]) => {
        impl $controller {
            pub(crate) fn install_fresh(
                self: std::pin::Pin<&mut Self>,
                candidate: crate::document_open::Candidate,
            ) -> Result<(), crate::document_open::OpenError> {
                if self.rust().session.is_some() || self.rust().doc.is_some() || self.rust().busy {
                    return Err(crate::document_open::OpenError::StaleIdentity);
                }
                self.install_opened(candidate)
            }
        }
        crate::editor::document_editor!($controller, $getter [$($flag)*]);
    };
    ($controller:ty, $getter:ident [fail $($flag:ident)*]) => {
        impl $controller {
            /// Something failed.
            fn fail(mut self: std::pin::Pin<&mut Self>, msg: &str) {
                self.as_mut().set_message_kind(cxx_qt_lib::QString::from("error"));
                self.as_mut().set_message(cxx_qt_lib::QString::from(msg));
            }
        }
        crate::editor::document_editor!($controller, $getter [$($flag)*]);
    };
    ($controller:ty, $getter:ident []) => {
        impl $controller {
            /// End the closed tab's session now (Main.qml `removeDocument`).
            pub fn close_session(mut self: std::pin::Pin<&mut Self>) {
                self.as_mut().rust_mut().session = None;
            }
            pub fn document_record(&self) -> cxx_qt_lib::QString {
                crate::provenance_bridge::session_report(self.rust().session.as_ref(), self.rust().doc)
            }
            pub fn record_revision(&self) -> cxx_qt_lib::QString {
                crate::provenance_bridge::session_revision(self.rust().session.as_ref(), self.rust().doc)
            }
            pub fn memory_report(&self) -> cxx_qt_lib::QString {
                cxx_qt_lib::QString::from(&crate::bridge::memory_text())
            }
            /// A neutral hint. Messages stay brief: say what happened.
            fn notify(mut self: std::pin::Pin<&mut Self>, msg: &str) {
                self.as_mut().set_message_kind(cxx_qt_lib::QString::default());
                self.as_mut().set_message(cxx_qt_lib::QString::from(msg));
            }
            /// Something completed.
            fn done(mut self: std::pin::Pin<&mut Self>, msg: &str) {
                self.as_mut().set_message_kind(cxx_qt_lib::QString::from("ok"));
                self.as_mut().set_message(cxx_qt_lib::QString::from(msg));
            }
            fn find_y(&self) -> f32 {
                0.0
            }

            /// The start of every refresh: find, history and recovery state.
            /// False when the document can no longer be read; the message
            /// then says why.
            fn refresh_start(mut self: std::pin::Pin<&mut Self>) -> bool {
                self.as_mut().find_update(false);
                let history = self.record_revision();
                self.as_mut().set_history_revision(history);
                if let Some(warning) =
                    crate::document_open::recovery_notice(self.rust().session.as_ref(), self.rust().doc)
                {
                    self.as_mut().set_message_kind(cxx_qt_lib::QString::from("error"));
                    self.as_mut().set_message(warning);
                }
                if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                }
                true
            }

            pub fn open_file(mut self: std::pin::Pin<&mut Self>, path: &cxx_qt_lib::QString) -> bool {
                if self.rust().busy {
                    self.fail("Wait for the running operation before replacing this document");
                    return false;
                }
                self.as_mut().set_busy(true);
                crate::document_open::open_file(self, path, |mut this, result| {
                    let result = result.and_then(|candidate| this.as_mut().install_opened(candidate));
                    this.as_mut().set_busy(false);
                    let ok = result.is_ok();
                    if let Err(error) = result {
                        this.as_mut().fail(&error.to_string());
                    }
                    this.opened(ok);
                })
            }

            pub fn undo(self: std::pin::Pin<&mut Self>) {
                self.history(false);
            }
            pub fn redo(self: std::pin::Pin<&mut Self>) {
                self.history(true);
            }
            /// Undo or redo one step. The document is unsaved exactly when
            /// the session says so: undoing back to the saved state leaves
            /// it clean.
            fn history(mut self: std::pin::Pin<&mut Self>, redo: bool) {
                if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                }
                if !self.as_mut().before_history() {
                    return;
                }
                let result = {
                    let mut r = self.as_mut().rust_mut();
                    let r = &mut *r;
                    let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                        return;
                    };
                    if redo { session.redo(doc) } else { session.undo(doc) }
                };
                match result {
                    Ok(_) => {
                        let dirty =
                            crate::document_open::unsaved(self.rust().session.as_ref(), self.rust().doc);
                        self.as_mut().set_dirty(dirty);
                        // Undoing or redoing makes the last message stale.
                        self.as_mut().notify("");
                        self.as_mut().after_history();
                    }
                    Err(e) => self.as_mut().fail(&e.to_string()),
                }
                self.refresh();
            }
        }
    };
}
pub(crate) use document_editor;

/// Figure export for an editor that draws its document. The bridge module
/// supplies `figure_svg(view, document, scope, theme, font, include_selection)`
/// and `has_figure_selection(view)`. Scope 0 is the visible view, 1 the selection,
/// 2 the whole document.
macro_rules! figure_export {
    ($controller:ty, $getter:ident) => {
        impl $controller {
            pub fn export_figure(
                self: std::pin::Pin<&mut Self>,
                path: &cxx_qt_lib::QString,
                scope: i32,
                publication: bool,
            ) {
                self.export_figure_with(path, scope, publication, None, false);
            }
            pub fn export_figure_png(
                self: std::pin::Pin<&mut Self>,
                path: &cxx_qt_lib::QString,
                scope: i32,
                publication: bool,
                width: f64,
            ) {
                self.export_figure_with(path, scope, publication, Some(width), false);
            }
            fn export_figure_with(
                mut self: std::pin::Pin<&mut Self>,
                path: &cxx_qt_lib::QString,
                scope: i32,
                publication: bool,
                png_width: Option<f64>,
                include_selection: bool,
            ) {
                if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                }
                if !(0..=2).contains(&scope) {
                    self.fail("Unknown figure scope");
                    return;
                }
                if scope == 1 && !has_figure_selection(&self.rust().view) {
                    self.fail("Select a region or clade first");
                    return;
                }
                let target = std::path::PathBuf::from(path.to_string());
                let result = (|| -> basecraft_ops::Result<()> {
                    let mut r = self.as_mut().rust_mut();
                    let r = &mut *r;
                    let Some(doc) = r.doc else { return Ok(()) };
                    let Some(session) = &r.session else { return Ok(()) };
                    let theme = if publication {
                        crate::appearance::publication_theme()
                    } else {
                        r.theme.clone()
                    };
                    let font = r.mono_font.to_string();
                    let document = session.$getter(doc)?;
                    let svg = figure_svg(&mut r.view, document, scope, &theme, &font, include_selection)?;
                    let Some(session) = r.session.as_mut() else { return Ok(()) };
                    if let Some(width) = png_width {
                        let bytes = crate::figure_png::render(&svg, width)
                            .map_err(basecraft_ops::Error::Invalid)?;
                        session.export(doc, &target, "png", |w| std::io::Write::write_all(w, &bytes))
                    } else {
                        session.export(doc, &target, "svg", |w| std::io::Write::write_all(w, svg.as_bytes()))
                    }
                })();
                match result {
                    Ok(()) => self.done(&format!("Exported {}", crate::bridge::file_name(&target))),
                    Err(e) => self.fail(&format!("Export failed: {e}")),
                }
            }
        }
    };
}
pub(crate) use figure_export;

/// Number the names the duplicate-name offer named (D29) in an open
/// alignment or tree, as one undo step.
macro_rules! number_names {
    ($controller:ty) => {
        impl $controller {
            pub fn number_duplicates(mut self: std::pin::Pin<&mut Self>) -> bool {
                let result = {
                    let mut r = self.as_mut().rust_mut();
                    let r = &mut *r;
                    let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                        return false;
                    };
                    crate::document_open::duplicates(session, doc).and_then(|found| {
                        let Some(found) = found else { return Ok(0) };
                        basecraft_ops::renames::make_unique(session, doc, &found)
                            .map(|()| found.renames.len())
                    })
                };
                self.as_mut()
                    .set_duplicate_names(cxx_qt_lib::QString::default());
                match result {
                    Ok(count) => {
                        let dirty = crate::document_open::unsaved(
                            self.rust().session.as_ref(),
                            self.rust().doc,
                        );
                        self.as_mut().set_dirty(dirty);
                        self.as_mut().after_history();
                        self.as_mut().done(&format!("Numbered {count} names"));
                        self.refresh();
                        true
                    }
                    Err(e) => {
                        self.fail(&e.to_string());
                        false
                    }
                }
            }
        }
    };
}
pub(crate) use number_names;

/// What the alignment and tree editors share as views of named taxa: their
/// document, its text for screen readers, the taxa selected and linked to
/// other views, and creating a species workspace from the document. The
/// bridge module supplies `selected_names(view, document)`.
macro_rules! taxa_editor {
    ($controller:ty, $getter:ident, $document:ty) => {
        impl $controller {
            fn with_doc<T>(&self, f: impl FnOnce(&$document) -> T) -> basecraft_ops::Result<Option<T>> {
                let r = self.rust();
                match (&r.session, r.doc) {
                    (Some(s), Some(d)) => Ok(Some(f(s.$getter(d)?))),
                    _ => Ok(None),
                }
            }
            fn check_document(&self) -> basecraft_ops::Result<()> {
                self.with_doc(|_| ()).map(|_| ())
            }
            pub fn accessible_page(&self, page: i32) -> cxx_qt_lib::QString {
                let result = crate::bridge::checked_document!(
                    self.qt_thread(),
                    self.with_doc(|d| basecraft_viewmodel::accessible::$getter(d, page))
                );
                let value = result.map_or_else(
                    || serde_json::json!({"page": 0, "pages": 1, "text": "No document loaded"}),
                    |p| serde_json::json!({"page": p.page, "pages": p.pages, "text": p.text}),
                );
                cxx_qt_lib::QString::from(&value.to_string())
            }

            #[cfg(test)]
            #[allow(dead_code)] // Read by the real-controller bootstrap journey.
            pub(crate) fn linked_taxa_snapshot(&self) -> Vec<String> {
                let mut taxa: Vec<_> = self.rust().view.linked_taxa.iter().cloned().collect();
                taxa.sort();
                taxa
            }
            /// Highlight the taxa another view selected, a JSON list of names.
            pub fn highlight_taxa(mut self: std::pin::Pin<&mut Self>, names: &cxx_qt_lib::QString) {
                if let Ok(taxa) =
                    serde_json::from_str::<std::collections::HashSet<String>>(&names.to_string())
                {
                    self.as_mut().rust_mut().view.linked_taxa = taxa;
                    self.refresh();
                }
            }
            /// Tell linked views which taxa are selected here, when that changed.
            fn emit_taxa(mut self: std::pin::Pin<&mut Self>) {
                let access_thread = self.qt_thread();
                if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                }
                let taxa = {
                    let r = self.rust();
                    let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                        return;
                    };
                    let document = crate::bridge::checked_document!(access_thread, session.$getter(doc));
                    serde_json::to_string(&selected_names(&r.view, document)).unwrap_or_default()
                };
                if taxa != self.rust().last_taxa {
                    self.as_mut().rust_mut().last_taxa.clone_from(&taxa);
                    self.taxa_selected(cxx_qt_lib::QString::from(&taxa));
                }
            }

            pub fn create_delimitation(mut self: std::pin::Pin<&mut Self>, method: &cxx_qt_lib::QString) -> bool {
                if self.rust().busy {
                    return false;
                }
                let Some(doc) = self.rust().doc else {
                    return false;
                };
                let Some(session) = self.as_mut().rust_mut().session.take() else {
                    return false;
                };
                let method = method.to_string();
                self.as_mut().set_busy(true);
                self.as_mut().notify("Creating species workspace");
                let qt_thread = self.qt_thread();
                let mut session = Some(session);
                let spawn = crate::jobs::with_session(
                    qt_thread.clone(),
                    &mut session,
                    move |session, work| {
                        crate::delimitation_bridge::create_workspace_in_session(session, work, doc, &method)
                    },
                    move |mut this, session, result| {
                        this.as_mut().rust_mut().session = Some(session);
                        this.as_mut().set_busy(false);
                        match result {
                            Ok(path) => {
                                this.as_mut().done("Species workspace created");
                                this.workspace_created(cxx_qt_lib::QString::from(path));
                            }
                            Err(error) => {
                                this.as_mut().fail(&error);
                                this.workspace_created(cxx_qt_lib::QString::default());
                            }
                        }
                    },
                );
                if let Err(error) = spawn {
                    self.as_mut().rust_mut().session = session;
                    self.as_mut().set_busy(false);
                    self.fail(&error);
                    return false;
                }
                true
            }

            pub fn resize(mut self: std::pin::Pin<&mut Self>, width: f64, height: f64) {
                self.as_mut().rust_mut().view.resize(f(width), f(height));
                self.refresh();
            }
            pub fn fit(mut self: std::pin::Pin<&mut Self>) {
                self.as_mut().rust_mut().view.fit();
                self.refresh();
            }
        }
    };
}
pub(crate) use taxa_editor;
