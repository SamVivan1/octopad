//! Task 1 (M3.1): the action dispatcher — every row of the behaviour table in
//! `docs/superpowers/plans/2026-10-19-tauri-rewrite-m3.md`, pinned to the
//! reason strings of `linux-app/system_actions.py:374` (`execute_action`).
//!
//! No real process is spawned and no real sleep happens: the command runner,
//! the input injector and the sleeper are all test doubles.

use octopad_core::config::model::Action;
use octopad_core::exec::{
    execute_action, ActionOutcome, CommandCall, CommandRunner, ExecDeps, InputInjector,
    NoInputInjector, RecordingRunner, Sleeper, DEFAULT_DELAY_SECONDS, INPUT_INJECTOR_HELP,
    MAX_DELAY_SECONDS,
};
use serde_json::{json, Value};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds an action from a JSON object (the on-disk shape: `type` + extras).
fn action(value: Value) -> Action {
    serde_json::from_value(value).expect("action shape")
}

/// A `delay` action with the given `seconds` payload value.
fn delay_action(seconds: Value) -> Action {
    action(json!({ "type": "delay", "seconds": seconds }))
}

/// Records the sleeps instead of performing them, so the suite is instant.
#[derive(Default)]
struct FakeSleeper {
    slept: Mutex<Vec<f64>>,
}

impl FakeSleeper {
    fn new() -> FakeSleeper {
        FakeSleeper::default()
    }

    fn slept(&self) -> Vec<f64> {
        self.slept.lock().unwrap().clone()
    }
}

impl Sleeper for FakeSleeper {
    fn sleep(&self, seconds: f64) {
        self.slept.lock().unwrap().push(seconds);
    }
}

/// One recorded injector call.
#[derive(Debug, Clone, PartialEq)]
enum InjectorCall {
    Shortcut(Vec<String>, String),
    Text(String),
    Media(String),
}

/// Stands in for the platform injector and records what it was asked to send.
struct RecordingInjector {
    tool: Option<String>,
    outcome: ActionOutcome,
    calls: Mutex<Vec<InjectorCall>>,
}

impl RecordingInjector {
    fn ok() -> RecordingInjector {
        RecordingInjector::with_tool(Some("wtype".to_string()), ActionOutcome::ok("Sent"))
    }

    fn with_tool(tool: Option<String>, outcome: ActionOutcome) -> RecordingInjector {
        RecordingInjector {
            tool,
            outcome,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<InjectorCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl InputInjector for RecordingInjector {
    fn available(&self) -> Option<String> {
        self.tool.clone()
    }

    fn shortcut(&self, modifiers: &[String], key: &str) -> ActionOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(InjectorCall::Shortcut(modifiers.to_vec(), key.to_string()));
        self.outcome.clone()
    }

    fn text(&self, text: &str) -> ActionOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(InjectorCall::Text(text.to_string()));
        self.outcome.clone()
    }

    fn media(&self, media_key: &str) -> ActionOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(InjectorCall::Media(media_key.to_string()));
        self.outcome.clone()
    }
}

/// A runner that panics, to pin the "any handler error" row of the table.
struct PanicRunner;

impl CommandRunner for PanicRunner {
    fn run(&self, _program: &str, _args: &[&str]) -> ActionOutcome {
        panic!("runner exploded");
    }

    fn spawn_detached(&self, _program: &str, _args: &[&str]) -> ActionOutcome {
        panic!("runner exploded");
    }
}

/// The shell used for `bash_script`, per platform (`sh -c` / `cmd /C`).
fn shell() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("cmd.exe", "/C")
    } else {
        ("/bin/sh", "-c")
    }
}

/// Dispatches with fresh doubles and returns the outcome plus the doubles.
struct Harness {
    outcome: ActionOutcome,
    runner: RecordingRunner,
    injector: RecordingInjector,
    sleeper: FakeSleeper,
}

impl Harness {
    fn new() -> Harness {
        Harness::with_outcome(ActionOutcome::ok("ran"))
    }

    fn with_outcome(outcome: ActionOutcome) -> Harness {
        Harness {
            outcome: ActionOutcome::ok("unused"),
            runner: RecordingRunner::new(outcome),
            injector: RecordingInjector::ok(),
            sleeper: FakeSleeper::new(),
        }
    }

    fn execute(&mut self, action: &Action) -> ActionOutcome {
        let deps = ExecDeps::with_sleeper(&self.runner, &self.injector, &self.sleeper);
        self.outcome = execute_action(action, &deps);
        self.outcome.clone()
    }
}

/// Asserts the exact reason string of a failure.
fn assert_fail(outcome: &ActionOutcome, expected: &str) {
    assert!(!outcome.ok, "expected failure, got {outcome:?}");
    assert_eq!(outcome.reason, expected);
}

/// Asserts the exact reason string of a success.
fn assert_ok(outcome: &ActionOutcome, expected: &str) {
    assert!(outcome.ok, "expected success, got {outcome:?}");
    assert_eq!(outcome.reason, expected);
}

// ---------------------------------------------------------------------------
// none / mode_toggle
// ---------------------------------------------------------------------------

#[test]
fn none_is_a_no_op_success() {
    let mut h = Harness::new();
    assert_ok(
        &h.execute(&action(json!({ "type": "none" }))),
        "Nothing to do.",
    );
    assert!(h.runner.calls().is_empty(), "none must not run anything");
    assert!(h.injector.calls().is_empty(), "none must not inject");
    assert!(h.sleeper.slept().is_empty(), "none must not sleep");
}

#[test]
fn mode_toggle_reports_that_the_firmware_owns_the_layer() {
    let mut h = Harness::new();
    assert_ok(
        &h.execute(&action(json!({ "type": "mode_toggle" }))),
        "Mode layer is controlled by the macropad firmware (LED)",
    );
    assert!(h.runner.calls().is_empty());
    assert!(h.injector.calls().is_empty());
    assert!(h.sleeper.slept().is_empty());
}

// ---------------------------------------------------------------------------
// delay
// ---------------------------------------------------------------------------

#[test]
fn delay_defaults_to_half_a_second_when_the_key_is_missing() {
    let mut h = Harness::new();
    assert_ok(
        &h.execute(&action(json!({ "type": "delay" }))),
        "Waited 0.5s.",
    );
    assert_eq!(h.sleeper.slept(), vec![DEFAULT_DELAY_SECONDS]);
    assert_eq!(DEFAULT_DELAY_SECONDS, 0.5);
}

#[test]
fn delay_accepts_a_number() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!(2.25))), "Waited 2.25s.");
    assert_eq!(h.sleeper.slept(), vec![2.25]);
}

#[test]
fn delay_accepts_a_numeric_string_like_python_float() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!("1.5"))), "Waited 1.5s.");
    assert_eq!(h.sleeper.slept(), vec![1.5]);
}

#[test]
fn delay_accepts_zero() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!(0))), "Waited 0s.");
    assert_eq!(h.sleeper.slept(), vec![0.0]);
}

#[test]
fn delay_accepts_the_maximum() {
    let mut h = Harness::new();
    assert_ok(
        &h.execute(&delay_action(json!(MAX_DELAY_SECONDS))),
        "Waited 600s.",
    );
    assert_eq!(MAX_DELAY_SECONDS, 600.0);
    assert_eq!(h.sleeper.slept(), vec![600.0]);
}

#[test]
fn delay_clamps_a_value_above_the_maximum() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!(9999))), "Waited 600s.");
    assert_eq!(h.sleeper.slept(), vec![600.0]);
}

#[test]
fn delay_clamps_a_negative_value_to_zero() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!(-5))), "Waited 0s.");
    assert_eq!(h.sleeper.slept(), vec![0.0]);
}

#[test]
fn delay_accepts_a_boolean_like_python_float_does() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!(true))), "Waited 1s.");
    assert_eq!(h.sleeper.slept(), vec![1.0]);
}

#[test]
fn delay_formats_seconds_like_python_percent_g() {
    let mut h = Harness::new();
    assert_ok(
        &h.execute(&delay_action(json!(123.456789))),
        "Waited 123.457s.",
    );
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!(0.0001))), "Waited 0.0001s.");
    let mut h = Harness::new();
    assert_ok(
        &h.execute(&delay_action(json!(0.0000123))),
        "Waited 1.23e-05s.",
    );
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!(0.000001))), "Waited 1e-06s.");
}

#[test]
fn delay_treats_nan_and_infinity_like_pythons_max_and_min() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!("nan"))), "Waited 0s.");
    assert_eq!(h.sleeper.slept(), vec![0.0]);
    let mut h = Harness::new();
    assert_ok(&h.execute(&delay_action(json!("inf"))), "Waited 600s.");
    assert_eq!(h.sleeper.slept(), vec![600.0]);
}

#[test]
fn delay_rejects_an_unparsable_duration() {
    for bad in [json!("abc"), json!(""), Value::Null, json!({}), json!([1])] {
        let mut h = Harness::new();
        let a = delay_action(bad.clone());
        assert_fail(&h.execute(&a), "Delay needs a valid duration.");
        assert!(
            h.sleeper.slept().is_empty(),
            "a rejected duration must not sleep ({bad})"
        );
    }
}

// ---------------------------------------------------------------------------
// shortcut / text / media
// ---------------------------------------------------------------------------

#[test]
fn shortcut_delegates_modifiers_and_key_to_the_injector() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "shortcut", "modifiers": ["ctrl", "shift"], "key": "T" }));
    assert_ok(&h.execute(&a), "Sent");
    assert_eq!(
        h.injector.calls(),
        vec![InjectorCall::Shortcut(
            vec!["ctrl".to_string(), "shift".to_string()],
            "T".to_string()
        )]
    );
    assert_eq!(
        RecordingInjector::ok().available(),
        Some("wtype".to_string())
    );
}

#[test]
fn shortcut_without_payload_delegates_with_empty_values() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&action(json!({ "type": "shortcut" }))), "Sent");
    assert_eq!(
        h.injector.calls(),
        vec![InjectorCall::Shortcut(Vec::new(), String::new())]
    );
}

#[test]
fn shortcut_with_a_malformed_payload_is_an_execution_error() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "shortcut", "modifiers": "ctrl" }));
    assert_fail(
        &h.execute(&a),
        "Action execution error: modifiers must be an array, got string",
    );
}

#[test]
fn text_delegates_to_the_injector() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "text", "text": "hello octo" }));
    assert_ok(&h.execute(&a), "Sent");
    assert_eq!(
        h.injector.calls(),
        vec![InjectorCall::Text("hello octo".to_string())]
    );
}

#[test]
fn text_that_is_empty_fails_without_calling_the_injector() {
    for payload in [
        json!({ "type": "text" }),
        json!({ "type": "text", "text": "" }),
    ] {
        let mut h = Harness::new();
        let a = action(payload.clone());
        assert_fail(&h.execute(&a), "Empty text");
        assert!(
            h.injector.calls().is_empty(),
            "empty text must not reach the injector ({payload})"
        );
    }
}

#[test]
fn text_that_is_only_whitespace_is_typed_like_python() {
    // Python: `if not text` — " " is truthy, so the space is typed.
    let mut h = Harness::new();
    let a = action(json!({ "type": "text", "text": " " }));
    assert_ok(&h.execute(&a), "Sent");
    assert_eq!(
        h.injector.calls(),
        vec![InjectorCall::Text(" ".to_string())]
    );
}

#[test]
fn text_with_a_malformed_payload_is_an_execution_error() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "text", "text": 42 }));
    assert_fail(
        &h.execute(&a),
        "Action execution error: text must be a string, got number",
    );
}

#[test]
fn media_delegates_the_camel_case_media_key() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "media", "mediaKey": "PLAY_PAUSE" }));
    assert_ok(&h.execute(&a), "Sent");
    assert_eq!(
        h.injector.calls(),
        vec![InjectorCall::Media("PLAY_PAUSE".to_string())]
    );
}

#[test]
fn media_without_a_payload_delegates_an_empty_key() {
    let mut h = Harness::new();
    assert_ok(&h.execute(&action(json!({ "type": "media" }))), "Sent");
    assert_eq!(h.injector.calls(), vec![InjectorCall::Media(String::new())]);
}

// ---------------------------------------------------------------------------
// The "no input tool" fallback (python `_injector_help()`)
// ---------------------------------------------------------------------------

#[test]
fn missing_injector_fails_with_the_python_help_text() {
    assert_eq!(INPUT_INJECTOR_HELP, INJECTOR_HELP_FROM_PYTHON);
    let runner = RecordingRunner::new(ActionOutcome::ok("ran"));
    let deps = ExecDeps::new(&runner, &NoInputInjector);
    assert!(NoInputInjector.available().is_none());
    for outcome in [
        execute_action(
            &action(json!({ "type": "shortcut", "modifiers": ["ctrl"], "key": "c" })),
            &deps,
        ),
        execute_action(&action(json!({ "type": "text", "text": "hi" })), &deps),
        execute_action(
            &action(json!({ "type": "media", "mediaKey": "MUTE" })),
            &deps,
        ),
    ] {
        assert_fail(&outcome, INJECTOR_HELP_FROM_PYTHON);
    }
}

/// `linux-app/system_actions.py:99` (`_injector_help`), spelled out here so a
/// drift in the Rust constant is caught by the compiler, not by a user.
const INJECTOR_HELP_FROM_PYTHON: &str = concat!(
    "Keyboard simulation requires one of: `wtype` (Wayland, ",
    "sudo apt install wtype), `ydotool` (recommended on GNOME Wayland: ",
    "sudo apt install ydotool then run the daemon `sudo ydotoold` ",
    "or `systemctl --user start ydotool`), or `xdotool` (X11)."
);

// ---------------------------------------------------------------------------
// bash_script
// ---------------------------------------------------------------------------

#[test]
fn bash_script_spawns_the_shell_detached_with_the_script() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "bash_script", "script": "  echo hi\n" }));
    assert_ok(&h.execute(&a), "Bash script dispatched in background");
    let (program, flag) = shell();
    assert_eq!(
        h.runner.calls(),
        vec![CommandCall::detached(program, vec![flag, "echo hi"])]
    );
}

#[test]
fn bash_script_without_content_fails() {
    for payload in [
        json!({ "type": "bash_script" }),
        json!({ "type": "bash_script", "script": "" }),
        json!({ "type": "bash_script", "script": "   \n\t " }),
        json!({ "type": "bash_script", "script": Value::Null }),
    ] {
        let mut h = Harness::new();
        let a = action(payload.clone());
        assert_fail(&h.execute(&a), "Script content is empty");
        assert!(
            h.runner.calls().is_empty(),
            "an empty script must not spawn a shell ({payload})"
        );
    }
}

#[test]
fn bash_script_reports_the_spawn_failure_instead_of_claiming_success() {
    let mut h = Harness::with_outcome(ActionOutcome::fail("/bin/sh not found"));
    let a = action(json!({ "type": "bash_script", "script": "echo hi" }));
    assert_fail(&h.execute(&a), "/bin/sh not found");
    assert_eq!(h.runner.calls().len(), 1);
}

#[test]
fn bash_script_with_a_malformed_payload_is_an_execution_error() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "bash_script", "script": ["echo"] }));
    assert_fail(
        &h.execute(&a),
        "Action execution error: script must be a string, got array",
    );
}

// ---------------------------------------------------------------------------
// system_action (the ten presets arrive with Task 3)
// ---------------------------------------------------------------------------

#[test]
fn unknown_system_action_id_fails() {
    let mut h = Harness::new();
    let a = action(json!({ "type": "system_action", "presetId": "does_not_exist" }));
    assert_fail(&h.execute(&a), "Unknown system action: does_not_exist");
    assert!(h.runner.calls().is_empty());
}

#[test]
fn system_action_without_an_id_fails_with_an_empty_id() {
    let mut h = Harness::new();
    assert_fail(
        &h.execute(&action(json!({ "type": "system_action" }))),
        "Unknown system action: ",
    );
}

// ---------------------------------------------------------------------------
// M3.2 types and the unknown-type fallback
// ---------------------------------------------------------------------------

#[test]
fn m32_types_fail_loudly_instead_of_silently_succeeding() {
    for action_type in ["audio_output", "launch_app", "home_assistant", "ha_webhook"] {
        let mut h = Harness::new();
        let a = action(json!({ "type": action_type }));
        assert_fail(
            &h.execute(&a),
            &format!("Not available until M3.2: {action_type}"),
        );
        assert!(h.runner.calls().is_empty());
        assert!(h.injector.calls().is_empty());
        assert!(h.sleeper.slept().is_empty());
    }
}

#[test]
fn an_unknown_type_fails_with_the_python_message() {
    let mut h = Harness::new();
    assert_fail(
        &h.execute(&action(json!({ "type": "frobnicate" }))),
        "Unknown action type: frobnicate",
    );
    let mut h = Harness::new();
    assert_fail(
        &h.execute(&action(json!({ "type": "" }))),
        "Unknown action type: ",
    );
    let mut h = Harness::new();
    assert_fail(
        &h.execute(&action(json!({ "text": "no type at all" }))),
        "Unknown action type: ",
    );
}

// ---------------------------------------------------------------------------
// Handler errors and Display
// ---------------------------------------------------------------------------

#[test]
fn a_panicking_handler_becomes_a_visible_failure() {
    let injector = RecordingInjector::ok();
    let deps = ExecDeps::new(&PanicRunner, &injector);
    let outcome = execute_action(
        &action(json!({ "type": "bash_script", "script": "echo hi" })),
        &deps,
    );
    assert!(!outcome.ok);
    assert!(
        outcome.reason.starts_with("Action execution error: "),
        "unexpected reason: {}",
        outcome.reason
    );
}

#[test]
fn outcome_display_carries_the_verdict() {
    assert_eq!(
        ActionOutcome::ok("Text typed").to_string(),
        "Text typed [OK]"
    );
    assert_eq!(
        ActionOutcome::fail("Empty text").to_string(),
        "Empty text [FAIL]"
    );
}

// `ha_service` is the legacy alias both Python apps route to the Home Assistant
// branch. Without this arm an old config using the alias would be reported as an
// unknown action type rather than as "not available yet", which reads like a
// corrupt config instead of an unfinished feature.
#[test]
fn legacy_ha_service_alias_is_reported_as_pending_not_unknown() {
    let runner = RecordingRunner::new(ActionOutcome::ok("ran"));
    let deps = ExecDeps::new(&runner, &NoInputInjector);
    let action = Action {
        action_type: "ha_service".to_string(),
        extra: Default::default(),
    };
    let outcome = execute_action(&action, &deps);
    assert!(!outcome.ok, "ha_service must not claim success before M3.2");
    assert_eq!(outcome.reason, "Not available until M3.2: ha_service");
}
