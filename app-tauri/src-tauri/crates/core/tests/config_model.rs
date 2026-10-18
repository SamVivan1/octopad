//! Task 1 (M1): config data model, defaults, timing command, normalize —
//! ports of linux-app/config.py pinned to Python semantics.

use octopad_core::config::{
    actions_list, default_action, default_config, migrate_config, normalize_config, timing_command,
    Action, Config, CURRENT_SCHEMA_VERSION, DEFAULT_TIMINGS, PINOUT,
};
use serde_json::json;

// ---------------------------------------------------------------------------
// default_config()
// ---------------------------------------------------------------------------

#[test]
fn default_config_has_two_modes_with_eight_buttons_each() {
    let cfg = default_config();
    assert_eq!(cfg.modes.len(), 2);
    assert_eq!(cfg.modes[0].id, "desktop");
    assert_eq!(cfg.modes[0].name, "Desktop Mode");
    assert!(cfg.modes[0].led_state);
    assert_eq!(cfg.modes[1].id, "ha");
    assert_eq!(cfg.modes[1].name, "Home Assistant Mode");
    assert!(!cfg.modes[1].led_state);
    assert_eq!(cfg.modes[0].buttons.len(), 8);
    assert_eq!(cfg.modes[1].buttons.len(), 8);
}

#[test]
fn default_config_top_level_matches_python() {
    let cfg = default_config();
    assert_eq!(cfg.version, 2.6);
    assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.device, "OctoPad");
    assert_eq!(cfg.active_mode_index, 0);
    assert_eq!(cfg.active_preset, None);
    assert_eq!(json!(cfg.debounce_ms), json!(25));
    assert_eq!(json!(cfg.click_timeout_ms), json!(250));
    assert_eq!(json!(cfg.hold_timeout_ms), json!(450));
}

#[test]
fn default_config_pins_match_pinout() {
    let cfg = default_config();
    for (slot, mode) in cfg.modes.iter().enumerate() {
        for (i, button) in mode.buttons.iter().enumerate() {
            assert_eq!(button.index, i as i64, "mode {slot} button {i} index");
            assert_eq!(button.pin, PINOUT[i], "mode {slot} button {i} pin");
        }
    }
    assert_eq!(PINOUT, [20, 9, 2, 1, 21, 10, 3, 0]);
}

#[test]
fn desktop_button_0_single_is_mode_toggle_with_passcode_hold() {
    let cfg = default_config();
    let button = &cfg.modes[0].buttons[0];
    assert_eq!(button.label, "Mode / Passcode");
    assert_eq!(button.single.action_type, "mode_toggle");
    assert!(button.single.extra.is_empty());
    assert_eq!(button.double.action_type, "none");
    assert_eq!(button.hold.action_type, "text");
    assert_eq!(button.hold.extra.get("text"), Some(&json!("031004")));
}

#[test]
fn default_config_default_timings_const_matches_python() {
    assert_eq!(DEFAULT_TIMINGS.debounce_ms, 25);
    assert_eq!(DEFAULT_TIMINGS.click_timeout_ms, 250);
    assert_eq!(DEFAULT_TIMINGS.hold_timeout_ms, 450);
}

// ---------------------------------------------------------------------------
// timing_command()
// ---------------------------------------------------------------------------

#[test]
fn timing_command_formats_default_timings() {
    let cfg = default_config();
    assert_eq!(timing_command(&cfg), Some("TIMING:25,250,450".to_string()));
}

#[test]
fn timing_command_uses_stored_values() {
    let mut cfg = default_config();
    cfg.debounce_ms = json!(10);
    cfg.click_timeout_ms = json!(200);
    cfg.hold_timeout_ms = json!(600);
    assert_eq!(timing_command(&cfg), Some("TIMING:10,200,600".to_string()));
}

#[test]
fn timing_command_none_for_non_numeric_values() {
    // Python port: int("not-a-number") raises ValueError → None.
    let mut cfg = default_config();
    cfg.debounce_ms = json!("not-a-number");
    assert_eq!(timing_command(&cfg), None);

    let mut cfg = default_config();
    cfg.click_timeout_ms = json!(null);
    assert_eq!(timing_command(&cfg), None);

    let mut cfg = default_config();
    cfg.hold_timeout_ms = json!([450]);
    assert_eq!(timing_command(&cfg), None);
}

#[test]
fn timing_command_accepts_python_int_coercions() {
    // Python int() coerces numeric strings (with surrounding whitespace) and floats.
    let mut cfg = default_config();
    cfg.debounce_ms = json!(" 25 ");
    cfg.click_timeout_ms = json!(250.9);
    cfg.hold_timeout_ms = json!(true);
    assert_eq!(timing_command(&cfg), Some("TIMING:25,250,1".to_string()));

    // Out-of-range float cannot be a firmware timing → None.
    let mut cfg = default_config();
    cfg.debounce_ms = json!(1e30);
    assert_eq!(timing_command(&cfg), None);
}

// ---------------------------------------------------------------------------
// actions_list()
// ---------------------------------------------------------------------------

#[test]
fn actions_list_wraps_legacy_dict() {
    let raw = json!({"type": "text", "text": "Hi"});
    let actions = actions_list(&raw);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action_type, "text");
    assert_eq!(actions[0].extra.get("text"), Some(&json!("Hi")));
}

#[test]
fn actions_list_filters_non_dicts_from_list() {
    let raw = json!([{"type": "text", "text": "a"}, 5, "x", {"type": "delay", "seconds": 1}]);
    let actions = actions_list(&raw);
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].action_type, "text");
    assert_eq!(actions[1].action_type, "delay");
}

#[test]
fn actions_list_other_shapes_yield_empty() {
    // Python: dict → [raw]; list → dicts only; anything else → [].
    assert!(actions_list(&json!(null)).is_empty());
    assert!(actions_list(&json!("shortcut")).is_empty());
    assert!(actions_list(&json!(5)).is_empty());
    assert!(actions_list(&json!(true)).is_empty());
    assert!(actions_list(&json!([])).is_empty());
}

// ---------------------------------------------------------------------------
// default_action()
// ---------------------------------------------------------------------------

#[test]
fn default_action_shortcut_matches_python() {
    let action = default_action("shortcut");
    assert_eq!(action.action_type, "shortcut");
    assert_eq!(
        action.extra.get("modifiers"),
        Some(&json!(["ctrl", "alt", "shift"]))
    );
    assert_eq!(action.extra.get("key"), Some(&json!("A")));
}

#[test]
fn default_action_other_types_match_python() {
    let text = default_action("text");
    assert_eq!(text.extra.get("text"), Some(&json!("Hello World")));

    let media = default_action("media");
    assert_eq!(media.extra.get("mediaKey"), Some(&json!("VOL_UP")));

    let audio = default_action("audio_output");
    assert_eq!(audio.extra.get("sinks"), Some(&json!([])));

    let launch = default_action("launch_app");
    assert_eq!(launch.extra.get("desktopId"), Some(&json!("code.desktop")));
    assert_eq!(
        launch.extra.get("appName"),
        Some(&json!("Visual Studio Code"))
    );

    let system = default_action("system_action");
    assert_eq!(
        system.extra.get("presetId"),
        Some(&json!("mic_toggle_mute"))
    );
    assert_eq!(
        system.extra.get("presetName"),
        Some(&json!("Toggle Microphone Mute"))
    );

    let script = default_action("bash_script");
    assert_eq!(
        script.extra.get("script"),
        Some(&json!(r#"notify-send "OctoPad" "Action!""#))
    );

    let ha = default_action("home_assistant");
    assert_eq!(ha.extra.get("domain"), Some(&json!("")));
    assert_eq!(ha.extra.get("service"), Some(&json!("")));
    assert_eq!(ha.extra.get("entityId"), Some(&json!("")));
    assert_eq!(ha.extra.get("friendlyName"), Some(&json!("")));

    let webhook = default_action("ha_webhook");
    assert_eq!(webhook.extra.get("url"), Some(&json!("")));

    let delay = default_action("delay");
    assert_eq!(delay.extra.get("seconds"), Some(&json!(0.5)));

    let toggle = default_action("mode_toggle");
    assert_eq!(toggle.action_type, "mode_toggle");
    assert!(toggle.extra.is_empty());
}

#[test]
fn default_action_unknown_type_falls_back_to_none() {
    let action = default_action("bogus");
    assert_eq!(action.action_type, "none");
    assert!(action.extra.is_empty());
}

// ---------------------------------------------------------------------------
// normalize_config()
// ---------------------------------------------------------------------------

#[test]
fn normalize_accepts_default_config() {
    let value = serde_json::to_value(default_config()).unwrap();
    assert_eq!(normalize_config(&value), Ok(()));
}

#[test]
fn normalize_rejects_non_object() {
    assert_eq!(
        normalize_config(&json!("nope")),
        Err("Not a JSON object.".to_string())
    );
    assert_eq!(
        normalize_config(&json!([1, 2])),
        Err("Not a JSON object.".to_string())
    );
    assert_eq!(
        normalize_config(&json!(null)),
        Err("Not a JSON object.".to_string())
    );
    assert_eq!(
        normalize_config(&json!(5)),
        Err("Not a JSON object.".to_string())
    );
}

#[test]
fn normalize_rejects_bad_modes_field() {
    // Missing / not a list / fewer than 2 modes — all one Python error message.
    let one_mode = json!({"modes": [{"buttons": []}]});
    assert_eq!(
        normalize_config(&one_mode),
        Err("Invalid 'modes' field (2 modes required).".to_string())
    );
    let not_a_list = json!({"modes": "desktop"});
    assert_eq!(
        normalize_config(&not_a_list),
        Err("Invalid 'modes' field (2 modes required).".to_string())
    );
    let empty = json!({"modes": []});
    assert_eq!(
        normalize_config(&empty),
        Err("Invalid 'modes' field (2 modes required).".to_string())
    );
    let missing = json!({});
    assert_eq!(
        normalize_config(&missing),
        Err("Invalid 'modes' field (2 modes required).".to_string())
    );
}

#[test]
fn normalize_rejects_bad_buttons_structure() {
    let mode_not_dict = json!({"modes": [5, {"buttons": []}]});
    assert_eq!(
        normalize_config(&mode_not_dict),
        Err("Invalid 'modes[].buttons' structure.".to_string())
    );
    let buttons_missing = json!({"modes": [{}, {}]});
    assert_eq!(
        normalize_config(&buttons_missing),
        Err("Invalid 'modes[].buttons' structure.".to_string())
    );
    let button_not_dict = json!({"modes": [{"buttons": [7]}, {"buttons": []}]});
    assert_eq!(
        normalize_config(&button_not_dict),
        Err("Invalid button entry.".to_string())
    );
}

#[test]
fn normalize_rejects_bad_action() {
    let scalar_action = json!({"modes": [{"buttons": [{"single": 5}]}, {"buttons": []}]});
    assert_eq!(
        normalize_config(&scalar_action),
        Err("Action 'single' is invalid.".to_string())
    );
    let list_with_non_dict = json!({"modes": [{"buttons": [{"double": ["x"]}]}, {"buttons": []}]});
    assert_eq!(
        normalize_config(&list_with_non_dict),
        Err("Action 'double' is invalid.".to_string())
    );
}

#[test]
fn normalize_accepts_legacy_and_list_actions() {
    // Legacy dict action.
    let dict_action =
        json!({"modes": [{"buttons": [{"single": {"type": "text"}}]}, {"buttons": []}]});
    assert_eq!(normalize_config(&dict_action), Ok(()));
    // List actions — including the empty list (Python `all(...)` is vacuously true).
    let list_action =
        json!({"modes": [{"buttons": [{"hold": [{"type": "delay"}]}]}, {"buttons": []}]});
    assert_eq!(normalize_config(&list_action), Ok(()));
    let empty_list = json!({"modes": [{"buttons": [{"hold": []}]}, {"buttons": []}]});
    assert_eq!(normalize_config(&empty_list), Ok(()));
    // Missing or null trigger slot is skipped.
    let null_action = json!({"modes": [{"buttons": [{"single": null}]}, {"buttons": []}]});
    assert_eq!(normalize_config(&null_action), Ok(()));
    let missing_action = json!({"modes": [{"buttons": [{}]}, {"buttons": []}]});
    assert_eq!(normalize_config(&missing_action), Ok(()));
}

// ---------------------------------------------------------------------------
// migrate_config()
// ---------------------------------------------------------------------------

#[test]
fn migrate_upgrades_schema_zero_to_one() {
    let mut cfg = default_config();
    cfg.schema_version = 0;
    migrate_config(&mut cfg);
    assert_eq!(cfg.schema_version, 1);
}

#[test]
fn migrate_keeps_current_schema_version() {
    let mut cfg = default_config();
    migrate_config(&mut cfg);
    assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
}

// ---------------------------------------------------------------------------
// serde shape (camelCase wire keys)
// ---------------------------------------------------------------------------

#[test]
fn config_serializes_with_camel_case_keys() {
    let value = serde_json::to_value(default_config()).unwrap();
    assert_eq!(value.get("schemaVersion"), Some(&json!(1)));
    assert_eq!(value.get("debounceMs"), Some(&json!(25)));
    assert_eq!(value.get("clickTimeoutMs"), Some(&json!(250)));
    assert_eq!(value.get("holdTimeoutMs"), Some(&json!(450)));
    assert_eq!(value.get("activeModeIndex"), Some(&json!(0)));
    // Python's default config has no activePreset key at all.
    assert!(value.get("activePreset").is_none());
    let mode = &value["modes"][0];
    assert_eq!(mode.get("ledState"), Some(&json!(true)));
    let button = &mode["buttons"][7];
    assert_eq!(button.get("index"), Some(&json!(7)));
    assert_eq!(button.get("pin"), Some(&json!(0)));
    assert_eq!(button["single"]["type"], json!("shortcut"));
    assert_eq!(button["single"]["key"], json!("8"));
}

#[test]
fn config_round_trips_through_json() {
    let cfg = default_config();
    let value = serde_json::to_value(&cfg).unwrap();
    let back: Config = serde_json::from_value(value).unwrap();
    assert_eq!(back, cfg);
}

#[test]
fn action_extras_flatten_at_top_level() {
    let action: Action = serde_json::from_value(json!({
        "type": "home_assistant",
        "domain": "light",
        "service": "turn_on",
        "entityId": "light.kitchen",
        "friendlyName": "Kitchen"
    }))
    .unwrap();
    assert_eq!(action.action_type, "home_assistant");
    assert_eq!(action.extra.len(), 4);
    assert_eq!(action.extra.get("entityId"), Some(&json!("light.kitchen")));
    let value = serde_json::to_value(&action).unwrap();
    // Flattened: no nested "extra" object on the wire.
    assert_eq!(value["domain"], json!("light"));
    assert!(value.get("extra").is_none());
}

#[test]
fn action_without_type_field_tolerates_legacy_dicts() {
    // Python keeps any dict in a trigger slot; a missing "type" degrades to "".
    let action: Action = serde_json::from_value(json!({"key": "A"})).unwrap();
    assert_eq!(action.action_type, "");
    assert_eq!(action.extra.get("key"), Some(&json!("A")));
}
