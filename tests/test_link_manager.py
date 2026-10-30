"""Input-link arbitration: wired wins in auto, manual override, cross-link dedupe."""

import pytest

from link_manager import LinkManager

WIRED = "wired"
BLUETOOTH = "bluetooth"


class FakeClock:
    def __init__(self) -> None:
        self.now = 1000.0

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float) -> None:
        self.now += seconds


@pytest.fixture
def clock():
    return FakeClock()


def make_manager(clock, policy="auto"):
    return LinkManager(policy=policy, dedupe_window=0.3, clock=clock)


EVENT = {"mode": "desktop", "index": 1, "trigger": "single"}


def test_auto_prefers_wired_when_both_links_connect(clock):
    manager = make_manager(clock)
    manager.set_connected(BLUETOOTH, True)
    manager.set_connected(WIRED, True)

    assert manager.active_link == WIRED
    assert manager.allows(WIRED, EVENT)
    assert not manager.allows(BLUETOOTH, EVENT)


def test_auto_falls_back_to_bluetooth_when_cable_is_unplugged(clock):
    manager = make_manager(clock)
    manager.set_connected(WIRED, True)
    manager.set_connected(BLUETOOTH, True)
    assert manager.allows(WIRED, EVENT)

    manager.set_connected(WIRED, False)

    assert manager.active_link == BLUETOOTH
    assert manager.allows(BLUETOOTH, {"mode": "desktop", "index": 2, "trigger": "single"})


def test_manual_wired_ignores_bluetooth_events(clock):
    manager = make_manager(clock, policy=WIRED)
    manager.set_connected(BLUETOOTH, True)
    manager.set_connected(WIRED, True)

    assert not manager.allows(BLUETOOTH, EVENT)
    assert manager.allows(WIRED, EVENT)


def test_manual_bluetooth_ignores_wired_events(clock):
    manager = make_manager(clock, policy=BLUETOOTH)
    manager.set_connected(WIRED, True)
    manager.set_connected(BLUETOOTH, True)

    assert not manager.allows(WIRED, EVENT)
    assert manager.allows(BLUETOOTH, EVENT)


def test_manual_bluetooth_stays_active_when_cable_is_plugged(clock):
    manager = make_manager(clock, policy=BLUETOOTH)
    manager.set_connected(BLUETOOTH, True)

    manager.set_connected(WIRED, True)

    assert manager.active_link == BLUETOOTH


def test_no_link_active_blocks_everything(clock):
    manager = make_manager(clock)

    assert manager.active_link is None
    assert not manager.allows(WIRED, EVENT)
    assert not manager.allows(BLUETOOTH, EVENT)


def test_same_event_from_both_links_executes_once(clock):
    manager = make_manager(clock)
    manager.set_connected(WIRED, True)
    manager.set_connected(BLUETOOTH, True)

    assert manager.allows(WIRED, EVENT)
    clock.advance(0.05)
    assert not manager.allows(BLUETOOTH, EVENT)


def test_same_event_after_the_window_executes_again(clock):
    manager = make_manager(clock)
    manager.set_connected(WIRED, True)

    assert manager.allows(WIRED, EVENT)
    clock.advance(0.4)
    assert manager.allows(WIRED, EVENT)


def test_different_button_inside_the_window_executes(clock):
    manager = make_manager(clock)
    manager.set_connected(WIRED, True)

    assert manager.allows(WIRED, EVENT)
    clock.advance(0.05)
    assert manager.allows(WIRED, {"mode": "desktop", "index": 2, "trigger": "single"})


def test_different_trigger_inside_the_window_executes(clock):
    manager = make_manager(clock)
    manager.set_connected(WIRED, True)

    assert manager.allows(WIRED, EVENT)
    clock.advance(0.05)
    assert manager.allows(WIRED, {"mode": "desktop", "index": 1, "trigger": "double"})


def test_unknown_policy_falls_back_to_auto(clock):
    manager = make_manager(clock, policy="nonsense")
    manager.set_connected(BLUETOOTH, True)
    manager.set_connected(WIRED, True)

    assert manager.policy == "auto"
    assert manager.active_link == WIRED


def test_set_policy_takes_effect_immediately(clock):
    manager = make_manager(clock)
    manager.set_connected(WIRED, True)
    manager.set_connected(BLUETOOTH, True)
    assert manager.active_link == WIRED

    manager.set_policy(BLUETOOTH)

    assert manager.active_link == BLUETOOTH
    assert manager.allows(BLUETOOTH, {"mode": "desktop", "index": 3, "trigger": "single"})
    assert not manager.allows(WIRED, {"mode": "desktop", "index": 4, "trigger": "single"})


def test_set_connected_reports_only_real_changes(clock):
    manager = make_manager(clock)

    assert manager.set_connected(WIRED, True) is True
    assert manager.set_connected(WIRED, True) is False
    assert manager.set_connected(WIRED, False) is True
