# PyInstaller runtime hook: make comtypes (used by pycaw) work in a frozen app.
# comtypes generates and imports typelib wrapper modules at runtime, so it needs
# a writable directory even when the application bundle itself is read-only.
import os
import tempfile

os.environ.setdefault(
    "COMTYPES_GEN_DIR", os.path.join(tempfile.gettempdir(), "octopad_comtypes_gen"))
