"""Which transport is allowed to run actions: USB serial or Bluetooth.

Both links can be connected at the same time (the pad keeps broadcasting over
BLE while the cable is plugged in). Without arbitration one physical press
would execute its action twice, so every event goes through this manager:
only the active link is allowed to execute, and an identical event seen again
within the dedupe window is ignored.
"""

import threading
import time

WIRED = "wired"
BLUETOOTH = "bluetooth"

POLICY_AUTO = "auto"
POLICY_WIRED = "wired"
POLICY_BLUETOOTH = "bluetooth"
POLICIES = (POLICY_AUTO, POLICY_WIRED, POLICY_BLUETOOTH)
DEFAULT_POLICY = POLICY_AUTO

DEDUPE_WINDOW_S = 0.3


class LinkManager:
    """Decide which connected link owns the buttons, and drop duplicates."""

    def __init__(self, policy: str = DEFAULT_POLICY,
                 dedupe_window: float = DEDUPE_WINDOW_S,
                 clock=time.monotonic) -> None:
        self._clock = clock
        self._dedupe_window = max(0.0, float(dedupe_window))
        self._connected = {WIRED: False, BLUETOOTH: False}
        self._policy = DEFAULT_POLICY
        self._last_signature = None
        self._last_time = 0.0
        # Events arrive from the serial and BLE threads at the same time.
        self._lock = threading.RLock()
        self.set_policy(policy)

    # ------------------------------------------------------------------ policy
    @property
    def policy(self) -> str:
        with self._lock:
            return self._policy

    def set_policy(self, policy: str) -> str:
        """Apply a policy ('auto' | 'wired' | 'bluetooth'); unknown -> auto."""
        normalized = (policy or "").strip().lower()
        if normalized not in POLICIES:
            normalized = DEFAULT_POLICY
        with self._lock:
            self._policy = normalized
            return self._policy

    # --------------------------------------------------------------- transport
    def set_connected(self, link: str, connected: bool) -> bool:
        """Record a transport state; return True only if it actually changed."""
        with self._lock:
            if link not in self._connected:
                return False
            value = bool(connected)
            changed = self._connected[link] != value
            self._connected[link] = value
            return changed

    @property
    def active_link(self):
        """The link that may execute actions, or None when nothing is usable."""
        with self._lock:
            return self._active_link_locked()

    def _active_link_locked(self):
        if self._policy == POLICY_WIRED:
            return WIRED if self._connected[WIRED] else None
        if self._policy == POLICY_BLUETOOTH:
            return BLUETOOTH if self._connected[BLUETOOTH] else None
        if self._connected[WIRED]:
            return WIRED
        if self._connected[BLUETOOTH]:
            return BLUETOOTH
        return None

    def allows(self, link: str, event: dict) -> bool:
        """True when this event from this link should execute exactly once."""
        with self._lock:
            if link != self._active_link_locked():
                return False

            signature = self._signature(event)
            now = self._clock()
            if (signature is not None and signature == self._last_signature
                    and (now - self._last_time) < self._dedupe_window):
                return False

            self._last_signature = signature
            self._last_time = now
            return True

    @staticmethod
    def _signature(event):
        if not isinstance(event, dict):
            return None
        return (event.get("mode"), event.get("index"), event.get("trigger"))
