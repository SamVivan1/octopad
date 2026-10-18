//! Config file IO: lenient per-key load/save merge + user presets.
//!
//! Direct port of `load_config`/`save_config` and the user-preset helpers from
//! `linux-app/config.py`. The merge is deliberately per-key and lenient: only a
//! malformed JSON document or a non-object top level counts as corrupt (→
//! defaults); every other stored value is merged into the defaults with a type
//! check, so a single bad key never resets the whole configuration.

use super::model::{Button, Config, TriggerActions};
use super::{action_from_value, default_config, from_json_list, migrate_config};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// The new app's configuration store rooted at its own directory
/// (`<config dir>/OctoPad/`); the legacy `~/.config/octopad` stays read-only.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    pub root: PathBuf,
}

impl ConfigStore {
    /// The app's real on-disk location: `dirs::config_dir()/OctoPad`.
    ///
    /// Falls back to `~/.config/OctoPad` when the platform config dir is
    /// unknown. Never panics — with no HOME at all it degrades to a relative
    /// path (save/load will simply fail gracefully from there).
    pub fn new_app() -> ConfigStore {
        let root = dirs::config_dir()
            .map(|dir| dir.join("OctoPad"))
            .or_else(|| dirs::home_dir().map(|home| home.join(".config").join("OctoPad")))
            .unwrap_or_else(|| PathBuf::from(".config").join("OctoPad"));
        ConfigStore { root }
    }

    /// Store rooted at an explicit directory (used by tests and the importer).
    pub fn at(root: PathBuf) -> ConfigStore {
        ConfigStore { root }
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn presets_dir(&self) -> PathBuf {
        self.root.join("presets")
    }

    /// Load the user configuration, filling in any missing default fields.
    ///
    /// Python `load_config()` parity: missing file → defaults; malformed JSON
    /// or a non-object top level → defaults + log line; otherwise a lenient
    /// per-key merge (wrong-typed key → keep *that* default + warning).
    pub fn load_config(&self) -> Config {
        let path = self.config_path();
        if !path.exists() {
            return default_config();
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(exc) => {
                tracing::warn!("[CONFIG] Failed to read {}: {exc}", path.display());
                return default_config();
            }
        };
        let stored: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(exc) => {
                tracing::warn!("[CONFIG] Failed to read {}: {exc}", path.display());
                return default_config();
            }
        };
        let Some(stored) = stored.as_object() else {
            tracing::warn!(
                "[CONFIG] {} is not a JSON object — using defaults",
                path.display()
            );
            return default_config();
        };
        merge_stored(default_config(), stored)
    }

    /// Save the configuration to disk (indent=2, plain UTF-8 — the
    /// `ensure_ascii=False` equivalent).
    pub fn save_config(&self, config: &Config) -> Result<(), String> {
        let path = self.config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|exc| format!("[CONFIG] Failed to create {}: {exc}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(config).map_err(|exc| exc.to_string())?;
        std::fs::write(&path, text)
            .map_err(|exc| format!("[CONFIG] Failed to write {}: {exc}", path.display()))
    }

    /// Names of saved user presets (alphabetical order, Python `sorted`
    /// file-name order).
    pub fn list_presets(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.presets_dir()) else {
            return Vec::new();
        };
        let mut files: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".json"))
            .collect();
        files.sort();
        files
            .iter()
            .filter_map(|name| {
                Path::new(name)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .collect()
    }

    /// Save a preset snapshot to the user preset directory.
    pub fn save_preset(&self, name: &str, config: &Config) -> Result<(), String> {
        let dir = self.presets_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|exc| format!("[CONFIG] Failed to create {}: {exc}", dir.display()))?;
        let path = dir.join(format!("{}.json", preset_slug(name)));
        let text = serde_json::to_string_pretty(config).map_err(|exc| exc.to_string())?;
        std::fs::write(&path, text)
            .map_err(|exc| format!("[CONFIG] Failed to write preset '{name}': {exc}"))
    }

    /// Load a user preset snapshot; `None` if it does not exist or is not a
    /// JSON object. The snapshot is merged over the defaults with the same
    /// lenient per-key rules as `load_config`, so legacy presets (partial or
    /// dict-action shaped) still load into a complete typed config.
    pub fn load_preset(&self, name: &str) -> Option<Config> {
        let path = self
            .presets_dir()
            .join(format!("{}.json", preset_slug(name)));
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(exc) => {
                tracing::warn!("[CONFIG] Failed to read preset '{name}': {exc}");
                return None;
            }
        };
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(exc) => {
                tracing::warn!("[CONFIG] Failed to read preset '{name}': {exc}");
                return None;
            }
        };
        let stored = value.as_object()?;
        Some(merge_stored(default_config(), stored))
    }

    /// Delete a preset file. Returns `false` if it did not exist (or the
    /// removal failed).
    pub fn delete_preset(&self, name: &str) -> bool {
        let path = self
            .presets_dir()
            .join(format!("{}.json", preset_slug(name)));
        if !path.exists() {
            return false;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(exc) => {
                tracing::warn!("[CONFIG] Failed to delete preset '{name}': {exc}");
                false
            }
        }
    }
}

/// Build a safe slug for a preset name (used as the file name) — exact port of
/// Python `preset_slug()`, including the `"preset"` fallback.
pub fn preset_slug(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == ' ' || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    // Python: re.sub(r"[-_ ]+", "-", cleaned).strip("-")
    let mut slug = String::with_capacity(cleaned.len());
    let mut in_separator = false;
    for ch in cleaned.chars() {
        if ch == '-' || ch == '_' || ch == ' ' {
            if !in_separator {
                slug.push('-');
            }
            in_separator = true;
        } else {
            slug.push(ch);
            in_separator = false;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "preset".to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Lenient per-key merge (Python load_config/_merge_button parity)
// ---------------------------------------------------------------------------

fn merge_stored(mut config: Config, stored: &Map<String, Value>) -> Config {
    merge_top_level(&mut config, stored);
    merge_modes(&mut config, stored);
    migrate_config(&mut config);
    config
}

/// Build a complete typed config from an already-parsed JSON value using the
/// same lenient per-key merge as [`ConfigStore::load_config`] (Python
/// `load_config()` parity). A non-object top level yields pure defaults.
///
/// The legacy importer reuses this so imported configs go through exactly the
/// same merge/migration path as anything loaded from the new app's own store.
pub(crate) fn config_from_value(stored: &Value) -> Config {
    match stored.as_object() {
        Some(object) => merge_stored(default_config(), object),
        None => default_config(),
    }
}

fn warn_bad_type(key: &str) {
    tracing::warn!("[CONFIG] Stored '{key}' has the wrong type — keeping default");
}

fn merge_top_level(config: &mut Config, stored: &Map<String, Value>) {
    // Python copies these keys verbatim; the plan pins an explicit type check
    // per key so a wrong-typed value keeps *its* default (with a warning)
    // instead of poisoning the typed model.
    if let Some(raw) = stored.get("version") {
        match raw.as_f64() {
            Some(version) => config.version = version,
            None => warn_bad_type("version"),
        }
    }
    if let Some(raw) = stored.get("schemaVersion") {
        match as_integer(raw) {
            Some(version) => config.schema_version = version,
            None => warn_bad_type("schemaVersion"),
        }
    }
    if let Some(raw) = stored.get("device") {
        match raw.as_str() {
            Some(device) => config.device = device.to_string(),
            None => warn_bad_type("device"),
        }
    }
    // Timings stay loosely typed on purpose: a stored non-numeric value must
    // survive the load so timing_command() can return None for it later
    // (Python port: int("not-a-number") → ValueError → no firmware sync).
    for key in ["debounceMs", "clickTimeoutMs", "holdTimeoutMs"] {
        if let Some(raw) = stored.get(key) {
            match key {
                "debounceMs" => config.debounce_ms = raw.clone(),
                "clickTimeoutMs" => config.click_timeout_ms = raw.clone(),
                _ => config.hold_timeout_ms = raw.clone(),
            }
        }
    }
    if let Some(raw) = stored.get("activeModeIndex") {
        match as_integer(raw) {
            Some(index) => config.active_mode_index = index,
            None => warn_bad_type("activeModeIndex"),
        }
    }
    if let Some(raw) = stored.get("activePreset") {
        match raw {
            Value::Null => config.active_preset = None,
            Value::String(name) => config.active_preset = Some(name.clone()),
            _ => warn_bad_type("activePreset"),
        }
    }
}

/// Python `int(x)` shape for stored integer fields: only JSON integers (or
/// whole floats) qualify; anything else is "wrong type".
fn as_integer(raw: &Value) -> Option<i64> {
    match raw {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|f| f.fract() == 0.0)
                .map(|f| f as i64)
        }),
        _ => None,
    }
}

fn merge_modes(config: &mut Config, stored: &Map<String, Value>) {
    let Some(raw_modes) = stored.get("modes") else {
        return;
    };
    let Some(stored_modes) = raw_modes.as_array() else {
        // Wrong-typed top-level key: defaults + warning (NOT full defaults —
        // top-level keys merged above still stand).
        warn_bad_type("modes");
        return;
    };
    // Python merges only when at least 2 modes are present; fewer is skipped
    // silently and the default modes stay.
    if stored_modes.len() < 2 {
        return;
    }
    for (slot, raw_mode) in stored_modes.iter().take(2).enumerate() {
        let Some(stored_mode) = raw_mode.as_object() else {
            continue; // Python: `if not isinstance(stored_mode, dict): continue`
        };
        let mode = &mut config.modes[slot];
        if let Some(raw) = stored_mode.get("id") {
            match raw.as_str() {
                Some(id) => mode.id = id.to_string(),
                None => warn_bad_type("modes[].id"),
            }
        }
        if let Some(raw) = stored_mode.get("name") {
            match raw.as_str() {
                Some(name) => mode.name = name.to_string(),
                None => warn_bad_type("modes[].name"),
            }
        }
        if let Some(raw) = stored_mode.get("ledState") {
            match raw {
                Value::Bool(led) => mode.led_state = *led,
                _ => warn_bad_type("modes[].ledState"),
            }
        }
        let Some(raw_buttons) = stored_mode.get("buttons") else {
            continue;
        };
        let Some(stored_buttons) = raw_buttons.as_array() else {
            continue; // Python: not a list → keep the template's buttons
        };
        // The template for every slot is this mode's *default* button (modes
        // start out as the defaults; merging one slot never touches another).
        let templates = mode.buttons.clone();
        let mut buttons = Vec::with_capacity(8);
        for index in 0..8 {
            let template = &templates[index];
            match stored_buttons.get(index) {
                Some(raw) if raw.is_object() => {
                    buttons.push(merge_button(raw.as_object().unwrap(), template))
                }
                // Python: missing or non-dict entry → deepcopy of the template.
                _ => buttons.push(template.clone()),
            }
        }
        mode.buttons = buttons;
    }
}

/// Port of Python `_merge_button(base, template)`.
fn merge_button(base: &Map<String, Value>, template: &Button) -> Button {
    let mut merged = template.clone();
    // Python: `merged.update({k: v for k, v in base.items() if k in
    // ("index", "pin", "label")})` — with a type check so the typed model
    // stays sound (wrong type keeps the template value + warning).
    if let Some(raw) = base.get("index") {
        match as_integer(raw) {
            Some(index) => merged.index = index,
            None => warn_bad_type("button.index"),
        }
    }
    if let Some(raw) = base.get("pin") {
        match as_integer(raw) {
            Some(pin) => merged.pin = pin,
            None => warn_bad_type("button.pin"),
        }
    }
    if let Some(raw) = base.get("label") {
        match raw.as_str() {
            Some(label) => merged.label = label.to_string(),
            None => warn_bad_type("button.label"),
        }
    }
    merged.single = merge_trigger(base.get("single"), &template.single);
    merged.double = merge_trigger(base.get("double"), &template.double);
    merged.hold = merge_trigger(base.get("hold"), &template.hold);
    merged
}

/// Port of the trigger half of Python `_merge_button`: a stored list keeps
/// **all** of its dict entries, a dict with a truthy `type` wins as-is (legacy
/// dict shape preserved), anything else keeps the template's trigger.
fn merge_trigger(raw: Option<&Value>, template: &TriggerActions) -> TriggerActions {
    match raw {
        // Python: `clean or [{"type": "none"}]`.
        Some(Value::Array(items)) => from_json_list(items),
        Some(value @ Value::Object(_)) if python_truthy(value.get("type")) => {
            TriggerActions::One(action_from_value(value))
        }
        // Missing, scalar, or a dict without a truthy "type" → template wins.
        _ => template.clone(),
    }
}

/// Python truthiness for JSON values (used for the trigger `type` check).
fn python_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}
