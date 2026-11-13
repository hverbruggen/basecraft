//! Project folders: the recent-projects list in application state, where
//! the project picker opens, choosing a project, and where file dialogs
//! start.
use std::path::{Path, PathBuf};

/// Where a never-saved document is first offered: the chosen project, else
/// the user's Documents folder, else the home folder.
pub fn untitled_folder(project: &str, home: Option<&Path>) -> PathBuf {
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
pub fn dialog_folder(project: &str, document: &str, file: &str, home: Option<&Path>) -> PathBuf {
    if document.starts_with("untitled:") {
        return untitled_folder(project, home);
    }
    let path = if file.is_empty() { document } else { file };
    if path.is_empty() {
        PathBuf::from(project)
    } else {
        Path::new(path)
            .parent()
            .map(Path::to_owned)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod folder_tests {
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
pub struct PickerLocation {
    last_opened: Option<PathBuf>,
}

impl PickerLocation {
    // Session state follows successful folder opens, even when history cannot
    // be saved. Failed opens and merely browsing/cancelling a picker leave it alone.
    pub fn opened(&mut self, path: &Path) -> Result<PathBuf, String> {
        let path = project_directory(path)?;
        self.last_opened = Some(path.clone());
        Ok(path)
    }

    pub fn directory(&self, history: &[PathBuf], home: &Path) -> PathBuf {
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

pub fn load_history(path: &Path) -> Result<Vec<PathBuf>, String> {
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

pub fn remember(path: &Path, file: &Path) -> Result<Vec<PathBuf>, String> {
    let canonical = project_directory(path)?;
    let mut projects = load_history(file)?;
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
mod history_tests {
    use super::*;

    #[test]
    fn choosing_a_project_remembers_it_first_and_refuses_a_missing_folder() {
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("state/projects.json");
        let (one, two) = (temp.path().join("one"), temp.path().join("two"));
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut picker = PickerLocation::default();
        choose(&mut picker, &one, &history).unwrap();
        let projects = choose(&mut picker, &two, &history).unwrap();
        assert_eq!(
            projects,
            [two.canonicalize().unwrap(), one.canonicalize().unwrap()]
        );
        let missing = choose(&mut picker, &temp.path().join("gone"), &history);
        assert!(
            missing
                .unwrap_err()
                .starts_with("Project folder is unavailable: ")
        );
        assert_eq!(load_history(&history).unwrap(), projects);
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
        let picker = PickerLocation::default();
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
        let mut picker = PickerLocation::default();
        picker.opened(&first).unwrap();
        assert!(picker.opened(&temp.path().join("missing")).is_err());
        assert_eq!(
            picker.directory(std::slice::from_ref(&second), &home),
            first.canonicalize().unwrap()
        );
        let damaged = temp.path().join("history.json");
        std::fs::write(&damaged, b"damaged").unwrap();
        let opened = picker.opened(&second).unwrap();
        assert!(remember(&opened, &damaged).is_err());
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
        let picker = PickerLocation::default();
        let home = temp.path().join("home");
        assert_eq!(picker.directory(&load_history(&file).unwrap(), &home), home);
        assert!(!file.exists());
        for bytes in [b"damaged".as_slice(), br#"{"version":99,"projects":[]}"#] {
            std::fs::write(&file, bytes).unwrap();
            assert!(load_history(&file).is_err());
            assert_eq!(
                picker.directory(&load_history(&file).unwrap_or_default(), &home),
                home
            );
            assert_eq!(std::fs::read(&file).unwrap(), bytes);
        }
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(load_history(&file).is_err());
        assert_eq!(
            picker.directory(&load_history(&file).unwrap_or_default(), &home),
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
        remember(&first, &file).unwrap();
        remember(&second, &file).unwrap();
        let projects = remember(&first, &file).unwrap();
        assert_eq!(
            projects,
            vec![
                first.canonicalize().unwrap(),
                second.canonicalize().unwrap()
            ]
        );
        std::fs::write(&file, b"damaged").unwrap();
        assert!(remember(&second, &file).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"damaged");
    }
}

/// Remember `path` as the latest project, refusing a missing folder.
pub fn choose(
    picker: &mut PickerLocation,
    path: &Path,
    history: &Path,
) -> Result<Vec<PathBuf>, String> {
    if !path.is_dir() {
        return Err(format!("Project folder is unavailable: {}", path.display()));
    }
    let canonical = picker.opened(path)?;
    remember(&canonical, history)
}
