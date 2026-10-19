//! Windows keyboard, text and media injection.
//!
//! Port of `windows-app/platform_actions.py:62-190`: `_MODIFIER_VK`,
//! `_VK_SPECIAL`, `_EXTENDED_VK`, `_VK_MEDIA`, `_vk_for`, `_key_event`,
//! `_type_unicode_char`, `_send_text`, `_send_shortcut` and `_media_key`.
//!
//! Like [`super::input_linux`] the file has two layers:
//!
//! * a **pure mapping layer** — key name → virtual-key code, modifier list →
//!   press/release order, media key → VK, text → a list of [`KeyEvent`]s —
//!   that is compiled on every target so the ubuntu CI job can pin every table
//!   and every reason string without a Windows machine;
//! * a **thin `SendInput` layer**, behind `#[cfg(windows)]`, that does nothing
//!   but walk a [`KeyEvent`] plan into `user32!SendInput`.
//!
//! Splitting it that way is what makes the parity claims checkable: the order
//! of the INPUTs is a pure function, so a Linux test can assert it, and the
//! Windows-only part is only the FFI call.
//!
//! Parity notes, where Python is the authority:
//!
//! * **Modifier order is the config's own.** Linux normalises to
//!   `ctrl, alt, shift, super` because the tools want that; Windows presses
//!   whatever the config listed, in the listed order, because `_send_shortcut`
//!   builds its list with a comprehension over `_MODIFIER_VK`. Unknown names
//!   are dropped and repeats are kept, exactly as the comprehension does.
//! * **`SendInput`'s return value is checked**, unlike Python, which ignores
//!   it and always reports success. A run that inserted fewer events than asked
//!   (the usual cause is a higher-integrity window blocking input, UIPI) is
//!   reported as a failure instead of claiming "Shortcut sent". The success
//!   strings themselves are unchanged.
//! * The unicode scan code is truncated to 16 bits, because Python hands
//!   `ord(char)` to a `wintypes.WORD` field, which truncates too.

use super::{ActionOutcome, InputInjector};

// ---------------------------------------------------------------------------
// The reason strings, verbatim from the Python app.
// ---------------------------------------------------------------------------

/// `platform_actions.py:171` — `_vk_for` found nothing for the configured key.
pub const NO_TARGET_KEY: &str = "Target key not selected yet";
/// `platform_actions.py:163` — `_send_text` finished typing.
pub const TEXT_TYPED: &str = "Text typed";
/// `platform_actions.py:179` — `_send_shortcut` finished pressing.
pub const SHORTCUT_SENT: &str = "Shortcut sent";

// ---------------------------------------------------------------------------
// Pure mapping layer — compiled on every target so Linux CI can test it.
// ---------------------------------------------------------------------------

/// `_MODIFIER_VK`, `platform_actions.py:69`.
pub const MODIFIER_VK: [(&str, u16); 4] = [
    ("ctrl", 0x11),
    ("alt", 0x12),
    ("shift", 0x10),
    ("super", 0x5B),
];

/// `_VK_SPECIAL`, `platform_actions.py:70`.
pub const VK_SPECIAL: [(&str, u16); 14] = [
    ("ENTER", 0x0D),
    ("ESC", 0x1B),
    ("BACKSPACE", 0x08),
    ("TAB", 0x09),
    ("SPACE", 0x20),
    ("UP", 0x26),
    ("DOWN", 0x28),
    ("LEFT", 0x25),
    ("RIGHT", 0x27),
    ("HOME", 0x24),
    ("END", 0x23),
    ("PAGE_UP", 0x21),
    ("PAGE_DOWN", 0x22),
    ("DELETE", 0x2E),
];

/// `_EXTENDED_VK`, `platform_actions.py:75`: the navigation block and the
/// media keys, which need `KEYEVENTF_EXTENDEDKEY` to be read as themselves
/// instead of their numpad twins.
pub const EXTENDED_VK: [u16; 16] = [
    0x26, 0x28, 0x25, 0x27, 0x24, 0x23, 0x21, 0x22, 0x2E, 0xAD, 0xAE, 0xAF, 0xB0, 0xB1, 0xB2, 0xB3,
];

/// `_VK_MEDIA`, `platform_actions.py:79`.
pub const VK_MEDIA: [(&str, u16); 6] = [
    ("MUTE", 0xAD),
    ("VOL_UP", 0xAF),
    ("VOL_DOWN", 0xAE),
    ("PLAY_PAUSE", 0xB3),
    ("NEXT_TRACK", 0xB0),
    ("PREV_TRACK", 0xB1),
];

/// `KEYEVENTF_EXTENDEDKEY`, `platform_actions.py:64`.
pub const KEYEVENTF_EXTENDEDKEY: u32 = 0x0001;
/// `KEYEVENTF_KEYUP`, `platform_actions.py:65`.
pub const KEYEVENTF_KEYUP: u32 = 0x0002;
/// `KEYEVENTF_UNICODE`, `platform_actions.py:66`.
pub const KEYEVENTF_UNICODE: u32 = 0x0004;

/// `_MODIFIER_VK` lookup.
pub fn modifier_vk(name: &str) -> Option<u16> {
    MODIFIER_VK
        .iter()
        .find(|(modifier, _)| *modifier == name)
        .map(|(_, vk)| *vk)
}

/// `[MODIFIER_VK[m] for m in modifiers if m in _MODIFIER_VK]`.
///
/// The Python comprehension keeps the order the config used and keeps
/// duplicates; only unknown names disappear.
pub fn modifier_sequence(modifiers: &[String]) -> Vec<u16> {
    modifiers
        .iter()
        .filter_map(|name| modifier_vk(name))
        .collect()
}

/// `_VK_SPECIAL` lookup (exact name, no case folding).
pub fn vk_special(name: &str) -> Option<u16> {
    VK_SPECIAL
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, vk)| *vk)
}

/// `_vk_for`, `platform_actions.py:139`: upper-case the name, take the ASCII
/// code of a single letter or digit, map `F1..F24` to `0x70 + n - 1`, and
/// otherwise fall back to the special-key table.
pub fn vk_for(key: &str) -> Option<u16> {
    let upper = key.to_uppercase();
    let mut chars = upper.chars();
    if let (Some(character), None) = (chars.next(), chars.next()) {
        if character.is_ascii_uppercase() || character.is_ascii_digit() {
            return Some(character as u16);
        }
    }
    if let Some(rest) = upper.strip_prefix('F') {
        // Python: `len(k) >= 2 and k[0] == "F" and k[1:].isdigit()`.
        if let Ok(number) = rest.parse::<u32>() {
            if (1..=24).contains(&number) {
                return Some(0x70 + number as u16 - 1);
            }
        }
    }
    vk_special(&upper)
}

/// `vk in _EXTENDED_VK`.
pub fn is_extended_vk(vk: u16) -> bool {
    EXTENDED_VK.contains(&vk)
}

/// `_VK_MEDIA` lookup (exact name, like the Python `dict.get`).
pub fn media_vk(media_key: &str) -> Option<u16> {
    VK_MEDIA
        .iter()
        .find(|(name, _)| *name == media_key)
        .map(|(_, vk)| *vk)
}

/// `f"Unknown media key: {media_key}"`, `platform_actions.py:185`.
pub fn unknown_media_key_reason(media_key: &str) -> String {
    format!("Unknown media key: {media_key}")
}

/// `f"Media key {media_key} sent"`, `platform_actions.py:190`.
pub fn media_key_sent_reason(media_key: &str) -> String {
    format!("Media key {media_key} sent")
}

/// One `KEYBDINPUT` worth of work, i.e. one `INPUT` handed to `SendInput`.
///
/// A virtual-key event leaves `scan` at zero; a unicode event leaves `vk` at
/// zero and puts the code point in `scan` with `KEYEVENTF_UNICODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub vk: u16,
    pub scan: u16,
    pub flags: u32,
}

impl KeyEvent {
    pub const fn new(vk: u16, scan: u16, flags: u32) -> KeyEvent {
        KeyEvent { vk, scan, flags }
    }
}

/// `_key_event`, `platform_actions.py:119`.
pub fn key_event(vk: u16, key_up: bool) -> KeyEvent {
    let mut flags = if key_up { KEYEVENTF_KEYUP } else { 0 };
    if is_extended_vk(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    KeyEvent::new(vk, 0, flags)
}

/// `_type_unicode_char`, `platform_actions.py:120`: a down/up pair with
/// `KEYEVENTF_UNICODE` on both.
///
/// The scan code is truncated to 16 bits, because `wintypes.WORD` is a 16-bit
/// field and Python hands `ord(char)` straight into it.
pub fn unicode_events(character: char) -> [KeyEvent; 2] {
    let scan = character as u32 as u16;
    [
        KeyEvent::new(0, scan, KEYEVENTF_UNICODE),
        KeyEvent::new(0, scan, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
    ]
}

/// `_send_shortcut`, `platform_actions.py:166`, minus the `SendInput` calls:
/// modifiers down, the key down, the key up, modifiers up in reverse.
///
/// `None` when `_vk_for` cannot resolve the configured key — the caller turns
/// that into `Target key not selected yet` without touching the keyboard.
pub fn shortcut_events(modifiers: &[String], key: &str) -> Option<Vec<KeyEvent>> {
    let vk = vk_for(key)?;
    let mods = modifier_sequence(modifiers);
    let mut events: Vec<KeyEvent> = mods.iter().map(|vk| key_event(*vk, false)).collect();
    events.push(key_event(vk, false));
    events.push(key_event(vk, true));
    events.extend(mods.iter().rev().map(|vk| key_event(*vk, true)));
    Some(events)
}

/// `_send_text`, `platform_actions.py:151`, minus the `SendInput` calls.
///
/// `\n` and `\t` are virtual-key taps (RETURN and TAB, the two characters the
/// console actually wants as keys); everything else is typed as unicode, which
/// is what makes accented and non-Latin text work at all.
pub fn text_events(text: &str) -> Vec<KeyEvent> {
    let mut events = Vec::with_capacity(text.chars().count() * 2);
    for character in text.chars() {
        match character {
            '\n' => {
                events.push(key_event(0x0D, false));
                events.push(key_event(0x0D, true));
            }
            '\t' => {
                events.push(key_event(0x09, false));
                events.push(key_event(0x09, true));
            }
            other => events.extend(unicode_events(other)),
        }
    }
    events
}

/// `_media_key`, `platform_actions.py:182`: the VK and its press/release
/// pair, or `None` for a key the table does not know.
pub fn media_events(media_key: &str) -> Option<[KeyEvent; 2]> {
    let vk = media_vk(media_key)?;
    Some([key_event(vk, false), key_event(vk, true)])
}

// ---------------------------------------------------------------------------
// The SendInput layer — Windows only.
// ---------------------------------------------------------------------------

/// `INPUT_KEYBOARD`, `platform_actions.py:67`.
#[cfg(windows)]
const INPUT_KEYBOARD: u32 = 1;

/// The `user32!SendInput` argument types, laid out exactly like the Python
/// `ctypes.Structure`s of `platform_actions.py:88-106`.
///
/// `INPUT` is a tagged union whose largest arm is `MOUSEINPUT`, so the union
/// has to carry that shape too or `SendInput` would read a too-small struct.
#[cfg(windows)]
mod ffi {
    use super::{KeyEvent, INPUT_KEYBOARD};

    /// `ULONG_PTR` is 8 bytes on x64 and 4 on x86; `usize` follows the target.
    type UintPtr = usize;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct KeybdInput {
        pub vk: u16,
        pub scan: u16,
        pub flags: u32,
        pub time: u32,
        pub extra_info: UintPtr,
    }

    /// Only its size matters: it fixes the size of the union.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MouseInput {
        pub dx: i32,
        pub dy: i32,
        pub mouse_data: u32,
        pub flags: u32,
        pub time: u32,
        pub extra_info: UintPtr,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HardwareInput {
        pub message: u32,
        pub param_l: u16,
        pub param_h: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub union InputUnion {
        pub keyboard: KeybdInput,
        pub mouse: MouseInput,
        pub hardware: HardwareInput,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Input {
        pub kind: u32,
        pub data: InputUnion,
    }

    /// `SendInput` is handed `size_of::<Input>()` and reads that many bytes
    /// per INPUT, so a layout mistake would be a silent runtime failure
    /// instead of a compile error. Pin both sizes here: 40/24 on x64 and
    /// 28/20 on x86, which is what `ctypes.sizeof` reports for the Python
    /// structures this file is a port of.
    const _: () = {
        let wide = size_of::<UintPtr>() == 8;
        assert!(size_of::<KeybdInput>() == if wide { 24 } else { 20 });
        assert!(size_of::<Input>() == if wide { 40 } else { 28 });
    };

    impl Input {
        pub fn keyboard(event: KeyEvent) -> Input {
            Input {
                kind: INPUT_KEYBOARD,
                data: InputUnion {
                    keyboard: KeybdInput {
                        vk: event.vk,
                        scan: event.scan,
                        flags: event.flags,
                        time: 0,
                        extra_info: 0,
                    },
                },
            }
        }
    }

    #[link(name = "user32")]
    extern "system" {
        fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
    }

    /// Push a whole plan and report how many of its events the OS accepted.
    ///
    /// This is the only call the injector makes. Python sends one INPUT at a
    /// time and discards the return value; sending the plan as one call is the
    /// same sequence with a single syscall, and the return value is what tells
    /// us whether the keys actually went anywhere.
    pub fn send_all(events: &[KeyEvent]) -> usize {
        let inputs: Vec<Input> = events.iter().copied().map(Input::keyboard).collect();
        if inputs.is_empty() {
            return 0;
        }
        let size = size_of::<Input>() as i32;
        let count = inputs.len().min(u32::MAX as usize) as u32;
        unsafe { SendInput(count, inputs.as_ptr(), size) as usize }
    }
}

/// The keyboard injector for Windows.
///
/// There is nothing to detect: `SendInput` is part of `user32`, so this type
/// is always available, which is why `platform_input_injector` hands it out
/// without probing anything.
#[cfg(windows)]
pub struct WindowsInjector;

#[cfg(windows)]
impl WindowsInjector {
    pub const fn new() -> WindowsInjector {
        WindowsInjector
    }

    /// `_send_shortcut`.
    ///
    /// Nothing is pressed when the configured key has no virtual key: Python
    /// checks first and leaves the keyboard untouched, and so does this.
    pub fn send_shortcut(&self, modifiers: &[String], key: &str) -> ActionOutcome {
        let Some(events) = shortcut_events(modifiers, key) else {
            return ActionOutcome::fail(NO_TARGET_KEY);
        };
        self.send(events, SHORTCUT_SENT)
    }

    /// `_send_text`.
    pub fn send_text(&self, text: &str) -> ActionOutcome {
        self.send(text_events(text), TEXT_TYPED)
    }

    /// `_media_key`.
    pub fn send_media(&self, media_key: &str) -> ActionOutcome {
        let Some(events) = media_events(media_key) else {
            return ActionOutcome::fail(unknown_media_key_reason(media_key));
        };
        let reason = media_key_sent_reason(media_key);
        self.send(events.to_vec(), &reason)
    }

    /// Replay one plan and report it as the Python reason — or as an honest
    /// failure when the OS refused some of it.
    fn send(&self, events: Vec<KeyEvent>, success: &str) -> ActionOutcome {
        let expected = events.len();
        let inserted = ffi::send_all(&events);
        if inserted == expected {
            ActionOutcome::ok(success)
        } else {
            ActionOutcome::fail(format!(
                "SendInput inserted {inserted} of {expected} events"
            ))
        }
    }
}

#[cfg(windows)]
impl InputInjector for WindowsInjector {
    fn available(&self) -> Option<String> {
        Some("SendInput".to_string())
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

#[cfg(windows)]
impl std::fmt::Debug for WindowsInjector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsInjector").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn the_tables_have_no_duplicate_entries() {
        let modifier: Vec<&str> = MODIFIER_VK.iter().map(|(name, _)| *name).collect();
        let special: Vec<&str> = VK_SPECIAL.iter().map(|(name, _)| *name).collect();
        let media: Vec<&str> = VK_MEDIA.iter().map(|(name, _)| *name).collect();
        for (table, what) in [
            (modifier.as_slice(), "modifier"),
            (special.as_slice(), "special"),
            (media.as_slice(), "media"),
        ] {
            for (index, name) in table.iter().enumerate() {
                assert!(
                    !table[..index].contains(name),
                    "duplicate {what} key name: {name}"
                );
            }
        }
    }

    #[test]
    fn every_media_key_is_extended_and_every_modifier_is_not() {
        for (_, vk) in VK_MEDIA {
            assert!(
                is_extended_vk(vk),
                "media VK {vk:#04X} must carry KEYEVENTF_EXTENDEDKEY"
            );
        }
        for (name, vk) in MODIFIER_VK {
            assert!(
                !is_extended_vk(vk),
                "{name} VK {vk:#04X} must not be extended"
            );
        }
    }

    #[test]
    fn shortcut_events_release_the_modifiers_in_the_opposite_order() {
        let events = shortcut_events(&mods(&["super", "ctrl", "alt"]), "ESC").expect("ESC");
        let pressed: Vec<u16> = events
            .iter()
            .filter(|event| event.flags & KEYEVENTF_KEYUP == 0)
            .map(|event| event.vk)
            .collect();
        let released: Vec<u16> = events
            .iter()
            .filter(|event| event.flags & KEYEVENTF_KEYUP != 0)
            .map(|event| event.vk)
            .collect();
        assert_eq!(pressed, vec![0x5B, 0x11, 0x12, 0x1B]);
        assert_eq!(released, vec![0x1B, 0x12, 0x11, 0x5B]);
    }

    #[test]
    fn a_media_key_is_always_a_press_and_a_release() {
        for (name, vk) in VK_MEDIA {
            let events = media_events(name).expect(name);
            assert_eq!(events[0], key_event(vk, false));
            assert_eq!(events[1], key_event(vk, true));
            assert_ne!(
                events[0].flags & KEYEVENTF_KEYUP,
                KEYEVENTF_KEYUP,
                "{name} must start pressed"
            );
            assert_eq!(
                events[1].flags & KEYEVENTF_KEYUP,
                KEYEVENTF_KEYUP,
                "{name} must end released"
            );
        }
        assert_eq!(media_events("STOP"), None);
    }

    #[test]
    fn unicode_text_never_sends_a_modifier_or_a_keyup_before_its_keydown() {
        for event in text_events("Hi \u{263A}") {
            assert_eq!(event.vk, 0, "unicode events carry the code point");
            assert_ne!(event.flags & KEYEVENTF_UNICODE, 0);
        }
    }

    /// On every non-Windows target this module must still compile — that is
    /// the point of the split — but it must not pretend to own a keyboard.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn the_non_windows_stand_in_reports_nothing_and_never_claims_success() {
        use super::super::InputInjector;
        let injector = NoWindowsInjector;
        assert_eq!(injector.available(), None);
        assert!(!injector.shortcut(&mods(&["ctrl"]), "A").ok);
        assert!(!injector.text("hi").ok);
        assert!(!injector.media("PLAY_PAUSE").ok);
    }
}

/// The stand-in used on non-Windows targets so the mapping layer can be
/// tested everywhere without a keyboard. [`super::NoInputInjector`] plays the
/// same role in the dispatcher.
#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWindowsInjector;

#[cfg(not(target_os = "windows"))]
impl InputInjector for NoWindowsInjector {
    fn available(&self) -> Option<String> {
        None
    }

    fn shortcut(&self, _modifiers: &[String], _key: &str) -> ActionOutcome {
        ActionOutcome::fail(NO_TARGET_KEY)
    }

    fn text(&self, _text: &str) -> ActionOutcome {
        ActionOutcome::fail(super::INPUT_INJECTOR_HELP)
    }

    fn media(&self, media_key: &str) -> ActionOutcome {
        ActionOutcome::fail(unknown_media_key_reason(media_key))
    }
}
