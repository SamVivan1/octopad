# Contributing to OctoPad

Thanks for your interest in improving OctoPad! This project is a DIY-friendly,
open-source 8-key macropad: an ESP32-C3 firmware plus a native GTK4/Libadwaita
Linux app.

## Ways to contribute

- **Firmware** (`octopad.ino`) — ESP32-C3 / NimBLE / Arduino.
- **Desktop app** (`linux-app/`) — Python 3, GTK4, Libadwaita, bluez D-Bus.
- **Docs** — README, setup guides, wiring, screenshots.
- **Hardware** — PCB/stripboard layouts, 3D-printed cases.
- **Bug reports & feature requests** — open an issue using the templates.

## Development setup

### Firmware
- Arduino IDE with the ESP32 board package.
- Library: **NimBLE-Arduino** (by h2zero).
- Open `octopad.ino` (the sketch folder must be named `octopad`).
- Target: **ESP32-C3 SuperMini** (or any ESP32-C3).

### Desktop app
Distro packages (Debian/Ubuntu):
```bash
sudo apt install python3 python3-gi python3-gi-cairo gir1.2-gtk-4.0 \
     gir1.2-adw-1 python3-serial
```
Run from source:
```bash
cd linux-app
./run.sh
```

## Before you open a pull request

1. **Lint / syntax check** the Python sources (this is what CI runs):
   ```bash
   cd linux-app
   python3 -m pyflakes *.py ui/*.py tray-icons/*.py
   python3 -m compileall -q .
   ```
2. **Build smoke test** — make sure the UI still constructs:
   ```bash
   cd linux-app
   python3 -c "import gi; print(gi.__version__)"
   ./run.sh --hidden   # should start in the tray without errors
   ```
3. Keep changes focused; match the existing code style (no new runtime
   dependencies unless discussed in an issue first).
4. Do **not** commit personal data: no Home Assistant URLs/tokens, no personal
   absolute paths, no `*.local.json`.

## Commit messages

Short, imperative, and descriptive. English is preferred in code/commits.

## License

By contributing you agree that your contributions are licensed under the
[MIT License](LICENSE).
