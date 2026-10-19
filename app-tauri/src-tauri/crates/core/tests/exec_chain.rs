//! Task 4 (M3.1): the button action chain runner.
//!
//! Source of truth: `linux-app/serial_listener.py:70` (`execute_button_trigger`),
//! mirrored by `windows-app/serial_listener.py:75`. Python resolves
//! mode → button → actions from the config, skips `none`, runs the chain on a
//! daemon thread (so a `delay` cannot freeze the UI) and prints an
//! `Executing action …` line before each action plus
//! `[ACTION] B<k> (<TRIGGER>): <reason> [OK|FAIL]` after it.
//!
//! Nothing here sleeps for real: the sleeper is a double that records the
//! requested duration (and can be gated), so a 600 s `delay` costs nothing.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use octopad_core::config::{default_config, Action, ConfigStore, TriggerActions};
use octopad_core::exec::{
    spawn_trigger_chain, ActionOutcome, ChainDeps, InputInjector, RecordingRunner, Sleeper,
};
use octopad_core::state::{AppEvent, AppState, Subscription};
use serde_json::json;
use tempfile::{tempdir, TempDir};

// ---------------------------------------------------------------------------
// Configured actions
// ---------------------------------------------------------------------------

fn action(action_type: &str) -> Action {
    Action {
        action_type: action_type.to_string(),
        extra: HashMap::new(),
    }
}

fn text_action(text: &str) -> Action {
    Action {
        action_type: "text".to_string(),
        extra: HashMap::from([("text".to_string(), json!(text))]),
    }
}

fn delay_action(seconds: f64) -> Action {
    Action {
        action_type: "delay".to_string(),
        extra: HashMap::from([("seconds".to_string(), json!(seconds))]),
    }
}

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// A one-shot gate the chain parks inside, so a test can observe the chain
/// while it is mid-action.
#[derive(Clone)]
struct Gate {
    flag: Arc<(Mutex<bool>, Condvar)>,
}

impl Gate {
    fn new() -> Gate {
        Gate {
            flag: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn wait(&self) {
        let (lock, condvar) = &*self.flag;
        let mut open = lock.lock().unwrap();
        while !*open {
            open = condvar.wait(open).unwrap();
        }
    }

    fn open(&self) {
        let (lock, condvar) = &*self.flag;
        *lock.lock().unwrap() = true;
        condvar.notify_all();
    }
}

/// Opens the gate even when a test panics, so a failure can never leave a
/// sleeping blocking task that stalls the runtime shutdown.
struct GateGuard(Gate);

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// Records every `sleep` request; optionally parks on a gate first and/or burns
/// a little wall-clock time to give another chain room to interleave.
#[derive(Clone, Default)]
struct RecordingSleeper {
    sleeps: Arc<Mutex<Vec<f64>>>,
    gate: Option<Gate>,
    step: Duration,
}

impl RecordingSleeper {
    fn instant() -> RecordingSleeper {
        RecordingSleeper::default()
    }

    fn gated(gate: &Gate) -> RecordingSleeper {
        RecordingSleeper {
            gate: Some(gate.clone()),
            ..RecordingSleeper::default()
        }
    }

    fn sleeps(&self) -> Vec<f64> {
        self.sleeps.lock().unwrap().clone()
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, seconds: f64) {
        self.sleeps.lock().unwrap().push(seconds);
        if let Some(gate) = &self.gate {
            gate.wait();
        }
        if !self.step.is_zero() {
            thread::sleep(self.step);
        }
    }
}

/// Records every injected call and replays scripted outcomes, so a chain can be
/// made to fail exactly where the test wants it to.
#[derive(Clone)]
struct RecordingInput {
    state: Arc<Mutex<Recorded>>,
}

#[derive(Default)]
struct Recorded {
    calls: Vec<String>,
    outcomes: Vec<ActionOutcome>,
    /// Burned inside every call, to let a second chain run in between.
    step: Duration,
}

impl Default for RecordingInput {
    fn default() -> RecordingInput {
        RecordingInput::scripted(Vec::new())
    }
}

impl RecordingInput {
    fn scripted(outcomes: Vec<ActionOutcome>) -> RecordingInput {
        RecordingInput::with_step(outcomes, Duration::ZERO)
    }

    /// Every call spends `step` before returning: the chance for a concurrently
    /// running chain to slip in between two of this chain's actions.
    fn yielding(step: Duration) -> RecordingInput {
        RecordingInput::with_step(Vec::new(), step)
    }

    fn with_step(outcomes: Vec<ActionOutcome>, step: Duration) -> RecordingInput {
        RecordingInput {
            state: Arc::new(Mutex::new(Recorded {
                calls: Vec::new(),
                outcomes,
                step,
            })),
        }
    }

    /// Every injected call, in order (`text:hello`, `media:MUTE`, …).
    fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }

    /// Calls belonging to one chain, identified by the text it types.
    fn calls_containing(&self, needle: &str) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| call.contains(needle))
            .collect()
    }

    fn record(&self, call: String) -> ActionOutcome {
        let (outcome, step) = {
            let mut state = self.state.lock().unwrap();
            state.calls.push(call);
            let index = state.calls.len() - 1;
            let scripted = &state.outcomes;
            let outcome = match scripted.get(index.min(scripted.len().saturating_sub(1))) {
                Some(outcome) => outcome.clone(),
                None => ActionOutcome::ok("injected"),
            };
            (outcome, state.step)
        };
        if !step.is_zero() {
            thread::sleep(step);
        }
        outcome
    }
}

impl InputInjector for RecordingInput {
    fn available(&self) -> Option<String> {
        Some("recording".to_string())
    }

    fn shortcut(&self, modifiers: &[String], key: &str) -> ActionOutcome {
        self.record(format!("shortcut:{}+{}", modifiers.join(","), key))
    }

    fn text(&self, text: &str) -> ActionOutcome {
        self.record(format!("text:{text}"))
    }

    fn media(&self, media_key: &str) -> ActionOutcome {
        self.record(format!("media:{media_key}"))
    }
}

// ---------------------------------------------------------------------------
// Fixture: a state whose desktop button 1 `single` slot holds `actions`
// ---------------------------------------------------------------------------

struct Fixture {
    _dir: TempDir,
    _subscription: Subscription,
    state: Arc<AppState>,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    fn with_actions(actions: Vec<Action>) -> Fixture {
        Fixture::with_button_trigger(0, "single", actions)
    }

    fn with_button_actions(button_index: usize, actions: Vec<Action>) -> Fixture {
        Fixture::with_button_trigger(button_index, "single", actions)
    }

    /// Desktop `button_index`'s `trigger` slot holds `actions`.
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

    /// Every console line so far.
    fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    /// The console lines belonging to one chain.
    ///
    /// The two lines of a pair spell the button differently (`Button 1 …` and
    /// `B1 (SINGLE) …`), so both are matched here.
    fn lines_for_button(&self, button: i64) -> Vec<String> {
        let (start, result) = (format!("Button {button} ("), format!("B{button} ("));
        self.lines()
            .into_iter()
            .filter(|line| line.contains(&start) || line.contains(&result))
            .collect()
    }
}

fn deps(state: &Arc<AppState>, input: &RecordingInput, sleeper: &RecordingSleeper) -> ChainDeps {
    ChainDeps::with_doubles(
        state.clone(),
        Arc::new(RecordingRunner::new(ActionOutcome::ok("ran"))),
        Arc::new(input.clone()),
        Arc::new(sleeper.clone()),
    )
}

/// Poll until `condition` holds; fail loudly instead of hanging.
async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the action chain"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Read the state from a plain thread that the runtime does not wait for: a
/// chain holding the state lock across its sleep shows up as a timeout here
/// instead of a process that hangs on shutdown.
fn state_lock_is_free(state: &Arc<AppState>) -> bool {
    let (tx, rx) = mpsc::channel();
    let state = state.clone();
    thread::spawn(move || {
        let modes = state.config().modes.len();
        let _ = tx.send(modes);
    });
    rx.recv_timeout(Duration::from_secs(5)).is_ok()
}

// ---------------------------------------------------------------------------
// The chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn none_actions_are_skipped_entirely() {
    let fixture = Fixture::with_actions(vec![action("none"), text_action("kept"), action("none")]);
    let input = RecordingInput::default();

    spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "single",
    )
    .await
    .unwrap();

    // Only the middle action produced anything — not even an "Executing" line.
    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): injected [OK]".to_string(),
        ]
    );
    assert_eq!(input.calls(), vec!["text:kept"]);
}

#[tokio::test]
async fn a_three_action_chain_runs_in_order_with_one_result_line_per_action() {
    let fixture = Fixture::with_actions(vec![
        text_action("one"),
        text_action("two"),
        text_action("three"),
    ]);
    let input = RecordingInput::default();

    spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "single",
    )
    .await
    .unwrap();

    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): injected [OK]".to_string(),
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): injected [OK]".to_string(),
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): injected [OK]".to_string(),
        ]
    );
    assert_eq!(input.calls(), vec!["text:one", "text:two", "text:three"]);
}

#[tokio::test]
async fn a_failing_action_does_not_stop_the_chain() {
    let fixture = Fixture::with_actions(vec![
        text_action("before"),
        text_action("broken"),
        text_action("after"),
    ]);
    let input = RecordingInput::scripted(vec![
        ActionOutcome::ok("first fine"),
        ActionOutcome::fail("wtype not found"),
        ActionOutcome::ok("recovered"),
    ]);

    spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "single",
    )
    .await
    .unwrap();

    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): first fine [OK]".to_string(),
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): wtype not found [FAIL]".to_string(),
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): recovered [OK]".to_string(),
        ],
        "the failing action is reported and the chain keeps going"
    );
    assert_eq!(
        input.calls(),
        vec!["text:before", "text:broken", "text:after"]
    );
}

#[tokio::test]
async fn the_action_line_upper_cases_the_trigger_but_the_start_line_does_not() {
    let fixture = Fixture::with_actions(vec![text_action("held")]);
    let input = RecordingInput::default();

    spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "hold",
    )
    .await
    .unwrap();

    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 1 (hold) -> Type: text".to_string(),
            "[ACTION] B1 (HOLD): injected [OK]".to_string(),
        ]
    );
}

#[tokio::test]
async fn the_executing_line_precedes_every_result_line() {
    let fixture =
        Fixture::with_button_trigger(0, "double", vec![text_action("a"), text_action("b")]);
    let input = RecordingInput::default();

    spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "double",
    )
    .await
    .unwrap();

    let lines = fixture.lines();
    assert_eq!(lines.len(), 4);
    for pair in lines.chunks(2) {
        assert_eq!(
            pair[0], "Executing action for Button 1 (double) -> Type: text",
            "each action is announced before it runs"
        );
        assert!(
            pair[1].starts_with("[ACTION] B1 (DOUBLE): "),
            "its result line follows immediately: {}",
            pair[1]
        );
        assert!(pair[1].ends_with(" [OK]"), "{}", pair[1]);
    }
}

#[tokio::test]
async fn an_out_of_range_mode_button_or_unknown_trigger_does_nothing() {
    for (mode, button_index, trigger) in [
        ("work", 0_i64, "single"), // no such mode id
        ("desktop", 99, "single"), // no such button
        ("desktop", -1, "single"), // negative index
        ("desktop", 0, "triple"),  // unknown trigger
    ] {
        let fixture = Fixture::with_actions(vec![text_action("never")]);
        let input = RecordingInput::default();

        spawn_trigger_chain(
            deps(&fixture.state, &input, &RecordingSleeper::instant()),
            mode,
            button_index,
            trigger,
        )
        .await
        .unwrap();

        assert!(
            fixture.lines().is_empty(),
            "{mode}/{button_index}/{trigger} logged {:?}",
            fixture.lines()
        );
        assert!(
            input.calls().is_empty(),
            "{mode}/{button_index}/{trigger} ran"
        );
    }
}

#[tokio::test]
async fn the_chain_runs_off_the_calling_thread_without_holding_the_state_lock() {
    let fixture = Fixture::with_actions(vec![delay_action(600.0)]);
    let input = RecordingInput::default();
    let gate = Gate::new();
    let _guard = GateGuard(gate.clone());
    let sleeper = RecordingSleeper::gated(&gate);

    let handle = spawn_trigger_chain(
        deps(&fixture.state, &input, &sleeper),
        "desktop",
        0,
        "single",
    );

    // The chain is parked inside its 600 s delay while the caller already has
    // its handle back — nothing about the press blocks the UI thread.
    wait_until(|| !sleeper.sleeps().is_empty()).await;
    assert!(!handle.is_finished());
    assert_eq!(
        sleeper.sleeps(),
        vec![600.0],
        "the delay is clamped, not slept"
    );
    assert_eq!(
        fixture.lines(),
        vec!["Executing action for Button 1 (single) -> Type: delay".to_string()],
        "only the start line so far"
    );
    assert!(
        state_lock_is_free(&fixture.state),
        "the state lock must not be held across a sleep"
    );

    gate.open();
    handle.await.unwrap();

    assert_eq!(
        fixture.lines(),
        vec![
            "Executing action for Button 1 (single) -> Type: delay".to_string(),
            "[ACTION] B1 (SINGLE): Waited 600s. [OK]".to_string(),
        ]
    );
    assert!(input.calls().is_empty(), "a delay injects nothing");
}

#[tokio::test]
async fn the_chain_re_reads_the_working_config_for_every_press() {
    let fixture = Fixture::with_actions(vec![text_action("first")]);
    let input = RecordingInput::default();

    spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "single",
    )
    .await
    .unwrap();

    // Re-configure while the app runs: the next press must pick the edit up
    // instead of replaying a cached copy of the config.
    let mut config = fixture.state.config();
    config.modes[0].buttons[0].single = TriggerActions::from_list(vec![text_action("second")]);
    fixture.state.set_config(config, "test");

    spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "single",
    )
    .await
    .unwrap();

    assert_eq!(input.calls(), vec!["text:first", "text:second"]);
}

#[tokio::test]
async fn two_concurrent_chains_do_not_interleave_inside_one_chain() {
    let fixture = Fixture::with_button_actions(
        0,
        vec![
            text_action("b0-a1"),
            text_action("b0-a2"),
            text_action("b0-a3"),
        ],
    );
    let mut config = fixture.state.config();
    config.modes[0].buttons[1].single = TriggerActions::from_list(vec![
        text_action("b1-a1"),
        text_action("b1-a2"),
        text_action("b1-a3"),
    ]);
    fixture.state.set_config(config, "test");

    // Shared input double: both chains inject into the same recording, so the
    // per-chain subsequences are what prove neither chain was cut in half.
    // Every call spends a few milliseconds inside the injector, which is the
    // window in which the other chain runs.
    let input = RecordingInput::yielding(Duration::from_millis(5));
    let first = spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        0,
        "single",
    );
    let second = spawn_trigger_chain(
        deps(&fixture.state, &input, &RecordingSleeper::instant()),
        "desktop",
        1,
        "single",
    );
    let (first, second) = tokio::join!(first, second);
    first.unwrap();
    second.unwrap();

    assert_eq!(
        input.calls_containing("b0-"),
        vec![
            "text:b0-a1".to_string(),
            "text:b0-a2".to_string(),
            "text:b0-a3".to_string(),
        ]
    );
    assert_eq!(
        input.calls_containing("b1-"),
        vec![
            "text:b1-a1".to_string(),
            "text:b1-a2".to_string(),
            "text:b1-a3".to_string(),
        ]
    );

    assert_eq!(
        fixture.lines_for_button(1),
        vec![
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): injected [OK]".to_string(),
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): injected [OK]".to_string(),
            "Executing action for Button 1 (single) -> Type: text".to_string(),
            "[ACTION] B1 (SINGLE): injected [OK]".to_string(),
        ]
    );
    assert_eq!(
        fixture.lines_for_button(2),
        vec![
            "Executing action for Button 2 (single) -> Type: text".to_string(),
            "[ACTION] B2 (SINGLE): injected [OK]".to_string(),
            "Executing action for Button 2 (single) -> Type: text".to_string(),
            "[ACTION] B2 (SINGLE): injected [OK]".to_string(),
            "Executing action for Button 2 (single) -> Type: text".to_string(),
            "[ACTION] B2 (SINGLE): injected [OK]".to_string(),
        ]
    );
}
