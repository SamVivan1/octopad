"""Both bridges must route events through the link manager instead of executing.

The firmware sends every press over USB serial and Bluetooth at the same time.
If each bridge executed the action on its own, one press would fire twice.
The routing helper lives in serial_listener so it can be tested without GTK.
"""

EVENT_LINE = "[DESKTOP] Button 3 -> SINGLE"
EVENT = {"mode": "desktop", "index": 2, "trigger": "single"}
CONFIG = {"marker": "config"}


def test_wired_listener_executes_when_the_wired_link_is_active(serial_listener,
                                                               linux_state,
                                                               monkeypatch):
    executed = []
    monkeypatch.setattr(serial_listener, "execute_button_trigger",
                        lambda *args, **kwargs: executed.append(args))
    monkeypatch.setattr(linux_state, "allow_button", lambda link, event: True)

    serial_listener.route_event_line("wired", EVENT_LINE, CONFIG)

    assert executed == [(CONFIG, "desktop", 2, "single")]


def test_route_blocks_the_inactive_transport(serial_listener, linux_state,
                                             monkeypatch):
    executed = []
    seen = []
    monkeypatch.setattr(serial_listener, "execute_button_trigger",
                        lambda *args, **kwargs: executed.append(args))
    monkeypatch.setattr(linux_state, "allow_button",
                        lambda link, event: seen.append((link, event)) or False)

    serial_listener.route_event_line("bluetooth", EVENT_LINE, CONFIG)

    assert executed == []
    assert seen == [("bluetooth", EVENT)]


def test_route_logs_the_line_before_executing(serial_listener, linux_state,
                                               monkeypatch):
    logged = []
    monkeypatch.setattr(serial_listener, "execute_button_trigger",
                        lambda *args, **kwargs: None)
    monkeypatch.setattr(linux_state, "allow_button", lambda link, event: True)

    serial_listener.route_event_line("wired", EVENT_LINE, CONFIG,
                                     on_log=logged.append)

    assert logged == [EVENT_LINE]


def test_route_reports_identity_lines(serial_listener, monkeypatch):
    seen = []
    monkeypatch.setattr(serial_listener, "execute_button_trigger",
                        lambda *args, **kwargs: None)

    serial_listener.route_event_line(
        "bluetooth", "HELLO:OCTOPAD:My Pad:v2.6.1", CONFIG,
        on_hello=seen.append)

    assert seen == [{"name": "My Pad", "version": "2.6.1"}]


def test_route_ignores_unrelated_lines(serial_listener, linux_state, monkeypatch):
    asked = []
    monkeypatch.setattr(serial_listener, "execute_button_trigger",
                        lambda *args, **kwargs: None)
    monkeypatch.setattr(linux_state, "allow_button",
                        lambda link, event: asked.append(link) or True)

    assert serial_listener.route_event_line("wired", "MODE: DESKTOP", CONFIG) is None
    assert asked == []


def test_serial_listener_executes_when_the_wired_link_is_active(serial_listener,
                                                                linux_state,
                                                                monkeypatch):
    listener = serial_listener.SerialDaemonListener.__new__(
        serial_listener.SerialDaemonListener)
    listener.config = CONFIG
    listener.on_event_callback = None
    executed = []
    monkeypatch.setattr(serial_listener, "execute_button_trigger",
                        lambda *args, **kwargs: executed.append(args))
    monkeypatch.setattr(linux_state, "allow_button", lambda link, event: True)

    listener._handle_hardware_trigger("desktop", 2, "single")

    assert len(executed) == 1
    assert executed[0][:4] == (CONFIG, "desktop", 2, "single")


def test_serial_listener_asks_about_the_wired_link(serial_listener, linux_state,
                                                   monkeypatch):
    listener = serial_listener.SerialDaemonListener.__new__(
        serial_listener.SerialDaemonListener)
    listener.config = CONFIG
    listener.on_event_callback = None
    seen = []
    monkeypatch.setattr(linux_state, "allow_button",
                        lambda link, event: seen.append((link, event)) or False)
    monkeypatch.setattr(serial_listener, "execute_button_trigger",
                        lambda *args, **kwargs: None)

    listener._handle_hardware_trigger("desktop", 2, "single")

    assert seen == [("wired", EVENT)]
