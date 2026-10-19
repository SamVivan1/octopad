//! Task 3 (M3.1): the ten built-in system presets.
//!
//! Source of truth: `linux-app/system_actions.py:16` (`SYSTEM_ACTION_PRESETS`)
//! and `:465` (`_execute_system_preset`), cross-checked against
//! `windows-app/platform_actions.py:38`/`:404`. Ids must stay identical on both
//! OSes so a profile keeps working after a platform switch (parity checklist
//! §7b decision 4), while the labels are OS neutral.
//!
//! Nothing shells out: availability is answered by a fake `which` and every
//! real command is recorded by a [`RecordingRunner`].

use octopad_core::config::model::Action;
use octopad_core::exec::{
    execute_action, execute_system_preset, preset_by_id, ActionOutcome, CommandCall, CommandRunner,
    ExecDeps, InputInjector, RecordingRunner, SYSTEM_PRESETS,
};
use serde_json::{json, Value};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// `which <tool>` request, if that is what the call is.
fn probe_target<'a>(program: &str, args: &'a [&'a str]) -> Option<&'a str> {
    match (program, args) {
        ("which" | "whence" | "command", [_tool]) => Some(args[0]),
        _ => None,
    }
}

/// A shell whose `PATH` is fixed: probes answer from `installed`, every other
/// call is recorded in the inner [`RecordingRunner`] so the assertions see the
/// exact argv the preset built — and nothing else.
struct FakeShell {
    installed: Vec<String>,
    commands: RecordingRunner,
    probes: Mutex<Vec<String>>,
}

impl FakeShell {
    /// A machine with exactly `installed` on the `PATH`.
    fn new(installed: &[&str]) -> FakeShell {
        FakeShell {
            installed: installed.iter().map(|tool| (*tool).to_string()).collect(),
            commands: RecordingRunner::new(ActionOutcome::ok("ran")),
            probes: Mutex::new(Vec::new()),
        }
    }

    /// Every command the preset actually issued, in order.
    fn commands(&self) -> Vec<CommandCall> {
        self.commands.calls()
    }

    /// Every tool that was looked up, in order.
    fn probes(&self) -> Vec<String> {
        self.probes.lock().unwrap().clone()
    }

    fn answer(&self, program: &str, args: &[&str], detached: bool) -> ActionOutcome {
        if let Some(tool) = probe_target(program, args) {
            self.probes.lock().unwrap().push(tool.to_string());
            return if self.installed.iter().any(|name| name == tool) {
                ActionOutcome::ok(format!("/usr/bin/{tool}"))
            } else {
                ActionOutcome::fail(format!("which: no {tool} in (PATH)"))
            };
        }
        if detached {
            self.commands.spawn_detached(program, args)
        } else {
            self.commands.run(program, args)
        }
    }
}

impl CommandRunner for FakeShell {
    fn run(&self, program: &str, args: &[&str]) -> ActionOutcome {
        self.answer(program, args, false)
    }

    fn spawn_detached(&self, program: &str, args: &[&str]) -> ActionOutcome {
        self.answer(program, args, true)
    }
}

/// Records the media keys the preset asked the platform injector to send.
struct RecordingInjector {
    outcome: ActionOutcome,
    media: Mutex<Vec<String>>,
}

impl RecordingInjector {
    fn ok() -> RecordingInjector {
        RecordingInjector::with(ActionOutcome::ok("Media key PLAY_PAUSE sent"))
    }

    fn with(outcome: ActionOutcome) -> RecordingInjector {
        RecordingInjector {
            outcome,
            media: Mutex::new(Vec::new()),
        }
    }

    fn media(&self) -> Vec<String> {
        self.media.lock().unwrap().clone()
    }
}

impl InputInjector for RecordingInjector {
    fn available(&self) -> Option<String> {
        Some("sendinput".to_string())
    }

    fn shortcut(&self, _modifiers: &[String], _key: &str) -> ActionOutcome {
        ActionOutcome::fail("not used by a system preset")
    }

    fn text(&self, _text: &str) -> ActionOutcome {
        ActionOutcome::fail("not used by a system preset")
    }

    fn media(&self, media_key: &str) -> ActionOutcome {
        self.media.lock().unwrap().push(media_key.to_string());
        self.outcome.clone()
    }
}

fn ok_run(id: &str, shell: &FakeShell, injector: &RecordingInjector) -> ActionOutcome {
    let outcome = execute_system_preset(id, shell, injector);
    assert!(outcome.ok, "{id} should have succeeded: {outcome:?}");
    outcome
}

fn fail_run(id: &str, shell: &FakeShell, injector: &RecordingInjector) -> ActionOutcome {
    let outcome = execute_system_preset(id, shell, injector);
    assert!(!outcome.ok, "{id} should have failed: {outcome:?}");
    outcome
}

fn waited(program: &str, args: &[&str]) -> CommandCall {
    CommandCall::waited(program, args.iter().map(|a| (*a).to_string()))
}

fn detached(program: &str, args: &[&str]) -> CommandCall {
    CommandCall::detached(program, args.iter().map(|a| (*a).to_string()))
}

// ---------------------------------------------------------------------------
// The preset table
// ---------------------------------------------------------------------------

/// `(id, name, category, description)` — asserted as a table so a drift from
/// the Python apps fails the build instead of shipping.
const EXPECTED: [(&str, &str, &str, &str); 10] = [
    (
        "mic_toggle_mute",
        "Toggle Microphone Mute",
        "Audio",
        "Mute/Unmute the current microphone (Meeting/Discord)",
    ),
    (
        "volume_up_5",
        "Volume Up (+5%)",
        "Audio",
        "Raise the output volume by 5%",
    ),
    (
        "volume_down_5",
        "Volume Down (-5%)",
        "Audio",
        "Lower the output volume by 5%",
    ),
    (
        "volume_toggle_mute",
        "Toggle Speaker Mute",
        "Audio",
        "Mute/Unmute speaker audio output",
    ),
    (
        "lock_screen",
        "Lock Session",
        "System",
        "Lock the desktop session instantly",
    ),
    (
        "open_terminal",
        "Open New Terminal",
        "System",
        "Open a new terminal window",
    ),
    (
        "screenshot_interactive",
        "Screenshot (Area)",
        "System",
        "Take a screenshot of a selected screen area",
    ),
    (
        "media_play_pause",
        "Media Play / Pause",
        "Media",
        "Play or pause the active media player",
    ),
    (
        "media_next",
        "Media Next Track",
        "Media",
        "Skip to the next track",
    ),
    (
        "media_prev",
        "Media Previous Track",
        "Media",
        "Go back to the previous track",
    ),
];

#[test]
fn there_are_exactly_ten_presets_with_the_ids_both_python_apps_share() {
    let ids: Vec<&str> = SYSTEM_PRESETS.iter().map(|preset| preset.id).collect();
    let expected_ids: Vec<&str> = EXPECTED.iter().map(|(id, ..)| *id).collect();
    assert_eq!(ids, expected_ids, "preset ids drifted from the Python apps");
}

#[test]
fn every_preset_carries_the_four_ui_fields_of_the_python_tables() {
    assert_eq!(SYSTEM_PRESETS.len(), EXPECTED.len());
    for (preset, (id, name, category, description)) in SYSTEM_PRESETS.iter().zip(EXPECTED) {
        assert_eq!(preset.id, id);
        assert_eq!(preset.name, name);
        assert_eq!(preset.category, category);
        assert_eq!(preset.description, description);
    }
}

#[test]
fn no_label_or_description_names_an_operating_system() {
    // Parity checklist §7b decision 4: one shared UI, so the wording is
    // OS neutral ("Lock Session", not "Lock Ubuntu Session").
    for preset in SYSTEM_PRESETS {
        for text in [preset.name, preset.description] {
            for banned in ["Ubuntu", "Windows", "Linux", "Mac", "Win+L"] {
                assert!(
                    !text.contains(banned),
                    "{} still says {banned}: {text}",
                    preset.id
                );
            }
        }
    }
    let lock = preset_by_id("lock_screen").expect("lock_screen");
    assert_eq!(lock.name, "Lock Session");
    let shot = preset_by_id("screenshot_interactive").expect("screenshot_interactive");
    assert_eq!(shot.name, "Screenshot (Area)");
}

#[test]
fn presets_can_be_looked_up_by_id() {
    for (id, ..) in EXPECTED {
        let found = preset_by_id(id).unwrap_or_else(|| panic!("{id} is not registered"));
        assert_eq!(found.id, id);
    }
    assert!(preset_by_id("does_not_exist").is_none());
}

// ---------------------------------------------------------------------------
// Microphone mute
// ---------------------------------------------------------------------------

#[test]
fn mic_toggle_mute_prefers_wpctl() {
    let shell = FakeShell::new(&["wpctl", "pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("mic_toggle_mute", &shell, &injector);
    assert_eq!(outcome.reason, "Toggled mic mute via wpctl");
    assert_eq!(shell.probes(), ["wpctl"]);
    assert_eq!(
        shell.commands(),
        [waited(
            "wpctl",
            &["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]
        )]
    );
}

#[test]
fn mic_toggle_mute_falls_back_to_pactl_without_wpctl() {
    let shell = FakeShell::new(&["pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("mic_toggle_mute", &shell, &injector);
    assert_eq!(outcome.reason, "Toggled mic mute via pactl");
    assert_eq!(
        shell.probes(),
        ["wpctl", "pactl"],
        "pactl is only probed once wpctl is missing"
    );
    assert_eq!(
        shell.commands(),
        [waited(
            "pactl",
            &["set-source-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]
        )]
    );
}

#[test]
fn mic_toggle_mute_ends_at_the_alsa_fallback() {
    let shell = FakeShell::new(&[]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("mic_toggle_mute", &shell, &injector);
    assert_eq!(outcome.reason, "Toggled mic mute via amixer");
    assert_eq!(
        shell.commands(),
        [waited("amixer", &["set", "Capture", "toggle"])]
    );
}

// ---------------------------------------------------------------------------
// Volume +5% / -5%
// ---------------------------------------------------------------------------

#[test]
fn volume_up_5_uses_wpctl() {
    let shell = FakeShell::new(&["wpctl", "pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("volume_up_5", &shell, &injector);
    assert_eq!(outcome.reason, "Volume +5% via wpctl");
    assert_eq!(shell.probes(), ["wpctl"]);
    assert_eq!(
        shell.commands(),
        [waited(
            "wpctl",
            &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%+"]
        )]
    );
}

#[test]
fn volume_down_5_uses_wpctl() {
    let shell = FakeShell::new(&["wpctl", "pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("volume_down_5", &shell, &injector);
    assert_eq!(outcome.reason, "Volume -5% via wpctl");
    assert_eq!(shell.probes(), ["wpctl"]);
    assert_eq!(
        shell.commands(),
        [waited(
            "wpctl",
            &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"]
        )]
    );
}

#[test]
fn volume_up_5_falls_back_to_pactl_without_wpctl() {
    let shell = FakeShell::new(&["pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("volume_up_5", &shell, &injector);
    assert_eq!(outcome.reason, "Volume +5% via pactl");
    assert_eq!(shell.probes(), ["wpctl", "pactl"]);
    assert_eq!(
        shell.commands(),
        [waited(
            "pactl",
            &["set-sink-volume", "@DEFAULT_SINK@", "+5%"]
        )]
    );
}

#[test]
fn volume_down_5_falls_back_to_pactl_without_wpctl() {
    let shell = FakeShell::new(&["pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("volume_down_5", &shell, &injector);
    assert_eq!(outcome.reason, "Volume -5% via pactl");
    assert_eq!(
        shell.commands(),
        [waited(
            "pactl",
            &["set-sink-volume", "@DEFAULT_SINK@", "-5%"]
        )]
    );
}

#[test]
fn volume_presets_fail_naming_what_is_missing() {
    for id in ["volume_up_5", "volume_down_5", "volume_toggle_mute"] {
        let shell = FakeShell::new(&[]);
        let injector = RecordingInjector::ok();
        let outcome = fail_run(id, &shell, &injector);
        assert_eq!(
            outcome.reason, "Volume control requires wpctl or pactl (pipewire/pulseaudio).",
            "the failure must name the missing tools for {id}"
        );
        assert_eq!(shell.probes(), ["wpctl", "pactl"], "for {id}");
        assert!(shell.commands().is_empty(), "for {id}");
    }
}

// ---------------------------------------------------------------------------
// Speaker mute
// ---------------------------------------------------------------------------

#[test]
fn volume_toggle_mute_uses_wpctl() {
    let shell = FakeShell::new(&["wpctl", "pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("volume_toggle_mute", &shell, &injector);
    assert_eq!(outcome.reason, "Toggled speaker mute via wpctl");
    assert_eq!(shell.probes(), ["wpctl"]);
    assert_eq!(
        shell.commands(),
        [waited(
            "wpctl",
            &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]
        )]
    );
}

#[test]
fn volume_toggle_mute_falls_back_to_pactl_without_wpctl() {
    let shell = FakeShell::new(&["pactl"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("volume_toggle_mute", &shell, &injector);
    assert_eq!(outcome.reason, "Toggled speaker mute via pactl");
    assert_eq!(
        shell.commands(),
        [waited(
            "pactl",
            &["set-sink-mute", "@DEFAULT_SINK@", "toggle"]
        )]
    );
}

// ---------------------------------------------------------------------------
// A failing tool is reported, never swallowed
// ---------------------------------------------------------------------------

#[test]
fn a_failing_command_is_reported_instead_of_a_fake_success() {
    let shell = FakeShell {
        installed: vec!["wpctl".to_string()],
        commands: RecordingRunner::new(ActionOutcome::fail("Connection refused: pactl")),
        probes: Mutex::new(Vec::new()),
    };
    let injector = RecordingInjector::ok();
    let outcome = fail_run("volume_up_5", &shell, &injector);
    assert_eq!(outcome.reason, "Connection refused: pactl");
}

// ---------------------------------------------------------------------------
// Lock session
// ---------------------------------------------------------------------------

#[test]
fn lock_screen_uses_loginctl() {
    let shell = FakeShell::new(&["loginctl", "xdg-screensaver"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("lock_screen", &shell, &injector);
    assert_eq!(outcome.reason, "Locked screen via loginctl");
    assert_eq!(shell.commands(), [waited("loginctl", &["lock-session"])]);
}

#[test]
fn lock_screen_falls_back_to_xdg_screensaver() {
    let shell = FakeShell::new(&["xdg-screensaver"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("lock_screen", &shell, &injector);
    assert_eq!(outcome.reason, "Locked screen via xdg-screensaver");
    assert_eq!(shell.probes(), ["loginctl", "xdg-screensaver"]);
    assert_eq!(shell.commands(), [waited("xdg-screensaver", &["lock"])]);
}

#[test]
fn lock_screen_fails_naming_what_is_missing() {
    let shell = FakeShell::new(&[]);
    let injector = RecordingInjector::ok();
    let outcome = fail_run("lock_screen", &shell, &injector);
    assert_eq!(
        outcome.reason,
        "Screen lock requires loginctl or xdg-screensaver."
    );
    assert!(shell.commands().is_empty());
}

// ---------------------------------------------------------------------------
// Open terminal
// ---------------------------------------------------------------------------

#[test]
fn open_terminal_takes_the_first_installed_emulator() {
    let shell = FakeShell::new(&["alacritty", "x-terminal-emulator"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("open_terminal", &shell, &injector);
    assert_eq!(outcome.reason, "Opened terminal: x-terminal-emulator");
    assert_eq!(
        shell.commands(),
        [detached("x-terminal-emulator", &[])],
        "the window must outlive the press (start_new_session)"
    );
}

#[test]
fn open_terminal_prefers_the_gnome_one_when_installed() {
    let shell = FakeShell::new(&["gnome-terminal", "alacritty", "x-terminal-emulator"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("open_terminal", &shell, &injector);
    assert_eq!(outcome.reason, "Opened terminal: gnome-terminal");
    assert_eq!(
        shell.probes(),
        ["gnome-terminal"],
        "the preference order stops at the first hit"
    );
}

#[test]
fn open_terminal_fails_naming_what_is_missing() {
    let shell = FakeShell::new(&[]);
    let injector = RecordingInjector::ok();
    let outcome = fail_run("open_terminal", &shell, &injector);
    assert!(
        outcome.reason.contains("gnome-terminal") && outcome.reason.contains("xfce4-terminal"),
        "the failure must list the emulators it looked for: {}",
        outcome.reason
    );
    assert!(shell.commands().is_empty());
}

// ---------------------------------------------------------------------------
// Screenshot
// ---------------------------------------------------------------------------

#[test]
fn screenshot_prefers_gnome_screenshot() {
    let shell = FakeShell::new(&["gnome-screenshot", "grim", "spectacle", "notify-send"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("screenshot_interactive", &shell, &injector);
    assert_eq!(
        outcome.reason,
        "Triggered interactive screenshot via gnome-screenshot"
    );
    assert_eq!(shell.commands(), [detached("gnome-screenshot", &["-a"])]);
}

#[test]
fn screenshot_falls_back_to_grim_then_spectacle() {
    let grim = FakeShell::new(&["grim", "spectacle"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("screenshot_interactive", &grim, &injector);
    assert_eq!(outcome.reason, "Triggered interactive screenshot via grim");
    assert_eq!(grim.commands(), [detached("grim", &["-g"])]);

    let spectacle = FakeShell::new(&["spectacle"]);
    let outcome = ok_run("screenshot_interactive", &spectacle, &injector);
    assert_eq!(
        outcome.reason,
        "Triggered interactive screenshot via spectacle"
    );
    assert_eq!(spectacle.commands(), [detached("spectacle", &["-a"])]);
}

#[test]
fn screenshot_ends_with_a_notification_and_then_fails_loudly() {
    let notify = FakeShell::new(&["notify-send"]);
    let injector = RecordingInjector::ok();
    let outcome = ok_run("screenshot_interactive", &notify, &injector);
    assert_eq!(outcome.reason, "Screenshot requested");
    assert_eq!(
        notify.commands(),
        [detached(
            "notify-send",
            &["OctoPad", "Use PrintScreen to take a screenshot"]
        )]
    );

    let nothing = FakeShell::new(&[]);
    let outcome = fail_run("screenshot_interactive", &nothing, &injector);
    assert_eq!(
        outcome.reason,
        "Screenshot requires gnome-screenshot, grim or spectacle."
    );
    assert!(nothing.commands().is_empty());
}

// ---------------------------------------------------------------------------
// Media presets
// ---------------------------------------------------------------------------

#[test]
fn media_presets_delegate_to_the_injector_with_the_platform_keys() {
    let shell = FakeShell::new(&[]);
    let injector = RecordingInjector::ok();
    for (id, key) in [
        ("media_play_pause", "PLAY_PAUSE"),
        ("media_next", "NEXT_TRACK"),
        ("media_prev", "PREV_TRACK"),
    ] {
        ok_run(id, &shell, &injector);
        assert_eq!(
            injector.media().last().map(String::as_str),
            Some(key),
            "for {id}"
        );
    }
    assert_eq!(injector.media(), ["PLAY_PAUSE", "NEXT_TRACK", "PREV_TRACK"]);
    assert!(
        shell.commands().is_empty(),
        "media is the injector's job, not a subprocess"
    );
    assert!(shell.probes().is_empty());
}

#[test]
fn media_presets_report_an_injector_failure() {
    let shell = FakeShell::new(&[]);
    let injector = RecordingInjector::with(ActionOutcome::fail(
        "The music player requires playerctl (sudo apt install playerctl).",
    ));
    let outcome = fail_run("media_play_pause", &shell, &injector);
    assert_eq!(
        outcome.reason,
        "The music player requires playerctl (sudo apt install playerctl)."
    );
}

// ---------------------------------------------------------------------------
// Unknown ids and the dispatcher wiring
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_preset_id_fails_with_the_dispatcher_message() {
    for id in ["does_not_exist", "", "volume_up_10"] {
        let shell = FakeShell::new(&["wpctl"]);
        let injector = RecordingInjector::ok();
        let outcome = fail_run(id, &shell, &injector);
        assert_eq!(outcome.reason, format!("Unknown system action: {id}"));
        assert!(
            shell.commands().is_empty(),
            "an unknown id must not run anything"
        );
    }
}

#[test]
fn the_system_action_arm_routes_through_the_presets() {
    let shell = FakeShell::new(&["loginctl"]);
    let injector = RecordingInjector::ok();
    let deps = ExecDeps::new(&shell, &injector);
    let action: Action =
        serde_json::from_value(json!({ "type": "system_action", "presetId": "lock_screen" }))
            .expect("action shape");
    let outcome = execute_action(&action, &deps);
    assert!(outcome.ok, "{outcome:?}");
    assert_eq!(outcome.reason, "Locked screen via loginctl");
    assert_eq!(shell.commands(), [waited("loginctl", &["lock-session"])]);
}

#[test]
fn the_system_action_arm_keeps_the_unknown_id_message() {
    let shell = FakeShell::new(&[]);
    let injector = RecordingInjector::ok();
    let deps = ExecDeps::new(&shell, &injector);
    for (payload, expected) in [
        (
            json!({ "type": "system_action", "presetId": "nope" }),
            "Unknown system action: nope",
        ),
        (
            json!({ "type": "system_action" }),
            "Unknown system action: ",
        ),
    ] {
        let action: Action = serde_json::from_value(payload.clone()).expect("action shape");
        let outcome = execute_action(&action, &deps);
        assert!(!outcome.ok, "{payload}");
        assert_eq!(outcome.reason, expected, "for {payload}");
    }
}

#[test]
fn a_malformed_preset_id_is_an_execution_error() {
    let shell = FakeShell::new(&[]);
    let injector = RecordingInjector::ok();
    let deps = ExecDeps::new(&shell, &injector);
    let payload: Value = json!({ "type": "system_action", "presetId": ["volume_up_5"] });
    let action: Action = serde_json::from_value(payload).expect("action shape");
    let outcome = execute_action(&action, &deps);
    assert!(!outcome.ok);
    assert_eq!(
        outcome.reason,
        "Action execution error: presetId must be a string, got array"
    );
}

#[test]
fn every_preset_is_reachable_through_execute_system_preset() {
    // Smoke: no preset may panic, whatever the machine looks like. The real
    // dispatcher wraps this in `catch_unwind` anyway.
    for machines in [
        vec![
            "wpctl",
            "pactl",
            "loginctl",
            "gnome-terminal",
            "gnome-screenshot",
            "amixer",
        ],
        vec![
            "pactl",
            "xdg-screensaver",
            "x-terminal-emulator",
            "notify-send",
        ],
        vec![],
    ] {
        let shell = FakeShell::new(&machines);
        let injector = RecordingInjector::ok();
        for preset in SYSTEM_PRESETS {
            let outcome = execute_system_preset(preset.id, &shell, &injector);
            assert!(!outcome.reason.is_empty(), "{} said nothing", preset.id);
            match preset.id {
                // These three always have a tool, even a bare machine.
                "mic_toggle_mute" | "media_play_pause" | "media_next" | "media_prev" => {
                    assert!(outcome.ok, "{} on {machines:?}: {outcome:?}", preset.id)
                }
                _ => assert!(!outcome.reason.is_empty()),
            }
        }
    }
}
