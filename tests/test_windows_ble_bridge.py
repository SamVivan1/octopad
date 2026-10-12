"""Device identification for the Windows BLE bridge (bleak version shapes)."""

from types import SimpleNamespace


class FakeDevice:
    def __init__(self, name=None, address="AA:BB:CC:DD:EE:FF",
                 metadata=None, details=None):
        self.name = name
        self.address = address
        if metadata is not None:
            self.metadata = metadata
        if details is not None:
            self.details = details


def test_uuids_from_metadata(ble_bridge):
    device = FakeDevice(metadata={"uuids": ["6F63746F-7061-4430-8001-000000000001"]})
    assert ble_bridge._device_uuids(device) == {"6f63746f-7061-4430-8001-000000000001"}


def test_uuids_from_bluez_details(ble_bridge):
    device = FakeDevice(details={"path": "/org/bluez/...", "props": {"UUIDs": [
        "0000fcf1-0000-1000-8000-00805f9b34fb",
        "6f63746f-7061-4430-8001-000000000001"]}})
    assert ble_bridge._device_uuids(device) == {
        "0000fcf1-0000-1000-8000-00805f9b34fb",
        "6f63746f-7061-4430-8001-000000000001",
    }


def test_uuids_from_winrt_details(ble_bridge):
    device = FakeDevice(details=SimpleNamespace(
        service_uuids=["6F63746F-7061-4430-8001-000000000001"]))
    assert ble_bridge._device_uuids(device) == {"6f63746f-7061-4430-8001-000000000001"}


def test_uuids_empty_when_no_advertisement(ble_bridge):
    assert ble_bridge._device_uuids(FakeDevice(details={"props": {}})) == set()


def test_is_pad_matches_service_uuid(ble_bridge):
    target = {"6f63746f-7061-4430-8001-000000000001"}
    device = FakeDevice(name="Renamed Pad", details={
        "props": {"UUIDs": ["6f63746f-7061-4430-8001-000000000001"]}})
    assert ble_bridge._is_pad(device, target)


def test_is_pad_matches_default_name(ble_bridge):
    assert ble_bridge._is_pad(FakeDevice(name="octopad"), set())


def test_is_pad_rejects_unknown_device(ble_bridge):
    assert not ble_bridge._is_pad(FakeDevice(name="Neighbour's Keyboard"), set())


def test_rejects_device_without_details(ble_bridge):
    assert not ble_bridge._is_pad(FakeDevice(name=None), set())
