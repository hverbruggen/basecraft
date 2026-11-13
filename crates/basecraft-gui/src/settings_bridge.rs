//! QML boundary for the Qt-independent preferences service.
use basecraft_platform::{
    paths::AppPaths,
    settings::{Change, DataColorScheme, Preferences, SettingsStore, ThemeChoice},
};
use basecraft_viewmodel::{projects, release};
use cxx_qt::CxxQtType;
use cxx_qt::Threading;
use cxx_qt_lib::{QString, QStringList};
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("releasecheck.h");
        /// Blocking; runs on a job thread.
        #[rust_name = "fetch_latest_release"]
        fn fetchLatestRelease(url: &QString) -> QString;
        #[rust_name = "cancel_release_check"]
        fn cancelReleaseCheck();
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, snapshot)]
        #[qproperty(QString, error)]
        /// Recently chosen project folders, newest first.
        #[qproperty(QStringList, recent_projects)]
        /// Why the recent projects could not be read, or a project not chosen.
        #[qproperty(QString, project_error)]
        type AppSettings = super::SettingsRust;
    }
    impl cxx_qt::Initialize for AppSettings {}
    impl cxx_qt::Threading for AppSettings {}
    unsafe extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut AppSettings>) -> bool;
        #[qinvokable]
        fn update(self: Pin<&mut AppSettings>, key: &QString, value: &QString) -> bool;
        /// Choose `path` as the project: remembered first in the recent
        /// projects. Returns `path`, or "" with `project_error` set.
        #[qinvokable]
        fn select_project(self: Pin<&mut AppSettings>, path: &QString) -> QString;
        #[qinvokable]
        fn project_picker_directory(self: &AppSettings) -> QString;
        #[qinvokable]
        /// Where a file dialog starts for the document at `document`
        /// (its tab path) whose file is `file` (may be empty).
        fn dialog_folder(
            self: &AppSettings,
            project: &QString,
            document: &QString,
            file: &QString,
        ) -> QString;
        /// Ask for the latest release when the user opted in, except in
        /// offscreen and screenshot runs. `release_available` follows when
        /// it is newer than this build.
        #[qinvokable]
        fn check_release(self: Pin<&mut AppSettings>);
        #[qsignal]
        fn release_available(self: Pin<&mut AppSettings>, notice: QString);
    }
}
pub struct SettingsRust {
    snapshot: QString,
    error: QString,
    recent_projects: QStringList,
    project_error: QString,
    project_picker: projects::PickerLocation,
    release: Option<crate::release_check::ReleaseCheck>,
}
impl Default for SettingsRust {
    fn default() -> Self {
        Self {
            snapshot: snapshot(&Preferences::default()).unwrap_or_default(),
            error: QString::default(),
            recent_projects: QStringList::default(),
            project_error: QString::default(),
            project_picker: projects::PickerLocation::default(),
            release: None,
        }
    }
}

fn store() -> Result<SettingsStore, String> {
    AppPaths::discover()
        .map(|paths| SettingsStore::new(paths.preferences()))
        .map_err(|e| e.to_string())
}
fn snapshot(prefs: &Preferences) -> Result<QString, String> {
    serde_json::to_string(prefs)
        .map(|s| QString::from(s.as_str()))
        .map_err(|e| e.to_string())
}
fn change(key: &str, value: &serde_json::Value) -> Result<Change, String> {
    let boolean = || {
        value
            .as_bool()
            .ok_or_else(|| format!("{key} requires a boolean"))
    };
    let integer = || {
        value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("{key} requires a positive integer"))
    };
    Ok(match key {
        "colorBlindFriendly" => Change::DataColorScheme(if boolean()? {
            DataColorScheme::ColorBlindFriendly
        } else {
            DataColorScheme::DesktopTheme
        }),
        "serviceEmail" => Change::ServiceEmail {
            service: value
                .get("service")
                .and_then(serde_json::Value::as_str)
                .ok_or("serviceEmail requires a service")?
                .into(),
            email: match value.get("email") {
                Some(serde_json::Value::String(email)) if !email.trim().is_empty() => {
                    Some(email.trim().to_owned())
                }
                _ => None,
            },
        },
        "theme" => Change::Theme(match value.as_str() {
            Some("light") => ThemeChoice::Light,
            Some("dark") => ThemeChoice::Dark,
            Some("follow-desktop") => ThemeChoice::FollowDesktop,
            _ => return Err("theme is light, dark or follow-desktop".into()),
        }),
        "releaseNotices" => Change::ReleaseNotices(boolean()?),
        "welcomed" => Change::Welcomed,
        "showGc" => Change::ShowGc(boolean()?),
        "sidebarOpen" => Change::SidebarOpen(boolean()?),
        "orfMinCodons" => Change::OrfMinCodons(integer()?),
        "alignmentInsert" => Change::AlignmentInsert(boolean()?),
        "fontFamily" => Change::FontFamily(value.as_str().map(str::to_owned)),
        "fontSizePoints" => Change::FontSizePoints(if value.is_null() {
            None
        } else {
            Some(integer()?)
        }),
        "ribbonSizePercent" => Change::RibbonSizePercent(integer()?),
        "tickBoxSize" => Change::TickBoxSize(integer()?),
        "inputBoxHeight" => Change::InputBoxHeight(integer()?),
        "localCpuThreads" => Change::CpuThreads(integer()?),
        "recovery" => Change::Recovery(boolean()?),
        "shortcut" => Change::Shortcut {
            id: value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or("shortcut requires a command ID")?
                .into(),
            value: match value.get("value") {
                Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(value)) => Some(value.clone()),
                _ => return Err("shortcut requires a string or null".into()),
            },
        },
        "keyHint" => Change::KeyHint {
            id: value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or("access key requires a command ID")?
                .into(),
            value: match value.get("value") {
                Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(value)) => Some(value.to_uppercase()),
                _ => return Err("access key requires a string or null".into()),
            },
        },
        "ribbonCommandHidden" => Change::RibbonCommandHidden {
            id: value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or("ribbon requires a command ID")?
                .into(),
            hidden: value
                .get("hidden")
                .and_then(serde_json::Value::as_bool)
                .ok_or("ribbon requires a boolean")?,
        },
        "ribbonHidden" => {
            Change::RibbonHidden(serde_json::from_value(value.clone()).map_err(|e| e.to_string())?)
        }
        "resetCustomization" => Change::ResetCustomization,
        "submitter" => Change::Submitter(Box::new(
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?,
        )),
        _ => return Err(format!("unknown preference {key}")),
    })
}
impl qobject::AppSettings {
    fn select_project(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let chosen = path.to_string();
        let result = history_path().and_then(|file| {
            projects::choose(
                &mut self.as_mut().rust_mut().project_picker,
                Path::new(&chosen),
                &file,
            )
        });
        let accepted = result.is_ok();
        self.show_projects(result);
        QString::from(if accepted { chosen.as_str() } else { "" })
    }
    /// Publish the recent projects, or keep the shown ones and say why not.
    fn show_projects(mut self: Pin<&mut Self>, result: Result<Vec<PathBuf>, String>) {
        match result {
            Ok(projects) => {
                let mut list = QStringList::default();
                for project in projects {
                    list.append(QString::from(project.to_string_lossy().as_ref()));
                }
                self.as_mut().set_recent_projects(list);
                self.set_project_error(QString::default());
            }
            Err(error) => self.set_project_error(QString::from(&error)),
        }
    }
    fn project_picker_directory(&self) -> QString {
        let home = AppPaths::discover()
            .ok()
            .and_then(|paths| paths.home)
            .unwrap_or_default();
        let folder = self
            .project_picker
            .directory(&load_project_history().unwrap_or_default(), &home);
        QString::from(folder.to_string_lossy().as_ref())
    }
    fn check_release(mut self: Pin<&mut Self>) {
        let opted_in = serde_json::from_str::<Preferences>(&self.snapshot.to_string())
            .is_ok_and(|prefs| prefs.ui.release_notices);
        let platform = std::env::var("QT_QPA_PLATFORM").ok();
        let args: Vec<String> = std::env::args().collect();
        if !release::wanted(opted_in, platform.as_deref(), &args) {
            return;
        }
        let check = crate::release_check::ReleaseCheck::start(
            self.qt_thread(),
            release::url(env!("CARGO_PKG_REPOSITORY")),
            |this: Pin<&mut Self>, body| {
                if let Some(notice) = release::notice(basecraft_protocol::VERSION, &body) {
                    this.release_available(QString::from(&notice));
                }
            },
        );
        self.as_mut().rust_mut().release = check;
    }
    fn dialog_folder(&self, project: &QString, document: &QString, file: &QString) -> QString {
        let home = AppPaths::discover().ok().and_then(|paths| paths.home);
        let folder = projects::dialog_folder(
            &project.to_string(),
            &document.to_string(),
            &file.to_string(),
            home.as_deref(),
        );
        QString::from(folder.to_string_lossy().as_ref())
    }
    fn publish(mut self: Pin<&mut Self>, result: Result<Preferences, String>) -> bool {
        match result.and_then(|prefs| snapshot(&prefs)) {
            Ok(value) => {
                self.as_mut().set_snapshot(value);
                self.set_error(QString::default());
                true
            }
            Err(error) => {
                self.set_error(QString::from(error.as_str()));
                false
            }
        }
    }
    fn reload(self: Pin<&mut Self>) -> bool {
        let result = store().and_then(|s| s.load().map_err(|e| e.to_string()));
        self.publish(result)
    }
    fn update(self: Pin<&mut Self>, key: &QString, value: &QString) -> bool {
        let key = key.to_string();
        let result = serde_json::from_str::<serde_json::Value>(&value.to_string())
            .map_err(|e| e.to_string())
            .and_then(|v| change(&key, &v))
            .and_then(|change| store()?.update(change).map_err(|e| e.to_string()));
        if result.is_ok() && matches!(key.as_str(), "colorBlindFriendly" | "theme" | "fontFamily") {
            crate::appearance::preferences_changed();
        }
        self.publish(result)
    }
}

fn history_path() -> Result<PathBuf, String> {
    AppPaths::discover()
        .map(|paths| paths.state.join("recent-projects.json"))
        .map_err(|e| e.to_string())
}

pub(crate) fn load_project_history() -> Result<Vec<PathBuf>, String> {
    let path = history_path()?;
    projects::load_history(&path)
}

impl cxx_qt::Initialize for qobject::AppSettings {
    fn initialize(self: Pin<&mut Self>) {
        self.show_projects(load_project_history());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_snapshot_has_complete_defaults_before_loading() {
        let state = SettingsRust::default();
        let prefs: Preferences = serde_json::from_str(&state.snapshot.to_string()).unwrap();
        assert_eq!(prefs, Preferences::default());
    }
    #[test]
    fn qml_values_require_the_declared_type() {
        let ribbon = change("ribbonSizePercent", &serde_json::json!(175)).unwrap();
        assert_eq!(
            Preferences::default()
                .resolve_session(&[ribbon])
                .unwrap()
                .ui
                .ribbon_size_percent,
            175
        );
        for value in [
            serde_json::json!("150"),
            serde_json::json!(150.5),
            serde_json::json!(-1),
            serde_json::Value::Null,
        ] {
            assert!(change("ribbonSizePercent", &value).is_err());
        }
        for size in [99, 201] {
            let ribbon = change("ribbonSizePercent", &serde_json::json!(size)).unwrap();
            assert!(Preferences::default().resolve_session(&[ribbon]).is_err());
        }
        assert!(change("colorBlindFriendly", &serde_json::json!(true)).is_ok());
        assert!(change("colorBlindFriendly", &serde_json::json!("true")).is_err());
        assert!(change("showGc", &serde_json::json!(true)).is_ok());
        assert!(change("showGc", &serde_json::json!("true")).is_err());
        assert!(change("localCpuThreads", &serde_json::json!(-1)).is_err());
        assert!(change("localCpuThreads", &serde_json::json!(1.5)).is_err());
        assert!(change("localCpuThreads", &serde_json::json!(u64::MAX)).is_err());
        // History placement is not a preference (user decision, 11 November).
        assert!(change("provenance", &serde_json::json!("portable")).is_err());
        assert!(change("unknown", &serde_json::Value::Null).is_err());
    }
}
