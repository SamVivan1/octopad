//! Configuration domain: schema constants, defaults, validation, migration.
//!
//! Direct port of `linux-app/config.py` — that file is the single source of
//! truth for the on-disk schema and its semantics.

pub mod import;
pub mod model;
pub mod store;

use serde_json::{json, Value};
use std::collections::HashMap;

pub use import::{
    auto_import_legacy, import_legacy, import_legacy_first_run, ImportError, ImportReport,
};
pub use model::{Action, Button, Config, Mode, TriggerActions};
pub use store::{preset_slug, ConfigStore};

/// Schema version of the stored configuration (independent of the product
/// version). Bump it whenever the on-disk shape changes and add the matching
/// upgrade step to `migrate_config()`.
pub const CURRENT_SCHEMA_VERSION: i64 = 1;

/// Physical pinout for buttons B1..B8 (same as octopad.ino).
pub const PINOUT: [i64; 8] = [20, 9, 2, 1, 21, 10, 3, 0];

/// Default timing-engine values, shared by the config and the firmware sync.
pub struct Timings {
    pub debounce_ms: i64,
    pub click_timeout_ms: i64,
    pub hold_timeout_ms: i64,
}

pub const DEFAULT_TIMINGS: Timings = Timings {
    debounce_ms: 25,
    click_timeout_ms: 250,
    hold_timeout_ms: 450,
};

/// Hardware button labels (B1..B8).
pub const DEFAULT_LABELS: [&str; 8] = [
    "Mode / Passcode",
    "Terminal",
    "Screenshot",
    "Mute Mic",
    "Vol Up",
    "Vol Down",
    "Lock Screen",
    "Macro 7",
];

fn none_action() -> Action {
    Action {
        action_type: "none".to_string(),
        extra: HashMap::new(),
    }
}

fn typed_action(action_type: &str, extra: HashMap<String, Value>) -> Action {
    Action {
        action_type: action_type.to_string(),
        extra,
    }
}

fn system_action(preset_id: &str, preset_name: &str) -> Action {
    let mut extra = HashMap::new();
    extra.insert("presetId".to_string(), json!(preset_id));
    extra.insert("presetName".to_string(), json!(preset_name));
    typed_action("system_action", extra)
}

fn shortcut(key: &str) -> Action {
    let mut extra = HashMap::new();
    extra.insert("modifiers".to_string(), json!(["ctrl", "alt", "shift"]));
    extra.insert("key".to_string(), json!(key));
    typed_action("shortcut", extra)
}

fn button(
    index: usize,
    label: &str,
    single: Action,
    double: Option<Action>,
    hold: Option<Action>,
) -> Button {
    Button {
        index: index as i64,
        pin: PINOUT[index],
        label: label.to_string(),
        // Python's default config stores every trigger as a legacy dict —
        // `One` keeps that shape through the round-trip.
        single: TriggerActions::One(single),
        double: TriggerActions::One(double.unwrap_or_else(none_action)),
        hold: TriggerActions::One(hold.unwrap_or_else(none_action)),
    }
}

/// Normalize a raw JSON list into `TriggerActions` with `_merge_button`
/// semantics: dict entries only (`clean`), empty collapses to
/// `[{"type": "none"}]` (Python: `clean or [{"type": "none"}]`).
pub(crate) fn from_json_list(items: &[Value]) -> TriggerActions {
    let clean: Vec<Action> = items
        .iter()
        .filter(|item| item.is_object())
        .map(action_from_value)
        .collect();
    if clean.is_empty() {
        TriggerActions::One(default_action("none"))
    } else {
        TriggerActions::Many(clean)
    }
}

/// Format the firmware `TIMING` command, or `None` when values are invalid.
///
/// Both transports (USB serial and BLE) use this so the pad always receives an
/// identical command built from a single source of truth.
pub fn timing_command(cfg: &Config) -> Option<String> {
    let debounce = timing_int(&cfg.debounce_ms)?;
    let click = timing_int(&cfg.click_timeout_ms)?;
    let hold = timing_int(&cfg.hold_timeout_ms)?;
    Some(format!("TIMING:{debounce},{click},{hold}"))
}

/// Port of Python `int(value)` for a stored timing value: numbers, numeric
/// strings (whitespace-tolerant) and bools coerce; anything else is invalid.
fn timing_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => {
            if let Some(i) = number.as_i64() {
                Some(i)
            } else if let Some(u) = number.as_u64() {
                i64::try_from(u).ok()
            } else {
                let f = number.as_f64()?;
                if f.is_finite() && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    Some(f as i64)
                } else {
                    None
                }
            }
        }
        Value::String(text) => text.trim().parse::<i64>().ok(),
        // Python: bool is an int subclass, int(True) == 1.
        Value::Bool(flag) => Some(i64::from(*flag)),
        _ => None,
    }
}

/// Default action for a type — with no hardcoded data (e.g. HA entity).
pub fn default_action(action_type: &str) -> Action {
    match action_type {
        "shortcut" => {
            let mut extra = HashMap::new();
            extra.insert("modifiers".to_string(), json!(["ctrl", "alt", "shift"]));
            extra.insert("key".to_string(), json!("A"));
            typed_action("shortcut", extra)
        }
        "text" => {
            let mut extra = HashMap::new();
            extra.insert("text".to_string(), json!("Hello World"));
            typed_action("text", extra)
        }
        "media" => {
            let mut extra = HashMap::new();
            extra.insert("mediaKey".to_string(), json!("VOL_UP"));
            typed_action("media", extra)
        }
        // User picks the devices to cycle through via the output picker.
        "audio_output" => {
            let mut extra = HashMap::new();
            extra.insert("sinks".to_string(), json!([]));
            typed_action("audio_output", extra)
        }
        "launch_app" => {
            let mut extra = HashMap::new();
            extra.insert("desktopId".to_string(), json!("code.desktop"));
            extra.insert("appName".to_string(), json!("Visual Studio Code"));
            typed_action("launch_app", extra)
        }
        "system_action" => system_action("mic_toggle_mute", "Toggle Microphone Mute"),
        "bash_script" => {
            let mut extra = HashMap::new();
            extra.insert(
                "script".to_string(),
                json!(r#"notify-send "OctoPad" "Action!""#),
            );
            typed_action("bash_script", extra)
        }
        // No built-in entity: the user selects one via the Entity Picker.
        "home_assistant" => {
            let mut extra = HashMap::new();
            extra.insert("domain".to_string(), json!(""));
            extra.insert("service".to_string(), json!(""));
            extra.insert("entityId".to_string(), json!(""));
            extra.insert("friendlyName".to_string(), json!(""));
            typed_action("home_assistant", extra)
        }
        "ha_webhook" => {
            let mut extra = HashMap::new();
            extra.insert("url".to_string(), json!(""));
            typed_action("ha_webhook", extra)
        }
        "delay" => {
            let mut extra = HashMap::new();
            extra.insert("seconds".to_string(), json!(0.5));
            typed_action("delay", extra)
        }
        "mode_toggle" => typed_action("mode_toggle", HashMap::new()),
        _ => none_action(),
    }
}

/// Default configuration. HA mode intentionally has no entities — the user
/// picks them.
pub fn default_config() -> Config {
    let mode_toggle = typed_action("mode_toggle", HashMap::new());
    let mut passcode_extra = HashMap::new();
    passcode_extra.insert("text".to_string(), json!("031004"));
    let passcode_hold = typed_action("text", passcode_extra);

    let desktop_buttons = vec![
        button(
            0,
            DEFAULT_LABELS[0],
            mode_toggle.clone(),
            None,
            Some(passcode_hold.clone()),
        ),
        button(
            1,
            DEFAULT_LABELS[1],
            system_action("open_terminal", "Open New Terminal"),
            None,
            None,
        ),
        button(
            2,
            DEFAULT_LABELS[2],
            system_action("screenshot_interactive", "Interactive Screenshot (Area)"),
            None,
            None,
        ),
        button(
            3,
            DEFAULT_LABELS[3],
            system_action("mic_toggle_mute", "Toggle Microphone Mute"),
            None,
            None,
        ),
        button(
            4,
            DEFAULT_LABELS[4],
            system_action("volume_up_5", "Volume Up (+5%)"),
            None,
            None,
        ),
        button(
            5,
            DEFAULT_LABELS[5],
            system_action("volume_down_5", "Volume Down (-5%)"),
            None,
            None,
        ),
        button(
            6,
            DEFAULT_LABELS[6],
            system_action("lock_screen", "Lock Ubuntu Session"),
            None,
            None,
        ),
        button(
            7,
            DEFAULT_LABELS[7],
            shortcut("8"),
            Some(shortcut("U")),
            Some(shortcut("J")),
        ),
    ];

    let mut ha_buttons = vec![button(
        0,
        DEFAULT_LABELS[0],
        mode_toggle,
        None,
        Some(passcode_hold),
    )];
    for i in 0..7 {
        ha_buttons.push(button(
            i + 1,
            &format!("HA Key {}", i + 1),
            default_action("home_assistant"),
            None,
            None,
        ));
    }

    Config {
        version: 2.6,
        schema_version: CURRENT_SCHEMA_VERSION,
        device: "OctoPad".to_string(),
        debounce_ms: json!(DEFAULT_TIMINGS.debounce_ms),
        click_timeout_ms: json!(DEFAULT_TIMINGS.click_timeout_ms),
        hold_timeout_ms: json!(DEFAULT_TIMINGS.hold_timeout_ms),
        active_mode_index: 0,
        active_preset: None,
        modes: vec![
            Mode {
                id: "desktop".to_string(),
                name: "Desktop Mode".to_string(),
                led_state: true,
                buttons: desktop_buttons,
            },
            Mode {
                id: "ha".to_string(),
                name: "Home Assistant Mode".to_string(),
                led_state: false,
                buttons: ha_buttons,
            },
        ],
    }
}

/// Normalize a raw JSON value into a list of actions.
///
/// A single trigger may hold multiple actions (legacy dicts still accepted):
/// dict → one action; list → its dict entries only; anything else → empty.
pub fn actions_list(raw: &Value) -> Vec<Action> {
    match raw {
        Value::Object(_) => vec![action_from_value(raw)],
        Value::Array(items) => items
            .iter()
            .filter(|item| item.is_object())
            .map(action_from_value)
            .collect(),
        _ => Vec::new(),
    }
}

/// Convert a JSON object into an `Action`, tolerating any shape Python would
/// keep (a missing or non-string `type` degrades to `""`).
fn action_from_value(value: &Value) -> Action {
    match value.as_object() {
        Some(map) => {
            let action_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let extra = map
                .iter()
                .filter(|(key, _)| key.as_str() != "type")
                .map(|(key, val)| (key.clone(), val.clone()))
                .collect();
            Action { action_type, extra }
        }
        None => Action {
            action_type: String::new(),
            extra: HashMap::new(),
        },
    }
}

/// Validate the structure of an imported profile. `Err(message)` mirrors the
/// Python `normalize_config` failure text.
pub fn normalize_config(value: &Value) -> Result<(), String> {
    let data = value
        .as_object()
        .ok_or_else(|| "Not a JSON object.".to_string())?;
    let modes = data
        .get("modes")
        .and_then(Value::as_array)
        .filter(|modes| modes.len() >= 2)
        .ok_or_else(|| "Invalid 'modes' field (2 modes required).".to_string())?;
    for mode in modes {
        let mode_obj = mode
            .as_object()
            .ok_or_else(|| "Invalid 'modes[].buttons' structure.".to_string())?;
        let buttons = mode_obj
            .get("buttons")
            .and_then(Value::as_array)
            .ok_or_else(|| "Invalid 'modes[].buttons' structure.".to_string())?;
        for button in buttons {
            let button_obj = button
                .as_object()
                .ok_or_else(|| "Invalid button entry.".to_string())?;
            for trigger in ["single", "double", "hold"] {
                let Some(action) = button_obj.get(trigger) else {
                    continue;
                };
                if action.is_null() || action.is_object() {
                    continue;
                }
                if let Some(items) = action.as_array() {
                    // Python: all(isinstance(a, dict) for a in action) — empty
                    // lists pass vacuously.
                    if items.iter().all(Value::is_object) {
                        continue;
                    }
                }
                return Err(format!("Action '{trigger}' is invalid."));
            }
        }
    }
    Ok(())
}

/// Upgrade a configuration to the current schema (in memory).
///
/// Migrations are applied in order based on the stored `schemaVersion` so an
/// old profile keeps working after a format change. New steps should append a
/// branch below and bump `CURRENT_SCHEMA_VERSION`.
pub fn migrate_config(cfg: &mut Config) {
    // Schema 0 = pre-versioning profiles: nothing structural to change yet,
    // the loading code already fills in any missing default fields.
    cfg.schema_version = CURRENT_SCHEMA_VERSION;
}
