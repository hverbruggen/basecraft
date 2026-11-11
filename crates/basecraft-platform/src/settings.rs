//! Versioned, readable preferences. Reads never create files; explicit updates
//! lock and reload the latest file before merging just the requested fields.
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("preferences are being changed by another process; retry shortly")]
    Busy,
    #[error("preferences I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid preferences: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub schema_version: u32,
    pub revision: u64,
    pub ui: Ui,
    pub files: Files,
    pub compute: Compute,
    pub customization: Customization,
    /// Contact details written to archive submission templates.
    pub submitter: Submitter,
    /// Email addresses for other services, such as NCBI BLAST.
    pub contacts: Contacts,
}

/// Email addresses for services other than GenBank submission. A service
/// without its own address uses the submitter's email.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Contacts {
    pub service_emails: BTreeMap<String, String>,
}

impl Preferences {
    /// The email to give `service`: its own, else the submitter's.
    pub fn service_email(&self, service: &str) -> &str {
        self.contacts
            .service_emails
            .get(service)
            .filter(|email| !email.is_empty())
            .unwrap_or(&self.submitter.email)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // independent on/off preferences
pub struct Ui {
    /// Optional data palette; its light/dark variant follows the active theme.
    pub data_color_scheme: DataColorScheme,
    pub show_gc: bool,
    pub sidebar_open: bool,
    pub orf_min_codons: u32,
    pub alignment_insert: bool,
    /// None uses the compact platform-relative default; explicit values are points.
    pub font_size_points: Option<u32>,
    /// Independent ribbon icon, text and layout scale, in percent.
    pub ribbon_size_percent: u32,
    /// Side of a tick box, in pixels.
    pub tick_box_size: u32,
    /// Height of number and text boxes, in pixels.
    pub input_box_height: u32,
    /// Light, dark, or the desktop's theme.
    pub theme: ThemeChoice,
    /// Tell the user when a new release is out; asked at first launch.
    pub release_notices: bool,
    /// The first-launch questions have been answered.
    pub welcomed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeChoice {
    #[default]
    FollowDesktop,
    Light,
    Dark,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            data_color_scheme: DataColorScheme::default(),
            show_gc: true,
            sidebar_open: true,
            orf_min_codons: 100,
            alignment_insert: true,
            font_size_points: None,
            ribbon_size_percent: 100,
            tick_box_size: 14,
            input_box_height: 20,
            theme: ThemeChoice::FollowDesktop,
            release_notices: false,
            welcomed: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DataColorScheme {
    #[default]
    DesktopTheme,
    ColorBlindFriendly,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenancePolicy {
    #[default]
    Portable,
    LocalOnly,
    None,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Files {
    pub provenance: ProvenancePolicy,
    pub recovery: bool,
}
impl Default for Files {
    fn default() -> Self {
        Self {
            provenance: ProvenancePolicy::Portable,
            recovery: true,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Compute {
    pub local_cpu_threads: u32,
}
impl Default for Compute {
    fn default() -> Self {
        Self {
            local_cpu_threads: 4,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Customization {
    /// Stable command IDs; unknown IDs remain available after plugin removal.
    pub shortcuts: BTreeMap<String, String>,
    pub ribbon_hidden: Vec<String>,
    /// Ribbon access keys, stored beside direct shortcut overrides.
    pub key_hints: BTreeMap<String, String>,
}
/// The person submitting sequences and their affiliation, entered once
/// and used for every submission (`.sbt` contact). Empty fields are unset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Submitter {
    pub first_name: String,
    pub middle_initials: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub institution: String,
    pub department: String,
    pub street: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}
impl Submitter {
    /// Field names and values, in form order.
    pub fn fields(&self) -> [(&'static str, &str); 12] {
        [
            ("first_name", &self.first_name),
            ("middle_initials", &self.middle_initials),
            ("last_name", &self.last_name),
            ("email", &self.email),
            ("phone", &self.phone),
            ("institution", &self.institution),
            ("department", &self.department),
            ("street", &self.street),
            ("city", &self.city),
            ("state", &self.state),
            ("postal_code", &self.postal_code),
            ("country", &self.country),
        ]
    }
    /// Required fields that are empty.
    pub fn missing(&self) -> Vec<&'static str> {
        self.fields()
            .into_iter()
            .filter(|(name, value)| {
                value.is_empty()
                    && matches!(
                        *name,
                        "first_name" | "last_name" | "email" | "institution" | "city" | "country"
                    )
            })
            .map(|(name, _)| name)
            .collect()
    }
    fn validate(&self) -> Result<(), Error> {
        for (name, value) in self.fields() {
            if value.len() > 200 || value.chars().any(char::is_control) || value.trim() != value {
                return Err(Error::Invalid(format!(
                    "submitter {name} must be at most 200 characters, without line breaks or surrounding spaces"
                )));
            }
        }
        if !self.email.is_empty() && !self.email.contains('@') {
            return Err(Error::Invalid("submitter email needs an @".into()));
        }
        Ok(())
    }
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            ui: Ui::default(),
            files: Files::default(),
            compute: Compute::default(),
            customization: Customization::default(),
            submitter: Submitter::default(),
            contacts: Contacts::default(),
        }
    }
}
impl Preferences {
    /// Resolve explicit session/command overrides above user preferences without
    /// changing the stored profile. Project analysis defaults belong to project
    /// metadata and cannot override machine/UI preferences through this API.
    pub fn resolve_session(&self, overrides: &[Change]) -> Result<Self, Error> {
        let mut effective = self.clone();
        for change in overrides {
            change.validate()?;
            change.clone().apply(&mut effective);
        }
        effective.validate()?;
        Ok(effective)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(Error::Invalid(format!(
                "unsupported schema version {}; original file retained",
                self.schema_version
            )));
        }
        if !(1..=1_000_000).contains(&self.ui.orf_min_codons) {
            return Err(Error::Invalid(
                "ORF minimum must be between 1 and 1000000 codons".into(),
            ));
        }
        if self
            .ui
            .font_size_points
            .is_some_and(|size| !(6..=32).contains(&size))
        {
            return Err(Error::Invalid(
                "Interface font size must be between 6 and 32 points".into(),
            ));
        }
        if !(100..=200).contains(&self.ui.ribbon_size_percent) {
            return Err(Error::Invalid(
                "Ribbon size must be between 100 and 200 percent".into(),
            ));
        }
        if !(8..=40).contains(&self.ui.tick_box_size) {
            return Err(Error::Invalid(
                "Tick box size must be between 8 and 40 pixels".into(),
            ));
        }
        if !(14..=60).contains(&self.ui.input_box_height) {
            return Err(Error::Invalid(
                "Input box height must be between 14 and 60 pixels".into(),
            ));
        }
        if !(1..=65536).contains(&self.compute.local_cpu_threads) {
            return Err(Error::Invalid(
                "CPU threads must be between 1 and 65536".into(),
            ));
        }
        self.submitter.validate()?;
        if self.customization.shortcuts.len() > 4096
            || self.customization.ribbon_hidden.len() > 4096
            || self.customization.key_hints.len() > 4096
        {
            return Err(Error::Invalid("too many customization entries".into()));
        }
        for id in self
            .customization
            .shortcuts
            .keys()
            .chain(self.customization.ribbon_hidden.iter())
            .chain(self.customization.key_hints.keys())
        {
            if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
                return Err(Error::Invalid("invalid command ID".into()));
            }
        }
        for value in self.customization.key_hints.values() {
            if value.len() > 2
                || value.is_empty()
                || !value.bytes().all(|c| c.is_ascii_uppercase())
                || value.starts_with(['J', 'Y', 'Z'])
            {
                return Err(Error::Invalid(
                    "access keys need one or two uppercase letters, without J, Y or Z first".into(),
                ));
            }
        }
        let mut shortcuts = BTreeMap::new();
        let defaults = [
            ("file.new-sequence", "Ctrl+N"),
            ("ribbon.annotate-the-selection", "Ctrl+Shift+N"),
            ("file.open", "Ctrl+O"),
            ("file.save", "Ctrl+S"),
            ("file.save-as", "Ctrl+Shift+S"),
            ("view.text", "Ctrl+Alt+T"),
        ];
        for (id, value) in defaults {
            if !self.customization.shortcuts.contains_key(id) {
                shortcuts.insert(canonical_shortcut(value)?, id);
            }
        }
        for (id, value) in &self.customization.shortcuts {
            let canonical = canonical_shortcut(value)?;
            if canonical.is_empty() {
                continue;
            }
            // Canvas navigation/editing remains local to its focused document.
            // These keys cannot be stolen by an application-wide override.
            let reserved = [
                "F2",
                "Ctrl+A",
                "Ctrl+V",
                "Ctrl+X",
                "Ctrl+Shift+V",
                "Ctrl+Home",
                "Ctrl+End",
                "Ctrl+Delete",
                "Ctrl+Shift+Home",
                "Ctrl+Shift+End",
                "Ctrl+Left",
                "Ctrl+Right",
                "Ctrl+Up",
                "Ctrl+Down",
                "Ctrl+Shift+Left",
                "Ctrl+Shift+Right",
                "F6",
                "Shift+F6",
                "Ctrl+Q",
                "Ctrl+W",
                "Ctrl+B",
                "Ctrl+Z",
                "Ctrl+Shift+Z",
                "Ctrl+Y",
                "Ctrl+C",
                "Ctrl+E",
                "Ctrl+0",
                "Ctrl+=",
                "Ctrl+-",
                "Ctrl+Alt+O",
                "Ctrl+Shift+D",
                "Ctrl+Shift+E",
                "Ctrl+Shift+C",
                "Ctrl+Shift+A",
                "Ctrl+Alt+A",
                "Ctrl+Alt+D",
                "Ctrl+Alt+M",
                "Ctrl+Alt+P",
                "Ctrl+Alt+Home",
                "Ctrl+Alt+End",
                "Alt+Left",
                "Alt+Right",
                "Alt+Return",
                "Alt+Up",
                "Alt+Down",
            ];
            if reserved
                .iter()
                .any(|key| canonical_shortcut(key).is_ok_and(|key| key == canonical))
            {
                return Err(Error::Invalid(format!(
                    "{value} is reserved for document editing or navigation"
                )));
            }
            if let Some(other) = shortcuts.insert(canonical, id.as_str()) {
                return Err(Error::Invalid(format!(
                    "shortcut {value} is already assigned to {other}"
                )));
            }
        }
        Ok(())
    }
}

/// Portable single-chord syntax, independent of Qt and platform locale.
/// Plain text keys are excluded to preserve typing in scientific documents.
pub fn canonical_shortcut(value: &str) -> Result<String, Error> {
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.len() > 128 || value.chars().any(char::is_control) {
        return Err(Error::Invalid("invalid shortcut; maximum 128 bytes".into()));
    }
    let parts: Vec<_> = value.split('+').map(str::trim).collect();
    let (key, modifiers) = parts
        .split_last()
        .ok_or_else(|| Error::Invalid("empty shortcut".into()))?;
    let mut key = key.to_ascii_uppercase();
    let mut mask = 0u8;
    for modifier in modifiers {
        let bit = match modifier.to_ascii_lowercase().as_str() {
            "ctrl" => 1,
            "alt" => 2,
            "shift" => 4,
            "meta" => 8,
            _ => {
                return Err(Error::Invalid(
                    "use Ctrl, Alt, Shift or Meta modifiers".into(),
                ));
            }
        };
        if mask & bit != 0 {
            return Err(Error::Invalid("duplicate shortcut modifier".into()));
        }
        mask |= bit;
    }
    let function = key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=35).contains(&n));
    if function
        && let Some(number) = key
            .strip_prefix('F')
            .and_then(|number| number.parse::<u8>().ok())
    {
        key = format!("F{number}");
    }
    let named = [
        "HOME", "END", "LEFT", "RIGHT", "UP", "DOWN", "RETURN", "ENTER", "SPACE", "TAB", "ESC",
        "DELETE", "INSERT", "PGUP", "PGDOWN",
    ]
    .contains(&key.as_str());
    let character = key.len() == 1 && key.as_bytes()[0].is_ascii_graphic();
    if (!function && !named && !character) || (!function && mask & (1 | 2 | 8) == 0) {
        return Err(Error::Invalid(
            "use a function key or a key with Ctrl, Alt or Meta".into(),
        ));
    }
    let mut normalized = String::new();
    for (bit, name) in [(1, "CTRL+"), (2, "ALT+"), (4, "SHIFT+"), (8, "META+")] {
        if mask & bit != 0 {
            normalized.push_str(name);
        }
    }
    normalized.push_str(if key == "ENTER" { "RETURN" } else { &key });
    Ok(normalized)
}

/// Each variant changes one field, preventing stale whole-file replacement.
#[derive(Debug, Clone)]
pub enum Change {
    DataColorScheme(DataColorScheme),
    ShowGc(bool),
    SidebarOpen(bool),
    OrfMinCodons(u32),
    AlignmentInsert(bool),
    FontSizePoints(Option<u32>),
    RibbonSizePercent(u32),
    TickBoxSize(u32),
    InputBoxHeight(u32),
    CpuThreads(u32),
    Provenance(ProvenancePolicy),
    Recovery(bool),
    Shortcut { id: String, value: Option<String> },
    KeyHint { id: String, value: Option<String> },
    RibbonHidden(Vec<String>),
    RibbonCommandHidden { id: String, hidden: bool },
    ResetCustomization,
    Submitter(Box<Submitter>),
    /// `None` makes the service use the submitter's email again.
    ServiceEmail { service: String, email: Option<String> },
    Theme(ThemeChoice),
    ReleaseNotices(bool),
    Welcomed,
}
impl Change {
    fn validate(&self) -> Result<(), Error> {
        if let Self::ServiceEmail { service, email } = self {
            if service.is_empty() || service.len() > 32 || !service.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                return Err(Error::Invalid("invalid service name".into()));
            }
            if let Some(email) = email
                && (!email.contains('@') || email.len() > 200 || email.trim() != email)
            {
                return Err(Error::Invalid("a service email needs an @, without surrounding spaces".into()));
            }
        }
        if let Self::Shortcut { id, .. }
        | Self::KeyHint { id, .. }
        | Self::RibbonCommandHidden { id, .. } = self
            && (id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
        {
            return Err(Error::Invalid("invalid command ID".into()));
        }
        Ok(())
    }
    fn apply(self, prefs: &mut Preferences) {
        match self {
            Self::DataColorScheme(v) => prefs.ui.data_color_scheme = v,
            Self::ShowGc(v) => prefs.ui.show_gc = v,
            Self::SidebarOpen(v) => prefs.ui.sidebar_open = v,
            Self::OrfMinCodons(v) => prefs.ui.orf_min_codons = v,
            Self::AlignmentInsert(v) => prefs.ui.alignment_insert = v,
            Self::FontSizePoints(v) => prefs.ui.font_size_points = v,
            Self::RibbonSizePercent(v) => prefs.ui.ribbon_size_percent = v,
            Self::TickBoxSize(v) => prefs.ui.tick_box_size = v,
            Self::InputBoxHeight(v) => prefs.ui.input_box_height = v,
            Self::CpuThreads(v) => prefs.compute.local_cpu_threads = v,
            Self::Provenance(v) => prefs.files.provenance = v,
            Self::Recovery(v) => prefs.files.recovery = v,
            Self::Shortcut { id, value } => {
                if let Some(v) = value {
                    prefs.customization.shortcuts.insert(id, v);
                } else {
                    prefs.customization.shortcuts.remove(&id);
                }
            }
            Self::KeyHint { id, value } => {
                if let Some(value) = value {
                    prefs.customization.key_hints.insert(id, value);
                } else {
                    prefs.customization.key_hints.remove(&id);
                }
            }
            Self::RibbonHidden(v) => prefs.customization.ribbon_hidden = v,
            Self::RibbonCommandHidden { id, hidden } => {
                prefs
                    .customization
                    .ribbon_hidden
                    .retain(|existing| existing != &id);
                if hidden {
                    prefs.customization.ribbon_hidden.push(id);
                }
            }
            Self::ResetCustomization => prefs.customization = Customization::default(),
            Self::Submitter(v) => prefs.submitter = *v,
            Self::ServiceEmail { service, email: Some(email) } => {
                prefs.contacts.service_emails.insert(service, email);
            }
            Self::ServiceEmail { service, email: None } => {
                prefs.contacts.service_emails.remove(&service);
            }
            Self::Theme(v) => prefs.ui.theme = v,
            Self::ReleaseNotices(v) => prefs.ui.release_notices = v,
            Self::Welcomed => prefs.ui.welcomed = true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}
impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn load(&self) -> Result<Preferences, Error> {
        Ok(self.read()?.0)
    }
    fn read(&self) -> Result<(Preferences, toml::Value), Error> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((
                    Preferences::default(),
                    toml::Value::Table(toml::map::Map::default()),
                ));
            }
            Err(e) => return Err(e.into()),
        };
        Self::parse(file)
    }
    fn parse(file: impl Read) -> Result<(Preferences, toml::Value), Error> {
        let mut text = String::new();
        file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
        if text.len() as u64 > MAX_BYTES {
            return Err(Error::Invalid("file exceeds 1 MiB".into()));
        }
        let raw: toml::Value = toml::from_str(&text).map_err(|e| Error::Invalid(e.to_string()))?;
        if raw.get("schema_version").is_none() {
            return Err(Error::Invalid(
                "no schema_version; original file retained".into(),
            ));
        }
        let prefs: Preferences = raw
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| Error::Invalid(e.to_string()))?;
        prefs.validate()?;
        Ok((prefs, raw))
    }
    pub fn update(&self, change: Change) -> Result<Preferences, Error> {
        change.validate()?;
        self.modify(|prefs| change.apply(prefs))
    }
    fn modify(&self, modify: impl FnOnce(&mut Preferences)) -> Result<Preferences, Error> {
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| Error::Invalid("preferences path must have a parent".into()))?;
        std::fs::create_dir_all(parent)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("toml.lock"))?;
        if !lock.try_lock_exclusive()? {
            return Err(Error::Busy);
        }
        let (mut prefs, mut raw) = self.read()?;
        let before = prefs.clone();
        modify(&mut prefs);
        prefs.validate()?;
        if prefs == before && self.path.exists() {
            return Ok(prefs);
        }
        prefs.revision = prefs
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("revision overflow".into()))?;
        let typed = toml::Value::try_from(&prefs).map_err(|e| Error::Invalid(e.to_string()))?;
        merge(&mut raw, typed);
        if prefs.ui.font_size_points.is_none()
            && let Some(ui) = raw.get_mut("ui").and_then(toml::Value::as_table_mut)
        {
            ui.remove("font_size_points");
        }
        // Map removals must be reflected, while unknown surrounding fields survive.
        if let Some(customization) = raw
            .get_mut("customization")
            .and_then(toml::Value::as_table_mut)
        {
            customization.insert(
                "key_hints".into(),
                toml::Value::try_from(&prefs.customization.key_hints)
                    .map_err(|e| Error::Invalid(e.to_string()))?,
            );
            customization.insert(
                "shortcuts".into(),
                toml::Value::try_from(&prefs.customization.shortcuts)
                    .map_err(|e| Error::Invalid(e.to_string()))?,
            );
        }
        let text = toml::to_string_pretty(&raw).map_err(|e| Error::Invalid(e.to_string()))?;
        if text.len() as u64 > MAX_BYTES {
            return Err(Error::Invalid("serialized file exceeds 1 MiB".into()));
        }
        let directory = self.path.parent().unwrap_or(Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        staged.write_all(text.as_bytes())?;
        staged.as_file().sync_all()?;
        staged.persist(&self.path).map_err(|error| error.error)?;
        Ok(prefs)
    }
}
fn merge(destination: &mut toml::Value, source: toml::Value) {
    if let (toml::Value::Table(dest), toml::Value::Table(src)) = (&mut *destination, &source) {
        for (key, value) in src {
            if let Some(existing) = dest.get_mut(key) {
                merge(existing, value.clone());
            } else {
                dest.insert(key.clone(), value.clone());
            }
        }
    } else {
        *destination = source;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_sequence_annotation_shortcuts_remap_and_detect_conflicts() {
        let mut prefs = Preferences::default();
        prefs
            .customization
            .shortcuts
            .insert("ribbon.annotate-the-selection".into(), "Ctrl+J".into());
        assert!(prefs.validate().is_ok());
        prefs
            .customization
            .shortcuts
            .insert("file.new-sequence".into(), "Ctrl+J".into());
        assert!(prefs.validate().is_err());
        prefs
            .customization
            .shortcuts
            .insert("file.new-sequence".into(), "Ctrl+Shift+N".into());
        assert!(prefs.validate().is_ok());
        prefs
            .customization
            .shortcuts
            .insert("ribbon.annotate-the-selection".into(), "Ctrl+N".into());
        assert!(prefs.validate().is_ok());
    }

    #[test]
    fn access_keys_migrate_preserve_and_reset() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.toml");
        let store = SettingsStore::new(path);
        let before = store.load().unwrap();
        assert!(before.customization.key_hints.is_empty());
        store
            .update(Change::KeyHint {
                id: "file.save".into(),
                value: Some("SV".into()),
            })
            .unwrap();
        assert_eq!(
            store.load().unwrap().customization.key_hints["file.save"],
            "SV"
        );
        assert!(
            store
                .update(Change::KeyHint {
                    id: "file.save".into(),
                    value: Some("ZA".into())
                })
                .is_err()
        );
        assert_eq!(
            store.load().unwrap().customization.key_hints["file.save"],
            "SV"
        );
        store
            .update(Change::KeyHint {
                id: "file.save".into(),
                value: None,
            })
            .unwrap();
        assert!(store.load().unwrap().customization.key_hints.is_empty());
        store
            .update(Change::KeyHint {
                id: "missing.plugin".into(),
                value: Some("PX".into()),
            })
            .unwrap();
        store.update(Change::ResetCustomization).unwrap();
        assert!(store.load().unwrap().customization.key_hints.is_empty());
    }
    #[test]
    fn data_palette_preference_round_trips_and_preserves_unknown_settings() {
        let temp = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(temp.path().join("preferences.toml"));
        assert_eq!(
            store.load().unwrap().ui.data_color_scheme,
            DataColorScheme::DesktopTheme
        );
        assert!(
            !store.path.exists(),
            "Viewing defaults must not write settings"
        );
        std::fs::write(
            &store.path,
            "schema_version = 1\n[ui]\nfuture_setting = 17\n",
        )
        .unwrap();
        store
            .update(Change::DataColorScheme(DataColorScheme::ColorBlindFriendly))
            .unwrap();
        assert_eq!(
            SettingsStore::new(store.path.clone())
                .load()
                .unwrap()
                .ui
                .data_color_scheme,
            DataColorScheme::ColorBlindFriendly
        );
        store.update(Change::ShowGc(false)).unwrap();
        assert_eq!(
            store.load().unwrap().ui.data_color_scheme,
            DataColorScheme::ColorBlindFriendly
        );
        let raw: toml::Value =
            toml::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(raw["ui"]["future_setting"].as_integer(), Some(17));
        store
            .update(Change::DataColorScheme(DataColorScheme::DesktopTheme))
            .unwrap();
        assert_eq!(
            store.load().unwrap().ui.data_color_scheme,
            DataColorScheme::DesktopTheme
        );
        std::fs::write(
            &store.path,
            "schema_version = 1\n[ui]\ndata_color_scheme = 'unknown'\n",
        )
        .unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn service_emails_default_to_the_submitter_and_can_differ() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        let submitter = Submitter {
            email: "jane@example.org".into(),
            ..Submitter::default()
        };
        store.update(Change::Submitter(Box::new(submitter))).unwrap();
        assert_eq!(store.load().unwrap().service_email("blast"), "jane@example.org");
        let lab = Change::ServiceEmail {
            service: "blast".into(),
            email: Some("lab@example.org".into()),
        };
        assert_eq!(store.update(lab).unwrap().service_email("blast"), "lab@example.org");
        assert_eq!(store.load().unwrap().service_email("zenodo"), "jane@example.org");
        for bad in [("blast", Some("no-at")), ("Bad Name", Some("a@b.org"))] {
            let change = Change::ServiceEmail {
                service: bad.0.into(),
                email: bad.1.map(Into::into),
            };
            assert!(store.update(change).is_err());
        }
        let reset = Change::ServiceEmail {
            service: "blast".into(),
            email: None,
        };
        assert_eq!(store.update(reset).unwrap().service_email("blast"), "jane@example.org");
    }

    #[test]
    fn first_launch_answers_persist() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        let first = store.load().unwrap();
        assert!(!first.ui.welcomed && !first.ui.release_notices);
        assert_eq!(first.ui.theme, ThemeChoice::FollowDesktop);
        store.update(Change::Theme(ThemeChoice::Dark)).unwrap();
        store.update(Change::ReleaseNotices(true)).unwrap();
        let answered = store.update(Change::Welcomed).unwrap();
        assert!(answered.ui.welcomed && answered.ui.release_notices);
        assert_eq!(answered.ui.theme, ThemeChoice::Dark);
    }

    #[test]
    fn submitter_round_trips_validates_and_keeps_other_fields() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        assert_eq!(store.load().unwrap().submitter, Submitter::default());
        assert!(Submitter::default().missing().contains(&"email"));
        let submitter = Submitter {
            first_name: "Jane".into(),
            last_name: "Doe".into(),
            email: "jane@example.org".into(),
            institution: "University of Melbourne".into(),
            city: "Melbourne".into(),
            country: "Australia".into(),
            ..Submitter::default()
        };
        store.update(Change::RibbonSizePercent(150)).unwrap();
        store
            .update(Change::Submitter(Box::new(submitter.clone())))
            .unwrap();
        let prefs = SettingsStore::new(store.path.clone()).load().unwrap();
        assert_eq!(prefs.submitter, submitter);
        assert_eq!(prefs.submitter.missing(), Vec::<&str>::new());
        assert_eq!(prefs.ui.ribbon_size_percent, 150);
        let original = std::fs::read_to_string(&store.path).unwrap();
        for bad in [
            Submitter {
                email: "no-at".into(),
                ..submitter.clone()
            },
            Submitter {
                city: "Two\nlines".into(),
                ..submitter.clone()
            },
        ] {
            assert!(store.update(Change::Submitter(Box::new(bad))).is_err());
            assert_eq!(std::fs::read_to_string(&store.path).unwrap(), original);
        }
    }
    #[test]
    fn ribbon_size_round_trips_defaults_and_preserves_invalid_or_future_files() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        assert_eq!(store.load().unwrap().ui.ribbon_size_percent, 100);
        assert!(!store.path.exists());
        std::fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        std::fs::write(
            &store.path,
            "schema_version = 1\n[ui]\nfont_size_points = 18\nfuture_ui = 'keep'\n[future]\nkeep = true\n",
        )
        .unwrap();
        assert_eq!(store.load().unwrap().ui.ribbon_size_percent, 100);
        for size in [125, 150, 175, 200, 100] {
            store.update(Change::RibbonSizePercent(size)).unwrap();
            // A new store instance reads the actual serialized preference.
            let prefs = SettingsStore::new(store.path.clone()).load().unwrap();
            assert_eq!(prefs.ui.ribbon_size_percent, size);
            assert_eq!(prefs.ui.font_size_points, Some(18));
            let raw: toml::Value =
                toml::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
            assert_eq!(raw["ui"]["future_ui"].as_str(), Some("keep"));
            assert_eq!(raw["future"]["keep"].as_bool(), Some(true));
        }
        let original = std::fs::read_to_string(&store.path).unwrap();
        for size in [0, 99, 201, u32::MAX] {
            assert!(store.update(Change::RibbonSizePercent(size)).is_err());
            assert_eq!(std::fs::read_to_string(&store.path).unwrap(), original);
        }
        // Control sizes round-trip and refuse sizes too small to use.
        store.update(Change::TickBoxSize(20)).unwrap();
        store.update(Change::InputBoxHeight(30)).unwrap();
        let prefs = SettingsStore::new(store.path.clone()).load().unwrap();
        assert_eq!(
            (prefs.ui.tick_box_size, prefs.ui.input_box_height),
            (20, 30)
        );
        assert!(store.update(Change::TickBoxSize(4)).is_err());
        assert!(store.update(Change::InputBoxHeight(100)).is_err());
        store.update(Change::TickBoxSize(14)).unwrap();
        store.update(Change::InputBoxHeight(20)).unwrap();
        for invalid in [
            "schema_version = 999\n[ui]\nribbon_size_percent = 150\n",
            "[ui]\nribbon_size_percent = 150\n",
            "schema_version = 1\n[ui]\nribbon_size_percent = 201\n",
            "[ui]\nribbon_size_percent = '150'\n",
            "[ui]\nribbon_size_percent = 150.5\n",
        ] {
            std::fs::write(&store.path, invalid).unwrap();
            assert!(store.load().is_err());
            assert!(store.update(Change::RibbonSizePercent(100)).is_err());
            assert_eq!(std::fs::read_to_string(&store.path).unwrap(), invalid);
        }
    }
    #[test]
    fn font_size_defaults_persist_validate_and_reset_without_losing_unknown_fields() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        assert_eq!(store.load().unwrap().ui.font_size_points, None);
        assert!(!store.path.exists());
        store.update(Change::FontSizePoints(Some(10))).unwrap();
        assert_eq!(store.load().unwrap().ui.font_size_points, Some(10));
        let original = std::fs::read_to_string(&store.path).unwrap();
        assert!(store.update(Change::FontSizePoints(Some(0))).is_err());
        assert!(store.update(Change::FontSizePoints(Some(33))).is_err());
        assert_eq!(std::fs::read_to_string(&store.path).unwrap(), original);
        std::fs::write(&store.path, format!("{original}\n[future]\nkeep = true\n")).unwrap();
        store.update(Change::FontSizePoints(None)).unwrap();
        assert_eq!(store.load().unwrap().ui.font_size_points, None);
        assert!(
            std::fs::read_to_string(&store.path)
                .unwrap()
                .contains("keep = true")
        );
    }
    fn store(temp: &tempfile::TempDir) -> SettingsStore {
        SettingsStore::new(temp.path().join("config/preferences.toml"))
    }
    #[test]
    fn shortcut_conflicts_limits_missing_ids_and_reset() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        let shortcut = |id: &str, value: &str| Change::Shortcut {
            id: id.into(),
            value: Some(value.into()),
        };
        assert!(store.update(shortcut("missing.plugin", "Ctrl+S")).is_err());
        assert!(
            store
                .update(shortcut("missing.plugin", "Ctrl+Alt+T"))
                .is_err()
        );
        assert!(store.update(shortcut("missing.plugin", "F6")).is_err());
        assert!(store.update(shortcut("missing.plugin", "A")).is_err());
        assert!(
            store
                .update(Change::Shortcut {
                    id: String::new(),
                    value: None
                })
                .is_err()
        );
        store.update(shortcut("view.text", "Ctrl+Alt+T")).unwrap();
        assert!(
            store
                .update(shortcut("missing.plugin", &"x".repeat(129)))
                .is_err()
        );
        store
            .update(shortcut("missing.plugin", "Alt+Ctrl+J"))
            .unwrap();
        assert!(
            store
                .update(shortcut("other.plugin", "ctrl+alt+j"))
                .is_err()
        );
        let prefs = store
            .update(Change::RibbonHidden(vec!["missing.plugin".into()]))
            .unwrap();
        assert!(prefs.customization.shortcuts.contains_key("missing.plugin"));
        assert_eq!(prefs.customization.ribbon_hidden, ["missing.plugin"]);
        store.update(shortcut("file.save", "")).unwrap();
        store.update(shortcut("other.plugin", "Ctrl+S")).unwrap();
        let prefs = store.update(Change::ResetCustomization).unwrap();
        assert_eq!(prefs.customization, Customization::default());
        assert_eq!(
            canonical_shortcut("Shift+Ctrl+Enter").unwrap(),
            "CTRL+SHIFT+RETURN"
        );
    }
    #[test]
    fn session_overrides_are_validated_and_not_persisted() {
        let prefs = Preferences::default();
        let effective = prefs.resolve_session(&[Change::CpuThreads(2)]).unwrap();
        assert_eq!(effective.compute.local_cpu_threads, 2);
        assert_eq!(prefs.compute.local_cpu_threads, 4);
        assert!(prefs.resolve_session(&[Change::CpuThreads(0)]).is_err());
    }
    #[test]
    fn public_example_is_valid() {
        let prefs: Preferences =
            toml::from_str(include_str!("../../../schemas/preferences.example.toml")).unwrap();
        prefs.validate().unwrap();
    }
    #[test]
    fn passive_load_creates_nothing() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(store(&temp).load().unwrap(), Preferences::default());
        assert!(!temp.path().join("config").exists());
    }
    #[test]
    fn targeted_updates_reload_and_preserve_unknown_fields() {
        let temp = tempfile::tempdir().unwrap();
        let a = store(&temp);
        let b = store(&temp);
        a.update(Change::ShowGc(false)).unwrap();
        let mut text = std::fs::read_to_string(&a.path).unwrap();
        text.push_str("\n[future_plugin]\nunknown = 'retained'\n");
        std::fs::write(&a.path, text).unwrap();
        b.update(Change::CpuThreads(8)).unwrap();
        let prefs = a.load().unwrap();
        assert!(!prefs.ui.show_gc);
        assert_eq!(prefs.compute.local_cpu_threads, 8);
        assert_eq!(prefs.revision, 2);
        assert!(
            std::fs::read_to_string(&a.path)
                .unwrap()
                .contains("retained")
        );
        a.update(Change::Shortcut {
            id: "file.save".into(),
            value: Some("Ctrl+S".into()),
        })
        .unwrap();
        a.update(Change::Shortcut {
            id: "file.save".into(),
            value: None,
        })
        .unwrap();
        assert!(a.load().unwrap().customization.shortcuts.is_empty());
    }
    #[test]
    fn invalid_or_future_files_are_never_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        store.update(Change::ShowGc(false)).unwrap();
        let original = std::fs::read(&store.path).unwrap();
        assert!(store.update(Change::CpuThreads(0)).is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), original);
        for text in [
            "schema_version = 99\n",
            "not valid TOML = [",
            "[compute]\nlocal_cpu_threads = 0\n",
        ] {
            std::fs::write(&store.path, text).unwrap();
            assert!(store.load().is_err());
            assert!(store.update(Change::ShowGc(true)).is_err());
            assert_eq!(std::fs::read_to_string(&store.path).unwrap(), text);
        }
    }
    fn retry_update(store: &SettingsStore, change: &Change) {
        for _ in 0..1000 {
            match store.update(change.clone()) {
                Ok(_) => return,
                Err(Error::Busy) => std::thread::sleep(std::time::Duration::from_millis(1)),
                Err(e) => panic!("unexpected settings failure: {e}"),
            }
        }
        panic!("settings lock remained busy");
    }
    #[test]
    fn held_lock_returns_busy_without_replacing_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        store.update(Change::ShowGc(false)).unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(store.path.with_extension("toml.lock"))
            .unwrap();
        lock.lock_exclusive().unwrap();
        assert!(matches!(
            store.update(Change::ShowGc(true)),
            Err(Error::Busy)
        ));
        assert!(!store.load().unwrap().ui.show_gc);
    }
    #[test]
    fn concurrent_updates_do_not_lose_other_fields() {
        let temp = tempfile::tempdir().unwrap();
        let a = store(&temp);
        let b = a.clone();
        let one = std::thread::spawn(move || {
            for _ in 0..20 {
                retry_update(&a, &Change::ShowGc(false));
            }
        });
        let two = std::thread::spawn(move || {
            for _ in 0..20 {
                retry_update(&b, &Change::CpuThreads(7));
            }
        });
        one.join().unwrap();
        two.join().unwrap();
        let prefs = store(&temp).load().unwrap();
        assert!(!prefs.ui.show_gc);
        assert_eq!(prefs.compute.local_cpu_threads, 7);
    }
    #[test]
    fn unavailable_destination_reports_failure() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("config"), "occupied").unwrap();
        assert!(store(&temp).update(Change::ShowGc(false)).is_err());
    }
}
