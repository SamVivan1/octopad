"""Input-link policy exposed through the application state."""

EVENT = {"mode": "desktop", "index": 0, "trigger": "single"}


def test_link_command_maps_policy_to_firmware_tokens(linux_state):
    assert linux_state.link_command("auto") == "LINK:AUTO"
    assert linux_state.link_command("wired") == "LINK:USB"
    assert linux_state.link_command("bluetooth") == "LINK:BLE"
    assert linux_state.link_command("nonsense") == "LINK:AUTO"


def test_allow_button_delegates_to_the_link_manager(linux_state):
    links = linux_state.links
    links.set_policy("auto")
    links.set_connected("bluetooth", True)
    links.set_connected("wired", True)

    assert linux_state.allow_button("wired", EVENT)
    assert not linux_state.allow_button("bluetooth", {"mode": "desktop", "index": 1,
                                                      "trigger": "single"})


def test_allow_button_blocks_everything_without_a_link(linux_state):
    links = linux_state.links
    links.set_connected("bluetooth", False)
    links.set_connected("wired", False)
    links.set_policy("auto")

    assert not linux_state.allow_button("wired", EVENT)
    assert not linux_state.allow_button("bluetooth", {"mode": "desktop", "index": 2,
                                                      "trigger": "single"})


def test_set_input_link_applies_immediately(linux_state):
    try:
        assert linux_state.set_input_link("bluetooth", persist=False) == "bluetooth"
        assert linux_state.cfg["inputLink"] == "bluetooth"
        assert linux_state.links.policy == "bluetooth"
    finally:
        linux_state.set_input_link("auto", persist=False)


def test_set_input_link_normalizes_unknown_values(linux_state):
    try:
        assert linux_state.set_input_link("telepathy", persist=False) == "auto"
    finally:
        linux_state.set_input_link("auto", persist=False)


def test_serial_status_drives_the_wired_link(linux_state):
    try:
        linux_state.set_serial_status(True, "/dev/ttyACM0")
        assert linux_state.links.active_link == "wired"
    finally:
        linux_state.set_serial_status(False, None)


def test_ble_status_drives_the_bluetooth_link(linux_state):
    try:
        linux_state.set_ble_status(True, "Connected")
        assert linux_state.links.active_link == "bluetooth"
    finally:
        linux_state.set_ble_status(False, "Disconnected")


def test_wired_still_wins_when_both_statuses_are_connected(linux_state):
    try:
        linux_state.set_ble_status(True, "Connected")
        linux_state.set_serial_status(True, "/dev/ttyACM0")
        assert linux_state.links.active_link == "wired"
    finally:
        linux_state.set_serial_status(False, None)
        linux_state.set_ble_status(False, "Disconnected")
