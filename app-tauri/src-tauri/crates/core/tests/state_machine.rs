//! Task 4: headless state machine (`state.rs`) — dirty tracking, per-method
//! emission rules and the subscriber event bus, ported from `linux-app/state.py`
//! (minus GTK/GLib).

use octopad_core::config::{default_action, default_config, ConfigStore};
use octopad_core::state::{AppEvent, AppState};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn store_in(dir: &tempfile::TempDir) -> ConfigStore {
    ConfigStore::at(dir.path().join("OctoPad"))
}

fn state_in(dir: &tempfile::TempDir) -> AppState {
    AppState::new(store_in(dir))
}

/// Collects every event a subscriber receives (usable from any thread).
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<AppEvent>>>);

impl Recorder {
    fn new() -> Recorder {
        Recorder::default()
    }

    fn events(&self) -> Vec<AppEvent> {
        self.0.lock().unwrap().clone()
    }

    fn names(&self) -> Vec<&'static str> {
        self.events()
            .iter()
            .map(|event| event.payload().0)
            .collect()
    }

    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    fn callback(&self) -> impl Fn(AppEvent) + Send + Sync + 'static {
        let sink = self.0.clone();
        move |event| sink.lock().unwrap().push(event)
    }
}

// ---------------------------------------------------------------------------
// Init / baseline
// ---------------------------------------------------------------------------

#[test]
fn starts_at_mode_index_zero_even_when_stored_active_mode_index_is_one() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let mut stored = default_config();
    stored.active_mode_index = 1;
    store.save_config(&stored).unwrap();

    let state = AppState::new(store_in(&dir));
    assert_eq!(state.mode_index(), 0);
    assert_eq!(state.current_mode().id, "desktop");
    assert_eq!(state.selected_index(), 0);
    assert!(!state.is_dirty());
    assert!(!state.has_changes());
}

#[test]
fn new_drops_stale_active_preset_when_the_preset_file_is_missing() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let mut stored = default_config();
    stored.active_preset = Some("ghost".to_string());
    store.save_config(&stored).unwrap();

    let state = AppState::new(store.clone());
    assert_eq!(state.active_preset(), None);

    store.save_preset("ghost", &default_config()).unwrap();
    let state = AppState::new(store);
    assert_eq!(state.active_preset(), Some("ghost".to_string()));
}

#[test]
fn state_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AppState>();
    assert_send_sync::<AppEvent>();
}

// ---------------------------------------------------------------------------
// Dirty tracking
// ---------------------------------------------------------------------------

#[test]
fn dirty_only_flips_on_a_real_edit() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    // Same value → the button event still fires, dirty must not flip.
    state.set_label("Mode / Passcode");
    assert_eq!(recorder.names(), vec!["button-changed"]);
    assert!(!state.has_changes());

    recorder.clear();
    state.set_label("Renamed");
    assert_eq!(recorder.names(), vec!["dirty-changed", "button-changed"]);
    assert!(state.has_changes());
    assert!(state.is_dirty());

    recorder.clear();
    state.set_label("Mode / Passcode");
    assert_eq!(recorder.names(), vec!["dirty-changed", "button-changed"]);
    assert!(!state.has_changes());
    assert!(!state.is_dirty());
}

#[test]
fn non_edit_keys_active_mode_index_and_active_preset_never_mark_dirty() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    state.set_mode(1);
    assert_eq!(recorder.names(), vec!["mode-changed"]);
    assert!(!state.has_changes());

    // activePreset bookkeeping from a preset save must not look like an edit.
    recorder.clear();
    assert!(state.save_preset("Snapshot", false));
    assert_eq!(state.active_preset(), Some("snapshot".to_string()));
    assert!(!state.has_changes());
    assert_eq!(recorder.names(), vec!["config-saved"]);

    // …nor may a replaced config differing only in the non-edit keys.
    let mut other = state.config();
    other.active_mode_index = 0;
    other.active_preset = Some("ghost".to_string());
    recorder.clear();
    state.set_config(other, "test");
    assert_eq!(recorder.names(), vec!["config-replaced"]);
    assert!(!state.has_changes());
}

#[test]
fn mark_dirty_emits_only_on_a_flip() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    state.set_label("Edited");
    assert!(state.is_dirty());

    recorder.clear();
    state.mark_dirty();
    assert!(recorder.events().is_empty());
}

// ---------------------------------------------------------------------------
// Action list mutation (TriggerActions parity)
// ---------------------------------------------------------------------------

#[test]
fn set_action_replaces_at_index_and_appends_beyond_the_end() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    // Default desktop B1 single is the legacy dict shape (mode_toggle).
    state.set_action("single", 0, default_action("text"));
    let actions = state.get_actions("single");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action_type, "text");
    assert_eq!(recorder.names(), vec!["dirty-changed", "button-changed"]);

    // index beyond the end appends (Python `else: actions.append(action)`).
    state.set_action("single", 9, default_action("none"));
    let actions = state.get_actions("single");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].action_type, "text");
    assert_eq!(actions[1].action_type, "none");
}

#[test]
fn add_action_appends_none_by_default_and_a_given_action_otherwise() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);

    // Default double for B1 is a single `none` action.
    state.add_action("double", None);
    let actions = state.get_actions("double");
    assert_eq!(actions.len(), 2);
    assert!(actions.iter().all(|action| action.action_type == "none"));

    state.add_action("double", Some(default_action("media")));
    let actions = state.get_actions("double");
    assert_eq!(actions.len(), 2 + 1);
    assert_eq!(actions[2].action_type, "media");
}

#[test]
fn remove_action_last_slot_leaves_one_none_action() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    // Default hold for B1 is a single text action.
    state.add_action("hold", None);
    state.add_action("hold", None);
    assert_eq!(state.get_actions("hold").len(), 3);

    state.remove_action("hold", 0);
    assert_eq!(state.get_actions("hold").len(), 2);
    state.remove_action("hold", 0);
    assert_eq!(state.get_actions("hold").len(), 1);

    recorder.clear();
    state.remove_action("hold", 0);
    let left = state.get_actions("hold");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].action_type, "none");
    assert_eq!(recorder.names(), vec!["button-changed"]);

    // Out-of-range removal is a no-op that still leaves one `none` slot.
    state.remove_action("hold", 5);
    assert_eq!(state.get_actions("hold").len(), 1);
    assert_eq!(state.get_actions("hold")[0].action_type, "none");
}

#[test]
fn move_action_swaps_and_out_of_range_is_a_silent_noop() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    state.set_action("single", 0, default_action("text"));
    state.add_action("single", Some(default_action("media")));
    assert_eq!(state.get_actions("single").len(), 2);

    recorder.clear();
    state.move_action("single", 0, 1);
    let actions = state.get_actions("single");
    assert_eq!(actions[0].action_type, "media");
    assert_eq!(actions[1].action_type, "text");
    assert_eq!(recorder.names(), vec!["button-changed"]);

    state.move_action("single", 1, -1);
    assert_eq!(state.get_actions("single")[0].action_type, "text");

    recorder.clear();
    state.move_action("single", 0, -1); // target < 0
    state.move_action("single", 9, 1); // index >= len
    state.move_action("single", -1, 0); // index < 0
    assert!(recorder.events().is_empty());
    assert_eq!(state.get_actions("single")[0].action_type, "text");
}

// ---------------------------------------------------------------------------
// save() / presets
// ---------------------------------------------------------------------------

#[test]
fn save_clears_dirty_and_emits_config_saved_ok() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let state = AppState::new(store.clone());
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    state.set_label("Saved Label");
    assert!(state.has_changes());

    recorder.clear();
    assert!(state.save());
    assert_eq!(recorder.names(), vec!["dirty-changed", "config-saved"]);
    let config_saved = recorder
        .events()
        .into_iter()
        .find_map(|event| match event {
            AppEvent::ConfigSaved { ok, preset } => Some((ok, preset)),
            _ => None,
        })
        .unwrap();
    assert_eq!(config_saved, (true, None));

    assert!(!state.has_changes());
    assert!(!state.is_dirty());

    let reloaded = store_in(&dir).load_config();
    assert_eq!(reloaded.modes[0].buttons[0].label, "Saved Label");
}

#[test]
fn save_preset_snapshot_drops_active_preset_and_activates_the_preset() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let state = AppState::new(store.clone());
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    state.set_label("Preset Label");
    assert!(state.has_changes());

    recorder.clear();
    assert!(state.save_preset("My Cool Preset!!", false));
    assert_eq!(state.active_preset(), Some("my-cool-preset".to_string()));
    assert!(!state.has_changes());

    let snapshot = store.load_preset("My Cool Preset!!").unwrap();
    assert_eq!(snapshot.active_preset, None);
    assert_eq!(snapshot.modes[0].buttons[0].label, "Preset Label");

    let persisted = store.load_config();
    assert_eq!(persisted.active_preset, Some("my-cool-preset".to_string()));
    assert_eq!(persisted.modes[0].buttons[0].label, "Preset Label");

    assert!(matches!(
        recorder.events().last(),
        Some(AppEvent::ConfigSaved { ok: true, preset: Some(name) }) if name == "my-cool-preset"
    ));
}

#[test]
fn save_preset_overwrite_with_empty_name_falls_back_to_the_active_preset() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let state = AppState::new(store.clone());

    assert!(state.save_preset("First", false));
    assert_eq!(state.active_preset(), Some("first".to_string()));

    state.set_label("Second");
    assert!(state.save_preset("", true));
    assert_eq!(state.active_preset(), Some("first".to_string()));

    let snapshot = store.load_preset("first").unwrap();
    assert_eq!(snapshot.modes[0].buttons[0].label, "Second");
}

#[test]
fn save_preset_overwrite_without_any_active_preset_emits_failure() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let state = AppState::new(store.clone());
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    assert!(!state.save_preset("", true));
    assert_eq!(state.active_preset(), None);
    assert!(matches!(
        recorder.events().last(),
        Some(AppEvent::ConfigSaved {
            ok: false,
            preset: None
        })
    ));
    // Nothing was written anywhere.
    assert!(!store.config_path().exists());
    assert!(store.list_presets().is_empty());
}

#[test]
fn apply_user_preset_sets_the_active_flag_before_set_config_and_leaves_dirty() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);

    let mut custom = default_config();
    custom.modes[0].buttons[0].label = "From Preset".to_string();
    store.save_preset("custom", &custom).unwrap();

    let state = Arc::new(AppState::new(store));
    let recorder = Recorder::new();
    let observed = state.clone();
    let seen_active = Arc::new(Mutex::new(Vec::new()));
    let seen_active_cb = seen_active.clone();
    let _observer = state.subscribe(move |event| {
        if let AppEvent::ConfigReplaced { source } = &event {
            if source == "preset" {
                // The active flag must already be set when listeners run, and
                // subscribers run outside the locks so reading back is safe.
                seen_active_cb
                    .lock()
                    .unwrap()
                    .push(observed.active_preset());
                let _ = observed.current_button();
            }
        }
    });
    let _collector = state.subscribe(recorder.callback());

    assert!(state.apply_user_preset("custom"));
    assert_eq!(state.active_preset(), Some("custom".to_string()));
    assert_eq!(
        *seen_active.lock().unwrap(),
        vec![Some("custom".to_string())]
    );
    assert_eq!(state.current_button().label, "From Preset");
    assert_eq!(recorder.names(), vec!["config-replaced", "dirty-changed"]);
    assert!(state.is_dirty());
    assert!(state.has_changes());

    assert!(!state.apply_user_preset("does-not-exist"));
}

#[test]
fn delete_user_preset_refuses_to_persist_when_dirty() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let state = AppState::new(store.clone());
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    assert!(state.save_preset("keep", false));
    assert_eq!(state.active_preset(), Some("keep".to_string()));

    state.set_label("Unsaved Edit");
    assert!(state.has_changes());

    recorder.clear();
    assert!(state.delete_user_preset("keep"));
    assert_eq!(state.active_preset(), None);
    assert_eq!(recorder.names(), vec!["presets-changed"]);
    assert!(state.has_changes());

    // The metadata change must NOT clobber the unsaved edit on disk.
    let persisted = store.load_config();
    assert_eq!(persisted.modes[0].buttons[0].label, "Mode / Passcode");
    assert!(store.list_presets().is_empty());

    // Deleting a missing preset reports false and emits nothing.
    recorder.clear();
    assert!(!state.delete_user_preset("missing"));
    assert!(recorder.events().is_empty());
}

#[test]
fn delete_user_preset_persists_the_metadata_when_clean() {
    let dir = tempdir().unwrap();
    let store = store_in(&dir);
    let state = AppState::new(store.clone());
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    assert!(state.save_preset("keep", false));
    assert!(!state.has_changes());

    recorder.clear();
    assert!(state.delete_user_preset("keep"));
    assert_eq!(state.active_preset(), None);
    assert_eq!(recorder.names(), vec!["presets-changed"]);
    assert_eq!(store.load_config().active_preset, None);
}

// ---------------------------------------------------------------------------
// set_config
// ---------------------------------------------------------------------------

#[test]
fn set_config_replaces_working_config_and_runs_the_dirty_flow() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    let mut imported = default_config();
    imported.modes[0].buttons[0].label = "Imported".to_string();
    imported.active_mode_index = 1;

    state.set_config(imported, "import");
    assert_eq!(recorder.names(), vec!["config-replaced", "dirty-changed"]);
    assert!(state.has_changes());
    assert_eq!(state.current_button().label, "Imported");
    // A config replacement does not reset the startup mode index.
    assert_eq!(state.mode_index(), 0);
}

// ---------------------------------------------------------------------------
// Emission gating
// ---------------------------------------------------------------------------

#[test]
fn label_and_timing_always_emit_even_for_same_value_edits() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    state.set_label("Mode / Passcode");
    assert_eq!(recorder.names(), vec!["button-changed"]);

    recorder.clear();
    assert!(state.set_timing("debounceMs", 25));
    assert_eq!(recorder.names(), vec!["timing-changed"]);
    assert!(!state.has_changes());
    assert_eq!(
        recorder.events()[0].payload(),
        (
            "timing-changed",
            json!({"field": "debounceMs", "value": 25})
        )
    );
}

#[test]
fn mode_and_selection_early_return_without_an_event_when_unchanged() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    assert!(state.set_mode(0));
    assert!(state.select_button(0));
    assert!(recorder.events().is_empty());

    assert!(state.set_mode(1));
    assert_eq!(recorder.names(), vec!["mode-changed"]);
    assert!(!state.has_changes());

    recorder.clear();
    assert!(state.select_button(2));
    assert_eq!(recorder.names(), vec!["selection-changed"]);
    assert_eq!(state.selected_index(), 2);
    assert!(!state.has_changes());
}

#[test]
fn invalid_mode_and_button_indices_leave_state_and_events_unchanged() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());
    let config_before = state.config();

    assert!(!state.set_mode(-1));
    assert!(!state.set_mode(2));
    assert!(!state.select_button(-1));
    assert!(!state.select_button(8));

    assert_eq!(state.config(), config_before);
    assert_eq!(state.mode_index(), 0);
    assert_eq!(state.selected_index(), 0);
    assert!(recorder.events().is_empty());
}

#[test]
fn set_timing_accepts_only_the_three_camel_case_fields() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    assert!(!state.set_timing("bounceMs", 5));
    assert!(!state.set_timing("debounce_ms", 5));
    assert!(!state.set_timing("", 5));
    assert!(recorder.events().is_empty());
    assert_eq!(state.config().debounce_ms, json!(25));

    assert!(state.set_timing("clickTimeoutMs", 300));
    assert!(state.set_timing("holdTimeoutMs", 500));
    assert_eq!(state.config().click_timeout_ms, json!(300));
    assert_eq!(state.config().hold_timeout_ms, json!(500));
    // The first real timing edit flips dirty before its own event.
    assert_eq!(
        recorder.names(),
        vec!["dirty-changed", "timing-changed", "timing-changed"]
    );
}

#[test]
fn status_setters_dedupe_their_events() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    // Serial
    state.set_serial_status(true, Some("/dev/ttyUSB0".to_string()));
    state.set_serial_status(true, Some("/dev/ttyUSB0".to_string()));
    assert_eq!(recorder.names(), vec!["serial-status-changed"]);
    state.set_serial_status(true, Some("/dev/ttyACM0".to_string()));
    assert_eq!(recorder.names().len(), 2);
    assert!(state.serial_connected());
    assert_eq!(state.serial_port(), Some("/dev/ttyACM0".to_string()));

    // BLE
    recorder.clear();
    state.set_ble_status(true, "connected");
    state.set_ble_status(true, "connected");
    assert_eq!(recorder.names(), vec!["ble-status-changed"]);
    state.set_ble_status(false, "Pad disconnected — retrying…");
    assert_eq!(recorder.names().len(), 2);
    assert!(!state.ble_connected());
    assert_eq!(state.ble_message(), "Pad disconnected — retrying…");

    // Home Assistant
    recorder.clear();
    state.set_ha_status(true, "Connected");
    state.set_ha_status(true, "Connected");
    assert_eq!(recorder.names(), vec!["ha-status-changed"]);
    state.set_ha_status(true, "Reconnecting");
    assert_eq!(recorder.names().len(), 2);
    assert!(state.ha_connected());
    assert_eq!(state.ha_message(), "Reconnecting");

    // Device name (trimmed, empty → None)
    recorder.clear();
    state.set_device_name(Some("  Pad  ".to_string()));
    assert_eq!(state.device_name(), Some("Pad".to_string()));
    state.set_device_name(Some("Pad".to_string()));
    assert_eq!(recorder.names(), vec!["device-name-changed"]);
    state.set_device_name(Some("   ".to_string()));
    assert_eq!(state.device_name(), None);
    assert_eq!(recorder.names().len(), 2);

    // Firmware version (trimmed, empty → None)
    recorder.clear();
    state.set_firmware_version(Some("1.2.3".to_string()));
    state.set_firmware_version(Some("1.2.3".to_string()));
    assert_eq!(recorder.names(), vec!["device-info-changed"]);
    assert_eq!(state.firmware_version(), Some("1.2.3".to_string()));

    // HA entities
    recorder.clear();
    state.set_ha_entities(3);
    state.set_ha_entities(3);
    assert_eq!(recorder.names(), vec!["ha-entities-changed"]);
    state.set_ha_entities(4);
    assert_eq!(recorder.names().len(), 2);
    assert_eq!(state.ha_entities_count(), 4);

    // Installed apps
    recorder.clear();
    state.set_apps(10);
    state.set_apps(10);
    assert_eq!(recorder.names(), vec!["apps-changed"]);
    state.set_apps(11);
    assert_eq!(recorder.names().len(), 2);
    assert_eq!(state.installed_apps_count(), 11);
}

// ---------------------------------------------------------------------------
// Event payload table
// ---------------------------------------------------------------------------

fn assert_payload(event: AppEvent, name: &str, expected: Value) {
    let (actual_name, payload) = event.payload();
    assert_eq!(actual_name, name, "event name mismatch for {expected}");
    assert!(payload.is_object(), "payload must be a flat object");
    assert_eq!(payload, expected, "payload mismatch for {name}");
}

#[test]
fn event_payloads_match_the_flat_snake_case_table() {
    assert_payload(
        AppEvent::DirtyChanged { dirty: true },
        "dirty-changed",
        json!({"dirty": true}),
    );
    assert_payload(
        AppEvent::ConfigReplaced {
            source: "import".to_string(),
        },
        "config-replaced",
        json!({"source": "import"}),
    );
    assert_payload(
        AppEvent::ButtonChanged { index: 3 },
        "button-changed",
        json!({"index": 3}),
    );
    assert_payload(
        AppEvent::TimingChanged {
            field: "debounceMs".to_string(),
            value: 10,
        },
        "timing-changed",
        json!({"field": "debounceMs", "value": 10}),
    );
    assert_payload(
        AppEvent::ModeChanged { mode_index: 1 },
        "mode-changed",
        json!({"mode_index": 1}),
    );
    assert_payload(
        AppEvent::SelectionChanged { index: 2 },
        "selection-changed",
        json!({"index": 2}),
    );
    assert_payload(
        AppEvent::ConfigSaved {
            ok: true,
            preset: Some("my-preset".to_string()),
        },
        "config-saved",
        json!({"ok": true, "preset": "my-preset"}),
    );
    assert_payload(
        AppEvent::ConfigSaved {
            ok: false,
            preset: None,
        },
        "config-saved",
        json!({"ok": false, "preset": null}),
    );
    assert_payload(
        AppEvent::PresetsChanged {
            name: "keep".to_string(),
        },
        "presets-changed",
        json!({"name": "keep"}),
    );
    assert_payload(
        AppEvent::SerialStatusChanged {
            connected: true,
            port: Some("/dev/ttyUSB0".to_string()),
        },
        "serial-status-changed",
        json!({"connected": true, "port": "/dev/ttyUSB0"}),
    );
    assert_payload(
        AppEvent::SerialStatusChanged {
            connected: false,
            port: None,
        },
        "serial-status-changed",
        json!({"connected": false, "port": null}),
    );
    assert_payload(
        AppEvent::BleStatusChanged {
            connected: true,
            message: "connected".to_string(),
        },
        "ble-status-changed",
        json!({"connected": true, "message": "connected"}),
    );
    assert_payload(
        AppEvent::DeviceNameChanged {
            name: Some("Pad".to_string()),
        },
        "device-name-changed",
        json!({"name": "Pad"}),
    );
    assert_payload(
        AppEvent::DeviceNameChanged { name: None },
        "device-name-changed",
        json!({"name": null}),
    );
    assert_payload(
        AppEvent::DeviceInfoChanged {
            version: Some("1.2.3".to_string()),
        },
        "device-info-changed",
        json!({"version": "1.2.3"}),
    );
    assert_payload(
        AppEvent::SerialLog {
            line: "[TX] WHO".to_string(),
        },
        "serial-log",
        json!({"line": "[TX] WHO"}),
    );
    assert_payload(
        AppEvent::HaStatusChanged {
            connected: true,
            message: "ok".to_string(),
        },
        "ha-status-changed",
        json!({"connected": true, "message": "ok"}),
    );
    assert_payload(
        AppEvent::HaEntitiesChanged { count: 5 },
        "ha-entities-changed",
        json!({"count": 5}),
    );
    assert_payload(
        AppEvent::AppsChanged { count: 7 },
        "apps-changed",
        json!({"count": 7}),
    );
}

// ---------------------------------------------------------------------------
// Event bus
// ---------------------------------------------------------------------------

#[test]
fn subscriber_panic_does_not_break_dispatch() {
    let dir = tempdir().unwrap();
    let state = Arc::new(state_in(&dir));
    let recorder = Recorder::new();

    let _panicky = state.subscribe(|_event| panic!("subscriber blew up"));
    let _collector = state.subscribe(recorder.callback());

    state.set_label("After Panic");
    assert_eq!(recorder.names(), vec!["dirty-changed", "button-changed"]);

    // A second dispatch still works after the panic.
    recorder.clear();
    state.set_label("Again");
    assert_eq!(recorder.names(), vec!["button-changed"]);

    // The panicking subscriber keeps receiving events (it was not removed).
    recorder.clear();
    state.set_mode(1);
    assert_eq!(recorder.names(), vec!["mode-changed"]);
}

#[test]
fn unsubscribe_stops_delivery_and_is_idempotent() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();

    let subscription = state.subscribe(recorder.callback());
    state.set_label("One");
    assert_eq!(recorder.names().len(), 2);

    subscription.unsubscribe();
    state.set_label("Two");
    assert_eq!(recorder.names().len(), 2);
    subscription.unsubscribe(); // idempotent

    // Subscribing again keeps working.
    let _second = state.subscribe(recorder.callback());
    state.set_label("Three");
    assert_eq!(recorder.names().len(), 3);
}

#[test]
fn unsubscribe_by_id_stops_delivery() {
    let dir = tempdir().unwrap();
    let state = state_in(&dir);
    let recorder = Recorder::new();

    let subscription = state.subscribe(recorder.callback());
    let id = subscription.id();
    state.set_label("One");
    assert_eq!(recorder.names().len(), 2);

    state.unsubscribe(id);
    state.set_label("Two");
    assert_eq!(recorder.names().len(), 2);

    // Unknown id is a no-op.
    state.unsubscribe(id + 1000);
}

#[test]
fn subscribers_run_outside_the_locks_and_may_mutate_state() {
    let dir = tempdir().unwrap();
    let state = Arc::new(state_in(&dir));
    let recorder = Recorder::new();

    let nested = state.clone();
    let _reentrant = state.subscribe(move |event| {
        if let AppEvent::ButtonChanged { .. } = event {
            // Re-entrant mutation: deadlocks if dispatch held the state lock.
            nested.set_mode(1);
            nested.select_button(3);
        }
    });
    let _collector = state.subscribe(recorder.callback());

    state.set_label("Reentrant");
    let names = recorder.names();
    assert!(names.contains(&"button-changed"));
    assert!(names.contains(&"mode-changed"));
    assert!(names.contains(&"selection-changed"));
    assert_eq!(state.mode_index(), 1);
    assert_eq!(state.selected_index(), 3);
}

#[test]
fn concurrent_mutations_from_many_threads_do_not_deadlock_or_lose_events() {
    let dir = tempdir().unwrap();
    let state = Arc::new(state_in(&dir));
    let recorder = Recorder::new();
    let _sub = state.subscribe(recorder.callback());

    let mut handles = Vec::new();
    for worker in 0..4 {
        let state = state.clone();
        handles.push(std::thread::spawn(move || {
            for step in 0..50 {
                state.set_label(&format!("Worker {worker} step {step}"));
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }

    let names = recorder.names();
    assert_eq!(
        names
            .iter()
            .filter(|name| **name == "button-changed")
            .count(),
        200
    );
    // Exactly one thread observed the dirty flip; the others saw it already set.
    assert_eq!(
        names
            .iter()
            .filter(|name| **name == "dirty-changed")
            .count(),
        1
    );
    assert!(state.is_dirty());
}
