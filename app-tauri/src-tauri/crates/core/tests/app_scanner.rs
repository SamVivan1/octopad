//! Task 7: the desktop application scanner (`apps.rs`), ported from
//! `linux-app/app_scanner.py` (list + `parse_desktop_file`).
//!
//! Every test feeds the scanner **synthetic** directory trees through the
//! `&[PathBuf]` roots injection — the real `$HOME` is never touched. The two
//! paths a test cannot build (`/var/lib/flatpak/...`, snap) are pinned through
//! [`octopad_core::apps::xdg_scan_roots`], which takes every location as an
//! argument, so the mandated superset list is testable deterministically.
//!
//! What is pinned (plan §4):
//!
//! - **XDG order, first wins.** Earlier roots take precedence over later ones
//!   for the same `.desktop` id.
//! - **Lowercase-name sort.** `(name.to_lowercase())`, the legacy sort key.
//! - **The floor filter.** `Type=Application` + `Name` + `Exec` required;
//!   `NoDisplay`/`Hidden` are skipped.
//! - **`[Desktop Entry]` only.** Names under other sections or locale keys
//!   (`Name[de]=`) are never read.

use std::fs;
use std::path::{Path, PathBuf};

use octopad_core::apps::{scan_applications, xdg_scan_roots, AppEntry};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write one `.desktop` file; returns nothing — the test tree is the fixture.
fn write_desktop(dir: &Path, id: &str, content: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(id), content).unwrap();
}

/// Exact `(id, name)` pairs, so ordering is asserted without extra plumbing.
fn entries(roots: &[PathBuf]) -> Vec<(String, String)> {
    scan_applications(roots)
        .into_iter()
        .map(|app| (app.id, app.name))
        .collect()
}

const APPLICATION: &str = "[Desktop Entry]
Type=Application
Name={name}
Exec={exec}
";

fn desktop_entry(name: &str, exec: &str) -> String {
    APPLICATION.replace("{name}", name).replace("{exec}", exec)
}

// ---------------------------------------------------------------------------
// The floor: what `app_scanner.py` already does
// ---------------------------------------------------------------------------

#[test]
fn picks_visible_applications_from_every_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("applications");
    write_desktop(&root, "alpha.desktop", &desktop_entry("Alpha", "alpha %U"));
    write_desktop(&root, "beta.desktop", &desktop_entry("Beta", "beta"));

    assert_eq!(entries(&[root]), vec![
        ("alpha.desktop".to_string(), "Alpha".to_string()),
        ("beta.desktop".to_string(), "Beta".to_string()),
    ]);
}

#[test]
fn earlier_roots_win_for_the_same_desktop_id() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user");
    let system = dir.path().join("system");
    let snap = dir.path().join("snap");

    // The same id shows up in every tree; only the first root's name survives.
    write_desktop(&user, "shared.desktop", &desktop_entry("From User", "user"));
    write_desktop(&system, "shared.desktop", &desktop_entry("From System", "system"));
    write_desktop(&snap, "shared.desktop", &desktop_entry("From Snap", "snap"));
    // A second id only present in a later root still appears.
    write_desktop(&snap, "snap-only.desktop", &desktop_entry("Snap Only", "snap"));

    assert_eq!(entries(&[user.clone(), system.clone(), snap.clone()]), vec![
        ("shared.desktop".to_string(), "From User".to_string()),
        ("snap-only.desktop".to_string(), "Snap Only".to_string()),
    ]);
}

#[test]
fn sorts_by_lowercase_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("applications");
    for (id, name, exec) in [
        ("zeta.desktop", "tHe Zeta", "z"),
        ("alpha.desktop", "the alpha", "a"),
        ("mid.desktop", "The Beta", "b"),
    ] {
        write_desktop(&root, id, &desktop_entry(name, exec));
    }

    let ids = entries(&[root])
        .into_iter()
        .map(|(id, _name)| id)
        .collect::<Vec<_>>();
    assert_eq!(ids, ["alpha.desktop", "mid.desktop", "zeta.desktop"]);
}

#[test]
fn skips_hidden_nondisplay_wrong_type_and_incomplete_entries() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("applications");

    write_desktop(
        &root,
        "hidden.desktop",
        &format!("{APPLICATION}Hidden=true\n")
            .replace("{name}", "Hidden One")
            .replace("{exec}", "x"),
    );
    write_desktop(&root, "nodisplay.desktop", &format!("{APPLICATION}NoDisplay=true\n").replace("{name}", "No Display One").replace("{exec}", "x"));
    // Case-insensitive values, as the Python `lower() == 'true'` check.
    write_desktop(&root, "nodef.desktop", &format!("{APPLICATION}NoDisplay=TRUE\n").replace("{name}", "Nodef").replace("{exec}", "x"));
    write_desktop(&root, "notapp.desktop", &desktop_entry("Wrong Type", "x").replace("Type=Application", "Type=Link"));
    write_desktop(&root, "noexec.desktop", "Type=Application\nName=No Exec\n");
    write_desktop(&root, "noname.desktop", "Type=Application\nExec=run\n");
    write_desktop(&root, "emptyname.desktop", &desktop_entry("", "x"));
    // This one does survive.
    write_desktop(&root, "visible.desktop", &desktop_entry("Visible", "x"));

    assert_eq!(entries(&[root]), vec![
        ("visible.desktop".to_string(), "Visible".to_string()),
    ]);
}

#[test]
fn reads_data_only_from_the_desktop_entry_section() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("applications");

    write_desktop(
        &root,
        "sections.desktop",
        &("[Another Section]\nType=Application\nName=From Another Section\nExec=run\n"
            .to_owned()
            + "\n[Desktop Entry]\nType=Application\nName=Real Name\nExec=real\n"),
    );
    // Entry data before the first section is ignored, exactly like Python's
    // parser that only starts after `[Desktop Entry]` is seen.
    write_desktop(
        &root,
        "no_section.desktop",
        "Type=Application\nName=Headless\nExec=run\n",
    );
    write_desktop(
        &root,
        "locale.desktop",
        "[Desktop Entry]\nType=Application\nName=Real Name\nName[de]=Fake Name\nExec=real\n",
    );

    // The `[Another Section]` data is never read: `sections.desktop` reports
    // the `[Desktop Entry]` name. A file whose only section header is the
    // invisible pre-section data (`no_section.desktop`) is dropped entirely.
    assert_eq!(entries(&[root]), vec![
        ("locale.desktop".to_string(), "Real Name".to_string()),
        ("sections.desktop".to_string(), "Real Name".to_string()),
    ]);
}

#[test]
fn missing_roots_are_ignored_not_errors() {
    let dir = tempfile::tempdir().unwrap();
    let present = dir.path().join("applications");
    write_desktop(&present, "only.desktop", &desktop_entry("Only", "x"));

    let missing = dir.path().join("does-not-exist");
    assert_eq!(entries(&[missing.clone(), present, missing]), vec![
        ("only.desktop".to_string(), "Only".to_string()),
    ]);
}

#[test]
fn non_desktop_files_and_subdirectories_are_not_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("applications");
    write_desktop(&root, "real.desktop", &desktop_entry("Real", "x"));
    fs::write(dir.path().join("notes.desktop"), "not a real file location").unwrap();
    fs::create_dir_all(root.join("nested.desktop")).unwrap();

    assert_eq!(entries(&[root]), vec![
        ("real.desktop".to_string(), "Real".to_string()),
    ]);
}

#[test]
fn the_first_stored_value_of_a_key_wins_in_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("applications");
    write_desktop(
        &root,
        "twice.desktop",
        "[Desktop Entry]\nType=Application\nName=First Name\nName=Second Name\nExec=x\n",
    );

    assert_eq!(entries(&[root]), vec![
        ("twice.desktop".to_string(), "First Name".to_string()),
    ]);
}

// ---------------------------------------------------------------------------
// The superset: the root list the plan mandates
// ---------------------------------------------------------------------------

#[test]
fn the_scan_root_list_is_the_xdg_superset() {
    let home = Path::new("/home/tester");
    let data_home = PathBuf::from("/home/tester/.local/share");
    let data_dirs = vec![
        PathBuf::from("/usr/local/share"),
        PathBuf::from("/usr/share"),
    ];

    assert_eq!(
        xdg_scan_roots(&data_home, &data_dirs, home),
        vec![
            // XDG_DATA_HOME first — the user's own entries take precedence.
            PathBuf::from("/home/tester/.local/share/applications"),
            // Then the XDG_DATA_DIRS, in order.
            PathBuf::from("/usr/local/share/applications"),
            PathBuf::from("/usr/share/applications"),
            // The Flatpak export trees and Snap's desktop dir complete the
            // superset (Python's `SEARCH_DIRS` floor is inside these).
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            PathBuf::from("/home/tester/.local/share/flatpak/exports/share/applications"),
            PathBuf::from("/var/lib/snapd/desktop/applications"),
        ]
    );
}

#[test]
fn entries_serialize_as_bare_id_and_name() {
    let app = AppEntry {
        id: "code.desktop".to_string(),
        name: "Visual Studio Code".to_string(),
    };
    let json = serde_json::to_value(&app).unwrap();

    assert_eq!(json, serde_json::json!({ "id": "code.desktop", "name": "Visual Studio Code" }));
}
