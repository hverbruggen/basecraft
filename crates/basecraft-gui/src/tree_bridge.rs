//! The Rust object QML talks to for one tree editor.
//!
//! As with the other editors, QML forwards raw input; the view model turns
//! it into viewport changes, selections and operations, and every edit goes
//! through the ops session.

use basecraft_ops::records::AlignmentLinkExt;
#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for TreeController {}
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
        #[qproperty(QString, canvas_geometry)]
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
        #[qproperty(bool, busy)]
        #[qproperty(bool, dirty)]
        #[qproperty(QString, zoom_text)]
        #[qproperty(f64, hscroll_pos)]
        #[qproperty(f64, hscroll_size)]
        #[qproperty(f64, vscroll_pos)]
        #[qproperty(f64, vscroll_size)]
        /// A node is selected; `tip_selected` if it's a tip.
        #[qproperty(bool, has_selection)]
        #[qproperty(bool, can_auto_root)]
        #[qproperty(QString, linked_alignment)]
        #[qproperty(QString, source_alignment)]
        #[qproperty(QString, alignment_link_status)]
        #[qproperty(bool, alignment_link_inferred)]
        #[qproperty(bool, tip_selected)]
        /// Where support values sit: "natural", "above" or "below".
        #[qproperty(QString, support_placement)]
        /// Style sizes in pixels, for the ribbon's size fields.
        #[qproperty(i32, tip_font)]
        #[qproperty(i32, support_font)]
        #[qproperty(i32, branch_width)]
        /// The tree has at least one set of support values.
        #[qproperty(bool, has_support)]
        /// The tree is drawn for another document, such as a species
        /// workspace: drawing edits go out as `drawingEdited` for that
        /// document to keep, and other edits are refused.
        #[qproperty(bool, display_only)]
        type TreeController = super::TreeRust;

        /// Show the tree in `path` with `edits`, a JSON list of drawing
        /// edits, replayed on it. Nothing is written.
        #[qinvokable]
        fn find_query(self: Pin<&mut TreeController>, query: &QString, reverse: bool);
        #[qinvokable]
        fn find_poll(self: Pin<&mut TreeController>, direction: i32) -> QString;

        #[qinvokable]
        fn show_tree(self: Pin<&mut TreeController>, path: &QString, edits: &QString) -> bool;
        #[qsignal]
        #[cxx_name = "drawingEdited"]
        fn drawing_edited(self: Pin<&mut TreeController>, edit: QString);
        /// Bar columns to draw after any clade annotation bars, as JSON.
        #[qinvokable]
        fn set_columns(self: Pin<&mut TreeController>, json: &QString);
        /// The owned bar under the pointer as JSON (`column`, `id`, `label`),
        /// or an empty string.
        #[qinvokable]
        fn bar_at(self: &TreeController, x: f64, y: f64) -> QString;

        #[qinvokable]
        fn document_record(self: &TreeController) -> QString;
        #[qinvokable]
        fn record_revision(self: &TreeController) -> QString;
        #[qinvokable]
        fn accessible_page(self: &TreeController, page: i32) -> QString;
        #[qinvokable]
        fn create_delimitation(self: Pin<&mut TreeController>, method: &QString) -> bool;
        #[qsignal]
        #[cxx_name = "workspaceCreated"]
        fn workspace_created(self: Pin<&mut TreeController>, path: QString);
        #[qinvokable]
        fn open_file(self: Pin<&mut TreeController>, path: &QString) -> bool;
        #[qsignal]
        fn opened(self: Pin<&mut TreeController>, success: bool);
        #[qinvokable]
        fn highlight_taxa(self: Pin<&mut TreeController>, names: &QString);
        #[qsignal]
        #[cxx_name = "taxaSelected"]
        fn taxa_selected(self: Pin<&mut TreeController>, names: QString);
        #[qinvokable]
        fn link_alignment(self: Pin<&mut TreeController>, path: &QString);
        #[qinvokable]
        fn extract_clade_alignment(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn select_clade(self: Pin<&mut TreeController>, names: &QString);
        #[qsignal]
        #[cxx_name = "fileCreated"]
        fn file_created(self: Pin<&mut TreeController>, path: QString);
        #[qinvokable]
        fn resize(self: Pin<&mut TreeController>, width: f64, height: f64);
        /// Screen pixels per QML pixel, so lines draw on whole pixels.
        #[qinvokable]
        fn set_pixel_ratio(self: Pin<&mut TreeController>, ratio: f64);
        #[qinvokable]
        fn wheel(self: Pin<&mut TreeController>, x: f64, y: f64, dy: f64, dx: f64, modifiers: i32);
        /// Select what's under the pointer.
        #[qinvokable]
        fn press(self: Pin<&mut TreeController>, x: f64, y: f64);
        #[qinvokable]
        fn press_modified(self: Pin<&mut TreeController>, x: f64, y: f64, modifiers: i32);
        /// Select what's under the pointer and describe it for the context
        /// menu, as JSON.
        #[qinvokable]
        fn context_press(self: Pin<&mut TreeController>, x: f64, y: f64) -> QString;
        /// What's under the pointer, or an empty string.
        #[qinvokable]
        fn hover(self: &TreeController, x: f64, y: f64) -> QString;
        /// Returns whether the key was handled.
        #[qinvokable]
        fn key(self: Pin<&mut TreeController>, key: i32, modifiers: i32) -> bool;
        #[qinvokable]
        fn zoom_in(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn set_zoom_percent(self: Pin<&mut TreeController>, percent: f64);
        #[qinvokable]
        fn zoom_out(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn fit(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn hscroll_to(self: Pin<&mut TreeController>, fraction: f64);
        #[qinvokable]
        fn vscroll_to(self: Pin<&mut TreeController>, fraction: f64);
        #[qinvokable]
        fn undo(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn redo(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn save(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn check_save(self: &TreeController, path: &QString) -> QString;
        #[qinvokable]
        fn save_confirmed(
            self: Pin<&mut TreeController>,
            path: &QString,
            token: &QString,
            policy: &QString,
            acknowledge: bool,
        ) -> bool;
        /// Save to a new file and carry on editing that one.
        #[qinvokable]
        fn save_as(self: Pin<&mut TreeController>, path: &QString) -> bool;
        /// Reload the Omarchy theme if it changed.
        #[qinvokable]
        fn check_theme(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn memory_report(self: &TreeController) -> QString;

        // Edits.
        /// Root on the branch above the selected clade.
        #[qinvokable]
        fn reroot(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn midpoint_root(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn minimum_variance_root(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn rotate(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn ladderize(self: Pin<&mut TreeController>, small_first: bool);
        /// The root branch for the root position dialog, as JSON: its two
        /// sides, its length and the first side's share, or null.
        #[qinvokable]
        fn root_json(self: &TreeController) -> QString;
        /// Put the root at `fraction` of its branch, measured from the side
        /// `root_json` named first.
        #[qinvokable]
        fn move_root(self: Pin<&mut TreeController>, fraction: f64);
        /// The selected tip's name, or empty.
        #[qinvokable]
        fn tip_name(self: &TreeController) -> QString;
        /// Rename the selected tip. Returns an error message, or empty.
        #[qinvokable]
        fn rename_tip(self: Pin<&mut TreeController>, name: &QString) -> QString;
        /// The selected clade's annotation as JSON (a new one if it has
        /// none), with its `id` and whether it `exists`.
        #[qinvokable]
        fn annotation_json(self: &TreeController) -> QString;
        /// Add or change an annotation from the editor's JSON. Returns an
        /// error message, or empty.
        #[qinvokable]
        fn set_annotation(self: Pin<&mut TreeController>, json: &QString) -> QString;
        #[qinvokable]
        fn remove_annotation(self: Pin<&mut TreeController>, id: i32);
        /// Support sets and style as JSON, for the support settings.
        #[qinvokable]
        fn style_json(self: &TreeController) -> QString;
        #[qinvokable]
        fn compose_metadata_labels(
            self: Pin<&mut TreeController>,
            project: &QString,
            fields: &QString,
        ) -> QString;
        /// Replace the style from JSON. Returns an error message, or empty.
        #[qinvokable]
        fn set_style(self: Pin<&mut TreeController>, json: &QString) -> QString;
        #[qinvokable]
        fn set_style_with_removals(
            self: Pin<&mut TreeController>,
            json: &QString,
            removed: &QString,
        ) -> QString;
        #[qinvokable]
        fn place_support(self: Pin<&mut TreeController>, placement: &QString);
        /// Set tip and support font sizes and branch width in pixels, as one
        /// edit; a value of 0 or less leaves that size unchanged.
        #[qinvokable]
        fn set_sizes(
            self: Pin<&mut TreeController>,
            tip_font: i32,
            support_font: i32,
            branch_width: i32,
        );
        /// Add the support values of the tree in `path`, matched branch by
        /// branch, named `name`.
        #[qinvokable]
        fn support_import_json(self: &TreeController, path: &QString) -> QString;
        #[qinvokable]
        fn add_support_from(self: Pin<&mut TreeController>, path: &QString, name: &QString);
        #[qinvokable]
        fn remove_support(self: Pin<&mut TreeController>, name: &QString);
        /// Write the tree as an SVG figure.
        #[qinvokable]
        fn export_svg(self: Pin<&mut TreeController>, path: &QString);
        #[qinvokable]
        fn export_figure(
            self: Pin<&mut TreeController>,
            path: &QString,
            scope: i32,
            publication: bool,
        );
        #[qinvokable]
        fn export_figure_png(
            self: Pin<&mut TreeController>,
            path: &QString,
            scope: i32,
            publication: bool,
            width: f64,
        );
    }
}

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Instant;

use basecraft_docs::TreeDoc;
use basecraft_ops::{DocId, Session};
use basecraft_protocol::delimitation::TreeDrawingEdit;
use basecraft_protocol::{CladeAnnotation, Op, SupportPlacement, TreeStyle};
use basecraft_viewmodel::tree::TipColumn;
use basecraft_viewmodel::tree::to_svg_with_style;
use basecraft_viewmodel::{Frame, Theme, TreeView};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QString};
use serde_json::{Value, json};

use crate::bridge::{
    ALT, CTRL, KEY_0, KEY_DOWN, KEY_EQUAL, KEY_ESCAPE, KEY_LEFT, KEY_MINUS, KEY_PLUS, KEY_RIGHT,
    KEY_UP, KEY_Y, KEY_Z, SHIFT, WHEEL_ZOOM, ZOOM_STEP, css, file_folder, file_name, load_theme,
    memory_text, monospace_font, theme_stamp, took,
};

#[allow(clippy::struct_excessive_bools)]
pub struct TreeRust {
    find: basecraft_viewmodel::find::Session,
    find_status_revision: u64,
    overview_content: QString,
    overview_matches: QString,
    overview_match_cache: basecraft_viewmodel::overview::Cache,
    overview_cache: basecraft_viewmodel::overview::Cache,
    frame: QByteArray,
    canvas_geometry: QString,
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
    busy: bool,
    zoom_text: QString,
    hscroll_pos: f64,
    hscroll_size: f64,
    vscroll_pos: f64,
    vscroll_size: f64,
    has_selection: bool,
    can_auto_root: bool,
    tip_selected: bool,
    linked_alignment: QString,
    source_alignment: QString,
    alignment_link_status: QString,
    alignment_link_inferred: bool,
    support_placement: QString,
    tip_font: i32,
    support_font: i32,
    branch_width: i32,
    has_support: bool,
    display_only: bool,

    session: Option<Session>,
    save_state: crate::editor_save::SaveState,
    doc: Option<DocId>,
    view: TreeView,
    theme: Theme,
    theme_stamp: crate::bridge::ThemeStamp,
    scratch: Frame,
}

impl Default for TreeRust {
    fn default() -> Self {
        let mut c = Self {
            find: basecraft_viewmodel::find::Session::default(),
            find_status_revision: u64::MAX,
            overview_content: QString::from("{\"shapes\":[]}"),
            overview_matches: QString::from("[]"),
            overview_match_cache: basecraft_viewmodel::overview::Cache::default(),
            overview_cache: basecraft_viewmodel::overview::Cache::default(),
            frame: QByteArray::default(),
            canvas_geometry: QString::from("{}"),
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
            busy: false,
            zoom_text: QString::default(),
            hscroll_pos: 0.0,
            hscroll_size: 1.0,
            vscroll_pos: 0.0,
            vscroll_size: 1.0,
            has_selection: false,
            can_auto_root: false,
            tip_selected: false,
            linked_alignment: QString::default(),
            source_alignment: QString::default(),
            alignment_link_status: QString::default(),
            alignment_link_inferred: false,
            support_placement: QString::from("natural"),
            tip_font: 0,
            support_font: 0,
            branch_width: 0,
            has_support: false,
            display_only: false,
            session: None,
            save_state: crate::editor_save::SaveState::default(),
            doc: None,
            view: TreeView::new(800.0, 600.0),
            theme: Theme::default(),
            theme_stamp: crate::bridge::ThemeStamp::default(),
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

fn placement_name(p: SupportPlacement) -> &'static str {
    match p {
        SupportPlacement::Natural => "natural",
        SupportPlacement::Above => "above",
        SupportPlacement::Below => "below",
    }
}

// Viewer annotations use the existing clade identity. Refuse targets that this
// identity cannot represent exactly (duplicate tips or a unary ancestor).
fn annotation_clade(doc: &TreeDoc, node: usize) -> Result<Vec<String>, String> {
    if node >= doc.nodes().len() {
        return Err("The annotation target no longer exists".into());
    }
    let clade = doc.clade_ref(node);
    check_annotation_clade(doc, node, &clade)?;
    Ok(clade)
}

fn check_annotation_clade(doc: &TreeDoc, node: usize, clade: &[String]) -> Result<(), String> {
    for name in clade {
        if doc
            .nodes()
            .iter()
            .filter(|n| n.is_tip() && n.name == *name)
            .count()
            != 1
        {
            return Err("Cannot name this clade: its tip identifiers are not unique".into());
        }
    }
    if doc.resolve(clade).ok() != Some(node) {
        return Err(
            "Cannot name this node separately from its unary child using a clade annotation".into(),
        );
    }
    Ok(())
}

fn annotation_request(
    doc: &TreeDoc,
    view: &TreeView,
    revision: &str,
    colour: &str,
) -> Result<Value, String> {
    let node = view.selected.ok_or("Select a branch, node or tip first")?;
    let clade = annotation_clade(doc, node)?;
    let original = view.selected_annotation(doc);
    let (id, annotation) = original.clone().unwrap_or_else(|| {
        let mut annotation = CladeAnnotation::new(clade, doc.node(node).name.clone());
        annotation.colour = colour.into();
        (doc.next_annotation_id(), annotation)
    });
    check_annotation_clade(doc, node, &annotation.clade)?;
    let mut value = serde_json::to_value(annotation).map_err(|e| e.to_string())?;
    value["id"] = json!(id);
    value["exists"] = json!(original.is_some());
    value["tips"] = json!(doc.tip_counts()[node]);
    value["target"] = json!({
        "revision": revision,
        "node": node,
        "original": original.map(|(_, a)| a),
    });
    Ok(value)
}

fn checked_annotation(
    doc: &TreeDoc,
    revision: &str,
    value: &Value,
) -> Result<(u32, CladeAnnotation), String> {
    let target = &value["target"];
    if revision.is_empty() || target["revision"].as_str() != Some(revision) {
        return Err("The tree changed or closed. Open the annotation again".into());
    }
    let node = target["node"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("Missing annotation target")?;
    annotation_clade(doc, node)?;
    let id = value["id"]
        .as_u64()
        .and_then(|i| u32::try_from(i).ok())
        .ok_or("Missing annotation identifier")?;
    let original: Option<CladeAnnotation> = serde_json::from_value(
        target
            .get("original")
            .ok_or("Missing original annotation")?
            .clone(),
    )
    .map_err(|e| format!("Bad original annotation: {e}"))?;
    if doc.annotations().get(&id) != original.as_ref()
        || (original.is_none() && id != doc.next_annotation_id())
    {
        return Err("The annotation changed. Open it again".into());
    }
    let annotation: CladeAnnotation =
        serde_json::from_value(value.clone()).map_err(|e| format!("Bad annotation: {e}"))?;
    check_annotation_clade(doc, node, &annotation.clade)?;
    if original
        .as_ref()
        .is_some_and(|a| a.clade != annotation.clade)
    {
        return Err("The annotation target changed. Open it again".into());
    }
    Ok((id, annotation))
}

impl qobject::TreeController {
    pub fn document_record(&self) -> QString {
        crate::provenance_bridge::session_report(self.rust().session.as_ref(), self.rust().doc)
    }
    pub fn record_revision(&self) -> QString {
        crate::provenance_bridge::session_revision(self.rust().session.as_ref(), self.rust().doc)
    }

    pub fn accessible_page(&self, page: i32) -> QString {
        let result = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| basecraft_viewmodel::accessible::tree(d, page))
        );
        let value = result.map_or_else(
            || serde_json::json!({"page": 0, "pages": 1, "text": "No document loaded"}),
            |p| serde_json::json!({"page": p.page, "pages": p.pages, "text": p.text}),
        );
        QString::from(&value.to_string())
    }

    fn with_doc<T>(&self, f: impl FnOnce(&TreeDoc) -> T) -> basecraft_ops::Result<Option<T>> {
        let r = self.rust();
        match (&r.session, r.doc) {
            (Some(s), Some(d)) => Ok(Some(f(s.tree(d)?))),
            _ => Ok(None),
        }
    }

    fn check_document(&self) -> basecraft_ops::Result<()> {
        self.with_doc(|_| ()).map(|_| ())
    }

    /// Rebuild the frame, status line and scroll bars.
    fn refresh(mut self: Pin<&mut Self>) {
        if let Some(warning) =
            crate::document_open::recovery_notice(self.rust().session.as_ref(), self.rust().doc)
        {
            self.as_mut().set_message_kind(QString::from("error"));
            self.as_mut().set_message(warning);
        }
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let (bytes, status, zoom, h, v, sel, tip, placement, sizes) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.tree(doc));
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
                r.view.selected.is_some(),
                r.view.selected.is_some_and(|s| d.node(s).is_tip()),
                placement_name(d.style().support_placement),
                (
                    i32::try_from(d.style().tip_font).unwrap_or(i32::MAX),
                    i32::try_from(d.style().support_font).unwrap_or(i32::MAX),
                    i32::try_from(d.style().branch_width).unwrap_or(i32::MAX),
                    !d.supports().is_empty(),
                ),
            )
        };
        let geometry = {
            let r = self.rust();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.tree(doc));
            serde_json::json!({"rows": r.view.tip_rows(d).iter().map(|(name,y,height)| serde_json::json!({"name":name,"y":y,"height":height})).collect::<Vec<_>>(),"root":r.view.node_position(d.root()),"tips_x":d.tips_in_order().iter().filter_map(|&i| r.view.node_position(i).map(|(x,_)|x)).fold(f32::NEG_INFINITY,f32::max),
                "columns": r.view.column_positions().iter().map(|(id, x, width)| serde_json::json!({"id": id, "x": x, "width": width})).collect::<Vec<_>>()})
        };
        self.as_mut()
            .set_canvas_geometry(QString::from(&geometry.to_string()));
        let (overview, matches) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.tree(doc));
            let key = format!(
                "{}:{}:{:?}:{:?}",
                session.id(),
                d.version(),
                r.view.overview_key(),
                r.theme
            );
            let content = r
                .overview_cache
                .get(key.clone(), || r.view.overview(d, &r.theme));
            let matches =
                r.overview_match_cache
                    .get_json(format!("{key}:{}", r.find.revision()), || {
                        crate::find_bridge::overview_marks(&r.find, |hit| {
                            r.view.overview_match(hit.row)
                        })
                    });
            (content, matches)
        };
        self.as_mut().set_overview_content(QString::from(&overview));
        self.as_mut().set_overview_matches(QString::from(&matches));
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
        self.as_mut().set_status(QString::from(&status));
        self.as_mut().set_zoom_text(QString::from(&zoom));
        self.as_mut().set_hscroll_size(f64::from(h.1));
        self.as_mut().set_hscroll_pos(f64::from(h.0));
        self.as_mut().set_vscroll_size(f64::from(v.1));
        self.as_mut().set_vscroll_pos(f64::from(v.0));
        let can_auto_root = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(TreeView::can_auto_root)
        )
        .unwrap_or(false);
        self.as_mut().set_can_auto_root(can_auto_root);
        self.as_mut().set_has_selection(sel);
        self.as_mut().set_tip_selected(tip);
        self.as_mut()
            .set_support_placement(QString::from(placement));
        self.as_mut().set_tip_font(sizes.0);
        self.as_mut().set_support_font(sizes.1);
        self.as_mut().set_branch_width(sizes.2);
        self.as_mut().set_has_support(sizes.3);
        self.emit_taxa();
    }

    /// A neutral hint. Messages stay brief: say what happened.
    fn notify(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().set_message_kind(QString::default());
        self.as_mut().set_message(QString::from(msg));
    }

    fn done(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().set_message_kind(QString::from("ok"));
        self.as_mut().set_message(QString::from(msg));
    }

    fn fail(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().set_message_kind(QString::from("error"));
        self.as_mut().set_message(QString::from(msg));
    }

    /// Apply an edit through the session; on failure, say why and return
    /// the message.
    fn edit(mut self: Pin<&mut Self>, op: Op) -> Result<(), String> {
        if let Err(error) = self.check_document() {
            let message = error.to_string();
            self.as_mut().fail(&message);
            return Err(message);
        }
        if self.rust().display_only {
            let Some(edit) = TreeDrawingEdit::from_op(&op) else {
                let msg =
                    "This tree is drawn for another document. Open its file to edit it".to_string();
                self.as_mut().fail(&msg);
                return Err(msg);
            };
            // Sent after this call returns: the owner replays it with
            // `show_tree`, which must not run inside this edit.
            let text = serde_json::to_string(&edit).map_err(|e| e.to_string())?;
            let _ = self
                .qt_thread()
                .queue(move |this| this.drawing_edited(QString::from(&text)));
            return Ok(());
        }
        let clears_selection =
            matches!(op, Op::TreeMidpointRoot {} | Op::TreeMinimumVarianceRoot {});
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return Err("No tree open".into());
            };
            let result = session.apply(doc, op);
            r.view.sync(session.tree(doc).map_err(|e| e.to_string())?);
            result
        };
        match result {
            Ok(_) => {
                if clears_selection {
                    let mut state = self.as_mut().rust_mut();
                    state.view.selected = None;
                    state.view.multi_taxa = None;
                }
                self.as_mut().set_dirty(true);
                self.as_mut().notify("");
                self.refresh();
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                let msg = msg
                    .strip_prefix("invalid operation: ")
                    .unwrap_or(&msg)
                    .to_string();
                self.as_mut().fail(&msg);
                Err(msg)
            }
        }
    }

    fn refresh_link(mut self: Pin<&mut Self>) {
        let path = PathBuf::from(self.rust().file_path.to_string());
        let origin = basecraft_ops::records::AlignmentLink::read(&path)
            .ok()
            .filter(|l| l.inferred);
        let linked = basecraft_ops::records::AlignmentLink::read_view(&path);
        let (source, status) = match linked {
            Ok(link) => {
                let status = crate::bridge::checked_document!(
                    self.qt_thread(),
                    self.with_doc(|tree| link.status(&path, tree))
                )
                .unwrap_or_default();
                (link.source.to_string_lossy().into_owned(), status)
            }
            Err(e)
                if basecraft_ops::records::metadata_present(&basecraft_ops::records::sidecar(
                    &path,
                ))
                .unwrap_or(true)
                    || basecraft_ops::records::metadata_present(
                        &basecraft_ops::records::view_sidecar(&path),
                    )
                    .unwrap_or(true) =>
            {
                (String::new(), format!("Alignment link unavailable: {e}"))
            }
            Err(_) => (String::new(), "No linked alignment".into()),
        };
        let source_path = origin
            .as_ref()
            .map(|l| l.source.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.as_mut().set_linked_alignment(QString::from(&source));
        self.as_mut()
            .set_source_alignment(QString::from(&source_path));
        self.as_mut()
            .set_alignment_link_status(QString::from(&status));
        self.as_mut().set_alignment_link_inferred(origin.is_some());
    }

    pub fn link_alignment(mut self: Pin<&mut Self>, path: &QString) {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let result = (|| -> basecraft_ops::Result<()> {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let tree_path = PathBuf::from(r.file_path.to_string());
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return Ok(());
            };
            let source = PathBuf::from(path.to_string());
            let outputs = basecraft_ops::records::AlignmentLink::associate(
                &tree_path,
                session.tree(doc)?,
                &source,
            )?;
            session.log_tool_run(
                doc,
                basecraft_protocol::Actor::Human {
                    user: basecraft_platform::username(),
                },
                ("basecraft.links", "associate"),
                json!({"alignment":source,"inferred":false}),
                &[(
                    source.to_string_lossy().into_owned(),
                    basecraft_io::hash_file(&source)?,
                )],
                &outputs,
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.as_mut().refresh_link();
                self.done("Linked alignment; matching names do not establish inference provenance");
            }
            Err(e) => self.fail(&format!("Cannot link alignment: {e}")),
        }
    }

    pub fn select_clade(mut self: Pin<&mut Self>, names: &QString) {
        let result = (|| -> Result<usize, Box<dyn std::error::Error>> {
            let names: Vec<String> = serde_json::from_str(&names.to_string())?;
            self.with_doc(|tree| tree.resolve(&names))?
                .ok_or("No tree open")?
                .map_err(Into::into)
        })();
        match result {
            Ok(node) => {
                self.as_mut().rust_mut().view.multi_taxa = None;
                self.as_mut().rust_mut().view.selected = Some(node);
                self.refresh();
            }
            Err(e) => self.fail(&e.to_string()),
        }
    }

    pub fn extract_clade_alignment(mut self: Pin<&mut Self>) {
        use basecraft_plugin_host::{
            jobs::JobManager,
            policy::SessionPolicy,
            registry::{Registry, RegistryConfig},
        };
        use basecraft_protocol::operation::{InputReference, OperationRequest, RuntimeState};
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        if self.rust().busy {
            return;
        }
        let Some(doc) = self.rust().doc else {
            return;
        };
        let taxa = self
            .rust()
            .session
            .as_ref()
            .map(|session| {
                self.rust()
                    .view
                    .selected_names(crate::bridge::checked_document!(
                        access_thread,
                        session.tree(doc)
                    ))
            })
            .unwrap_or_default();
        if taxa.is_empty() {
            self.fail("Select one or more clades first");
            return;
        }
        let Some(session) = self.as_mut().rust_mut().session.take() else {
            return;
        };
        let tree_path = PathBuf::from(self.rust().file_path.to_string());
        self.as_mut().set_busy(true);
        self.as_mut().notify("Extracting selected clades");
        let qt_thread = self.qt_thread();
        let spawn = crate::document_jobs::background_session(
            session,
            move |session| {
                let tree = session.tree(doc).map_err(|e| e.to_string())?;
                let link = basecraft_ops::records::AlignmentLink::read_view(&tree_path)
                    .map_err(|e| format!("Link an alignment first: {e}"))?;
                let input = link
                    .checked_snapshot(&tree_path, tree)
                    .map_err(|e| e.to_string())?;
                let output =
                    basecraft_plugin_host::file_tool::output_beside(&tree_path, "clade", "afa");
                let registry = RegistryConfig::discover()
                    .and_then(|c| Registry::load(&c))
                    .map_err(|e| e.to_string())?;
                let policy = SessionPolicy::new(session.root(), true).map_err(|e| e.to_string())?;
                let destination = output
                    .strip_prefix(&policy.root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned();
                let request = OperationRequest {
                    operation: "basecraft.clade-alignment/extract@1".into(),
                    inputs: [
                        (
                            "alignment".into(),
                            vec![InputReference::File {
                                path: input.to_string_lossy().into_owned(),
                            }],
                        ),
                        (
                            "selection_tree".into(),
                            vec![InputReference::Document {
                                doc: doc.0.to_string(),
                                generation: session.generation(doc).map_err(|e| e.to_string())?,
                                revision: session.revision(doc).map_err(|e| e.to_string())?,
                            }],
                        ),
                    ]
                    .into(),
                    parameters: json!({"taxa":taxa}),
                    destinations: [("artifacts/clade.afa".into(), destination)].into(),
                    pins: None,
                    request_key: None,
                    target: None,
                };
                let mut jobs = JobManager::default().with_task_cancellation();
                let accepted = jobs
                    .submit_in_session(&registry, request, policy, "GUI", session)
                    .map_err(|e| e.to_string())?;
                let outcome = jobs
                    .wait_in_session(&accepted.run_id, session, &|| false)
                    .map_err(|e| e.to_string())?;
                if outcome.state != RuntimeState::Completed {
                    return Err(format!(
                        "run ended {:?}: {:?}",
                        outcome.state, outcome.errors
                    ));
                }
                Ok((output, link.source_changed()))
            },
            move |session, result| {
                let _ = qt_thread.queue(move |mut this| {
                this.as_mut().rust_mut().session = Some(session);
                this.as_mut().set_busy(false);
                match result {
                    Ok((output, source_changed)) => {
                        this.as_mut().done(if source_changed { "Opened clades from the retained alignment snapshot; the source file has changed" } else { "Opened alignment for the selected clades" });
                        this.file_created(QString::from(output.to_string_lossy().as_ref()));
                    }
                    Err(error) => this.fail(&format!("Cannot extract clades: {error}")),
                }
            });
            },
        );
        if let Err((session, error)) = spawn {
            self.as_mut().rust_mut().session = Some(*session);
            self.as_mut().set_busy(false);
            self.fail(&error);
        }
    }

    #[cfg(test)]
    #[allow(dead_code)] // Read by the real-controller bootstrap journey.
    pub(crate) fn linked_taxa_snapshot(&self) -> Vec<String> {
        let mut taxa: Vec<_> = self.rust().view.linked_taxa.iter().cloned().collect();
        taxa.sort();
        taxa
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
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let taxa = {
            let r = self.rust();
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let tree = crate::bridge::checked_document!(access_thread, session.tree(doc));
            let names = r.view.selected_names(tree);
            serde_json::to_string(&names).unwrap_or_default()
        };
        if taxa != self.rust().last_taxa {
            self.as_mut().rust_mut().last_taxa.clone_from(&taxa);
            self.taxa_selected(QString::from(&taxa));
        }
    }

    pub fn create_delimitation(mut self: Pin<&mut Self>, method: &QString) -> bool {
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
        let spawn = crate::document_jobs::background_session(
            session,
            move |session| {
                crate::delimitation_bridge::create_workspace_in_session(session, doc, &method)
            },
            move |session, result| {
                let _ = qt_thread.queue(move |mut this| {
                    this.as_mut().rust_mut().session = Some(session);
                    this.as_mut().set_busy(false);
                    match result {
                        Ok(path) => {
                            this.as_mut().done("Species workspace created");
                            this.workspace_created(QString::from(path));
                        }
                        Err(error) => {
                            this.as_mut().fail(&error);
                            this.workspace_created(QString::default());
                        }
                    }
                });
            },
        );
        if let Err((session, error)) = spawn {
            self.as_mut().rust_mut().session = Some(*session);
            self.as_mut().set_busy(false);
            self.fail(&error);
            return false;
        }
        true
    }
    pub fn open_file(mut self: Pin<&mut Self>, path: &QString) -> bool {
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

    pub(crate) fn install_fresh(
        self: Pin<&mut Self>,
        candidate: crate::document_open::Candidate,
    ) -> Result<(), crate::document_open::OpenError> {
        if self.rust().session.is_some() || self.rust().doc.is_some() || self.rust().busy {
            return Err(crate::document_open::OpenError::StaleIdentity);
        }
        self.install_opened(candidate)
    }

    #[cfg(test)]
    pub(crate) fn open_identity(&self) -> Option<(String, DocId, u64, u64)> {
        let session = self.rust().session.as_ref()?;
        let doc = self.rust().doc?;
        if !session.is_open(doc) {
            return None;
        }
        Some((
            session.id().into(),
            doc,
            session.generation(doc).unwrap(),
            session.revision(doc).unwrap(),
        ))
    }

    pub(crate) fn install_opened(
        mut self: Pin<&mut Self>,
        candidate: crate::document_open::Candidate,
    ) -> Result<(), crate::document_open::OpenError> {
        candidate.check(basecraft_ops::files::Kind::Tree)?;
        let crate::document_open::Candidate {
            session,
            doc,
            path,
            root,
            ..
        } = candidate;
        let _ = root;
        let warning = session
            .tree_metadata_warning(doc)?
            .unwrap_or_default()
            .to_owned();
        let tree = session.tree(doc)?;
        {
            let mut r = self.as_mut().rust_mut();
            r.view.selected = None;
            r.view.multi_taxa = None;
            r.view.forget_layout();
            r.view.fit();
            r.view.sync(tree);
            r.session = Some(session);
            r.doc = Some(doc);
        }
        self.as_mut()
            .set_file_path(QString::from(path.to_string_lossy().as_ref()));
        self.as_mut().refresh_link();
        self.as_mut().set_dirty(false);
        self.as_mut().notify(&warning);
        self.refresh();
        Ok(())
    }

    pub fn resize(mut self: Pin<&mut Self>, width: f64, height: f64) {
        self.as_mut().rust_mut().view.resize(f(width), f(height));
        self.refresh();
    }

    pub fn set_pixel_ratio(mut self: Pin<&mut Self>, ratio: f64) {
        self.as_mut().rust_mut().view.set_pixel_ratio(f(ratio));
        self.refresh();
    }

    pub fn wheel(mut self: Pin<&mut Self>, x: f64, y: f64, dy: f64, dx: f64, modifiers: i32) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.tree(doc));
            if modifiers & (CTRL | ALT) != 0 {
                // Ctrl zooms both ways about the pointer, Ctrl+Shift only the
                // width, Alt only the rows. Qt may turn Alt+wheel into a
                // sideways wheel, so take whichever axis moved.
                let delta = if dy == 0.0 { dx } else { dy };
                let factor = f(WHEEL_ZOOM.powf(delta));
                let (px, py) = (f(x), f(y));
                if modifiers & CTRL == 0 {
                    r.view.zoom_at(d, px, py, factor, true);
                } else if modifiers & SHIFT != 0 {
                    r.view.zoom_at(d, px, py, factor, false);
                } else {
                    r.view.zoom_at(d, px, py, factor, true);
                    r.view.zoom_at(d, px, py, factor, false);
                }
            } else if modifiers & SHIFT != 0 || dx.abs() > dy.abs() {
                let d = if dx.abs() > dy.abs() { dx } else { dy };
                r.view.scroll_by(f(-d / 2.0), 0.0);
            } else {
                r.view.scroll_by(0.0, f(-dy / 2.0));
            }
        }
        self.refresh();
    }

    pub fn press_modified(mut self: Pin<&mut Self>, x: f64, y: f64, modifiers: i32) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        if modifiers & CTRL == 0 {
            self.press(x, y);
            return;
        }
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let (Some(session), Some(doc)) = (&r.session, r.doc) {
                let tree = crate::bridge::checked_document!(access_thread, session.tree(doc));
                if let Some(node) = r.view.node_at(tree, f(x), f(y)) {
                    r.view.toggle_clade(tree, node);
                }
            }
        }
        self.refresh();
    }

    pub fn press(mut self: Pin<&mut Self>, x: f64, y: f64) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let (Some(session), Some(doc)) = (&r.session, r.doc) {
                r.view.select_at(
                    crate::bridge::checked_document!(access_thread, session.tree(doc)),
                    f(x),
                    f(y),
                );
            }
        }
        self.refresh();
    }

    pub fn context_press(mut self: Pin<&mut Self>, x: f64, y: f64) -> QString {
        self.as_mut().press(x, y);
        let info = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| {
                let v = &self.rust().view;
                let sel = v.selected;
                json!({
                    "node": sel.is_some(),
                    "tip": sel.is_some_and(|s| d.node(s).is_tip()),
                    "root": sel == Some(d.root()),
                    // On one of the two branches either side of the root.
                    "rootBranch": d.root_branch().is_some()
                        && sel.is_some_and(|s| d.node(s).parent == Some(d.root())),
                    "annotated": v.selected_annotation(d).is_some(),
                    "annotation": v.selected_annotation(d).map_or(0, |(id, _)| id),
                })
            })
        )
        .unwrap_or(Value::Null);
        QString::from(&info.to_string())
    }

    pub fn show_tree(mut self: Pin<&mut Self>, path: &QString, edits: &QString) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        if self.rust().busy {
            return false;
        }
        let edits: Vec<TreeDrawingEdit> = match serde_json::from_str(&edits.to_string()) {
            Ok(edits) => edits,
            Err(e) => {
                self.fail(&format!("Bad drawing edits: {e}"));
                return false;
            }
        };
        let path = PathBuf::from(path.to_string());
        // Replaying on a display tree leaves no records.
        let mut session = Session::unlogged(file_folder(&path));
        let doc = match session.open(&path) {
            Ok(doc) if session.is_tree(doc) => doc,
            Ok(_) => {
                self.fail(&format!("{} is not a tree", path.display()));
                return false;
            }
            Err(e) => {
                self.fail(&format!("Can't open {}: {e}", path.display()));
                return false;
            }
        };
        for edit in &edits {
            let _ = session.apply(doc, edit.to_op());
        }
        let abs = std::fs::canonicalize(&path).unwrap_or(path);
        let abs = QString::from(&abs.to_string_lossy().into_owned());
        // Redrawing the same tree keeps the zoom, scroll and selection:
        // drawing edits do not renumber nodes.
        let same = self.rust().file_path == abs;
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if !same {
                r.view.selected = None;
                r.view.multi_taxa = None;
                r.view.fit();
            }
            r.view.forget_layout();
            r.view.sync(crate::bridge::checked_document!(
                access_thread,
                session.tree(doc)
            ));
            r.session = Some(session);
            r.doc = Some(doc);
        }
        self.as_mut().set_file_path(abs);
        self.as_mut().set_dirty(false);
        self.as_mut().notify("");
        self.refresh();
        true
    }

    pub fn set_columns(mut self: Pin<&mut Self>, json: &QString) {
        match serde_json::from_str::<Vec<TipColumn>>(&json.to_string()) {
            Ok(columns) => {
                self.as_mut().rust_mut().view.set_columns(columns);
                self.refresh();
            }
            Err(e) => self.fail(&format!("Bad bar columns: {e}")),
        }
    }

    pub fn bar_at(&self, x: f64, y: f64) -> QString {
        let bar = self
            .rust()
            .view
            .bar_at(f(x), f(y))
            .filter(|b| !b.column.is_empty());
        QString::from(
            &bar.map(|b| json!({"column": b.column, "id": b.id, "label": b.label}).to_string())
                .unwrap_or_default(),
        )
    }

    pub fn hover(&self, x: f64, y: f64) -> QString {
        let text = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| self.rust().view.hover_text(d, f(x), f(y)))
        )
        .flatten()
        .unwrap_or_default();
        QString::from(&text)
    }

    pub fn key(mut self: Pin<&mut Self>, key: i32, modifiers: i32) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let ctrl = modifiers & CTRL != 0;
        let shift = modifiers & SHIFT != 0;
        // Undo belongs to the document a display tree is drawn for.
        if self.rust().display_only && ctrl && matches!(key, KEY_Z | KEY_Y) {
            return false;
        }
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
            // Ctrl zooms both ways; Alt only spreads the rows.
            KEY_PLUS | KEY_EQUAL | KEY_MINUS if ctrl || modifiers & ALT != 0 => {
                let factor = if key == KEY_MINUS {
                    1.0 / ZOOM_STEP
                } else {
                    ZOOM_STEP
                };
                if ctrl {
                    self.as_mut().zoom_both(f(factor));
                } else {
                    self.as_mut().zoom_centre(f(factor));
                }
                true
            }
            KEY_ESCAPE => {
                self.as_mut().rust_mut().view.selected = None;
                true
            }
            KEY_LEFT | KEY_RIGHT | KEY_UP | KEY_DOWN => {
                let mut r = self.as_mut().rust_mut();
                let r = &mut *r;
                if let (Some(session), Some(doc)) = (&r.session, r.doc) {
                    let d = crate::bridge::checked_document!(access_thread, session.tree(doc));
                    match key {
                        KEY_LEFT => r.view.select_parent(d),
                        KEY_RIGHT => r.view.select_child(d),
                        KEY_UP => r.view.select_sibling(d, -1),
                        _ => r.view.select_sibling(d, 1),
                    }
                }
                true
            }
            _ => false,
        };
        if handled {
            self.refresh();
        }
        handled
    }

    fn zoom_centre(mut self: Pin<&mut Self>, factor: f32) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        if let (Some(session), Some(doc)) = (&r.session, r.doc) {
            let (w, h) = (r.view_size().0, r.view_size().1);
            r.view.zoom_at(
                crate::bridge::checked_document!(access_thread, session.tree(doc)),
                w / 2.0,
                h / 2.0,
                factor,
                true,
            );
        }
    }

    fn zoom_both(mut self: Pin<&mut Self>, factor: f32) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        if let (Some(session), Some(doc)) = (&r.session, r.doc) {
            r.view.zoom_both(
                crate::bridge::checked_document!(access_thread, session.tree(doc)),
                factor,
            );
        }
    }

    pub fn set_zoom_percent(mut self: Pin<&mut Self>, percent: f64) {
        if !percent.is_finite() || !(0.000_001..=20_000.0).contains(&percent) {
            return;
        }
        let factor = percent / self.rust().view.zoom_percent();
        self.as_mut().zoom_both(f(factor));
        self.refresh();
    }

    pub fn zoom_in(mut self: Pin<&mut Self>) {
        self.as_mut().zoom_both(f(ZOOM_STEP));
        self.refresh();
    }

    pub fn zoom_out(mut self: Pin<&mut Self>) {
        self.as_mut().zoom_both(f(1.0 / ZOOM_STEP));
        self.refresh();
    }

    pub fn fit(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.fit();
        self.refresh();
    }

    pub fn hscroll_to(mut self: Pin<&mut Self>, fraction: f64) {
        self.as_mut().rust_mut().view.hscroll_to(f(fraction));
        self.refresh();
    }

    pub fn vscroll_to(mut self: Pin<&mut Self>, fraction: f64) {
        self.as_mut().rust_mut().view.vscroll_to(f(fraction));
        self.refresh();
    }

    fn history(mut self: Pin<&mut Self>, redo: bool) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        // A display tree's history belongs to the document it is drawn for.
        if self.rust().display_only {
            return;
        }
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
            r.view.sync(crate::bridge::checked_document!(
                access_thread,
                session.tree(doc)
            ));
            result
        };
        match result {
            Ok(_) => {
                self.as_mut().set_dirty(true);
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

    pub fn check_theme(mut self: Pin<&mut Self>) {
        if let Some(font) = crate::editor_core::prepare_font(&self.rust().mono_font.to_string()) {
            self.as_mut().set_mono_font(QString::from(&font));
        }

        if let Some(update) =
            crate::editor_core::prepare_theme(&self.rust().theme_stamp, theme_stamp, load_theme)
        {
            {
                let mut r = self.as_mut().rust_mut();
                r.theme = update.theme;
                r.theme_stamp = update.stamp;
            }
            self.as_mut()
                .set_background(QString::from(&update.background));
            self.as_mut()
                .set_foreground(QString::from(&update.foreground));
            self.as_mut().set_accent(QString::from(&update.accent));
            self.as_mut().set_ok_color(QString::from(&update.ok));
            self.as_mut().set_error_color(QString::from(&update.error));
            self.refresh();
        }
    }

    pub fn memory_report(&self) -> QString {
        QString::from(&memory_text())
    }

    // Edits.

    pub fn reroot(mut self: Pin<&mut Self>) {
        let op = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| self.rust().view.reroot_op(d))
        )
        .flatten();
        match op {
            Some(op) => {
                if self.as_mut().edit(op).is_ok() {
                    // The clade's node has a new index; select nothing.
                    self.as_mut().rust_mut().view.selected = None;
                    self.as_mut().done("Rerooted");
                    self.refresh();
                }
            }
            None => self.notify("Select a branch to root on"),
        }
    }

    pub fn midpoint_root(self: Pin<&mut Self>) {
        self.automatic_root(Op::TreeMidpointRoot {}, "Midpoint rooted");
    }

    pub fn minimum_variance_root(self: Pin<&mut Self>) {
        self.automatic_root(Op::TreeMinimumVarianceRoot {}, "MRTTV rooted");
    }

    fn automatic_root(mut self: Pin<&mut Self>, op: Op, message: &str) {
        if self.as_mut().edit(op).is_ok() {
            self.done(message);
        }
    }

    pub fn rotate(mut self: Pin<&mut Self>) {
        let op = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| self.rust().view.rotate_op(d))
        )
        .flatten();
        match op {
            Some(op) => {
                let _ = self.as_mut().edit(op);
            }
            None => self.notify("Select a clade to rotate"),
        }
    }

    pub fn ladderize(mut self: Pin<&mut Self>, small_first: bool) {
        let _ = self.as_mut().edit(Op::TreeLadderize { small_first });
    }

    pub fn root_json(&self) -> QString {
        let v = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| {
                let (side, other, length, share) = self.rust().view.root_side(d)?;
                let name = |n: usize| {
                    if d.node(n).is_tip() {
                        basecraft_viewmodel::tree::display_name(&d.node(n).name)
                    } else {
                        let (a, b) = d.tip_range(n);
                        format!(
                            "{} … {}",
                            basecraft_viewmodel::tree::display_name(&d.node(a).name),
                            basecraft_viewmodel::tree::display_name(&d.node(b).name)
                        )
                    }
                };
                Some(json!({
                    "side": name(side),
                    "other": name(other),
                    "length": length,
                    "share": share,
                }))
            })
        )
        .flatten()
        .unwrap_or(Value::Null);
        QString::from(&v.to_string())
    }

    pub fn move_root(mut self: Pin<&mut Self>, fraction: f64) {
        let op = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| {
                let (side, ..) = self.rust().view.root_side(d)?;
                Some(Op::TreeRootPosition {
                    clade: d.clade_ref(side),
                    fraction: fraction.clamp(0.0, 1.0),
                })
            })
        )
        .flatten();
        match op {
            Some(op) => {
                if self.as_mut().edit(op).is_ok() {
                    self.done("Moved the root");
                }
            }
            None => self.fail("The root needs two branches with lengths"),
        }
    }

    pub fn tip_name(&self) -> QString {
        let name = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| {
                let s = self.rust().view.selected?;
                d.node(s).is_tip().then(|| d.node(s).name.clone())
            })
        )
        .flatten()
        .unwrap_or_default();
        QString::from(&name)
    }

    pub fn rename_tip(mut self: Pin<&mut Self>, name: &QString) -> QString {
        let from = self.tip_name().to_string();
        if from.is_empty() {
            return QString::from("Select a tip first");
        }
        let to = name.to_string();
        if to.trim() == from {
            return QString::default();
        }
        match self.as_mut().edit(Op::TreeRenameTip { from, to }) {
            Ok(()) => QString::default(),
            Err(msg) => QString::from(&msg),
        }
    }

    pub fn annotation_json(&self) -> QString {
        let v = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| {
                annotation_request(
                    d,
                    &self.rust().view,
                    &self.record_revision().to_string(),
                    &css(self.rust().theme.foreground),
                )
                .unwrap_or_else(|error| json!({"error": error}))
            })
        )
        .unwrap_or(Value::Null);
        QString::from(&v.to_string())
    }

    pub fn set_annotation(mut self: Pin<&mut Self>, json: &QString) -> QString {
        let parsed: Result<(u32, CladeAnnotation), String> =
            serde_json::from_str::<Value>(&json.to_string())
                .map_err(|e| e.to_string())
                .and_then(|v| {
                    self.with_doc(|d| {
                        checked_annotation(d, &self.record_revision().to_string(), &v)
                    })
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "No tree open".to_string())?
                });
        match parsed {
            Ok((id, annotation)) => {
                let label = annotation.label.clone();
                match self.as_mut().edit(Op::TreeAnnotate {
                    id,
                    annotation: Some(annotation),
                }) {
                    Ok(()) => {
                        self.as_mut().done(&if label.is_empty() {
                            "Annotated the clade".to_string()
                        } else {
                            format!("Annotated {label}")
                        });
                        QString::default()
                    }
                    Err(msg) => QString::from(&msg),
                }
            }
            Err(msg) => QString::from(&msg),
        }
    }

    pub fn remove_annotation(mut self: Pin<&mut Self>, id: i32) {
        let Ok(id) = u32::try_from(id) else { return };
        if self
            .as_mut()
            .edit(Op::TreeAnnotate {
                id,
                annotation: None,
            })
            .is_ok()
        {
            self.done("Removed the annotation");
        }
    }

    pub fn compose_metadata_labels(
        mut self: Pin<&mut Self>,
        project: &QString,
        fields: &QString,
    ) -> QString {
        let planned = (|| -> Result<(TreeStyle, usize), String> {
            let r = self.rust();
            let (session, doc) = r.session.as_ref().zip(r.doc).ok_or("No tree open")?;
            let tree = session.tree(doc).map_err(|e| e.to_string())?;
            let mut style = tree.style().clone();
            let previous_style = style.clone();
            let fields: Vec<basecraft_protocol::metadata::LabelField> =
                serde_json::from_str(&fields.to_string()).map_err(|e| e.to_string())?;
            style.tip_labels.clear();
            style.extra.remove("metadata_labels");
            if fields.is_empty() {
                return Ok((style, 0));
            }
            let root = PathBuf::from(project.to_string());
            let mut reader = Session::unlogged(&root);
            let metadata = basecraft_ops::metadata::open(&mut reader, false)
                .map_err(|e| e.to_string())?
                .ok_or("This project has no specimen metadata")?;
            let source = basecraft_ops::metadata::consumer_source(&reader, metadata)
                .map_err(|e| e.to_string())?;
            basecraft_ops::metadata::verify_label_owner(&previous_style, &source)
                .map_err(|e| e.to_string())?;
            let path = session
                .root()
                .join(session.doc_key(doc).map_err(|e| e.to_string())?);
            let request = basecraft_ops::metadata::ConsumerRequest::Labels {
                document: path.to_string_lossy().into_owned(),
                records: tree
                    .nodes()
                    .iter()
                    .filter(|n| n.is_tip())
                    .map(|n| n.name.clone())
                    .collect(),
                fields,
            };
            let plan = basecraft_ops::metadata::consumer_plan(&reader, metadata, &request)
                .map_err(|e| e.to_string())?;
            let labels: Vec<basecraft_protocol::metadata::ComposedLabel> =
                serde_json::from_value(plan["labels"].clone()).map_err(|e| e.to_string())?;
            let issues = labels.iter().filter(|l| l.issue.is_some()).count();
            style.tip_labels = labels
                .into_iter()
                .filter(|l| l.issue.is_none())
                .map(|l| (l.record, l.label))
                .collect();
            style.extra.insert("metadata_labels".into(), plan);
            Ok((style, issues))
        })();
        match planned {
            Err(error) => QString::from(error),
            Ok((style, issues)) => {
                if let Err(error) = self.as_mut().edit(Op::TreeStyle {
                    style,
                    remove_supports: Vec::new(),
                }) {
                    QString::from(error)
                } else {
                    self.done(&format!("Composed tip labels; {issues} tips retain their identity because links are missing or ambiguous"));
                    QString::default()
                }
            }
        }
    }

    pub fn style_json(&self) -> QString {
        let v = crate::bridge::checked_document!(self.qt_thread(), self
            .with_doc(|d| {
                json!({
                    "sets": d.supports().iter().map(|s| json!({
                        "name": s.name,
                        "source": s.source,
                        "decimals": s.decimals(d.style()),
                        "automatic_decimals": if s.uses_proportions(d.style()) { 2 } else { 0 },
                        "proportions": s.uses_proportions(d.style()),
                        "kind": s.kind,
                    })).collect::<Vec<_>>(),
                    "style": d.style(),
                    "tips": d.nodes().iter().filter(|n| n.is_tip()).map(|n| &n.name).collect::<Vec<_>>(),
                })
            }))
            .unwrap_or(Value::Null);
        QString::from(&v.to_string())
    }

    pub fn set_style(self: Pin<&mut Self>, json: &QString) -> QString {
        self.set_style_with_removals(json, &QString::from("[]"))
    }

    pub fn set_style_with_removals(
        mut self: Pin<&mut Self>,
        json: &QString,
        removed: &QString,
    ) -> QString {
        let remove_supports: Vec<String> = match serde_json::from_str(&removed.to_string()) {
            Ok(names) => names,
            Err(e) => return QString::from(&format!("Bad support removals: {e}")),
        };
        let style: TreeStyle = match serde_json::from_str(&json.to_string()) {
            Ok(s) => s,
            Err(e) => return QString::from(&format!("Bad style: {e}")),
        };
        if remove_supports.is_empty()
            && match self.with_doc(|d| *d.style() == style) {
                Ok(value) => value,
                Err(error) => {
                    let message = error.to_string();
                    self.as_mut().fail(&message);
                    return QString::from(&message);
                }
            }
            .unwrap_or(true)
        {
            return QString::default();
        }
        match self.as_mut().edit(Op::TreeStyle {
            style,
            remove_supports,
        }) {
            Ok(()) => QString::default(),
            Err(msg) => QString::from(&msg),
        }
    }

    pub fn place_support(mut self: Pin<&mut Self>, placement: &QString) {
        let p = match placement.to_string().as_str() {
            "above" => SupportPlacement::Above,
            "below" => SupportPlacement::Below,
            _ => SupportPlacement::Natural,
        };
        let Some(mut style) = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| d.style().clone())
        ) else {
            return;
        };
        if style.support_placement == p {
            return;
        }
        style.support_placement = p;
        let _ = self.as_mut().edit(Op::TreeStyle {
            style,
            remove_supports: Vec::new(),
        });
    }

    pub fn set_sizes(
        mut self: Pin<&mut Self>,
        tip_font: i32,
        support_font: i32,
        branch_width: i32,
    ) {
        let Some(mut style) = crate::bridge::checked_document!(
            self.qt_thread(),
            self.with_doc(|d| d.style().clone())
        ) else {
            return;
        };
        let before = style.clone();
        for (value, field) in [
            (tip_font, &mut style.tip_font),
            (support_font, &mut style.support_font),
            (branch_width, &mut style.branch_width),
        ] {
            if let Ok(value) = u32::try_from(value)
                && value > 0
            {
                *field = value;
            }
        }
        if style != before {
            let _ = self.as_mut().edit(Op::TreeStyle {
                style,
                remove_supports: Vec::new(),
            });
        }
    }

    pub fn support_import_json(&self, path: &QString) -> QString {
        let result = match basecraft_ops::files::load_tree(Path::new(&path.to_string())) {
            Ok(tree) => json!({"sets": tree.doc.supports().iter().map(|set| json!({
                "name": set.name, "kind": set.kind, "source": set.source
            })).collect::<Vec<_>>()}),
            Err(error) => json!({"error": error.to_string(), "sets": []}),
        };
        QString::from(&result.to_string())
    }

    pub fn add_support_from(mut self: Pin<&mut Self>, path: &QString, name: &QString) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let started = Instant::now();
        let path = PathBuf::from(path.to_string());
        if self.rust().display_only {
            self.fail("This tree is drawn for another document. Open its file to edit it");
            return;
        }
        // Shared with headless routes: values replay from typed edits and the
        // source tree is referenced as input evidence, not copied.
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return;
            };
            let result = session.import_support(doc, &path, &name.to_string());
            r.view.sync(crate::bridge::checked_document!(
                access_thread,
                session.tree(doc)
            ));
            result
        };
        match result {
            Ok(imported) if imported.added.is_empty() => {
                self.notify(&format!("{} has no support values", file_name(&path)));
            }
            Ok(imported) => {
                self.as_mut().set_dirty(true);
                self.as_mut().refresh();
                self.done(&format!(
                    "Added {} on {} branches{}",
                    imported.added.join(", "),
                    imported.matched,
                    took(started)
                ));
            }
            Err(e) => self.fail(&format!(
                "Can't import support from {}: {e}",
                file_name(&path)
            )),
        }
    }

    pub fn remove_support(mut self: Pin<&mut Self>, name: &QString) {
        let name = name.to_string();
        if self
            .as_mut()
            .edit(Op::TreeRemoveSupport { name: name.clone() })
            .is_ok()
        {
            self.done(&format!("Removed {name}"));
        }
    }

    pub fn export_svg(self: Pin<&mut Self>, path: &QString) {
        self.export_figure(path, 2, false);
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
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        if !(0..=2).contains(&scope) {
            self.fail("Unknown figure scope");
            return;
        }
        if scope == 1 && self.rust().view.selected.is_none() {
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
                crate::bridge::publication_theme()
            } else {
                r.theme.clone()
            };
            let tree = session.tree(doc)?;
            let svg = if scope == 0 {
                let mut frame = Frame::new();
                r.view.build(tree, &theme, &mut frame);
                let (width, height) = r.view.size();
                basecraft_viewmodel::svg::frame_svg(
                    &frame,
                    width,
                    height,
                    &r.mono_font.to_string(),
                    theme.background,
                )
            } else if scope == 1 {
                let trees = r
                    .view
                    .selected_roots(tree)
                    .into_iter()
                    .map(|node| tree.subtree(node))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| basecraft_ops::Error::Invalid(e.to_string()))?;
                basecraft_viewmodel::tree::forest_svg(
                    &trees,
                    900.0,
                    20.0,
                    &theme,
                    &r.mono_font.to_string(),
                )
            } else {
                to_svg_with_style(tree, 900.0, 20.0, &theme, &r.mono_font.to_string())
            };
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
}

impl TreeRust {
    fn view_size(&self) -> (f32, f32) {
        self.view.size()
    }
}

crate::find_bridge::controller_find!(qobject::TreeController, tree, |d| {
    basecraft_viewmodel::find::Input::Names(
        d.tips_in_order()
            .iter()
            .map(|&node| (node, d.node(node).name.clone()))
            .collect(),
    )
});

impl qobject::TreeController {
    fn find_reveal(mut self: std::pin::Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.tree(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        let (Some(session), Some(doc), Some(hit)) = (&r.session, r.doc, r.find.hit()) else {
            return;
        };
        let d = crate::bridge::checked_document!(access_thread, session.tree(doc));
        r.view.sync(d);
        r.view.reveal_find(hit.row);
    }
    fn find_y(&self) -> f32 {
        0.0
    }
}

#[cfg(test)]
mod annotation_tests {
    use super::{annotation_request, checked_annotation};
    use basecraft_docs::TreeDoc;
    use basecraft_viewmodel::TreeView;
    use serde_json::json;

    #[test]
    fn captured_annotation_keeps_target_and_rejects_stale_or_retargeted_requests() {
        let mut doc = TreeDoc::from_newick("test", "((A,B)95,C);").unwrap();
        let node = doc.resolve(&["A".into(), "B".into()]).unwrap();
        let mut view = TreeView::new(600.0, 400.0);
        view.selected = Some(node);
        let mut value = annotation_request(&doc, &view, "session:0:1:4", "#808080").unwrap();
        value["label"] = json!("95 / display name");
        view.selected = Some(doc.root());
        let (id, annotation) = checked_annotation(&doc, "session:0:1:4", &value).unwrap();
        assert_eq!(doc.resolve(&annotation.clade).unwrap(), node);
        assert_eq!(annotation.label, "95 / display name");
        assert!(checked_annotation(&doc, "session:1:1:4", &value).is_err());
        assert!(checked_annotation(&doc, "session:0:2:4", &value).is_err());
        let mut changed = value.clone();
        changed["clade"] = json!(["C"]);
        assert!(checked_annotation(&doc, "session:0:1:4", &changed).is_err());
        changed = value.clone();
        changed["id"] = json!(99);
        assert!(checked_annotation(&doc, "session:0:1:4", &changed).is_err());
        changed = value.clone();
        changed["target"]["node"] = json!(u64::MAX);
        assert!(checked_annotation(&doc, "session:0:1:4", &changed).is_err());
        assert!(checked_annotation(&doc, "", &json!({})).is_err());
        doc.annotate(id, Some(annotation)).unwrap();
        assert!(checked_annotation(&doc, "session:0:1:4", &value).is_err());
    }

    #[test]
    fn existing_annotation_edits_survive_branch_reordering() {
        let mut doc = TreeDoc::from_newick("test", "((A,B)95,C);").unwrap();
        let node = doc.resolve(&["A".into(), "B".into()]).unwrap();
        let mut view = TreeView::new(600.0, 400.0);
        view.selected = Some(node);
        let request = annotation_request(&doc, &view, "before", "#808080").unwrap();
        let (id, mut annotation) = checked_annotation(&doc, "before", &request).unwrap();
        annotation.label = "original".into();
        doc.annotate(id, Some(annotation)).unwrap();
        doc.rotate(&["A".into(), "B".into()]).unwrap();
        let mut request = annotation_request(&doc, &view, "after", "#808080").unwrap();
        assert_eq!(request["label"], "original");
        assert_eq!(request["exists"], true);
        request["label"] = json!("edited");
        let (_, annotation) = checked_annotation(&doc, "after", &request).unwrap();
        assert_eq!(doc.resolve(&annotation.clade).unwrap(), node);
    }

    #[test]
    fn ambiguous_targets_are_refused_without_mutation() {
        for text in ["((A)unary,B);", "((A,A),B);"] {
            let doc = TreeDoc::from_newick("test", text).unwrap();
            let original = doc.to_newick();
            let mut view = TreeView::new(600.0, 400.0);
            view.selected = Some(1);
            assert!(annotation_request(&doc, &view, "revision", "#808080").is_err());
            assert_eq!(doc.to_newick(), original);
            assert!(doc.annotations().is_empty());
        }
    }
}

#[cfg(test)]
#[allow(dead_code)] // Invoked by the windowless application_bootstrap harness.
#[path = "../tests/controller_refusal/tree.rs"]
pub(crate) mod refusal_tests;

crate::editor_save::ordinary_save!(qobject::TreeController, busy);

crate::editor_save::confirmed_save!(qobject::TreeController);
