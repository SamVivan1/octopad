//! The core test runner: run one action or a whole trigger on demand.
//!
//! The editor's Test buttons (M3.2) drive the same execution machinery a pad
//! press drives, but without a device event. Like a real press, the work runs
//! on a tokio blocking task — a `delay` of up to 600 s cannot freeze the
//! UI — and both functions return immediately, handing back the handle to
//! await.
//!
//! * [`spawn_test_action`] runs a single configured action and returns only
//!   its [`ActionOutcome`]: the UI shows the verdict next to the card, and
//!   nothing is written to the console (there is no button a console line
//!   could name).
//! * [`spawn_test_trigger`] resolves `mode → button → trigger` from the
//!   working config exactly like [`super::spawn_trigger_chain`] — the editor's
//!   **current** mode standing in for the firmware-reported one — skips
//!   `none`, and emits the identical console pair per action with the trigger
//!   label `test`, while returning one outcome per action. A failure never
//!   aborts the rest.
//!
//! Both run on production wiring ([`ChainDeps::production`]): real commands, a
//! real sleep, the platform input injector. The verdict is honest — whatever
//! the host would do for a real press happens here.

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::config::Action;
use crate::state::AppState;

use super::chain::run_trigger_chain;
use super::{
    platform_input_injector, ActionOutcome, ChainDeps, CommandRunner, ExecDeps, InputInjector,
    Sleeper,
};

/// Run one action for the editor's per-card Test button, off the UI thread.
///
/// The action is used exactly as configured — `none` included, which the
/// dispatcher answers with its own "Nothing to do." (slot resolution, which
/// skips `none`, is the trigger test's job).
pub fn spawn_test_action(state: Arc<AppState>, action: Action) -> JoinHandle<ActionOutcome> {
    tokio::task::spawn_blocking(move || {
        let deps = ChainDeps::production(state);
        run_single_action(&deps, &action)
    })
}

/// Run a whole trigger slot for the editor's "test all" button, off the UI
/// thread.
///
/// Unknown trigger, out-of-range button or an empty slot: an empty outcome
/// list and no console line — the same silent no-op as a real press.
pub fn spawn_test_trigger(
    state: Arc<AppState>,
    button_index: usize,
    trigger: &str,
) -> JoinHandle<Vec<ActionOutcome>> {
    // The task outlives this call, so it cannot borrow the caller's string.
    let trigger = trigger.to_string();
    tokio::task::spawn_blocking(move || run_test_chain(state, button_index, &trigger))
}

/// The test run's body: resolve the editor's current mode and the requested
/// slot from the working config, then run the chain with the `test` console
/// label.
fn run_test_chain(state: Arc<AppState>, button_index: usize, trigger: &str) -> Vec<ActionOutcome> {
    // Read inside the task, like the actions are: a mode switch racing the
    // spawn cannot pair one mode's id with another mode's actions.
    let mode = state.current_mode().id;
    let index = i64::try_from(button_index).unwrap_or(-1);
    let deps = ChainDeps::production(state);
    run_trigger_chain(deps, &mode, index, trigger, "test")
}

/// Run exactly one action against `deps`' wiring.
///
/// The single-action counterpart of `run_trigger_chain`: no resolution, no
/// console output — just the honest verdict. When the deps carry no injected
/// injector, the platform injector is detected for this one run (the same
/// `which` probe a real press performs).
fn run_single_action(deps: &ChainDeps, action: &Action) -> ActionOutcome {
    let runner: &dyn CommandRunner = &*deps.runner;
    let sleeper: &dyn Sleeper = &*deps.sleeper;
    match &deps.input {
        Some(input) => execute_with(runner, &**input, sleeper, action),
        None => {
            let injector = platform_input_injector(runner);
            execute_with(runner, &*injector, sleeper, action)
        }
    }
}

fn execute_with(
    runner: &dyn CommandRunner,
    input: &dyn InputInjector,
    sleeper: &dyn Sleeper,
    action: &Action,
) -> ActionOutcome {
    super::execute_action(action, &ExecDeps::with_sleeper(runner, input, sleeper))
}
