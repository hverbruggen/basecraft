//! The Rust object QML talks to: one genome editor.
//!
//! QML forwards raw input here. The view model turns it into viewport
//! changes, selections and operations; every edit goes through the ops
//! session, so it's logged and undoable like any other operation.

/// Stop a GUI read on an invalid document and report through the existing
/// status properties. Queuing also supports immutable QML getters.
macro_rules! checked_document {
    (sync $controller:ident, $result:expr) => {
        match $result {
            Ok(document) => document,
            Err(error) => {
                let message = error.to_string();
                $controller
                    .as_mut()
                    .set_message_kind(cxx_qt_lib::QString::from("error"));
                $controller
                    .as_mut()
                    .set_message(cxx_qt_lib::QString::from(&message));
                return Default::default();
            }
        }
    };
    ($thread:expr, $result:expr) => {
        match $result {
            Ok(document) => document,
            Err(error) => {
                let message = error.to_string();
                let _ = $thread.queue(move |mut controller| {
                    controller
                        .as_mut()
                        .set_message_kind(cxx_qt_lib::QString::from("error"));
                    controller
                        .as_mut()
                        .set_message(cxx_qt_lib::QString::from(&message));
                });
                return Default::default();
            }
        }
    };
}
pub(crate) use checked_document;

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for GenomeController {}
    unsafe extern "C++" {
        include!("instanceview.h");
        fn configure_canvas_graphics();
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, overview_content)]
        #[qproperty(QString, overview_matches)]
        #[qproperty(QByteArray, frame)]
        #[qproperty(QString, status)]
        #[qproperty(QString, title)]
        #[qproperty(QString, message)]
        /// "ok", "error", or "" for a neutral hint.
        #[qproperty(QString, message_kind)]
        #[qproperty(QString, ok_color)]
        #[qproperty(QString, error_color)]
        #[qproperty(QString, background)]
        #[qproperty(QString, foreground)]
        #[qproperty(QString, accent)]
        #[qproperty(bool, busy)]
        #[qproperty(bool, circular)]
        #[qproperty(QString, mode)]
        #[qproperty(QString, project_dir)]
        #[qproperty(QString, mono_font)]
        #[qproperty(QString, file_path)]
        #[qproperty(bool, dirty)]
        #[qproperty(QString, zoom_text)]
        #[qproperty(f64, scroll_pos)]
        #[qproperty(f64, overview_pos)]
        #[qproperty(f64, overview_size)]
        #[qproperty(f64, scroll_size)]
        type GenomeController = super::ControllerRust;

        #[qinvokable]
        fn find_query(self: Pin<&mut GenomeController>, query: &QString, reverse: bool);
        #[qinvokable]
        fn find_poll(self: Pin<&mut GenomeController>, direction: i32) -> QString;

        #[qinvokable]
        fn document_record(self: &GenomeController) -> QString;
        #[qinvokable]
        fn record_revision(self: &GenomeController) -> QString;
        #[qinvokable]
        fn accessible_page(self: &GenomeController, page: i32) -> QString;
        #[qinvokable]
        fn open_file(self: Pin<&mut GenomeController>, path: &QString) -> bool;
        #[qinvokable]
        fn new_sequence_error(self: &GenomeController, name: &QString, text: &QString) -> QString;
        #[qinvokable]
        fn create_sequence(
            self: Pin<&mut GenomeController>,
            name: &QString,
            text: &QString,
            root: &QString,
        ) -> QString;
        #[qinvokable]
        fn local_file_url(self: &GenomeController, path: &QString) -> QUrl;
        #[qinvokable]
        fn local_file_path(self: &GenomeController, url: &QUrl) -> QString;
        #[qinvokable]
        fn delimitation_engines(self: &GenomeController) -> QString;
        #[qinvokable]
        fn parent_folder_url(self: &GenomeController, path: &QString) -> QUrl;
        #[qinvokable]
        fn export_svg(self: Pin<&mut GenomeController>, path: &QString);
        #[qinvokable]
        fn export_figure(
            self: Pin<&mut GenomeController>,
            path: &QString,
            scope: i32,
            publication: bool,
        );
        #[qinvokable]
        fn export_figure_png(
            self: Pin<&mut GenomeController>,
            path: &QString,
            scope: i32,
            publication: bool,
            width: f64,
        );
        /// Set `project_dir` to the project a file belongs to, without
        /// opening it.
        #[qinvokable]
        fn use_project_of(self: Pin<&mut GenomeController>, path: &QString);
        #[qinvokable]
        fn resize(self: Pin<&mut GenomeController>, width: f64, height: f64);
        #[qinvokable]
        fn wheel(
            self: Pin<&mut GenomeController>,
            x: f64,
            y: f64,
            dy: f64,
            dx: f64,
            modifiers: i32,
        );
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
        /// Switch view: "wrapped", "linear" or "circular".
        #[qinvokable]
        fn switch_view(self: Pin<&mut GenomeController>, mode: &QString);
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
        /// Save to a new file and carry on editing that one.
        #[qinvokable]
        fn save_as(self: Pin<&mut GenomeController>, path: &QString) -> bool;
        /// Reload the Omarchy theme if it changed.
        #[qinvokable]
        fn check_theme(self: Pin<&mut GenomeController>);
        /// Write a pending typing burst to the log.
        #[qinvokable]
        fn flush(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn memory_report(self: &GenomeController) -> QString;
        /// `path` relative to the open project folder, or to the file's own
        /// project when it lies outside it.
        #[qinvokable]
        fn relative_path(self: &GenomeController, path: &QString) -> QString;
        /// Write a readable account of this run's activity in the projects
        /// of `files` (one path per line) to a text file; returns its path.
        #[qinvokable]
        fn activity_log(self: &GenomeController, files: &QString, utc_offset_min: i32) -> QString;
        /// "alignment" or "sequence": which editor a file opens in.
        #[qinvokable]
        fn file_kind(self: &GenomeController, path: &QString) -> QString;

        /// Right-click at a point. Returns JSON describing what the context
        /// menu should offer: `{"feature": id or -1, "label", "selection": bool,
        /// "length"}`.
        #[qinvokable]
        fn context_press(self: Pin<&mut GenomeController>, x: f64, y: f64) -> QString;
        #[qinvokable]
        fn double_click(self: Pin<&mut GenomeController>, x: f64, y: f64);
        /// Zoom by `factor` around `x` (pinch gestures).
        #[qinvokable]
        fn zoom_by(self: Pin<&mut GenomeController>, x: f64, y: f64, factor: f64);
        #[qinvokable]
        fn zoom_in(self: Pin<&mut GenomeController>);
        #[qinvokable]
        fn set_zoom_percent(self: Pin<&mut GenomeController>, percent: f64);
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
        /// Run the ORF finder plugin and add the open reading frames of at
        /// least `min_codons` codons it finds, in the whole sequence or only
        /// the selection, as one undoable step. Returns a summary for the
        /// status bar.
        #[qinvokable]
        fn find_orfs(
            self: Pin<&mut GenomeController>,
            min_codons: i32,
            in_selection: bool,
        ) -> QString;
        /// Predict protein-coding genes with the Pyrodigal plugin, using
        /// genetic code `code`, and add them as CDS features in one
        /// undoable step. Returns a summary for the status bar.
        #[qinvokable]
        fn predict_genes(
            self: Pin<&mut GenomeController>,
            code: i32,
            in_selection: bool,
        ) -> QString;
        /// The genetic code of the open file: the first CDS's
        /// `transl_table`, else 11.
        #[qinvokable]
        fn genetic_code(self: &GenomeController) -> i32;
        /// The selected residues, for the clipboard.
        #[qinvokable]
        fn selection_text(self: &GenomeController) -> QString;
        #[qinvokable]
        fn has_selection(self: &GenomeController) -> bool;
        /// Write the selection, with its annotations, to a new GenBank file
        /// next to the open one. Returns acceptance; extractionFinished carries the path.
        #[qinvokable]
        fn extract_selection(self: Pin<&mut GenomeController>) -> bool;
        /// The same for the span of an annotation.
        #[qinvokable]
        fn extract_feature(self: Pin<&mut GenomeController>, id: i32) -> bool;
        #[qsignal]
        #[cxx_name = "extractionFinished"]
        fn extraction_finished(self: Pin<&mut GenomeController>, path: QString);
        /// An annotation's own sequence: its parts joined in biological
        /// order, reverse-complemented on the reverse strand.
        #[qinvokable]
        fn feature_sequence(self: &GenomeController, id: i32) -> QString;
        #[qinvokable]
        fn blast_queries(self: &GenomeController, source: &QString, feature: i32) -> QString;
        /// Scroll the linear view to a fraction of the sequence.
        #[qinvokable]
        fn overview_scroll_to(self: Pin<&mut GenomeController>, fraction: f64);
        #[qinvokable]
        fn scroll_to(self: Pin<&mut GenomeController>, fraction: f64);
        /// Called repeatedly while a selection drag is near or past an edge;
        /// returns whether the view moved.
        #[qinvokable]
        fn edge_scroll(self: Pin<&mut GenomeController>, x: f64, y: f64) -> bool;
        /// Rewrite a GenBank location with a direction (`forward`, `reverse`,
        /// `none`, or `mixed` to keep it) and 5'/3' completeness. Returns an
        /// empty string if the location can't be read.
        #[qinvokable]
        fn adjust_location(
            self: &GenomeController,
            text: &QString,
            strand: &QString,
            partial5: bool,
            partial3: bool,
        ) -> QString;
        /// What a GenBank location says, as JSON: `valid`, `strand`,
        /// `partial5`, `partial3`, `length`, `parts`.
        #[qinvokable]
        fn location_info(self: &GenomeController, text: &QString) -> QString;
        /// Feature description under the pointer, or an empty string.
        #[qinvokable]
        fn hover(self: &GenomeController, x: f64, y: f64) -> QString;
        /// Start panning (middle button).
        #[qinvokable]
        fn pan_press(self: Pin<&mut GenomeController>, x: f64, y: f64);
        /// Show or hide the GC content plot.
        #[qinvokable]
        fn set_show_gc(self: Pin<&mut GenomeController>, show: bool);
        /// Select the next (or previous) annotation; false if there's none.
        #[qinvokable]
        fn step_feature(self: Pin<&mut GenomeController>, forward: bool) -> bool;
        /// The Basecraft version.
        #[qinvokable]
        fn version(self: &GenomeController) -> QString;
        #[qinvokable]
        fn available_cpu_threads(self: &GenomeController) -> i32;
        #[qinvokable]
        fn configure_cpu_threads(self: &GenomeController, threads: i32);
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
use std::time::{Instant, SystemTime};

use basecraft_docs::FeatureId;
use basecraft_io::genbank::{format_location, parse_location};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{Feature, Location, Op, Qualifier, Strand};
use basecraft_viewmodel::{Frame, GenomeView, Mode, Theme};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QString, QUrl};
use serde_json::{Value, json};

// Qt key codes and modifiers used below.
/// One zoom step from the keyboard or the ribbon.
pub(crate) const ZOOM_STEP: f64 = 1.25;
/// Zoom per wheel unit (120 to a notch): about 13 % a notch.
pub(crate) const WHEEL_ZOOM: f64 = 1.001;

/// How long something took, for the end of a status message: " in 40 ms",
/// " in 2.3 s", " in 4 min 5 s".
pub(crate) fn took(started: Instant) -> String {
    let d = started.elapsed();
    let ms = d.as_millis();
    if ms < 1000 {
        format!(" in {ms} ms")
    } else if ms < 60_000 {
        format!(" in {:.1} s", d.as_secs_f64())
    } else {
        let s = d.as_secs();
        format!(" in {} min {} s", s / 60, s % 60)
    }
}

/// One readable line for a status bar from a plugin or tool error. The plugin
/// transport prefix and generic Python exception names are dropped; for a
/// traceback only its final exception line is kept. The full text belongs in
/// the run record or a details view, never in the bar.
pub(crate) fn error_summary(text: &str) -> String {
    const LIMIT: usize = 200;
    let mut line = text.trim();
    if let Some(rest) = line.strip_prefix("plugin error ") {
        line = rest.split_once(": ").map_or(rest, |(_, m)| m);
    }
    if line.contains("Traceback (most recent call last):") {
        line = line
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('^') && !l.starts_with('~'))
            .unwrap_or(line);
    } else {
        line = line
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("");
    }
    for generic in ["RuntimeError: ", "ValueError: ", "Exception: "] {
        if let Some(rest) = line.strip_prefix(generic) {
            line = rest;
        }
    }
    if line.chars().count() > LIMIT {
        let cut: String = line.chars().take(LIMIT - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        line.to_owned()
    }
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |f| f.to_string_lossy().into_owned())
}

pub(crate) const KEY_ESCAPE: i32 = 0x0100_0000;
pub(crate) const KEY_BACKSPACE: i32 = 0x0100_0003;
pub(crate) const KEY_DELETE: i32 = 0x0100_0007;
pub(crate) const KEY_HOME: i32 = 0x0100_0010;
pub(crate) const KEY_END: i32 = 0x0100_0011;
pub(crate) const KEY_LEFT: i32 = 0x0100_0012;
pub(crate) const KEY_RIGHT: i32 = 0x0100_0014;
pub(crate) const KEY_UP: i32 = 0x0100_0013;
pub(crate) const KEY_DOWN: i32 = 0x0100_0015;
pub(crate) const KEY_PAGE_UP: i32 = 0x0100_0016;
pub(crate) const KEY_PAGE_DOWN: i32 = 0x0100_0017;
pub(crate) const KEY_F2: i32 = 0x0100_0031;
pub(crate) const KEY_PLUS: i32 = 0x2b;
pub(crate) const KEY_MINUS: i32 = 0x2d;
pub(crate) const KEY_EQUAL: i32 = 0x3d;
pub(crate) const KEY_0: i32 = 0x30;
pub(crate) const KEY_A: i32 = 0x41;
pub(crate) const KEY_Y: i32 = 0x59;
pub(crate) const KEY_Z: i32 = 0x5a;
pub(crate) const SHIFT: i32 = 0x0200_0000;
pub(crate) const CTRL: i32 = 0x0400_0000;
pub(crate) const ALT: i32 = 0x0800_0000;

pub struct ControllerRust {
    find: basecraft_viewmodel::find::Session,
    find_status_revision: u64,
    overview_content: QString,
    overview_matches: QString,
    overview_match_cache: basecraft_viewmodel::overview::Cache,
    overview_cache: basecraft_viewmodel::overview::Cache,
    frame: QByteArray,
    status: QString,
    title: QString,
    message: QString,
    message_kind: QString,
    ok_color: QString,
    error_color: QString,
    background: QString,
    foreground: QString,
    accent: QString,
    busy: bool,
    circular: bool,
    mode: QString,
    project_dir: QString,
    mono_font: QString,
    file_path: QString,
    dirty: bool,
    zoom_text: QString,
    scroll_pos: f64,
    overview_pos: f64,
    overview_size: f64,
    scroll_size: f64,

    session: Option<Session>,
    doc: Option<DocId>,
    view: GenomeView,
    theme: Theme,
    theme_stamp: ThemeStamp,
    scratch: Frame,
}

impl Default for ControllerRust {
    fn default() -> Self {
        let mut c = Self {
            find: basecraft_viewmodel::find::Session::default(),
            find_status_revision: u64::MAX,
            overview_content: QString::from("{\"shapes\":[]}"),
            overview_matches: QString::from("[]"),
            overview_match_cache: basecraft_viewmodel::overview::Cache::default(),
            overview_cache: basecraft_viewmodel::overview::Cache::default(),
            frame: QByteArray::default(),
            status: QString::default(),
            title: QString::from("Basecraft"),
            message: QString::default(),
            message_kind: QString::default(),
            ok_color: QString::default(),
            error_color: QString::default(),
            background: QString::default(),
            foreground: QString::default(),
            accent: QString::default(),
            busy: false,
            circular: false,
            mode: QString::from("wrapped"),
            project_dir: QString::default(),
            mono_font: QString::from(&monospace_font()),
            file_path: QString::default(),
            dirty: false,
            zoom_text: QString::default(),
            scroll_pos: 0.0,
            overview_pos: 0.0,
            overview_size: 1.0,
            scroll_size: 1.0,
            session: None,
            doc: None,
            view: GenomeView::new(800.0, 600.0),
            theme: Theme::default(),
            theme_stamp: ThemeStamp::default(),
            scratch: Frame::new(),
        };
        started();
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

/// The desktop's monospace font: Omarchy's current font when available,
/// else whatever fontconfig calls monospace.
pub(crate) fn monospace_font() -> String {
    // Never wait for a desktop helper from a controller/theme callback. A
    // failed refresh leaves the last usable font in place; at most one helper
    // is active process-wide.
    struct Cache {
        checked: std::time::Instant,
        font: String,
        pending: bool,
    }
    static CACHE: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<Cache>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        std::sync::Arc::new(std::sync::Mutex::new(Cache {
            checked: std::time::Instant::now(),
            font: "monospace".into(),
            pending: false,
        }))
    });
    let mut current = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !current.pending
        && (current.font == "monospace"
            || current.checked.elapsed() >= std::time::Duration::from_secs(2))
    {
        current.pending = true;
        current.checked = std::time::Instant::now();
        let worker_cache = cache.clone();
        if std::thread::Builder::new()
            .name("desktop-font".into())
            .spawn(move || {
                let font = read_monospace_font();
                let mut current = worker_cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                current.font = font;
                current.pending = false;
            })
            .is_err()
        {
            current.pending = false;
        }
    }
    current.font.clone()
}

fn read_monospace_font() -> String {
    std::process::Command::new("omarchy")
        .args(["font", "current"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "monospace".to_string())
}

pub(crate) fn omarchy_theme_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".local/state/omarchy/current/theme/colors.toml");
    p.exists().then_some(p)
}

/// Existing appearance invalidation also watches application palette preferences.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ThemeStamp {
    desktop: Option<SystemTime>,
    preferences: Option<SystemTime>,
}

fn theme_stamp_at(desktop: Option<&Path>, preferences: Option<&Path>) -> ThemeStamp {
    let modified = |path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    ThemeStamp {
        desktop: desktop.and_then(modified),
        preferences: preferences.and_then(modified),
    }
}

pub(crate) fn theme_stamp() -> ThemeStamp {
    let preferences = basecraft_platform::paths::AppPaths::discover()
        .ok()
        .map(|p| p.preferences());
    theme_stamp_at(omarchy_theme_path().as_deref(), preferences.as_deref())
}

fn color_blind_friendly_at(preferences: &Path) -> bool {
    basecraft_platform::settings::SettingsStore::new(preferences.to_owned())
        .load()
        .is_ok_and(|p| {
            p.ui.data_color_scheme
                == basecraft_platform::settings::DataColorScheme::ColorBlindFriendly
        })
}

fn color_blind_friendly() -> bool {
    basecraft_platform::paths::AppPaths::discover()
        .is_ok_and(|p| color_blind_friendly_at(&p.preferences()))
}

fn load_theme_at(desktop: Option<&Path>, preferences: &Path) -> (Theme, ThemeStamp) {
    // Capture before reading, so a concurrent update is seen on the next poll.
    let stamp = theme_stamp_at(desktop, Some(preferences));
    let theme = desktop
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| Theme::from_omarchy(&text))
        .unwrap_or_default();
    (
        theme.with_color_blind_friendly(color_blind_friendly_at(preferences)),
        stamp,
    )
}

pub(crate) fn load_theme() -> Option<(Theme, ThemeStamp)> {
    // Absent desktop configuration still gets the optional light data palette.
    // Failed preferences retain the existing usable default; AppSettings owns
    // startup/update error presentation and never overwrites a damaged file.
    let paths = basecraft_platform::paths::AppPaths::discover().ok()?;
    Some(load_theme_at(
        omarchy_theme_path().as_deref(),
        &paths.preferences(),
    ))
}

pub(crate) fn publication_theme() -> Theme {
    Theme::publication().with_color_blind_friendly(color_blind_friendly())
}

pub(crate) fn css(c: [f32; 4]) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// When this run of the app started (RFC 3339 UTC, as in log entries).
fn started() -> &'static str {
    static STARTED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    STARTED.get_or_init(basecraft_platform::now_rfc3339)
}

/// A bundled plugin's directory: under `$BASECRAFT_PLUGINS` if set, else
/// the source tree's `plugins/` (development builds).
pub(crate) fn plugin_dir(name: &str) -> PathBuf {
    basecraft_plugin_host::installation::plugins().join(name)
}

/// The project a file belongs to: the nearest ancestor with a `.basecraft`
/// directory, else the file's own directory.
pub(crate) fn project_root(file: &Path) -> PathBuf {
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
    pub fn document_record(&self) -> QString {
        crate::provenance_bridge::session_report(self.rust().session.as_ref(), self.rust().doc)
    }
    pub fn record_revision(&self) -> QString {
        crate::provenance_bridge::session_revision(self.rust().session.as_ref(), self.rust().doc)
    }

    pub fn accessible_page(&self, page: i32) -> QString {
        let access_thread = self.qt_thread();
        let result = match (&self.rust().session, self.rust().doc) {
            (Some(s), Some(d)) => {
                let sequence = crate::bridge::checked_document!(access_thread, s.doc(d));
                Some(basecraft_viewmodel::accessible::sequence(sequence, page))
            }
            _ => None,
        };
        let value = result.map_or_else(
            || serde_json::json!({"page": 0, "pages": 1, "text": "No document loaded"}),
            |p| serde_json::json!({"page": p.page, "pages": p.pages, "text": p.text}),
        );
        QString::from(&value.to_string())
    }

    fn local_file_url(&self, path: &QString) -> QUrl {
        QUrl::from_local_file(path)
    }

    fn local_file_path(&self, url: &QUrl) -> QString {
        url.to_local_file().unwrap_or_default()
    }

    /// Availability is advisory; job execution always checks the engine again.
    /// The first request starts a background discovery and returns no cached
    /// diagnostics. Later requests reuse its report without blocking Qt.
    fn delimitation_engines(&self) -> QString {
        struct Report {
            checked: Instant,
            value: Option<String>,
            pending: bool,
        }
        static REPORT: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<Report>>> =
            std::sync::OnceLock::new();
        let report = REPORT.get_or_init(|| {
            std::sync::Arc::new(std::sync::Mutex::new(Report {
                checked: Instant::now(),
                value: None,
                pending: false,
            }))
        });
        let mut current = report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !current.pending
            && (current.value.is_none()
                || current.checked.elapsed() >= std::time::Duration::from_secs(2))
        {
            current.pending = true;
            current.checked = Instant::now();
            let result = report.clone();
            let spawned = std::thread::Builder::new()
                .name("engine-discovery".into())
                .spawn(move || {
                    let value = (|| -> Result<String, String> {
                        let manifest =
                            basecraft_plugin_host::Manifest::load(&plugin_dir("delimitation"))
                                .map_err(|e| e.to_string())?;
                        let mut plugin = basecraft_plugin_host::PluginProcess::spawn(&manifest)
                            .map_err(|e| e.to_string())?;
                        plugin
                            .invoke("engines", json!({}), None)
                            .map_err(|e| e.to_string())?
                            .message
                            .ok_or_else(|| "No engine report".into())
                    })()
                    .unwrap_or_else(|error| {
                        json!({"asap":error,"ptp":error,"mptp":error}).to_string()
                    });
                    let mut current = result
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    current.value = Some(value);
                    current.pending = false;
                });
            if spawned.is_err() {
                current.pending = false;
            }
        }
        QString::from(current.value.as_deref().unwrap_or("{}"))
    }

    fn parent_folder_url(&self, path: &QString) -> QUrl {
        let path = path.to_string();
        let parent = std::path::Path::new(&path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        QUrl::from_local_file(&QString::from(parent.to_string_lossy().as_ref()))
    }

    fn len(&self) -> basecraft_ops::Result<u64> {
        let r = self.rust();
        match (&r.session, r.doc) {
            (Some(s), Some(d)) => Ok(s.doc(d)?.len()),
            _ => Ok(0),
        }
    }

    /// Rebuild the frame and status line.
    fn refresh(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let (bytes, status) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.doc(doc));
            r.find
                .invalidate(&crate::find_bridge::document_key(session, doc));
            r.view.build(d, &r.theme, &mut r.scratch);
            r.view.draw_find(d.len(), &r.find, &r.theme, &mut r.scratch);
            (r.scratch.to_bytes(), r.view.status(d))
        };
        let (zoom, (pos, size)) = {
            let len = crate::bridge::checked_document!(sync self, self.len());
            let v = &self.rust().view;
            let zoom = v.zoom_text(len);
            let scroll = if v.mode == Mode::Linear {
                v.scroll_fraction(len)
            } else {
                (0.0, 1.0)
            };
            (zoom, scroll)
        };
        let (overview, matches) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                return;
            };
            let d = crate::bridge::checked_document!(access_thread, session.doc(doc));
            let key = format!("{}:{}:{:?}", session.id(), d.version(), r.theme);
            let content = r.overview_cache.get(key.clone(), || {
                basecraft_viewmodel::overview::sequence(d, &r.theme)
            });
            let matches =
                r.overview_match_cache
                    .get_json(format!("{key}:{}", r.find.revision()), || {
                        crate::find_bridge::overview_marks(&r.find, |hit| {
                            basecraft_viewmodel::genome::GenomeView::overview_match(
                                hit.start,
                                hit.end,
                                d.len(),
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
        let (overview_pos, overview_size) = self
            .rust()
            .view
            .overview_fraction(crate::bridge::checked_document!(sync self, self.len()));
        self.as_mut().set_overview_pos(overview_pos);
        self.as_mut().set_overview_size(overview_size);
        self.as_mut().set_scroll_size(size);
        self.as_mut().set_scroll_pos(pos);
    }

    /// A neutral hint. Messages stay brief: say what happened, nothing more.
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
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
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
                publication_theme()
            } else {
                r.theme.clone()
            };
            let mut view = r.view.clone();
            let sequence = session.doc(doc)?;
            if scope == 2 {
                view.fit_doc(sequence);
            }
            if scope == 1 {
                let (start, end) = view.selection.unwrap_or((0, sequence.len()));
                view.mode = basecraft_viewmodel::genome::Mode::Linear;
                #[allow(clippy::cast_precision_loss)]
                {
                    view.start = start as f64;
                    view.bp_per_px = (end - start).max(1) as f64 / f64::from(view.width);
                }
                view.auto_fit = false;
            }
            if scope != 0 {
                view.selection = None;
                view.selection_parts = None;
                view.selected = None;
                view.cursor = None;
            }
            let mut frame = Frame::new();
            view.build(sequence, &theme, &mut frame);
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

    pub fn new_sequence_error(&self, name: &QString, text: &QString) -> QString {
        Session::validate_new_sequence(&name.to_string(), &text.to_string())
            .err()
            .map_or_else(QString::default, |error| QString::from(&error.to_string()))
    }

    /// Create in the ordinary recoverable session, without a source-file surrogate.
    pub fn create_sequence(
        mut self: Pin<&mut Self>,
        name: &QString,
        text: &QString,
        root: &QString,
    ) -> QString {
        if self.rust().session.is_some() || self.rust().busy {
            return QString::from("This controller already owns a document");
        }
        let result = (|| {
            let root = if root.to_string().is_empty() {
                std::env::current_dir().map_err(|e| e.to_string())?
            } else {
                PathBuf::from(root.to_string())
            };
            let mut session = Session::recoverable(&root).map_err(|e| e.to_string())?;
            let doc = session
                .create_sequence(&name.to_string(), &text.to_string())
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((session, doc))
        })();
        let (session, doc) = match result {
            Ok(created) => created,
            Err(error) => return QString::from(&error),
        };
        let sequence = match session.doc(doc) {
            Ok(sequence) => sequence,
            Err(error) => return QString::from(error.to_string()),
        };
        {
            let mut r = self.as_mut().rust_mut();
            r.view.mode = Mode::Wrapped;
            r.view.fit_doc(sequence);
            r.session = Some(session);
            r.doc = Some(doc);
        }
        self.as_mut().set_dirty(true);
        self.as_mut().sync_mode();
        self.as_mut().refresh();
        QString::default()
    }

    pub fn open_file(mut self: Pin<&mut Self>, path: &QString) -> bool {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        if self.rust().busy {
            self.fail("Wait for the running operation before replacing this document");
            return false;
        }
        let path = PathBuf::from(path.to_string());
        let root = project_root(&path);
        let mut session = match Session::recoverable(&root) {
            Ok(session) => session,
            Err(error) => {
                self.fail(&error.to_string());
                return false;
            }
        };
        match session.open(&path) {
            Ok(doc) => {
                let (name, len, circular) = {
                    let d = crate::bridge::checked_document!(sync self, session.doc(doc));
                    (d.name.clone(), d.len(), d.circular)
                };
                {
                    let mut r = self.as_mut().rust_mut();
                    let r = &mut *r;
                    // Wrapped, fitted to the window, circular molecules too;
                    // the other views are a switch away.
                    let _ = (circular, len);
                    r.view.mode = Mode::Wrapped;
                    r.view.fit_doc(crate::bridge::checked_document!(sync self,
                        session.doc(doc)
                    ));
                    r.session = Some(session);
                    r.doc = Some(doc);
                }
                let file = path
                    .file_name()
                    .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
                self.as_mut()
                    .set_title(QString::from(&format!("{file} — {name} — Basecraft")));
                self.as_mut().sync_mode();
                let abs = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                let root_abs = std::fs::canonicalize(&root).unwrap_or(root);
                self.as_mut()
                    .set_file_path(QString::from(&abs.to_string_lossy().into_owned()));
                self.as_mut()
                    .set_project_dir(QString::from(&root_abs.to_string_lossy().into_owned()));
                self.as_mut().set_dirty(false);
                self.as_mut().notify("");
                self.refresh();
                true
            }
            Err(e) => {
                self.fail(&format!("Can't open {}: {e}", path.display()));
                false
            }
        }
    }

    pub fn use_project_of(mut self: Pin<&mut Self>, path: &QString) {
        let root = project_root(Path::new(&path.to_string()));
        let abs = std::fs::canonicalize(&root).unwrap_or(root);
        self.as_mut()
            .set_project_dir(QString::from(&abs.to_string_lossy().into_owned()));
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn resize(mut self: Pin<&mut Self>, width: f64, height: f64) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let len = crate::bridge::checked_document!(sync self, self.len());
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            r.view.resize(width as f32, height as f32, len);
            // Until the user zooms or scrolls, keep the whole sequence fitted:
            // the pane often settles into its final size after opening.
            if r.view.auto_fit
                && let (Some(s), Some(d)) = (&r.session, r.doc)
            {
                r.view
                    .fit_doc(crate::bridge::checked_document!(access_thread, s.doc(d)));
            }
        }
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn wheel(mut self: Pin<&mut Self>, x: f64, y: f64, dy: f64, dx: f64, modifiers: i32) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        {
            let mut r = self.as_mut().rust_mut();
            if r.view.mode == Mode::Circular {
                return;
            }
            if r.view.mode == Mode::Wrapped {
                // The wheel scrolls lines; Ctrl+wheel zooms.
                if modifiers & CTRL != 0 {
                    r.view
                        .zoom_at_point(x as f32, y as f32, WHEEL_ZOOM.powf(dy), len);
                } else {
                    r.view.scroll_by((-dy * 0.5) as f32);
                }
            } else if modifiers & SHIFT != 0 || dx.abs() > dy.abs() {
                let d = if dx.abs() > dy.abs() { dx } else { dy };
                r.view.pan((d / 2.0) as f32, len);
            } else {
                r.view.zoom_at(x as f32, WHEEL_ZOOM.powf(dy), len);
            }
        }
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn press(mut self: Pin<&mut Self>, x: f64, y: f64, modifiers: i32) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let len = crate::bridge::checked_document!(sync self, self.len());
        self.as_mut()
            .rust_mut()
            .view
            .press(x as f32, y as f32, modifiers & SHIFT != 0, len);
        // A clicked feature's span takes the feature's direction.
        if let Some(id) = self.rust().view.selected {
            let reverse = self
                .with_doc(|s, d| {
                    crate::bridge::checked_document!(access_thread, s.doc(d))
                        .feature(id)
                        .map(|f| f.location.strand() == Strand::Reverse)
                })
                .flatten()
                .unwrap_or(false);
            self.as_mut().rust_mut().view.selection_reverse = reverse;
            self.as_mut().adopt_parts();
        }
        self.refresh();
    }

    /// A selected multi-part annotation selects its parts.
    fn adopt_parts(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        if let (Some(s), Some(d)) = (&r.session, r.doc) {
            r.view
                .adopt_feature_parts(crate::bridge::checked_document!(access_thread, s.doc(d)));
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn drag(mut self: Pin<&mut Self>, x: f64, y: f64) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        self.as_mut()
            .rust_mut()
            .view
            .drag_to(x as f32, y as f32, len);
        self.refresh();
    }

    pub fn release(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().view.release();
    }

    /// Mirror the view mode into the `mode` and `circular` properties.
    fn sync_mode(mut self: Pin<&mut Self>) {
        let mode = self.rust().view.mode;
        let name = match mode {
            Mode::Wrapped => "wrapped",
            Mode::Linear => "linear",
            Mode::Circular => "circular",
        };
        self.as_mut().set_mode(QString::from(name));
        self.as_mut().set_circular(mode == Mode::Circular);
    }

    /// Cycle wrapped → linear → circular.
    pub fn toggle_mode(mut self: Pin<&mut Self>) {
        let next = match self.rust().view.mode {
            Mode::Wrapped => "linear",
            Mode::Linear => "circular",
            Mode::Circular => "wrapped",
        };
        self.as_mut().switch_view(&QString::from(next));
    }

    pub fn switch_view(mut self: Pin<&mut Self>, mode: &QString) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        {
            let mut r = self.as_mut().rust_mut();
            let (cursor, selection) = (r.view.cursor, r.view.selection);
            let new = match mode.to_string().as_str() {
                "linear" => Mode::Linear,
                "circular" => Mode::Circular,
                _ => Mode::Wrapped,
            };
            if new == r.view.mode {
                return;
            }
            r.view.mode = new;
            // Keep what the user was looking at in view.
            let focus = selection.map(|(a, _)| a).or(cursor);
            match new {
                Mode::Wrapped => {
                    let w = r.view.width;
                    let h = r.view.height;
                    r.view.resize(w, h, len);
                    if let Some(p) = focus {
                        r.view.reveal(p, len);
                    }
                }
                Mode::Linear => {
                    if let Some(p) = focus {
                        r.view.reveal(p, len);
                    }
                }
                Mode::Circular => {}
            }
        }
        self.as_mut().sync_mode();
        self.refresh();
    }

    pub fn fit(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let (Some(s), Some(d)) = (&r.session, r.doc) {
                r.view
                    .fit_doc(crate::bridge::checked_document!(access_thread, s.doc(d)));
            }
        }
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn show_range(mut self: Pin<&mut Self>, start: f64, end: f64) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        {
            let mut r = self.as_mut().rust_mut();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            r.view
                .zoom_to(start.max(0.0) as u64, end.max(0.0) as u64, len);
        }
        self.as_mut().sync_mode();
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
                // A new edit makes the last message stale.
                self.as_mut().notify("");
                Some(out)
            }
            Err(e) => {
                self.fail(&e.to_string());
                None
            }
        }
    }

    pub fn key(mut self: Pin<&mut Self>, key: i32, text: &QString, modifiers: i32) -> bool {
        let len = crate::bridge::checked_document!(sync self, self.len());
        let ctrl = modifiers & CTRL != 0;
        let shift = modifiers & SHIFT != 0;
        let (cursor, selection, selected) = {
            let v = &self.rust().view;
            (v.cursor, v.selection, v.selected)
        };
        let handled = match key {
            KEY_A if modifiers == CTRL => {
                self.as_mut().rust_mut().view.select_all(len);
                true
            }
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
            KEY_0 if ctrl => {
                self.as_mut().fit();
                true
            }
            // F2 cycled layouts; only the wrapped layout is offered for now.
            KEY_F2 if false => {
                self.as_mut().toggle_mode();
                true
            }
            KEY_PLUS | KEY_EQUAL | KEY_MINUS if ctrl => {
                let factor = if key == KEY_MINUS {
                    1.0 / ZOOM_STEP
                } else {
                    ZOOM_STEP
                };
                let _ = cursor;
                self.as_mut().rust_mut().view.zoom_step(factor, len);
                true
            }
            KEY_UP | KEY_DOWN if self.rust().view.mode == Mode::Wrapped => {
                // A line up or down in the wrapped view.
                let line = i64::try_from(self.rust().view.line_bp()).unwrap_or(1);
                let delta = if key == KEY_UP { -line } else { line };
                self.as_mut().rust_mut().view.move_cursor(delta, shift, len);
                true
            }
            KEY_PAGE_UP | KEY_PAGE_DOWN if self.rust().view.mode == Mode::Wrapped => {
                let mut r = self.as_mut().rust_mut();
                let page = r.view.height * 0.9;
                r.view
                    .scroll_by(if key == KEY_PAGE_UP { -page } else { page });
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
                let residue = t.len() == 1 && t.bytes().all(|b| b.is_ascii_alphabetic());
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
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let len = crate::bridge::checked_document!(sync self, self.len());
        let mut r = self.as_mut().rust_mut();
        if let Some(c) = r.view.cursor {
            r.view.cursor = Some(c.min(len));
        }
        if let Some((a, b)) = r.view.selection {
            r.view.selection = (a.min(len) < b.min(len)).then(|| (a.min(len), b.min(len)));
        }
        if let (Some(id), Some(session), Some(doc)) = (r.view.selected, &r.session, r.doc)
            && crate::bridge::checked_document!(access_thread, session.doc(doc))
                .feature(id)
                .is_none()
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
            Ok(_) => {
                self.as_mut().set_dirty(true);
                // Undoing or redoing makes the last message stale.
                self.as_mut().notify("");
            }
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

        let stamp = theme_stamp();
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

    pub fn activity_log(&self, files: &QString, utc_offset_min: i32) -> QString {
        let mut roots: Vec<PathBuf> = files
            .to_string()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|f| project_root(Path::new(f)))
            .collect();
        roots.sort();
        roots.dedup();
        let text = basecraft_ops::log::activity_text(&roots, started(), utc_offset_min)
            .unwrap_or_else(|e| format!("Couldn't read the log: {e}\n"));
        let Some(home) = std::env::var_os("HOME") else {
            return QString::default();
        };
        let dir = PathBuf::from(home).join(".cache/basecraft/activity");
        let path = dir.join(format!("activity-{}.txt", started().replace(':', "")));
        match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text)) {
            Ok(()) => QString::from(&path.to_string_lossy().into_owned()),
            Err(_) => QString::default(),
        }
    }

    pub fn relative_path(&self, path: &QString) -> QString {
        let path = PathBuf::from(path.to_string());
        let project = PathBuf::from(self.rust().project_dir.to_string());
        let rel = path
            .strip_prefix(&project)
            .ok()
            .filter(|_| !project.as_os_str().is_empty())
            .or_else(|| path.strip_prefix(project_root(&path)).ok())
            .unwrap_or(&path);
        QString::from(&rel.to_string_lossy().into_owned())
    }

    pub fn memory_report(&self) -> QString {
        QString::from(&memory_text())
    }

    pub fn file_kind(&self, path: &QString) -> QString {
        if basecraft_ops::sanger_review::evidence_kind(Path::new(&path.to_string())).is_some() {
            return QString::from("trace");
        }
        if Path::new(&path.to_string())
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ab1") || ext.eq_ignore_ascii_case("abi"))
        {
            return QString::from("trace");
        }
        let kind = basecraft_ops::files::kind(Path::new(&path.to_string()));
        QString::from(match kind {
            Ok(basecraft_ops::files::Kind::Alignment) => "alignment",
            Ok(basecraft_ops::files::Kind::Tree) => "tree",
            Ok(basecraft_ops::files::Kind::Delimitation) => "delimitation",
            Ok(basecraft_ops::files::Kind::Metadata) => "metadata",
            Ok(basecraft_ops::files::Kind::Submission) => "submission",
            _ => "sequence",
        })
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
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let len = crate::bridge::checked_document!(sync self, self.len());
        self.as_mut()
            .rust_mut()
            .view
            .context_press(x as f32, y as f32, len);
        self.as_mut().adopt_parts();
        self.as_mut().refresh();
        let (selected, selection) = {
            let v = &self.rust().view;
            (v.selected, v.selection)
        };
        let label = selected
            .and_then(|id| {
                self.with_doc(|s, d| {
                    crate::bridge::checked_document!(access_thread, s.doc(d))
                        .feature(id)
                        .map(|f| f.label().unwrap_or(&f.kind).to_string())
                })
            })
            .flatten()
            .unwrap_or_default();
        let feature_length = selected
            .and_then(|id| {
                self.with_doc(|s, d| {
                    crate::bridge::checked_document!(access_thread, s.doc(d))
                        .feature(id)
                        .map(|f| f.location.len())
                })
            })
            .flatten()
            .unwrap_or(0);
        let info = json!({
            "feature": selected.map_or(-1, |FeatureId(i)| i64::from(i)),
            "label": label,
            "featureLength": feature_length,
            "selection": selection.is_some(),
            "length": selection.map_or(0, |(a, b)| b - a),
        });
        QString::from(&info.to_string())
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn double_click(mut self: Pin<&mut Self>, x: f64, y: f64) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        self.as_mut()
            .rust_mut()
            .view
            .zoom_at_point(x as f32, y as f32, 3.0, len);
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn zoom_by(mut self: Pin<&mut Self>, x: f64, y: f64, factor: f64) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        if factor > 0.0 {
            self.as_mut()
                .rust_mut()
                .view
                .zoom_at_point(x as f32, y as f32, factor, len);
        }
        self.refresh();
    }

    fn zoom_centre(mut self: Pin<&mut Self>, factor: f64) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        {
            let mut r = self.as_mut().rust_mut();
            if r.view.mode == Mode::Circular {
                r.view.mode = Mode::Linear;
            }
            r.view.zoom_step(factor, len);
        }
        self.as_mut().sync_mode();
        self.refresh();
    }

    pub fn set_zoom_percent(self: Pin<&mut Self>, percent: f64) {
        if !percent.is_finite() || !(0.000_001..=20_000.0).contains(&percent) {
            return;
        }
        let factor = percent / self.rust().view.zoom_percent();
        self.zoom_centre(factor);
    }

    pub fn zoom_in(self: Pin<&mut Self>) {
        self.zoom_centre(ZOOM_STEP);
    }

    pub fn zoom_out(self: Pin<&mut Self>) {
        self.zoom_centre(1.0 / ZOOM_STEP);
    }

    pub fn zoom_to_selection(mut self: Pin<&mut Self>) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        let Some((a, b)) = self.rust().view.selection else {
            self.notify("Nothing selected");
            return;
        };
        self.as_mut().rust_mut().view.zoom_to(a, b, len);
        self.as_mut().sync_mode();
        self.refresh();
    }

    fn feature_bounds(&self, id: i32) -> Option<(u64, u64)> {
        let access_thread = self.qt_thread();
        let id = FeatureId(u32::try_from(id).ok()?);
        self.with_doc(|s, d| {
            crate::bridge::checked_document!(access_thread, s.doc(d))
                .feature(id)
                .and_then(|f| f.location.bounds())
        })
        .flatten()
    }

    pub fn zoom_to_feature(mut self: Pin<&mut Self>, id: i32) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        if let Some((a, b)) = self.feature_bounds(id) {
            self.as_mut().rust_mut().view.zoom_to(a, b, len);
            self.as_mut().sync_mode();
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
        let access_thread = self.qt_thread();
        let Ok(fid) = u32::try_from(id) else {
            return QString::default();
        };
        let out = self
            .with_doc(|s, d| {
                crate::bridge::checked_document!(access_thread, s.doc(d))
                    .feature(FeatureId(fid))
                    .map(|f| feature_to_json(id, f))
            })
            .flatten()
            .unwrap_or(Value::Null);
        QString::from(&out.to_string())
    }

    pub fn new_feature_json(&self) -> QString {
        let access_thread = self.qt_thread();
        let location = self
            .rust()
            .view
            .new_feature_location(crate::bridge::checked_document!(access_thread, self.len()));
        let f = Feature {
            kind: "misc_feature".into(),
            location,
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

    pub fn find_orfs(self: Pin<&mut Self>, min_codons: i32, in_selection: bool) -> QString {
        let params = json!({ "min_codons": min_codons.max(1) });
        self.run_annotator(
            ("orf-finder", "find_orfs", "ORF finder"),
            params,
            in_selection,
            ("ORF", "ORFs"),
        )
    }

    pub fn predict_genes(self: Pin<&mut Self>, code: i32, in_selection: bool) -> QString {
        let params = json!({ "translation_table": code });
        self.run_annotator(
            ("pyrodigal", "predict_genes", "Gene prediction"),
            params,
            in_selection,
            ("gene", "genes"),
        )
    }

    pub fn genetic_code(&self) -> i32 {
        let access_thread = self.qt_thread();
        let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) else {
            return 11;
        };
        // A refused read must not be confused with a sequence without a CDS
        // annotation, for which the existing UI suggestion is code 11.
        let sequence = crate::bridge::checked_document!(access_thread, session.doc(doc));
        sequence
            .features()
            .filter(|(_, f)| f.kind == "CDS")
            .find_map(|(_, f)| f.qualifier("transl_table")?.trim().parse().ok())
            .unwrap_or(11)
    }

    /// Run an annotating plugin operation over the sequence or the
    /// selection, add what it proposes that isn't annotated yet, and report
    /// in the status bar.
    fn run_annotator(
        mut self: Pin<&mut Self>,
        (plugin, operation, title): (&str, &str, &str),
        params: serde_json::Value,
        in_selection: bool,
        (one, many): (&str, &str),
    ) -> QString {
        let range = if in_selection {
            self.rust().view.selection
        } else {
            None
        };
        if in_selection && range.is_none() {
            self.as_mut().notify("Select bases first");
            return QString::default();
        }
        if self.rust().busy {
            return QString::default();
        }
        let Some(doc) = self.rust().doc else {
            return QString::default();
        };
        let Some(session) = self.as_mut().rust_mut().session.take() else {
            return QString::default();
        };
        self.as_mut().set_busy(true);
        self.as_mut().notify(&format!("Running {title}"));
        let started = Instant::now();
        let (plugin, operation, title, one, many) = (
            plugin.to_owned(),
            operation.to_owned(),
            title.to_owned(),
            one.to_owned(),
            many.to_owned(),
        );
        let qt_thread = self.qt_thread();
        let spawn = crate::document_jobs::background_session(
            session,
            move |session| {
                Self::run_plugin_in_session(session, doc, &plugin, &operation, params, range)
            },
            move |session, result| {
                let _ = qt_thread.queue(move |mut this| {
                    // Replacement is refused while the session is owned by this
                    // continuation. A destroyed QObject simply drops this return.
                    this.as_mut().rust_mut().session = Some(session);
                    this.as_mut().set_busy(false);
                    match result {
                        Ok((added, known)) => {
                            let message = match (added, known) {
                                (0, 0) => format!("No {many} found"),
                                (0, _) => format!("No new {many}"),
                                (1, _) => format!("Added 1 {one}"),
                                (n, _) => format!("Added {n} {many}"),
                            };
                            if added > 0 {
                                this.as_mut().set_dirty(true);
                            }
                            this.as_mut().done(&format!("{message}{}", took(started)));
                        }
                        Err(error) => this.as_mut().fail(&format!("{title} failed: {error}")),
                    }
                    this.refresh();
                });
            },
        );
        if let Err((session, error)) = spawn {
            self.as_mut().rust_mut().session = Some(*session);
            self.as_mut().set_busy(false);
            self.fail(&error);
        }
        QString::default()
    }

    /// Returns how many features were added and how many proposals were
    /// already annotated. Runs the plugin's registered sequence operation
    /// through the shared job runner, so the run keeps its submitted snapshot,
    /// effective settings and package pins in the folder's hidden analysis
    /// store and records them in the document's history.
    fn run_plugin_in_session(
        session: &mut Session,
        doc: DocId,
        name: &str,
        operation: &str,
        mut params: serde_json::Value,
        range: Option<(u64, u64)>,
    ) -> Result<(usize, usize), String> {
        use basecraft_plugin_host::{
            jobs::JobManager,
            policy::SessionPolicy,
            registry::{Registry, RegistryConfig},
        };
        use basecraft_protocol::operation::{InputReference, OperationRequest, RuntimeState};
        let id = basecraft_plugin_host::Manifest::load(&plugin_dir(name))
            .map_err(|e| e.to_string())?
            .id;
        let registry = RegistryConfig::discover()
            .and_then(|config| Registry::load(&config))
            .map_err(|e| e.to_string())?;
        let registered = registry
            .operations
            .values()
            .find(|op| op.package.manifest.id == id && op.descriptor.name == operation)
            .ok_or_else(|| format!("{id} has no registered {operation} operation"))?;
        let binding = registered
            .descriptor
            .inputs
            .keys()
            .next()
            .ok_or("operation declares no snapshot input")?
            .clone();
        let operation_id = registered.id.clone();
        if let Some((a, b)) = range {
            params["start"] = json!(a);
            params["end"] = json!(b);
        }
        let outcome = {
            let revision = session
                .checked_sequence(doc)
                .map_err(|e| e.to_string())?
                .version();
            let request = OperationRequest {
                operation: operation_id,
                inputs: [(
                    binding,
                    vec![InputReference::Document {
                        doc: doc.0.to_string(),
                        generation: session.generation(doc),
                        revision,
                    }],
                )]
                .into(),
                parameters: params,
                target: None,
                destinations: std::collections::BTreeMap::default(),
                pins: None,
                request_key: None,
            };
            let policy = SessionPolicy::new(session.root(), true).map_err(|e| e.to_string())?;
            let mut jobs = JobManager::default()
                .with_task_cancellation()
                .skipping_known_features();
            let accepted = jobs
                .submit_in_session(&registry, request, policy, "GUI", session)
                .map_err(|e| e.to_string())?;
            jobs.wait_in_session(&accepted.run_id, session, &|| false)
                .map_err(|e| e.to_string())?
        };
        if outcome.state != RuntimeState::Completed {
            return Err(outcome.errors.first().map_or_else(
                || format!("run ended {:?}", outcome.state),
                |error| error.message.clone(),
            ));
        }
        let (applied, known) = outcome
            .proposals
            .map_or((0, 0), |p| (p.applied, p.rejected.len()));
        Ok((applied, known))
    }

    pub fn has_selection(&self) -> bool {
        self.rust().view.selection.is_some()
    }

    /// The selected bases. A selected annotation is copied as itself:
    /// spliced if it has parts, reverse-complemented on the reverse strand.
    pub fn selection_text(&self) -> QString {
        let access_thread = self.qt_thread();
        let v = &self.rust().view;
        let Some((a, b)) = v.selection else {
            return QString::default();
        };
        if let Some(FeatureId(id)) = v.selected
            && let Ok(id) = i32::try_from(id)
            && self.feature_bounds(id) == Some((a, b))
        {
            return self.feature_sequence(id);
        }
        let text = self
            .with_doc(|s, d| {
                String::from_utf8_lossy(
                    &crate::bridge::checked_document!(access_thread, s.doc(d)).read(a, b),
                )
                .into_owned()
            })
            .unwrap_or_default();
        QString::from(&text)
    }

    /// Extract `start..end` to a new file named after the open one and the
    /// range, next to it; returns the path.
    fn extract_range(
        mut self: Pin<&mut Self>,
        (start, end): (u64, u64),
        segments: &[(u64, u64)],
        reverse: bool,
        label: Option<&str>,
    ) -> bool {
        if self.rust().busy {
            return false;
        }
        let Some(doc) = self.rust().doc else {
            return false;
        };
        let Some(session) = self.as_mut().rust_mut().session.take() else {
            return false;
        };
        let source = PathBuf::from(self.rust().file_path.to_string());
        let segments = segments.to_vec();
        let label = label.map(str::to_owned);
        self.as_mut().set_busy(true);
        self.as_mut().notify("Extracting sequence");
        let qt_thread = self.qt_thread();
        let started = Instant::now();
        let spawn = crate::document_jobs::background_session(
            session,
            move |session| {
                let dir = source.parent().map(Path::to_path_buf).unwrap_or_default();
                let stem = source.file_stem().map_or_else(
                    || "extract".to_string(),
                    |s| s.to_string_lossy().into_owned(),
                );
                let clean = |l: &str| {
                    l.chars()
                        .map(|c| {
                            if c.is_alphanumeric() || "-_.".contains(c) {
                                c
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>()
                };
                let base = match &label {
                    Some(l) => format!("{stem}_{}", clean(l)),
                    None if reverse => format!("{stem}_{}-{end}_rc", start + 1),
                    None => format!("{stem}_{}-{end}", start + 1),
                };
                let mut target = dir.join(format!("{base}.gb"));
                let mut n = 2;
                while target.exists() {
                    target = dir.join(format!("{base}_{n}.gb"));
                    n += 1;
                }
                session
                    .extract(doc, (start, end), &segments, reverse, &target)
                    .map_err(|e| e.to_string())?;
                Ok(target)
            },
            move |session, result| {
                let _ = qt_thread.queue(move |mut this| {
                    this.as_mut().rust_mut().session = Some(session);
                    this.as_mut().set_busy(false);
                    match result {
                        Ok(target) => {
                            this.as_mut().done(&format!(
                                "Extracted {} bp{}",
                                basecraft_viewmodel::genome::format_thousands(end - start),
                                took(started)
                            ));
                            this.extraction_finished(QString::from(
                                target.to_string_lossy().as_ref(),
                            ));
                        }
                        Err(error) => {
                            this.as_mut().fail(&format!("Extract failed: {error}"));
                            this.extraction_finished(QString::default());
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

    /// Extract the selection. A selected annotation is extracted as itself:
    /// spliced if it has several parts, reverse-complemented if it lies on
    /// the reverse strand.
    pub fn extract_selection(mut self: Pin<&mut Self>) -> bool {
        let (selection, selected) = {
            let v = &self.rust().view;
            (v.selection, v.selected)
        };
        let Some((a, b)) = selection else {
            self.as_mut().notify("Select bases to extract");
            return false;
        };
        if let Some(FeatureId(id)) = selected
            && let Ok(id) = i32::try_from(id)
        {
            return self.extract_feature(id);
        }
        self.extract_range((a, b), &[], false, None)
    }

    pub fn extract_feature(mut self: Pin<&mut Self>, id: i32) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let Ok(fid) = u32::try_from(id) else {
            return false;
        };
        let info = self
            .with_doc(|s, d| {
                let f = crate::bridge::checked_document!(access_thread, s.doc(d))
                    .feature(FeatureId(fid))?;
                let segs: Vec<(u64, u64)> = f
                    .location
                    .segments
                    .iter()
                    .map(|s| (s.start, s.end))
                    .collect();
                let bounds = f.location.bounds()?;
                let label = f.label().map(|l| format!("{l}_{}", f.kind));
                Some((bounds, segs, f.location.strand() == Strand::Reverse, label))
            })
            .flatten();
        match info {
            Some((bounds, segs, reverse, label)) => {
                let segs = if segs.len() > 1 { segs } else { Vec::new() };
                self.extract_range(bounds, &segs, reverse, label.as_deref())
            }
            None => false,
        }
    }

    pub fn blast_queries(&self, source: &QString, feature: i32) -> QString {
        let result = self
            .with_doc(|session, id| -> basecraft_ops::Result<_> {
                let source = source.to_string();
                if source == "all" {
                    return session.blast_records(id);
                }
                let doc = session.checked_sequence(id)?;
                let selected = if source == "annotation" {
                    let fid = u32::try_from(feature).map_err(|_| {
                        basecraft_ops::Error::Invalid("Choose an annotation".into())
                    })?;
                    Some(doc.feature(FeatureId(fid)).ok_or_else(|| {
                        basecraft_ops::Error::Invalid("Annotation no longer exists".into())
                    })?)
                } else {
                    None
                };
                let range = if source == "selection" {
                    Some(self.rust().view.selection.ok_or_else(|| {
                        basecraft_ops::Error::Invalid("Select a sequence range".into())
                    })?)
                } else {
                    None
                };
                let mut query = basecraft_ops::blast::from_doc(doc, range, selected)?;
                query.source["session"] = serde_json::json!(session.id());
                query.source["generation"] = serde_json::json!(session.generation(id));
                Ok(vec![query])
            })
            .unwrap_or_else(|| Err(basecraft_ops::Error::Invalid("Sequence closed".into())));
        QString::from(&match result {
            Ok(queries) => serde_json::to_string(&queries).unwrap_or_default(),
            Err(error) => serde_json::json!({"error":error.to_string()}).to_string(),
        })
    }

    pub fn feature_sequence(&self, id: i32) -> QString {
        let access_thread = self.qt_thread();
        let Ok(fid) = u32::try_from(id) else {
            return QString::default();
        };
        let text = self
            .with_doc(|s, d| {
                let doc = crate::bridge::checked_document!(access_thread, s.doc(d));
                let feature = doc.feature(FeatureId(fid))?;
                basecraft_ops::files::feature_sequence(doc, feature)
                    .ok()
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            })
            .flatten()
            .unwrap_or_default();
        QString::from(&text)
    }

    pub fn overview_scroll_to(mut self: Pin<&mut Self>, fraction: f64) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        self.as_mut()
            .rust_mut()
            .view
            .overview_scroll_to(fraction, len);
        self.refresh();
    }

    pub fn scroll_to(mut self: Pin<&mut Self>, fraction: f64) {
        let len = crate::bridge::checked_document!(sync self, self.len());
        self.as_mut().rust_mut().view.scroll_to(fraction, len);
        self.refresh();
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn edge_scroll(mut self: Pin<&mut Self>, x: f64, y: f64) -> bool {
        let len = crate::bridge::checked_document!(sync self, self.len());
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
        let access_thread = self.qt_thread();
        let text = self
            .with_doc(|s, d| {
                self.rust().view.hover_text(
                    x as f32,
                    y as f32,
                    crate::bridge::checked_document!(access_thread, s.doc(d)),
                )
            })
            .flatten()
            .unwrap_or_default();
        QString::from(&text)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn pan_press(mut self: Pin<&mut Self>, x: f64, y: f64) {
        self.as_mut().rust_mut().view.pan_press(x as f32, y as f32);
    }

    pub fn adjust_location(
        &self,
        text: &QString,
        strand: &QString,
        partial5: bool,
        partial3: bool,
    ) -> QString {
        let Some(mut loc) = parse_location(text.to_string().trim()) else {
            return QString::default();
        };
        if let Some(st) = strand_from_name(&strand.to_string()) {
            loc.set_strand(st);
        }
        loc.set_partial(partial5, partial3);
        QString::from(&format_location(&loc))
    }

    pub fn location_info(&self, text: &QString) -> QString {
        let info = match parse_location(text.to_string().trim()) {
            Some(loc) if !loc.segments.is_empty() => location_summary(&loc),
            _ => json!({ "valid": false }),
        };
        QString::from(&info.to_string())
    }

    pub fn set_show_gc(mut self: Pin<&mut Self>, show: bool) {
        self.as_mut().rust_mut().view.show_gc = show;
        self.refresh();
    }

    pub fn step_feature(mut self: Pin<&mut Self>, forward: bool) -> bool {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let found = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            match (&r.session, r.doc) {
                (Some(s), Some(d)) => r.view.step_feature(
                    crate::bridge::checked_document!(access_thread, s.doc(d)),
                    forward,
                ),
                _ => false,
            }
        };
        if !found {
            self.as_mut().notify(if forward {
                "No annotation further on"
            } else {
                "No annotation before this"
            });
        }
        self.refresh();
        found
    }

    pub fn available_cpu_threads(&self) -> i32 {
        i32::try_from(basecraft_plugin_host::compute::available()).unwrap_or(i32::MAX)
    }
    pub fn configure_cpu_threads(&self, threads: i32) {
        basecraft_plugin_host::compute::configure(usize::try_from(threads).unwrap_or(1));
    }
    pub fn version(&self) -> QString {
        QString::from(env!("CARGO_PKG_VERSION"))
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

/// Memory use of this process, for scripted runs.
pub(crate) fn memory_text() -> String {
    let text = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
    let field = |name: &str| -> f64 {
        text.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
            .map_or(0.0, |kb| kb / 1024.0)
    };
    // Private_Clean is mostly pages of memory-mapped files, which the
    // kernel can drop at any time; anonymous memory is what we own.
    format!(
        "rss {:.1} MB, pss {:.1} MB, private {:.1} MB, of which anonymous {:.1} MB",
        field("Rss:"),
        field("Pss:"),
        field("Private_Clean:") + field("Private_Dirty:"),
        field("Anonymous:")
    )
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

/// Editor JSON: `{"id", "kind", "location": "GenBank syntax, 1-based",
/// "qualifiers": [{"key", "value", "flag"}]}`. Internal `gff:` qualifiers
/// are passed through untouched so a round trip keeps them.
fn strand_name(loc: &Location) -> &'static str {
    let first = loc.segments.first().map(|s| s.strand);
    if loc.segments.iter().any(|s| Some(s.strand) != first) {
        return "mixed";
    }
    match first {
        Some(Strand::Forward) => "forward",
        Some(Strand::Reverse) => "reverse",
        _ => "none",
    }
}

fn strand_from_name(name: &str) -> Option<Strand> {
    match name {
        "forward" => Some(Strand::Forward),
        "reverse" => Some(Strand::Reverse),
        "none" => Some(Strand::Unstranded),
        _ => None,
    }
}

fn location_summary(loc: &Location) -> Value {
    json!({
        "valid": true,
        "strand": strand_name(loc),
        "partial5": loc.partial_5(),
        "partial3": loc.partial_3(),
        "length": loc.len(),
        "parts": loc.segments.len(),
    })
}

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
        "info": location_summary(&f.location),
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

crate::find_bridge::controller_find!(qobject::GenomeController);

impl qobject::GenomeController {
    fn find_update(mut self: std::pin::Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        let (Some(session), Some(doc)) = (&r.session, r.doc) else {
            return;
        };
        let key = crate::find_bridge::document_key(session, doc);
        let d = crate::bridge::checked_document!(access_thread, session.doc(doc));
        r.find.update(key, || {
            basecraft_viewmodel::find::Input::Sequence(d.clone())
        });
    }
    fn find_reveal(mut self: std::pin::Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.doc(doc));
        }
        let mut r = self.as_mut().rust_mut();
        let r = &mut *r;
        let (Some(session), Some(doc), Some(hit)) = (&r.session, r.doc, r.find.hit()) else {
            return;
        };
        let d = crate::bridge::checked_document!(access_thread, session.doc(doc));
        r.view.reveal(hit.start, d.len());
    }
    fn find_y(&self) -> f32 {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_palette_refresh_uses_preference_and_appearance_stamps() {
        use basecraft_platform::settings::{Change, DataColorScheme, SettingsStore};
        let temp = tempfile::tempdir().unwrap();
        let preferences = temp.path().join("preferences.toml");
        let desktop = temp.path().join("colors.toml");
        let (normal, before) = load_theme_at(None, &preferences);
        assert_eq!(normal, Theme::default());
        assert!(!preferences.exists());
        SettingsStore::new(preferences.clone())
            .update(Change::DataColorScheme(DataColorScheme::ColorBlindFriendly))
            .unwrap();
        let (light, after) = load_theme_at(None, &preferences);
        assert_ne!(before, after);
        assert_eq!(light, Theme::default().with_color_blind_friendly(true));
        std::fs::write(&desktop, "mode = \"dark\"\nbackground = \"#111111\"\ndark_background = \"#222222\"\ndarker_background = \"#333333\"\nselection = \"#444444\"\n").unwrap();
        let (dark, stamp) = load_theme_at(Some(&desktop), &preferences);
        assert!(dark.dark);
        assert_ne!(stamp, after);
        assert_ne!(dark.data_palette(), light.data_palette());
        SettingsStore::new(preferences.clone())
            .update(Change::DataColorScheme(DataColorScheme::DesktopTheme))
            .unwrap();
        assert!(
            load_theme_at(Some(&desktop), &preferences)
                .0
                .data_colors
                .is_none()
        );
    }

    #[test]
    fn error_summary_keeps_one_readable_line() {
        let asap = "plugin error -32000: RuntimeError: Traceback (most recent call last):\n  File \"asap_worker.py\", line 12, in <module>\n    asap = importlib.import_module(\"itaxotools.asapy.asap\")\n           ~~~~~~^^^^\nModuleNotFoundError: No module named 'itaxotools'\n\nDuring handling of the above exception, another exception occurred:\n\nTraceback (most recent call last):\n  File \"asap_worker.py\", line 14, in <module>\nModuleNotFoundError: No module named 'asap'\n";
        assert_eq!(
            error_summary(asap),
            "ModuleNotFoundError: No module named 'asap'"
        );
        assert_eq!(
            error_summary("plugin error -32000: RuntimeError: zero-length ancestral event"),
            "zero-length ancestral event"
        );
        assert_eq!(
            error_summary("Cannot attach tree: bad\nsecond"),
            "Cannot attach tree: bad"
        );
        let long = "x".repeat(500);
        assert_eq!(error_summary(&long).chars().count(), 200);
        assert!(!error_summary(&"line\n".repeat(50)).contains('\n'));
    }

    #[test]
    fn local_file_urls_round_trip_reserved_and_unicode_names() {
        for path in [
            "/tmp/a b.fasta",
            "/tmp/100%real#fragment?.fasta",
            "/tmp/échantillon/藻.fasta",
        ] {
            let url = QUrl::from_local_file(&QString::from(path));
            let encoded = Vec::<u8>::from(&url.to_encoded());
            let parsed = QUrl::from(std::str::from_utf8(&encoded).unwrap());
            assert_eq!(parsed.to_local_file().unwrap().to_string(), path);
        }
        assert!(
            QUrl::from("https://example.org/a.fasta")
                .to_local_file()
                .is_none()
        );
        assert_eq!(
            QUrl::from("file:///tmp/100%25real.fasta")
                .to_local_file()
                .unwrap()
                .to_string(),
            "/tmp/100%real.fasta"
        );
    }

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

    #[test]
    fn direction_and_completeness_rewrite_locations() {
        let mut loc = parse_location("join(10..20,30..40)").unwrap();
        loc.set_strand(Strand::Reverse);
        loc.set_partial(true, false);
        assert_eq!(format_location(&loc), "complement(join(10..20,30..>40))");
        loc.set_strand(Strand::Forward);
        assert_eq!(format_location(&loc), "join(<10..20,30..40)");
        assert_eq!(location_summary(&loc)["partial5"], json!(true));
    }
}
