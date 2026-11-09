//! QML boundary for the Qt-independent preferences service.
use basecraft_platform::{
    paths::AppPaths,
    settings::{Change, Preferences, SettingsStore},
};
use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, snapshot)]
        #[qproperty(QString, error)]
        type AppSettings = super::SettingsRust;
        #[qinvokable]
        fn reload(self: Pin<&mut AppSettings>) -> bool;
        #[qinvokable]
        fn update(self: Pin<&mut AppSettings>, key: &QString, value: &QString) -> bool;
        #[qinvokable]
        fn import_legacy(self: Pin<&mut AppSettings>, values: &QString) -> bool;
        #[qinvokable]
        fn project_history(self: &AppSettings) -> QString;
        #[qinvokable]
        fn remember_project(self: Pin<&mut AppSettings>, path: &QString) -> QString;
        #[qinvokable]
        fn project_picker_directory(self: &AppSettings, home: &QString) -> QString;
        #[qinvokable]
        fn project_is_directory(self: &AppSettings, path: &QString) -> bool;
    }
}
pub struct SettingsRust {
    snapshot: QString,
    error: QString,
    project_picker: ProjectPickerLocation,
}
impl Default for SettingsRust {
    fn default() -> Self {
        Self {
            snapshot: snapshot(&Preferences::default()).unwrap_or_default(),
            error: QString::default(),
            project_picker: ProjectPickerLocation::default(),
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
        "colorBlindFriendly" => Change::ColorBlindFriendly(boolean()?),
        "showGc" => Change::ShowGc(boolean()?),
        "sidebarOpen" => Change::SidebarOpen(boolean()?),
        "orfMinCodons" => Change::OrfMinCodons(integer()?),
        "alignmentInsert" => Change::AlignmentInsert(boolean()?),
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
        "rememberWorkspace" => Change::RememberWorkspace(boolean()?),
        "persistentBrowsingIndexes" => Change::PersistentBrowsingIndexes(boolean()?),
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
        "provenance" => {
            Change::Provenance(serde_json::from_value(value.clone()).map_err(|e| e.to_string())?)
        }
        "submitter" => Change::Submitter(Box::new(
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?,
        )),
        _ => return Err(format!("unknown preference {key}")),
    })
}
impl qobject::AppSettings {
    fn project_history(&self) -> QString {
        history_reply(load_project_history())
    }
    fn remember_project(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let result = self
            .as_mut()
            .rust_mut()
            .project_picker
            .opened(Path::new(&path.to_string()))
            .and_then(|path| remember_project_path(&path));
        history_reply(result)
    }
    fn project_picker_directory(&self, home: &QString) -> QString {
        let folder = self.project_picker.directory(
            &load_project_history().unwrap_or_default(),
            Path::new(&home.to_string()),
        );
        QString::from(folder.to_string_lossy().as_ref())
    }
    fn project_is_directory(&self, path: &QString) -> bool {
        Path::new(&path.to_string()).is_dir()
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
        let result = serde_json::from_str::<serde_json::Value>(&value.to_string())
            .map_err(|e| e.to_string())
            .and_then(|v| change(&key.to_string(), &v))
            .and_then(|change| store()?.update(change).map_err(|e| e.to_string()));
        self.publish(result)
    }
    fn import_legacy(self: Pin<&mut Self>, values: &QString) -> bool {
        let result = store().and_then(|store| {
            import_legacy_values(
                &store,
                &values.to_string(),
                std::env::var_os("BASECRAFT_PROFILE_DIR").is_some(),
            )
        });
        self.publish(result)
    }
}

const PROJECT_HISTORY_VERSION: u32 = 1;
const PROJECT_HISTORY_LIMIT: usize = 10;

#[derive(Default)]
struct ProjectPickerLocation {
    last_opened: Option<PathBuf>,
}

impl ProjectPickerLocation {
    // Session state follows successful folder opens, even when history cannot
    // be saved. Failed opens and merely browsing/cancelling a picker leave it alone.
    fn opened(&mut self, path: &Path) -> Result<PathBuf, String> {
        let path = project_directory(path)?;
        self.last_opened = Some(path.clone());
        Ok(path)
    }

    fn directory(&self, history: &[PathBuf], home: &Path) -> PathBuf {
        if let Some(path) = &self.last_opened
            && path.is_dir()
        {
            return path.clone();
        }
        let mut usable = history
            .iter()
            .filter(|path| path.is_absolute())
            .filter_map(|path| project_directory(path).ok());
        let Some(mut common) = usable.next() else {
            return home.into();
        };
        for path in usable {
            while !path.starts_with(&common) {
                if !common.pop() {
                    return home.into();
                }
            }
        }
        // A filesystem root shared by unrelated projects is no useful starting point.
        if common.parent().is_none() {
            home.into()
        } else {
            common
        }
    }
}

fn project_directory(path: &Path) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("cannot open project {}: {e}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("project {} is not a folder", canonical.display()));
    }
    Ok(canonical)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ProjectHistory {
    version: u32,
    projects: Vec<PathBuf>,
}

fn history_path() -> Result<PathBuf, String> {
    AppPaths::discover()
        .map(|paths| paths.state.join("recent-projects.json"))
        .map_err(|e| e.to_string())
}

fn load_project_history() -> Result<Vec<PathBuf>, String> {
    let path = history_path()?;
    load_project_history_at(&path)
}

fn load_project_history_at(path: &Path) -> Result<Vec<PathBuf>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot read recent projects: {error}")),
    };
    if bytes.len() > 64 * 1024 {
        return Err("recent projects file is too large; original retained".into());
    }
    let history: ProjectHistory = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid recent projects file; original retained: {e}"))?;
    if history.version != PROJECT_HISTORY_VERSION || history.projects.len() > PROJECT_HISTORY_LIMIT
    {
        return Err("unsupported recent projects file; original retained".into());
    }
    Ok(history.projects)
}

fn remember_project_path(path: &Path) -> Result<Vec<PathBuf>, String> {
    remember_project_path_at(path, &history_path()?)
}

fn remember_project_path_at(path: &Path, file: &Path) -> Result<Vec<PathBuf>, String> {
    let canonical = project_directory(path)?;
    let mut projects = load_project_history_at(file)?;
    projects.retain(|existing| existing != &canonical);
    projects.insert(0, canonical);
    projects.truncate(PROJECT_HISTORY_LIMIT);
    std::fs::create_dir_all(file.parent().ok_or("recent projects path has no parent")?)
        .map_err(|e| format!("cannot create application state: {e}"))?;
    let bytes = serde_json::to_vec_pretty(&ProjectHistory {
        version: PROJECT_HISTORY_VERSION,
        projects: projects.clone(),
    })
    .map_err(|e| e.to_string())?;
    basecraft_io::write_atomic(file, |writer| writer.write_all(&bytes))
        .map_err(|e| format!("cannot save recent projects: {e}"))?;
    Ok(projects)
}

#[cfg(test)]
mod project_history_tests {
    use super::*;

    #[test]
    fn picker_uses_available_directories_and_component_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let first = temp.path().join("projects/étude one");
        let second = temp.path().join("projects/étude two");
        let lookalike = temp.path().join("projects-other/three");
        for path in [&home, &first, &second, &lookalike] {
            std::fs::create_dir_all(path).unwrap();
        }
        let file = temp.path().join("not-a-directory");
        std::fs::write(&file, b"document").unwrap();
        let picker = ProjectPickerLocation::default();
        assert_eq!(picker.directory(&[], &home), home);
        assert_eq!(
            picker.directory(
                &[temp.path().join("missing"), file, "relative".into()],
                &home
            ),
            home
        );
        assert_eq!(
            picker.directory(std::slice::from_ref(&first), &home),
            first.canonicalize().unwrap()
        );
        assert_eq!(
            picker.directory(&[first.clone(), second], &home),
            temp.path().join("projects").canonicalize().unwrap()
        );
        // "projects" and "projects-other" share no directory component.
        assert_eq!(
            picker.directory(&[first.clone(), lookalike], &home),
            temp.path().canonicalize().unwrap()
        );
        let root = first.ancestors().last().unwrap().to_path_buf();
        assert_eq!(picker.directory(&[first, root], &home), home);
    }

    #[test]
    fn picker_session_survives_failed_opens_and_history_writes() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let home = temp.path().join("home");
        let mut picker = ProjectPickerLocation::default();
        picker.opened(&first).unwrap();
        assert!(picker.opened(&temp.path().join("missing")).is_err());
        assert_eq!(
            picker.directory(std::slice::from_ref(&second), &home),
            first.canonicalize().unwrap()
        );
        let damaged = temp.path().join("history.json");
        std::fs::write(&damaged, b"damaged").unwrap();
        let opened = picker.opened(&second).unwrap();
        assert!(remember_project_path_at(&opened, &damaged).is_err());
        assert_eq!(picker.directory(&[], &home), second.canonicalize().unwrap());
        assert_eq!(std::fs::read(&damaged).unwrap(), b"damaged");
        std::fs::remove_dir(&second).unwrap();
        assert_eq!(
            picker.directory(std::slice::from_ref(&first), &home),
            first.canonicalize().unwrap()
        );
        std::fs::remove_dir(&first).unwrap();
        assert_eq!(picker.directory(&[], &home), home);
    }

    #[test]
    fn unavailable_or_malformed_history_falls_back_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("history.json");
        let picker = ProjectPickerLocation::default();
        let home = temp.path().join("home");
        assert_eq!(
            picker.directory(&load_project_history_at(&file).unwrap(), &home),
            home
        );
        assert!(!file.exists());
        for bytes in [b"damaged".as_slice(), br#"{"version":99,"projects":[]}"#] {
            std::fs::write(&file, bytes).unwrap();
            assert!(load_project_history_at(&file).is_err());
            assert_eq!(
                picker.directory(&load_project_history_at(&file).unwrap_or_default(), &home),
                home
            );
            assert_eq!(std::fs::read(&file).unwrap(), bytes);
        }
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(load_project_history_at(&file).is_err());
        assert_eq!(
            picker.directory(&load_project_history_at(&file).unwrap_or_default(), &home),
            home
        );
    }

    #[test]
    fn recent_projects_are_deduplicated_and_damaged_history_is_retained() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let file = temp.path().join("state/recent-projects.json");
        remember_project_path_at(&first, &file).unwrap();
        remember_project_path_at(&second, &file).unwrap();
        let projects = remember_project_path_at(&first, &file).unwrap();
        assert_eq!(
            projects,
            vec![
                first.canonicalize().unwrap(),
                second.canonicalize().unwrap()
            ]
        );
        std::fs::write(&file, b"damaged").unwrap();
        assert!(remember_project_path_at(&second, &file).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"damaged");
    }
}

fn history_reply(result: Result<Vec<PathBuf>, String>) -> QString {
    let value = match result {
        Ok(projects) => serde_json::json!({ "projects": projects, "error": "" }),
        Err(error) => serde_json::json!({ "projects": [], "error": error }),
    };
    QString::from(
        serde_json::to_string(&value)
            .unwrap_or_else(|_| {
                "{\"projects\":[],\"error\":\"Cannot display recent projects\"}".into()
            })
            .as_str(),
    )
}

fn import_legacy_values(
    store: &SettingsStore,
    values: &str,
    isolated: bool,
) -> Result<Preferences, String> {
    let existing = store.load().map_err(|e| e.to_string())?;
    if isolated || existing.migration.qsettings_imported {
        return Ok(existing);
    }
    let values: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(values).map_err(|e| e.to_string())?;
    if values.values().all(serde_json::Value::is_null) {
        return Ok(existing);
    }
    // QSettings INI values can arrive as strings after restarting Qt. Only
    // migration accepts these representations; live edits stay strictly typed.
    // A bad legacy field must not discard valid neighbours or block startup.
    let changes = values
        .iter()
        .filter_map(|(key, value)| {
            let normalized = match (key.as_str(), value.as_str()) {
                ("showGc" | "sidebarOpen" | "alignmentInsert", Some(text)) => {
                    match text.trim().to_ascii_lowercase().as_str() {
                        "true" | "1" => serde_json::json!(true),
                        "false" | "0" => serde_json::json!(false),
                        _ => return None,
                    }
                }
                ("orfMinCodons" | "localCpuThreads", Some(text)) => {
                    serde_json::json!(text.trim().parse::<u32>().ok()?)
                }
                _ => value.clone(),
            };
            let candidate = change(key, &normalized).ok()?;
            Preferences::default()
                .resolve_session(std::slice::from_ref(&candidate))
                .ok()?;
            Some(candidate)
        })
        .collect();
    store.import_qsettings(changes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_ini_strings_and_invalid_fields_migrate_independently() {
        let temp = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(temp.path().join("preferences.toml"));
        let prefs = import_legacy_values(&store, r#"{"alignmentInsert":"false","showGc":"true","sidebarOpen":"bogus","orfMinCodons":"250","localCpuThreads":"0","unknown":"old"}"#, false).unwrap();
        assert!(!prefs.ui.alignment_insert);
        assert!(prefs.ui.show_gc && prefs.ui.sidebar_open);
        assert_eq!(prefs.ui.orf_min_codons, 250);
        assert_eq!(prefs.compute.local_cpu_threads, 4);
        assert!(prefs.migration.qsettings_imported);
        assert_eq!(
            import_legacy_values(&store, r#"{"alignmentInsert":"true"}"#, false).unwrap(),
            prefs
        );
    }
    #[test]
    fn settings_snapshot_has_complete_defaults_before_loading() {
        let state = SettingsRust::default();
        let prefs: Preferences = serde_json::from_str(&state.snapshot.to_string()).unwrap();
        assert_eq!(prefs, Preferences::default());
    }
    #[test]
    fn isolated_profile_ignores_global_legacy_store_without_writes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config/preferences.toml");
        let store = SettingsStore::new(path.clone());
        let prefs =
            import_legacy_values(&store, r#"{"showGc":false,"localCpuThreads":8}"#, true).unwrap();
        assert!(prefs.ui.show_gc);
        assert_eq!(prefs.compute.local_cpu_threads, 4);
        assert!(!path.parent().unwrap().exists());
        assert!(!prefs.migration.qsettings_imported);
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
        assert!(change("provenance", &serde_json::json!("portable")).is_ok());
        assert!(change("provenance", &serde_json::json!("unknown")).is_err());
        assert!(change("unknown", &serde_json::Value::Null).is_err());
    }
}
