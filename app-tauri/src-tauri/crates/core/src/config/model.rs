//! Serde data model mirroring the on-disk JSON shape of `linux-app/config.py`.
//!
//! Action payloads are deliberately loose: `{"type": ...}` plus flattened
//! extras so every action shape (present and future) survives a round-trip
//! without the model knowing each type's fields.

use serde::{Deserialize, Serialize};
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

/// One physical button within a mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    pub index: i64,
    pub pin: i64,
    pub label: String,
    pub single: Action,
    pub double: Action,
    pub hold: Action,
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
