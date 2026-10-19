//! Re-runnable legacy config importer: legacy Python app → this app's store.
//!
//! Ports `linux-app/config.py`'s on-disk artefacts into the new `ConfigStore`
//! (`OctoPad/config.json` + `OctoPad/presets/`):
//!
//! * reads `<legacy_dir>/config.json`, validates it with
//!   [`normalize_config`](super::normalize_config) and migrates it;
//! * provides both a persisted importer and an in-memory loader for the UI's
//!   explicit import flow, which must wait for the user's Save action;
//! * the persisted importer replaces the new store's config via a temp file +
//!   atomic rename, so a corrupt legacy file (or a crash mid-write) can never
//!   leave a half-written or partially-imported config behind;
//! * copies `<legacy_dir>/presets/*.json` into the new presets directory,
//!   overwriting the same slug and reporting any preset that could not be read
//!   as a warning.
//!
//! Import is deliberately **re-runnable**: it always re-reads whatever the
//! legacy app has now, so a user who keeps using the old app can import again
//! and pick up their newest edits. The legacy file wins — this is a full
//! replace from the legacy file, not a merge with the new app's edits (the
//! user's explicit choice at import time).
//!
//! ## First-run auto-import
//!
//! [`import_legacy_first_run`] (and its platform-default wrapper
//! [`auto_import_legacy`]) runs **once, at first run only**: only when the new
//! app's config file is missing *and* a legacy config file exists. If the new
//! app already has a config, or there is no legacy config, it is a no-op. Any
//! later import is on demand via [`import_legacy`].
//!
//! ## Windows profiles (deferred to M3)
//!
//! `windows-app` has no on-disk legacy *store* for profiles: profiles are
//! explicit import/export JSON files the user picks, not an app directory.
//! There is therefore nothing to auto-discover and profile import is
//! intentionally **not** implemented here — it arrives with the profiles
//! feature/UI in M3 (M1 plan, Global Constraints).

use super::model::Config;
use super::store::{preset_slug, ConfigStore};
use super::{migrate_config, normalize_config};
use crate::config_paths::legacy_config_dir;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Why a legacy import did not happen.
///
/// Serialized for the Tauri command surface (Task 8) as an externally tagged
/// value, e.g. `"NotFound"` or `{"Parse": "…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ImportError {
    /// `<legacy_dir>/config.json` does not exist — nothing to import.
    NotFound,
    /// The legacy config file is not valid JSON.
    Parse(String),
    /// The legacy config parsed but failed `normalize_config` validation.
    Invalid(String),
    /// Reading the legacy file or replacing the new store config failed.
    Io(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NotFound => write!(formatter, "no legacy config found"),
            ImportError::Parse(message) => {
                write!(formatter, "legacy config is not valid JSON: {message}")
            }
            ImportError::Invalid(message) => {
                write!(formatter, "legacy config is invalid: {message}")
            }
            ImportError::Io(message) => write!(formatter, "legacy import I/O error: {message}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// What a legacy import did (serialized to the UI by the Tauri command).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// True when the legacy config was successfully imported into the app.
    pub imported_config: bool,
    /// Slugs of the legacy presets copied into the new store (sorted, unique).
    pub presets: Vec<String>,
    /// Non-fatal problems (e.g. a preset file that could not be copied).
    pub warnings: Vec<String>,
    /// True only for the once-per-first-run auto-import.
    pub first_run_auto: bool,
}

impl ImportReport {
    /// A report for "nothing to do" (used by the first-run rule).
    fn noop() -> ImportReport {
        ImportReport {
            imported_config: false,
            presets: Vec::new(),
            warnings: Vec::new(),
            first_run_auto: false,
        }
    }
}

/// Import a legacy config directory into `store`, replacing the new app's
/// config and copying over its user presets.
///
/// Re-runnable by design: each call re-reads the legacy files as they are now.
/// Errors leave the new store exactly as it was — validation happens before any
/// write, and the config itself is replaced atomically (temp file + rename).
pub fn import_legacy(store: &ConfigStore, legacy_dir: &Path) -> Result<ImportReport, ImportError> {
    let config = read_legacy_config(legacy_dir)?;
    write_config_atomic(store, &config)?;

    Ok(import_report(store, legacy_dir))
}

/// Load a legacy config into memory for the interactive UI import flow.
///
/// Unlike [`import_legacy`], this does not replace the saved config; the
/// caller applies the returned config to its working state and waits for an
/// explicit Save. Legacy presets are still copied into the store.
pub fn load_legacy_config(
    store: &ConfigStore,
    legacy_dir: &Path,
) -> Result<(Config, ImportReport), ImportError> {
    let config = read_legacy_config(legacy_dir)?;
    let report = import_report(store, legacy_dir);
    Ok((config, report))
}

fn read_legacy_config(legacy_dir: &Path) -> Result<Config, ImportError> {
    let legacy_file = legacy_dir.join("config.json");
    if !legacy_file.is_file() {
        return Err(ImportError::NotFound);
    }

    let text = fs::read_to_string(&legacy_file).map_err(|exc| {
        ImportError::Io(format!("failed to read {}: {exc}", legacy_file.display()))
    })?;
    let stored: Value = serde_json::from_str(&text)
        .map_err(|exc| ImportError::Parse(format!("{}: {exc}", legacy_file.display())))?;

    // Validate before touching the new store: a corrupt legacy profile must
    // leave the existing new-app config exactly as it was.
    normalize_config(&stored).map_err(ImportError::Invalid)?;

    let mut config = super::store::config_from_value(&stored);
    migrate_config(&mut config);
    Ok(config)
}

fn import_report(store: &ConfigStore, legacy_dir: &Path) -> ImportReport {
    let mut warnings = Vec::new();
    let presets = copy_legacy_presets(store, legacy_dir, &mut warnings);

    ImportReport {
        imported_config: true,
        presets,
        warnings,
        first_run_auto: false,
    }
}

/// The once-per-first-run auto-import: imports only when the new app's config
/// file is missing *and* a legacy config file exists in `legacy_dir`.
///
/// Every other case is a no-op returning an [`ImportReport`] with
/// `imported_config: false` and `first_run_auto: false`. Errors from an actual
/// import are propagated to the caller (the app can choose to ignore them and
/// start with defaults).
pub fn import_legacy_first_run(
    store: &ConfigStore,
    legacy_dir: &Path,
) -> Result<ImportReport, ImportError> {
    if store.config_path().exists() || !legacy_dir.join("config.json").is_file() {
        return Ok(ImportReport::noop());
    }
    let mut report = import_legacy(store, legacy_dir)?;
    report.first_run_auto = true;
    Ok(report)
}

/// [`import_legacy_first_run`] against the platform's legacy config directory
/// (`~/.config/octopad` on Linux, `%APPDATA%\OctoPad` on Windows).
pub fn auto_import_legacy(store: &ConfigStore) -> Result<ImportReport, ImportError> {
    import_legacy_first_run(store, &legacy_config_dir())
}

/// Replace the store's config atomically: write a sibling temp file, flush it
/// to disk, then rename it over `config.json`.
///
/// The temp file lives in the same directory so the rename is atomic, and it is
/// removed on any failure so a failed import leaves no litter behind.
fn write_config_atomic(store: &ConfigStore, config: &Config) -> Result<(), ImportError> {
    let path = store.config_path();
    let directory = path.parent().ok_or_else(|| {
        ImportError::Io(format!(
            "config path {} has no parent directory",
            path.display()
        ))
    })?;
    fs::create_dir_all(directory).map_err(|exc| {
        ImportError::Io(format!("failed to create {}: {exc}", directory.display()))
    })?;

    let text = serde_json::to_string_pretty(config)
        .map_err(|exc| ImportError::Io(format!("failed to serialize config: {exc}")))?;

    let temp = temp_path(directory);
    let write_result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(exc) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(ImportError::Io(format!(
            "failed to write {}: {exc}",
            temp.display()
        )));
    }
    if let Err(exc) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(ImportError::Io(format!(
            "failed to replace {}: {exc}",
            path.display()
        )));
    }

    Ok(())
}

/// Copy `<legacy_dir>/presets/*.json` over the new presets directory.
///
/// Returns the copied slugs (sorted, de-duplicated); unreadable preset files
/// are reported as warnings instead of failing the whole import.
fn copy_legacy_presets(
    store: &ConfigStore,
    legacy_dir: &Path,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let source = legacy_dir.join("presets");
    if !source.is_dir() {
        return Vec::new();
    }
    let destination = store.presets_dir();
    if let Err(exc) = fs::create_dir_all(&destination) {
        warnings.push(format!(
            "failed to create preset directory {}: {exc}",
            destination.display()
        ));
        return Vec::new();
    }
    let entries = match fs::read_dir(&source) {
        Ok(entries) => entries,
        Err(exc) => {
            warnings.push(format!(
                "failed to read preset directory {}: {exc}",
                source.display()
            ));
            return Vec::new();
        }
    };

    let mut copied = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(exc) => {
                warnings.push(format!(
                    "failed to read a preset entry in {}: {exc}",
                    source.display()
                ));
                continue;
            }
        };
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let slug = preset_slug(stem);
        let target = destination.join(format!("{slug}.json"));
        match fs::copy(&path, &target) {
            Ok(_) => copied.push(slug),
            Err(exc) => warnings.push(format!(
                "failed to copy preset file {}: {exc}",
                path.display()
            )),
        }
    }
    copied.sort();
    copied.dedup();
    copied
}

/// A unique sibling temp path for the atomic config replace.
fn temp_path(directory: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    directory.join(format!(
        ".config.json.import-{}-{nanos}-{count}.tmp",
        std::process::id()
    ))
}
