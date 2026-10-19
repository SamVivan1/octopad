//! Task 2 (M3.1): Linux keyboard/text/media injection.
//!
//! Port of `linux-app/system_actions.py:92-253`: `_injector`, `_injector_help`,
//! `_xkey`, `_evdev_key`, `_ydotool_combo`, `_send_shortcut`, `_send_text`,
//! `_send_media`, `MODIFIER_ORDER` and `MEDIA_XF86`.
//!
//! Every argv asserted here is byte-for-byte the argv the Python builds, and
//! every reason string is the Python one. No process is spawned: the injector
//! probes and runs through an injected [`CommandRunner`] double that models
//! `shutil.which`.

#![cfg(target_os = "linux")]

use std::sync::{Arc, Mutex};

use octopad_core::exec::{
    ActionOutcome, CommandCall, CommandRunner, InputInjector, InputTool, LinuxInjector,
    NoInputInjector, RecordingRunner, INPUT_INJECTOR_HELP,
};

/// `system_actions.py:99` (`_injector_help`), character for character.
const PYTHON_HELP: &str = concat!(
    "Keyboard simulation requires one of: `wtype` (Wayland, ",
    "sudo apt install wtype), `ydotool` (recommended on GNOME Wayland: ",
    "sudo apt install ydotool then run the daemon `sudo ydotoold` ",
    "or `systemctl --user start ydotool`), or `xdotool` (X11)."
);

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// Models `shutil.which`: `which <tool>` succeeds only for installed tools.
/// Everything else is recorded and reported as a plain success.
#[derive(Clone, Default)]
struct FakeTools {
    installed: Vec<String>,
    command_outcome: Option<ActionOutcome>,
    calls: Arc<Mutex<Vec<CommandCall>>>,
}

impl FakeTools {
    fn with(installed: &[&str]) -> FakeTools {
        FakeTools {
            installed: installed.iter().map(|tool| tool.to_string()).collect(),
            command_outcome: None,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn failing(installed: &[&str], reason: &str) -> FakeTools {
        let mut tools = FakeTools::with(installed);
        tools.command_outcome = Some(ActionOutcome::fail(reason.to_string()));
        tools
    }

    /// The calls that actually simulate something (`which` probes excluded).
    fn runs(&self) -> Vec<CommandCall> {
        self.calls()
            .into_iter()
            .filter(|call| call.program != "which")
            .collect()
    }

    fn calls(&self) -> Vec<CommandCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for FakeTools {
    fn run(&self, program: &str, args: &[&str]) -> ActionOutcome {
        self.calls.lock().unwrap().push(CommandCall::waited(
            program,
            args.iter().map(|arg| arg.to_string()),
        ));
        if program == "which" {
            let tool = args.first().copied().unwrap_or("");
            return if self.installed.iter().any(|name| name == tool) {
                ActionOutcome::ok(format!("/usr/bin/{tool}"))
            } else {
                ActionOutcome::fail(format!("{tool} not found"))
            };
        }
        match &self.command_outcome {
            Some(outcome) => outcome.clone(),
            None => ActionOutcome::ok(format!("{program} ran")),
        }
    }

    fn spawn_detached(&self, program: &str, args: &[&str]) -> ActionOutcome {
        self.calls.lock().unwrap().push(CommandCall::detached(
            program,
            args.iter().map(|arg| arg.to_string()),
        ));
        self.command_outcome
            .clone()
            .unwrap_or_else(|| ActionOutcome::ok(format!("{program} ran")))
    }
}

fn mods(list: &[&str]) -> Vec<String> {
    list.iter().map(|mod_name| mod_name.to_string()).collect()
}

/// An injector bound to `tool`, skipping detection (the tool matrix itself is
/// covered by the detection tests below).
fn injector(tool: InputTool, runner: &FakeTools) -> LinuxInjector<'_> {
    LinuxInjector::with_tool(tool, runner)
}

/// Assert a single recorded run: program plus exact argv.
fn assert_run(call: &CommandCall, program: &str, args: &[&str]) {
    assert_eq!(call.program, program, "wrong program in {call:?}");
    assert_eq!(
        call.args,
        args.iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<String>>(),
        "wrong argv in {call:?}"
    );
    assert!(!call.detached, "injection must wait for the tool: {call:?}");
}

// ---------------------------------------------------------------------------
// Tool detection: wtype -> ydotool -> xdotool
// ---------------------------------------------------------------------------

#[test]
fn wtype_wins_when_every_tool_is_installed() {
    let tools = FakeTools::with(&["wtype", "ydotool", "xdotool"]);
    let injector = LinuxInjector::detect(&tools).expect("wtype is installed");
    assert_eq!(injector.tool_name(), "wtype");
    assert_eq!(injector.available().as_deref(), Some("wtype"));
    assert_eq!(
        tools.runs(),
        Vec::new(),
        "detection must not run an injection"
    );
    // `shutil.which("wtype")` succeeds, so nothing else is probed.
    assert_eq!(
        tools.calls(),
        vec![CommandCall::waited("which", vec!["wtype"])]
    );
}

#[test]
fn ydotool_wins_when_it_is_the_only_tool() {
    let tools = FakeTools::with(&["ydotool"]);
    let injector = LinuxInjector::detect(&tools).expect("ydotool is installed");
    assert_eq!(injector.tool_name(), "ydotool");
    assert_eq!(
        tools.calls(),
        vec![
            CommandCall::waited("which", vec!["wtype"]),
            CommandCall::waited("which", vec!["ydotool"]),
        ]
    );
}

#[test]
fn xdotool_is_the_last_resort() {
    let tools = FakeTools::with(&["xdotool"]);
    let injector = LinuxInjector::detect(&tools).expect("xdotool is installed");
    assert_eq!(injector.tool_name(), "xdotool");
    assert_eq!(
        tools.calls(),
        vec![
            CommandCall::waited("which", vec!["wtype"]),
            CommandCall::waited("which", vec!["ydotool"]),
            CommandCall::waited("which", vec!["xdotool"]),
        ]
    );
}

#[test]
fn detection_is_driven_by_the_injected_runner_not_by_the_filesystem() {
    // The scripted runner fails `which wtype`, succeeds on `which ydotool`:
    // the real filesystem (this machine has no ydotool) must not matter.
    let runner = RecordingRunner::scripted(vec![
        ActionOutcome::fail("wtype not found"),
        ActionOutcome::ok("/usr/bin/ydotool"),
    ]);
    let injector = LinuxInjector::detect(&runner).expect("the scripted runner says ydotool");
    assert_eq!(injector.tool_name(), "ydotool");
    assert_eq!(runner.call_count(), 2);
    assert_eq!(
        runner.calls()[0],
        CommandCall::waited("which", vec!["wtype"])
    );
    assert_eq!(
        runner.calls()[1],
        CommandCall::waited("which", vec!["ydotool"])
    );
}

#[test]
fn no_tool_installed_falls_back_to_no_input_injector_and_the_python_help() {
    let tools = FakeTools::with(&[]);
    assert!(
        LinuxInjector::detect(&tools).is_none(),
        "no tool installed must not pick one"
    );
    assert_eq!(
        tools.calls(),
        vec![
            CommandCall::waited("which", vec!["wtype"]),
            CommandCall::waited("which", vec!["ydotool"]),
            CommandCall::waited("which", vec!["xdotool"]),
        ]
    );

    // The dispatcher then uses `NoInputInjector`, which reports the help text
    // instead of pretending the keys were sent.
    let none = NoInputInjector;
    assert!(none.available().is_none());
    assert_eq!(
        none.shortcut(&mods(&["ctrl"]), "t").reason,
        INPUT_INJECTOR_HELP
    );
    assert_eq!(none.text("hello").reason, INPUT_INJECTOR_HELP);
    assert_eq!(none.media("VOL_UP").reason, INPUT_INJECTOR_HELP);
    for outcome in [
        none.shortcut(&mods(&["ctrl"]), "t"),
        none.text("hello"),
        none.media("VOL_UP"),
    ] {
        assert!(!outcome.ok, "a missing tool must never be a silent success");
    }
}

#[test]
fn the_help_text_matches_the_python_source() {
    assert_eq!(INPUT_INJECTOR_HELP, PYTHON_HELP);
}

// ---------------------------------------------------------------------------
// Shortcut argv, per tool
// ---------------------------------------------------------------------------

#[test]
fn wtype_presses_modifiers_in_order_then_the_key_then_releases_in_reverse() {
    let tools = FakeTools::with(&["wtype"]);
    let injector = injector(InputTool::Wtype, &tools);

    // The caller order is deliberately scrambled; MODIFIER_ORDER decides.
    let outcome = injector.shortcut(&mods(&["super", "shift", "ctrl", "alt"]), "Return");
    assert!(outcome.ok, "unexpected failure: {outcome:?}");

    assert_eq!(
        tools.runs(),
        vec![CommandCall::waited(
            "wtype",
            vec![
                "-M", "ctrl", "-M", "alt", "-M", "shift", "-M", "super", "-P", "Return", "-p",
                "Return", "-m", "super", "-m", "shift", "-m", "alt", "-m", "ctrl",
            ]
        )]
    );
}

#[test]
fn wtype_drops_unknown_duplicated_and_miscased_modifiers() {
    let tools = FakeTools::with(&["wtype"]);
    let injector = injector(InputTool::Wtype, &tools);
    // Python: `[m for m in MODIFIER_ORDER if m in modifiers]` — unknown,
    // repeated and differently-cased names are all dropped.
    let outcome = injector.shortcut(&mods(&["ctrl", "hyper", "ctrl", "alt", "SHIFT"]), "space");
    assert!(outcome.ok, "unexpected failure: {outcome:?}");
    assert_run(
        &tools.runs()[0],
        "wtype",
        &[
            "-M", "ctrl", "-M", "alt", "-P", "space", "-p", "space", "-m", "alt", "-m", "ctrl",
        ],
    );
}

#[test]
fn ydotool_uses_evdev_codes_press_then_release() {
    let tools = FakeTools::with(&["ydotool"]);
    let injector = injector(InputTool::Ydotool, &tools);

    // ctrl 29, alt 56, shift 42, super 125, PAGE_DOWN 109.
    let outcome = injector.shortcut(&mods(&["shift", "super", "alt", "ctrl"]), "PAGE_DOWN");
    assert!(outcome.ok, "unexpected failure: {outcome:?}");
    assert_run(
        &tools.runs()[0],
        "ydotool",
        &[
            "key", "29:1", "56:1", "42:1", "125:1", "109:1", "109:0", "125:0", "42:0", "56:0",
            "29:0",
        ],
    );
}

#[test]
fn ydotool_maps_single_letters_and_function_keys_to_evdev_codes() {
    let tools = FakeTools::with(&["ydotool"]);
    let injector = injector(InputTool::Ydotool, &tools);

    assert!(injector.shortcut(&[], "a").ok);
    assert_run(&tools.runs()[0], "ydotool", &["key", "30:1", "30:0"]);

    assert!(injector.shortcut(&[], "F5").ok);
    assert_run(&tools.runs()[1], "ydotool", &["key", "63:1", "63:0"]);

    assert!(injector.shortcut(&[], "0").ok);
    assert_run(&tools.runs()[2], "ydotool", &["key", "11:1", "11:0"]);

    assert!(injector.shortcut(&[], "9").ok);
    assert_run(&tools.runs()[3], "ydotool", &["key", "10:1", "10:0"]);
}

#[test]
fn ydotool_fails_loudly_for_an_unknown_key() {
    let tools = FakeTools::with(&["ydotool"]);
    let injector = injector(InputTool::Ydotool, &tools);
    let outcome = injector.shortcut(&[], "NOPE");
    assert!(!outcome.ok);
    assert_eq!(outcome.reason, "Key 'NOPE' not recognized by ydotool");
    assert!(
        tools.runs().is_empty(),
        "an unmapped key must not run ydotool: {:?}",
        tools.runs()
    );
}

#[test]
fn xdotool_builds_one_plus_separated_combo() {
    let tools = FakeTools::with(&["xdotool"]);
    let injector = injector(InputTool::Xdotool, &tools);
    let outcome = injector.shortcut(&mods(&["alt", "ctrl"]), "t");
    assert!(outcome.ok, "unexpected failure: {outcome:?}");
    assert_run(
        &tools.runs()[0],
        "xdotool",
        &["key", "--clearmodifiers", "ctrl+alt+t"],
    );
}

#[test]
fn a_shortcut_without_modifiers_still_sends_the_key() {
    let tools = FakeTools::with(&["wtype"]);
    let injector = injector(InputTool::Wtype, &tools);
    assert!(injector.shortcut(&[], "F1").ok);
    assert_run(&tools.runs()[0], "wtype", &["-P", "F1", "-p", "F1"]);
}

// ---------------------------------------------------------------------------
// Key normalisation and the empty-key guard
// ---------------------------------------------------------------------------

#[test]
fn single_character_keys_are_lowercased() {
    let tools = FakeTools::with(&["wtype"]);
    let injector = injector(InputTool::Wtype, &tools);
    assert!(injector.shortcut(&mods(&["ctrl"]), "A").ok);
    assert_run(
        &tools.runs()[0],
        "wtype",
        &["-M", "ctrl", "-P", "a", "-p", "a", "-m", "ctrl"],
    );
}

#[test]
fn named_keys_are_translated_through_the_key_name_map() {
    let tools = FakeTools::with(&["wtype"]);
    let injector = injector(InputTool::Wtype, &tools);
    for (key, expected) in [
        ("ENTER", "Return"),
        ("ESC", "Escape"),
        ("BACKSPACE", "BackSpace"),
        ("PAGE_UP", "Page_Up"),
        ("PAGE_DOWN", "Page_Down"),
        ("DELETE", "Delete"),
        ("UP", "Up"),
        ("HOME", "Home"),
    ] {
        assert!(injector.shortcut(&[], key).ok, "key {key}");
        let runs = tools.runs();
        assert_run(
            runs.last().unwrap(),
            "wtype",
            &["-P", expected, "-p", expected],
        );
    }
}

#[test]
fn an_empty_key_fails_before_any_tool_runs() {
    for tool in [InputTool::Wtype, InputTool::Ydotool, InputTool::Xdotool] {
        let tools = FakeTools::with(&[tool.name()]);
        let injector = injector(tool, &tools);
        let outcome = injector.shortcut(&mods(&["ctrl"]), "");
        assert!(!outcome.ok, "{tool:?} accepted an empty key");
        assert_eq!(outcome.reason, "Target key not selected yet");
        assert!(
            tools.runs().is_empty(),
            "{tool:?} ran a tool for an empty key: {:?}",
            tools.runs()
        );
    }
}

// ---------------------------------------------------------------------------
// Text argv, per tool
// ---------------------------------------------------------------------------

#[test]
fn text_argv_matches_python_for_every_tool() {
    let cases: [(InputTool, &[&str]); 3] = [
        (InputTool::Wtype, &["--", "hello world"]),
        (InputTool::Ydotool, &["type", "--", "hello world"]),
        (
            InputTool::Xdotool,
            &["type", "--clearmodifiers", "--", "hello world"],
        ),
    ];
    for (tool, expected) in cases {
        let tools = FakeTools::with(&[tool.name()]);
        let injector = injector(tool, &tools);
        let outcome = injector.text("hello world");
        assert!(outcome.ok, "unexpected failure: {outcome:?}");
        assert_run(&tools.runs()[0], tool.name(), expected);
    }
}

#[test]
fn text_failure_from_the_tool_is_reported_verbatim() {
    let tools = FakeTools::failing(&["wtype"], "wtype: compositor said no");
    let injector = injector(InputTool::Wtype, &tools);
    let outcome = injector.text("hi");
    assert!(!outcome.ok);
    assert_eq!(outcome.reason, "wtype: compositor said no");
}

// ---------------------------------------------------------------------------
// Media: wpctl / pactl / playerctl, then the XF86 fallback
// ---------------------------------------------------------------------------

#[test]
fn volume_prefers_wpctl() {
    let tools = FakeTools::with(&["wpctl", "pactl", "playerctl", "wtype"]);
    let injector = injector(InputTool::Wtype, &tools);

    assert!(injector.media("VOL_UP").ok);
    assert_run(
        &tools.runs()[0],
        "wpctl",
        &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%+"],
    );

    assert!(injector.media("VOL_DOWN").ok);
    assert_run(
        &tools.runs()[1],
        "wpctl",
        &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"],
    );

    assert!(injector.media("MUTE").ok);
    assert_run(
        &tools.runs()[2],
        "wpctl",
        &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
    );
}

#[test]
fn volume_falls_back_to_pactl() {
    let tools = FakeTools::with(&["pactl", "wtype"]);
    let injector = injector(InputTool::Wtype, &tools);

    assert!(injector.media("VOL_UP").ok);
    assert_run(
        &tools.runs()[0],
        "pactl",
        &["set-sink-volume", "@DEFAULT_AUDIO_SINK@", "+5%"],
    );

    assert!(injector.media("VOL_DOWN").ok);
    assert_run(
        &tools.runs()[1],
        "pactl",
        &["set-sink-volume", "@DEFAULT_AUDIO_SINK@", "-5%"],
    );

    assert!(injector.media("MUTE").ok);
    assert_run(
        &tools.runs()[2],
        "pactl",
        &["set-sink-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
    );
}

#[test]
fn volume_without_wpctl_or_pactl_is_a_visible_failure() {
    let tools = FakeTools::with(&["wtype"]);
    let injector = injector(InputTool::Wtype, &tools);
    for media_key in ["VOL_UP", "VOL_DOWN", "MUTE"] {
        let outcome = injector.media(media_key);
        assert!(!outcome.ok, "{media_key} claimed success");
        assert_eq!(
            outcome.reason,
            "Volume requires wpctl or pactl (pipewire/pulseaudio)."
        );
    }
    assert!(
        tools.runs().is_empty(),
        "nothing should have been simulated: {:?}",
        tools.runs()
    );
}

#[test]
fn playback_uses_playerctl() {
    let tools = FakeTools::with(&["playerctl", "wtype"]);
    let injector = injector(InputTool::Wtype, &tools);

    for (media_key, command) in [
        ("PLAY_PAUSE", "play-pause"),
        ("NEXT_TRACK", "next"),
        ("PREV_TRACK", "previous"),
    ] {
        assert!(injector.media(media_key).ok, "{media_key}");
        let runs = tools.runs();
        assert_run(runs.last().unwrap(), "playerctl", &[command]);
    }
}

#[test]
fn playback_without_playerctl_falls_back_to_the_xf86_key() {
    for (tool, expected_program, expected_args) in [
        (
            InputTool::Wtype,
            "wtype",
            vec!["-P", "XF86AudioPlay", "-p", "XF86AudioPlay"].as_slice(),
        ),
        (
            InputTool::Xdotool,
            "xdotool",
            vec!["key", "XF86AudioPlay"].as_slice(),
        ),
        (
            InputTool::Ydotool,
            "ydotool",
            vec!["key", "207:1", "207:0"].as_slice(),
        ),
    ] {
        let tools = FakeTools::with(&[tool.name()]);
        let injector = injector(tool, &tools);
        let outcome = injector.media("PLAY_PAUSE");
        assert!(outcome.ok, "{tool:?}: {outcome:?}");
        assert_run(&tools.runs()[0], expected_program, expected_args);
    }
}

#[test]
fn the_xf86_fallback_covers_next_and_previous_track() {
    let tools = FakeTools::with(&["wtype"]);
    let injector = injector(InputTool::Wtype, &tools);

    assert!(injector.media("NEXT_TRACK").ok);
    assert_run(
        &tools.runs()[0],
        "wtype",
        &["-P", "XF86AudioNext", "-p", "XF86AudioNext"],
    );

    assert!(injector.media("PREV_TRACK").ok);
    assert_run(
        &tools.runs()[1],
        "wtype",
        &["-P", "XF86AudioPrev", "-p", "XF86AudioPrev"],
    );
}

#[test]
fn an_unknown_media_key_without_playerctl_is_a_visible_failure() {
    let tools = FakeTools::with(&["wtype", "playerctl"]);
    let injector = injector(InputTool::Wtype, &tools);
    let outcome = injector.media("SCREEN_BRIGHTNESS_UP");
    assert!(!outcome.ok);
    assert_eq!(
        outcome.reason,
        "The music player requires playerctl (sudo apt install playerctl)."
    );
    assert!(
        tools.runs().is_empty(),
        "an unknown key must not be simulated: {:?}",
        tools.runs()
    );
}
