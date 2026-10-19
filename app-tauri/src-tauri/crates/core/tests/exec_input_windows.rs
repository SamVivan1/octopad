//! Task 5 (M3.1): Windows keyboard/text/media injection.
//!
//! Port of `windows-app/platform_actions.py:62-190`: `_MODIFIER_VK`,
//! `_VK_SPECIAL`, `_EXTENDED_VK`, `_VK_MEDIA`, `_vk_for`, `_key_event`,
//! `_type_unicode_char`, `_send_text`, `_send_shortcut` and `_media_key`.
//!
//! Two layers are asserted here:
//!
//! * **The mapping layer** (key → virtual key, modifier list → press/release
//!   order, media key → VK) is pure and deliberately compiled on *every*
//!   target, so this file's first section runs on the ubuntu CI job too. Every
//!   table and every reason string is compared with the Python source value by
//!   value, not by eyeball.
//! * **The `SendInput` layer** only exists on Windows and is gated behind
//!   `#[cfg(target_os = "windows")]`. Its tests only exercise the paths that
//!   fail *before* anything is injected — the ones that would otherwise type
//!   into the machine running the suite. The bytes that reach `SendInput` are
//!   the [`KeyEvent`] plans the portable section already pins, so the gated
//!   tests assert the wiring, not the keystrokes.

use octopad_core::exec::input_windows::*;

// ---------------------------------------------------------------------------
// The mapping layer — runs on every target, including Linux CI.
// ---------------------------------------------------------------------------

fn mods(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

/// `_MODIFIER_VK`, `platform_actions.py:69`.
#[test]
fn the_modifier_map_matches_python() {
    assert_eq!(MODIFIER_VK.len(), 4);
    assert_eq!(modifier_vk("ctrl"), Some(0x11));
    assert_eq!(modifier_vk("alt"), Some(0x12));
    assert_eq!(modifier_vk("shift"), Some(0x10));
    assert_eq!(modifier_vk("super"), Some(0x5B));
    assert_eq!(modifier_vk("win"), None, "the Python dict has no 'win'");
    assert_eq!(modifier_vk("Control"), None, "keys are not case folded");
    assert_eq!(modifier_vk(""), None);
}

/// `[MODIFIER_VK[m] for m in modifiers if m in _MODIFIER_VK]` — the config
/// order is kept (Windows does *not* normalise to ctrl/alt/shift/super the way
/// the Linux tools do), unknown names are dropped and duplicates survive.
#[test]
fn the_modifier_sequence_keeps_the_config_order_and_drops_unknowns() {
    assert_eq!(
        modifier_sequence(&mods(&["super", "ctrl"])),
        vec![0x5B, 0x11]
    );
    assert_eq!(
        modifier_sequence(&mods(&["shift", "nope", "alt"])),
        vec![0x10, 0x12]
    );
    assert_eq!(
        modifier_sequence(&mods(&["ctrl", "ctrl"])),
        vec![0x11, 0x11],
        "the Python comprehension keeps repeats"
    );
    assert_eq!(modifier_sequence(&mods(&["hyper"])), Vec::<u16>::new());
    assert_eq!(modifier_sequence(&[]), Vec::<u16>::new());
}

/// `_VK_SPECIAL`, `platform_actions.py:70`.
#[test]
fn the_special_key_map_matches_python() {
    for (name, vk) in [
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
    ] {
        assert_eq!(vk_special(name), Some(vk), "for {name}");
        assert_eq!(vk_for(name), Some(vk), "_vk_for upper-cases first");
    }
    assert_eq!(VK_SPECIAL.len(), 14);
    assert_eq!(vk_special("CAPSLOCK"), None);
}

/// `_vk_for` — `platform_actions.py:139`.
#[test]
fn vk_for_maps_letters_digits_function_keys_and_specials() {
    assert_eq!(vk_for("A"), Some(0x41));
    assert_eq!(vk_for("z"), Some(0x5A), "the name is upper-cased first");
    assert_eq!(vk_for("0"), Some(0x30));
    assert_eq!(vk_for("9"), Some(0x39));
    assert_eq!(vk_for("F1"), Some(0x70));
    assert_eq!(vk_for("f24"), Some(0x87));
    assert_eq!(vk_for("ENTER"), Some(0x0D));
}

/// Everything `_vk_for` cannot express comes back as `None`, which is what
/// turns into `Target key not selected yet`.
#[test]
fn vk_for_rejects_everything_the_python_table_cannot_express() {
    assert_eq!(vk_for(""), None);
    assert_eq!(vk_for("NOPE"), None);
    assert_eq!(vk_for("space"), Some(0x20), "_vk_for upper-cases first");
    assert_eq!(vk_for("Caps_Lock"), None, "not in _VK_SPECIAL");
    assert_eq!(vk_for("F0"), None, "1 <= number <= 24");
    assert_eq!(vk_for("F25"), None);
    assert_eq!(
        vk_for("F"),
        Some(0x46),
        "the letter branch runs first in Python"
    );
    assert_eq!(vk_for("FX"), None);
    assert_eq!(vk_for("!"), None, "punctuation has no VK here");
    assert_eq!(vk_for("é"), None, "a letter outside A-Z is not a VK");
    assert_eq!(
        vk_for("ß"),
        None,
        "Python upper-cases it to \"SS\", also unmapped"
    );
}

/// `_EXTENDED_VK`, `platform_actions.py:75`. The extended flag is what tells
/// the OS this is the cursor/navigation block and not the numpad.
#[test]
fn the_extended_key_table_matches_python() {
    assert_eq!(EXTENDED_VK.len(), 16);
    for vk in [
        0x26, 0x28, 0x25, 0x27, // arrows
        0x24, 0x23, 0x21, 0x22, 0x2E, // home/end/pgup/pgdn/delete
        0xAD, 0xAE, 0xAF, 0xB0, 0xB1, 0xB2, 0xB3, // media + numpad enter
    ] {
        assert!(is_extended_vk(vk), "{vk:#04X} must be extended");
    }
    for vk in [
        0x0D, 0x1B, 0x08, 0x09, 0x20, 0x41, 0x70, 0x11, 0x12, 0x10, 0x5B,
    ] {
        assert!(!is_extended_vk(vk), "{vk:#04X} must not be extended");
    }
}

/// `_VK_MEDIA`, `platform_actions.py:79`.
#[test]
fn the_media_key_table_matches_python() {
    assert_eq!(VK_MEDIA.len(), 6);
    assert_eq!(media_vk("MUTE"), Some(0xAD));
    assert_eq!(media_vk("VOL_UP"), Some(0xAF));
    assert_eq!(media_vk("VOL_DOWN"), Some(0xAE));
    assert_eq!(media_vk("PLAY_PAUSE"), Some(0xB3));
    assert_eq!(media_vk("NEXT_TRACK"), Some(0xB0));
    assert_eq!(media_vk("PREV_TRACK"), Some(0xB1));
    assert_eq!(media_vk("mute"), None, "media ids are not case folded");
    assert_eq!(media_vk("STOP"), None);
    assert_eq!(media_vk(""), None);
}

/// `_key_event`, `platform_actions.py:119`: KEYUP plus, for the extended
/// block, KEYEVENTF_EXTENDEDKEY.
#[test]
fn key_event_flags_match_the_python_combination() {
    assert_eq!(key_event(0x41, false).flags, 0);
    assert_eq!(key_event(0x41, true).flags, KEYEVENTF_KEYUP);
    assert_eq!(
        key_event(0x25, false).flags,
        KEYEVENTF_EXTENDEDKEY,
        "the arrow keys are extended even going down"
    );
    assert_eq!(
        key_event(0xAF, true).flags,
        KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
    );
    assert_eq!(key_event(0x0D, false).vk, 0x0D);
    assert_eq!(key_event(0x0D, false).scan, 0, "a VK event carries no scan");
}

/// `_type_unicode_char`, `platform_actions.py:120`: `wVk = 0`, the code point
/// goes into `wScan` and the pair is down-then-up with UNICODE set on both.
#[test]
fn unicode_events_are_a_unicode_down_up_pair() {
    let [down, up] = unicode_events('A');
    assert_eq!(down, KeyEvent::new(0, 0x41, KEYEVENTF_UNICODE));
    assert_eq!(
        up,
        KeyEvent::new(0, 0x41, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)
    );
    let [down, _] = unicode_events('é');
    assert_eq!(down.scan, 0xE9);
    // ctypes' `wintypes.WORD` keeps 16 bits, so a code point above the BMP is
    // truncated exactly like the Python app truncates it.
    let [down, up] = unicode_events('\u{1F600}');
    assert_eq!(down.scan, 0xF600);
    assert_eq!(up.scan, 0xF600);
}

/// `_send_shortcut` builds: every modifier down, the key down, the key up,
/// then the modifiers up in **reverse** order.
#[test]
fn shortcut_events_press_then_release_in_reverse() {
    let plan = shortcut_events(&mods(&["ctrl", "alt", "shift"]), "t").expect("t is a key");
    let vks: Vec<u16> = plan.iter().map(|event| event.vk).collect();
    assert_eq!(vks, vec![0x11, 0x12, 0x10, 0x54, 0x54, 0x10, 0x12, 0x11]);
    let ups: Vec<u32> = plan.iter().map(|event| event.flags).collect();
    assert_eq!(
        ups,
        vec![
            0,
            0,
            0,
            0,
            KEYEVENTF_KEYUP,
            KEYEVENTF_KEYUP,
            KEYEVENTF_KEYUP,
            KEYEVENTF_KEYUP
        ],
        "only the second half releases"
    );
    // The two halves mirror each other, so the key can never stay stuck down.
    assert_eq!(plan.len(), 2 * (3 + 1));
}

/// A shortcut with no modifiers is just the key, and an extended key carries
/// the extended flag in both directions.
#[test]
fn shortcut_events_handle_bare_and_extended_keys() {
    let plan = shortcut_events(&[], "ENTER").expect("ENTER");
    assert_eq!(
        plan,
        vec![
            KeyEvent::new(0x0D, 0, 0),
            KeyEvent::new(0x0D, 0, KEYEVENTF_KEYUP),
        ]
    );
    let plan = shortcut_events(&mods(&["super"]), "LEFT").expect("LEFT");
    assert_eq!(
        plan,
        vec![
            KeyEvent::new(0x5B, 0, 0),
            KeyEvent::new(0x25, 0, KEYEVENTF_EXTENDEDKEY),
            KeyEvent::new(0x25, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
            KeyEvent::new(0x5B, 0, KEYEVENTF_KEYUP),
        ]
    );
}

/// No virtual key for the configured key → no events at all, which is what
/// the caller turns into `Target key not selected yet`.
#[test]
fn shortcut_events_are_absent_without_a_target_key() {
    assert_eq!(shortcut_events(&mods(&["ctrl"]), ""), None);
    assert_eq!(shortcut_events(&[], "NOPE"), None);
    assert_eq!(shortcut_events(&mods(&["ctrl"]), "F25"), None);
}

/// `_send_text`, `platform_actions.py:151`: `\n` and `\t` go out as VK taps,
/// everything else as a unicode pair.
#[test]
fn text_events_unicode_everything_but_newline_and_tab() {
    assert_eq!(text_events(""), Vec::<KeyEvent>::new());
    assert_eq!(text_events("A"), unicode_events('A').to_vec());
    let plan = text_events("a\n\t");
    assert_eq!(
        plan,
        vec![
            KeyEvent::new(0, 0x61, KEYEVENTF_UNICODE),
            KeyEvent::new(0, 0x61, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            key_event(0x0D, false),
            key_event(0x0D, true),
            key_event(0x09, false),
            key_event(0x09, true),
        ]
    );
    assert_eq!(text_events("hi").len(), 4, "two INPUTs per character");
}

/// The reason strings the console prints are the Python ones, character for
/// character — the dispatcher and the tests pin them.
#[test]
fn the_reason_strings_are_the_python_ones() {
    assert_eq!(NO_TARGET_KEY, "Target key not selected yet");
    assert_eq!(TEXT_TYPED, "Text typed");
    assert_eq!(SHORTCUT_SENT, "Shortcut sent");
    assert_eq!(
        unknown_media_key_reason("STOP"),
        "Unknown media key: STOP",
        "f-string parity, including the empty key"
    );
    assert_eq!(unknown_media_key_reason(""), "Unknown media key: ");
    assert_eq!(
        media_key_sent_reason("PLAY_PAUSE"),
        "Media key PLAY_PAUSE sent"
    );
}

// ---------------------------------------------------------------------------
// The SendInput layer — Windows only. These assertions never inject a
// keystroke: they cover the branches that fail before any INPUT is built, so
// running them on a developer machine cannot type into it.
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod sendinput {
    use super::*;
    use octopad_core::exec::{
        platform_input_injector, ActionOutcome, InputInjector, RealCommandRunner,
    };

    fn injector() -> WindowsInjector {
        WindowsInjector::new()
    }

    #[test]
    fn the_injector_is_available_because_sendinput_is_part_of_user32() {
        assert_eq!(
            injector().available().as_deref(),
            Some("SendInput"),
            "there is nothing to detect on Windows"
        );
        let runner = RealCommandRunner::new();
        let platform = platform_input_injector(&runner);
        assert_eq!(platform.available().as_deref(), Some("SendInput"));
    }

    #[test]
    fn a_shortcut_without_a_target_key_fails_with_the_python_reason() {
        assert_eq!(
            injector().shortcut(&mods(&["ctrl"]), ""),
            ActionOutcome::fail(NO_TARGET_KEY)
        );
        assert_eq!(
            injector().shortcut(&[], "NOPE"),
            ActionOutcome::fail("Target key not selected yet")
        );
    }

    #[test]
    fn an_unknown_media_key_fails_with_the_python_reason() {
        assert_eq!(
            injector().media("STOP"),
            ActionOutcome::fail("Unknown media key: STOP")
        );
        assert_eq!(
            injector().media(""),
            ActionOutcome::fail("Unknown media key: ")
        );
    }

    #[test]
    fn empty_text_keeps_the_python_success_because_it_injects_nothing() {
        assert_eq!(injector().text(""), ActionOutcome::ok("Text typed"));
    }
}
