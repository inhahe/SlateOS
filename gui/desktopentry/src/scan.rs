//! Finding the desktop entries installed on this machine.
//!
//! The XDG Base Directory specification's data directories, in precedence
//! order -- the user's own (`$XDG_DATA_HOME`, `~/.local/share`) first, then
//! the system's (`$XDG_DATA_DIRS`, `/usr/local/share:/usr/share`) -- each
//! with an `applications` directory, walked for `*.desktop` files.
//!
//! An entry is known by its *desktop file ID*: its path under `applications`,
//! with `/` replaced by `-` (`kde/konsole.desktop` is `kde-konsole.desktop`).
//! The first file found for an ID is the entry; later ones are shadowed. That
//! is what lets a user override a system program's entry by writing their own
//! copy -- or remove it from their menus with a copy saying `Hidden=true`.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{App, DesktopEntry, Locale};

/// How deep under `applications` the walk goes. Entries are rarely more than
/// one directory down; the bound is what keeps a symbolic link to a parent
/// directory from being walked for ever.
const MAX_DEPTH: usize = 8;

/// The data directories to look in, in precedence order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDirs {
    dirs: Vec<PathBuf>,
}

impl DataDirs {
    /// These directories, in this order -- for a test, or a caller that knows
    /// better than the environment.
    #[must_use]
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self { dirs }
    }

    /// The directories the environment names, as the XDG Base Directory
    /// specification reads it: `$XDG_DATA_HOME`, or `$HOME/.local/share`,
    /// then each of `$XDG_DATA_DIRS`, or `/usr/local/share` and `/usr/share`.
    ///
    /// A relative path in either variable is ignored, as that specification
    /// requires -- it would mean something different from every directory a
    /// program happened to be started in. `get` is the environment:
    /// `std::env::var_os` in a program, a table in a test.
    #[must_use]
    pub fn from_env(get: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |name: &str| get(name).filter(|value| !value.is_empty());
        let mut dirs = Vec::new();
        match set("XDG_DATA_HOME") {
            Some(home) => {
                let home = PathBuf::from(home);
                if home.is_absolute() {
                    dirs.push(home);
                }
            }
            None => {
                if let Some(home) = set("HOME") {
                    dirs.push(PathBuf::from(home).join(".local").join("share"));
                }
            }
        }
        match set("XDG_DATA_DIRS") {
            Some(list) => dirs.extend(std::env::split_paths(&list).filter(|dir| dir.is_absolute())),
            None => {
                dirs.push(PathBuf::from("/usr/local/share"));
                dirs.push(PathBuf::from("/usr/share"));
            }
        }
        Self { dirs }
    }

    /// The directories, in precedence order.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }
}

/// An entry file that was found and parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Its desktop file ID.
    pub id: String,
    /// Where it is.
    pub path: PathBuf,
    /// What it says.
    pub entry: DesktopEntry,
}

/// A file that looked like an entry and could not be used, and why -- kept
/// rather than dropped, so that "my program is not in the menu" has an
/// answer somewhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The file.
    pub path: PathBuf,
    /// Why, in a sentence.
    pub why: String,
}

/// What a scan found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// The entries, one per desktop file ID, in the order found.
    pub found: Vec<Found>,
    /// Files that could not be read or parsed.
    pub skipped: Vec<Skipped>,
}

/// Find every desktop entry under `dirs`.
///
/// A directory that does not exist is not an error: most of the defaults do
/// not, on most machines. One that exists and cannot be listed is recorded
/// as skipped.
#[must_use]
pub fn scan(dirs: &DataDirs) -> Scan {
    let mut scan = Scan::default();
    let mut ids = std::collections::BTreeSet::new();
    for dir in dirs.dirs() {
        let root = dir.join("applications");
        let mut files = Vec::new();
        walk(&root, &root, 0, &mut files, &mut scan.skipped);
        for (id, path) in files {
            // Shadowed: an earlier directory, or an earlier file here, has
            // this ID already.
            if !ids.insert(id.clone()) {
                continue;
            }
            match fs::read(&path) {
                Ok(bytes) => match DesktopEntry::parse(&bytes) {
                    Ok(entry) => scan.found.push(Found { id, path, entry }),
                    Err(why) => scan.skipped.push(Skipped {
                        path,
                        why: why.to_string(),
                    }),
                },
                Err(why) => scan.skipped.push(Skipped {
                    path,
                    why: why.to_string(),
                }),
            }
        }
    }
    scan
}

/// Collect `(id, path)` for every `*.desktop` file under `dir`, sorted by
/// name at each level so that two files claiming one ID resolve the same
/// way on every machine.
fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<(String, PathBuf)>,
    skipped: &mut Vec<Skipped>,
) {
    let listing = match fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(why) => {
            skipped.push(Skipped {
                path: dir.to_path_buf(),
                why: why.to_string(),
            });
            return;
        }
    };
    let mut entries: Vec<(OsString, PathBuf)> = Vec::new();
    for item in listing {
        match item {
            Ok(item) => entries.push((item.file_name(), item.path())),
            Err(why) => skipped.push(Skipped {
                path: dir.to_path_buf(),
                why: why.to_string(),
            }),
        }
    }
    entries.sort();
    for (_, path) in entries {
        // `metadata` follows a symbolic link, which is how entries are often
        // installed; the depth bound is what keeps a link to a parent from
        // looping.
        let Ok(meta) = fs::metadata(&path) else {
            // A dangling link: nothing to read, and not an entry.
            continue;
        };
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                walk(root, &path, depth.saturating_add(1), out, skipped);
            }
            continue;
        }
        if path.extension() != Some(OsStr::new("desktop")) {
            continue;
        }
        match desktop_file_id(root, &path) {
            Some(id) => out.push((id, path)),
            None => skipped.push(Skipped {
                path,
                why: "its name is not UTF-8, so it has no desktop file ID".to_owned(),
            }),
        }
    }
}

/// `root/kde/konsole.desktop` -> `kde-konsole.desktop`.
///
/// `None` when a component of the relative path is not UTF-8: an ID is a
/// name other programs use to refer to the entry, and the specification's
/// IDs are text.
fn desktop_file_id(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = relative.iter().map(OsStr::to_str).collect();
    Some(parts?.join("-"))
}

/// The applications among `scan`'s entries, in `locale`, with the entries
/// that are not valid applications' reasons -- ready for a menu to filter
/// with [`crate::menu::shows_in_menu`].
///
/// An entry marked `Hidden` is left out whether or not the rest of it is
/// valid: a user's `Hidden=true` copy is often nothing else, and it is
/// meant to remove the system's entry, not to be reported as broken.
#[must_use]
pub fn apps(scan: &Scan, locale: Option<&Locale>) -> (Vec<App>, Vec<Skipped>) {
    let mut apps = Vec::new();
    let mut invalid = Vec::new();
    for found in &scan.found {
        if found.entry.boolean(crate::DESKTOP_ENTRY, "Hidden") == Some(true) {
            continue;
        }
        match App::from_entry(&found.entry, &found.id, locale) {
            Ok(app) => apps.push(app),
            Err(why) => invalid.push(Skipped {
                path: found.path.clone(),
                why: why.to_string(),
            }),
        }
    }
    (apps, invalid)
}

/// Whether `program` -- a `TryExec` value -- names a program that is here:
/// an absolute path to a file, or a name found in one of `path_var`'s
/// directories (`$PATH`).
///
/// Executable permission is checked where there is such a thing to check.
#[must_use]
pub fn program_exists(program: &str, path_var: Option<&OsStr>) -> bool {
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        return is_program(candidate);
    }
    // A relative path with a directory in it is relative to nothing a menu
    // knows: not found, rather than found somewhere by accident.
    if candidate.components().count() != 1 {
        return false;
    }
    path_var
        .is_some_and(|dirs| std::env::split_paths(dirs).any(|dir| is_program(&dir.join(program))))
}

fn is_program(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use scratchdir::ScratchDir;

    fn write(dir: &Path, relative: &str, text: &str) -> PathBuf {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        fs::write(&path, text).expect("write");
        path
    }

    fn entry(name: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec=prog\n")
    }

    /// **The first directory's entry for an ID wins, and the ID comes from
    /// the path under `applications`.**
    #[test]
    fn the_first_directory_wins_and_ids_come_from_paths() {
        let scratch = ScratchDir::new("desktopentry-scan");
        let user = scratch.path("user");
        let system = scratch.path("system");
        write(&user, "applications/calc.desktop", &entry("Mine"));
        write(&system, "applications/calc.desktop", &entry("System's"));
        write(
            &system,
            "applications/kde/konsole.desktop",
            &entry("Konsole"),
        );
        write(&system, "applications/readme.txt", "not an entry");
        let found = scan(&DataDirs::new(vec![user, system]));
        let names: Vec<(String, Option<String>)> = found
            .found
            .iter()
            .map(|f| (f.id.clone(), f.entry.string(crate::DESKTOP_ENTRY, "Name")))
            .collect();
        assert_eq!(
            names,
            [
                ("calc.desktop".to_owned(), Some("Mine".to_owned())),
                ("kde-konsole.desktop".to_owned(), Some("Konsole".to_owned())),
            ]
        );
        assert!(found.skipped.is_empty(), "{:?}", found.skipped);
    }

    /// **A user's `Hidden=true` copy removes the system's entry**, even when
    /// it says nothing else.
    #[test]
    fn a_users_hidden_copy_removes_the_systems_entry() {
        let scratch = ScratchDir::new("desktopentry-hidden");
        let user = scratch.path("user");
        let system = scratch.path("system");
        write(
            &user,
            "applications/ads.desktop",
            "[Desktop Entry]\nHidden=true\n",
        );
        write(&system, "applications/ads.desktop", &entry("Adverts"));
        write(&system, "applications/calc.desktop", &entry("Calculator"));
        let (apps, invalid) = apps(&scan(&DataDirs::new(vec![user, system])), None);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Calculator"]);
        assert!(
            invalid.is_empty(),
            "a hiding copy was reported as broken: {invalid:?}"
        );
    }

    /// A file that is not a valid entry is kept with its reason, and does
    /// not stop the rest being read.
    #[test]
    fn a_broken_file_is_reported_and_the_rest_are_read() {
        let scratch = ScratchDir::new("desktopentry-broken");
        let dir = scratch.path("data");
        write(&dir, "applications/a.desktop", "no group here\n");
        write(
            &dir,
            "applications/b.desktop",
            "[Desktop Entry]\nType=Application\n",
        );
        write(&dir, "applications/c.desktop", &entry("Fine"));
        let found = scan(&DataDirs::new(vec![dir]));
        assert_eq!(found.skipped.len(), 1);
        assert!(found.skipped[0].path.ends_with("a.desktop"));
        let (apps, invalid) = apps(&found, None);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Fine");
        assert_eq!(invalid.len(), 1);
        assert!(invalid[0].why.contains("Name"), "{}", invalid[0].why);
    }

    /// Directories that do not exist are the ordinary case, not an error.
    #[test]
    fn missing_directories_are_not_errors() {
        let scratch = ScratchDir::new("desktopentry-missing");
        let found = scan(&DataDirs::new(vec![scratch.path("nowhere")]));
        assert_eq!(found, Scan::default());
    }

    /// An environment of `pairs` and nothing else.
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    /// An absolute path on this host: `/x` here is `C:\x` on Windows, where
    /// a path with no drive is not absolute.
    fn abs(tail: &str) -> String {
        if cfg!(windows) {
            format!("C:\\{tail}")
        } else {
            format!("/{tail}")
        }
    }

    /// **The environment is read as the XDG specification says**: the
    /// user's directory first, the defaults when unset, relative paths
    /// ignored.
    #[test]
    fn the_environment_is_read_as_the_xdg_specification_says() {
        let home = abs("home");
        let dirs = DataDirs::from_env(env(&[("HOME", &home)]));
        assert_eq!(
            dirs.dirs(),
            [
                PathBuf::from(&home).join(".local").join("share"),
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        );

        let list =
            std::env::join_paths([abs("a"), "relative".to_owned(), abs("b")]).expect("a list");
        let list = list.into_string().expect("utf-8");
        let data = abs("data");
        let dirs = DataDirs::from_env(env(&[
            ("XDG_DATA_HOME", &data),
            ("XDG_DATA_DIRS", &list),
            ("HOME", "ignored"),
        ]));
        assert_eq!(
            dirs.dirs(),
            [
                PathBuf::from(&data),
                PathBuf::from(abs("a")),
                PathBuf::from(abs("b"))
            ]
        );

        // A relative directory is ignored, not joined to anything.
        let dirs = DataDirs::from_env(env(&[("XDG_DATA_HOME", "rel"), ("XDG_DATA_DIRS", "rel2")]));
        assert!(dirs.dirs().is_empty(), "{:?}", dirs.dirs());
    }

    /// `TryExec` finds a program by absolute path or on `$PATH`, and nothing
    /// else.
    #[test]
    fn a_program_is_found_by_path_or_on_the_search_path() {
        let scratch = ScratchDir::new("desktopentry-tryexec");
        let bin = scratch.path("bin");
        let program = write(&bin, "calc", "#!/bin/sh\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        // The scratch root is on the search path too, so that `bin/calc` --
        // a relative path with a directory in it -- *would* be found there if
        // such names were searched for.
        let path_var =
            std::env::join_paths([bin.clone(), scratch.dir().to_path_buf()]).expect("a PATH");
        assert!(program_exists(program.to_str().expect("utf-8"), None));
        assert!(program_exists("calc", Some(&path_var)));
        assert!(!program_exists("nope", Some(&path_var)));
        assert!(!program_exists("calc", None));
        assert!(
            !program_exists("bin/calc", Some(&path_var)),
            "a relative path is not searched"
        );
        assert!(
            !program_exists(bin.to_str().expect("utf-8"), None),
            "a directory is not a program"
        );
    }
}
