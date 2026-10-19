//! Task 2 (M3.2): the core test runner — the editor's Test buttons.
//!
//! The UI needs the real execution machinery without a device event: the same
//! chain a pad press runs (`chain.rs::spawn_trigger_chain`), but fired from the
//! editor. `spawn_test_action` runs one configured action and hands back only
//! its outcome (the UI shows it as a toast; nothing is logged, because there
//! is no button a console line could name). `spawn_test_trigger` resolves
//! `mode → button → trigger` from the working config exactly like a press,
//! skips `none`, and writes the same console pair per action with the trigger
//! label `test` while returning one outcome per action.
//!
//! No hardware is involved anywhere: the fixture is a plain `AppState` over a
//! temporary config store — no device, no serial or BLE double — and the
//! actions are ones whose verdict cannot depend on the host machine
//! (`mode_toggle` always succeeds without touching it, an empty `text` always
//! fails before any injector is consulted, a zero-length delay never sleeps).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use octopad_core::config::{default_config, Action, ConfigStore, TriggerActions};
use octopad_core::exec::{spawn_test_action, spawn_test_trigger, ActionOutcome};
use octopad_core::state::{AppEvent, AppState, Subscription};
use serde_json::json;
use tempfile::{tempdir, TempDir};

// ---------------------------------------------------------------------------
// Actions whose outcomes cannot depend on the host machine
// ---------------------------------------------------------------------------

/// `mode_toggle` never calls the platform input injector and never runs a
/// process; the dispatcher answers with the fixed firmware reason.
fn mode_toggle() -> Action {
    Action {
        action_type: "mode_toggle".to_string(),
        extra: HashMap::new(),
    }
}

/// A zero-length delay: the production sleeper does not block on it and the
/// reason is the exact Python text (`f"Waited {seconds:g}s."`).
fn instant_delay() -> Action {
    Action {
        action_type: "delay".to_string(),
        extra: HashMap::from([("seconds".to_string(), json!(0.0))]),
    }
}

/// Empty text fails with "Empty text" before any injector exists — for a
/// deterministic `[FAIL]` in the middle of a chain.
fn empty_text() -> Action {
    Action {
        action_type: "text".to_string(),
        extra: HashMap::from([("text".to_string(), json!(""))]),
    }
}

/// The skipped action of the chains (`chain.rs::is_skipped`).
fn none() -> Action {
    Action {
        action_type: "none".to_string(),
        extra: HashMap::new(),
    }
}

const LED_REASON: &str = "Mode layer is controlled by the macropad firmware (LED)";

// ---------------------------------------------------------------------------
// Fixture: a plain state, observed through the console subscription
// ---------------------------------------------------------------------------

/// A plain `AppState` over a temporary store — deliberately **no** device
/// manager, no serial or BLE double: a test run must get by with the state
/// alone. The console is observed the same way the real console pane sees it,
/// through a `SerialLog` subscription.
struct Fixture {
    _dir: TempDir,
    _subscription: Subscription,
    state: Arc<AppState>,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    /// Desktop (the default current mode) button `button_index` holds
    /// `actions` in its `trigger` slot.
    fn with_button_trigger(button_index: usize, trigger: &str, actions: Vec<Action>) -> Fixture {
        let dir = tempdir().unwrap();
        let state = Arc::new(AppState::new(ConfigStore::at(dir.path().join("OctoPad"))));
        let mut config = default_config();
        let slot = TriggerActions::from_list(actions);
        match trigger {
            "double" => config.modes[0].buttons[button_index].double = slot,
            "hold" => config.modes[0].buttons[button_index].hold = slot,
            _ => config.modes[0].buttons[button_index].single = slot,
        }
        state.set_config(config, "test");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        let subscription = state.subscribe(move |event| {
            if let AppEvent::SerialLog { line } = event {
                sink.lock().unwrap().push(line);
            }
        });
        Fixture {
            _dir: dir,
            _subscription: subscription,
            state,
            lines,
        }
    }

    fn with_actions(actions: Vec<Action>) -> Fixture {
        Fixture::with_button_trigger(0, "single", actions)
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// Trigger tests
// ---------------------------------------------------------------------------

/// `none` is skipped before dispatch — it produces no console line at all, the
/// same silence a real press keeps for it.
#[tokio::test]
async fn a_skipped_none_produces_no_test_line() {
    let fixture = Fixture::with_actions(vec![none(), mode_toggle(), none()]);

    let outcomes = spawn_test_trigger(fixture.state.clone(), 0, "single")
        .await
        .unwrap();

    assert_eq!(outcomes, vec![ActionOutcome::ok(LED_REASON)]);
    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 1 (test) -> Type: mode_toggle".to_string(),
            format!("[ACTION] B1 (TEST): {LED_REASON} [OK]"),
        ],
        "the `none` actions never reach the console"
    );
}

/// One outcome per action, in config order — the shape Task 3 joins into its
/// `Test: …` toast.
#[tokio::test]
async fn a_two_action_test_returns_both_outcomes_in_order() {
    let fixture = Fixture::with_actions(vec![mode_toggle(), instant_delay()]);

    let outcomes = spawn_test_trigger(fixture.state.clone(), 0, "single")
        .await
        .unwrap();

    assert_eq!(
        outcomes,
        vec![
            ActionOutcome::ok(LED_REASON),
            ActionOutcome::ok("Waited 0s."),
        ]
    );
    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 1 (test) -> Type: mode_toggle".to_string(),
            format!("[ACTION] B1 (TEST): {LED_REASON} [OK]"),
            "Executing action for Button 1 (test) -> Type: delay".to_string(),
            "[ACTION] B1 (TEST): Waited 0s. [OK]".to_string(),
        ]
    );
}

/// Parity with a real press: a failure mid-chain is reported and the rest of
/// the chain still runs.
#[tokio::test]
async fn a_middle_failure_still_yields_the_last_ok() {
    let fixture = Fixture::with_actions(vec![mode_toggle(), empty_text(), mode_toggle()]);

    let outcomes = spawn_test_trigger(fixture.state.clone(), 0, "single")
        .await
        .unwrap();

    assert_eq!(
        outcomes,
        vec![
            ActionOutcome::ok(LED_REASON),
            ActionOutcome::fail("Empty text"),
            ActionOutcome::ok(LED_REASON),
        ]
    );
    assert!(
        fixture.lines().iter().any(|line| line.ends_with("[FAIL]")),
        "the failure shows in the console: {:?}",
        fixture.lines()
    );
    assert!(
        fixture
            .lines()
            .last()
            .is_some_and(|line| line.ends_with("[OK]")),
        "the last action still ran: {:?}",
        fixture.lines()
    );
}

/// The trigger label is `test`: lower-case in the `Executing …` preamble,
/// upper-cased in the `[ACTION]` line — and the button number is the configured
/// slot, not the pressed button.
#[tokio::test]
async fn console_lines_carry_the_test_label_for_the_right_button() {
    let fixture = Fixture::with_button_trigger(1, "single", vec![mode_toggle()]);

    let outcomes = spawn_test_trigger(fixture.state.clone(), 1, "single")
        .await
        .unwrap();

    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 2 (test) -> Type: mode_toggle".to_string(),
            format!("[ACTION] B2 (TEST): {LED_REASON} [OK]"),
        ]
    );
}

/// The console pair, outcome list and no-op rules all follow the real chain.
#[tokio::test]
async fn an_unknown_trigger_or_out_of_range_button_test_is_a_no_op() {
    let fixture = Fixture::with_actions(vec![mode_toggle()]);

    let trigger = spawn_test_trigger(fixture.state.clone(), 0, "triple")
        .await
        .unwrap();
    let out_of_range = spawn_test_trigger(fixture.state.clone(), 99, "single")
        .await
        .unwrap();

    // Note: the default config's `hold` slot ships with default actions, so a
    // genuinely unknown trigger name is the honest no-op probe here.
    assert_eq!(trigger, Vec::new());
    assert_eq!(out_of_range, Vec::new());
    assert!(
        fixture.lines().is_empty(),
        "neither no-op logs anything: {:?}",
        fixture.lines()
    );
}

/// A test run gets by with the state alone: no device manager, no serial or
/// BLE transport, before or after.
#[tokio::test]
async fn a_test_run_needs_no_device_connection() {
    let fixture = Fixture::with_actions(vec![mode_toggle(), instant_delay()]);
    assert!(!fixture.state.serial_connected());
    assert!(!fixture.state.ble_connected());

    let outcomes = spawn_test_trigger(fixture.state.clone(), 0, "single")
        .await
        .unwrap();

    assert_eq!(
        outcomes,
        vec![
            ActionOutcome::ok(LED_REASON),
            ActionOutcome::ok("Waited 0s."),
        ]
    );
    assert!(!fixture.state.serial_connected());
    assert!(!fixture.state.ble_connected());
}

// ---------------------------------------------------------------------------
// Single-action tests
// ---------------------------------------------------------------------------

/// A single action returns its outcome only: no console preamble, no result
/// line — the UI shows the verdict itself.
#[tokio::test]
async fn a_tested_action_returns_its_outcome_without_a_console_line() {
    let fixture = Fixture::with_actions(Vec::new());

    let ok = spawn_test_action(fixture.state.clone(), mode_toggle())
        .await
        .unwrap();
    let fail = spawn_test_action(fixture.state.clone(), empty_text())
        .await
        .unwrap();

    assert_eq!(ok, ActionOutcome::ok(LED_REASON));
    assert_eq!(fail, ActionOutcome::fail("Empty text"));
    assert!(
        fixture.lines().is_empty(),
        "the single-action test never logs: {:?}",
        fixture.lines()
    );
}

/// Slot tests skip `none`; a *lone* `none` handed to the per-action Test
/// button reaches the dispatcher, which answers with its own honest verdict.
#[tokio::test]
async fn a_tested_lone_none_action_reports_nothing_to_do() {
    let fixture = Fixture::with_actions(Vec::new());

    let outcome = spawn_test_action(fixture.state.clone(), none())
        .await
        .unwrap();

    assert_eq!(outcome, ActionOutcome::ok("Nothing to do."));
    assert!(fixture.lines().is_empty());
}
