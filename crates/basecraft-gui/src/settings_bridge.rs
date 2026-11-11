//! QML boundary for the Qt-independent preferences service.
use basecraft_platform::{
    paths::AppPaths,
    settings::{Change, DataColorScheme, Preferences, SettingsStore, ThemeChoice},
};
use cxx_qt::CxxQtType;
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
        fn dialog_folder(self: &AppSettings, project: &QString, document: &QString, file: &QString) -> QString;
        /// Where the latest release is described, for the release notice.
        #[qinvokable]
        fn release_url(self: &AppSettings) -> QString;
        /// A notice when `response` (the latest release) is newer than this
        /// build, else "".
        #[qinvokable]
        fn release_notice(self: &AppSettings, response: &QString) -> QString;
    }
}
pub struct SettingsRust {
    snapshot: QString,
    error: QString,
    recent_projects: QStringList,
    project_error: QString,
    project_picker: ProjectPickerLocation,
}
impl Default for SettingsRust {
    fn default() -> Self {
        Self {
            snapshot: snapshot(&Preferences::default()).unwrap_or_default(),
            error: QString::default(),
            recent_projects: QStringList::default(),
            project_error: QString::default(),
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
    fn select_project(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let chosen = path.to_string();
        let result = history_path().and_then(|file| {
            choose_project(
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
    fn release_url(&self) -> QString {
        QString::from(&release_url(env!("CARGO_PKG_REPOSITORY")))
    }
    fn release_notice(&self, response: &QString) -> QString {
        QString::from(
            &release_notice(env!("CARGO_PKG_VERSION"), &response.to_string()).unwrap_or_default(),
        )
    }
    fn dialog_folder(&self, project: &QString, document: &QString, file: &QString) -> QString {
        let home = AppPaths::discover().ok().and_then(|paths| paths.home);
        let folder = dialog_folder(
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
        if result.is_ok() && (key == "colorBlindFriendly" || key == "theme") {
            crate::appearance::preferences_changed();
        }
        self.publish(result)
    }
}

/// GitHub's description of the latest release of `repository`.
fn release_url(repository: &str) -> String {
    let project = repository.trim_end_matches('/').trim_start_matches("https://github.com/");
    format!("https://api.github.com/repos/{project}/releases/latest")
}

/// "Basecraft X is available: URL" when GitHub's latest release is newer
/// than `current`.
fn release_notice(current: &str, response: &str) -> Option<String> {
    let release: serde_json::Value = serde_json::from_str(response).ok()?;
    let tag = release["tag_name"].as_str()?;
    let version = |text: &str| -> Option<Vec<u64>> {
        text.trim_start_matches('v')
            .split(['.', '-'])
            .take(3)
            .map(|part| part.parse().ok())
            .collect()
    };
    let newer = version(tag)? > version(current)?;
    newer.then(|| {
        format!(
            "Basecraft {} is available: {}",
            tag.trim_start_matches('v'),
            release["html_url"].as_str().unwrap_or("")
        )
    })
}

#[cfg(test)]
mod release_tests {
    use super::*;

    #[test]
    fn a_newer_release_gives_a_notice_and_others_do_not() {
        assert_eq!(
            release_url("https://github.com/someone/basecraft"),
            "https://api.github.com/repos/someone/basecraft/releases/latest"
        );
        let release = |tag: &str| {
            serde_json::json!({"tag_name": tag, "html_url": "https://example.org/r"}).to_string()
        };
        assert_eq!(
            release_notice("0.5.0", &release("v0.6.0")).as_deref(),
            Some("Basecraft 0.6.0 is available: https://example.org/r")
        );
        assert!(release_notice("0.5.0", &release("0.10.0")).is_some());
        assert_eq!(release_notice("0.5.0", &release("v0.5.0")), None);
        assert_eq!(release_notice("0.6.1", &release("v0.6.0")), None);
        assert_eq!(release_notice("0.5.0", "not json"), None);
        assert_eq!(release_notice("0.5.0", &release("nightly")), None);
    }
}

/// Where a never-saved document is first offered: the chosen project, else
/// the user's Documents folder, else the home folder.
fn untitled_folder(project: &str, home: Option<&Path>) -> PathBuf {
    if !project.is_empty() {
        return PathBuf::from(project);
    }
    let Some(home) = home else {
        return PathBuf::new();
    };
    let documents = home.join("Documents");
    if documents.is_dir() {
        documents
    } else {
        home.to_owned()
    }
}

/// Where a file dialog starts: a never-saved document's untitled folder,
/// else the folder of its file (or its tab path), else the project.
fn dialog_folder(project: &str, document: &str, file: &str, home: Option<&Path>) -> PathBuf {
    if document.starts_with("untitled:") {
        return untitled_folder(project, home);
    }
    let path = if file.is_empty() { document } else { file };
    if path.is_empty() {
        PathBuf::from(project)
    } else {
        Path::new(path).parent().map(Path::to_owned).unwrap_or_default()
    }
}

#[cfg(test)]
mod untitled_tests {
    use super::*;

    #[test]
    fn untitled_documents_start_in_the_project_then_documents_then_home() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            untitled_folder("/project", Some(home.path())),
            Path::new("/project")
        );
        assert_eq!(untitled_folder("", Some(home.path())), home.path());
        std::fs::create_dir(home.path().join("Documents")).unwrap();
        assert_eq!(
            untitled_folder("", Some(home.path())),
            home.path().join("Documents")
        );
        assert_eq!(untitled_folder("", None), PathBuf::new());
    }

    #[test]
    fn dialogs_start_beside_the_file_else_the_tab_else_the_project() {
        let home = tempfile::tempdir().unwrap();
        let at = |document, file| dialog_folder("/project", document, file, Some(home.path()));
        assert_eq!(at("untitled:2", ""), Path::new("/project"));
        assert_eq!(at("/data/a.gb", "/exports/a.fasta"), Path::new("/exports"));
        assert_eq!(at("/data/a.gb", ""), Path::new("/data"));
        assert_eq!(at("", ""), Path::new("/project"));
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
    fn choosing_a_project_remembers_it_first_and_refuses_a_missing_folder() {
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("state/projects.json");
        let (one, two) = (temp.path().join("one"), temp.path().join("two"));
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut picker = ProjectPickerLocation::default();
        choose_project(&mut picker, &one, &history).unwrap();
        let projects = choose_project(&mut picker, &two, &history).unwrap();
        assert_eq!(
            projects,
            [two.canonicalize().unwrap(), one.canonicalize().unwrap()]
        );
        let missing = choose_project(&mut picker, &temp.path().join("gone"), &history);
        assert!(missing.unwrap_err().starts_with("Project folder is unavailable: "));
        assert_eq!(load_project_history_at(&history).unwrap(), projects);
    }

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

impl cxx_qt::Initialize for qobject::AppSettings {
    fn initialize(self: Pin<&mut Self>) {
        self.show_projects(load_project_history());
    }
}

/// Remember `path` as the latest project, refusing a missing folder.
fn choose_project(
    picker: &mut ProjectPickerLocation,
    path: &Path,
    history: &Path,
) -> Result<Vec<PathBuf>, String> {
    if !path.is_dir() {
        return Err(format!("Project folder is unavailable: {}", path.display()));
    }
    let canonical = picker.opened(path)?;
    remember_project_path_at(&canonical, history)
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
        assert!(change("provenance", &serde_json::json!("portable")).is_ok());
        assert!(change("provenance", &serde_json::json!("unknown")).is_err());
        assert!(change("unknown", &serde_json::Value::Null).is_err());
    }
}
