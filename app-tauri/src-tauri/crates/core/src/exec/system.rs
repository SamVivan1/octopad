//! The ten built-in system presets (M3.1 Task 3).
//!
//! Port of `linux-app/system_actions.py:16` (`SYSTEM_ACTION_PRESETS`) and `:465`
//! (`_execute_system_preset`), with the ids kept identical to
//! `windows-app/platform_actions.py:38` so a profile stays portable between the
//! two OSes. The labels are the OS-neutral wording of parity checklist §7b
//! decision 4 ("Lock Session", "Screenshot (Area)").
//!
//! Three rules this module keeps:
//!
//! * Every command — including the `which <tool>` availability probes of
//!   Python's `shutil.which` — goes through the injected [`CommandRunner`], so
//!   the tests spawn nothing and can script an empty `PATH`.
//! * Media keys go through the injected [`InputInjector`]: the Linux injector
//!   plays them with `playerctl`/XF86, the Windows one with `SendInput`, and
//!   only this module knows that `media_next` means `NEXT_TRACK`.
//! * A missing tool is a visible failure that names what is missing. Python
//!   returns `Unknown preset: …` (or `True, "Screenshot requested"` for a
//!   `notify-send` that may not exist) when a machine has no audio tool at all;
//!   that is a silent lie, so each branch says what is missing instead.

use super::{ActionOutcome, CommandRunner, InputInjector};

/// One row of the preset table, as the Profiles page renders it.
///
/// `id` is the contract with the saved configs and with the other OS app;
/// `name`/`description` are presentation and must stay OS neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
}

/// The ten presets, in the order both Python apps list them.
pub static SYSTEM_PRESETS: &[SystemPreset] = &[
    SystemPreset {
        id: "mic_toggle_mute",
        name: "Toggle Microphone Mute",
        category: "Audio",
        description: "Mute/Unmute the current microphone (Meeting/Discord)",
    },
    SystemPreset {
        id: "volume_up_5",
        name: "Volume Up (+5%)",
        category: "Audio",
        description: "Raise the output volume by 5%",
    },
    SystemPreset {
        id: "volume_down_5",
        name: "Volume Down (-5%)",
        category: "Audio",
        description: "Lower the output volume by 5%",
    },
    SystemPreset {
        id: "volume_toggle_mute",
        name: "Toggle Speaker Mute",
        category: "Audio",
        description: "Mute/Unmute speaker audio output",
    },
    SystemPreset {
        id: "lock_screen",
        name: "Lock Session",
        category: "System",
        description: "Lock the desktop session instantly",
    },
    SystemPreset {
        id: "open_terminal",
        name: "Open New Terminal",
        category: "System",
        description: "Open a new terminal window",
    },
    SystemPreset {
        id: "screenshot_interactive",
        name: "Screenshot (Area)",
        category: "System",
        description: "Take a screenshot of a selected screen area",
    },
    SystemPreset {
        id: "media_play_pause",
        name: "Media Play / Pause",
        category: "Media",
        description: "Play or pause the active media player",
    },
    SystemPreset {
        id: "media_next",
        name: "Media Next Track",
        category: "Media",
        description: "Skip to the next track",
    },
    SystemPreset {
        id: "media_prev",
        name: "Media Previous Track",
        category: "Media",
        description: "Go back to the previous track",
    },
];

/// Look a preset up by id (`None` for an unknown or malformed id).
pub fn preset_by_id(id: &str) -> Option<&'static SystemPreset> {
    SYSTEM_PRESETS.iter().find(|preset| preset.id == id)
}

/// Terminal emulators in preference order (`system_actions.py:521`).
pub const TERMINAL_CANDIDATES: &[&str] = &[
    "gnome-terminal",
    "x-terminal-emulator",
    "alacritty",
    "kitty",
    "konsole",
    "xfce4-terminal",
];

/// The three area-screenshot tools, in preference order. Python only knew
/// `gnome-screenshot`; `grim` (Wayland) and `spectacle` (KDE) are the fallbacks
/// the plan adds. Each one takes its area flag as the only argument.
const SCREENSHOT_CANDIDATES: &[(&str, &str)] = &[
    ("gnome-screenshot", "-a"),
    ("grim", "-g"),
    ("spectacle", "-a"),
];

/// Last resort of `screenshot_interactive` (`system_actions.py:534`): there is
/// no screenshot tool, so at least tell the user what to press.
const SCREENSHOT_HINT_SUMMARY: &str = "OctoPad";
const SCREENSHOT_HINT_BODY: &str = "Use PrintScreen to take a screenshot";

/// Failures for the three audio presets: same text, one wording.
///
/// Python returns `Unknown preset: volume_up_5` when neither `wpctl` nor
/// `pactl` is installed, which hides a missing dependency behind a wrong
/// message; `_send_media` gets closer with "Volume requires wpctl or pactl
/// (pipewire/pulseaudio).", so the presets name both tools explicitly.
const NO_AUDIO_TOOL: &str = "Volume control requires wpctl or pactl (pipewire/pulseaudio).";

/// Execute one preset by id.
///
/// Unknown ids fail with the dispatcher's `Unknown system action: {id}` so an
/// id deleted from a future table is visible instead of silently ignored.
pub fn execute_system_preset(
    id: &str,
    runner: &dyn CommandRunner,
    input: &dyn InputInjector,
) -> ActionOutcome {
    match id {
        "mic_toggle_mute" => mic_toggle_mute(runner),
        "volume_up_5" => volume_step(runner, VolumeStep::Up),
        "volume_down_5" => volume_step(runner, VolumeStep::Down),
        "volume_toggle_mute" => speaker_mute(runner),
        "lock_screen" => lock_session(runner),
        "open_terminal" => open_terminal(runner),
        "screenshot_interactive" => screenshot(runner),
        // The injector owns the tool order (playerctl then XF86 on Linux,
        // SendInput on Windows); this table only names the key.
        "media_play_pause" => input.media("PLAY_PAUSE"),
        "media_next" => input.media("NEXT_TRACK"),
        "media_prev" => input.media("PREV_TRACK"),
        other => ActionOutcome::fail(format!("Unknown system action: {other}")),
    }
}

enum VolumeStep {
    Up,
    Down,
}

impl VolumeStep {
    /// `5%+` for wpctl, `+5%` for pactl — the two CLIs spell the same step
    /// differently.
    fn wpctl_step(&self) -> &'static str {
        match self {
            VolumeStep::Up => "5%+",
            VolumeStep::Down => "5%-",
        }
    }

    fn pactl_step(&self) -> &'static str {
        match self {
            VolumeStep::Up => "+5%",
            VolumeStep::Down => "-5%",
        }
    }

    fn sign(&self) -> &'static str {
        match self {
            VolumeStep::Up => "+",
            VolumeStep::Down => "-",
        }
    }
}

/// `system_actions.py:472` — wpctl, else pactl, else plain ALSA.
///
/// The aliases are **tool-specific**, and mixing them up fails silently:
///
/// ```text
/// $ pactl get-source-mute @DEFAULT_SOURCE@        # -> Mute: yes
/// $ pactl get-source-mute @DEFAULT_AUDIO_SOURCE@  # -> No such entity
/// $ wpctl  get-volume @DEFAULT_AUDIO_SINK@        # -> Volume: 0.00 [MUTED]
/// ```
///
/// `wpctl` understands the long `@DEFAULT_AUDIO_*` forms; `pactl` only knows the
/// short `@DEFAULT_SOURCE@` / `@DEFAULT_SINK@` ones. So the wpctl branch uses the
/// long alias and the pactl branch the short one — the pairing `system_actions.py`
/// uses. Verified on this machine against PulseAudio on PipeWire 1.6.8.
fn mic_toggle_mute(runner: &dyn CommandRunner) -> ActionOutcome {
    if installed(runner, "wpctl") {
        return run_tool(
            runner,
            "wpctl",
            &["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"],
            "Toggled mic mute via wpctl",
        );
    }
    if installed(runner, "pactl") {
        return run_tool(
            runner,
            "pactl",
            &["set-source-mute", "@DEFAULT_SOURCE@", "toggle"],
            "Toggled mic mute via pactl",
        );
    }
    // ALSA needs no daemon and is present on every machine with a card, so
    // this branch always runs something (`check=False` in Python).
    run_tool(
        runner,
        "amixer",
        &["set", "Capture", "toggle"],
        "Toggled mic mute via amixer",
    )
}

/// `system_actions.py:493` / `:503` — `wpctl set-volume … 5%+`, else pactl.
fn volume_step(runner: &dyn CommandRunner, step: VolumeStep) -> ActionOutcome {
    let reason = format!("Volume {}5% via ", step.sign());
    if installed(runner, "wpctl") {
        return run_tool(
            runner,
            "wpctl",
            &["set-volume", "@DEFAULT_AUDIO_SINK@", step.wpctl_step()],
            &format!("{reason}wpctl"),
        );
    }
    if installed(runner, "pactl") {
        return run_tool(
            runner,
            "pactl",
            &["set-sink-volume", "@DEFAULT_SINK@", step.pactl_step()],
            &format!("{reason}pactl"),
        );
    }
    ActionOutcome::fail(NO_AUDIO_TOOL)
}

/// `system_actions.py:484` — `wpctl set-mute …`, else pactl, else the failure
/// both branches of Python fall into.
fn speaker_mute(runner: &dyn CommandRunner) -> ActionOutcome {
    if installed(runner, "wpctl") {
        return run_tool(
            runner,
            "wpctl",
            &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
            "Toggled speaker mute via wpctl",
        );
    }
    if installed(runner, "pactl") {
        return run_tool(
            runner,
            "pactl",
            &["set-sink-mute", "@DEFAULT_SINK@", "toggle"],
            "Toggled speaker mute via pactl",
        );
    }
    ActionOutcome::fail(NO_AUDIO_TOOL)
}

/// `system_actions.py:511` — `loginctl`, else `xdg-screensaver`.
fn lock_session(runner: &dyn CommandRunner) -> ActionOutcome {
    if installed(runner, "loginctl") {
        return run_tool(
            runner,
            "loginctl",
            &["lock-session"],
            "Locked screen via loginctl",
        );
    }
    if installed(runner, "xdg-screensaver") {
        return run_tool(
            runner,
            "xdg-screensaver",
            &["lock"],
            "Locked screen via xdg-screensaver",
        );
    }
    ActionOutcome::fail("Screen lock requires loginctl or xdg-screensaver.")
}

/// `system_actions.py:520` — first emulator on the `PATH`, started detached so
/// the new window outlives the press.
fn open_terminal(runner: &dyn CommandRunner) -> ActionOutcome {
    match first_installed(runner, TERMINAL_CANDIDATES) {
        Some(terminal) => spawn_tool(
            runner,
            terminal,
            &[],
            &format!("Opened terminal: {terminal}"),
        ),
        None => ActionOutcome::fail(format!(
            "No terminal emulator found ({}).",
            TERMINAL_CANDIDATES.join(", ")
        )),
    }
}

/// `system_actions.py:528` — `gnome-screenshot -a`, then `grim -g` /
/// `spectacle -a`, then the `notify-send` hint.
fn screenshot(runner: &dyn CommandRunner) -> ActionOutcome {
    for (tool, area_flag) in SCREENSHOT_CANDIDATES {
        if installed(runner, tool) {
            return spawn_tool(
                runner,
                tool,
                &[area_flag],
                &format!("Triggered interactive screenshot via {tool}"),
            );
        }
    }
    if installed(runner, "notify-send") {
        return spawn_tool(
            runner,
            "notify-send",
            &[SCREENSHOT_HINT_SUMMARY, SCREENSHOT_HINT_BODY],
            "Screenshot requested",
        );
    }
    ActionOutcome::fail("Screenshot requires gnome-screenshot, grim or spectacle.")
}

/// `shutil.which(tool)` — probed through the runner, never through the process
/// environment, so a test can script an empty `PATH`.
fn installed(runner: &dyn CommandRunner, tool: &str) -> bool {
    runner.run("which", &[tool]).ok
}

/// First candidate on the `PATH`, probing in order and stopping at the first
/// hit (so a present first choice is never preceded by a second run).
fn first_installed<'a>(runner: &dyn CommandRunner, candidates: &'a [&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .find(|tool| installed(runner, tool))
}

/// Run to completion; a failing command keeps its own message instead of being
/// reported as the preset's success string.
///
/// Python passes `check=False` and returns `True` regardless, so a broken
/// `wpctl` still printed "Toggled speaker mute via wpctl"; the console would
/// claim work that never happened.
fn run_tool(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[&str],
    reason: &str,
) -> ActionOutcome {
    let outcome = runner.run(program, args);
    if outcome.ok {
        ActionOutcome::ok(reason)
    } else {
        outcome
    }
}

/// Start and do not wait (`start_new_session=True` in Python).
fn spawn_tool(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[&str],
    reason: &str,
) -> ActionOutcome {
    let outcome = runner.spawn_detached(program, args);
    if outcome.ok {
        ActionOutcome::ok(reason)
    } else {
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{CommandCall, RecordingRunner};

    fn ids() -> Vec<&'static str> {
        SYSTEM_PRESETS.iter().map(|preset| preset.id).collect()
    }

    #[test]
    fn the_table_is_the_ten_ids_of_both_python_apps_in_order() {
        assert_eq!(
            ids(),
            vec![
                "mic_toggle_mute",
                "volume_up_5",
                "volume_down_5",
                "volume_toggle_mute",
                "lock_screen",
                "open_terminal",
                "screenshot_interactive",
                "media_play_pause",
                "media_next",
                "media_prev",
            ]
        );
    }

    #[test]
    fn ids_are_unique_and_never_empty() {
        for preset in SYSTEM_PRESETS {
            assert!(!preset.id.is_empty());
            assert!(!preset.name.is_empty());
            assert!(!preset.category.is_empty());
            assert!(!preset.description.is_empty());
            assert_eq!(ids().iter().filter(|id| **id == preset.id).count(), 1);
        }
    }

    #[test]
    fn an_unknown_id_is_reported_verbatim() {
        let runner = RecordingRunner::new(ActionOutcome::ok("ran"));
        let outcome = execute_system_preset("volume_up_10", &runner, &crate::exec::NoInputInjector);
        assert_eq!(
            outcome,
            ActionOutcome::fail("Unknown system action: volume_up_10")
        );
        assert_eq!(runner.call_count(), 0, "an unknown id runs nothing");
    }

    #[test]
    fn first_installed_stops_at_the_first_hit() {
        // `which` is never found here, so every probe fails: the helper must
        // walk the whole list and return `None` rather than panic.
        let runner = RecordingRunner::new(ActionOutcome::fail("which: no such tool"));
        assert_eq!(first_installed(&runner, TERMINAL_CANDIDATES), None);
        assert_eq!(runner.call_count(), TERMINAL_CANDIDATES.len());
    }

    #[test]
    fn the_notify_send_hint_keeps_the_python_summary_and_body() {
        assert_eq!(SCREENSHOT_HINT_SUMMARY, "OctoPad");
        assert!(SCREENSHOT_HINT_BODY.contains("PrintScreen"));
    }

    #[test]
    fn a_failing_tool_is_not_reported_as_the_presets_success() {
        let runner = RecordingRunner::new(ActionOutcome::fail("pactl: no connection"));
        let outcome = lock_session(&runner);
        // `which loginctl` fails (the scripted failure), so both lock tools are
        // missing and nothing is launched.
        assert_eq!(
            outcome,
            ActionOutcome::fail("Screen lock requires loginctl or xdg-screensaver.")
        );
        assert!(runner
            .calls()
            .iter()
            .all(|call: &CommandCall| call.program == "which"));
    }
}
