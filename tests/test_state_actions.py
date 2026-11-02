"""Syncing the standalone action table to the pad over the USB cable."""

import config as config_module


class FakeTransport:
    def __init__(self, ok: bool = True):
        self.ok = ok
        self.commands = []

    def send_command(self, command: str, log: bool = True) -> bool:
        self.commands.append(command)
        return self.ok


def test_sync_firmware_actions_sends_the_whole_table_to_serial(linux_state):
    serial = FakeTransport()
    ble = FakeTransport()
    linux_state.listener = serial
    linux_state.ble = ble
    try:
        assert linux_state.sync_firmware_actions() is True
    finally:
        linux_state.listener = None
        linux_state.ble = None

    assert serial.commands == config_module.firmware_actions(linux_state.cfg)
    assert ble.commands == []


def test_sync_firmware_actions_needs_the_cable(linux_state):
    ble = FakeTransport()
    linux_state.listener = None
    linux_state.ble = ble
    try:
        assert linux_state.sync_firmware_actions() is False
    finally:
        linux_state.ble = None

    assert ble.commands == []


def test_sync_firmware_actions_reports_a_failed_write(linux_state):
    serial = FakeTransport(ok=False)
    linux_state.listener = serial
    try:
        assert linux_state.sync_firmware_actions() is False
    finally:
        linux_state.listener = None

    assert serial.commands == ["ACTCLR"]


def test_ping_pad_uses_whichever_transport_is_connected(linux_state):
    serial = FakeTransport(ok=False)
    ble = FakeTransport(ok=True)
    linux_state.listener = serial
    linux_state.ble = ble
    try:
        assert linux_state.ping_pad() is True
    finally:
        linux_state.listener = None
        linux_state.ble = None

    assert serial.commands == ["PING"]
    assert ble.commands == ["PING"]


def test_connecting_does_not_sync_before_the_pad_finished_booting(linux_state):
    serial = FakeTransport()
    linux_state.listener = serial
    linux_state.serial_manually_disconnected = False
    try:
        linux_state.set_serial_status(True, "/dev/ttyACM0")
    finally:
        linux_state.listener = None
        linux_state.set_serial_status(False, None)

    assert not [line for line in serial.commands if line.startswith("ACT")]


def test_sync_paces_the_burst_for_the_usb_buffer(linux_state, monkeypatch):
    import state as state_module

    sleeps = []
    monkeypatch.setattr(state_module.time, "sleep",
                        lambda seconds: sleeps.append(seconds))
    serial = FakeTransport()
    linux_state.listener = serial
    try:
        assert linux_state.sync_firmware_actions() is True
    finally:
        linux_state.listener = None

    assert len(sleeps) >= len(serial.commands) - 1
    assert all(0 < seconds <= 0.05 for seconds in sleeps)
