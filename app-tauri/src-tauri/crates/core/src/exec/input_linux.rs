//! Linux keyboard, text and media injection.
//!
//! Port of `linux-app/system_actions.py:92-253`. GNOME Wayland does not allow
//! `xdotool`, so the preference order is **wtype** (Wayland) → **ydotool**
//! (uinput) → **xdotool** (X11), exactly like `_injector()`. Which tool is used
//! is decided once by [`LinuxInjector::detect`], which probes `which <tool>`
//! through the injected [`CommandRunner`] — never by touching the filesystem —
//! so every branch below is testable without wtype, ydotool or xdotool being
//! installed.
//!
//! Nothing here shells out directly and nothing reports success it did not
//! achieve: a tool that is missing, a key ydotool cannot map and a media key
//! with no handler all come back as `ActionOutcome::fail` with the reason
//! string the Python app prints.

use super::{ActionOutcome, CommandRunner, InputInjector};

/// `MODIFIER_ORDER` — the order modifiers are pressed and released in,
/// regardless of the order the config stored them.
pub const MODIFIER_ORDER: [&str; 4] = ["ctrl", "alt", "shift", "super"];

/// `KEY_NAME_MAP` — our UI key names translated to tool key names. Single
/// characters are lower-cased instead (see [`xkey`]).
pub const KEY_NAME_MAP: [(&str, &str); 14] = [
    ("ENTER", "Return"),
    ("ESC", "Escape"),
    ("BACKSPACE", "BackSpace"),
    ("TAB", "Tab"),
    ("SPACE", "space"),
    ("UP", "Up"),
    ("DOWN", "Down"),
    ("LEFT", "Left"),
    ("RIGHT", "Right"),
    ("HOME", "Home"),
    ("END", "End"),
    ("PAGE_UP", "Page_Up"),
    ("PAGE_DOWN", "Page_Down"),
    ("DELETE", "Delete"),
];

/// `MEDIA_XF86` — the fallback XF86 key names, for when no media tool handles
/// the key.
pub const MEDIA_XF86: [(&str, &str); 3] = [
    ("PLAY_PAUSE", "XF86AudioPlay"),
    ("NEXT_TRACK", "XF86AudioNext"),
    ("PREV_TRACK", "XF86AudioPrev"),
];

/// `_EVDEV_MODS` — ydotool takes Linux evdev keycodes, not key names:
/// `29:1` is "press Ctrl".
const EVDEV_MODS: [(&str, u16); 4] = [("ctrl", 29), ("alt", 56), ("shift", 42), ("super", 125)];

/// `_EVDEV_KEYS` — the non-printable keys ydotool can type, lower-cased.
const EVDEV_KEYS: [(&str, u16); 17] = [
    ("return", 28),
    ("escape", 1),
    ("backspace", 14),
    ("tab", 15),
    ("space", 57),
    ("up", 103),
    ("down", 108),
    ("left", 105),
    ("right", 106),
    ("home", 102),
    ("end", 107),
    ("page_up", 104),
    ("page_down", 109),
    ("delete", 111),
    ("xf86audioplay", 207),
    ("xf86audionext", 163),
    ("xf86audioprev", 165),
];

/// `@DEFAULT_AUDIO_SINK@` — the PipeWire/PulseAudio default sink.
const DEFAULT_SINK: &str = "@DEFAULT_AUDIO_SINK@";

/// `linux-app/system_actions.py:188`.
pub const NO_TARGET_KEY: &str = "Target key not selected yet";
/// `linux-app/system_actions.py:252`.
pub const NO_VOLUME_TOOL: &str = "Volume requires wpctl or pactl (pipewire/pulseaudio).";
/// `linux-app/system_actions.py:253`.
pub const NO_PLAYERCTL: &str = "The music player requires playerctl (sudo apt install playerctl).";

/// The keyboard simulation tools, in the order they are preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTool {
    /// Wayland's own virtual keyboard protocol.
    Wtype,
    /// uinput based; the recommended tool on GNOME Wayland.
    Ydotool,
    /// X11 only; useless on a pure Wayland session.
    Xdotool,
}

impl InputTool {
    /// The preference order of `_injector()`.
    pub const ALL: [InputTool; 3] = [InputTool::Wtype, InputTool::Ydotool, InputTool::Xdotool];

    /// The program name as it appears in the argv.
    pub const fn name(self) -> &'static str {
        match self {
            InputTool::Wtype => "wtype",
            InputTool::Ydotool => "ydotool",
            InputTool::Xdotool => "xdotool",
        }
    }

    /// Reverse of [`InputTool::name`], used by the detection probe.
    pub const fn from_name(name: &str) -> Option<InputTool> {
        match name.as_bytes() {
            b"wtype" => Some(InputTool::Wtype),
            b"ydotool" => Some(InputTool::Ydotool),
            b"xdotool" => Some(InputTool::Xdotool),
            _ => None,
        }
    }
}

/// `shutil.which(tool)` through the injected runner.
pub fn has_tool(runner: &dyn CommandRunner, tool: &str) -> bool {
    runner.run("which", &[tool]).ok
}

/// Borrowed argv for [`CommandRunner::run`], which takes `&[&str]`.
fn as_args(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// `_injector_help`, kept next to the code that fails to find a tool.
pub const INJECTOR_HELP: &str = super::INPUT_INJECTOR_HELP;

/// `_xkey`: a named key through [`KEY_NAME_MAP`], a single character
/// lower-cased, anything else passed through untouched.
pub fn xkey(key: &str) -> String {
    for (name, mapped) in KEY_NAME_MAP {
        if name == key {
            return mapped.to_string();
        }
    }
    if key.chars().count() == 1 {
        key.to_lowercase()
    } else {
        key.to_string()
    }
}

/// `_EVDEV_MODS` lookup.
pub fn evdev_mod(name: &str) -> Option<u16> {
    EVDEV_MODS
        .iter()
        .find(|(mod_name, _)| *mod_name == name)
        .map(|(_, code)| *code)
}

/// `_evdev_key`: the evdev keycode for a key name, or `None` when ydotool
/// cannot type it.
pub fn evdev_key(name: &str) -> Option<u16> {
    let lower = name.to_lowercase();
    for (key_name, code) in EVDEV_KEYS {
        if key_name == lower {
            return Some(code);
        }
    }
    // F1..F24 sit at 59..82.
    if let Some(rest) = lower.strip_prefix('f') {
        if !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()) {
            if let Ok(number) = rest.parse::<u32>() {
                if (1..=24).contains(&number) {
                    return Some(58 + number as u16);
                }
            }
        }
    }
    let mut chars = lower.chars();
    if let (Some(character), None) = (chars.next(), chars.next()) {
        return match character {
            // a = 30
            'a'..='z' => Some((character as u32 - 96 + 29) as u16),
            '0' => Some(11),
            // 1 = 2
            '1'..='9' => Some((character as u32 - 48 + 1) as u16),
            _ => None,
        };
    }
    None
}

/// The `ydotool key` arguments: press every key, then release them in reverse.
pub fn ydotool_args(mods: &[&str], key: &str) -> Option<Vec<String>> {
    let mut codes: Vec<u16> = mods.iter().filter_map(|name| evdev_mod(name)).collect();
    match evdev_key(key) {
        Some(code) => codes.push(code),
        // Python: an unmapped key with no modifiers produces no arguments,
        // which `_send_shortcut` turns into a failure.
        None if codes.is_empty() => return None,
        None => {}
    }
    let mut args: Vec<String> = codes.iter().map(|code| format!("{code}:1")).collect();
    args.extend(codes.iter().rev().map(|code| format!("{code}:0")));
    Some(args)
}

/// `MODIFIER_ORDER` applied to the config's list: unknown and repeated
/// modifiers are dropped, the canonical order wins.
pub fn ordered_modifiers(modifiers: &[String]) -> Vec<&str> {
    MODIFIER_ORDER
        .iter()
        .copied()
        .filter(|name| modifiers.iter().any(|given| given == name))
        .collect()
}

/// The `wtype` argv: press each modifier, tap the key, release in reverse.
pub fn wtype_args(mods: &[&str], key: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    for name in mods {
        args.push("-M".to_string());
        args.push((*name).to_string());
    }
    args.push("-P".to_string());
    args.push(key.to_string());
    args.push("-p".to_string());
    args.push(key.to_string());
    for name in mods.iter().rev() {
        args.push("-m".to_string());
        args.push((*name).to_string());
    }
    args
}

/// The `xdotool` argv: one `+`-separated combo, existing modifiers cleared.
pub fn xdotool_args(mods: &[&str], key: &str) -> Vec<String> {
    let mut combo: Vec<&str> = mods.to_vec();
    combo.push(key);
    vec![
        "key".to_string(),
        "--clearmodifiers".to_string(),
        combo.join("+"),
    ]
}

/// `MEDIA_XF86` lookup.
pub fn media_xf86(media_key: &str) -> Option<&'static str> {
    MEDIA_XF86
        .iter()
        .find(|(name, _)| *name == media_key)
        .map(|(_, key)| *key)
}

/// `playerctl` sub-command for the playback keys.
pub fn playerctl_command(media_key: &str) -> Option<&'static str> {
    match media_key {
        "PLAY_PAUSE" => Some("play-pause"),
        "NEXT_TRACK" => Some("next"),
        "PREV_TRACK" => Some("previous"),
        _ => None,
    }
}

/// The keyboard injector for Linux, bound to one specific tool.
///
/// The tool is chosen once by [`LinuxInjector::detect`]; every call then reuses
/// it, so one burst of presses cannot switch tools halfway through.
pub struct LinuxInjector<'a> {
    tool: InputTool,
    runner: &'a dyn CommandRunner,
}

impl<'a> LinuxInjector<'a> {
    /// The first of wtype, ydotool, xdotool that `which` can find, or `None`
    /// when the machine has none of them (the caller then uses
    /// [`NoInputInjector`](super::NoInputInjector) and the help text).
    pub fn detect(runner: &'a dyn CommandRunner) -> Option<LinuxInjector<'a>> {
        InputTool::ALL
            .into_iter()
            .find(|tool| has_tool(runner, tool.name()))
            .map(|tool| LinuxInjector { tool, runner })
    }

    /// Bind to a known tool without probing (used when the caller already
    /// detected, and by the tests).
    pub fn with_tool(tool: InputTool, runner: &'a dyn CommandRunner) -> LinuxInjector<'a> {
        LinuxInjector { tool, runner }
    }

    /// The tool in use, as `available()` reports it.
    pub fn tool_name(&self) -> &'static str {
        self.tool.name()
    }

    /// The tool enum itself.
    pub fn tool(&self) -> InputTool {
        self.tool
    }

    /// The [`CommandRunner`] this injector runs tools with.
    pub fn runner(&self) -> &'a dyn CommandRunner {
        self.runner
    }

    /// `_send_shortcut`.
    pub fn send_shortcut(&self, modifiers: &[String], key: &str) -> ActionOutcome {
        let mods = ordered_modifiers(modifiers);
        let target = xkey(key);
        if target.is_empty() {
            return ActionOutcome::fail(NO_TARGET_KEY);
        }
        match self.tool {
            InputTool::Wtype => {
                let args = wtype_args(&mods, &target);
                self.runner.run(self.tool.name(), &as_args(&args))
            }
            InputTool::Ydotool => match ydotool_args(&mods, &target) {
                Some(mut args) => {
                    args.insert(0, "key".to_string());
                    self.runner.run(self.tool.name(), &as_args(&args))
                }
                None => ActionOutcome::fail(format!("Key '{key}' not recognized by ydotool")),
            },
            InputTool::Xdotool => {
                let args = xdotool_args(&mods, &target);
                self.runner.run(self.tool.name(), &as_args(&args))
            }
        }
    }

    /// `_send_text`.
    pub fn send_text(&self, text: &str) -> ActionOutcome {
        let args: Vec<&str> = match self.tool {
            InputTool::Wtype => vec!["--", text],
            InputTool::Ydotool => vec!["type", "--", text],
            InputTool::Xdotool => vec!["type", "--clearmodifiers", "--", text],
        };
        self.runner.run(self.tool.name(), &args)
    }

    /// `_send_media`: `wpctl`/`pactl` for volume, `playerctl` for playback,
    /// and the XF86 key through the keyboard tool as the last resort.
    pub fn send_media(&self, media_key: &str) -> ActionOutcome {
        let is_volume = matches!(media_key, "VOL_UP" | "VOL_DOWN" | "MUTE");
        if is_volume {
            if has_tool(self.runner, "wpctl") {
                return match media_key {
                    "VOL_UP" => self
                        .runner
                        .run("wpctl", &["set-volume", DEFAULT_SINK, "5%+"]),
                    "VOL_DOWN" => self
                        .runner
                        .run("wpctl", &["set-volume", DEFAULT_SINK, "5%-"]),
                    _ => self
                        .runner
                        .run("wpctl", &["set-mute", DEFAULT_SINK, "toggle"]),
                };
            }
            if has_tool(self.runner, "pactl") {
                return match media_key {
                    "VOL_UP" => self
                        .runner
                        .run("pactl", &["set-sink-volume", DEFAULT_SINK, "+5%"]),
                    "VOL_DOWN" => self
                        .runner
                        .run("pactl", &["set-sink-volume", DEFAULT_SINK, "-5%"]),
                    _ => self
                        .runner
                        .run("pactl", &["set-sink-mute", DEFAULT_SINK, "toggle"]),
                };
            }
        }
        if let Some(command) = playerctl_command(media_key) {
            if has_tool(self.runner, "playerctl") {
                return self.runner.run("playerctl", &[command]);
            }
        }
        if let Some(xf86) = media_xf86(media_key) {
            return self.send_xf86(xf86, media_key);
        }
        if is_volume {
            return ActionOutcome::fail(NO_VOLUME_TOOL);
        }
        ActionOutcome::fail(NO_PLAYERCTL)
    }

    /// The XF86 media key, pressed through the keyboard tool.
    fn send_xf86(&self, xf86: &str, media_key: &str) -> ActionOutcome {
        match self.tool {
            InputTool::Wtype => self.runner.run(self.tool.name(), &["-P", xf86, "-p", xf86]),
            InputTool::Xdotool => self.runner.run(self.tool.name(), &["key", xf86]),
            InputTool::Ydotool => match ydotool_args(&[], xf86) {
                Some(mut args) => {
                    args.insert(0, "key".to_string());
                    self.runner.run(self.tool.name(), &as_args(&args))
                }
                None => ActionOutcome::fail(format!(
                    "Media key '{media_key}' not recognized by ydotool"
                )),
            },
        }
    }
}

impl InputInjector for LinuxInjector<'_> {
    fn available(&self) -> Option<String> {
        Some(self.tool.name().to_string())
    }

    fn shortcut(&self, modifiers: &[String], key: &str) -> ActionOutcome {
        self.send_shortcut(modifiers, key)
    }

    fn text(&self, text: &str) -> ActionOutcome {
        self.send_text(text)
    }

    fn media(&self, media_key: &str) -> ActionOutcome {
        self.send_media(media_key)
    }
}

impl std::fmt::Debug for LinuxInjector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxInjector")
            .field("tool", &self.tool)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ActionOutcome, InputInjector, RecordingRunner};
    use super::*;

    fn mods(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn tool_names_round_trip() {
        for tool in InputTool::ALL {
            assert_eq!(InputTool::from_name(tool.name()), Some(tool));
        }
        assert_eq!(InputTool::from_name("ydotool"), Some(InputTool::Ydotool));
        assert_eq!(InputTool::from_name("wtype"), Some(InputTool::Wtype));
        assert_eq!(InputTool::from_name("xdotool"), Some(InputTool::Xdotool));
        assert_eq!(InputTool::from_name("ydotoold"), None);
        assert_eq!(InputTool::from_name(""), None);
        assert_eq!(InputTool::ALL.len(), 3, "the order is the contract");
    }

    #[test]
    fn modifiers_are_ordered_and_filtered() {
        assert_eq!(
            ordered_modifiers(&mods(&["super", "alt", "nope", "ctrl", "ctrl"])),
            vec!["ctrl", "alt", "super"]
        );
        assert!(ordered_modifiers(&mods(&["hyper"])).is_empty());
    }

    #[test]
    fn xkey_maps_named_keys_lowercases_single_chars_and_passes_the_rest_through() {
        assert_eq!(xkey("ENTER"), "Return");
        assert_eq!(xkey("PAGE_UP"), "Page_Up");
        assert_eq!(xkey("BACKSPACE"), "BackSpace");
        assert_eq!(xkey("ESC"), "Escape");
        assert_eq!(xkey("DELETE"), "Delete");
        assert_eq!(xkey("TAB"), "Tab");
        assert_eq!(xkey("SPACE"), "space");
        assert_eq!(xkey("A"), "a");
        assert_eq!(xkey("7"), "7");
        assert_eq!(xkey("F1"), "F1", "multi-char names are left alone");
        assert_eq!(xkey(""), "");
    }

    #[test]
    fn evdev_codes_cover_the_python_tables() {
        assert_eq!(evdev_mod("ctrl"), Some(29));
        assert_eq!(evdev_mod("alt"), Some(56));
        assert_eq!(evdev_mod("shift"), Some(42));
        assert_eq!(evdev_mod("super"), Some(125));
        assert_eq!(evdev_mod("win"), None);

        assert_eq!(evdev_key("Return"), Some(28));
        assert_eq!(evdev_key("RETURN"), Some(28));
        assert_eq!(evdev_key("Escape"), Some(1));
        assert_eq!(evdev_key("space"), Some(57));
        assert_eq!(evdev_key("Page_Up"), Some(104));
        assert_eq!(evdev_key("Delete"), Some(111));
        assert_eq!(evdev_key("XF86AudioPlay"), Some(207));
        assert_eq!(evdev_key("XF86AudioNext"), Some(163));
        assert_eq!(evdev_key("XF86AudioPrev"), Some(165));
        assert_eq!(evdev_key("f1"), Some(59));
        assert_eq!(evdev_key("F24"), Some(82));
        assert_eq!(evdev_key("f25"), None, "F25 is outside the range");
        assert_eq!(evdev_key("a"), Some(30));
        assert_eq!(evdev_key("z"), Some(55));
        assert_eq!(evdev_key("0"), Some(11));
        assert_eq!(evdev_key("1"), Some(2));
        assert_eq!(evdev_key("9"), Some(10));
        assert_eq!(evdev_key("f"), Some(35), "'f' is a letter, not F-nothing");
        assert_eq!(evdev_key("!"), None);
        assert_eq!(evdev_key(""), None);
        assert_eq!(evdev_key("Shift"), None, "modifiers use evdev_mod");
    }

    #[test]
    fn ydotool_args_press_then_release_in_reverse() {
        assert_eq!(
            ydotool_args(&["ctrl", "alt"], "t").unwrap(),
            vec!["29:1", "56:1", "49:1", "49:0", "56:0", "29:0"]
        );
        assert_eq!(
            ydotool_args(&[], "XF86AudioPlay").unwrap(),
            vec!["207:1", "207:0"]
        );
        // Unknown key, no modifiers: Python returns an empty argument list.
        assert_eq!(ydotool_args(&[], "NOPE"), None);
        // Unknown key, but the modifiers alone are still a valid press.
        assert_eq!(
            ydotool_args(&["ctrl"], "NOPE").unwrap(),
            vec!["29:1", "29:0"]
        );
    }

    #[test]
    fn argv_builders_match_python() {
        assert_eq!(
            wtype_args(&["ctrl", "alt"], "Return"),
            vec![
                "-M", "ctrl", "-M", "alt", "-P", "Return", "-p", "Return", "-m", "alt", "-m",
                "ctrl"
            ]
        );
        assert_eq!(wtype_args(&[], "a"), vec!["-P", "a", "-p", "a"]);
        assert_eq!(
            xdotool_args(&["ctrl", "alt"], "t"),
            vec!["key", "--clearmodifiers", "ctrl+alt+t"]
        );
        assert_eq!(
            xdotool_args(&[], "Return"),
            vec!["key", "--clearmodifiers", "Return"]
        );
    }

    #[test]
    fn media_lookups_match_python() {
        assert_eq!(media_xf86("PLAY_PAUSE"), Some("XF86AudioPlay"));
        assert_eq!(media_xf86("NEXT_TRACK"), Some("XF86AudioNext"));
        assert_eq!(media_xf86("PREV_TRACK"), Some("XF86AudioPrev"));
        assert_eq!(media_xf86("VOL_UP"), None);
        assert_eq!(playerctl_command("PLAY_PAUSE"), Some("play-pause"));
        assert_eq!(playerctl_command("NEXT_TRACK"), Some("next"));
        assert_eq!(playerctl_command("PREV_TRACK"), Some("previous"));
        assert_eq!(playerctl_command("VOL_UP"), None);
    }

    #[test]
    fn detect_probes_in_order_and_stops_at_the_first_hit() {
        let runner = RecordingRunner::scripted(vec![
            ActionOutcome::fail("wtype not found"),
            ActionOutcome::fail("ydotool not found"),
            ActionOutcome::ok("/usr/bin/xdotool"),
        ]);
        let injector = LinuxInjector::detect(&runner).expect("xdotool");
        assert_eq!(injector.tool_name(), "xdotool");
        assert_eq!(injector.tool(), InputTool::Xdotool);
        assert_eq!(runner.call_count(), 3);
        assert_eq!(
            injector.available().as_deref(),
            Some("xdotool"),
            "the console reports the tool in use"
        );
    }

    #[test]
    fn detect_returns_none_when_nothing_is_installed() {
        let runner = RecordingRunner::new(ActionOutcome::fail("not found"));
        assert!(LinuxInjector::detect(&runner).is_none());
        assert_eq!(runner.call_count(), 3, "all three candidates are probed");
    }

    #[test]
    fn the_help_text_is_the_python_one() {
        assert!(INJECTOR_HELP.contains("sudo apt install wtype"));
        assert!(INJECTOR_HELP.contains("systemctl --user start ydotool"));
        assert!(INJECTOR_HELP.contains("or `xdotool` (X11)."));
    }

    #[test]
    fn has_tool_asks_the_runner() {
        let runner = RecordingRunner::new(ActionOutcome::ok("/usr/bin/wtype"));
        assert!(has_tool(&runner, "wtype"));
        let missing = RecordingRunner::new(ActionOutcome::fail("wtype not found"));
        assert!(!has_tool(&missing, "wtype"));
    }
}
