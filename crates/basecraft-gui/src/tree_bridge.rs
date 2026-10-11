//! The Rust object QML talks to for one tree editor.
//!
//! As with the other editors, QML forwards raw input; the view model turns
//! it into viewport changes, selections and operations, and every edit goes
//! through the ops session.

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
        /// A node is selected; `tip_selected` if it's a tip.
        #[qproperty(bool, has_selection)]
        #[qproperty(bool, tip_selected)]
        /// Where support values sit: "natural", "above" or "below".
        #[qproperty(QString, support_placement)]
        type TreeController = super::TreeRust;

        #[qinvokable]
        fn open_file(self: Pin<&mut TreeController>, path: &QString) -> bool;
        #[qinvokable]
        fn resize(self: Pin<&mut TreeController>, width: f64, height: f64);
        #[qinvokable]
        fn wheel(self: Pin<&mut TreeController>, x: f64, y: f64, dy: f64, dx: f64, modifiers: i32);
        /// Select what's under the pointer.
        #[qinvokable]
        fn press(self: Pin<&mut TreeController>, x: f64, y: f64);
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
        /// Save to a new file and carry on editing that one.
        #[qinvokable]
        fn save_as(self: Pin<&mut TreeController>, path: &QString) -> bool;
        /// Reload the Omarchy theme if it changed.
        #[qinvokable]
        fn check_theme(self: Pin<&mut TreeController>);
        /// Write a pending log entry.
        #[qinvokable]
        fn flush(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn memory_report(self: &TreeController) -> QString;

        // Edits.
        /// Root on the branch above the selected clade.
        #[qinvokable]
        fn reroot(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn rotate(self: Pin<&mut TreeController>);
        #[qinvokable]
        fn ladderize(self: Pin<&mut TreeController>, small_first: bool);
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
        /// Replace the style from JSON. Returns an error message, or empty.
        #[qinvokable]
        fn set_style(self: Pin<&mut TreeController>, json: &QString) -> QString;
        #[qinvokable]
        fn place_support(self: Pin<&mut TreeController>, placement: &QString);
        /// Add the support values of the tree in `path`, matched branch by
        /// branch, named `name`.
        #[qinvokable]
        fn add_support_from(self: Pin<&mut TreeController>, path: &QString, name: &QString);
        #[qinvokable]
        fn remove_support(self: Pin<&mut TreeController>, name: &QString);
        /// Write the tree as an SVG figure.
        #[qinvokable]
        fn export_svg(self: Pin<&mut TreeController>, path: &QString);
    }
}

use std::path::PathBuf;
use std::pin::Pin;
use std::time::{Instant, SystemTime};

use basecraft_docs::TreeDoc;
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{CladeAnnotation, Op, SupportPlacement, TreeStyle};
use basecraft_viewmodel::tree::to_svg;
use basecraft_viewmodel::{Frame, Theme, TreeView};
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString};
use serde_json::{Value, json};

use crate::bridge::{
    CTRL, KEY_0, KEY_DOWN, KEY_EQUAL, KEY_ESCAPE, KEY_LEFT, KEY_MINUS, KEY_PLUS, KEY_RIGHT, KEY_S,
    KEY_UP, KEY_Y, KEY_Z, SHIFT, WHEEL_ZOOM, ZOOM_STEP, css, file_name, load_theme, memory_text,
    monospace_font, omarchy_theme_path, project_root, took,
};

pub struct TreeRust {
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
    dirty: bool,
    zoom_text: QString,
    hscroll_pos: f64,
    hscroll_size: f64,
    vscroll_pos: f64,
    vscroll_size: f64,
    has_selection: bool,
    tip_selected: bool,
    support_placement: QString,

    session: Option<Session>,
    doc: Option<DocId>,
    view: TreeView,
    theme: Theme,
    theme_stamp: Option<SystemTime>,
    scratch: Frame,
}

impl Default for TreeRust {
    fn default() -> Self {
        let mut c = Self {
            frame: QByteArray::default(),
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
            has_selection: false,
            tip_selected: false,
            support_placement: QString::from("natural"),
            session: None,
            doc: None,
            view: TreeView::new(800.0, 600.0),
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

fn placement_name(p: SupportPlacement) -> &'static str {
    match p {
        SupportPlacement::Natural => "natural",
        SupportPlacement::Above => "above",
        SupportPlacement::Below => "below",
    }
}

impl qobject::TreeController {
    fn with_doc<T>(&self, f: impl FnOnce(&TreeDoc) -> T) -> Option<T> {
        let r = self.rust();
        match (&r.session, r.doc) {
            (Some(s), Some(d)) => Some(f(s.tree(d))),
            _ => None,
        }
    }

    /// Rebuild the frame, status line and scroll bars.
    fn refresh(mut self: Pin<&mut Self>) {
        let (bytes, status, zoom, h, v, sel, tip, placement) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = session.tree(doc);
            r.view.build(d, &r.theme, &mut r.scratch);
            (
                r.scratch.to_bytes(),
                r.view.status(d),
                r.view.zoom_text(),
                r.view.hscroll(),
                r.view.vscroll(),
                r.view.selected.is_some(),
                r.view.selected.is_some_and(|s| d.node(s).is_tip()),
                placement_name(d.style().support_placement),
            )
        };
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
        self.as_mut().set_status(QString::from(&status));
        self.as_mut().set_zoom_text(QString::from(&zoom));
        self.as_mut().set_hscroll_size(f64::from(h.1));
        self.as_mut().set_hscroll_pos(f64::from(h.0));
        self.as_mut().set_vscroll_size(f64::from(v.1));
        self.as_mut().set_vscroll_pos(f64::from(v.0));
        self.as_mut().set_has_selection(sel);
        self.as_mut().set_tip_selected(tip);
        self.as_mut()
            .set_support_placement(QString::from(placement));
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
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return Err("No tree open".into());
            };
            let result = session.apply(doc, op);
            r.view.sync(session.tree(doc));
            result
        };
        match result {
            Ok(_) => {
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

    pub fn open_file(mut self: Pin<&mut Self>, path: &QString) -> bool {
        let path = PathBuf::from(path.to_string());
        let mut session = Session::new(project_root(&path));
        match session.open(&path) {
            Ok(doc) if session.is_tree(doc) => {
                {
                    let mut r = self.as_mut().rust_mut();
                    let r = &mut *r;
                    r.view.selected = None;
                    r.view.fit();
                    r.view.sync(session.tree(doc));
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
                self.fail(&format!("{} is not a tree", path.display()));
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
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = session.tree(doc);
            if modifiers & CTRL != 0 {
                // Ctrl: rows closer or further apart; with Shift, the tree
                // wider or narrower.
                let factor = f(WHEEL_ZOOM.powf(dy));
                r.view
                    .zoom_at(d, f(x), f(y), factor, modifiers & SHIFT == 0);
            } else if modifiers & SHIFT != 0 || dx.abs() > dy.abs() {
                let d = if dx.abs() > dy.abs() { dx } else { dy };
                r.view.scroll_by(f(-d / 2.0), 0.0);
            } else {
                r.view.scroll_by(0.0, f(-dy / 2.0));
            }
        }
        self.refresh();
    }

    pub fn press(mut self: Pin<&mut Self>, x: f64, y: f64) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let (Some(session), Some(doc)) = (&r.session, r.doc) {
                r.view.select_at(session.tree(doc), f(x), f(y));
            }
        }
        self.refresh();
    }

    pub fn context_press(mut self: Pin<&mut Self>, x: f64, y: f64) -> QString {
        self.as_mut().press(x, y);
        let info = self
            .with_doc(|d| {
                let v = &self.rust().view;
                let sel = v.selected;
                json!({
                    "node": sel.is_some(),
                    "tip": sel.is_some_and(|s| d.node(s).is_tip()),
                    "root": sel == Some(d.root()),
                    "annotated": v.selected_annotation(d).is_some(),
                    "annotation": v.selected_annotation(d).map_or(0, |(id, _)| id),
                })
            })
            .unwrap_or(Value::Null);
        QString::from(&info.to_string())
    }

    pub fn hover(&self, x: f64, y: f64) -> QString {
        let text = self
            .with_doc(|d| self.rust().view.hover_text(d, f(x), f(y)))
            .flatten()
            .unwrap_or_default();
        QString::from(&text)
    }

    pub fn key(mut self: Pin<&mut Self>, key: i32, modifiers: i32) -> bool {
        let ctrl = modifiers & CTRL != 0;
        let shift = modifiers & SHIFT != 0;
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
            KEY_S if ctrl && !shift => {
                self.as_mut().save();
                true
            }
            KEY_0 if ctrl => {
                self.as_mut().rust_mut().view.fit();
                true
            }
            KEY_PLUS | KEY_EQUAL | KEY_MINUS if ctrl => {
                let factor = if key == KEY_MINUS {
                    1.0 / ZOOM_STEP
                } else {
                    ZOOM_STEP
                };
                self.as_mut().zoom_centre(f(factor));
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
                    let d = session.tree(doc);
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
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        if let (Some(session), Some(doc)) = (&r.session, r.doc) {
            let (w, h) = (r.view_size().0, r.view_size().1);
            r.view
                .zoom_at(session.tree(doc), w / 2.0, h / 2.0, factor, true);
        }
    }

    pub fn zoom_in(mut self: Pin<&mut Self>) {
        self.as_mut().zoom_centre(f(ZOOM_STEP));
        self.refresh();
    }

    pub fn zoom_out(mut self: Pin<&mut Self>) {
        self.as_mut().zoom_centre(f(1.0 / ZOOM_STEP));
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
            r.view.sync(session.tree(doc));
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
        if !self.as_mut().open_file(path) {
            return false;
        }
        self.done(&format!("Saved as {}{}", file_name(&target), took(started)));
        true
    }

    pub fn check_theme(mut self: Pin<&mut Self>) {
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
            let (ok, err) = (css(theme.nearest_hue(120.0)), css(theme.nearest_hue(0.0)));
            {
                let mut r = self.as_mut().rust_mut();
                r.theme = theme;
                r.theme_stamp = stamp;
            }
            self.as_mut().set_background(QString::from(&bg));
            self.as_mut().set_foreground(QString::from(&fg));
            self.as_mut().set_accent(QString::from(&ac));
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

    // Edits.

    pub fn reroot(mut self: Pin<&mut Self>) {
        let op = self.with_doc(|d| self.rust().view.reroot_op(d)).flatten();
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

    pub fn rotate(mut self: Pin<&mut Self>) {
        let op = self.with_doc(|d| self.rust().view.rotate_op(d)).flatten();
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

    pub fn tip_name(&self) -> QString {
        let name = self
            .with_doc(|d| {
                let s = self.rust().view.selected?;
                d.node(s).is_tip().then(|| d.node(s).name.clone())
            })
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
        let v = self
            .with_doc(|d| {
                let view = &self.rust().view;
                let s = view.selected?;
                let (id, a, exists) = if let Some((id, a)) = view.selected_annotation(d) {
                    (id, a, true)
                } else {
                    let mut a = CladeAnnotation::new(d.clade_ref(s), d.node(s).name.clone());
                    a.colour = css(self.rust().theme.foreground);
                    (d.next_annotation_id(), a, false)
                };
                let mut v = serde_json::to_value(&a).ok()?;
                v["id"] = json!(id);
                v["exists"] = json!(exists);
                v["tips"] = json!(d.tip_counts()[s]);
                Some(v)
            })
            .flatten()
            .unwrap_or(Value::Null);
        QString::from(&v.to_string())
    }

    pub fn set_annotation(mut self: Pin<&mut Self>, json: &QString) -> QString {
        let parsed: Result<(u32, CladeAnnotation), String> =
            serde_json::from_str::<Value>(&json.to_string())
                .map_err(|e| e.to_string())
                .and_then(|v| {
                    let id = v["id"]
                        .as_u64()
                        .and_then(|i| u32::try_from(i).ok())
                        .unwrap_or(1);
                    let a: CladeAnnotation =
                        serde_json::from_value(v).map_err(|e| format!("Bad annotation: {e}"))?;
                    Ok((id, a))
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

    pub fn style_json(&self) -> QString {
        let v = self
            .with_doc(|d| {
                json!({
                    "sets": d.supports().iter().map(|s| json!({
                        "name": s.name,
                        "source": s.source,
                    })).collect::<Vec<_>>(),
                    "style": d.style(),
                })
            })
            .unwrap_or(Value::Null);
        QString::from(&v.to_string())
    }

    pub fn set_style(mut self: Pin<&mut Self>, json: &QString) -> QString {
        let style: TreeStyle = match serde_json::from_str(&json.to_string()) {
            Ok(s) => s,
            Err(e) => return QString::from(&format!("Bad style: {e}")),
        };
        if self.with_doc(|d| *d.style() == style).unwrap_or(true) {
            return QString::default();
        }
        match self.as_mut().edit(Op::TreeStyle { style }) {
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
        let Some(mut style) = self.with_doc(|d| d.style().clone()) else {
            return;
        };
        if style.support_placement == p {
            return;
        }
        style.support_placement = p;
        let _ = self.as_mut().edit(Op::TreeStyle { style });
    }

    pub fn add_support_from(mut self: Pin<&mut Self>, path: &QString, name: &QString) {
        let started = Instant::now();
        let path = PathBuf::from(path.to_string());
        let other = match basecraft_ops::files::load_tree(&path) {
            Ok(t) => t,
            Err(e) => {
                self.fail(&format!("Can't read {}: {e}", path.display()));
                return;
            }
        };
        let sets = match self.with_doc(|d| d.support_from(&other.doc)) {
            Some(Ok(sets)) => sets,
            Some(Err(e)) => {
                self.fail(&e.0);
                return;
            }
            None => return,
        };
        if sets.is_empty() {
            self.notify(&format!("{} has no support values", file_name(&path)));
            return;
        }
        let base = name.to_string().trim().to_string();
        let source = file_name(&path);
        let many = sets.len() > 1;
        let matched: usize = sets
            .iter()
            .map(|(_, v)| v.iter().filter(|x| x.is_some()).count())
            .max()
            .unwrap_or(0);
        if let Some(s) = &mut self.as_mut().rust_mut().session {
            s.begin_group();
        }
        let mut added = Vec::new();
        for (set, values) in sets {
            let name = match (base.is_empty(), many) {
                (true, _) => set,
                (false, false) => base.clone(),
                (false, true) => format!("{base} {set}"),
            };
            if self
                .as_mut()
                .edit(Op::TreeAddSupport {
                    name: name.clone(),
                    source: source.clone(),
                    values,
                })
                .is_err()
            {
                break;
            }
            added.push(name);
        }
        if let Some(s) = &mut self.as_mut().rust_mut().session {
            s.end_group();
        }
        if !added.is_empty() {
            self.done(&format!(
                "Added {} on {matched} branches{}",
                added.join(", "),
                took(started)
            ));
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

    pub fn export_svg(mut self: Pin<&mut Self>, path: &QString) {
        let started = Instant::now();
        let target = PathBuf::from(path.to_string());
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return;
            };
            let svg = to_svg(session.tree(doc), 900.0, 20.0);
            session.export(doc, &target, "svg", |w| w.write_all(svg.as_bytes()))
        };
        match result {
            Ok(()) => self.done(&format!("Exported {}{}", file_name(&target), took(started))),
            Err(e) => self.fail(&format!("Export failed: {e}")),
        }
    }
}

impl TreeRust {
    fn view_size(&self) -> (f32, f32) {
        self.view.size()
    }
}
