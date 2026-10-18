//! Headless application state machine — working config, saved-baseline dirty
//! diff and subscriber event bus.
//!
//! Direct port of `linux-app/state.py` minus GTK/GLib: emissions go through a
//! plain callback registry (the Tauri layer bridges [`AppEvent`]s to the UI in
//! M2), and every public method is safe to call from the device thread while
//! the UI thread reads (internal `Mutex`, `AppState: Send + Sync`).
//!
//! Semantics pinned to Python:
//! - the dirty flag is derived by comparing the working config against the
//!   baseline snapshot, **ignoring** the non-edit keys `activeModeIndex` and
//!   `activePreset`; `dirty-changed` is emitted only when the flag flips;
//! - `set_label`/`set_action`/`set_timing`/`add_action`/`remove_action`/
//!   `move_action` always emit their event (only *dirty* is flip-gated);
//! - `set_mode`/`select_button` early-return without an event when unchanged;
//! - status setters dedupe (no event when the value is unchanged);
//! - `mode_index` always starts at 0 regardless of the stored `activeModeIndex`;
//! - subscriber callbacks run **outside** the internal locks.

use crate::config::{
    default_action, preset_slug, Action, Button, Config, ConfigStore, Mode, TriggerActions,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A subscriber callback: receives every emitted [`AppEvent`].
type Callback = Arc<dyn Fn(AppEvent) + Send + Sync>;
type Subscribers = Arc<Mutex<Vec<(u64, Callback)>>>;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Everything the core can tell the UI about.
///
/// The wire shape is pinned by [`AppEvent::payload`]: a flat snake_case object
/// per event (no external-tag nesting), matching the keys `linux-app` emits.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppEvent {
    DirtyChanged {
        dirty: bool,
    },
    ConfigReplaced {
        source: String,
    },
    ButtonChanged {
        index: i64,
    },
    TimingChanged {
        field: String,
        value: i64,
    },
    ModeChanged {
        mode_index: i64,
    },
    SelectionChanged {
        index: i64,
    },
    ConfigSaved {
        ok: bool,
        preset: Option<String>,
    },
    PresetsChanged {
        name: String,
    },
    SerialStatusChanged {
        connected: bool,
        port: Option<String>,
    },
    BleStatusChanged {
        connected: bool,
        message: String,
    },
    DeviceNameChanged {
        name: Option<String>,
    },
    DeviceInfoChanged {
        version: Option<String>,
    },
    SerialLog {
        line: String,
    },
    HaStatusChanged {
        connected: bool,
        message: String,
    },
    HaEntitiesChanged {
        count: usize,
    },
    AppsChanged {
        count: usize,
    },
}

impl AppEvent {
    /// The Python event name (`"dirty-changed"`, …).
    pub fn name(&self) -> &'static str {
        match self {
            AppEvent::DirtyChanged { .. } => "dirty-changed",
            AppEvent::ConfigReplaced { .. } => "config-replaced",
            AppEvent::ButtonChanged { .. } => "button-changed",
            AppEvent::TimingChanged { .. } => "timing-changed",
            AppEvent::ModeChanged { .. } => "mode-changed",
            AppEvent::SelectionChanged { .. } => "selection-changed",
            AppEvent::ConfigSaved { .. } => "config-saved",
            AppEvent::PresetsChanged { .. } => "presets-changed",
            AppEvent::SerialStatusChanged { .. } => "serial-status-changed",
            AppEvent::BleStatusChanged { .. } => "ble-status-changed",
            AppEvent::DeviceNameChanged { .. } => "device-name-changed",
            AppEvent::DeviceInfoChanged { .. } => "device-info-changed",
            AppEvent::SerialLog { .. } => "serial-log",
            AppEvent::HaStatusChanged { .. } => "ha-status-changed",
            AppEvent::HaEntitiesChanged { .. } => "ha-entities-changed",
            AppEvent::AppsChanged { .. } => "apps-changed",
        }
    }

    /// `(event name, flat snake_case payload)` — exactly what the Tauri bridge
    /// forwards as `octopad://<name>` without any external-tag nesting.
    pub fn payload(&self) -> (&'static str, Value) {
        let payload = match self {
            AppEvent::DirtyChanged { dirty } => json!({ "dirty": dirty }),
            AppEvent::ConfigReplaced { source } => json!({ "source": source }),
            AppEvent::ButtonChanged { index } => json!({ "index": index }),
            AppEvent::TimingChanged { field, value } => {
                json!({ "field": field, "value": value })
            }
            AppEvent::ModeChanged { mode_index } => json!({ "mode_index": mode_index }),
            AppEvent::SelectionChanged { index } => json!({ "index": index }),
            AppEvent::ConfigSaved { ok, preset } => json!({ "ok": ok, "preset": preset }),
            AppEvent::PresetsChanged { name } => json!({ "name": name }),
            AppEvent::SerialStatusChanged { connected, port } => {
                json!({ "connected": connected, "port": port })
            }
            AppEvent::BleStatusChanged { connected, message } => {
                json!({ "connected": connected, "message": message })
            }
            AppEvent::DeviceNameChanged { name } => json!({ "name": name }),
            AppEvent::DeviceInfoChanged { version } => json!({ "version": version }),
            AppEvent::SerialLog { line } => json!({ "line": line }),
            AppEvent::HaStatusChanged { connected, message } => {
                json!({ "connected": connected, "message": message })
            }
            AppEvent::HaEntitiesChanged { count } => json!({ "count": count }),
            AppEvent::AppsChanged { count } => json!({ "count": count }),
        };
        (self.name(), payload)
    }
}

// ---------------------------------------------------------------------------
// Subscription handle
// ---------------------------------------------------------------------------

/// Handle returned by [`AppState::subscribe`].
///
/// Dropping the handle does **not** remove the callback (Python parity:
/// `state.unsubscribe(callback)` is explicit) — call [`Subscription::unsubscribe`]
/// or [`AppState::unsubscribe`] with [`Subscription::id`].
#[derive(Clone)]
pub struct Subscription {
    id: u64,
    subscribers: Subscribers,
}

impl Subscription {
    /// Stable id of this subscription (accepted by [`AppState::unsubscribe`]).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Stop receiving events. Idempotent.
    pub fn unsubscribe(&self) {
        let mut guard = lock_ignore_poison(&self.subscribers);
        guard.retain(|(id, _)| *id != self.id);
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct Inner {
    config: Config,
    /// Snapshot of the config exactly as it was last loaded/saved.
    saved: Config,
    mode_index: i64,
    selected_index: i64,
    dirty: bool,
    active_preset: Option<String>,
    serial_connected: bool,
    serial_port: Option<String>,
    ble_connected: bool,
    ble_message: String,
    device_name: Option<String>,
    firmware_version: Option<String>,
    ha_connected: bool,
    ha_message: String,
    ha_entities_count: usize,
    installed_apps_count: usize,
}

/// Thread-safe application state (`Send + Sync`).
pub struct AppState {
    store: ConfigStore,
    inner: Mutex<Inner>,
    subscribers: Subscribers,
    next_subscription: AtomicU64,
}

impl AppState {
    /// Load the configuration from `store`, set the saved baseline and drop a
    /// stale `activePreset` whose preset file no longer exists.
    ///
    /// Mirrors `state.py` module init: Python hardcodes the startup mode to
    /// Desktop, so **`mode_index` is always 0** regardless of the stored
    /// `activeModeIndex`.
    pub fn new(store: ConfigStore) -> AppState {
        let config = store.load_config();
        let active_preset = config
            .active_preset
            .clone()
            .filter(|name| store.list_presets().contains(name));
        let saved = config.clone();
        AppState {
            store,
            inner: Mutex::new(Inner {
                config,
                saved,
                mode_index: 0,
                selected_index: 0,
                dirty: false,
                active_preset,
                serial_connected: false,
                serial_port: None,
                ble_connected: false,
                ble_message: String::new(),
                device_name: None,
                firmware_version: None,
                ha_connected: false,
                ha_message: String::new(),
                ha_entities_count: 0,
                installed_apps_count: 0,
            }),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            next_subscription: AtomicU64::new(1),
        }
    }

    /// The store backing this state (config + user presets).
    pub fn store(&self) -> &ConfigStore {
        &self.store
    }

    // -- subscriptions ------------------------------------------------------

    /// Register a callback for every [`AppEvent`].
    pub fn subscribe<F>(&self, callback: F) -> Subscription
    where
        F: Fn(AppEvent) + Send + Sync + 'static,
    {
        let id = self.next_subscription.fetch_add(1, Ordering::Relaxed);
        let mut guard = lock_ignore_poison(&self.subscribers);
        guard.push((id, Arc::new(callback)));
        drop(guard);
        Subscription {
            id,
            subscribers: self.subscribers.clone(),
        }
    }

    /// Remove a subscription by id (see [`Subscription::id`]). Unknown ids are
    /// ignored.
    pub fn unsubscribe(&self, id: u64) {
        let mut guard = lock_ignore_poison(&self.subscribers);
        guard.retain(|(subscriber_id, _)| *subscriber_id != id);
    }

    /// Deliver already-collected events to every subscriber.
    ///
    /// The subscriber list is cloned and the lock released **before** invoking
    /// any callback, so a subscriber may re-enter the state (or subscribe/
    /// unsubscribe) without deadlocking. A panicking subscriber is caught and
    /// the remaining callbacks still run.
    fn dispatch(&self, events: Vec<AppEvent>) {
        if events.is_empty() {
            return;
        }
        let subscribers: Vec<(u64, Callback)> = {
            let guard = lock_ignore_poison(&self.subscribers);
            guard.clone()
        };
        if subscribers.is_empty() {
            return;
        }
        for event in events {
            for (id, callback) in &subscribers {
                let result = catch_unwind(AssertUnwindSafe(|| callback(event.clone())));
                if result.is_err() {
                    tracing::warn!("[STATE] subscriber {id} panicked");
                }
            }
        }
    }

    /// Publish a serial-console line to every event subscriber.
    pub fn log(&self, line: impl Into<String>) {
        self.dispatch(vec![AppEvent::SerialLog { line: line.into() }]);
    }

    // -- read-only views ----------------------------------------------------

    /// Copy of the working configuration.
    pub fn config(&self) -> Config {
        self.lock().config.clone()
    }

    /// The mode currently displayed (mode_index always starts at 0).
    pub fn current_mode(&self) -> Mode {
        let inner = self.lock();
        mode_ref(&inner).clone()
    }

    /// The button currently being edited.
    pub fn current_button(&self) -> Button {
        let inner = self.lock();
        button_ref(&inner).clone()
    }

    pub fn mode_index(&self) -> i64 {
        self.lock().mode_index
    }

    pub fn selected_index(&self) -> i64 {
        self.lock().selected_index
    }

    pub fn is_dirty(&self) -> bool {
        self.lock().dirty
    }

    pub fn active_preset(&self) -> Option<String> {
        self.lock().active_preset.clone()
    }

    /// True when the working config differs from the saved/loaded baseline.
    ///
    /// `activeModeIndex`/`activePreset` are navigation metadata, not edits.
    pub fn has_changes(&self) -> bool {
        working_differs(&self.lock())
    }

    /// The actions in a trigger slot (`single`/`double`/`hold`); a legacy dict
    /// slot normalizes to a one-element list.
    pub fn get_actions(&self, trigger: &str) -> Vec<Action> {
        let inner = self.lock();
        actions_of(button_ref(&inner), trigger)
    }

    pub fn serial_connected(&self) -> bool {
        self.lock().serial_connected
    }

    pub fn serial_port(&self) -> Option<String> {
        self.lock().serial_port.clone()
    }

    pub fn ble_connected(&self) -> bool {
        self.lock().ble_connected
    }

    pub fn ble_message(&self) -> String {
        self.lock().ble_message.clone()
    }

    pub fn device_name(&self) -> Option<String> {
        self.lock().device_name.clone()
    }

    pub fn firmware_version(&self) -> Option<String> {
        self.lock().firmware_version.clone()
    }

    pub fn ha_connected(&self) -> bool {
        self.lock().ha_connected
    }

    pub fn ha_message(&self) -> String {
        self.lock().ha_message.clone()
    }

    pub fn ha_entities_count(&self) -> usize {
        self.lock().ha_entities_count
    }

    pub fn installed_apps_count(&self) -> usize {
        self.lock().installed_apps_count
    }

    // -- dirty bookkeeping --------------------------------------------------

    /// Re-evaluate the dirty flag (emits `dirty-changed` only on a flip).
    pub fn mark_dirty(&self) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if let Some(event) = sync_dirty(&mut inner) {
                events.push(event);
            }
        }
        self.dispatch(events);
    }

    // -- config mutations ---------------------------------------------------

    /// Replace the entire configuration (import/preset/reset) and apply it.
    pub fn set_config(&self, config: Config, source: &str) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            inner.config = config;
            events.push(AppEvent::ConfigReplaced {
                source: source.to_string(),
            });
            if let Some(event) = sync_dirty(&mut inner) {
                events.push(event);
            }
        }
        self.dispatch(events);
    }

    /// Change the label of the button currently being edited.
    pub fn set_label(&self, text: &str) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            button_mut(&mut inner).label = text.to_string();
            if let Some(event) = sync_dirty(&mut inner) {
                events.push(event);
            }
            events.push(AppEvent::ButtonChanged {
                index: inner.selected_index,
            });
        }
        self.dispatch(events);
    }

    /// Replace the action at `index` in the trigger slot, appending beyond the
    /// end (Python `state.set_action`).
    pub fn set_action(&self, trigger: &str, index: i64, action: Action) {
        if !is_trigger(trigger) {
            return;
        }
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            let mut actions = actions_of(button_ref(&inner), trigger);
            let len = actions.len() as i64;
            let target = if index < 0 { index + len } else { index };
            if target >= 0 && target < len {
                actions[target as usize] = action;
            } else {
                actions.push(action);
            }
            set_actions(button_mut(&mut inner), trigger, actions);
            push_button_changed(&mut inner, &mut events);
        }
        self.dispatch(events);
    }

    /// Append an action to the trigger slot (defaults to `none`).
    pub fn add_action(&self, trigger: &str, action: Option<Action>) {
        if !is_trigger(trigger) {
            return;
        }
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            let mut actions = actions_of(button_ref(&inner), trigger);
            actions.push(action.unwrap_or_else(|| default_action("none")));
            set_actions(button_mut(&mut inner), trigger, actions);
            push_button_changed(&mut inner, &mut events);
        }
        self.dispatch(events);
    }

    /// Remove the action at `index`; the slot always keeps at least one `none`.
    pub fn remove_action(&self, trigger: &str, index: i64) {
        if !is_trigger(trigger) {
            return;
        }
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            let mut actions = actions_of(button_ref(&inner), trigger);
            let len = actions.len() as i64;
            let target = if index < 0 { index + len } else { index };
            if target >= 0 && target < len {
                actions.remove(target as usize);
            }
            if actions.is_empty() {
                actions.push(default_action("none"));
            }
            set_actions(button_mut(&mut inner), trigger, actions);
            push_button_changed(&mut inner, &mut events);
        }
        self.dispatch(events);
    }

    /// Move an action within the trigger slot; out-of-range moves are silent
    /// no-ops (no event, matching `state.py`).
    pub fn move_action(&self, trigger: &str, index: i64, delta: i64) {
        if !is_trigger(trigger) {
            return;
        }
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            let mut actions = actions_of(button_ref(&inner), trigger);
            let len = actions.len() as i64;
            let target = index + delta;
            if index < 0 || index >= len || target < 0 || target >= len {
                return;
            }
            actions.swap(index as usize, target as usize);
            set_actions(button_mut(&mut inner), trigger, actions);
            push_button_changed(&mut inner, &mut events);
        }
        self.dispatch(events);
    }

    /// Update a timing field; only the three camelCase keys are accepted, any
    /// other field is a no-op returning `false`.
    pub fn set_timing(&self, field: &str, value: i64) -> bool {
        if !matches!(field, "debounceMs" | "clickTimeoutMs" | "holdTimeoutMs") {
            return false;
        }
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            match field {
                "debounceMs" => inner.config.debounce_ms = json!(value),
                "clickTimeoutMs" => inner.config.click_timeout_ms = json!(value),
                _ => inner.config.hold_timeout_ms = json!(value),
            }
            if let Some(event) = sync_dirty(&mut inner) {
                events.push(event);
            }
            events.push(AppEvent::TimingChanged {
                field: field.to_string(),
                value,
            });
        }
        self.dispatch(events);
        true
    }

    /// Switch the active mode (no event when unchanged; not an edit).
    pub fn set_mode(&self, index: i64) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if index == inner.mode_index {
                return;
            }
            inner.mode_index = index;
            inner.config.active_mode_index = index;
            events.push(AppEvent::ModeChanged { mode_index: index });
        }
        self.dispatch(events);
    }

    /// Select the button being edited (no event when unchanged).
    pub fn select_button(&self, index: i64) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if index == inner.selected_index {
                return;
            }
            inner.selected_index = index;
            events.push(AppEvent::SelectionChanged { index });
        }
        self.dispatch(events);
    }

    /// Save the working config to disk and make it the new saved baseline.
    pub fn save(&self) -> bool {
        let mut events = Vec::new();
        let mut ok = false;
        {
            let mut inner = self.lock_mut();
            match self.store.save_config(&inner.config) {
                Ok(()) => {
                    inner.saved = inner.config.clone();
                    if let Some(event) = sync_dirty(&mut inner) {
                        events.push(event);
                    }
                    events.push(AppEvent::ConfigSaved {
                        ok: true,
                        preset: None,
                    });
                    ok = true;
                }
                Err(exc) => {
                    tracing::warn!("[STATE] failed to save config: {exc}");
                    events.push(AppEvent::ConfigSaved {
                        ok: false,
                        preset: None,
                    });
                }
            }
        }
        self.dispatch(events);
        ok
    }

    /// Save the working config as a user preset.
    ///
    /// `overwrite=true` replaces the given preset, or the active one when
    /// `name` is empty; with neither, `config-saved{ok:false}` is emitted.
    pub fn save_preset(&self, name: &str, overwrite: bool) -> bool {
        let mut events = Vec::new();
        let mut ok = false;
        {
            let mut inner = self.lock_mut();
            let preset_name = if overwrite {
                if name.is_empty() {
                    inner.active_preset.clone()
                } else {
                    Some(preset_slug(name))
                }
            } else {
                Some(preset_slug(name))
            };

            let Some(preset_name) = preset_name else {
                events.push(AppEvent::ConfigSaved {
                    ok: false,
                    preset: None,
                });
                drop(inner);
                self.dispatch(events);
                return false;
            };

            // The snapshot must not pin the current "active preset" pointer.
            let mut snapshot = inner.config.clone();
            snapshot.active_preset = None;
            match self.store.save_preset(&preset_name, &snapshot) {
                Ok(()) => {
                    inner.active_preset = Some(preset_name.clone());
                    inner.config.active_preset = Some(preset_name.clone());
                    if let Err(exc) = self.store.save_config(&inner.config) {
                        tracing::warn!("[STATE] failed to persist active preset: {exc}");
                    }
                    inner.saved = inner.config.clone();
                    if let Some(event) = sync_dirty(&mut inner) {
                        events.push(event);
                    }
                    events.push(AppEvent::ConfigSaved {
                        ok: true,
                        preset: Some(preset_name),
                    });
                    ok = true;
                }
                Err(exc) => {
                    tracing::warn!("[STATE] failed to save preset '{name}': {exc}");
                    events.push(AppEvent::ConfigSaved {
                        ok: false,
                        preset: None,
                    });
                }
            }
        }
        self.dispatch(events);
        ok
    }

    /// Load a user preset into the working config.
    ///
    /// The active flag is set **before** `config-replaced` reaches listeners so
    /// the UI can rebuild its preset list with the right indicator; the config
    /// is then replaced through the normal dirty flow (presets leave the
    /// configuration looking changed, like `state.py`).
    pub fn apply_user_preset(&self, name: &str) -> bool {
        let Some(mut data) = self.store.load_preset(name) else {
            return false;
        };
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            inner.active_preset = Some(name.to_string());
            data.active_preset = Some(name.to_string());
            inner.config = data;
            events.push(AppEvent::ConfigReplaced {
                source: "preset".to_string(),
            });
            if let Some(event) = sync_dirty(&mut inner) {
                events.push(event);
            }
        }
        self.dispatch(events);
        true
    }

    /// Delete a user preset.
    ///
    /// When it was the active preset the metadata is persisted, but **never**
    /// over unsaved edits (`state.py`: the working config is only written when
    /// clean). Always emits `presets-changed` on success.
    pub fn delete_user_preset(&self, name: &str) -> bool {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if !self
                .store
                .list_presets()
                .iter()
                .any(|preset| preset == name)
            {
                return false;
            }
            if !self.store.delete_preset(name) {
                return false;
            }
            if inner.active_preset.as_deref() == Some(name) {
                inner.active_preset = None;
                inner.config.active_preset = None;
                if !inner.dirty {
                    if let Err(exc) = self.store.save_config(&inner.config) {
                        tracing::warn!("[STATE] failed to persist preset metadata: {exc}");
                    }
                    inner.saved = inner.config.clone();
                    if let Some(event) = sync_dirty(&mut inner) {
                        events.push(event);
                    }
                }
            }
            events.push(AppEvent::PresetsChanged {
                name: name.to_string(),
            });
            if let Some(event) = sync_dirty(&mut inner) {
                events.push(event);
            }
        }
        self.dispatch(events);
        true
    }

    // -- status setters (all dedupe) ----------------------------------------

    pub fn set_serial_status(&self, connected: bool, port: Option<String>) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if inner.serial_connected == connected && inner.serial_port == port {
                return;
            }
            inner.serial_connected = connected;
            inner.serial_port = port.clone();
            events.push(AppEvent::SerialStatusChanged { connected, port });
        }
        self.dispatch(events);
    }

    pub fn set_ble_status(&self, connected: bool, message: &str) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if inner.ble_connected == connected && inner.ble_message == message {
                return;
            }
            inner.ble_connected = connected;
            inner.ble_message = message.to_string();
            events.push(AppEvent::BleStatusChanged {
                connected,
                message: message.to_string(),
            });
        }
        self.dispatch(events);
    }

    /// Set the pad's user-configured name (trimmed; empty/blank → `None`).
    pub fn set_device_name(&self, name: Option<String>) {
        let name = normalize_optional(name);
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if inner.device_name == name {
                return;
            }
            inner.device_name = name.clone();
            events.push(AppEvent::DeviceNameChanged { name });
        }
        self.dispatch(events);
    }

    /// Update the firmware version reported by the HELLO line.
    pub fn set_firmware_version(&self, version: Option<String>) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            let version = normalize_optional(version);
            if inner.firmware_version == version {
                return;
            }
            inner.firmware_version = version.clone();
            events.push(AppEvent::DeviceInfoChanged { version });
        }
        self.dispatch(events);
    }

    pub fn set_ha_status(&self, connected: bool, message: &str) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if inner.ha_connected == connected && inner.ha_message == message {
                return;
            }
            inner.ha_connected = connected;
            inner.ha_message = message.to_string();
            events.push(AppEvent::HaStatusChanged {
                connected,
                message: message.to_string(),
            });
        }
        self.dispatch(events);
    }

    pub fn set_ha_entities(&self, count: usize) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if inner.ha_entities_count == count {
                return;
            }
            inner.ha_entities_count = count;
            events.push(AppEvent::HaEntitiesChanged { count });
        }
        self.dispatch(events);
    }

    pub fn set_apps(&self, count: usize) {
        let mut events = Vec::new();
        {
            let mut inner = self.lock_mut();
            if inner.installed_apps_count == count {
                return;
            }
            inner.installed_apps_count = count;
            events.push(AppEvent::AppsChanged { count });
        }
        self.dispatch(events);
    }

    // -- internal locking ----------------------------------------------------

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        lock_ignore_poison(&self.inner)
    }

    fn lock_mut(&self) -> std::sync::MutexGuard<'_, Inner> {
        lock_ignore_poison(&self.inner)
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Lock a mutex, recovering the value if another thread panicked while holding
/// it (a poisoned lock must not take the whole app down).
fn lock_ignore_poison<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn is_trigger(trigger: &str) -> bool {
    matches!(trigger, "single" | "double" | "hold")
}

/// A normalized view used to detect real user edits: runtime metadata keys are
/// excluded (Python `_config_signature`).
fn working_differs(inner: &Inner) -> bool {
    let mut working = inner.config.clone();
    working.active_mode_index = 0;
    working.active_preset = None;
    let mut saved = inner.saved.clone();
    saved.active_mode_index = 0;
    saved.active_preset = None;
    working != saved
}

/// Recompute the dirty flag; emit `dirty-changed` only when it flips.
fn sync_dirty(inner: &mut Inner) -> Option<AppEvent> {
    let dirty = working_differs(inner);
    if dirty == inner.dirty {
        return None;
    }
    inner.dirty = dirty;
    Some(AppEvent::DirtyChanged { dirty })
}

/// Dirty bookkeeping plus the `button-changed` event, in Python's order.
fn push_button_changed(inner: &mut Inner, events: &mut Vec<AppEvent>) {
    if let Some(event) = sync_dirty(inner) {
        events.push(event);
    }
    events.push(AppEvent::ButtonChanged {
        index: inner.selected_index,
    });
}

fn clamp(index: i64, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    index.clamp(0, (len - 1) as i64) as usize
}

fn mode_ref(inner: &Inner) -> &Mode {
    let index = clamp(inner.mode_index, inner.config.modes.len());
    inner
        .config
        .modes
        .get(index)
        .expect("configuration must contain at least one mode")
}

fn button_ref(inner: &Inner) -> &Button {
    let mode = mode_ref(inner);
    let index = clamp(inner.selected_index, mode.buttons.len());
    mode.buttons
        .get(index)
        .expect("mode must contain at least one button")
}

fn button_mut(inner: &mut Inner) -> &mut Button {
    let mode_index = clamp(inner.mode_index, inner.config.modes.len());
    let selected = inner.selected_index;
    let mode = inner
        .config
        .modes
        .get_mut(mode_index)
        .expect("configuration must contain at least one mode");
    let button_index = clamp(selected, mode.buttons.len());
    mode.buttons
        .get_mut(button_index)
        .expect("mode must contain at least one button")
}

/// The `actions_list` view of one trigger slot.
fn actions_of(button: &Button, trigger: &str) -> Vec<Action> {
    match trigger {
        "single" => button.single.as_slice().to_vec(),
        "double" => button.double.as_slice().to_vec(),
        "hold" => button.hold.as_slice().to_vec(),
        _ => Vec::new(),
    }
}

/// Write a trigger slot back, reusing the shared `TriggerActions` shape helpers.
fn set_actions(button: &mut Button, trigger: &str, actions: Vec<Action>) {
    let slot = TriggerActions::from_list(actions);
    match trigger {
        "single" => button.single = slot,
        "double" => button.double = slot,
        "hold" => button.hold = slot,
        _ => {}
    }
}

/// Python `(value or "").strip() or None`.
fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
