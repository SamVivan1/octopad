"""Releasing the USB port for flashing without killing the BLE link."""

import pytest


class FakeListener:
    def __init__(self):
        self.started = 0
        self.stopped = 0

    def start(self):
        self.started += 1

    def stop(self):
        self.stopped += 1


@pytest.fixture
def clean_state(linux_state):
    linux_state.serial_manually_disconnected = False
    linux_state.listener = None
    yield linux_state
    linux_state.listener = None
    linux_state.serial_manually_disconnected = False


def test_disconnect_serial_releases_the_port(clean_state):
    listener = FakeListener()
    clean_state.listener = listener

    assert clean_state.disconnect_serial() is True

    assert listener.stopped == 1
    assert clean_state.serial_manually_disconnected is True
    assert clean_state.serial_connected is False


def test_reconnect_serial_starts_the_listener_again(clean_state):
    listener = FakeListener()
    clean_state.listener = listener
    clean_state.disconnect_serial()

    assert clean_state.reconnect_serial() is True

    assert listener.started == 1
    assert clean_state.serial_manually_disconnected is False


def test_disconnect_without_a_listener_is_safe(clean_state):
    assert clean_state.disconnect_serial() is True
    assert clean_state.serial_manually_disconnected is True

    assert clean_state.reconnect_serial() is True
    assert clean_state.serial_manually_disconnected is False


def test_release_notifies_the_ui_and_tray(clean_state):
    events = []
    clean_state.subscribe(lambda event, **data: events.append((event, data)))
    try:
        clean_state.disconnect_serial()
        clean_state.reconnect_serial()
    finally:
        clean_state.unsubscribe(clean_state._subscribers[-1])

    names = [event for event, _data in events]
    assert "serial-manual-changed" in names
    assert [data.get("released") for event, data in events
            if event == "serial-manual-changed"] == [True, False]
