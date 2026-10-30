//! Desktop application scanner (M3.2 Task 7) — the Rust port of
//! `linux-app/app_scanner.py`'s parse + scan, feeding the frontend application
//! picker.
//!
//! What is ported, and where the port deliberately goes further:
//!
//! - **The INI-ish reader.** One pass, `[Desktop Entry]` section only; data
//!   before the first section or under any other section is ignored, a
//!   `Key[locale]` line is never read (default locale only), and the first
//!   stored value of a key wins — all matching `parse_desktop_file`.
//! - **The floor filter.** `Type=Application`, `Name` and `Exec` must all be
//!   present; `NoDisplay` / `Hidden` (case-insensitive `"true"`) drop the
//!   entry. That is exactly the Python set, so the new picker never offers an
//!   app the old one hid.
//! - **The superset of roots.** The plan mandates the XDG standard dirs *plus*
//!   the Flatpak export trees (`/var/lib/flatpak/exports/share/applications`
//!   and `~/.local/share/flatpak/exports/share/applications`) and the Snap
//!   desktop dir (`/var/lib/snapd/desktop/applications`) — the owner's machine
//!   ships Snap apps, so `/var/lib/snapd/...` is not optional. See
//!   [`xdg_scan_roots`] for the exact order.
//! - **First wins, then sort.** Earlier roots take precedence for the same
//!   `.desktop` id (Python's `seen_ids`), and the surviving entries sort by
//!   `name.to_lowercase()`.
//!
//! Everything here is pure and injectable: [`scan_applications`] takes the
//! roots to scan, so tests build synthetic trees and never touch the real
//! `$HOME`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One pickable application (`AppEntry { id, name }` in the spec contract).
///
/// `id` is the `.desktop` basename (`code.desktop`), the value the stored
/// `launch_app` action's `desktopId` field carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
}

/// Scan `roots` for `.desktop` files and return the visible applications.
///
/// Roots are scanned in the given order; the first root containing a given id
/// decides the entry, and unknown roots are skipped rather than erroring. Only
/// direct `*.desktop` files are read (the legacy `glob(directory, '*.desktop')`
/// behaviour — subdirectories are not recursed). The result is sorted by the
/// lowercased display name.
pub fn scan_applications(roots: &[PathBuf]) -> Vec<AppEntry> {
    let mut apps = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for root in roots {
        let Ok(dir_entries) = std::fs::read_dir(root) else {
            continue;
        };
        for dir_entry in dir_entries.flatten() {
            let path = dir_entry.path();
            let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !id.ends_with(".desktop") || !path.is_file() || !seen.insert(id.to_string()) {
                continue;
            }
            match parse_desktop_entry(&path) {
                Some(app) => apps.push(app),
                None => continue,
            }
        }
    }

    // The legacy sort key, applied to the display name lowercased.
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// The directory list [`default_scan_roots`] produces, with every location
/// spelled out — the injection point that lets tests pin the superset plan
/// without an environment.
///
/// Order is XDG precedence (`XDG_DATA_HOME`, then `XDG_DATA_DIRS` in order)
/// followed by the mandated Flatpak and Snap trees:
///
/// 1. `<data_home>/applications`
/// 2. `<data_dir>/applications` for every entry of `data_dirs`
/// 3. `/var/lib/flatpak/exports/share/applications`
/// 4. `<home>/.local/share/flatpak/exports/share/applications`
/// 5. `/var/lib/snapd/desktop/applications`
pub fn xdg_scan_roots(data_home: &Path, data_dirs: &[PathBuf], home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![data_home.join("applications")];
    for dir in data_dirs {
        roots.push(dir.join("applications"));
    }
    roots.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    roots.push(home.join(".local/share/flatpak/exports/share/applications"));
    roots.push(PathBuf::from("/var/lib/snapd/desktop/applications"));
    roots
}

/// The roots the production scan uses: the XDG data dirs with the standard
/// defaults, plus the Flatpak and Snap trees (see [`xdg_scan_roots`]).
///
/// Pure convenience for the command layer — never called from tests, which
/// inject their own synthetic trees instead.
pub fn default_scan_roots() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.trim().is_empty())
        .map(|dirs| dirs.split(':').map(PathBuf::from).collect())
        .unwrap_or_else(|| vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
    xdg_scan_roots(&data_home, &data_dirs, &home)
}

/// Parse one `.desktop` file: application metadata or `None` when the entry is
/// not a visible application.
///
/// A read or encoding error, a missing `Type=Application`, `Name` or `Exec`
/// line, or a `NoDisplay` / `Hidden` flag (case-insensitively `"true"`) all
/// drop the file silently — the scanner never reports why, exactly like the
/// Python scanner whose `parse_desktop_file` returns `None`.
fn parse_desktop_entry(path: &Path) -> Option<AppEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let data = parse_desktop_entry_section(&text, &["Desktop Entry"]);
    if data.get("Type").map(String::as_str) != Some("Application") {
        return None;
    }
    if flag_is_true(data.get("NoDisplay")) || flag_is_true(data.get("Hidden")) {
        return None;
    }
    let name = data.get("Name")?;
    let exec = data.get("Exec")?;
    if name.trim().is_empty() || exec.trim().is_empty() {
        return None;
    }
    Some(AppEntry {
        id: path
            .file_name()?
            .to_str()?
            .to_string(),
        name: name.trim().to_string(),
    })
}

/// Whether a boolean-looking `.desktop` value means "on", matching the Python
/// `value.lower() == 'true'` check (an absent value is not "on").
fn flag_is_true(value: Option<&String>) -> bool {
    value.map(|value| value.trim().to_lowercase()) == Some(String::from("true"))
}

/// Line-based INI-ish reader: entries of the *first* matching `[section]`,
/// only default-locale keys (`Key=`, never `Key[lang]=`), first value wins.
///
/// Blank and `#` lines are skipped; a line without `=` is ignored. Data before
/// any section, or under a section that is not in `wanted`, never reaches the
/// map — which is what pins name extraction to `[Desktop Entry]`.
fn parse_desktop_entry_section(text: &str, wanted: &[&str]) -> std::collections::HashMap<String, String> {
    let wanted: HashSet<&str> = wanted.iter().copied().collect();
    let mut data = std::collections::HashMap::new();
    let mut section: Option<&str> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .map(str::trim)
                .filter(|name| wanted.contains::<str>(name));
            continue;
        }
        let in_section = matches!(section, Some(name) if wanted.contains(name));
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !key.contains('[') && !data.contains_key(key) {
            data.insert(key.to_string(), value.trim().to_string());
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unwanted_section_is_ignored() {
        let data = parse_desktop_entry_section(
            "[ActionCheese]\nName=Not the entry\n\n[Desktop Entry]\nName=Entry\n",
            &["Desktop Entry"],
        );
        assert_eq!(data.get("Name"), Some(&"Entry".to_string()));
        assert!(data.get("exec").is_none());
    }

    #[test]
    fn locale_keys_never_shadow_the_default_value() {
        let data = parse_desktop_entry_section(
            "[Desktop Entry]\nName=Real Name\nName[de]=Fake Name\n",
            &["Desktop Entry"],
        );
        assert_eq!(data.get("Name"), Some(&"Real Name".to_string()));
    }
}
