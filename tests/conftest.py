"""Shared fixtures for the OctoPad test-suite.

The Linux and Windows applications each ship a top-level ``config.py`` (and
other same-named modules), so they cannot be imported together through the
normal ``sys.path`` mechanism. The Linux modules are imported normally (their
directory is prepended to ``sys.path``); the Windows ``config`` is loaded under
a distinct module name from its file path.
"""

import importlib.util
import os
import sys
import tempfile

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LINUX_APP = os.path.join(ROOT, "linux-app")
WINDOWS_APP = os.path.join(ROOT, "windows-app")

# Make the Linux application modules (``config``, ``serial_listener``, ...)
# importable. This must run before any test imports them.
if LINUX_APP not in sys.path:
    sys.path.insert(0, LINUX_APP)


def _load_from_path(module_name: str, path: str):
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="session")
def linux_config():
    import config

    return config


@pytest.fixture(scope="session")
def windows_config():
    # Point APPDATA at a throwaway directory so loading the Windows config never
    # touches the real user profile.
    os.environ["APPDATA"] = tempfile.mkdtemp(prefix="octopad-tests-")
    return _load_from_path("win_config", os.path.join(WINDOWS_APP, "config.py"))


@pytest.fixture(scope="session")
def serial_listener():
    import serial_listener

    return serial_listener


@pytest.fixture(scope="session")
def linux_state():
    import state

    return state


@pytest.fixture(scope="session")
def windows_ble_bridge():
    """Load the Windows BLE bridge without importing its PySide6 dependencies."""
    # The module only imports PySide6/bleak inside functions, so loading it by
    # path is safe (its `import state` / `import config` resolve to the Linux
    # modules already in sys.modules, which these tests do not exercise).
    return _load_from_path("win_ble_bridge",
                           os.path.join(WINDOWS_APP, "ble_bridge.py"))


@pytest.fixture(scope="session")
def ble_bridge(windows_ble_bridge):
    return windows_ble_bridge.BleBridge
