"""Firmware line parsers (USB serial / BLE) and the TIMING sync command."""


def test_parse_event_desktop_double(serial_listener):
    parsed = serial_listener.parse_event_line(
        "[DESKTOP] Button 3 -> DOUBLE (LED Feedback Processed)")
    assert parsed == {"mode": "desktop", "index": 2, "trigger": "double"}


def test_parse_event_ha_hold(serial_listener):
    parsed = serial_listener.parse_event_line("[HA] Button 8 -> HOLD")
    assert parsed == {"mode": "ha", "index": 7, "trigger": "hold"}


def test_parse_event_ignores_unrelated(serial_listener):
    assert serial_listener.parse_event_line("MODE: HOME ASSISTANT") is None
    assert serial_listener.parse_event_line("") is None


def test_parse_hello(serial_listener):
    assert serial_listener.parse_hello_line("HELLO:OCTOPAD:My Pad:v2.6") == "My Pad"


def test_parse_hello_rejects_other_brand(serial_listener):
    assert serial_listener.parse_hello_line("HELLO:OTHER:Pad:v1") is None


def test_parse_hello_defaults_when_unnamed(serial_listener):
    assert serial_listener.parse_hello_line("HELLO:OCTOPAD::v2.6") == "OctoPad"


def test_parse_hello_extracts_version(serial_listener):
    info = serial_listener.parse_hello("HELLO:OCTOPAD:My Pad:v2.6")
    assert info == {"name": "My Pad", "version": "2.6"}


def test_parse_hello_without_version(serial_listener):
    info = serial_listener.parse_hello("HELLO:OCTOPAD:My Pad")
    assert info == {"name": "My Pad", "version": ""}


def test_parse_hello_bad_brand(serial_listener):
    assert serial_listener.parse_hello("HELLO:OTHER:Pad:v1") is None


def test_sync_timings_formats_command(serial_listener):
    listener = serial_listener.SerialDaemonListener()
    listener.config = {
        "debounceMs": 10,
        "clickTimeoutMs": 300,
        "holdTimeoutMs": 500,
    }
    sent = []

    def fake_send(command, log=True):
        sent.append(command)
        return True

    listener.send_command = fake_send
    assert listener.sync_timings() is True
    assert sent == ["TIMING:10,300,500"]


def test_sync_timings_rejects_bad_values(serial_listener):
    listener = serial_listener.SerialDaemonListener()
    listener.config = {"debounceMs": "not-a-number"}
    sent = []
    listener.send_command = lambda command, log=True: sent.append(command) or True
    assert listener.sync_timings() is False
    assert sent == []
