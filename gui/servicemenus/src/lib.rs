//! Context-menu extensions: programs adding items to the menu a right-click on
//! a file opens -- "Compress", "Rotate", "Open a terminal here".
//!
//! `design.txt` asks whether programs should be allowed to, and answers:
//! *allow it, but with controls* -- a program must ask to; its items load
//! lazily ("don't load the program just to show the menu"); a settings page
//! lists every extension and can turn each off; and a handler slower than
//! 200 ms is skipped. `design-decisions.md` §1448 has the reasoning.
//!
//! # The format: KDE's service menus
//!
//! A service menu is a small desktop-entry file, as KDE's file manager reads
//! them -- which is what makes one a ported program already ships work here
//! as it is:
//!
//! ```ini
//! [Desktop Entry]
//! Type=Service
//! MimeType=image/png;image/jpeg;
//! Actions=rotate;
//! X-KDE-Submenu=Image
//!
//! [Desktop Action rotate]
//! Name=Rotate right
//! Icon=object-rotate-right
//! Exec=mogrify -rotate 90 %f
//! ```
//!
//! found under `kio/servicemenus/` (and the older `kservices5/ServiceMenus/`)
//! in each XDG data directory, the user's (`~/.local/share`) before the
//! system's. A menu is known by its file name, and the user's copy of a name
//! shadows the system's -- `Hidden=true` in it removes the system's menu, as
//! a desktop entry's does.
//!
//! What this reads of KDE's keys: `MimeType` (with `all/all`, `all/allfiles`,
//! `inode/directory` and `type/*`), `Actions` (with `_SEPARATOR_`) and each
//! action's `Name`, `Icon` and `Exec`, `X-KDE-Submenu`,
//! `X-KDE-Priority=TopLevel`, the three `X-KDE-*NumberOfUrls` keys,
//! `X-KDE-Require=Write`, `X-KDE-Protocol` and `X-KDE-Protocols`. KDE reads
//! the last four of those with commas between the elements of a list, the
//! desktop entry specification's lists with semicolons; either is read here,
//! since authors write both and each means a list.
//!
//! A kind of file is also each kind it is a sub-class of, as the MIME
//! database says: a menu for `text/plain` is for a shell script, and one for
//! `application/octet-stream` is for any file. The two rules the
//! specification states for every type are applied here -- `text/*` is
//! `text/plain`, everything not a folder is `application/octet-stream` --
//! and the rest come from the caller, which knows its files
//! ([`Target::inherits`]).
//!
//! A menu that shows only when a condition this desktop cannot check holds --
//! `X-KDE-ShowIfRunning`, `X-KDE-ShowIfDBusCall`, `X-KDE-AuthorizeAction` -- is
//! not offered at all, and says why ([`Scan::skipped`]): an item shown when
//! its author meant it hidden is worse than one missing.
//!
//! # The controls
//!
//! - **Asking to**: a menu the system installed -- under a system data
//!   directory, put there by the package manager -- is on unless the user
//!   turns it off. One a user dropped into their own directory is off until
//!   they turn it on ([`Choices`]): nothing adds to their menus without them
//!   saying so. That is KDE's rule too, which shows a user's own menu only
//!   once it is marked executable. The package manager's capabilities, when
//!   they exist, are the finer version of the first half.
//! - **Lazily**: an item is its declaration. Nothing runs to show a menu; the
//!   program starts only when its item is chosen.
//! - **A page to turn each off**: every menu found, on or off, is listed by
//!   [`scan`]; `context-menus.yaml` records the user's choices, which the
//!   Settings application writes and every file menu reads.
//! - **200 ms**: there is no handler to time -- a declaration costs a file
//!   read at scan time, not a call per right-click -- so nothing is skipped
//!   for slowness, and nothing can make a menu slow to open.

use desktopentry::{DesktopEntry, Exec, Invocation, Locale, Target as ExecTarget};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

mod choices;

pub use choices::{CONFIG_NAME, Choices, ChoicesFile};

/// The group every service menu's keys are in.
const GROUP: &str = "Desktop Entry";

/// The largest service-menu file read. A real one is a few hundred bytes.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Where under each data directory service menus are looked for: KDE's
/// current place, then its older one.
const SUBDIRS: [&str; 2] = ["kio/servicemenus", "kservices5/ServiceMenus"];

/// The `Actions=` element that is a line between items rather than an item.
const SEPARATOR: &str = "_SEPARATOR_";

/// The keys naming a condition this desktop cannot evaluate.
const CONDITIONS: [&str; 3] = [
    "X-KDE-ShowIfRunning",
    "X-KDE-ShowIfDBusCall",
    "X-KDE-AuthorizeAction",
];

// ============================================================================
// Where menus are
// ============================================================================

/// Whose a menu is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Installed with the system: on unless the user turns it off.
    System,
    /// Put in the user's own data directory: off until they turn it on.
    User,
}

/// The data directories service menus are found in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// The user's (`$XDG_DATA_HOME`, or `~/.local/share`), if there is one.
    pub user: Option<PathBuf>,
    /// The system's, in precedence order (`$XDG_DATA_DIRS`, or
    /// `/usr/local/share` and `/usr/share`).
    pub system: Vec<PathBuf>,
}

impl Dirs {
    /// The directories the environment names, read as the XDG Base Directory
    /// specification reads them -- a relative path ignored, as it requires.
    #[must_use]
    pub fn standard() -> Self {
        Self::from_env(|name| std::env::var_os(name))
    }

    /// [`standard`](Self::standard), from `get` -- a table, in a test.
    #[must_use]
    pub fn from_env(get: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |name: &str| get(name).filter(|value| !value.is_empty());
        let user = match set("XDG_DATA_HOME") {
            Some(home) => Some(PathBuf::from(home)).filter(|p| p.is_absolute()),
            None => set("HOME").map(|home| PathBuf::from(home).join(".local").join("share")),
        };
        let system = match set("XDG_DATA_DIRS") {
            Some(list) => std::env::split_paths(&list)
                .filter(|dir| dir.is_absolute())
                .collect(),
            None => vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ],
        };
        Self { user, system }
    }

    /// Every directory to look in, with whose it is, in precedence order.
    fn roots(&self) -> Vec<(PathBuf, Origin)> {
        let mut out = Vec::new();
        for (base, origin) in self
            .user
            .iter()
            .map(|dir| (dir, Origin::User))
            .chain(self.system.iter().map(|dir| (dir, Origin::System)))
        {
            for sub in SUBDIRS {
                out.push((base.join(sub), origin));
            }
        }
        out
    }
}

// ============================================================================
// A menu
// ============================================================================

/// How many things a menu may be offered for at once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// `X-KDE-MinNumberOfUrls`.
    pub min: Option<usize>,
    /// `X-KDE-MaxNumberOfUrls`.
    pub max: Option<usize>,
    /// `X-KDE-RequiredNumberOfUrls`: one of these exactly, when not empty.
    pub exactly: Vec<usize>,
}

impl Counts {
    /// Whether `n` things are allowed.
    #[must_use]
    pub fn allows(&self, n: usize) -> bool {
        self.min.is_none_or(|min| n >= min)
            && self.max.is_none_or(|max| n <= max)
            && (self.exactly.is_empty() || self.exactly.contains(&n))
    }
}

/// One item a menu adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuAction {
    /// Its identifier, as `Actions=` lists it.
    pub id: String,
    /// What the item says, in the reader's language.
    pub name: String,
    /// Its icon, a name in the icon theme or a path.
    pub icon: Option<String>,
    /// What choosing it starts.
    pub exec: Exec,
    /// Whether a line goes above it: an `_SEPARATOR_` came between it and
    /// the item before it in `Actions=`. Never set on the first item.
    pub separator_before: bool,
}

/// A service menu, read. See the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceMenu {
    /// Its file's name -- what it is known by, and what a user's copy of the
    /// same name shadows.
    pub id: OsString,
    /// Whose it is.
    pub origin: Origin,
    /// Its file.
    pub path: PathBuf,
    /// The kinds of thing it is for: MIME types, `type/*`, `all/all`,
    /// `all/allfiles`, `inode/directory`.
    pub mime_types: Vec<String>,
    /// The submenu its items go in (`X-KDE-Submenu`), in the reader's
    /// language.
    pub submenu: Option<String>,
    /// Whether its items go at the top of the menu (`X-KDE-Priority=TopLevel`)
    /// rather than under an "Actions" submenu.
    pub top_level: bool,
    /// How many things it may be offered for at once.
    pub counts: Counts,
    /// Whether every thing must be writable (`X-KDE-Require=Write`).
    pub requires_write: bool,
    /// Its items, in the order `Actions=` lists them.
    pub actions: Vec<MenuAction>,
    /// The items it lists and does not offer, each with why -- a missing
    /// name, a missing or refused command -- for the settings page to say.
    pub unusable: Vec<String>,
}

/// A thing a menu is offered for: a file or a folder, with what kind it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Its path.
    pub path: PathBuf,
    /// Its MIME type -- `inode/directory` for a folder.
    pub mime: String,
    /// The types it is also, by the MIME database's `sub-class-of` -- a
    /// shell script is also `text/plain`. The two every type has (see the
    /// module docs) need not be listed.
    pub inherits: Vec<String>,
    /// Whether it is a folder.
    pub is_dir: bool,
    /// Whether it may be written.
    pub writable: bool,
}

impl Target {
    /// `path`, as what it is: a folder, or a file of type `mime`, writable or
    /// not by what the file system says of it now. It inherits nothing but
    /// what every type does; set [`inherits`](Self::inherits) for more.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>, mime: &str) -> Self {
        let path = path.into();
        // A path that cannot be looked at is neither a folder nor writable,
        // so a menu requiring either is not offered for it: the reading that
        // cannot put a writing command on a file it would fail on.
        let meta = fs::metadata(&path).ok();
        let is_dir = meta.as_ref().is_some_and(fs::Metadata::is_dir);
        Self {
            writable: meta.as_ref().is_some_and(|m| !m.permissions().readonly()),
            mime: if is_dir {
                String::from("inode/directory")
            } else {
                mime.to_owned()
            },
            inherits: Vec::new(),
            is_dir,
            path,
        }
    }
}

impl ServiceMenu {
    /// Whether this menu is for `targets`: each is a kind it names, there are
    /// as many as it allows, and each is writable where it requires that.
    /// Nothing is for no targets.
    #[must_use]
    pub fn applies_to(&self, targets: &[Target]) -> bool {
        !targets.is_empty()
            && self.counts.allows(targets.len())
            && targets.iter().all(|t| {
                (!self.requires_write || t.writable)
                    && self.mime_types.iter().any(|m| mime_matches(m, t))
            })
    }
}

impl MenuAction {
    /// The command lines choosing this item on `targets` starts, program
    /// first: one for all of them, or one each where the line takes one file
    /// at a time (`%f`) -- `desktopentry`'s reading of the specification.
    #[must_use]
    pub fn command_lines(&self, targets: &[Target], menu: &ServiceMenu) -> Vec<Vec<OsString>> {
        let files: Vec<ExecTarget> = targets
            .iter()
            .map(|t| ExecTarget::File(t.path.clone()))
            .collect();
        let invocation = Invocation {
            icon: self.icon.as_deref(),
            name: &self.name,
            location: Some(&menu.path),
        };
        self.exec.command_lines(&files, &invocation)
    }
}

/// Whether the pattern `pattern` names `target`'s kind: its own type, a type
/// it inherits, or a `type/*` any of those is in.
fn mime_matches(pattern: &str, target: &Target) -> bool {
    match pattern {
        "all/all" => true,
        "all/allfiles" => !target.is_dir,
        _ => {
            let kinds = || {
                core::iter::once(target.mime.as_str())
                    .chain(target.inherits.iter().map(String::as_str))
            };
            match pattern.strip_suffix("/*") {
                Some(major) => kinds().any(|kind| {
                    kind.split_once('/')
                        .is_some_and(|(m, _)| m.eq_ignore_ascii_case(major))
                }),
                None => {
                    kinds().any(|kind| kind.eq_ignore_ascii_case(pattern))
                        || every_type_inherits(pattern, target)
                }
            }
        }
    }
}

/// The two sub-classes the shared MIME-info specification gives every type:
/// each `text/*` is `text/plain`, and each that is not an `inode/*` -- a
/// folder, a device -- is `application/octet-stream`.
fn every_type_inherits(pattern: &str, target: &Target) -> bool {
    let major = |kind: &str| kind.split_once('/').map(|(m, _)| m.to_ascii_lowercase());
    if pattern.eq_ignore_ascii_case("text/plain") {
        major(&target.mime).as_deref() == Some("text")
    } else if pattern.eq_ignore_ascii_case("application/octet-stream") {
        major(&target.mime).as_deref() != Some("inode")
    } else {
        false
    }
}

// ============================================================================
// Finding menus
// ============================================================================

/// A file that looked like a service menu and is not offered, and why.
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
    /// Every usable menu, the user's first, each name once.
    pub menus: Vec<ServiceMenu>,
    /// What was found and not used, and why -- for the settings page to say.
    pub skipped: Vec<Skipped>,
}

impl Scan {
    /// The menus for `targets` that are on by `choices`: what a file menu
    /// shows, in the order they were found.
    #[must_use]
    pub fn offered(&self, choices: &Choices, targets: &[Target]) -> Vec<&ServiceMenu> {
        self.menus
            .iter()
            .filter(|m| choices.is_on(m) && m.applies_to(targets))
            .collect()
    }
}

/// Every service menu in `dirs`, read in `locale`.
///
/// A name found in the user's directory shadows the same name in the
/// system's -- a copy saying `Hidden=true` removes it. A file that cannot be
/// used is listed in [`Scan::skipped`] and still shadows its name: a user's
/// copy gone wrong is said, not quietly replaced by the system's.
#[must_use]
pub fn scan(dirs: &Dirs, locale: Option<&Locale>) -> Scan {
    let mut out = Scan::default();
    let mut seen: Vec<OsString> = Vec::new();
    for (root, origin) in dirs.roots() {
        // A directory that is not there has no menus in it -- the ordinary
        // state of most of them.
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension() == Some(OsStr::new("desktop")) && p.is_file())
            .collect();
        files.sort();
        for path in files {
            let Some(id) = path.file_name().map(OsStr::to_owned) else {
                continue;
            };
            if seen.contains(&id) {
                continue;
            }
            seen.push(id.clone());
            match read_menu(&path, id, origin, locale) {
                Ok(Some(menu)) => out.menus.push(menu),
                // Hidden: the name is removed, and that is all.
                Ok(None) => {}
                Err(why) => out.skipped.push(Skipped { path, why }),
            }
        }
    }
    out
}

/// The menu in the file at `path`; `None` when it says `Hidden=true`.
fn read_menu(
    path: &Path,
    id: OsString,
    origin: Origin,
    locale: Option<&Locale>,
) -> Result<Option<ServiceMenu>, String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("could not be read ({e})"))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!(
            "is {size} bytes, over the {MAX_FILE_BYTES}-byte limit for a service menu"
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("could not be read ({e})"))?;
    let entry = DesktopEntry::parse(&bytes).map_err(|e| format!("is not a desktop entry ({e})"))?;
    parse_menu(&entry, id, origin, path, locale)
}

/// The menu `entry` describes.
///
/// # Errors
///
/// A sentence saying why the entry is not a menu this desktop offers.
fn parse_menu(
    entry: &DesktopEntry,
    id: OsString,
    origin: Origin,
    path: &Path,
    locale: Option<&Locale>,
) -> Result<Option<ServiceMenu>, String> {
    if !entry.has_group(GROUP) {
        return Err(String::from("has no [Desktop Entry] group"));
    }
    if entry.boolean(GROUP, "Hidden") == Some(true) {
        return Ok(None);
    }
    match entry.string(GROUP, "Type").as_deref() {
        Some("Service") => {}
        Some(other) => return Err(format!("is of type {other}, not a service menu (Service)")),
        None => return Err(String::from("has no Type")),
    }
    if let Some(condition) = CONDITIONS
        .iter()
        .find(|key| entry.raw(GROUP, key).is_some())
    {
        return Err(format!(
            "shows only when {condition} says so, which this desktop cannot check"
        ));
    }
    if let Some(key) = not_for_files(entry) {
        return Err(format!(
            "is only for places other than this machine's files ({key})"
        ));
    }
    let mime_types = entry.list(GROUP, "MimeType");
    if mime_types.is_empty() {
        return Err(String::from("names no kind of file it is for (MimeType)"));
    }
    let mut actions: Vec<MenuAction> = Vec::new();
    let mut unusable = Vec::new();
    // A line is owed before the next item, when there is one before it.
    let mut line_owed = false;
    for action_id in entry.list(GROUP, "Actions") {
        if action_id == SEPARATOR {
            line_owed = !actions.is_empty();
            continue;
        }
        let group = format!("Desktop Action {action_id}");
        let name = entry.locale_string(&group, "Name", locale);
        let exec = entry.string(&group, "Exec").map(|line| Exec::parse(&line));
        match (name, exec) {
            (Some(name), Some(Ok(exec))) => actions.push(MenuAction {
                id: action_id,
                name,
                icon: entry.string(&group, "Icon"),
                exec,
                separator_before: std::mem::take(&mut line_owed),
            }),
            (None, _) => unusable.push(format!("{action_id}: it has no Name")),
            (_, None) => unusable.push(format!("{action_id}: it has no Exec")),
            (_, Some(Err(e))) => {
                unusable.push(format!("{action_id}: its Exec cannot be used: {e}"));
            }
        }
    }
    if actions.is_empty() {
        return Err(if unusable.is_empty() {
            String::from("lists no actions (Actions)")
        } else {
            format!("has no usable action ({})", unusable.join("; "))
        });
    }
    // A bound that is not a number is no bound, as KDE's reader has it (it
    // reads one as nought, which bounds nothing). The value comes trimmed.
    let number = |key: &str| {
        entry
            .string(GROUP, key)
            .and_then(|v| v.parse::<usize>().ok())
    };
    Ok(Some(ServiceMenu {
        id,
        origin,
        path: path.to_path_buf(),
        mime_types,
        submenu: entry
            .locale_string(GROUP, "X-KDE-Submenu", locale)
            .filter(|s| !s.trim().is_empty()),
        top_level: entry
            .string(GROUP, "X-KDE-Priority")
            .is_some_and(|p| p.trim() == "TopLevel"),
        counts: Counts {
            min: number("X-KDE-MinNumberOfUrls"),
            max: number("X-KDE-MaxNumberOfUrls"),
            // A count that is not a number is no count: one bad element
            // of the list does not unmake the others.
            exactly: kde_list(entry, "X-KDE-RequiredNumberOfUrls")
                .iter()
                .filter_map(|v| v.parse().ok())
                .collect(),
        },
        requires_write: kde_list(entry, "X-KDE-Require")
            .iter()
            .any(|r| r == "Write"),
        actions,
        unusable,
    }))
}

/// The key that keeps the menu from this machine's files, if one does.
///
/// `X-KDE-Protocol` names the one kind of place the menu is for, or with a
/// `!` the one it is not for; `X-KDE-Protocols` lists the kinds it is for.
/// The first, when there, is the one KDE reads. Files here are `file`.
fn not_for_files(entry: &DesktopEntry) -> Option<&'static str> {
    if let Some(protocol) = entry
        .string(GROUP, "X-KDE-Protocol")
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
    {
        let for_files = match protocol.strip_prefix('!') {
            Some(excluded) => excluded.trim() != "file",
            None => protocol == "file",
        };
        return (!for_files).then_some("X-KDE-Protocol");
    }
    let protocols = kde_list(entry, "X-KDE-Protocols");
    (!protocols.is_empty() && !protocols.iter().any(|p| p == "file")).then_some("X-KDE-Protocols")
}

/// A list KDE reads its own way -- elements between commas -- read with
/// commas or semicolons, each element trimmed, empty ones dropped. See the
/// module docs.
fn kde_list(entry: &DesktopEntry, key: &str) -> Vec<String> {
    entry
        .string(GROUP, key)
        .map(|value| {
            value
                .split([',', ';'])
                .map(str::trim)
                .filter(|element| !element.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
