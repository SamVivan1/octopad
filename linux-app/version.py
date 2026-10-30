"""Single source of truth for the application version.

The repository ``VERSION`` file is authoritative: the packaging scripts ship it
next to the application so an installed copy reports the same number as the
checkout. A missing file never breaks the UI, it only reports a dev fallback.
"""

import os

FALLBACK_VERSION = "0.0.0+dev"


def _candidates():
    here = os.path.dirname(os.path.abspath(__file__))
    yield os.path.join(here, "VERSION")                      # bundled beside the app
    yield os.path.join(os.path.dirname(here), "VERSION")     # repository checkout
    yield os.path.join("/usr/share/octopad", "VERSION")      # installed package


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
