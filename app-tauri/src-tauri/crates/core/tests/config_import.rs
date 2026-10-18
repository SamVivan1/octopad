//! Task 3 (M1): re-runnable legacy config importer.
//!
//! Ports the legacy Python app's on-disk store (`~/.config/octopad` on Linux,
//! `%APPDATA%\OctoPad` on Windows) into the new app's own store. The import is
//! a full replace of the new config from the legacy file and is deliberately
//! **re-runnable**: a later import picks up whatever the legacy app has now
//! (plan Task 3, Review Focus Input 6).
//!
//! Windows "profiles" are import/export files with no on-disk legacy store, so
//! profile import is intentionally out of scope here (deferred to M3).

use octopad_core::config::{
    auto_import_legacy, default_config, import_legacy, import_legacy_first_run, ConfigStore,
    ImportError, ImportReport,
};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_store(dir: &tempfile::TempDir) -> ConfigStore {
    ConfigStore::at(dir.path().join("OctoPad"))
}
/// A realistic legacy `config.json`: 2 modes, a custom label on button 0, a
/// legacy dict-shaped action, schemaVersion 0 (needs migration).
fn legacy_config_value(label: &str) -> Value {
    json!({
        "version": 2.6,
        "schemaVersion": 0,
        "device": "OctoPad",
        "debounceMs": 25,
        "clickTimeoutMs": 250,
        "holdTimeoutMs": 450,
        "activeModeIndex": 1,
        "activePreset": "legacy-preset",
        "modes": [
            {
                "id": "desktop",
                "name": "Desktop Mode",
                "ledState": true,
                "buttons": [
                    {
                        "index": 0,
                        "pin": 20,
                        "label": label,
                        "single": {"type": "text", "text": "Legacy!"},
                        "double": {"type": "none"},
                        "hold": {"type": "none"}
                    }
                ]
            },
            {
                "id": "ha",
                "name": "Home Assistant Mode",
                "ledState": false,
                "buttons": []
            }
        ]
    })
}

fn write_legacy_config(legacy_dir: &Path, value: &Value) {
    fs::create_dir_all(legacy_dir).unwrap();
    fs::write(
        legacy_dir.join("config.json"),
        serde_json::to_string_pretty(value).unwrap(),
    )
    .unwrap();
}

/// A full preset snapshot (so `ConfigStore::load_preset` returns it verbatim).
fn preset_value(label: &str) -> Value {
    let mut config = serde_json::to_value(default_config()).unwrap();
    config["modes"][0]["buttons"][0]["label"] = json!(label);
    config
}

fn assert_no_import_temp_files(store: &ConfigStore) {
    let Ok(entries) = fs::read_dir(&store.root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.ends_with(".tmp"),
            "leftover importer temp file: {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// Explicit import: happy path
// ---------------------------------------------------------------------------

#[test]
fn imports_legacy_config_into_new_store() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("Imported Label"));

    let report = import_legacy(&store, &legacy).unwrap();

    assert!(report.imported_config);
    assert!(!report.first_run_auto);
    assert!(report.presets.is_empty());
    assert!(
        report.warnings.is_empty(),
        "unexpected: {:?}",
        report.warnings
    );

    let cfg = store.load_config();
    assert_eq!(cfg.schema_version, 1, "legacy schema 0 must be migrated");
    assert_eq!(cfg.modes[0].buttons[0].label, "Imported Label");
    assert_eq!(cfg.active_mode_index, 1);
    assert_eq!(cfg.active_preset.as_deref(), Some("legacy-preset"));
    // Legacy dict-shaped action survived the import.
    let single = &cfg.modes[0].buttons[0].single.as_slice()[0];
    assert_eq!(single.action_type, "text");
    assert_eq!(single.extra.get("text"), Some(&json!("Legacy!")));
    // Button slots missing from the legacy file fall back to the defaults.
    assert_eq!(cfg.modes[0].buttons[1].label, "Terminal");

    // The on-disk file is valid JSON with the migrated schema version.
    let written: Value =
        serde_json::from_str(&fs::read_to_string(store.config_path()).unwrap()).unwrap();
    assert_eq!(written["schemaVersion"], json!(1));
    assert_eq!(
        written["modes"][0]["buttons"][0]["label"],
        json!("Imported Label")
    );
    assert_no_import_temp_files(&store);
}

#[test]
fn import_replaces_existing_new_config_entirely() {
    // Import is a full replace from the legacy file — new-app-only edits are
    // not merged on top.
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("From Legacy"));

    let mut existing = default_config();
    existing.device = "New-App-Only".to_string();
    existing.modes[0].buttons[0].label = "New App Label".to_string();
    store.save_config(&existing).unwrap();

    import_legacy(&store, &legacy).unwrap();

    let cfg = store.load_config();
    assert_eq!(cfg.modes[0].buttons[0].label, "From Legacy");
    assert_eq!(
        cfg.device, "OctoPad",
        "legacy value wins, not the new app's"
    );
}

// ---------------------------------------------------------------------------
// Explicit import: error paths leave the new store untouched
// ---------------------------------------------------------------------------

#[test]
fn missing_legacy_config_is_not_found() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);

    // Legacy dir exists but has no config.json.
    let legacy = tmp.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    assert!(matches!(
        import_legacy(&store, &legacy).unwrap_err(),
        ImportError::NotFound
    ));

    // Legacy dir does not exist at all.
    let missing = tmp.path().join("nope");
    assert!(matches!(
        import_legacy(&store, &missing).unwrap_err(),
        ImportError::NotFound
    ));

    assert!(!store.config_path().exists());
}

#[test]
fn corrupt_legacy_json_is_parse_error_and_new_store_untouched() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("config.json"), "{ this is not json").unwrap();

    let err = import_legacy(&store, &legacy).unwrap_err();
    assert!(matches!(err, ImportError::Parse(_)), "{err:?}");

    // New store untouched: still defaults, no file written, no temp litter.
    assert!(!store.config_path().exists());
    assert_eq!(store.load_config(), default_config());
    assert_no_import_temp_files(&store);
}

#[test]
fn non_object_legacy_json_is_invalid() {
    // Valid JSON, invalid structure → Invalid (not Parse).
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("config.json"), "[1, 2, 3]").unwrap();

    let err = import_legacy(&store, &legacy).unwrap_err();
    assert!(matches!(err, ImportError::Invalid(_)), "{err:?}");
    assert!(!store.config_path().exists());
}

#[test]
fn failed_import_leaves_existing_new_config_untouched() {
    // A bad legacy file must never damage the new app's saved config.
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("config.json"), "{ broken").unwrap();

    let mut existing = default_config();
    existing.modes[0].buttons[0].label = "Keep Me".to_string();
    store.save_config(&existing).unwrap();

    assert!(matches!(
        import_legacy(&store, &legacy).unwrap_err(),
        ImportError::Parse(_)
    ));
    assert_eq!(store.load_config(), existing);
}

#[test]
fn io_failure_while_replacing_reports_io_error() {
    // Occupy the destination path with a directory so the atomic rename fails;
    // the importer reports Io and leaves no temp file behind.
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("X"));
    fs::create_dir_all(store.config_path()).unwrap();

    let err = import_legacy(&store, &legacy).unwrap_err();
    assert!(matches!(err, ImportError::Io(_)), "{err:?}");
    assert_no_import_temp_files(&store);
}

#[test]
fn single_mode_legacy_config_is_invalid() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(
        &legacy,
        &json!({
            "schemaVersion": 1,
            "modes": [{"id": "desktop", "name": "Desktop", "ledState": true, "buttons": []}]
        }),
    );

    let err = import_legacy(&store, &legacy).unwrap_err();
    match err {
        ImportError::Invalid(message) => {
            assert!(message.contains("modes"), "message: {message}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(!store.config_path().exists());
}

#[test]
fn malformed_action_in_legacy_config_is_invalid() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    let mut value = legacy_config_value("X");
    // A scalar in a trigger slot: normalize_config's "Action 'single' is invalid."
    value["modes"][0]["buttons"][0]["single"] = json!(5);
    write_legacy_config(&legacy, &value);

    let err = import_legacy(&store, &legacy).unwrap_err();
    match err {
        ImportError::Invalid(message) => {
            assert!(message.contains("single"), "{message}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(!store.config_path().exists());
}

// ---------------------------------------------------------------------------
// Re-runnability (Review Focus Input 6 / Global Constraint)
// ---------------------------------------------------------------------------

#[test]
fn import_is_re_runnable_and_picks_up_legacy_edits() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");

    write_legacy_config(&legacy, &legacy_config_value("First Pass"));
    import_legacy(&store, &legacy).unwrap();
    assert_eq!(store.load_config().modes[0].buttons[0].label, "First Pass");

    // The user keeps editing in the legacy app, then imports again.
    let mut second = legacy_config_value("Second Pass");
    second["holdTimeoutMs"] = json!(600);
    write_legacy_config(&legacy, &second);

    let report = import_legacy(&store, &legacy).unwrap();
    assert!(report.imported_config);
    let cfg = store.load_config();
    assert_eq!(cfg.modes[0].buttons[0].label, "Second Pass");
    assert_eq!(cfg.hold_timeout_ms, json!(600));
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

#[test]
fn copies_legacy_presets_overwriting_same_slug() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("X"));

    // A pre-existing new-app preset with the same slug gets overwritten.
    store
        .save_preset("My Cool Preset!!", &default_config())
        .unwrap();

    let legacy_presets = legacy.join("presets");
    fs::create_dir_all(&legacy_presets).unwrap();
    fs::write(
        legacy_presets.join("My Cool Preset!!.json"),
        serde_json::to_string_pretty(&preset_value("Legacy Override")).unwrap(),
    )
    .unwrap();
    fs::write(
        legacy_presets.join("Weird Name!!.json"),
        serde_json::to_string_pretty(&preset_value("Weird One")).unwrap(),
    )
    .unwrap();
    // Non-JSON files are ignored.
    fs::write(legacy_presets.join("notes.txt"), "ignore me").unwrap();

    let report = import_legacy(&store, &legacy).unwrap();

    assert_eq!(report.presets, vec!["my-cool-preset", "weird-name"]);
    let overridden = store.load_preset("my-cool-preset").unwrap();
    assert_eq!(overridden.modes[0].buttons[0].label, "Legacy Override");
    let weird = store.load_preset("weird-name").unwrap();
    assert_eq!(weird.modes[0].buttons[0].label, "Weird One");
    // The unrelated preset from before still exists.
    assert!(store.presets_dir().join("my-cool-preset.json").is_file());
}

#[test]
fn missing_legacy_presets_dir_is_not_an_error() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("X"));

    let report = import_legacy(&store, &legacy).unwrap();
    assert!(report.imported_config);
    assert!(report.presets.is_empty());
    assert!(report.warnings.is_empty());
}

#[test]
fn unreadable_preset_becomes_a_warning_not_a_failure() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("X"));

    let legacy_presets = legacy.join("presets");
    fs::create_dir_all(&legacy_presets).unwrap();
    // A directory named "broken.json" cannot be read as a file.
    fs::create_dir_all(legacy_presets.join("broken.json")).unwrap();
    fs::write(
        legacy_presets.join("good.json"),
        serde_json::to_string_pretty(&preset_value("Good")).unwrap(),
    )
    .unwrap();

    let report = import_legacy(&store, &legacy).unwrap();
    assert!(report.imported_config);
    assert_eq!(report.presets, vec!["good"]);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(
        report.warnings[0].contains("broken.json"),
        "{:?}",
        report.warnings
    );
}

// ---------------------------------------------------------------------------
// First-run auto-import rule (Global Constraints)
// ---------------------------------------------------------------------------

#[test]
fn first_run_auto_imports_when_new_config_missing_and_legacy_present() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("Auto Imported"));

    let report = import_legacy_first_run(&store, &legacy).unwrap();

    assert!(report.first_run_auto);
    assert!(report.imported_config);
    assert!(store.config_path().is_file());
    assert_eq!(
        store.load_config().modes[0].buttons[0].label,
        "Auto Imported"
    );
}

#[test]
fn first_run_does_nothing_when_new_config_already_exists() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    write_legacy_config(&legacy, &legacy_config_value("Legacy Label"));

    let mut existing = default_config();
    existing.modes[0].buttons[0].label = "Already Mine".to_string();
    store.save_config(&existing).unwrap();

    let report = import_legacy_first_run(&store, &legacy).unwrap();
    assert!(!report.first_run_auto);
    assert!(!report.imported_config);
    assert_eq!(store.load_config(), existing, "existing config untouched");
}

#[test]
fn first_run_does_nothing_when_legacy_config_missing() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy"); // never created

    let report = import_legacy_first_run(&store, &legacy).unwrap();
    assert!(!report.first_run_auto);
    assert!(!report.imported_config);
    assert!(report.presets.is_empty());
    assert!(
        !store.config_path().exists(),
        "nothing to import, nothing written"
    );
}

#[test]
fn first_run_propagates_legacy_errors() {
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let legacy = tmp.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("config.json"), "{ broken").unwrap();

    let err = import_legacy_first_run(&store, &legacy).unwrap_err();
    assert!(matches!(err, ImportError::Parse(_)), "{err:?}");
    assert!(!store.config_path().exists());
}

#[test]
fn auto_import_legacy_uses_platform_dir_and_noops_when_config_exists() {
    // Production entry point (no explicit legacy dir): with the new config in
    // place it must be a no-op without ever reading the real legacy directory.
    let tmp = tempdir().unwrap();
    let store = new_store(&tmp);
    let mut existing = default_config();
    existing.modes[0].buttons[0].label = "Mine".to_string();
    store.save_config(&existing).unwrap();

    let report = auto_import_legacy(&store).unwrap();
    assert!(!report.first_run_auto);
    assert!(!report.imported_config);
    assert!(report.presets.is_empty());
    assert!(report.warnings.is_empty());
    assert_eq!(store.load_config(), existing);
}

// ---------------------------------------------------------------------------
// Serialization (the Tauri command surface in Task 8 returns these)
// ---------------------------------------------------------------------------

#[test]
fn import_report_serializes_with_flat_snake_case_fields() {
    let report = ImportReport {
        imported_config: true,
        presets: vec!["alpha".to_string()],
        warnings: vec!["w".to_string()],
        first_run_auto: true,
    };
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        json!({
            "imported_config": true,
            "presets": ["alpha"],
            "warnings": ["w"],
            "first_run_auto": true,
        })
    );
}

#[test]
fn import_error_serializes_as_a_structured_tag() {
    assert_eq!(
        serde_json::to_value(ImportError::NotFound).unwrap(),
        json!("NotFound")
    );
    assert_eq!(
        serde_json::to_value(ImportError::Parse("bad".to_string())).unwrap(),
        json!({"Parse": "bad"})
    );
    assert_eq!(
        serde_json::to_value(ImportError::Invalid("nope".to_string())).unwrap(),
        json!({"Invalid": "nope"})
    );
    assert_eq!(
        serde_json::to_value(ImportError::Io("disk".to_string())).unwrap(),
        json!({"Io": "disk"})
    );
}
