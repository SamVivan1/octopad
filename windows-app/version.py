"""Single source of truth for the application version.

The repository ``VERSION`` file is authoritative. PyInstaller bundles it next
to the frozen application (see ``octopad.spec``); a missing file never breaks
the UI, it only reports a dev fallback.
"""

import os
import sys

FALLBACK_VERSION = "0.0.0+dev"


def _app_dir() -> str:
    if getattr(sys, "frozen", False):
        return getattr(sys, "_MEIPASS", os.path.dirname(sys.executable))
    return os.path.dirname(os.path.abspath(__file__))


def _candidates():
    here = _app_dir()
    yield os.path.join(here, "VERSION")                      # bundled with the exe
    yield os.path.join(os.path.dirname(here), "VERSION")     # repository checkout


def app_version() -> str:
    for path in _candidates():
        try:
            with open(path, "r", encoding="utf-8") as handle:
                value = handle.read().strip()
        except OSError:
            continue
        if value:
            return value
    return FALLBACK_VERSION
