use std::path::PathBuf;

/// Compute the legacy OctoPad config directory without touching the filesystem.
///
/// Mirrors the Python apps exactly:
///
/// - Unix: `$HOME/.config/octopad` (`os.path.expanduser('~/.config/octopad')`
///   in `linux-app/config.py`) — deliberately ignores `XDG_CONFIG_HOME`, just
///   like the legacy app.
/// - Windows: `%APPDATA%\OctoPad` (`os.environ['APPDATA']` with `~` fallback
///   in `windows-app/serial_listener.py`).
///
/// A missing directory is never an error: this only computes a path and never
/// creates or reads anything.
pub fn legacy_config_dir() -> PathBuf {
    legacy_config_dir_impl()
}

#[cfg(target_os = "windows")]
fn legacy_config_dir_impl() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join("OctoPad");
        }
    }
    // Python fallback: `os.environ.get("APPDATA") or os.path.expanduser("~")`.
    dirs::home_dir().unwrap_or_default().join("OctoPad")
}

#[cfg(not(target_os = "windows"))]
fn legacy_config_dir_impl() -> PathBuf {
    // `dirs::home_dir()` expands `~` (HOME env, then passwd entry); the empty
    // fallback keeps the result non-empty and relative, never a panic.
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config").join("octopad")
}
