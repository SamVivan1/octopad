# Tray icons

The three connection states of the OctoPad tray icon, copied verbatim from
`linux-app/tray-icons/hicolor/32x32/apps/`:

| file                        | state      | colour |
| --------------------------- | ---------- | ------ |
| `octopad-tray.png`          | connected  | green  |
| `octopad-tray-checking.png` | connecting | amber  |
| `octopad-tray-off.png`      | not linked | red    |

They are listed in `bundle.resources` (`tauri.conf.json`) so the installer
copies them into the app's resource directory, where `load_tray_icons` in
`src/lib.rs` decodes them once at start-up. The status-event → artwork mapping
is pinned by the tests at the bottom of `src/lib.rs`.

Do not redesign these files; change the artwork in `linux-app/` and re-copy.
