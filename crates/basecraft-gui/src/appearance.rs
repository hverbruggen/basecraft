//! One app-wide appearance: the desktop theme's colours, the colour-blind data
//! palette preference and the desktop monospace font. A watcher thread reloads
//! it when the desktop theme or font configuration changes, and the settings
//! object reloads it after a preference changes. Every controller that follows
//! it is updated on its window thread. Nothing polls.
use basecraft_platform::{paths::AppPaths, settings::ThemeChoice};
use basecraft_viewmodel::Theme;
use std::{
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use crate::bridge::css;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Appearance {
    pub theme: Theme,
    pub color_blind: bool,
    pub font: String,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub ok: String,
    pub error: String,
}

/// Desktop settings files live under the user's home folder.
struct Desktop {
    /// Omarchy's current theme; the folder is replaced when the theme changes.
    theme: Option<PathBuf>,
    fonts: Option<PathBuf>,
    preferences: Option<PathBuf>,
}

impl Desktop {
    fn discover() -> Self {
        let paths = AppPaths::discover().ok();
        let home = paths.as_ref().and_then(|paths| paths.home.clone());
        Self {
            theme: home
                .as_ref()
                .map(|home| home.join(".local/state/omarchy/current")),
            fonts: home.map(|home| home.join(".config/fontconfig")),
            preferences: paths.map(|paths| paths.preferences()),
        }
    }
}

/// The colour-blind palette choice and the theme choice; unreadable
/// preferences give the defaults.
fn display_choices(preferences: &Path) -> (bool, ThemeChoice) {
    basecraft_platform::settings::SettingsStore::new(preferences.to_owned())
        .load()
        .map_or((false, ThemeChoice::FollowDesktop), |p| {
            (
                p.ui.data_color_scheme
                    == basecraft_platform::settings::DataColorScheme::ColorBlindFriendly,
                p.ui.theme,
            )
        })
}

/// The desktop's monospace family as Omarchy reports it, else "monospace".
fn read_font() -> String {
    std::process::Command::new("omarchy")
        .args(["font", "current"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "monospace".into())
}

impl Appearance {
    /// Colours from the desktop theme file (absent: the default theme) and the
    /// preferences file (absent or damaged: no colour-blind palette).
    fn load(desktop_theme: Option<&Path>, preferences: Option<&Path>, font: String) -> Self {
        let (color_blind, choice) =
            preferences.map_or((false, ThemeChoice::FollowDesktop), display_choices);
        let theme = match choice {
            ThemeChoice::Light => Theme::default(),
            ThemeChoice::Dark => Theme::dark(),
            ThemeChoice::FollowDesktop => desktop_theme
                .and_then(|folder| std::fs::read_to_string(folder.join("theme/colors.toml")).ok())
                .map(|text| Theme::from_omarchy(&text))
                .unwrap_or_default(),
        }
        .with_color_blind_friendly(color_blind);
        Self {
            background: css(theme.background),
            foreground: css(theme.foreground),
            accent: css(theme.accent),
            ok: css(theme.nearest_hue(120.0)),
            error: css(theme.nearest_hue(0.0)),
            theme,
            color_blind,
            font,
        }
    }
}

type Follower = Arc<dyn Fn(&Arc<Appearance>) -> bool + Send + Sync>;
/// Followers queue onto window threads, whose queued calls may read the
/// current appearance, so the two never share a lock.
static FOLLOWERS: Mutex<Vec<Follower>> = Mutex::new(Vec::new());

fn state() -> &'static Mutex<Arc<Appearance>> {
    static STATE: OnceLock<Mutex<Arc<Appearance>>> = OnceLock::new();
    STATE.get_or_init(|| {
        let desktop = Desktop::discover();
        // The font comes from a slow external command; the watcher reads it.
        let first = Appearance::load(
            desktop.theme.as_deref(),
            desktop.preferences.as_deref(),
            "monospace".into(),
        );
        let _ = crate::jobs::spawn("appearance", move || watch(&desktop));
        Mutex::new(Arc::new(first))
    })
}

pub(crate) fn current() -> Arc<Appearance> {
    state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The publication palette, with the colour-blind data colours when chosen.
pub(crate) fn publication_theme() -> Theme {
    Theme::publication().with_color_blind_friendly(current().color_blind)
}

/// Apply the appearance to `object` now (`changed` false) and after every
/// change (`changed` true), on its window thread, until the object is gone.
pub(crate) fn follow<T: cxx_qt::Threading + 'static>(
    object: Pin<&mut T>,
    apply: fn(Pin<&mut T>, &Appearance, bool),
) {
    let thread = object.qt_thread();
    FOLLOWERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Arc::new(move |look| {
            let look = look.clone();
            thread
                .queue(move |object| apply(object, &look, true))
                .is_ok()
        }));
    apply(object, &current(), false);
}

fn replace(next: Appearance) {
    let next = Arc::new(next);
    {
        let mut current = state().lock().unwrap_or_else(PoisonError::into_inner);
        if **current == *next {
            return;
        }
        *current = next.clone();
    }
    // Followers queue onto window threads; call them without holding a lock.
    let followers = FOLLOWERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let gone: Vec<_> = followers
        .into_iter()
        .filter(|follow| !follow(&next))
        .collect();
    if !gone.is_empty() {
        FOLLOWERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|follower| !gone.iter().any(|dead| Arc::ptr_eq(dead, follower)));
    }
}

/// Held from reading the files to publishing the result, so a reload that
/// read older files can never land after one that read newer files.
static LOADING: Mutex<()> = Mutex::new(());

/// Reload after a preference changed. Runs off the window thread.
pub(crate) fn preferences_changed() {
    let _ = crate::jobs::spawn("appearance", || {
        let _loading = LOADING.lock().unwrap_or_else(PoisonError::into_inner);
        let desktop = Desktop::discover();
        let font = current().font.clone();
        replace(Appearance::load(
            desktop.theme.as_deref(),
            desktop.preferences.as_deref(),
            font,
        ));
    });
}

fn reload(desktop: &Desktop) {
    let font = read_font();
    let _loading = LOADING.lock().unwrap_or_else(PoisonError::into_inner);
    replace(Appearance::load(
        desktop.theme.as_deref(),
        desktop.preferences.as_deref(),
        font,
    ));
}

/// Reload once, then again after each change to the desktop theme or font
/// configuration. Without inotify (other systems) the first load is final.
fn watch(desktop: &Desktop) {
    reload(desktop);
    #[cfg(target_os = "linux")]
    {
        let folders: Vec<PathBuf> = [
            desktop.theme.clone(),
            desktop.theme.as_ref().map(|theme| theme.join("theme")),
            desktop.fonts.clone(),
        ]
        .into_iter()
        .flatten()
        .collect();
        inotify::changes(&folders, || reload(desktop));
    }
}

#[cfg(target_os = "linux")]
mod inotify {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, path::PathBuf};

    /// Call `changed` after each burst of changes in `folders`. Returns when
    /// inotify is unavailable.
    pub(super) fn changes(folders: &[PathBuf], changed: impl Fn()) {
        // SAFETY: plain syscall; the descriptor is owned by this function.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return;
        }
        let mask = libc::IN_CLOSE_WRITE
            | libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_MOVED_TO
            | libc::IN_MOVED_FROM;
        let names: Vec<CString> = folders
            .iter()
            .filter_map(|folder| CString::new(folder.as_os_str().as_bytes()).ok())
            .collect();
        let mut buffer = [0u8; 4096];
        loop {
            // A replaced folder loses its watch; adding again is harmless.
            for name in &names {
                // SAFETY: `name` is NUL-terminated and outlives the call.
                unsafe { libc::inotify_add_watch(fd, name.as_ptr(), mask) };
            }
            // SAFETY: the buffer is valid for its length.
            let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
            if read < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            if read <= 0 {
                break;
            }
            // A theme switch writes many files; read them once it settles.
            std::thread::sleep(std::time::Duration::from_millis(150));
            changed();
        }
        // SAFETY: closes the descriptor opened above, once.
        unsafe { libc::close(fd) };
    }
}

/// The four document editors follow the appearance the same way.
macro_rules! editor_follows_appearance {
    ($controller:ty) => {
        impl cxx_qt::Initialize for $controller {
            fn initialize(self: std::pin::Pin<&mut Self>) {
                crate::appearance::follow(self, |mut this, look, changed| {
                    this.as_mut().rust_mut().theme = look.theme.clone();
                    this.as_mut()
                        .set_mono_font(cxx_qt_lib::QString::from(&look.font));
                    this.as_mut()
                        .set_background(cxx_qt_lib::QString::from(&look.background));
                    this.as_mut()
                        .set_foreground(cxx_qt_lib::QString::from(&look.foreground));
                    this.as_mut()
                        .set_accent(cxx_qt_lib::QString::from(&look.accent));
                    this.as_mut()
                        .set_ok_color(cxx_qt_lib::QString::from(&look.ok));
                    this.as_mut()
                        .set_error_color(cxx_qt_lib::QString::from(&look.error));
                    if changed {
                        this.refresh();
                    }
                });
            }
        }
    };
}
pub(crate) use editor_follows_appearance;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_desktop_theme_and_the_colour_blind_preference() {
        use basecraft_platform::settings::{Change, DataColorScheme, SettingsStore};
        let temp = tempfile::tempdir().unwrap();
        let preferences = temp.path().join("preferences.toml");
        let desktop = temp.path().join("current");
        let plain = Appearance::load(None, Some(&preferences), "mono".into());
        assert_eq!(plain.theme, Theme::default());
        assert!(!preferences.exists(), "loading never writes preferences");
        SettingsStore::new(preferences.clone())
            .update(Change::DataColorScheme(DataColorScheme::ColorBlindFriendly))
            .unwrap();
        let light = Appearance::load(None, Some(&preferences), "mono".into());
        assert!(light.color_blind);
        assert_eq!(
            light.theme,
            Theme::default().with_color_blind_friendly(true)
        );
        std::fs::create_dir_all(desktop.join("theme")).unwrap();
        std::fs::write(desktop.join("theme/colors.toml"), "mode = \"dark\"\nbackground = \"#111111\"\ndark_background = \"#222222\"\ndarker_background = \"#333333\"\nselection = \"#444444\"\n").unwrap();
        let dark = Appearance::load(Some(&desktop), Some(&preferences), "mono".into());
        assert!(dark.theme.dark);
        assert_eq!(dark.background, css(dark.theme.background));
        assert_ne!(dark, light);
        assert_ne!(dark.theme.data_palette(), light.theme.data_palette());
        SettingsStore::new(preferences.clone())
            .update(Change::DataColorScheme(DataColorScheme::DesktopTheme))
            .unwrap();
        let desktop_colours = Appearance::load(Some(&desktop), Some(&preferences), "mono".into());
        assert!(desktop_colours.theme.data_colors.is_none());
        // A chosen theme wins over the desktop's.
        SettingsStore::new(preferences.clone())
            .update(Change::Theme(basecraft_platform::settings::ThemeChoice::Light))
            .unwrap();
        let light = Appearance::load(Some(&desktop), Some(&preferences), "mono".into());
        assert_eq!(light.theme, Theme::default());
        SettingsStore::new(preferences.clone())
            .update(Change::Theme(basecraft_platform::settings::ThemeChoice::Dark))
            .unwrap();
        assert!(Appearance::load(None, Some(&preferences), "mono".into()).theme.dark);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn watcher_reports_a_replaced_theme_folder() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("current");
        std::fs::create_dir_all(current.join("theme")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let folders = vec![current.clone(), current.join("theme")];
        std::thread::spawn(move || inotify::changes(&folders, move || tx.send(()).unwrap()));
        // Give the watcher time to register before switching themes.
        std::thread::sleep(std::time::Duration::from_millis(200));
        std::fs::rename(current.join("theme"), current.join("old")).unwrap();
        std::fs::create_dir(current.join("theme")).unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        while rx.try_recv().is_ok() {}
        std::fs::write(current.join("theme/colors.toml"), "mode = \"dark\"\n").unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    }
}
