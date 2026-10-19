use crate::actions::{is_runnable_action, ActionExecutor};
use octopad_core::config::Action;
use octopad_core::device::DeviceEvent;
use octopad_core::state::AppState;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Route transport button events to configured actions on one blocking worker.
pub fn spawn_action_runtime(
    state: Arc<AppState>,
    rx: mpsc::Receiver<DeviceEvent>,
    executor: Arc<dyn ActionExecutor>,
) -> JoinHandle<()> {
    let (action_tx, action_rx) = std::sync::mpsc::channel();
    let worker_state = state.clone();
    let worker = tokio::task::spawn_blocking(move || {
        while let Ok((mode, index, trigger)) = action_rx.recv() {
            run_button_actions(&worker_state, executor.as_ref(), &mode, index, &trigger);
        }
    });
    let bridge_task = octopad_core::bridge::connect_device_events_with_handler(
        state,
        rx,
        move |mode, index, trigger| {
            let _ = action_tx.send((mode, index, trigger));
        },
    );

    tokio::spawn(async move {
        let _ = bridge_task.await;
        let _ = worker.await;
    })
}

fn run_button_actions(
    state: &AppState,
    executor: &dyn ActionExecutor,
    mode_id: &str,
    index: i64,
    trigger: &str,
) {
    let config = state.config();
    let Some(mode) = config.modes.iter().find(|mode| mode.id == mode_id) else {
        state.log(format!(
            "[ACTION] Ignoring button event for unknown mode: {mode_id}"
        ));
        return;
    };
    let Some(button) = mode.buttons.iter().find(|button| button.index == index) else {
        state.log(format!(
            "[ACTION] Ignoring button event with unknown index {index} for mode {mode_id}"
        ));
        return;
    };
    let actions = match trigger {
        "single" => button.single.as_slice(),
        "double" => button.double.as_slice(),
        "hold" => button.hold.as_slice(),
        _ => {
            state.log(format!(
                "[ACTION] Ignoring button event with unknown trigger: {trigger}"
            ));
            return;
        }
    };
    execute_and_log(state, executor, actions, index, trigger);
}

pub(crate) fn execute_and_log(
    state: &AppState,
    executor: &dyn ActionExecutor,
    actions: &[Action],
    index: i64,
    trigger: &str,
) {
    for action in actions.iter().filter(|action| is_runnable_action(action)) {
        let result = executor.execute(action);
        state.log(format!(
            "[ACTION] B{} ({}) {} [{}]",
            index.saturating_add(1),
            trigger.to_uppercase(),
            result.message,
            if result.ok { "OK" } else { "FAIL" }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::spawn_action_runtime;
    use octopad_core::config::{default_config, Action, Config, ConfigStore, TriggerActions};
    use octopad_core::device::DeviceEvent;
    use octopad_core::state::AppState;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestConfigDir(PathBuf);

    impl TestConfigDir {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
                ".action-runtime-test-{}-{}",
                std::process::id(),
                NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestConfigDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Clone, Default)]
    struct RecordingExecutor(Arc<Mutex<Vec<Action>>>);

    impl super::super::actions::ActionExecutor for RecordingExecutor {
        fn execute(&self, action: &Action) -> super::super::actions::ActionResult {
            self.0.lock().unwrap().push(action.clone());
            super::super::actions::ActionResult {
                ok: true,
                message: action.action_type.clone(),
            }
        }
    }

    fn action(action_type: &str, key: &str, value: &str) -> Action {
        Action {
            action_type: action_type.to_string(),
            extra: HashMap::from([(key.to_string(), serde_json::json!(value))]),
        }
    }

    fn state_with_config(config: Config, dir: &TestConfigDir) -> Arc<AppState> {
        let store = ConfigStore::at(dir.0.join("OctoPad"));
        store.save_config(&config).unwrap();
        Arc::new(AppState::new(store))
    }

    #[tokio::test]
    async fn button_event_runs_actions_for_reported_mode_button_and_trigger_in_order() {
        let dir = TestConfigDir::new();
        let mut config = default_config();
        config.modes[1].buttons[2].single = TriggerActions::Many(vec![
            action("text", "text", "first"),
            action("media", "mediaKey", "NEXT_TRACK"),
        ]);
        let state = state_with_config(config, &dir);
        let executor = RecordingExecutor::default();
        let seen = executor.0.clone();
        let (tx, rx) = mpsc::channel(8);
        let task = spawn_action_runtime(state.clone(), rx, Arc::new(executor));

        tx.send(DeviceEvent::ButtonEvent {
            mode: "ha".to_string(),
            index: 2,
            trigger: "single".to_string(),
        })
        .await
        .unwrap();
        drop(tx);
        task.await.unwrap();

        let actions = seen.lock().unwrap();
        assert_eq!(
            actions
                .iter()
                .map(|action| action.extra.values().next().unwrap().as_str().unwrap())
                .collect::<Vec<_>>(),
            ["first", "NEXT_TRACK"]
        );
        assert_eq!(
            state.mode_index(),
            0,
            "event routing must not change UI mode"
        );
        assert_eq!(
            state.selected_index(),
            0,
            "event routing must not change UI button"
        );
    }

    #[tokio::test]
    async fn unknown_mode_or_button_and_invalid_action_do_not_panic_or_run_wrong_actions() {
        let dir = TestConfigDir::new();
        let mut config = default_config();
        config.modes[1].buttons[2].single = TriggerActions::Many(vec![
            action("", "bad", "ignored"),
            action("text", "text", "valid"),
        ]);
        let state = state_with_config(config, &dir);
        let executor = RecordingExecutor::default();
        let seen = executor.0.clone();
        let (tx, rx) = mpsc::channel(8);
        let task = spawn_action_runtime(state, rx, Arc::new(executor));

        for (mode, index) in [("missing", 2), ("ha", 99), ("ha", 2)] {
            tx.send(DeviceEvent::ButtonEvent {
                mode: mode.to_string(),
                index,
                trigger: "single".to_string(),
            })
            .await
            .unwrap();
        }
        drop(tx);
        task.await.unwrap();

        let actions = seen.lock().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].extra["text"], "valid");
    }

    #[tokio::test]
    async fn action_runtime_starts_without_hardware_and_keeps_device_disconnected() {
        let dir = TestConfigDir::new();
        let state = state_with_config(default_config(), &dir);
        let (tx, rx) = mpsc::channel(8);
        let task = spawn_action_runtime(state.clone(), rx, Arc::new(RecordingExecutor::default()));

        drop(tx);
        task.await.unwrap();

        assert!(!state.serial_connected());
        assert!(!state.ble_connected());
    }
}
