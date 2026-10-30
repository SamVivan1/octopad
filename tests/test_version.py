"""One version for every shipped piece: apps, firmware, changelog, VERSION."""

import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def read_version_file():
    with open(os.path.join(ROOT, "VERSION"), encoding="utf-8") as handle:
        return handle.read().strip()


def test_repo_version_is_semver():
    assert re.fullmatch(r"\d+\.\d+\.\d+", read_version_file())


def test_linux_app_reports_the_repo_version():
    import version

    assert version.app_version() == read_version_file()


def test_windows_app_reports_the_repo_version(windows_config):
    import importlib.util
    import sys

    path = os.path.join(ROOT, "windows-app", "version.py")
    spec = importlib.util.spec_from_file_location("win_version", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules["win_version"] = module
    spec.loader.exec_module(module)

    assert module.app_version() == read_version_file()


def test_firmware_version_matches_the_repo_version():
    with open(os.path.join(ROOT, "octopad.ino"), encoding="utf-8") as handle:
        source = handle.read()
    match = re.search(r'#define\s+FIRMWARE_VERSION\s+"([^"]+)"', source)
    assert match is not None, "FIRMWARE_VERSION missing from octopad.ino"
    assert match.group(1) == read_version_file()


def test_changelog_head_matches_the_repo_version():
    with open(os.path.join(ROOT, "CHANGELOG.md"), encoding="utf-8") as handle:
        lines = handle.read().splitlines()
    headings = [line for line in lines if line.startswith("## [")]
    assert headings, "CHANGELOG has no version headings"
    match = re.match(r"##\s+\[([^\]]+)\]", headings[0])
    assert match is not None
    assert match.group(1) == read_version_file()


def test_readme_states_the_repo_version():
    with open(os.path.join(ROOT, "README.md"), encoding="utf-8") as handle:
        first_line = handle.readline().strip()
    assert read_version_file() in first_line
