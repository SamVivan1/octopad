"""Configuration logic shared by the Linux and Windows applications."""

import pytest


@pytest.fixture(params=["linux_config", "windows_config"])
def cfg(request):
    return request.getfixturevalue(request.param)


def test_timing_command_formats_values(cfg):
    assert cfg.timing_command({"debounceMs": 20, "clickTimeoutMs": 250,
                               "holdTimeoutMs": 500}) == "TIMING:20,250,500"


def test_timing_command_uses_defaults(cfg):
    assert cfg.timing_command({}) == "TIMING:25,250,450"


def test_timing_command_rejects_garbage(cfg):
    assert cfg.timing_command({"debounceMs": "nope"}) is None


def test_default_config_shape(cfg):
    data = cfg.default_config()
    assert data["schemaVersion"] == cfg.CURRENT_SCHEMA_VERSION
    assert len(data["modes"]) == 2
    for mode in data["modes"]:
        assert len(mode["buttons"]) == 8


def test_default_timings(cfg):
    data = cfg.default_config()
    assert (data["debounceMs"], data["clickTimeoutMs"], data["holdTimeoutMs"]) == (25, 250, 450)


def test_actions_list_normalizes(cfg):
    assert cfg.actions_list(None) == []
    assert cfg.actions_list({"type": "none"}) == [{"type": "none"}]
    assert cfg.actions_list([{"type": "text"}, "bad", {"x": 1}]) == [
        {"type": "text"}, {"x": 1}]


def test_normalize_config_accepts_default(cfg):
    ok, _msg = cfg.normalize_config(cfg.default_config())
    assert ok


def test_normalize_config_rejects_invalid(cfg):
    ok, _msg = cfg.normalize_config({"modes": []})
    assert not ok

    ok2, _msg2 = cfg.normalize_config("not a dict")
    assert not ok2


def test_migrate_stamps_schema_version(cfg):
    migrated = cfg.migrate_config({})
    assert migrated["schemaVersion"] == cfg.CURRENT_SCHEMA_VERSION


def test_migrate_preserves_data(cfg):
    migrated = cfg.migrate_config({"schemaVersion": 0, "device": "TestPad"})
    assert migrated["device"] == "TestPad"
    assert migrated["schemaVersion"] == cfg.CURRENT_SCHEMA_VERSION


def test_migrate_non_dict_is_passthrough(cfg):
    assert cfg.migrate_config(None) is None


def test_default_config_has_input_link(cfg):
    assert cfg.default_config()["inputLink"] == "auto"


def test_default_config_has_no_legacy_version_field(cfg):
    assert "version" not in cfg.default_config()


def test_migrate_drops_the_legacy_version_field(cfg):
    migrated = cfg.migrate_config({"schemaVersion": 1, "version": 2.6})
    assert "version" not in migrated
    assert migrated["schemaVersion"] == cfg.CURRENT_SCHEMA_VERSION


def test_migrate_adds_the_default_input_link(cfg):
    assert cfg.migrate_config({"schemaVersion": 1})["inputLink"] == "auto"


def test_migrate_keeps_a_chosen_input_link(cfg):
    migrated = cfg.migrate_config({"schemaVersion": 1, "inputLink": "bluetooth"})
    assert migrated["inputLink"] == "bluetooth"


def test_migrate_rejects_an_unknown_input_link(cfg):
    migrated = cfg.migrate_config({"schemaVersion": 1, "inputLink": "carrier-pigeon"})
    assert migrated["inputLink"] == "auto"


def test_default_config_ships_no_embedded_passcode(cfg):
    import json

    assert "031004" not in json.dumps(cfg.default_config())


def test_preset_slug(cfg):
    assert cfg.preset_slug("My Profile!!") == "my-profile"
    assert cfg.preset_slug("") == "preset"
