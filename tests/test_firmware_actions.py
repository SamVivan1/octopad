"""Compact action payload the pad stores for standalone (HID) use."""

import json

import pytest


@pytest.fixture(params=["linux_config", "windows_config"])
def cfg(request):
    return request.getfixturevalue(request.param)


def test_volume_preset_maps_to_a_consumer_action(cfg):
    action = {"type": "system_action", "presetId": "volume_up_5"}
    assert cfg.firmware_action(0, 4, 0, action) == "ACT:0,4,0,3,0,0,"


def test_volume_down_and_mute_map_to_consumer_actions(cfg):
    assert cfg.firmware_action(0, 5, 0, {"type": "system_action",
                                         "presetId": "volume_down_5"}) == "ACT:0,5,0,4,0,0,"
    assert cfg.firmware_action(0, 3, 0, {"type": "system_action",
                                         "presetId": "volume_toggle_mute"}) == "ACT:0,3,0,5,0,0,"


def test_text_is_hex_encoded(cfg):
    assert cfg.firmware_action(0, 0, 2, {"type": "text", "text": "Hi"}) == "ACT:0,0,2,1,0,0,4869"


def test_non_ascii_text_falls_back_to_app_only(cfg):
    action = {"type": "text", "text": "café"}
    assert cfg.firmware_action(0, 0, 2, action) == "ACT:0,0,2,9,0,0,"


def test_text_longer_than_the_firmware_buffer_is_app_only(cfg):
    action = {"type": "text", "text": "x" * (cfg.FIRMWARE_TEXT_MAX + 1)}
    assert cfg.firmware_action(0, 0, 2, action) == "ACT:0,0,2,9,0,0,"


def test_shortcut_maps_modifiers_and_key(cfg):
    action = {"type": "shortcut", "modifiers": ["ctrl", "shift"], "key": "A"}
    assert cfg.firmware_action(0, 1, 0, action) == "ACT:0,1,0,2,3,4,"


def test_super_modifier_maps_to_gui(cfg):
    action = {"type": "shortcut", "modifiers": ["super"], "key": "L"}
    assert cfg.firmware_action(0, 1, 0, action) == "ACT:0,1,0,2,8,15,"


def test_every_editor_key_has_an_hid_code(cfg):
    keys = [key for _group, group_keys in cfg.KEY_GROUPS for key in group_keys]
    for key in keys:
        action = {"type": "shortcut", "modifiers": [], "key": key}
        assert cfg.firmware_action(0, 0, 0, action).split(",")[3] == "2"


def test_unknown_shortcut_key_is_app_only(cfg):
    action = {"type": "shortcut", "modifiers": ["ctrl"], "key": "NOT_A_KEY"}
    assert cfg.firmware_action(0, 0, 0, action) == "ACT:0,0,0,9,0,0,"


def test_system_presets_map_to_hid_actions(cfg):
    cases = {
        "screenshot_interactive": 6,
        "lock_screen": 7,
        "open_terminal": 8,
        "media_play_pause": 11,
        "media_next": 12,
        "media_prev": 13,
    }
    for preset, expected in cases.items():
        action = {"type": "system_action", "presetId": preset}
        assert cfg.firmware_action(0, 0, 0, action).split(",")[3] == str(expected)


def test_mic_mute_falls_back_to_app_only(cfg):
    action = {"type": "system_action", "presetId": "mic_toggle_mute"}
    assert cfg.firmware_action(0, 0, 0, action) == "ACT:0,0,0,9,0,0,"


def test_home_assistant_is_app_only(cfg):
    action = {"type": "home_assistant", "entityId": "light.desk"}
    assert cfg.firmware_action(0, 0, 0, action) == "ACT:0,0,0,10,0,0,"


def test_mode_toggle_is_handled_by_the_firmware_itself(cfg):
    assert cfg.firmware_action(0, 0, 0, {"type": "mode_toggle"}) == "ACT:0,0,0,0,0,0,"


def test_none_and_missing_actions_are_empty(cfg):
    assert cfg.firmware_action(0, 0, 0, {"type": "none"}) == "ACT:0,0,0,0,0,0,"
    assert cfg.firmware_action(0, 0, 0, None) == "ACT:0,0,0,0,0,0,"


def test_unknown_action_type_is_app_only(cfg):
    assert cfg.firmware_action(0, 0, 0, {"type": "delay", "seconds": 1}) == "ACT:0,0,0,9,0,0,"


def test_firmware_actions_cover_every_slot(cfg):
    config = cfg.default_config()
    lines = cfg.firmware_actions(config)

    assert lines[0] == "ACTCLR"
    assert lines[-1] == "ACTSAVE"
    assert len(lines) == 2 + 2 * 8 * 3
    assert all(line.startswith("ACT:") for line in lines[1:-1])


def test_firmware_actions_reflect_the_default_volume_key(cfg):
    lines = cfg.firmware_actions(cfg.default_config())
    # Desktop mode, button 5 (index 4), single click = volume up.
    assert "ACT:0,4,0,3,0,0," in lines


def test_firmware_actions_do_not_contain_a_passcode(cfg):
    lines = cfg.firmware_actions(cfg.default_config())
    assert "031004" not in json.dumps(lines)


def test_media_action_type_maps_to_consumer_types(cfg):
    cases = {
        "MUTE": 5, "VOL_UP": 3, "VOL_DOWN": 4,
        "PLAY_PAUSE": 11, "NEXT_TRACK": 12, "PREV_TRACK": 13,
    }
    for media_key, expected in cases.items():
        action = {"type": "media", "mediaKey": media_key}
        assert cfg.firmware_action(0, 0, 0, action).split(",")[3] == str(expected)


def test_multi_step_sequence_is_app_only(cfg):
    sequence = [
        {"type": "text", "text": "a"},
        {"type": "delay", "seconds": 0.5},
    ]
    assert cfg.firmware_action(0, 0, 0, sequence) == "ACT:0,0,0,9,0,0,"
