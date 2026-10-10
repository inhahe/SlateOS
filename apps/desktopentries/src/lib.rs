//! The applications' desktop entries, checked.
//!
//! Every program under `apps/` that opens a window carries a freedesktop
//! desktop entry, `apps/<crate>/org.slateos.<Id>.desktop`: the small text file
//! that tells the start menu the program exists, what it is called, which
//! picture stands for it, which folder it goes in and how to start it
//! (`gui/desktopentry`; `requests/c-e-ship-a-desktop-entry-with-each-program.md`).
//! Lane D installs them into `/usr/share/applications`
//! (`requests/c-d-install-desktop-entries-into-the-image.md`).
//!
//! A file like that is easy to get quietly wrong -- a program renamed, its
//! entry still naming the old one; a category the menu does not know, so the
//! program lands under Other; an entry that offers to open files its program
//! never reads. This crate has no code of its own, only the tests that keep
//! those files true, read through the same `desktopentry` reader the desktop
//! uses.

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        clippy::indexing_slicing
    )]

    use desktopentry::menu::Category;
    use desktopentry::{App, DesktopEntry};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn apps_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn crate_dirs() -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(apps_dir())
            .expect("apps/ is readable")
            .map(|e| e.expect("a directory entry").path())
            .filter(|p| p.join("Cargo.toml").is_file())
            .collect();
        dirs.sort();
        dirs
    }

    /// The program's `main.rs`, if the crate has a program.
    fn main_rs(dir: &Path) -> Option<String> {
        std::fs::read_to_string(dir.join("src").join("main.rs")).ok()
    }

    /// Whether the crate's program opens a window: it starts through the
    /// application framework, `oswindow::app`.
    fn opens_a_window(dir: &Path) -> bool {
        main_rs(dir).is_some_and(|t| t.contains("app::launch") || t.contains("oswindow"))
    }

    /// The crate's desktop entries.
    fn entries_in(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .expect("the crate's directory is readable")
            .map(|e| e.expect("a directory entry").path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("org.slateos.") && n.ends_with(".desktop"))
            })
            .collect();
        found.sort();
        found
    }

    /// The name the program is built and installed under: its `[[bin]]`'s,
    /// or else the package's.
    fn binary_name(dir: &Path) -> String {
        let manifest =
            std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml is readable");
        let (mut section, mut package, mut bin) = ("", None, None);
        for line in manifest.lines().map(str::trim) {
            if line.starts_with('[') {
                section = line;
                continue;
            }
            if let Some(value) = line.strip_prefix("name = ") {
                let value = value.trim_matches('"').to_owned();
                match section {
                    "[package]" => package = package.or(Some(value)),
                    "[[bin]]" => bin = bin.or(Some(value)),
                    _ => {}
                }
            }
        }
        bin.or(package).expect("a package name")
    }

    /// The entry in `path`, as the desktop's reader validates it.
    fn read(path: &Path) -> App {
        let bytes = std::fs::read(path).expect("the entry is readable");
        let entry = DesktopEntry::parse(&bytes)
            .unwrap_or_else(|e| panic!("{path:?} does not parse: {e:?}"));
        let id = path.file_name().and_then(|n| n.to_str()).expect("a name");
        App::from_entry(&entry, id, None)
            .unwrap_or_else(|e| panic!("{path:?} is not a valid entry: {e:?}"))
    }

    fn name_of(dir: &Path) -> String {
        dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned()
    }

    #[test]
    fn every_program_with_a_window_has_one_entry_the_menu_can_read() {
        let mut checked = 0;
        for dir in crate_dirs().into_iter().filter(|d| opens_a_window(d)) {
            let name = name_of(&dir);
            let entries = entries_in(&dir);
            assert_eq!(
                entries.len(),
                1,
                "apps/{name} opens a window and has {} desktop entries",
                entries.len()
            );
            let app = read(&entries[0]);
            let exec = app.exec.as_ref().expect("an Exec line");
            assert_eq!(
                exec.program(),
                binary_name(&dir),
                "apps/{name}'s entry starts a program of another name"
            );
            assert_ne!(
                Category::of(&app.categories),
                Category::Other,
                "apps/{name}'s entry names no main category, so the menu files it under Other: {:?}",
                app.categories
            );
            assert!(
                app.icon.as_deref().is_some_and(|i| !i.is_empty()),
                "apps/{name}'s entry names no icon"
            );
            assert!(
                app.comment.as_deref().is_some_and(|c| !c.is_empty()),
                "apps/{name}'s entry says nothing of what the program is for"
            );
            assert!(!app.terminal, "apps/{name} has a window of its own");
            checked += 1;
        }
        assert!(
            checked > 100,
            "only {checked} programs with windows were found"
        );
    }

    #[test]
    fn no_two_programs_share_an_entry_id() {
        let mut seen = BTreeSet::new();
        for dir in crate_dirs() {
            for entry in entries_in(&dir) {
                let id = entry
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_owned();
                assert!(seen.insert(id.clone()), "{id} is used twice");
            }
        }
    }

    #[test]
    fn only_programs_with_windows_have_entries() {
        for dir in crate_dirs() {
            if !entries_in(&dir).is_empty() {
                assert!(
                    opens_a_window(&dir),
                    "apps/{} has a desktop entry and no window to open",
                    name_of(&dir)
                );
            }
        }
    }

    /// An entry that offers to open files (`%f`, `%F`) belongs to a program
    /// that reads the files it is given, through the application framework's
    /// arguments; and only such an entry lists file types (`MimeType`). An
    /// entry that promises to open a file its program ignores sends the user
    /// to a window that shows something else.
    #[test]
    fn an_entry_offers_to_open_files_only_for_a_program_that_reads_them() {
        for dir in crate_dirs() {
            for entry in entries_in(&dir) {
                let app = read(&entry);
                let name = name_of(&dir);
                let takes = app
                    .exec
                    .as_ref()
                    .is_some_and(desktopentry::Exec::takes_targets);
                if takes {
                    let main = main_rs(&dir).unwrap_or_default();
                    assert!(
                        main.contains("ArgsOs") && main.contains("args.rest"),
                        "apps/{name}'s entry offers to open files its program does not read"
                    );
                }
                assert!(
                    takes || app.mime_types.is_empty(),
                    "apps/{name}'s entry lists file types but takes no files"
                );
            }
        }
    }
}
