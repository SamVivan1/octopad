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
