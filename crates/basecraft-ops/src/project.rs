//! The project is the folder the user chose. Basecraft never works it out
//! from the files: a file inside the folder belongs to it, any other file is
//! a loose file.
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Clones share one use count, so whoever holds the chosen project can tell
/// whether an open session or a running job still works in it.
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    uses: Arc<()>,
}

impl PartialEq for Project {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}
impl Eq for Project {}

impl Project {
    /// The folder the user chose. It must exist.
    pub fn new(folder: impl AsRef<Path>) -> crate::Result<Self> {
        let root = folder.as_ref().canonicalize()?;
        if !root.is_dir() {
            return Err(crate::error::invalid("a project must be a folder"));
        }
        Ok(Self {
            root,
            uses: Arc::default(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What a file's run links are relative to: the root of `project` when it
    /// holds the file, else the file's own folder.
    pub fn base_of(project: Option<&Self>, file: &Path) -> crate::Result<PathBuf> {
        match project.filter(|project| project.contains(file)) {
            Some(project) => Ok(project.root.clone()),
            None => std::path::absolute(file)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| crate::error::invalid("the file has no folder")),
        }
    }

    /// The `.basecraft` that holds a file's records, and the file's key in
    /// it (D23): the project's, with the project-relative path, when the
    /// project holds the file; else the one beside the loose file, with its
    /// name.
    fn records_of(project: Option<&Self>, file: &Path) -> (PathBuf, PathBuf) {
        let held =
            project.and_then(|project| Some((project.root.clone(), project.relative(file)?)));
        let (base, key) = held.unwrap_or_else(|| {
            let absolute = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
            let base = absolute
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            (
                base,
                PathBuf::from(absolute.file_name().unwrap_or_default()),
            )
        });
        (base.join(".basecraft"), key)
    }

    /// Where basecraft keeps its record of one document (D23): the
    /// `.basecraft` that holds the file's records, at `documents/<key>.json`.
    /// Nothing is written beside the document itself.
    pub fn document_record(project: Option<&Self>, file: &Path) -> PathBuf {
        Self::document_record_with(project, file, ".json")
    }

    /// The same place, for another record of the document named by
    /// `suffix`, such as `.alignment.json` for its alignment link.
    pub fn document_record_with(project: Option<&Self>, file: &Path, suffix: &str) -> PathBuf {
        let (records, key) = Self::records_of(project, file);
        let mut record = records.join("documents").join(key).into_os_string();
        record.push(suffix);
        PathBuf::from(record)
    }

    /// Where a file's saved history lives: `provenance/<hash of its key>` in
    /// the `.basecraft` that holds its records.
    pub fn provenance_record(project: Option<&Self>, file: &Path) -> PathBuf {
        let (records, key) = Self::records_of(project, file);
        records.join("provenance").join(
            basecraft_io::hash_bytes(key.to_string_lossy().as_bytes()).trim_start_matches("b3:"),
        )
    }

    /// Whether another clone of this project is alive, such as the project
    /// of an open session.
    pub fn in_use(&self) -> bool {
        Arc::strong_count(&self.uses) > 1
    }

    /// Whether `path` lies inside the project. A missing file counts by its
    /// folder, so a Save As destination is placed like an existing file.
    pub fn contains(&self, path: &Path) -> bool {
        self.relative(path).is_some()
    }

    fn relative(&self, path: &Path) -> Option<PathBuf> {
        let absolute = std::path::absolute(path).ok()?;
        let canonical = absolute.canonicalize().ok().or_else(|| {
            let parent = absolute.parent()?.canonicalize().ok()?;
            Some(parent.join(absolute.file_name()?))
        })?;
        canonical
            .strip_prefix(&self.root)
            .ok()
            .map(Path::to_path_buf)
    }

    /// Where a result inside the project keeps its history.
    pub fn provenance_bundle(&self, result: &Path) -> Option<PathBuf> {
        self.contains(result)
            .then(|| Self::provenance_record(Some(self), result))
    }
}

#[cfg(test)]
mod tests {
    use super::Project;

    #[test]
    fn membership_is_the_chosen_folder_only() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let sub = project.join("analysis/.basecraft");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.path().join("loose.fasta"), b">a\nA\n").unwrap();
        let project = Project::new(&project).unwrap();
        assert!(project.contains(&project.root().join("analysis/new.fasta")));
        assert!(!project.contains(&root.path().join("loose.fasta")));
        assert!(
            project
                .provenance_bundle(&root.path().join("loose.fasta"))
                .is_none()
        );
        assert!(Project::new(root.path().join("missing")).is_err());
    }
}
