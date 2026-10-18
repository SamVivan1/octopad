# M2 implementation and smoke-test notes

## Task 8: tray, single instance, autostart, and shortcuts

### Automated checks

- Frontend tests: 48 passed across 7 files.
- Frontend type-check: passed with 0 errors and 0 warnings.
- Frontend production build: passed.
- Rust formatting check: passed.
- Core tests with `serial,ble`: 199 passed, 3 hardware-dependent tests ignored, 0 failed.
- Tauri application compilation is blocked on this Linux host by missing `javascriptcoregtk-4.1`, `libsoup-3.0`, and `webkit2gtk-4.1` development packages. The app crate has not been type-checked here.
- A Windows-target cross-check was also blocked because `x86_64-w64-mingw32-windres` is not installed.

### Manual smoke checklist

Run the following on both Linux and Windows before accepting M2. No manual runtime checks have been performed yet.

| Check | Linux | Windows |
|---|---|---|
| Tray icon shows Open, connection status, and Quit; status follows serial/Bluetooth changes | Not run: app cannot be built on this host | Not run: Windows environment unavailable |
| Closing the window hides it; tray Open restores it; device runtime remains active | Not run: app cannot be built on this host | Not run: Windows environment unavailable |
| Quit exits the app and shuts down the device runtime | Not run: app cannot be built on this host | Not run: Windows environment unavailable |
| A second normal launch focuses the existing window; a `--hidden` launch leaves it hidden | Not run: app cannot be built on this host | Not run: Windows environment unavailable |
| Autostart enable/disable persists, and autostart launches hidden | Not run: app cannot be built on this host | Not run: Windows environment unavailable |
| Ctrl+S saves and Ctrl+Q quits; text inputs retain normal editing behavior | Not run: app cannot be built on this host | Not run: Windows environment unavailable |

Do not treat the UI unit tests as a substitute for these platform checks. Hardware-specific serial/Bluetooth checks must only be run with the legacy Python app closed.

## Task 9: Windows CI and artifact

The Windows workflow now runs frontend tests, type-checking, and a frontend build after `npm ci` and before the Rust checks and Tauri bundle build. Its existing artifact-upload step still requires both NSIS (`.exe`) and MSI (`.msi`) outputs.

### Locally verified

- Workflow YAML parses successfully.
- `npm ci` completed successfully.
- Frontend tests: 48 passed across 7 files.
- Frontend type-check: passed with 0 errors and 0 warnings.
- Frontend production build: passed.
- Rust formatting check: passed.
- Core tests with `serial,ble`: 199 passed, 3 hardware-dependent tests ignored, 0 failed.

### Pending external verification

- `cargo check --workspace` cannot complete on this Linux host because the Tauri WebKitGTK dependencies `libsoup-3.0`, `javascriptcoregtk-4.1`, and `webkit2gtk-4.1` are unavailable.
- The Windows workflow was not run: this branch does not exist on the remote, and pushing or opening a PR to trigger external CI was not authorized. No NSIS/MSI artifact has been confirmed.
- Windows install/UI/tray/single-instance acceptance and the Linux tray/autostart/hide-on-close smoke checks remain owner gates.
