# Security Policy

## Reporting a Vulnerability

Please do **not** open a public issue for security problems. Instead, report
them privately via GitHub's [Security Advisories](../../security/advisories/new)
or by e-mail to the maintainer listed on the repository profile. We aim to
acknowledge reports within a few days.

## How OctoPad handles data

- The desktop applications run **entirely on your machine**. There is no
  telemetry and no OctoPad-operated server.
- **Home Assistant** access tokens are stored locally, in
  `~/.config/octopad/ha_config.json` (Linux) or
  `%APPDATA%\OctoPad\ha_config.json` (Windows), with owner-only permissions
  where the OS supports it. They are sent only to the Home Assistant URL you
  configure.
- **Configuration** (button mappings, timing, presets) lives in the same config
  directory. It does not contain secrets unless you put them in an action.
- The firmware stores only the device name, the active mode and the timing
  parameters in NVS. It does not store credentials.

## Scope

In scope: the firmware (`octopad.ino`), the Linux and Windows applications, and
the packaging/CI workflows in this repository. Out of scope: vulnerabilities in
third-party dependencies (please report those upstream) and issues that require
physical access to an already-unlocked machine.
