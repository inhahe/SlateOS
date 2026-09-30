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

/// How Settings is asked to open on one of its pages: `settings --page
/// <name>`, with the name `SettingsPage::name` gives the page (lane E,
/// 3fc93a617, `requests/c-e-settings-opens-on-the-page-it-is-asked-for.md`).
pub const SETTINGS_PAGE_OPTION: &str = "--page";

/// Settings' Notifications page, by the name it answers to: what a
/// notification's menu opens.
pub const NOTIFICATIONS_PAGE: &str = "notifications";

/// Settings, asked to open on `page`.
///
/// The option and the name are two arguments, never one string: a program
/// path with the option inside it names a file that cannot exist -- the
/// defect `every_program_the_menus_start_is_one_this_workspace_builds`
/// exists to catch.
#[must_use]
pub fn settings_page(page: &str) -> crate::hotkeys::Launch {
    crate::hotkeys::Launch {
        program: std::path::PathBuf::from(SETTINGS),
        args: vec![SETTINGS_PAGE_OPTION.into(), page.into()],
        dir: None,
    }
}

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
    /// The kinds of file it opens (`MimeType`), as its entry lists them:
    /// what puts it in a file's "Open with" (see [`Self::opens`]).
    pub mime_types: Vec<String>,
}

/// A kind of file, as a program's `MimeType` is matched against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileKind {
    /// Its MIME type: `inode/directory` for a folder.
    pub mime: String,
    /// Whether it is text of some kind -- a shell script or JSON as much as
    /// `text/*` -- so a program for plain text opens it.
    pub text: bool,
}

impl FileKind {
    /// The kind of the file or folder at `path`: a folder is
    /// `inode/directory`; a file's kind is the toolkit's reading of its
    /// extension.
    #[must_use]
    pub fn of(path: &std::path::Path, is_dir: bool) -> Self {
        if is_dir {
            return Self {
                mime: String::from("inode/directory"),
                text: false,
            };
        }
        let extension = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let info = guitk::filetypes::detect_from_extension(extension);
        Self {
            mime: info.mime_type.to_owned(),
            text: info.is_text,
        }
    }

    /// Whether this is a folder.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.mime == "inode/directory"
    }
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
            mime_types: app.mime_types,
        })
    }

    /// Whether it opens files of `kind`: its `MimeType` lists the kind; or
    /// lists `text/plain` and the kind is text; or lists
    /// `application/octet-stream`, which every file is -- the sub-classes the
    /// shared MIME-info specification gives every type -- and the kind is a
    /// file. Case is not asked about, as MIME types' is not.
    #[must_use]
    pub fn opens(&self, kind: &FileKind) -> bool {
        self.mime_types.iter().any(|listed| {
            listed.eq_ignore_ascii_case(&kind.mime)
                || (kind.text && listed.eq_ignore_ascii_case("text/plain"))
                || (!kind.is_dir() && listed.eq_ignore_ascii_case("application/octet-stream"))
        })
    }

    /// How to start it on `files`: its desktop entry's command line with the
    /// files put in -- one program for all of them, or one each where the
    /// line takes one file at a time (`%f`) -- or, for an entry with no
    /// command line, its program once per file. Inside a terminal when the
    /// entry says it runs in one.
    #[must_use]
    pub fn launch_opening(&self, files: &[&std::path::Path]) -> Vec<crate::hotkeys::Launch> {
        let Some(exec) = &self.exec else {
            return files
                .iter()
                .map(|file| crate::hotkeys::Launch::opening(&self.executable_path, file))
                .collect();
        };
        let targets: Vec<desktopentry::Target> = files
            .iter()
            .map(|file| desktopentry::Target::File(file.to_path_buf()))
            .collect();
        let invocation = desktopentry::Invocation {
            icon: self.icon.as_deref(),
            name: &self.name,
            location: None,
        };
        exec.command_lines(&targets, &invocation)
            .into_iter()
            .map(|argv| self.launch_line(argv))
            .collect()
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
        self.launch_line(argv)
    }

    /// Start the command line `argv`, program first -- inside a terminal
    /// when the entry says it runs in one.
    fn launch_line(&self, argv: Vec<std::ffi::OsString>) -> crate::hotkeys::Launch {
        use std::ffi::OsString;
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
                dir: None,
            };
        }
        crate::hotkeys::Launch {
            program: std::path::PathBuf::from(program),
            args: argv.collect(),
            dir: None,
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

    /// **Every Settings page the desktop asks for is one Settings answers
    /// to**: a notification's menu's, and each `settings --page` line in
    /// SlateOS's own programs' entries -- Settings' jump list of Display,
    /// Network and Sound. And the desktop's own asks with two arguments.
    ///
    /// Settings refuses a page it does not know -- a message on its terminal
    /// and no window -- so a misspelt name is a row that opens nothing, with
    /// every other test green. Settings is another lane's program, a binary
    /// this crate cannot link, so its source is read: the names are the
    /// string arms of `SettingsPage::name`.
    #[test]
    fn every_settings_page_the_desktop_asks_for_is_one_settings_has() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = std::fs::read_to_string(root.join("apps/settings/src/main.rs"))
            .expect("Settings' source")
            .replace("\r\n", "\n");
        let body = source
            .split("fn name(self) -> &'static str {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .expect("SettingsPage::name, which gives each page its name");
        let names: Vec<&str> = body
            .lines()
            .filter_map(|line| line.split("=> \"").nth(1)?.split('"').next())
            .collect();
        // A floor, so a moved function fails as itself and not as a missing
        // page.
        assert!(
            names.len() > 20,
            "found {} page names: {names:?}",
            names.len()
        );

        let notifications = settings_page(NOTIFICATIONS_PAGE);
        assert_eq!(notifications.program, std::path::PathBuf::from(SETTINGS));
        assert_eq!(
            notifications.args,
            [SETTINGS_PAGE_OPTION, NOTIFICATIONS_PAGE]
        );

        let mut asked = vec![notifications];
        for entry in builtin_app_database() {
            asked.push(entry.launch());
            for action in &entry.actions {
                asked.extend(entry.launch_action(&action.id));
            }
        }
        let pages: Vec<String> = asked
            .iter()
            .filter(|launch| launch.program == std::path::Path::new(SETTINGS))
            .filter_map(|launch| match launch.args.as_slice() {
                [option, page] if option == SETTINGS_PAGE_OPTION => {
                    Some(page.to_str().expect("a page's name is text").to_owned())
                }
                _ => None,
            })
            .collect();
        // The notifications page and Settings' three actions, at least: a
        // floor, so an entry that stopped naming pages fails here.
        assert!(pages.len() >= 4, "found {pages:?}");
        for page in &pages {
            assert!(
                names.contains(&page.as_str()),
                "Settings has no page called {page:?}"
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

    // -------- what a program opens, and how it is started on files --------

    /// A program whose entry says `exec`, `types` and `terminal`.
    fn program(exec: &str, types: &[&str], terminal: bool) -> AppEntry {
        let exec = desktopentry::Exec::parse(exec).unwrap();
        AppEntry {
            name: String::from("Program"),
            executable_path: exec.program(),
            exec: Some(exec),
            mime_types: types.iter().map(|t| (*t).to_owned()).collect(),
            terminal,
            ..AppEntry::default()
        }
    }

    fn kind(mime: &str, text: bool) -> FileKind {
        FileKind {
            mime: mime.to_owned(),
            text,
        }
    }

    /// **A program opens the kinds its entry lists** -- in any case -- and
    /// a program for plain text opens every kind of text, one for any file
    /// every file but not a folder.
    #[test]
    fn a_program_opens_what_its_entry_lists() {
        let viewer = program("viewer %f", &["image/png", "Image/JPEG"], false);
        assert!(viewer.opens(&kind("image/png", false)));
        assert!(viewer.opens(&kind("image/jpeg", false)));
        assert!(!viewer.opens(&kind("image/gif", false)));
        let editor = program("editor %F", &["text/plain"], false);
        assert!(editor.opens(&kind("text/x-rust", true)));
        assert!(editor.opens(&kind("application/json", true)));
        assert!(!editor.opens(&kind("image/png", false)));
        let hex = program("hexeditor %F", &["application/octet-stream"], false);
        assert!(hex.opens(&kind("image/png", false)));
        assert!(hex.opens(&kind("text/plain", true)));
        assert!(!hex.opens(&kind("inode/directory", false)));
        let files = program("explorer %f", &["inode/directory"], false);
        assert!(files.opens(&kind("inode/directory", false)));
        assert!(!files.opens(&kind("image/png", false)));
        assert!(!program("calculator", &[], false).opens(&kind("text/plain", true)));
    }

    /// **The kind of a path is the toolkit's reading of its extension**; a
    /// folder is a folder whatever it is called.
    #[test]
    fn the_kind_of_a_path_is_its_extensions() {
        let png = FileKind::of(std::path::Path::new("/p/photo.PNG"), false);
        assert_eq!(png.mime, "image/png");
        assert!(!png.text && !png.is_dir());
        let json = FileKind::of(std::path::Path::new("/p/data.json"), false);
        assert!(json.text, "a JSON file is text");
        let folder = FileKind::of(std::path::Path::new("/p/photos.png"), true);
        assert_eq!(folder.mime, "inode/directory");
        assert!(folder.is_dir());
    }

    /// **A program is started on files as its command line says**: one for
    /// all where it takes them all, one each where it takes one; inside a
    /// terminal when it runs in one; with no command line, its program once
    /// per file.
    #[test]
    fn a_program_is_started_on_files_as_its_line_says() {
        use std::ffi::OsString;
        use std::path::Path;
        let (a, b) = (Path::new("/p/a.txt"), Path::new("/p/b.txt"));
        let all = program("editor --new %F", &[], false).launch_opening(&[a, b]);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].program, Path::new("editor"));
        assert_eq!(
            all[0].args,
            [
                OsString::from("--new"),
                OsString::from("/p/a.txt"),
                OsString::from("/p/b.txt")
            ]
        );
        let each = program("viewer %f", &[], false).launch_opening(&[a, b]);
        assert_eq!(each.len(), 2);
        assert_eq!(each[1].args, [OsString::from("/p/b.txt")]);
        let boxed = program("vim %f", &[], true).launch_opening(&[a]);
        assert_eq!(boxed[0].program, Path::new(TERMINAL));
        assert_eq!(
            boxed[0].args,
            [
                OsString::from("-e"),
                OsString::from("vim"),
                OsString::from("/p/a.txt")
            ]
        );
        let bare = AppEntry {
            executable_path: String::from("/usr/bin/tool"),
            ..AppEntry::default()
        };
        let launches = bare.launch_opening(&[a, b]);
        assert_eq!(launches.len(), 2);
        assert_eq!(launches[0].program, Path::new("/usr/bin/tool"));
        assert_eq!(launches[0].args, [OsString::from("/p/a.txt")]);
    }
}
