//! Serde data model mirroring the on-disk JSON shape of `linux-app/config.py`.
//!
//! Action payloads are deliberately loose: `{"type": ...}` plus flattened
//! extras so every action shape (present and future) survives a round-trip
//! without the model knowing each type's fields.

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;

/// A single action in a trigger slot. Known field is `type`; every other key
/// (`modifiers`, `key`, `text`, …) lives in `extra` and serializes flattened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    #[serde(rename = "type", default)]
    pub action_type: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Contents of one trigger slot (`single`/`double`/`hold`).
///
/// Python parity (`config.actions_list`, `state.add_action/remove_action/…`):
/// a slot holds a **list** of actions, but legacy configs store a single dict —
/// both shapes must round-trip byte-identically, so the original JSON shape is
/// preserved (`One` serializes as an object, `Many` as an array).
///
/// `Raw` is the escape hatch for anything neither shape matches (scalars,
/// null); Python would keep those verbatim in the dict and let later logic
/// deal with them, so they serialize back untouched too.
///
/// Manual (de)serialization instead of `#[serde(untagged)]`: `Action` accepts
/// any JSON object via its flattened extras, which would make an untagged
/// `One(Action)` variant swallow arrays as well.
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerActions {
    /// Legacy single-action slot (dict on disk).
    One(Action),
    /// Multi-action sequence (list on disk).
    Many(Vec<Action>),
    /// Non-dict, non-list slot — preserved verbatim, never interpreted.
    Raw(serde_json::Value),
}

impl TriggerActions {
    /// The actions in this slot (the `actions_list` view): `One` → one item,
    /// `Many` → every kept action, anything else → empty.
    pub fn as_slice(&self) -> &[Action] {
        match self {
            TriggerActions::One(action) => std::slice::from_ref(action),
            TriggerActions::Many(actions) => actions.as_slice(),
            TriggerActions::Raw(_) => &[],
        }
    }

    /// Build a slot from a list of actions: one item collapses to the legacy
    /// dict shape, more stay a list (matches what the merge would produce for
    /// the same sequence).
    pub fn from_list(actions: Vec<Action>) -> Self {
        match actions.len() {
            0 => TriggerActions::Many(Vec::new()),
            1 => TriggerActions::One(actions.into_iter().next().unwrap()),
            _ => TriggerActions::Many(actions),
        }
    }
}

// `One` is the legacy dict shape; `as_slice()` is the canonical list view.

impl Serialize for TriggerActions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TriggerActions::One(action) => action.serialize(serializer),
            TriggerActions::Many(actions) => actions.serialize(serializer),
            TriggerActions::Raw(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for TriggerActions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value {
            Value::Object(_) => {
                TriggerActions::One(serde_json::from_value(value).map_err(de::Error::custom)?)
            }
            Value::Array(items) => {
                let actions = items
                    .into_iter()
                    .filter(Value::is_object)
                    .map(|item| serde_json::from_value(item).map_err(de::Error::custom))
                    .collect::<Result<Vec<_>, _>>()?;
                TriggerActions::Many(actions)
            }
            other => TriggerActions::Raw(other),
        })
    }
}

/// One physical button within a mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    pub index: i64,
    pub pin: i64,
    pub label: String,
    pub single: TriggerActions,
    pub double: TriggerActions,
    pub hold: TriggerActions,
}

/// A mode layer (desktop, ha, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub id: String,
    pub name: String,
    pub led_state: bool,
    pub buttons: Vec<Button>,
}

/// The full configuration, matching the `config.json` shape.
///
/// Timings stay loosely typed (`serde_json::Value`) so a stored non-numeric
/// value can flow through to `timing_command`, which returns `None` for it —
/// Python parity (`int("not-a-number")` → ValueError → no sync).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub version: f64,
    pub schema_version: i64,
    pub device: String,
    pub debounce_ms: serde_json::Value,
    pub click_timeout_ms: serde_json::Value,
    pub hold_timeout_ms: serde_json::Value,
    pub active_mode_index: i64,
    /// Absent on disk when `None` (Python's default config has no such key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_preset: Option<String>,
    pub modes: Vec<Mode>,
}
