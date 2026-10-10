//! The Rust object QML talks to: one genome editor.
//!
//! QML forwards raw input here. The view model turns it into viewport
//! changes, selections and operations; every edit goes through the ops
//! session, so it's logged and undoable like any other operation.

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
        #[qproperty(QString, title)]
        #[qproperty(QString, message)]
        #[qproperty(QString, background)]
        #[qproperty(QString, foreground)]
        #[qproperty(QString, accent)]
        #[qproperty(bool, circular)]
        #[qproperty(bool, dirty)]
        #[qproperty(QString, zoom_text)]
        #[qproperty(f64, scroll_pos)]
        #[qproperty(f64, scroll_size)]
        type GenomeController = super::ControllerRust;

        #[qinvokable]
        fn open_file(self: Pin<&mut GenomeController>, path: &QString) -> bool;
        #[qinvokable]
        fn resize(self: Pin<&mut GenomeController>, width: f64, height: f64);
        #[qinvokable]
        fn wheel(self: Pin<&mut GenomeController>, x: f64, dy: f64, dx: f64, modifiers: i32);
        #[qinvokable]
        fn press(self: Pin<&mut GenomeController>, x: f64, y: f64, modifiers: i32);
        #[qinvokable]
        fn drag(self: Pin<&mut GenomeController>, x: f64, y: f64);
        #[qinvokable]
        fn release(self: Pin<&mut GenomeController>);
        /// Returns whether the key was handled.
        #[qinvokable]
        fn key(self: Pin<&mut GenomeController>, key: i32, text: &QString, modifiers: i32) -> bool;
        #[qinvokable]
        fn toggle_mode(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn fit(self: Pin<&mut GenomeController>);
        /// Show residues `start..end`.
        #[qinvokable]
        fn show_range(self: Pin<&mut GenomeController>, start: f64, end: f64);
        #[qinvokable]
        fn undo(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn redo(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn save(self: Pin<&mut GenomeController>);
        /// Reload the Omarchy theme if it changed.
        #[qinvokable]
        fn check_theme(self: Pin<&mut GenomeController>);
        /// Write a pending typing burst to the log.
        #[qinvokable]
        fn flush(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn memory_report(self: &GenomeController) -> QString;

        /// Right-click at a point. Returns JSON describing what the context
        /// menu should offer: `{"feature": id or -1, "label", "selection": bool,
        /// "length"}`.
        #[qinvokable]
        fn context_press(self: Pin<&mut GenomeController>, x: f64, y: f64) -> QString;
        #[qinvokable]
        fn double_click(self: Pin<&mut GenomeController>, x: f64, y: f64);
        /// Zoom by `factor` around `x` (pinch gestures).
        #[qinvokable]
        fn zoom_by(self: Pin<&mut GenomeController>, x: f64, factor: f64);
        #[qinvokable]
        fn zoom_in(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn zoom_out(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn zoom_to_selection(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn zoom_to_feature(self: Pin<&mut GenomeController>, id: i32);
        /// Select the residues a feature covers.
        #[qinvokable]
        fn select_feature_range(self: Pin<&mut GenomeController>, id: i32);
        /// A feature as JSON for the editor dialog: kind, GenBank-style
        /// location, qualifiers.
        #[qinvokable]
        fn feature_json(self: &GenomeController, id: i32) -> QString;
        /// A new feature over the selection, as JSON, not yet added.
        #[qinvokable]
        fn new_feature_json(self: &GenomeController) -> QString;
        /// Add (`id` < 0) or replace a feature from editor JSON. Returns an
        /// error message, or an empty string on success.
        #[qinvokable]
        fn save_feature(self: Pin<&mut GenomeController>, id: i32, json: &QString) -> QString;
        #[qinvokable]
        fn delete_feature(self: Pin<&mut GenomeController>, id: i32);
        /// The selected residues, for the clipboard.
        #[qinvokable]
        fn selection_text(self: &GenomeController) -> QString;
        #[qinvokable]
        fn reverse_complement_selection(self: Pin<&mut GenomeController>);
        /// Scroll the linear view to a fraction of the sequence.
        #[qinvokable]
        fn scroll_to(self: Pin<&mut GenomeController>, fraction: f64);
        /// Called repeatedly while a selection drag is near or past an edge;
        /// returns whether the view moved.
        #[qinvokable]
        fn edge_scroll(self: Pin<&mut GenomeController>, x: f64, y: f64) -> bool;
        /// Feature description under the pointer, or an empty string.
        #[qinvokable]
        fn hover(self: &GenomeController, x: f64, y: f64) -> QString;
        /// Start panning (middle button).
        #[qinvokable]
        fn pan_press(self: Pin<&mut GenomeController>, x: f64);
        /// Id of the selected feature, or -1.
        #[qinvokable]
        fn selected_feature(self: &GenomeController) -> i32;
        /// Common INSDC feature keys, as a JSON array.
        #[qinvokable]
        fn feature_kinds(self: &GenomeController) -> QString;
    }
}

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::SystemTime;

use basecraft_docs::FeatureId;
use basecraft_io::genbank::{format_location, parse_location};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{Feature, Location, Op, Qualifier, Strand};
use basecraft_viewmodel::{Frame, GenomeView, Mode, Theme};
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString};
use serde_json::{Value, json};

// Qt key codes and modifiers used below.
const KEY_ESCAPE: i32 = 0x0100_0000;
const KEY_BACKSPACE: i32 = 0x0100_0003;
const KEY_DELETE: i32 = 0x0100_0007;
const KEY_HOME: i32 = 0x0100_0010;
const KEY_END: i32 = 0x0100_0011;
const KEY_LEFT: i32 = 0x0100_0012;
const KEY_RIGHT: i32 = 0x0100_0014;
const KEY_F2: i32 = 0x0100_0031;
const KEY_PLUS: i32 = 0x2b;
const KEY_MINUS: i32 = 0x2d;
const KEY_EQUAL: i32 = 0x3d;
const KEY_0: i32 = 0x30;
const KEY_N: i32 = 0x4e;
const KEY_S: i32 = 0x53;
const KEY_Y: i32 = 0x59;
const KEY_Z: i32 = 0x5a;
const SHIFT: i32 = 0x0200_0000;
const CTRL: i32 = 0x0400_0000;
const ALT: i32 = 0x0800_0000;

pub struct ControllerRust {
    frame: QByteArray,
    status: QString,
    title: QString,
    message: QString,
    background: QString,
    foreground: QString,
    accent: QString,
    circular: bool,
    dirty: bool,
    zoom_text: QString,
    scroll_pos: f64,
    scroll_size: f64,

    session: Option<Session>,
    doc: Option<DocId>,
    view: GenomeView,
    theme: Theme,
    theme_stamp: Option<SystemTime>,
    scratch: Frame,
}

impl Default for ControllerRust {
    fn default() -> Self {
        let mut c = Self {
            frame: QByteArray::default(),
            status: QString::default(),
            title: QString::from("Basecraft"),
            message: QString::default(),
            background: QString::default(),
            foreground: QString::default(),
            accent: QString::default(),
            circular: false,
            dirty: false,
            zoom_text: QString::default(),
            scroll_pos: 0.0,
            scroll_size: 1.0,
            session: None,
            doc: None,
            view: GenomeView::new(800.0, 600.0),
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
        c
    }
}

fn omarchy_theme_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".local/state/omarchy/current/theme/colors.toml");
    p.exists().then_some(p)
}

fn load_theme() -> Option<(Theme, Option<SystemTime>)> {
    let path = omarchy_theme_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    Some((Theme::from_omarchy(&text), stamp))
}

fn css(c: [f32; 4]) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// The project a file belongs to: the nearest ancestor with a `.basecraft`
/// directory, else the file's own directory.
fn project_root(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut cur = Some(dir.as_path());
    while let Some(d) = cur {
        if d.join(".basecraft").is_dir() {
            return d.to_path_buf();
        }
        cur = d.parent();
    }
    dir
}

impl qobject::GenomeController {
    fn len(&self) -> u64 {
        let r = self.rust();
        match (&r.session, r.doc) {
            (Some(s), Some(d)) => s.doc(d).len(),
            _ => 0,
        }
    }

    /// Rebuild the frame and status line.
    fn refresh(mut self: Pin<&mut Self>) {
        let (bytes, status) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = session.doc(doc);
            r.view.build(d, &r.theme, &mut r.scratch);
            (r.scratch.to_bytes(), r.view.status(d))
        };
        let (zoom, (pos, size)) = {
            let len = self.len();
            let v = &self.rust().view;
            let zoom = if v.mode == Mode::Linear {
                v.zoom_text(len)
            } else {
                String::new()
            };
            let scroll = if v.mode == Mode::Linear {
                v.scroll_fraction(len)
            } else {
                (0.0, 1.0)
            };
            (zoom, scroll)
        };
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
        self.as_mut().set_status(QString::from(&status));
        self.as_mut().set_zoom_text(QString::from(&zoom));
        self.as_mut().set_scroll_size(size);
        self.as_mut().set_scroll_pos(pos);
    }

    fn notify(mut self: Pin<&mut Self>, msg: &str) {
        self.as_mut().set_message(QString::from(msg));
    }

    pub fn open_file(mut self: Pin<&mut Self>, path: &QString) -> bool {
        let path = PathBuf::from(path.to_string());
        let mut session = Session::new(project_root(&path));
        match session.open(&path) {
            Ok(doc) => {
                let (name, len, circular) = {
                    let d = session.doc(doc);
                    (d.name.clone(), d.len(), d.circular)
                };
                {
                    let mut r = self.as_mut().rust_mut();
                    r.session = Some(session);
                    r.doc = Some(doc);
                    // Linear by default; circular is a view you switch to.
                    let _ = circular;
                    r.view.mode = Mode::Linear;
                    r.view.fit(len);
                }
                let file = path
                    .file_name()
                    .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
                self.as_mut()
                    .set_title(QString::from(&format!("{file} — {name} — Basecraft")));
                let circ = self.rust().view.mode == Mode::Circular;
                self.as_mut().set_circular(circ);
                self.as_mut().set_dirty(false);
                self.as_mut().notify("");
                self.refresh();
                true
            }
            Err(e) => {
                self.notify(&format!("Can't open {}: {e}", path.display()));
                false
            }
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn resize(mut self: Pin<&mut Self>, width: f64, height: f64) {
        let len = self.len();
        self.as_mut()
            .rust_mut()
            .view
            .resize(width as f32, height as f32, len);
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn wheel(mut self: Pin<&mut Self>, x: f64, dy: f64, dx: f64, modifiers: i32) {
        let len = self.len();
        {
            let mut r = self.as_mut().rust_mut();
            if r.view.mode == Mode::Circular {
                return;
            }
            if modifiers & SHIFT != 0 || dx.abs() > dy.abs() {
                let d = if dx.abs() > dy.abs() { dx } else { dy };
                r.view.pan((d / 2.0) as f32, len);
            } else {
                r.view.zoom_at(x as f32, 1.0015f64.powf(dy), len);
            }
        }
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn press(mut self: Pin<&mut Self>, x: f64, y: f64, modifiers: i32) {
        let len = self.len();
        self.as_mut()
            .rust_mut()
            .view
            .press(x as f32, y as f32, modifiers & SHIFT != 0, len);
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn drag(mut self: Pin<&mut Self>, x: f64, y: f64) {
        let len = self.len();
        self.as_mut()
            .rust_mut()
            .view
            .drag_to(x as f32, y as f32, len);
        self.refresh();
    }

    pub fn release(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.release();
    }

    pub fn toggle_mode(mut self: Pin<&mut Self>) {
        let circ = {
            let mut r = self.as_mut().rust_mut();
            r.view.mode = match r.view.mode {
                Mode::Linear => Mode::Circular,
                Mode::Circular => Mode::Linear,
            };
            r.view.mode == Mode::Circular
        };
        self.as_mut().set_circular(circ);
        self.refresh();
    }

    pub fn fit(mut self: Pin<&mut Self>) {
        let len = self.len();
        self.as_mut().rust_mut().view.fit(len);
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn show_range(mut self: Pin<&mut Self>, start: f64, end: f64) {
        let len = self.len();
        {
            let mut r = self.as_mut().rust_mut();
            r.view.mode = Mode::Linear;
            r.view.bp_per_px = (end - start).max(1.0) / f64::from(r.view.width);
            r.view.start = start;
            let w = r.view.width;
            let h = r.view.height;
            r.view.resize(w, h, len);
        }
        self.as_mut().set_circular(false);
        self.refresh();
    }

    /// Apply an edit through the session and report failures.
    fn edit(mut self: Pin<&mut Self>, op: Op) -> Option<basecraft_ops::session::Outcome> {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return None;
            };
            session.apply(doc, op)
        };
        match result {
            Ok(out) => {
                self.as_mut().set_dirty(true);
                Some(out)
            }
            Err(e) => {
                self.notify(&e.to_string());
                None
            }
        }
    }

    pub fn key(mut self: Pin<&mut Self>, key: i32, text: &QString, modifiers: i32) -> bool {
        let len = self.len();
        let ctrl = modifiers & CTRL != 0;
        let shift = modifiers & SHIFT != 0;
        let (cursor, selection, selected) = {
            let v = &self.rust().view;
            (v.cursor, v.selection, v.selected)
        };
        let handled = match key {
            KEY_Z if ctrl && shift => {
                self.as_mut().redo();
                true
            }
            KEY_Z if ctrl => {
                self.as_mut().undo();
                true
            }
            KEY_Y if ctrl => {
                self.as_mut().redo();
                true
            }
            KEY_S if ctrl => {
                self.as_mut().save();
                true
            }
            KEY_0 if ctrl => {
                self.as_mut().fit();
                true
            }
            KEY_N if ctrl => {
                if let Some((a, b)) = selection {
                    let strand = if shift {
                        Strand::Reverse
                    } else {
                        Strand::Forward
                    };
                    let feature = Feature {
                        kind: "misc_feature".into(),
                        location: Location::simple(a, b, strand),
                        qualifiers: vec![Qualifier::new("note", "new feature")],
                    };
                    if let Some(out) = self.as_mut().edit(Op::FeatureAdd { feature }) {
                        self.as_mut().rust_mut().view.selected = out.feature;
                    }
                } else {
                    self.as_mut().notify("Select a range first to annotate it");
                }
                true
            }
            KEY_F2 => {
                self.as_mut().toggle_mode();
                true
            }
            KEY_PLUS | KEY_EQUAL | KEY_MINUS if !ctrl => {
                let factor = if key == KEY_MINUS { 1.0 / 1.6 } else { 1.6 };
                let mut r = self.as_mut().rust_mut();
                #[allow(clippy::cast_precision_loss)]
                let x = cursor.map_or(r.view.width / 2.0, |c| r.view.x_of(c as f64));
                r.view.zoom_at(x, factor, len);
                true
            }
            KEY_LEFT | KEY_RIGHT => {
                let step = if ctrl { 10 } else { 1 };
                let delta = if key == KEY_LEFT { -step } else { step };
                self.as_mut().rust_mut().view.move_cursor(delta, shift, len);
                true
            }
            KEY_HOME | KEY_END => {
                let target = if key == KEY_HOME { 0 } else { len };
                let c = i64::try_from(cursor.unwrap_or(0)).unwrap_or(0);
                let t = i64::try_from(target).unwrap_or(0);
                self.as_mut().rust_mut().view.move_cursor(t - c, shift, len);
                true
            }
            KEY_ESCAPE => {
                let mut r = self.as_mut().rust_mut();
                r.view.selection = None;
                r.view.selected = None;
                true
            }
            KEY_DELETE if selected.is_some() => {
                let id = selected.map_or(0, |FeatureId(i)| i);
                if self.as_mut().edit(Op::FeatureRemove { id }).is_some() {
                    self.as_mut().rust_mut().view.selected = None;
                }
                true
            }
            KEY_BACKSPACE | KEY_DELETE => {
                let range = match (selection, cursor) {
                    (Some((a, b)), _) => Some((a, b)),
                    (None, Some(c)) if key == KEY_BACKSPACE && c > 0 => Some((c - 1, c)),
                    (None, Some(c)) if key == KEY_DELETE && c < len => Some((c, c + 1)),
                    _ => None,
                };
                if let Some((a, b)) = range {
                    let op = Op::SeqReplace {
                        start: a,
                        end: b,
                        residues: String::new(),
                    };
                    if self.as_mut().edit(op).is_some() {
                        let mut r = self.as_mut().rust_mut();
                        r.view.selection = None;
                        r.view.cursor = Some(a);
                    }
                }
                true
            }
            _ => {
                let t = text.to_string();
                let residue =
                    t.len() == 1 && t.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-');
                if residue && !ctrl && modifiers & ALT == 0 {
                    if let Some(c) = cursor {
                        let (start, end) = selection.unwrap_or((c, c));
                        let op = Op::SeqReplace {
                            start,
                            end,
                            residues: t.to_ascii_uppercase(),
                        };
                        if self.as_mut().edit(op).is_some() {
                            let mut r = self.as_mut().rust_mut();
                            r.view.selection = None;
                            r.view.cursor = Some(start + 1);
                            let l = len + 1 - (end - start);
                            r.view.reveal(start + 1, l);
                        }
                    } else {
                        self.as_mut()
                            .notify("Click in the sequence to place the cursor before typing");
                    }
                    true
                } else {
                    false
                }
            }
        };
        if handled {
            self.as_mut().clamp_cursor();
            self.refresh();
        }
        handled
    }

    fn clamp_cursor(mut self: Pin<&mut Self>) {
        let len = self.len();
        let mut r = self.as_mut().rust_mut();
        if let Some(c) = r.view.cursor {
            r.view.cursor = Some(c.min(len));
        }
        if let Some((a, b)) = r.view.selection {
            r.view.selection = (a.min(len) < b.min(len)).then(|| (a.min(len), b.min(len)));
        }
        if let (Some(id), Some(session), Some(doc)) = (r.view.selected, &r.session, r.doc)
            && session.doc(doc).feature(id).is_none()
        {
            r.view.selected = None;
        }
    }

    fn history(mut self: Pin<&mut Self>, redo: bool) {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return;
            };
            if redo {
                session.redo(doc)
            } else {
                session.undo(doc)
            }
        };
        match result {
            Ok(_) => self.as_mut().set_dirty(true),
            Err(e) => self.as_mut().notify(&e.to_string()),
        }
        self.as_mut().clamp_cursor();
        self.refresh();
    }

    pub fn undo(self: Pin<&mut Self>) {
        self.history(false);
    }

    pub fn redo(self: Pin<&mut Self>) {
        self.history(true);
    }

    pub fn save(mut self: Pin<&mut Self>) {
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return;
            };
            session
                .save(doc, None)
                .map(|()| session.doc_key(doc).to_string())
        };
        match result {
            Ok(key) => {
                self.as_mut().set_dirty(false);
                self.notify(&format!("Saved {key}"));
            }
            Err(e) => self.notify(&format!("Save failed: {e}")),
        }
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
            {
                let mut r = self.as_mut().rust_mut();
                r.theme = theme;
                r.theme_stamp = stamp;
            }
            self.as_mut().set_background(QString::from(&bg));
            self.as_mut().set_foreground(QString::from(&fg));
            self.as_mut().set_accent(QString::from(&ac));
            self.refresh();
        }
    }

    pub fn flush(mut self: Pin<&mut Self>) {
        if let Some(s) = &mut self.as_mut().rust_mut().session {
            let _ = s.flush();
        }
    }

    pub fn memory_report(&self) -> QString {
        let text = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
        let field = |name: &str| -> f64 {
            text.lines()
                .find(|l| l.starts_with(name))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
                .map_or(0.0, |kb| kb / 1024.0)
        };
        // Private_Clean is mostly pages of memory-mapped files, which the
        // kernel can drop at any time; anonymous memory is what we own.
        QString::from(&format!(
            "rss {:.1} MB, pss {:.1} MB, private {:.1} MB, of which anonymous {:.1} MB",
            field("Rss:"),
            field("Pss:"),
            field("Private_Clean:") + field("Private_Dirty:"),
            field("Anonymous:")
        ))
    }

    /// Run `f` on the open document's session, if any.
    fn with_doc<T>(&self, f: impl FnOnce(&Session, DocId) -> T) -> Option<T> {
        let r = self.rust();
        match (&r.session, r.doc) {
            (Some(s), Some(d)) => Some(f(s, d)),
            _ => None,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn context_press(mut self: Pin<&mut Self>, x: f64, y: f64) -> QString {
        let len = self.len();
        self.as_mut()
            .rust_mut()
            .view
            .context_press(x as f32, y as f32, len);
        self.as_mut().refresh();
        let (selected, selection) = {
            let v = &self.rust().view;
            (v.selected, v.selection)
        };
        let label = selected
            .and_then(|id| {
                self.with_doc(|s, d| {
                    s.doc(d)
                        .feature(id)
                        .map(|f| f.label().unwrap_or(&f.kind).to_string())
                })
            })
            .flatten()
            .unwrap_or_default();
        let info = json!({
            "feature": selected.map_or(-1, |FeatureId(i)| i64::from(i)),
            "label": label,
            "selection": selection.is_some(),
            "length": selection.map_or(0, |(a, b)| b - a),
        });
        QString::from(&info.to_string())
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn double_click(mut self: Pin<&mut Self>, x: f64, _y: f64) {
        let len = self.len();
        self.as_mut().rust_mut().view.zoom_at(x as f32, 3.0, len);
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn zoom_by(mut self: Pin<&mut Self>, x: f64, factor: f64) {
        let len = self.len();
        if factor > 0.0 {
            self.as_mut().rust_mut().view.zoom_at(x as f32, factor, len);
        }
        self.refresh();
    }

    fn zoom_centre(mut self: Pin<&mut Self>, factor: f64) {
        let len = self.len();
        {
            let mut r = self.as_mut().rust_mut();
            #[allow(clippy::cast_precision_loss)]
            let x = r
                .view
                .cursor
                .map_or(r.view.width / 2.0, |c| r.view.x_of(c as f64));
            let x = x.clamp(0.0, r.view.width);
            r.view.mode = Mode::Linear;
            r.view.zoom_at(x, factor, len);
        }
        self.as_mut().set_circular(false);
        self.refresh();
    }

    pub fn zoom_in(self: Pin<&mut Self>) {
        self.zoom_centre(2.0);
    }

    pub fn zoom_out(self: Pin<&mut Self>) {
        self.zoom_centre(0.5);
    }

    pub fn zoom_to_selection(mut self: Pin<&mut Self>) {
        let len = self.len();
        let Some((a, b)) = self.rust().view.selection else {
            self.notify("Nothing selected");
            return;
        };
        self.as_mut().rust_mut().view.zoom_to(a, b, len);
        self.as_mut().set_circular(false);
        self.refresh();
    }

    fn feature_bounds(&self, id: i32) -> Option<(u64, u64)> {
        let id = FeatureId(u32::try_from(id).ok()?);
        self.with_doc(|s, d| s.doc(d).feature(id).and_then(|f| f.location.bounds()))
            .flatten()
    }

    pub fn zoom_to_feature(mut self: Pin<&mut Self>, id: i32) {
        let len = self.len();
        if let Some((a, b)) = self.feature_bounds(id) {
            self.as_mut().rust_mut().view.zoom_to(a, b, len);
            self.as_mut().set_circular(false);
            self.refresh();
        }
    }

    pub fn select_feature_range(mut self: Pin<&mut Self>, id: i32) {
        if let Some((a, b)) = self.feature_bounds(id) {
            {
                let mut r = self.as_mut().rust_mut();
                r.view.selection = Some((a, b));
                r.view.cursor = Some(b);
            }
            self.refresh();
        }
    }

    pub fn feature_json(&self, id: i32) -> QString {
        let Ok(fid) = u32::try_from(id) else {
            return QString::default();
        };
        let out = self
            .with_doc(|s, d| {
                s.doc(d)
                    .feature(FeatureId(fid))
                    .map(|f| feature_to_json(id, f))
            })
            .flatten()
            .unwrap_or(Value::Null);
        QString::from(&out.to_string())
    }

    pub fn new_feature_json(&self) -> QString {
        let v = &self.rust().view;
        let (a, b) = v.selection.unwrap_or_else(|| {
            let c = v.cursor.unwrap_or(0);
            (c, c + 1)
        });
        let f = Feature {
            kind: "misc_feature".into(),
            location: Location::simple(a, b.max(a + 1), Strand::Forward),
            qualifiers: vec![Qualifier::new("label", "")],
        };
        QString::from(&feature_to_json(-1, &f).to_string())
    }

    pub fn save_feature(mut self: Pin<&mut Self>, id: i32, json: &QString) -> QString {
        let feature = match feature_from_json(&json.to_string()) {
            Ok(f) => f,
            Err(e) => return QString::from(&e),
        };
        let op = match u32::try_from(id) {
            Ok(i) => Op::FeatureUpdate { id: i, feature },
            Err(_) => Op::FeatureAdd { feature },
        };
        let result = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&mut r.session, r.doc) else {
                return QString::from("No document open");
            };
            session.apply(doc, op)
        };
        match result {
            Ok(out) => {
                if let Some(fid) = out.feature {
                    self.as_mut().rust_mut().view.selected = Some(fid);
                }
                self.as_mut().set_dirty(true);
                self.refresh();
                QString::default()
            }
            Err(e) => QString::from(&e.to_string()),
        }
    }

    pub fn delete_feature(mut self: Pin<&mut Self>, id: i32) {
        let Ok(i) = u32::try_from(id) else { return };
        if self.as_mut().edit(Op::FeatureRemove { id: i }).is_some() {
            self.as_mut().rust_mut().view.selected = None;
        }
        self.refresh();
    }

    pub fn selection_text(&self) -> QString {
        let Some((a, b)) = self.rust().view.selection else {
            return QString::default();
        };
        let text = self
            .with_doc(|s, d| String::from_utf8_lossy(&s.doc(d).read(a, b)).into_owned())
            .unwrap_or_default();
        QString::from(&text)
    }

    pub fn reverse_complement_selection(mut self: Pin<&mut Self>) {
        let Some((a, b)) = self.rust().view.selection else {
            self.notify("Select a range to reverse-complement");
            return;
        };
        let rc: String = self
            .with_doc(|s, d| s.doc(d).read(a, b))
            .unwrap_or_default()
            .iter()
            .rev()
            .map(|&c| char::from(complement(c)))
            .collect();
        let op = Op::SeqReplace {
            start: a,
            end: b,
            residues: rc,
        };
        if self.as_mut().edit(op).is_some() {
            self.as_mut().rust_mut().view.selection = Some((a, b));
        }
        self.refresh();
    }

    pub fn scroll_to(mut self: Pin<&mut Self>, fraction: f64) {
        let len = self.len();
        self.as_mut().rust_mut().view.scroll_to(fraction, len);
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn edge_scroll(mut self: Pin<&mut Self>, x: f64, y: f64) -> bool {
        let len = self.len();
        let moved = self
            .as_mut()
            .rust_mut()
            .view
            .edge_scroll(x as f32, y as f32, len);
        if moved {
            self.refresh();
        }
        moved
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn hover(&self, x: f64, y: f64) -> QString {
        let text = self
            .with_doc(|s, d| self.rust().view.hover_text(x as f32, y as f32, s.doc(d)))
            .flatten()
            .unwrap_or_default();
        QString::from(&text)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn pan_press(mut self: Pin<&mut Self>, x: f64) {
        self.as_mut().rust_mut().view.pan_press(x as f32);
    }

    pub fn selected_feature(&self) -> i32 {
        self.rust()
            .view
            .selected
            .and_then(|FeatureId(i)| i32::try_from(i).ok())
            .unwrap_or(-1)
    }

    pub fn feature_kinds(&self) -> QString {
        QString::from(&json!(FEATURE_KINDS).to_string())
    }
}

/// Feature keys offered in the editor; any other key can still be typed.
const FEATURE_KINDS: &[&str] = &[
    "gene",
    "CDS",
    "mRNA",
    "exon",
    "intron",
    "tRNA",
    "rRNA",
    "ncRNA",
    "misc_RNA",
    "5'UTR",
    "3'UTR",
    "promoter",
    "regulatory",
    "repeat_region",
    "mobile_element",
    "rep_origin",
    "primer_bind",
    "protein_bind",
    "misc_binding",
    "misc_feature",
    "variation",
    "misc_difference",
    "source",
];

fn complement(b: u8) -> u8 {
    match b.to_ascii_uppercase() {
        b'A' => b'T',
        b'T' | b'U' => b'A',
        b'C' => b'G',
        b'G' => b'C',
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

/// Editor JSON: `{"id", "kind", "location": "GenBank syntax, 1-based",
/// "qualifiers": [{"key", "value", "flag"}]}`. Internal `gff:` qualifiers
/// are passed through untouched so a round trip keeps them.
fn feature_to_json(id: i32, f: &Feature) -> Value {
    let qualifiers: Vec<Value> = f
        .qualifiers
        .iter()
        .map(|q| json!({ "key": q.key, "value": q.value.clone().unwrap_or_default(), "flag": q.value.is_none() }))
        .collect();
    json!({
        "id": id,
        "kind": f.kind,
        "location": format_location(&f.location),
        "length": f.location.len(),
        "qualifiers": qualifiers,
    })
}

fn feature_from_json(text: &str) -> Result<Feature, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("Bad editor data: {e}"))?;
    let kind = v["kind"].as_str().unwrap_or("").trim().to_string();
    if kind.is_empty() {
        return Err("The feature needs a type".into());
    }
    let loc_text = v["location"].as_str().unwrap_or("").trim();
    let location =
        parse_location(loc_text).ok_or_else(|| format!("Can't read location {loc_text:?}"))?;
    if location.segments.is_empty() {
        return Err("The location is empty".into());
    }
    let mut qualifiers = Vec::new();
    for q in v["qualifiers"].as_array().into_iter().flatten() {
        let key = q["key"].as_str().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }
        let value = q["value"].as_str().unwrap_or("");
        let flag = q["flag"].as_bool().unwrap_or(false) && value.is_empty();
        qualifiers.push(Qualifier {
            key: key.to_string(),
            value: (!flag).then(|| value.to_string()),
        });
    }
    Ok(Feature {
        kind,
        location,
        qualifiers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_json_roundtrip() {
        let f = Feature {
            kind: "CDS".into(),
            location: parse_location("complement(join(10..20,30..40))").unwrap(),
            qualifiers: vec![
                Qualifier::new("gene", "abc"),
                Qualifier {
                    key: "pseudo".into(),
                    value: None,
                },
            ],
        };
        let j = feature_to_json(3, &f).to_string();
        assert_eq!(feature_from_json(&j).unwrap(), f);
        assert!(feature_from_json(r#"{"kind":"gene","location":"oops"}"#).is_err());
    }
}
