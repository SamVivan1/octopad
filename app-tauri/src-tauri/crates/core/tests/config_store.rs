//! Task 2 (M1): config load/save with lenient per-key merge + user presets —
//! ports of linux-app/config.py pinned to Python semantics.

use octopad_core::config::{default_config, preset_slug, timing_command, Config, ConfigStore};
use serde_json::{json, Value};
use std::fs;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn store_in(dir: &tempfile::TempDir) -> ConfigStore {
    ConfigStore::at(dir.path().join("OctoPad"))
}

/// Write a raw `config.json` into the store's root (creating directories).
fn write_stored(store: &ConfigStore, value: &Value) {
    let path = store.config_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

// ---------------------------------------------------------------------------
// load_config()
// ---------------------------------------------------------------------------

#[test]
fn load_missing_file_returns_defaults() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    assert_eq!(store.load_config(), default_config());
}

#[test]
fn new_app_store_never_panics_and_targets_octopad_dir() {
    let store = ConfigStore::new_app();
    assert_eq!(store.root.file_name().unwrap(), "OctoPad");
    assert_eq!(store.config_path(), store.root.join("config.json"));
    assert_eq!(store.presets_dir(), store.root.join("presets"));
    // Missing file on a real machine: loads defaults, never panics.
    assert_eq!(store.load_config().schema_version, 1);
}

#[test]
fn save_then_load_round_trip_preserves_edits() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);

    let mut edited = default_config();
    edited.version = 3.0;
    edited.device = "OctoPad Pro".to_string();
    edited.debounce_ms = json!(10);
    edited.click_timeout_ms = json!(200);
    edited.hold_timeout_ms = json!(600);
    edited.active_mode_index = 1;
    edited.active_preset = Some("my-preset".to_string());
    edited.modes[0].name = "Work".to_string();
    edited.modes[0].buttons[2].label = "Shotgun".to_string();
    edited.modes[0].buttons[2].single = serde_json::from_value(json!({
        "type": "bash_script",
        "script": "shutter",
    }))
    .unwrap();

    store.save_config(&edited).unwrap();
    assert_eq!(store.load_config(), edited);
}

#[test]
fn save_creates_missing_directories() {
    let dir = tempdir().unwrap();
    let store = ConfigStore::at(dir.path().join("deep").join("nested").join("OctoPad"));
    assert!(store.save_config(&default_config()).is_ok());
    assert!(store.config_path().is_file());
    assert_eq!(store.load_config(), default_config());
}

#[test]
fn save_writes_plain_utf8_not_ascii_escaped() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);

    let mut cfg = default_config();
    cfg.modes[0].buttons[0].label = "Ünïcodé 😀".to_string();
    store.save_config(&cfg).unwrap();

    let text = fs::read_to_string(store.config_path()).unwrap();
    assert!(text.contains("Ünïcodé 😀"), "expected raw UTF-8 in file");
    assert!(
        !text.contains("\\u"),
        "ensure_ascii=false parity: no escapes"
    );
}

#[test]
fn load_corrupt_json_returns_defaults_without_panic() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    fs::create_dir_all(store.root.as_path()).unwrap();
    fs::write(store.config_path(), "{ this is not json").unwrap();
    assert_eq!(store.load_config(), default_config());
}

#[test]
fn load_non_object_top_level_returns_defaults() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(&store, &json!(42));
    assert_eq!(store.load_config(), default_config());
    write_stored(&store, &json!([1, 2]));
    assert_eq!(store.load_config(), default_config());
}

#[test]
fn load_wrong_typed_top_level_keys_keep_their_default_not_full_defaults() {
    // Python parity (per-key merge): a wrong-typed key keeps *its* default —
    // every other stored edit still comes through.
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(
        &store,
        &json!({
            "device": 123,
            "activeModeIndex": "1",
            "activePreset": 5,
            "modes": [
                {"buttons": [{"index": 0, "pin": 20, "label": "Custom",
                              "single": {"type": "text", "text": "Hi"}}]},
                {"buttons": []}
            ]
        }),
    );
    let cfg = store.load_config();
    assert_eq!(cfg.device, "OctoPad"); // wrong type → default kept
    assert_eq!(cfg.active_mode_index, 0); // wrong type → default kept
    assert_eq!(cfg.active_preset, None); // wrong type → default kept
    assert_eq!(cfg.version, 2.6); // untouched default survives
                                  // Stored edits are NOT discarded wholesale (NOT full defaults):
    assert_eq!(cfg.modes[0].buttons[0].label, "Custom");
    assert_eq!(cfg.modes[0].buttons[0].single.action_type, "text");
    assert_eq!(
        cfg.modes[0].buttons[0].single.extra.get("text"),
        Some(&json!("Hi"))
    );
}

#[test]
fn load_keeps_loosely_typed_timing_for_late_validation() {
    // Python test port: debounceMs "not-a-number" loads fine; the sync fails
    // later because timing_command() returns None.
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(&store, &json!({"debounceMs": "not-a-number"}));
    let cfg = store.load_config();
    assert_eq!(cfg.debounce_ms, json!("not-a-number"));
    assert_eq!(timing_command(&cfg), None);
}

#[test]
fn stored_schema_version_is_migrated_up() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(&store, &json!({"schemaVersion": 0}));
    assert_eq!(store.load_config().schema_version, 1);
}

#[test]
fn short_button_list_fills_remaining_from_default_template() {
    // _merge_button parity: fewer than 8 stored buttons → remaining slots are
    // the default template's buttons (not blank).
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(
        &store,
        &json!({
            "modes": [
                {"buttons": [
                    {"index": 0, "pin": 20, "label": "One",
                     "single": {"type": "text", "text": "a"}},
                    {"index": 1, "pin": 9, "label": "Two",
                     "single": {"type": "text", "text": "b"}}
                ]},
                {"buttons": []}
            ]
        }),
    );
    let cfg = store.load_config();
    let buttons = &cfg.modes[0].buttons;
    assert_eq!(buttons.len(), 8);
    assert_eq!(buttons[0].label, "One");
    assert_eq!(buttons[1].label, "Two");
    // Remaining slots come from the default desktop template:
    assert_eq!(buttons[2].label, "Screenshot");
    assert_eq!(buttons[2].single.action_type, "system_action");
    assert_eq!(buttons[7].single.extra.get("key"), Some(&json!("8")));
    // Second stored mode with an empty list → all default template buttons.
    assert_eq!(cfg.modes[1].buttons.len(), 8);
    assert_eq!(cfg.modes[1].buttons[0].label, "Mode / Passcode");
}

#[test]
fn load_accepts_legacy_dict_and_list_wrapped_actions() {
    // _merge_button trigger semantics: list → its dict entries; dict with a
    // truthy type → as-is; otherwise the template's trigger survives.
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(
        &store,
        &json!({
            "modes": [
                {"buttons": [{
                    "index": 0, "pin": 20, "label": "X",
                    "single": {"type": "text", "text": "Hi"},   // legacy dict
                    "double": [{"type": "media", "mediaKey": "MUTE"}], // list-wrapped
                    "hold": []                                  // empty → none
                }]},
                {"buttons": []}
            ]
        }),
    );
    let cfg = store.load_config();
    let button = &cfg.modes[0].buttons[0];
    assert_eq!(button.single.action_type, "text");
    assert_eq!(button.single.extra.get("text"), Some(&json!("Hi")));
    assert_eq!(button.double.action_type, "media");
    assert_eq!(button.double.extra.get("mediaKey"), Some(&json!("MUTE")));
    // Empty list collapses to none — NOT the template's passcode-text hold.
    assert_eq!(button.hold.action_type, "none");
    assert!(button.hold.extra.is_empty());
}

#[test]
fn load_keeps_template_trigger_when_stored_dict_lacks_type() {
    // Python: `elif isinstance(action, dict) and action.get("type")` — a dict
    // without a truthy type falls through to the template's trigger.
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(
        &store,
        &json!({
            "modes": [
                {"buttons": [{"index": 1, "pin": 9, "label": "X",
                              "single": {"key": "A"}}]},
                {"buttons": []}
            ]
        }),
    );
    let cfg = store.load_config();
    let button = &cfg.modes[0].buttons[1];
    assert_eq!(button.single.action_type, "system_action");
    assert_eq!(
        button.single.extra.get("presetId"),
        Some(&json!("open_terminal"))
    );
}

#[test]
fn load_keeps_default_modes_when_stored_modes_shape_is_unusable() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    // Not a list at all → wrong-typed top-level key → defaults + warning.
    write_stored(&store, &json!({"modes": "desktop"}));
    assert_eq!(store.load_config(), default_config());
    // A list with fewer than 2 modes: Python skips the merge silently.
    write_stored(
        &store,
        &json!({"modes": [{"buttons": [{"index": 0, "pin": 20, "label": "X"}]}]}),
    );
    assert_eq!(store.load_config(), default_config());
}

#[test]
fn load_skips_non_dict_mode_entries_and_non_dict_buttons() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    write_stored(
        &store,
        &json!({
            "modes": [
                5,                                              // not a dict → skip
                {"id": "ha", "name": "HA", "ledState": true,
                 "buttons": ["junk",                            // not a dict → template
                             {"index": 1, "pin": 9, "label": "Kept",
                              "single": {"type": "text", "text": "k"}}]}
            ]
        }),
    );
    let cfg = store.load_config();
    // Slot 0 skipped → defaults for mode 0.
    assert_eq!(cfg.modes[0].id, "desktop");
    assert_eq!(cfg.modes[0].buttons[0].label, "Mode / Passcode");
    // Slot 1: metadata merged, button 0 fell back to the template.
    assert_eq!(cfg.modes[1].id, "ha");
    assert_eq!(cfg.modes[1].name, "HA");
    assert!(cfg.modes[1].led_state);
    assert_eq!(cfg.modes[1].buttons[0].label, "Mode / Passcode");
    assert_eq!(cfg.modes[1].buttons[1].label, "Kept");
}

// ---------------------------------------------------------------------------
// preset_slug()
// ---------------------------------------------------------------------------

#[test]
fn preset_slug_matches_python() {
    assert_eq!(preset_slug("My Cool Preset!!"), "my-cool-preset");
    assert_eq!(preset_slug(""), "preset");
    assert_eq!(preset_slug("   "), "preset");
    assert_eq!(preset_slug("!!!"), "preset");
    assert_eq!(preset_slug("Hello_World"), "hello-world");
    assert_eq!(preset_slug("  padded  "), "padded");
    assert_eq!(preset_slug("a__b  c"), "a-b-c");
}

// ---------------------------------------------------------------------------
// User preset lifecycle
// ---------------------------------------------------------------------------

#[test]
fn preset_lifecycle_save_list_load_delete() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);

    let mut cfg = default_config();
    cfg.modes[0].buttons[1].label = "Term".to_string();

    // No presets yet.
    assert_eq!(store.list_presets(), Vec::<String>::new());

    store.save_preset("My Cool Preset!!", &cfg).unwrap();
    assert!(store.presets_dir().join("my-cool-preset.json").is_file());

    // Missing/unknown presets.
    assert!(store.load_preset("nope").is_none());

    // Load returns the saved snapshot.
    assert_eq!(store.load_preset("My Cool Preset!!"), Some(cfg.clone()));

    // Alphabetical order (Python sorts file names).
    store.save_preset("Beta", &cfg).unwrap();
    store.save_preset("Alpha", &cfg).unwrap();
    assert_eq!(
        store.list_presets(),
        vec!["alpha", "beta", "my-cool-preset"]
    );

    assert!(store.delete_preset("My Cool Preset!!"));
    assert_eq!(store.list_presets(), vec!["alpha", "beta"]);
    assert!(!store.delete_preset("My Cool Preset!!")); // already gone
}

#[test]
fn corrupt_preset_returns_none() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let presets = store.presets_dir();
    fs::create_dir_all(&presets).unwrap();
    fs::write(presets.join("junk.json"), "{ not json").unwrap();
    fs::write(presets.join("arr.json"), "[]").unwrap();
    assert!(store.load_preset("junk").is_none());
    assert!(store.load_preset("arr").is_none());
}

#[test]
fn preset_round_trip_survives_utf8_labels() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let mut cfg = Config::clone(&default_config());
    cfg.modes[0].buttons[0].label = "Ünïcodé 😀".to_string();
    store.save_preset("unicode", &cfg).unwrap();
    assert_eq!(store.load_preset("unicode"), Some(cfg));
}
