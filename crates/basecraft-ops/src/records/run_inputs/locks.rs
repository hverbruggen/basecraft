//! The lock files that keep a save and an analysis from changing the same
//! input records at once.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_platform::files::lexical;
use basecraft_protocol::provenance::FileStamp;
use fs4::fs_std::FileExt;

use super::check::{InputCheck, check_for, inspect, selection};
use super::{RunInput, index, read_input, stamp};
use crate::{Result, error::limit};

/// Lock files only coordinate running processes and hold no data. Deleting
/// the folder while basecraft is closed loses nothing. A key's lock is one
/// of a fixed table of 65,536 files chosen by its hash, so the folder never grows
/// past it; keys that share a file wait for each other.
fn lock_path(key: &Path) -> Result<PathBuf> {
    let paths = basecraft_platform::paths::AppPaths::discover()?;
    let key = basecraft_io::hash_bytes(key.as_os_str().as_encoded_bytes());
    Ok(paths.state.join("locks").join(&key["b3:".len()..][..4]))
}

pub(super) fn open_lock(path: &Path) -> Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?)
}

/// The lock files this scope holds, all exclusively: a read is often
/// followed by a write, and a shared hold cannot be raised safely. Store
/// keys are only taken for store-level changes and by saves, up front.
#[derive(Default)]
struct Scope {
    held: std::cell::RefCell<BTreeMap<PathBuf, std::fs::File>>,
    /// The project of the session this scope works for (rule 2.5): input record
    /// writes inside it index inputs at the project's root.
    project: std::cell::RefCell<Option<PathBuf>>,
}
type HeldLocks = std::rc::Rc<Scope>;
thread_local! {
    static HELD_LOCKS: std::cell::RefCell<std::rc::Weak<Scope>> = const { std::cell::RefCell::new(std::rc::Weak::new()) };
}
const RESERVATION_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
const BACKGROUND_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
thread_local! {
    static BACKGROUND: std::cell::RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
        const { std::cell::RefCell::new(None) };
}

/// While alive, reservations on this thread wait up to a minute instead of
/// 2 s, and stop early once `cancelled` is set. For background work such as
/// publishing analysis results; a save a person makes keeps the short wait.
pub struct BackgroundWait(());

pub fn wait_in_background(
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> BackgroundWait {
    BACKGROUND.with(|slot| *slot.borrow_mut() = Some(cancelled));
    BackgroundWait(())
}

impl Drop for BackgroundWait {
    fn drop(&mut self) {
        BACKGROUND.with(|slot| *slot.borrow_mut() = None);
    }
}

/// A thread-confined mutation scope; nested operations reuse it. Cooperating writers hold keys briefly, so
/// a reservation waits up to 2 s (a minute in the background); contention then
/// ends in Busy, never deadlock.
pub struct Locks(HeldLocks);

/// While this thread holds locks, add `run`'s record to them.
pub(super) fn hold_record(run: &Path) -> Result<()> {
    if let Some(held) = HELD_LOCKS.with(|slot| slot.borrow().upgrade()) {
        Locks(held).record(run)?;
    }
    Ok(())
}

pub fn lock() -> Result<Locks> {
    Ok(HELD_LOCKS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let held = slot.upgrade().unwrap_or_default();
        *slot = std::rc::Rc::downgrade(&held);
        Locks(held)
    }))
}

/// A scope working for the session's `project`; nested writers on this
/// thread index their inputs at that project's root.
pub fn lock_in(project: Option<&crate::project::Project>) -> Result<Locks> {
    let locks = lock()?;
    locks.set_project(project.map(crate::project::Project::root));
    Ok(locks)
}

impl Locks {
    pub(super) fn set_project(&self, project: Option<&Path>) {
        if let Some(project) = project {
            *self.0.project.borrow_mut() = Some(project.into());
        }
    }
    pub(super) fn project(&self) -> Option<PathBuf> {
        self.0.project.borrow().clone()
    }
    pub(super) fn record(&self, directory: &Path) -> Result<()> {
        let directory = lexical(&std::path::absolute(directory)?);
        self.reserve([directory])
    }
    pub(super) fn input(
        &self,
        directory: &Path,
        input: &RunInput,
        seen: &mut std::collections::BTreeSet<PathBuf>,
        sources: &mut std::collections::BTreeSet<PathBuf>,
    ) -> Result<()> {
        self.record(directory)?;
        if !seen.insert(directory.into()) {
            return Ok(());
        }
        if seen.len() > 32 {
            return Err(limit("input ancestry", 32));
        }
        if let Some(source) = &input.source {
            let path = lexical(&directory.join(&source.path));
            self.source(&path)?;
            sources.extend(index::spellings(&path));
        }
        let mut dependencies = Vec::new();
        dependencies.extend(input.origin_run.as_deref().map(Path::new));
        dependencies.extend(input.analysis_run.as_deref().map(Path::new));
        dependencies.extend(input.upstream.iter().map(|link| link.bundle.as_path()));
        for relative in dependencies {
            let path = lexical(&directory.join(relative));
            match read_input(&path) {
                Ok(input) => self.input(&path, &input, seen, sources)?,
                Err(error @ crate::Error::Busy(_)) => return Err(error),
                // A damaged ancestor is reported by the saves that read it.
                Err(_) => {}
            }
        }
        Ok(())
    }
    fn reserve(&self, paths: impl IntoIterator<Item = PathBuf>) -> Result<()> {
        let mut held = self.0.held.borrow_mut();
        // In lock-file order, so two scopes never wait for each other in turn.
        let mut locks = BTreeMap::new();
        for path in paths {
            let path = if path.is_absolute() {
                path.canonicalize().unwrap_or(path)
            } else {
                path
            };
            locks.insert(lock_path(&path)?, path);
        }
        for (lock, path) in locks {
            if held.contains_key(&lock) {
                continue;
            }
            let file = open_lock(&lock)?;
            Self::acquire(&file, &path)?;
            held.insert(lock, file);
        }
        Ok(())
    }
    fn acquire(file: &std::fs::File, path: &Path) -> Result<()> {
        let started = std::time::Instant::now();
        let background = BACKGROUND.with(|slot| slot.borrow().clone());
        let limit = if background.is_some() {
            BACKGROUND_WAIT
        } else {
            RESERVATION_WAIT
        };
        loop {
            if file.try_lock_exclusive()? {
                return Ok(());
            }
            if background
                .as_ref()
                .is_some_and(|cancelled| cancelled.load(std::sync::atomic::Ordering::Acquire))
            {
                return Err(crate::Error::Busy(format!(
                    "cancelled while waiting for {}",
                    path.display()
                )));
            }
            if started.elapsed() > limit {
                return Err(crate::Error::Busy(format!(
                    "an analysis or save is using {}; retry shortly",
                    path.display()
                )));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    /// A file's keys: its path, which stays the same when a save replaces
    /// the file or creates it, and while it exists its inode, which every
    /// other name of it shares. Both go in one reservation, in lock order.
    pub(crate) fn source(&self, path: &Path) -> Result<()> {
        let mut keys = vec![
            crate::records::save_guard::target_path(path)
                .or_else(|_| std::path::absolute(path).map(|path| lexical(&path)))?,
        ];
        if let Ok(FileStamp {
            device: Some(device),
            inode: Some(inode),
            ..
        }) = stamp(path)
        {
            keys.push(PathBuf::from(format!("inode:{device}:{inode}")));
        }
        self.reserve(keys)
    }
}

pub(crate) struct SaveLock {
    _locks: Locks,
    check: Option<InputCheck>,
    project: Option<PathBuf>,
}

impl SaveLock {
    pub(crate) fn check(&mut self, target: &Path) -> Result<InputCheck> {
        let project = self.project.clone();
        self.check
            .take()
            .map_or_else(|| check_for(target, project.as_deref()), Ok)
    }
}

/// Reserve, then select and inspect; the check is carried into the save.
/// `writes` are stores this save will change (its history bundle's store);
/// they are reserved first, then the target, then the stores selected for it.
pub(crate) fn lock_save(
    target: &Path,
    writes: &[PathBuf],
    project: Option<&Path>,
) -> Result<SaveLock> {
    let locks = lock()?;
    locks.set_project(project);
    // Holding the target's key stops any new reference to it, so the stores
    // the index lists for it stay the right set.
    locks.reserve(writes.iter().cloned())?;
    locks.source(target)?;
    let (canonical, selected, inspection) = selection(target, project)?;
    locks.reserve(selected.iter().cloned())?;
    let check = inspect(&canonical, &selected, inspection)?;
    Ok(SaveLock {
        _locks: locks,
        check: Some(check),
        project: project.map(Path::to_path_buf),
    })
}
