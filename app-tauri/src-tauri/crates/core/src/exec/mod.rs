//! Action execution: turn a configured [`Action`] into a real side effect.
//!
//! Port of `linux-app/system_actions.py:374` (`execute_action`). The dispatcher
//! is platform agnostic and every side effect goes through one of two injected
//! traits — [`CommandRunner`] for processes and [`InputInjector`] for
//! keyboard/text/media simulation — so each branch is testable without hardware,
//! a display or a sleep. Platform specifics (which tool to call, how to talk to
//! the OS) live behind those traits, not in here.
//!
//! Parity rules this module keeps:
//!
//! * The `(ok, reason)` pair is what the console prints, so every reason
//!   string is copied verbatim from the Python source.
//! * A missing capability is always a visible failure; nothing returns `ok`
//!   for work it did not do.
//! * Nothing here may panic or abort: `execute_action` converts a panicking
//!   handler into `fail("Action execution error: …")`, mirroring Python's
//!   `try/except` around the whole dispatch.

pub mod runner;

/// The button action chain: press → mode → button → actions
/// (`linux-app/serial_listener.py:70`).
pub mod chain;

/// The ten built-in presets (`linux-app/system_actions.py:16`). Always
/// compiled: the ids and the tool order are the same everywhere.
pub mod system;

/// Linux keyboard/text/media injection. Every other target falls back to
/// [`NoInputInjector`].
#[cfg(target_os = "linux")]
pub mod input_linux;

/// Windows keyboard/text/media injection.
///
/// Compiled on **every** target: the virtual-key tables and the event order
/// are pure, so the Linux CI job pins them, while the `SendInput` calls sit
/// behind `#[cfg(windows)]` and are only compiled and tested by the Windows
/// CI job.
pub mod input_windows;

use std::collections::HashMap;
use std::fmt;

use serde_json::Value;

use crate::config::model::Action;

pub use runner::{CommandCall, RealCommandRunner, RecordingRunner};

pub use chain::{
    resolve_actions, spawn_trigger_chain, ChainDeps, SharedCommandRunner, SharedInputInjector,
    SharedSleeper,
};

pub use system::{execute_system_preset, preset_by_id, SystemPreset, SYSTEM_PRESETS};

#[cfg(target_os = "linux")]
pub use input_linux::{InputTool, LinuxInjector};

#[cfg(target_os = "windows")]
pub use input_windows::WindowsInjector;

/// Result of one action: the verdict plus the exact console text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub ok: bool,
    pub reason: String,
}

impl ActionOutcome {
    pub fn ok(reason: impl Into<String>) -> ActionOutcome {
        ActionOutcome {
            ok: true,
            reason: reason.into(),
        }
    }

    pub fn fail(reason: impl Into<String>) -> ActionOutcome {
        ActionOutcome {
            ok: false,
            reason: reason.into(),
        }
    }

    /// `OK` / `FAIL` for the `[OK]` / `[FAIL]` tail of a console line.
    pub fn verdict(&self) -> &'static str {
        if self.ok {
            "OK"
        } else {
            "FAIL"
        }
    }
}

impl fmt::Display for ActionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.reason, self.verdict())
    }
}

/// Runs external programs (the `subprocess` calls of the Python apps).
pub trait CommandRunner {
    /// Run to completion and report what happened.
    fn run(&self, program: &str, args: &[&str]) -> ActionOutcome;

    /// Start and do not wait; the child outlives the caller.
    fn spawn_detached(&self, program: &str, args: &[&str]) -> ActionOutcome;
}

/// Simulates keyboard shortcuts, typed text and media keys.
pub trait InputInjector {
    /// The tool in use (`wtype`, `ydotool`, `xdotool`, `SendInput`, …), or
    /// `None` when the platform has none installed.
    fn available(&self) -> Option<String>;

    fn shortcut(&self, modifiers: &[String], key: &str) -> ActionOutcome;

    fn text(&self, text: &str) -> ActionOutcome;

    fn media(&self, media_key: &str) -> ActionOutcome;
}

/// `linux-app/system_actions.py:99` (`_injector_help`), verbatim.
pub const INPUT_INJECTOR_HELP: &str = concat!(
    "Keyboard simulation requires one of: `wtype` (Wayland, ",
    "sudo apt install wtype), `ydotool` (recommended on GNOME Wayland: ",
    "sudo apt install ydotool then run the daemon `sudo ydotoold` ",
    "or `systemctl --user start ydotool`), or `xdotool` (X11)."
);

/// The injector used when no input tool exists. Every call fails with the
/// Python help text instead of pretending the keys were sent.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoInputInjector;

impl InputInjector for NoInputInjector {
    fn available(&self) -> Option<String> {
        None
    }

    fn shortcut(&self, _modifiers: &[String], _key: &str) -> ActionOutcome {
        ActionOutcome::fail(INPUT_INJECTOR_HELP)
    }

    fn text(&self, _text: &str) -> ActionOutcome {
        ActionOutcome::fail(INPUT_INJECTOR_HELP)
    }

    fn media(&self, _media_key: &str) -> ActionOutcome {
        ActionOutcome::fail(INPUT_INJECTOR_HELP)
    }
}

/// The injector this platform should use: the Linux tool chain when one of
/// wtype/ydotool/xdotool is installed, the Windows injector (always, because
/// `SendInput` is part of `user32`), and the honest [`NoInputInjector`]
/// everywhere else — so a missing tool is a visible failure, never a silent
/// success.
///
/// Borrows the runner, because that is how Linux detects and runs its tool.
pub fn platform_input_injector<'a>(runner: &'a dyn CommandRunner) -> Box<dyn InputInjector + 'a> {
    #[cfg(target_os = "linux")]
    if let Some(injector) = input_linux::LinuxInjector::detect(runner) {
        return Box::new(injector);
    }
    // Windows has nothing to probe — `SendInput` is part of user32 — so it
    // takes the runner only to keep this one signature on every platform.
    let _ = runner;
    #[cfg(target_os = "windows")]
    return Box::new(input_windows::WindowsInjector::new());
    // Reached on Linux when no tool is installed and on every other target:
    // the honest fallback, not a pretend success.
    #[cfg(not(target_os = "windows"))]
    Box::new(NoInputInjector)
}

/// Pauses an action chain (`time.sleep`).
pub trait Sleeper {
    fn sleep(&self, seconds: f64);
}

/// The production sleeper: a real blocking sleep.
///
/// [`ExecDeps::new`] uses it; tests inject a double so `delay` is instant.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, seconds: f64) {
        if seconds.is_finite() && seconds > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
        }
    }
}

/// Shared handle to [`ThreadSleeper`], so `ExecDeps` can own a borrow of it.
pub static THREAD_SLEEPER: ThreadSleeper = ThreadSleeper;

/// Everything [`execute_action`] needs from the outside world.
#[derive(Clone, Copy)]
pub struct ExecDeps<'a> {
    pub runner: &'a dyn CommandRunner,
    pub input: &'a dyn InputInjector,
    pub sleeper: &'a dyn Sleeper,
}

impl<'a> ExecDeps<'a> {
    /// Production wiring: real commands and a real sleep.
    pub fn new(runner: &'a dyn CommandRunner, input: &'a dyn InputInjector) -> ExecDeps<'a> {
        ExecDeps {
            runner,
            input,
            sleeper: &THREAD_SLEEPER,
        }
    }

    /// Wiring with an explicit sleeper (tests, or a cancellable wait later).
    pub fn with_sleeper(
        runner: &'a dyn CommandRunner,
        input: &'a dyn InputInjector,
        sleeper: &'a dyn Sleeper,
    ) -> ExecDeps<'a> {
        ExecDeps {
            runner,
            input,
            sleeper,
        }
    }
}

/// `action_data.get("seconds", 0.5)`.
pub const DEFAULT_DELAY_SECONDS: f64 = 0.5;

/// `max(0.0, min(seconds, 600.0))`.
pub const MAX_DELAY_SECONDS: f64 = 600.0;

/// Shell used by `bash_script` (Python's `shell=True`).
#[cfg(windows)]
pub const SHELL_PROGRAM: &str = "cmd.exe";
/// Python's `executable='/bin/bash'` on Windows is `cmd.exe /C`.
#[cfg(windows)]
pub const SHELL_FLAG: &str = "/C";
#[cfg(not(windows))]
pub const SHELL_PROGRAM: &str = "/bin/sh";
#[cfg(not(windows))]
pub const SHELL_FLAG: &str = "-c";

/// Execute one configured action.
///
/// Never panics and never aborts: a panicking handler or a malformed payload
/// becomes `fail("Action execution error: …")`, exactly like the `except` in
/// `execute_action`.
pub fn execute_action(action: &Action, deps: &ExecDeps<'_>) -> ActionOutcome {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(action, deps))) {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(message)) => ActionOutcome::fail(format!("Action execution error: {message}")),
        Err(payload) => ActionOutcome::fail(format!(
            "Action execution error: {}",
            panic_message(payload.as_ref())
        )),
    }
}

/// The behaviour table of the M3.1 plan, one arm per action type.
fn dispatch(action: &Action, deps: &ExecDeps<'_>) -> Result<ActionOutcome, String> {
    let Action { action_type, extra } = action;
    match action_type.as_str() {
        // Chains skip `none` before calling, but a lone `none` is a no-op ok.
        "none" => Ok(ActionOutcome::ok("Nothing to do.")),
        "mode_toggle" => Ok(ActionOutcome::ok(
            "Mode layer is controlled by the macropad firmware (LED)",
        )),
        "delay" => delay(extra.get("seconds"), deps),
        "shortcut" => {
            let modifiers = modifiers(extra)?;
            let key = string_field(extra, "key", "")?;
            Ok(deps.input.shortcut(&modifiers, &key))
        }
        "text" => {
            let text = string_field(extra, "text", "")?;
            // Python: `if not text` — missing, null and "" fail, " " is typed.
            if text.is_empty() {
                Ok(ActionOutcome::fail("Empty text"))
            } else {
                Ok(deps.input.text(&text))
            }
        }
        "media" => {
            let media_key = string_field(extra, "mediaKey", "")?;
            Ok(deps.input.media(&media_key))
        }
        "bash_script" => bash_script(extra.get("script"), deps),
        // The ten built-in presets (`exec::system`); an id that is not in the
        // table keeps this exact message, so a stale config stays visible.
        "system_action" => Ok(system::execute_system_preset(
            &string_field(extra, "presetId", "")?,
            deps.runner,
            deps.input,
        )),
        // M3.2 types: visible failures, never a silent success.
        // `ha_service` is the legacy alias both Python apps route to the Home
        // Assistant branch (`elif action_type in ['home_assistant', 'ha_service']`),
        // so it belongs here too — otherwise an old config using that alias would
        // report "unknown action type" instead of "not yet available".
        "audio_output" | "launch_app" | "home_assistant" | "ha_service" | "ha_webhook" => Ok(
            ActionOutcome::fail(format!("Not available until M3.2: {action_type}")),
        ),
        other => Ok(ActionOutcome::fail(format!("Unknown action type: {other}"))),
    }
}

/// `time.sleep(seconds)` then `f"Waited {seconds:g}s."`.
fn delay(seconds: Option<&Value>, deps: &ExecDeps<'_>) -> Result<ActionOutcome, String> {
    let Some(raw) = parse_delay_seconds(seconds) else {
        return Ok(ActionOutcome::fail("Delay needs a valid duration."));
    };
    // Python: `max(0.0, min(seconds, 600.0))` — `min(nan, 600.0)` is `nan` and
    // `max(0.0, nan)` keeps 0.0, so a NaN payload also ends at zero.
    let clamped = if raw.is_nan() {
        0.0
    } else {
        raw.clamp(0.0, MAX_DELAY_SECONDS)
    };
    deps.sleeper.sleep(clamped);
    Ok(ActionOutcome::ok(format!(
        "Waited {}s.",
        format_like_python_g(clamped)
    )))
}

/// `float(action_data.get("seconds", 0.5))`, `None` when Python raises.
fn parse_delay_seconds(seconds: Option<&Value>) -> Option<f64> {
    match seconds {
        None => Some(DEFAULT_DELAY_SECONDS),
        Some(Value::Null) => None,
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        // Python: `float(True)` is 1.0.
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        Some(_) => None,
    }
}

/// `subprocess.Popen(script, shell=True, start_new_session=True)`.
fn bash_script(script: Option<&Value>, deps: &ExecDeps<'_>) -> Result<ActionOutcome, String> {
    let script = match script {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.trim().to_string(),
        Some(other) => {
            return Err(format!(
                "script must be a string, got {}",
                json_type_name(other)
            ))
        }
    };
    if script.is_empty() {
        return Ok(ActionOutcome::fail("Script content is empty"));
    }
    let outcome = deps
        .runner
        .spawn_detached(SHELL_PROGRAM, &[SHELL_FLAG, script.as_str()]);
    // A failed spawn is reported as itself: claiming the script was
    // "dispatched in background" when nothing started would be a lie.
    if outcome.ok {
        Ok(ActionOutcome::ok("Bash script dispatched in background"))
    } else {
        Ok(outcome)
    }
}

/// `action_data.get("modifiers", [])` — the order is the user's, the injector
/// decides how to apply it.
fn modifiers(extra: &HashMap<String, Value>) -> Result<Vec<String>, String> {
    match extra.get("modifiers") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()),
        Some(other) => Err(format!(
            "modifiers must be an array, got {}",
            json_type_name(other)
        )),
    }
}

/// Read an optional string payload: missing/null → `default`, a string → the
/// string, anything else → the error Python would have raised.
fn string_field(
    extra: &HashMap<String, Value>,
    key: &str,
    default: &str,
) -> Result<String, String> {
    match extra.get(key) {
        None | Some(Value::Null) => Ok(default.to_string()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(other) => Err(format!(
            "{key} must be a string, got {}",
            json_type_name(other)
        )),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Python's `f"{value:g}"` — six significant digits, trailing zeros stripped,
/// exponential below 1e-4 and at 1e6 or more (`600.0` → `600`).
fn format_like_python_g(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    // `{:e}` is the only rounding-to-6-significant-digits formatter available.
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("rust always emits an exponent");
    let exponent: i32 = exponent.parse().expect("rust always emits an integer");
    if exponent < -4 || exponent >= 6 {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{sign}{:02}",
            trim_trailing_zeros(mantissa),
            exponent.abs()
        )
    } else {
        let decimals = usize::try_from(5 - exponent).unwrap_or(0);
        trim_trailing_zeros(&format!("{value:.decimals$}"))
    }
}

fn trim_trailing_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    let trimmed = text.trim_end_matches('0');
    trimmed.strip_suffix('.').unwrap_or(trimmed).to_string()
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "panic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn python_g_matches_cpython() {
        for (value, expected) in [
            (0.0, "0"),
            (0.5, "0.5"),
            (1.25, "1.25"),
            (3.0, "3"),
            (600.0, "600"),
            (999999.0, "999999"),
            (0.0001, "0.0001"),
            (0.0000123, "1.23e-05"),
            (0.000001, "1e-06"),
            (123.456789, "123.457"),
        ] {
            assert_eq!(format_like_python_g(value), expected, "for {value}");
        }
    }

    #[test]
    fn python_g_uses_the_exponential_form_at_1e6() {
        assert_eq!(format_like_python_g(1_000_000.0), "1e+06");
        assert_eq!(format_like_python_g(1_234_567.0), "1.23457e+06");
    }

    #[test]
    fn trim_trailing_zeros_keeps_integers_intact() {
        assert_eq!(trim_trailing_zeros("600.000"), "600");
        assert_eq!(trim_trailing_zeros("600"), "600");
        assert_eq!(trim_trailing_zeros("1.23000"), "1.23");
    }

    #[test]
    fn json_type_names_cover_every_shape() {
        assert_eq!(json_type_name(&Value::Null), "null");
        assert_eq!(json_type_name(&json!(true)), "bool");
        assert_eq!(json_type_name(&json!(1)), "number");
        assert_eq!(json_type_name(&json!("s")), "string");
        assert_eq!(json_type_name(&json!([])), "array");
        assert_eq!(json_type_name(&json!({})), "object");
    }

    #[test]
    fn panic_payloads_are_rendered() {
        assert_eq!(panic_message(&"static str"), "static str");
        assert_eq!(panic_message(&String::from("owned")), "owned");
        assert_eq!(panic_message(&42_u8), "panic");
    }

    #[test]
    fn the_default_sleeper_never_blocks_on_a_non_positive_delay() {
        let runner = RecordingRunner::new(ActionOutcome::ok("ran"));
        let deps = ExecDeps::new(&runner, &NoInputInjector);
        deps.sleeper.sleep(0.0);
        deps.sleeper.sleep(-1.0);
        deps.sleeper.sleep(f64::NAN);
        deps.sleeper.sleep(f64::INFINITY);
    }

    // Linux-only contract: with no keyboard-injection tool installed, the
    // platform injector must admit it and show the help text. On Windows the
    // injector is SendInput and always available, so this does not apply there —
    // Windows gets its own contract below (executed by the Windows CI job).
    #[cfg(target_os = "linux")]
    #[test]
    fn the_platform_injector_reports_a_missing_tool_instead_of_pretending() {
        let installed = RecordingRunner::new(ActionOutcome::ok("/usr/bin/wtype"));
        assert_eq!(
            platform_input_injector(&installed).available().as_deref(),
            Some("wtype")
        );
        let missing = RecordingRunner::new(ActionOutcome::fail("not found"));
        let injector = platform_input_injector(&missing);
        assert!(injector.available().is_none());
        assert_eq!(injector.text("hi").reason, INPUT_INJECTOR_HELP);
        assert_eq!(
            injector.shortcut(&["ctrl".into()], "t").reason,
            INPUT_INJECTOR_HELP
        );
        assert_eq!(injector.media("PLAY_PAUSE").reason, INPUT_INJECTOR_HELP);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn the_windows_injector_never_reports_a_missing_tool() {
        let runner = RecordingRunner::new(ActionOutcome::fail("unused on windows"));
        assert_eq!(
            platform_input_injector(&runner).available().as_deref(),
            Some("SendInput")
        );
    }
}
