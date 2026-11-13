//! The project the user chose in this window: the one place that holds it.
//! Only the project picker changes it; opening a file never does. Every
//! session the window makes takes a clone of it, so its use count says
//! whether documents or jobs still work in it.
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use basecraft_ops::{Session, project::Project};

static CHOSEN: Mutex<Option<Project>> = Mutex::new(None);

fn held() -> MutexGuard<'static, Option<Project>> {
    CHOSEN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Choose `folder` (empty for none). Refused while a session or job still
/// works in the current project: its saves would go to the old project's
/// history while every reader looks in the new one. Close those documents,
/// after asking about unsaved work, and choose again.
pub(crate) fn choose(folder: &str) -> Result<(), String> {
    let next = (!folder.is_empty())
        .then(|| Project::new(folder))
        .transpose()
        .map_err(|_| format!("Project folder is unavailable: {folder}"))?;
    let mut held = held();
    if *held == next {
        return Ok(());
    }
    if let Some(current) = held.as_ref()
        && current.in_use()
    {
        return Err(format!(
            "Close the documents open in {} before choosing another project",
            current.root().display()
        ));
    }
    *held = next;
    Ok(())
}

/// Choose `folder` after a project switch closed every tab. Qt deletes a
/// removed tab, and then its controller with its session, on later turns of
/// the window's event loop, so a refusal is tried again on each of the next
/// `turns` turns before `done` hears it.
pub(crate) fn choose_after_close<T, O: crate::jobs::Owner<T>>(
    owner: &O,
    folder: String,
    turns: u32,
    done: impl FnOnce(std::pin::Pin<&mut T>, Result<(), String>) + Send + 'static,
) {
    let again = owner.clone();
    owner.deliver(move |this| match choose(&folder) {
        Err(_) if turns > 0 => choose_after_close(&again, folder, turns - 1, done),
        result => done(this, result),
    });
}

/// The chosen project. A chosen folder that has gone missing is an error,
/// never a reason to read or write somewhere else.
pub(crate) fn chosen() -> Result<Option<Project>, String> {
    let held = held();
    match held.as_ref() {
        Some(project) if !project.root().is_dir() => Err(format!(
            "The chosen project folder is missing: {}",
            project.root().display()
        )),
        project => Ok(project.cloned()),
    }
}

/// A recoverable session for `file`, in the chosen project if there is one.
pub(crate) fn session_for(file: &Path) -> Result<Session, String> {
    let project = chosen()?;
    let root = project.as_ref().map_or_else(
        || crate::bridge::file_folder(file),
        |p| p.root().to_path_buf(),
    );
    Ok(Session::recoverable(root)
        .map_err(|e| e.to_string())?
        .in_project(project))
}

#[cfg(test)]
mod tests {
    /// Both tests change the window's one chosen project.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[derive(Clone)]
    struct Window;
    struct Probe;
    impl crate::jobs::Owner<Probe> for Window {
        fn deliver(&self, call: impl FnOnce(std::pin::Pin<&mut Probe>) + Send + 'static) -> bool {
            call(std::pin::Pin::new(&mut Probe));
            true
        }
    }

    /// A view opening its own file, here a species workspace in a project
    /// subfolder, gets the chosen project: its history goes to the project's
    /// records and the open session counts as a use of the project.
    #[test]
    fn a_view_opening_its_own_file_gets_the_chosen_project() {
        let _serial = serial();
        let project = tempfile::tempdir().unwrap();
        let sub = project.path().join("analyses");
        std::fs::create_dir(&sub).unwrap();
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/species-delimitation");
        for file in ["hypotheses.spart", "specimens.afa"] {
            std::fs::copy(fixtures.join(file), sub.join(file)).unwrap();
        }
        super::choose(project.path().to_str().unwrap()).unwrap();
        let (sender, received) = std::sync::mpsc::channel();
        crate::document_open::start_in_chosen(
            Window,
            sub.join("hypotheses.spart").to_string_lossy().into_owned(),
            move |_, result| sender.send(result.unwrap()).unwrap(),
        )
        .unwrap();
        let candidate = received
            .recv_timeout(std::time::Duration::from_secs(60))
            .unwrap();
        assert_eq!(
            candidate.session.project().unwrap().root(),
            project.path().canonicalize().unwrap()
        );
        assert_eq!(
            candidate.session.root(),
            project.path().canonicalize().unwrap()
        );
        let other = tempfile::tempdir().unwrap();
        assert!(super::choose(other.path().to_str().unwrap()).is_err());
        drop(candidate);
        super::choose("").unwrap();
    }

    /// The closed tabs' sessions end a turn or two after the tabs go; the
    /// choice waits those turns instead of reporting the project in use.
    #[test]
    fn a_choice_after_closing_waits_for_the_closed_sessions() {
        type Turn = Box<dyn FnOnce(std::pin::Pin<&mut Probe>) + Send>;
        #[derive(Clone)]
        struct Queue(std::sync::Arc<std::sync::Mutex<Vec<Turn>>>);
        impl crate::jobs::Owner<Probe> for Queue {
            fn deliver(
                &self,
                call: impl FnOnce(std::pin::Pin<&mut Probe>) + Send + 'static,
            ) -> bool {
                self.0.lock().unwrap().push(Box::new(call));
                true
            }
        }
        let _serial = serial();
        let turn = |queue: &Queue| {
            let calls: Vec<Turn> = std::mem::take(&mut *queue.0.lock().unwrap());
            for call in calls {
                call(std::pin::Pin::new(&mut Probe));
            }
        };
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        super::choose(first.path().to_str().unwrap()).unwrap();
        let session = super::session_for(&first.path().join("a.fasta")).unwrap();
        let queue = Queue(std::sync::Arc::default());
        let (sender, result) = std::sync::mpsc::channel();
        super::choose_after_close(
            &queue,
            second.path().to_string_lossy().into_owned(),
            10,
            move |_, chosen| sender.send(chosen).unwrap(),
        );
        turn(&queue);
        turn(&queue);
        assert!(
            result.try_recv().is_err(),
            "a refusal was reported while turns remain"
        );
        drop(session);
        turn(&queue);
        assert_eq!(result.try_recv().unwrap(), Ok(()));
        assert_eq!(
            super::chosen().unwrap().unwrap().root(),
            second.path().canonicalize().unwrap()
        );
        // With every turn used, the refusal is reported.
        let held = super::session_for(&second.path().join("b.fasta")).unwrap();
        let (sender, result) = std::sync::mpsc::channel();
        super::choose_after_close(
            &queue,
            first.path().to_string_lossy().into_owned(),
            2,
            move |_, chosen| sender.send(chosen).unwrap(),
        );
        for _ in 0..3 {
            turn(&queue);
        }
        assert!(
            result
                .try_recv()
                .unwrap()
                .unwrap_err()
                .starts_with("Close the documents")
        );
        drop(held);
        super::choose("").unwrap();
    }

    #[test]
    fn switching_waits_until_the_old_project_is_no_longer_in_use() {
        let _serial = serial();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_path = first.path().to_str().unwrap();
        super::choose(first_path).unwrap();
        let session = super::session_for(&first.path().join("a.fasta")).unwrap();
        let error = super::choose(second.path().to_str().unwrap()).unwrap_err();
        assert!(error.starts_with("Close the documents"), "{error}");
        // Choosing the same folder again is not a switch.
        super::choose(first_path).unwrap();
        drop(session);
        super::choose(second.path().to_str().unwrap()).unwrap();
        assert_eq!(
            super::chosen().unwrap().unwrap().root(),
            second.path().canonicalize().unwrap()
        );
        super::choose("").unwrap();
    }
}
