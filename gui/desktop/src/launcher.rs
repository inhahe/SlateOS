//! The programs the shell knows how to start: the built-in database, and the
//! paths the menus and shortcuts name.
//!
//! [`builtin_app_database`] is SlateOS's own programs, from the one list of
//! programs (`gui/programs`), as the start menu's fallback for those no
//! installed desktop entry names; [`FILE_MANAGER`],
//! [`SETTINGS`] and [`TERMINAL`] are the three paths the menus, the desktop
//! icons and the shortcuts start by name; `search_score` is how the start
//! menu's search ranks a program against what was typed; and
//! [`program_started`] says which program a launch really starts, for the
//! "recently used" list.
//!
//! This module also held `LauncherState`, a search-as-you-type launcher dialog
//! with its own keys, ranking by frecency, and drawing -- which nothing ever
//! constructed. The start menu's search does its job, and the standalone
//! launcher program (`apps/launcher`) keeps its own copy. It was deleted on
//! 2026-09-27 rather than taught keys it would never receive (`known-issues.md`
//! `TD-C-THE-DESKTOP-CRATE-CARRIES-A-SECOND-LAUNCHER-NOTHING-USES`).

/// The file manager: what opens a folder, from the desktop and from Super+E.
///
/// One constant for the three places that name it -- this database's entry,
/// the default Super+E shortcut, and the desktop's folder icons -- so that
/// moving the program is one edit rather than three that can disagree about
/// which program a folder opens in.
pub const FILE_MANAGER: &str = "/usr/bin/explorer";

/// What the file manager is started with to show the recycle bin: its view
/// of what was deleted, with Restore and Empty (lane E, `explorer
/// --recycle-bin`). What the desktop's Recycle Bin icon opens.
pub const RECYCLE_BIN_VIEW_ARG: &str = "--recycle-bin";

/// The settings application: what the start menu's Settings button and the
/// Settings shortcut start, and this database's entry for it.
pub const SETTINGS: &str = "/usr/bin/settings";

/// The terminal: what the start menu's Terminal button starts, and this
/// database's entry for it.
pub const TERMINAL: &str = "/usr/bin/terminal";

// ============================================================================
// Category
// ============================================================================

/// Category of a launchable item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    Application,
    System,
    Setting,
    File,
    Command,
}

// ============================================================================
// App entry
// ============================================================================

/// The picture a program is drawn with when its own cannot be: the icon
/// theme's generic program.
pub const GENERIC_PROGRAM_ICON: &str = "application-x-executable";

/// A launchable item in the database.
///
/// Either one of the shell's own ([`builtin_app_database`]) or an installed
/// program's desktop entry ([`AppEntry::from_desktop`]). `executable_path`
/// is the program's identity in both: pins and dragged buttons store it, so
/// it is what a click finds the entry again by -- see
/// `DesktopShell::launch_for`.
#[derive(Clone, Debug, Default)]
pub struct AppEntry {
    pub name: String,
    pub description: String,
    pub executable_path: String,
    pub keywords: Vec<String>,
    pub category: Category,
    pub launch_count: u32,
    /// The picture that stands for it: a name in the icon theme. `None`
    /// draws [`GENERIC_PROGRAM_ICON`].
    pub icon: Option<String>,
    /// The start menu folder it is listed in.
    pub folder: Folder,
    /// How to start it, when its desktop entry says: the program with its
    /// arguments. `None` starts `executable_path` with none.
    pub exec: Option<desktopentry::Exec>,
    /// Its additional actions -- the rows of its jump list.
    pub actions: Vec<desktopentry::Action>,
    /// It runs in a terminal, which is started to hold it.
    pub terminal: bool,
    /// The desktop file ID it was read from; `None` for the shell's own.
    pub desktop_id: Option<String>,
    /// The window class its windows will declare (`StartupWMClass`), when
    /// its entry says: one of the ways a window is known to be this program's
    /// (`DesktopShell::program_for_app_id`).
    pub wm_class: Option<String>,
}

/// The folders of the start menu's applications tree.
pub use desktopentry::menu::Category as Folder;

impl AppEntry {
    /// The shell's entry for an installed program's desktop entry, or `None`
    /// for one it cannot start: an entry with no `Exec` is started by D-Bus
    /// activation, and this system has no D-Bus.
    #[must_use]
    pub fn from_desktop(app: desktopentry::App) -> Option<Self> {
        let exec = app.exec?;
        let folder = Folder::of(&app.categories);
        Some(Self {
            name: app.name,
            // A comment says what it does; a generic name says what it is.
            // Either is a better second line than nothing.
            description: app.comment.or(app.generic_name).unwrap_or_default(),
            executable_path: exec.program(),
            keywords: app.keywords,
            category: if folder == Folder::Settings {
                Category::Setting
            } else {
                Category::Application
            },
            launch_count: 0,
            icon: app.icon,
            folder,
            exec: Some(exec),
            actions: app.actions,
            terminal: app.terminal,
            desktop_id: Some(app.id),
            wm_class: app.startup_wm_class,
        })
    }

    /// How to start it with nothing to open: its desktop entry's command
    /// line, or its program with no arguments -- inside a terminal when the
    /// entry says it runs in one.
    #[must_use]
    pub fn launch(&self) -> crate::hotkeys::Launch {
        self.launch_with(self.exec.as_ref())
    }

    /// How to start one of its actions (its jump list), by the action's id:
    /// `None` for an action it does not have, or one with no command line.
    #[must_use]
    pub fn launch_action(&self, id: &str) -> Option<crate::hotkeys::Launch> {
        let action = self.actions.iter().find(|a| a.id == id)?;
        Some(self.launch_with(Some(action.exec.as_ref()?)))
    }

    fn launch_with(&self, exec: Option<&desktopentry::Exec>) -> crate::hotkeys::Launch {
        use std::ffi::OsString;
        let argv: Vec<OsString> = match exec {
            Some(exec) => {
                let invocation = desktopentry::Invocation {
                    icon: self.icon.as_deref(),
                    name: &self.name,
                    location: None,
                };
                exec.command_lines(&[], &invocation)
                    .into_iter()
                    .next()
                    .unwrap_or_default()
            }
            None => vec![OsString::from(&self.executable_path)],
        };
        let mut argv = argv.into_iter();
        let Some(program) = argv.next() else {
            // A parsed line always has a program; this is the built-in
            // entry's own path, which cannot be empty either.
            return crate::hotkeys::Launch::program(&self.executable_path);
        };
        if self.terminal {
            // `-e`, as xterm and every terminal that copied it take a
            // command: the rest of the line is the program and its
            // arguments, each its own argument.
            let mut args = vec![OsString::from("-e"), program];
            args.extend(argv);
            return crate::hotkeys::Launch {
                program: std::path::PathBuf::from(TERMINAL),
                args,
            };
        }
        crate::hotkeys::Launch {
            program: std::path::PathBuf::from(program),
            args: argv.collect(),
        }
    }
}

/// The program `launch` starts: its own -- or, for a program started in a
/// terminal as [`AppEntry::launch`] wraps one (`terminal -e program ...`), the
/// one inside. The inverse of that wrapping, kept beside it so the two cannot
/// drift: a start menu that credited every terminal program to the terminal
/// would list "Terminal" as the one recently used program.
#[must_use]
pub fn program_started(launch: &crate::hotkeys::Launch) -> &std::ffi::OsStr {
    if launch.program.as_os_str() == TERMINAL
        && launch.args.first().is_some_and(|flag| flag == "-e")
        && let Some(inner) = launch.args.get(1)
    {
        return inner;
    }
    launch.program.as_os_str()
}

// ============================================================================
// Fuzzy matcher
// ============================================================================

/// Score how well `query` fuzzy-matches `target`.
///
/// Re-exported so the launcher's own callers keep their path, but the
/// implementation lives in `textfind` — this ranking was written out three
/// times (here, the Run dialog, and the standalone launcher application), the
/// second copy carrying a comment promising it stayed in step with this one.
/// See `textfind::fuzzy_score` for what the score rewards.
pub use guitk::textfind::fuzzy_score;

/// How well `query` finds `entry`, or `None` if it does not: the name
/// counts double, the description once, and a keyword a little over the
/// name. `pub(crate)` for the start menu's search field, which ranks the
/// same programs by the same rule this launcher does -- two searches over one
/// list that disagreed about the best match would be a strange thing to find.
pub(crate) fn search_score(query: &str, entry: &AppEntry) -> Option<u32> {
    let mut best: Option<u32> = None;

    if let Some(s) = fuzzy_score(query, &entry.name) {
        let boosted = s.saturating_mul(2);
        best = Some(best.map_or(boosted, |b: u32| b.max(boosted)));
    }

    if let Some(s) = fuzzy_score(query, &entry.description) {
        best = Some(best.map_or(s, |b: u32| b.max(s)));
    }

    for kw in &entry.keywords {
        if let Some(s) = fuzzy_score(query, kw) {
            let boosted = s.saturating_add(5);
            best = Some(best.map_or(boosted, |b: u32| b.max(boosted)));
        }
    }

    best
}

// ============================================================================
// Built-in app database
// ============================================================================

/// The applications this desktop knows how to start: SlateOS's own programs,
/// from the one list of programs (`programs::built_in`, design-decisions
/// §1425), as the start menu's entries.
///
/// The fallback behind the programs installed on the machine: an installed
/// entry with a built-in entry's desktop file ID replaces it
/// (`programs::with_built_in`, `DesktopShell::set_programs`, design-decisions
/// §1445). Until 2026-09-27 this was a table
/// the shell kept for itself -- ten programs, one of them started by the
/// wrong path (`/usr/bin/sysinfo`, the command-line tool) -- beside three
/// other lists that disagreed with it; `gui/programs/INVENTORY.md` records
/// what each held and where it went.
///
/// Untranslated: the built-in entries carry no translations yet, so a locale
/// would change nothing.
#[must_use]
pub fn builtin_app_database() -> Vec<AppEntry> {
    programs::built_in(None)
        .into_iter()
        .filter_map(AppEntry::from_desktop)
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // -------- fuzzy_score happy path --------

    #[test]
    fn fuzzy_score_empty_query_matches_everything() {
        // Empty query is the "freshly shown" state — must match so the
        // initial list is non-empty.
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert_eq!(fuzzy_score("", ""), Some(0));
    }

    #[test]
    fn fuzzy_score_query_longer_than_target_fails() {
        assert_eq!(fuzzy_score("abcdef", "abc"), None);
    }

    #[test]
    fn fuzzy_score_non_matching_returns_none() {
        // 'z' is not in "abc" — fuzzy match must fail.
        assert_eq!(fuzzy_score("z", "abc"), None);
        // Out-of-order characters: 'ba' cannot fuzzy-match "abc" because
        // we already consumed 'a' before reaching 'b'.
        assert_eq!(fuzzy_score("ba", "abc"), None);
    }

    #[test]
    fn fuzzy_score_is_case_insensitive() {
        let a = fuzzy_score("ABC", "abcdef").expect("matches");
        let b = fuzzy_score("abc", "ABCDEF").expect("matches");
        assert_eq!(a, b);
    }

    #[test]
    fn fuzzy_score_prefix_beats_substring() {
        // Prefix match gets +50 bonus, so "fi" against "file" must
        // outscore "fi" against "wifi".
        let prefix = fuzzy_score("fi", "file").expect("matches");
        let middle = fuzzy_score("fi", "wifi").expect("matches");
        assert!(
            prefix > middle,
            "prefix score {prefix} should beat substring {middle}"
        );
    }

    // -------- The programs the menus start --------

    /// Every binary this workspace builds, by name: a crate's package name
    /// when it has a `src/main.rs`, each `[[bin]]` it declares, and each
    /// `src/bin/*.rs`. Read from the manifests under the workspace's member
    /// globs (`apps/*`, `gui/*`, `init/*`, `net/*`, `userspace/*` in the root
    /// `Cargo.toml`) -- line by line, since a name is all this needs.
    fn workspace_binaries() -> std::collections::BTreeSet<String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut bins = std::collections::BTreeSet::new();
        for zone in ["apps", "gui", "init", "net", "userspace"] {
            let dir = std::fs::read_dir(root.join(zone)).expect("a member directory");
            for krate in dir.flatten() {
                let krate = krate.path();
                let Ok(manifest) = std::fs::read_to_string(krate.join("Cargo.toml")) else {
                    continue;
                };
                let mut section = String::new();
                for line in manifest.lines().map(str::trim) {
                    if line.starts_with('[') {
                        section = line.to_string();
                        continue;
                    }
                    let Some(value) = line.strip_prefix("name") else {
                        continue;
                    };
                    let Some(value) = value.trim_start().strip_prefix('=') else {
                        continue;
                    };
                    let name = value.trim().trim_matches('"').to_string();
                    let is_main = section == "[package]" && krate.join("src/main.rs").exists();
                    if is_main || section == "[[bin]]" {
                        bins.insert(name);
                    }
                }
                if let Ok(extra) = std::fs::read_dir(krate.join("src/bin")) {
                    for bin in extra.flatten() {
                        let path = bin.path();
                        if path.extension().is_some_and(|e| e == "rs")
                            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                        {
                            bins.insert(stem.to_string());
                        }
                    }
                }
            }
        }
        bins
    }

    /// **Every program the start menu and the power menu can start is one this
    /// workspace builds**, named by a path with no space in it.
    ///
    /// Nothing else connects these strings to programs that exist, and three
    /// times one named nothing: the screenshot shortcut's flags were part of
    /// its path until 2026-09-17, the power menu started `/sbin/shutdown` and
    /// `/usr/bin/logout` until 2026-09-25, and three settings entries started
    /// `/usr/bin/settings --display` and its neighbours -- a file name with a
    /// space and a flag in it. Each drew, each launched, and each started
    /// nothing, with every other test green. The shortcuts are held to this
    /// list by `hotkeys`' own test, so they are covered here too.
    ///
    /// The directory is not checked: no image installs the desktop's programs
    /// anywhere yet, so there is no layout to hold `/usr/bin` against.
    #[test]
    fn every_program_the_menus_start_is_one_this_workspace_builds() {
        let bins = workspace_binaries();
        // A floor, so an empty scan -- a moved manifest, a wrong root -- fails
        // as itself rather than as every program being missing, or none.
        assert!(bins.len() > 50, "the scan found {} binaries", bins.len());

        let mut programs: Vec<(String, String)> = builtin_app_database()
            .into_iter()
            .map(|entry| (entry.name, entry.executable_path))
            .collect();
        for choice in crate::power::PowerChoice::ALL {
            if let Some(launch) = choice.command() {
                let program = launch.program.to_str().expect("a literal path").to_string();
                programs.push((choice.label().to_string(), program));
            }
        }
        for (what, program) in programs {
            assert!(
                program.starts_with('/'),
                "{what} starts {program:?}, which is not a path"
            );
            assert!(
                !program.contains(char::is_whitespace),
                "{what} starts {program:?}: a program path with a space in it is a command line \
                 in disguise, looked up whole as one file name"
            );
            let name = program.rsplit('/').next().unwrap_or(&program);
            assert!(
                bins.contains(name),
                "{what} starts {program:?}, and nothing in this workspace builds a program \
                 called {name:?}"
            );
        }
    }

    /// A search for what the three folded entries covered still finds a way
    /// in: Settings, which owns every one of those pages.
    #[test]
    fn a_search_for_a_settings_page_finds_settings() {
        let settings = builtin_app_database()
            .into_iter()
            .find(|entry| entry.executable_path == SETTINGS)
            .expect("Settings is in the database");
        for word in ["display", "resolution", "wifi", "vpn", "sound", "volume"] {
            assert!(
                search_score(word, &settings).is_some(),
                "{word:?} does not find Settings"
            );
        }
    }
}
