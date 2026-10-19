//! The button action chain: press → mode → button → actions.
//!
//! Port of `linux-app/serial_listener.py:70` (`execute_button_trigger`),
//! mirrored by `windows-app/serial_listener.py:75`. Python resolves the
//! actions, skips `none`, starts a daemon thread and runs the sequence there,
//! so a `delay` of up to 600 s cannot freeze the GTK main loop; the tokio
//! blocking task used here is the same idea for the Tauri build.
//!
//! Parity rules kept from the Python source:
//!
//! * `none` (and a slot with no `type`) is skipped before dispatch — it never
//!   reaches [`execute_action`] and never logs a line.
//! * A failing action is logged as `[FAIL]` and the chain **continues**
//!   (resolved parity decision 1).
//! * An unknown mode, an out-of-range button or an unknown trigger is a silent
//!   no-op: no work, no log line.
//! * The log pair per action is `Executing action for Button {n} ({trigger}) ->
//!   Type: {type}` followed by `[ACTION] B{n} ({TRIGGER}): {reason} [OK|FAIL]`,
//!   with the trigger upper-cased only in the second one.
//!
//! Two invariants the tests pin, because both are easy to regress:
//!
//! * the working config is re-read **at run time** (a press that lands after an
//!   edit executes the edit), and
//! * the state lock is taken only long enough to copy the actions out of the
//!   config, never across a `delay`.

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::config::Action;
use crate::state::{actions_of, AppState};

use super::{execute_action, CommandRunner, ExecDeps, InputInjector, RealCommandRunner, Sleeper};

/// A `'static` command runner, so the blocking task can own it.
pub type SharedCommandRunner = Arc<dyn CommandRunner + Send + Sync>;
/// A `'static` input injector. The platform injector borrows its runner, which
/// is why it is built *inside* the task instead (see [`ChainDeps::input`]).
pub type SharedInputInjector = Arc<dyn InputInjector + Send + Sync>;
/// A `'static` sleeper — real, or a test double that returns immediately.
pub type SharedSleeper = Arc<dyn Sleeper + Send + Sync>;

/// Everything a chain needs from the outside world.
///
/// Cheap to clone: a button press clones three `Arc`s, and nothing is probed or
/// spawned until the task actually finds work to do.
#[derive(Clone)]
pub struct ChainDeps {
    pub state: Arc<AppState>,
    pub runner: SharedCommandRunner,
    /// `None` → detect the platform injector inside the chain task.
    ///
    /// `platform_input_injector` hands back a `LinuxInjector` that *borrows*
    /// the runner it probed with, so it cannot be stored as a `'static` handle;
    /// the chain builds it per run instead. Detecting is a `which` probe, so it
    /// only happens for chains that really have actions to run.
    pub input: Option<SharedInputInjector>,
    pub sleeper: SharedSleeper,
}

impl ChainDeps {
    /// Production wiring: real commands, the platform input injector, a real
    /// (blocking) sleep.
    pub fn production(state: Arc<AppState>) -> ChainDeps {
        ChainDeps {
            state,
            runner: Arc::new(RealCommandRunner::new()),
            input: None,
            sleeper: Arc::new(super::ThreadSleeper),
        }
    }

    /// Wiring with explicit doubles (tests, or a cancellable sleeper later).
    pub fn with_doubles(
        state: Arc<AppState>,
        runner: SharedCommandRunner,
        input: SharedInputInjector,
        sleeper: SharedSleeper,
    ) -> ChainDeps {
        ChainDeps {
            state,
            runner,
            input: Some(input),
            sleeper,
        }
    }
}

/// `if action.get("type") in ("none", None): continue`.
///
/// A missing `type` deserializes to the empty string, so both arms of the
/// Python check are covered.
fn is_skipped(action: &Action) -> bool {
    action.action_type.is_empty() || action.action_type == "none"
}

/// mode → button → actions, read from the working config **now**.
///
/// `None` when the mode or the button does not exist; an empty list when the
/// trigger is unknown or the slot holds nothing. Both are no-ops.
pub fn resolve_actions(
    state: &AppState,
    mode: &str,
    button_index: i64,
    trigger: &str,
) -> Option<Vec<Action>> {
    // One short critical section: everything below runs on the owned copy, so
    // no lock is ever held while an action executes or sleeps.
    let config = state.config();
    // Python maps every non-`desktop` mode onto `ha`
    // (`serial_listener.py:81`), which would run the Home Assistant layer for a
    // mode that does not exist; the plan requires an unknown mode to be a
    // no-op, so the id is matched directly. For the two modes the firmware can
    // report the behaviour is identical.
    let mode_config = config
        .modes
        .iter()
        .find(|candidate| candidate.id.eq_ignore_ascii_case(mode))?;
    // `usize::try_from` rejects the negative indices Python would silently
    // wrap around to a button from the end of the list.
    let button = usize::try_from(button_index)
        .ok()
        .and_then(|index| mode_config.buttons.get(index))?;
    Some(actions_of(button, trigger))
}

/// Spawn the chain for `(mode, button_index, trigger)` on a blocking task.
///
/// Returns immediately: the caller (the device-event bridge) only ever logs the
/// press and moves on. Must be called from within a Tokio runtime, like
/// `tokio::spawn`.
///
/// Known limitation, tracked with M3.2's cancellable sleeper: a chain that is
/// mid-`delay` when the app quits keeps its blocking task (and therefore the
/// runtime shutdown) alive until the delay expires.
pub fn spawn_trigger_chain(
    deps: ChainDeps,
    mode: &str,
    button_index: i64,
    trigger: &str,
) -> JoinHandle<()> {
    let ChainDeps {
        state,
        runner,
        input,
        sleeper,
    } = deps;
    // The task outlives this call, so it cannot borrow the caller's strings.
    let mode = mode.to_string();
    let trigger = trigger.to_string();
    tokio::task::spawn_blocking(move || {
        run_trigger_chain(state, runner, input, sleeper, &mode, button_index, &trigger)
    })
}

/// Resolve and run, entirely off the caller's thread.
fn run_trigger_chain(
    state: Arc<AppState>,
    runner: SharedCommandRunner,
    input: Option<SharedInputInjector>,
    sleeper: SharedSleeper,
    mode: &str,
    button_index: i64,
    trigger: &str,
) {
    let Some(actions) = resolve_actions(&state, mode, button_index, trigger) else {
        return;
    };
    // Nothing to do: no log line and, on the production path, no `which` probe.
    if actions.iter().all(is_skipped) {
        return;
    }
    let runner: &dyn CommandRunner = &*runner;
    let sleeper: &dyn Sleeper = &*sleeper;
    match input {
        Some(input) => execute_chain(
            &state,
            &actions,
            runner,
            &*input,
            sleeper,
            button_index,
            trigger,
        ),
        None => execute_chain_with_platform_input(
            &state,
            &actions,
            runner,
            sleeper,
            button_index,
            trigger,
        ),
    }
}

/// The chain body: one log pair per action, in order, never aborted by a
/// failure.
fn execute_chain(
    state: &AppState,
    actions: &[Action],
    runner: &dyn CommandRunner,
    input: &dyn InputInjector,
    sleeper: &dyn Sleeper,
    button_index: i64,
    trigger: &str,
) {
    let deps = ExecDeps::with_sleeper(runner, input, sleeper);
    let number = button_index.saturating_add(1);
    for action in actions {
        if is_skipped(action) {
            continue;
        }
        state.log(format!(
            "Executing action for Button {number} ({trigger}) -> Type: {}",
            action.action_type
        ));
        let outcome = execute_action(action, &deps);
        // Log honesty: the line reports the real verdict, including a failure
        // in the middle of an otherwise successful chain.
        state.log(format!(
            "[ACTION] B{number} ({}): {} [{}]",
            trigger.to_uppercase(),
            outcome.reason,
            outcome.verdict()
        ));
    }
}

/// Detect the platform injector (wtype → ydotool → xdotool) for this run.
///
/// A machine with no input tool gets [`NoInputInjector`], so a `shortcut`
/// fails with the Python help text instead of pretending the keys were sent.
#[cfg(target_os = "linux")]
fn execute_chain_with_platform_input(
    state: &AppState,
    actions: &[Action],
    runner: &dyn CommandRunner,
    sleeper: &dyn Sleeper,
    button_index: i64,
    trigger: &str,
) {
    match super::input_linux::LinuxInjector::detect(runner) {
        Some(injector) => execute_chain(
            state,
            actions,
            runner,
            &injector,
            sleeper,
            button_index,
            trigger,
        ),
        None => execute_chain(
            state,
            actions,
            runner,
            &super::NoInputInjector,
            sleeper,
            button_index,
            trigger,
        ),
    }
}

/// Every non-Linux target has no injector yet (Windows lands in Task 5), so the
/// chain still runs and reports the missing capability honestly.
#[cfg(not(target_os = "linux"))]
fn execute_chain_with_platform_input(
    state: &AppState,
    actions: &[Action],
    runner: &dyn CommandRunner,
    sleeper: &dyn Sleeper,
    button_index: i64,
    trigger: &str,
) {
    execute_chain(
        state,
        actions,
        runner,
        &super::NoInputInjector,
        sleeper,
        button_index,
        trigger,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_and_type_less_actions_are_skipped() {
        assert!(is_skipped(&Action {
            action_type: "none".to_string(),
            extra: Default::default(),
        }));
        assert!(is_skipped(&Action {
            action_type: String::new(),
            extra: Default::default(),
        }));
        assert!(!is_skipped(&Action {
            action_type: "text".to_string(),
            extra: Default::default(),
        }));
    }
}
