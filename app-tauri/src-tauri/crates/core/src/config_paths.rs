use std::path::PathBuf;

/// Compute the legacy OctoPad config directory without touching the filesystem.
///
/// - Unix: `$HOME/.config/octopad`
/// - Windows: `%APPDATA%/OctoPad`
pub fn legacy_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            PathBuf::from(appdata).join("OctoPad")
        } else {
            PathBuf::from("OctoPad")
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config").join("octopad")
        } else {
            PathBuf::from(".config").join("octopad")
        }
    }
}
