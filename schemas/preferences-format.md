# Preferences format, version 1

Basecraft preferences are ordinary UTF-8 TOML. See
[preferences.example.toml](preferences.example.toml). `schema_version` selects
the reader; `revision` increases on a successful change. Unknown fields survive
updates. Unsupported versions, malformed data and invalid known values are errors;
the original file is retained. Missing fields use the version's documented defaults
shown in the example (shortcut overrides default to an empty map).

Optional `ui.font_size_points` sets interface text size in points (6–32), applied
immediately. When absent, the GUI uses the rounded platform point size minus three,
with a minimum of six points. Resetting the size removes this override. Optional
`ui.font_family` names the interface font; when absent the GUI follows the
desktop's monospace family. Scientific canvas zoom is separate.

`ui.theme` is `follow-desktop` (the default; the desktop's theme colours),
`light` or `dark` (Basecraft's own palettes). `ui.release_notices` (default
false) makes the GUI ask GitHub for the latest version number at startup and
show a notice when it is newer; offscreen and screenshot runs never ask.
`ui.welcomed` records that the first-launch questions were answered.

`ui.ribbon_size_percent` independently scales ribbon icons, labels, fields, group
captions and tabs. It defaults to 100 and accepts integers from 100 to 200; the GUI
steppers offer 25-percent increments and a reset to 100. It composes with interface
font size and Qt display scaling without changing the window font or canvas zoom.

`submitter` holds the contact details and affiliation written to every GenBank
submission template (`.sbt`): name, email, phone, institution, department,
street, city, state, postal code and country, each at most 200 characters on
one line. First and last name, email (with an `@`), institution, city and
country are required before a package is ready; empty fields are unset. Authors
and the publication title belong to each submission document, not here.

`contacts.service_emails` maps a service name (lowercase letters and `-`, at
most 32 characters) to the email that service receives; NCBI BLAST uses
`blast`. A service without an entry uses `submitter.email`. Clearing an entry
removes it.

`basecraft_platform::paths::AppPaths` discovers native config, data, state, cache
and scratch locations without creating them. On Linux these use XDG locations
with the application directory `basecraft`; macOS uses Library application support
and cache directories; Windows uses roaming config and local application data.
`BASECRAFT_PROFILE_DIR` selects an isolated root containing config/data/state/cache.
`BASECRAFT_STATE_DIR`, `BASECRAFT_CACHE_DIR` and `BASECRAFT_SCRATCH_DIR` override
their respective locations. Overrides must be absolute native paths. Discovery
never creates directories. Preferences live at `config/preferences.toml`.

`SettingsStore::load` is read-only. `SettingsStore::update(Change)` creates the
config directory only for an explicit preference change, tries to obtain an OS lock on
`preferences.toml.lock` (busy returns immediately with a retry error), reloads, changes the requested field, validates and uses
the shared atomic writer to replace the file. Failed validation never replaces
it. Locks coordinate cooperating Basecraft processes on filesystems supporting
OS file locks; cloud synchronization is not a distributed lock protocol. No stale
whole-file snapshot is accepted for an update. A process crash releases its OS
lock; the small lock file remains and contains no document information.

A file without `schema_version` is an error. The GUI reads known UI values from the old
QSettings `ui` group once and records `migration.qsettings_imported`. It never
changes the original QSettings store. An explicit profile never imports the global
legacy store. Existing explicit TOML changes take precedence over legacy values. Invalid imported values produce an error
instead of silently substituting defaults. Customization keys use stable command
IDs; missing plugin IDs are retained for future availability. The GUI applies shortcut and ribbon visibility overrides to shared application
commands and contextual ribbon commands. New commands use defaults when there is
no override; unavailable IDs remain stored and are listed in Preferences. Reset
clears both collections, including unavailable IDs. Hiding an application ribbon
command does not remove its menu entry or keyboard shortcut; contextual ribbon
shortcuts are active only while their page/control is available. Command IDs are
nonempty, contain no control characters and are at most 256 bytes. Collections
are limited to 4096 entries. Shortcuts are portable single chords, at most 128
bytes: Ctrl/Alt/Shift/Meta modifiers followed by one key, or F1–F35. An empty
string disables a shortcut; removing its override restores the default. Modifier
order and case are normalized when checking conflicts. Plain typing keys and
reserved canvas/navigation bindings are rejected. Conflicts are checked against
both stored overrides and application defaults, including commands that are
currently unavailable. Visibility changes target one ID under the preferences
lock so concurrent changes to unrelated commands survive. Files are limited to 1 MiB. CPU threads range from
1 to 65536; ORF minimum codons range from 1 to 1000000.

Recovery is consumed by lazy document sessions. History and provenance always go
to the project's `.basecraft`, or beside a loose file; there is no setting for it. No recent-document list belongs in this
file. Passive preference reads create no file-specific record. The GUI emits
property change notifications for its preference snapshot; external process
changes are observed on reload and always before a targeted write.

`Preferences::resolve_session` applies validated explicit session/command overrides
above user preferences without changing disk state. Project analysis defaults are
separate project metadata; they cannot override machine/UI preferences here.

The GUI preferences form groups appearance, file policy, compute, ribbon/shortcut
customization and plugin information. Profile reload refreshes its controls and
the live application properties. Native document controls retain reserved local
editing/navigation keys; these are documented separately from the shared command
shortcut list. Host-launched plugins, CLI and MCP jobs load the same validated CPU
allocation when no application/session allocation was supplied. Invalid profiles
fail job creation with a useful error rather than silently using another budget.
The standalone Python tool utility consumes the host's `BASECRAFT_CPU_THREADS`
environment allocation. Outside the host it invokes `basecraft preferences`
(`BASECRAFT` can select the executable), which returns this Rust-validated schema
as JSON without writing files. Missing CLI, invalid profiles and timeouts are
errors, never a reason to silently substitute a different CPU budget. Concurrent
file jobs share process-local CPU permits; independent application processes
retain independent budgets.
