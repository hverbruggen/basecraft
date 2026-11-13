//! Application locations. Discovery is read-only; callers create directories on demand.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub scratch: PathBuf,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot discover application paths: {0}")]
pub struct PathError(pub String);

impl AppPaths {
    pub fn discover() -> Result<Self, PathError> {
        Self::from_environment(|key| std::env::var_os(key))
    }

    /// Environment injection avoids mutating process-global variables in tests.
    pub fn from_environment(
        env: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Self, PathError> {
        let absolute = |key: &str| -> Result<Option<PathBuf>, PathError> {
            env(key)
                .map(PathBuf::from)
                .map(|path| {
                    if path.is_absolute() {
                        Ok(path)
                    } else {
                        Err(PathError(format!("{key} must be an absolute path")))
                    }
                })
                .transpose()
        };
        if let Some(profile) = absolute("BASECRAFT_PROFILE_DIR")? {
            return Ok(Self {
                config: profile.join("config"),
                data: profile.join("data"),
                state: absolute("BASECRAFT_STATE_DIR")?.unwrap_or_else(|| profile.join("state")),
                cache: absolute("BASECRAFT_CACHE_DIR")?.unwrap_or_else(|| profile.join("cache")),
                scratch: absolute("BASECRAFT_SCRATCH_DIR")?.unwrap_or_else(std::env::temp_dir),
            });
        }
        let home =
            absolute(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?.ok_or_else(|| {
                PathError("home directory is unavailable; set BASECRAFT_PROFILE_DIR".into())
            })?;
        let (config, data, state, cache) = if cfg!(windows) {
            let roaming = absolute("APPDATA")?.unwrap_or_else(|| home.join("AppData/Roaming"));
            let local = absolute("LOCALAPPDATA")?.unwrap_or_else(|| home.join("AppData/Local"));
            (
                roaming.join("Basecraft"),
                local.join("Basecraft/data"),
                local.join("Basecraft/state"),
                local.join("Basecraft/cache"),
            )
        } else if cfg!(target_os = "macos") {
            let support = home.join("Library/Application Support/Basecraft");
            (
                support.join("config"),
                support.join("data"),
                support.join("state"),
                home.join("Library/Caches/Basecraft"),
            )
        } else {
            (
                absolute("XDG_CONFIG_HOME")?
                    .unwrap_or_else(|| home.join(".config"))
                    .join("basecraft"),
                absolute("XDG_DATA_HOME")?
                    .unwrap_or_else(|| home.join(".local/share"))
                    .join("basecraft"),
                absolute("XDG_STATE_HOME")?
                    .unwrap_or_else(|| home.join(".local/state"))
                    .join("basecraft"),
                absolute("XDG_CACHE_HOME")?
                    .unwrap_or_else(|| home.join(".cache"))
                    .join("basecraft"),
            )
        };
        Ok(Self {
            config,
            data,
            state: absolute("BASECRAFT_STATE_DIR")?.unwrap_or(state),
            cache: absolute("BASECRAFT_CACHE_DIR")?.unwrap_or(cache),
            scratch: absolute("BASECRAFT_SCRATCH_DIR")?.unwrap_or_else(std::env::temp_dir),
        })
    }

    pub fn preferences(&self) -> PathBuf {
        self.config.join("preferences.toml")
    }

    /// The converted copies of analysis inputs that views display. The
    /// operating system may clear it; basecraft rebuilds what it needs.
    pub fn display_cache(&self) -> PathBuf {
        self.cache.join("analysis-inputs")
    }

    /// Delete cache folders older builds wrote and nothing reads any more.
    pub fn remove_old_caches(&self) {
        for name in ["index", "hash", "display-trees", "activity"] {
            let _ = std::fs::remove_dir_all(self.cache.join(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_is_lazy_and_overrides_are_native_paths() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("profile");
        let paths = AppPaths::from_environment(|key| match key {
            "BASECRAFT_PROFILE_DIR" => Some(profile.clone().into_os_string()),
            "BASECRAFT_STATE_DIR" => Some(temp.path().join("state override").into_os_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!(paths.preferences(), profile.join("config/preferences.toml"));
        assert_eq!(paths.state, temp.path().join("state override"));
        assert!(!profile.exists());
    }
    #[test]
    fn old_caches_go_and_the_display_cache_stays() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_environment(|key| {
            (key == "BASECRAFT_PROFILE_DIR").then(|| temp.path().as_os_str().into())
        })
        .unwrap();
        for name in [
            "index",
            "hash",
            "display-trees",
            "activity",
            "analysis-inputs",
        ] {
            std::fs::create_dir_all(paths.cache.join(name)).unwrap();
            std::fs::write(paths.cache.join(name).join("entry"), b"x").unwrap();
        }
        paths.remove_old_caches();
        let left: Vec<_> = std::fs::read_dir(&paths.cache)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(left, [paths.display_cache()]);
        paths.remove_old_caches();
    }
    #[test]
    fn relative_overrides_are_rejected() {
        assert!(
            AppPaths::from_environment(
                |key| (key == "BASECRAFT_PROFILE_DIR").then(|| "relative".into())
            )
            .is_err()
        );
    }
}
