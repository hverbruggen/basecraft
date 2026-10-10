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
    }
}

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::SystemTime;

use basecraft_docs::FeatureId;
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{Feature, Location, Op, Qualifier, Strand};
use basecraft_viewmodel::{Frame, GenomeView, Mode, Theme};
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString};

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
        self.as_mut().set_frame(QByteArray::from(bytes.as_slice()));
        self.as_mut().set_status(QString::from(&status));
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
                    r.view.mode = if circular && len < 2_000_000 {
                        Mode::Circular
                    } else {
                        Mode::Linear
                    };
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
}
