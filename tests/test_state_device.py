"""Device-command routing between the USB serial and BLE transports."""


class FakeTransport:
    def __init__(self, ok: bool = True):
        self.ok = ok
        self.commands = []

    def send_command(self, command: str, log: bool = True) -> bool:
        self.commands.append(command)
        return self.ok


def test_send_device_command_prefers_serial(linux_state):
    serial = FakeTransport(True)
    ble = FakeTransport(True)
    linux_state.listener = serial
    linux_state.ble = ble

    assert linux_state.send_device_command("WHO") is True
    assert serial.commands == ["WHO"]
    assert ble.commands == []


def test_send_device_command_falls_back_to_ble(linux_state):
    serial = FakeTransport(False)
    ble = FakeTransport(True)
    linux_state.listener = serial
    linux_state.ble = ble

    assert linux_state.send_device_command("WHO") is True
    assert serial.commands == ["WHO"]
    assert ble.commands == ["WHO"]


def test_send_device_command_without_transport(linux_state):
    linux_state.listener = None
    linux_state.ble = None
    assert linux_state.send_device_command("WHO") is False


def test_rename_device_via_transport(linux_state):
    ble = FakeTransport(True)
    linux_state.listener = None
    linux_state.ble = ble

    assert linux_state.rename_device("  My Pad  ") is True
    assert ble.commands == ["NAME:My Pad"]


def test_rename_device_rejects_invalid_names(linux_state):
    linux_state.listener = None
    linux_state.ble = FakeTransport(True)

    assert linux_state.rename_device("") is False
    assert linux_state.rename_device("x" * 25) is False
