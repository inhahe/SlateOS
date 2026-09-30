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
//! found under `kio/servicemenus/` in each XDG data directory, the user's
//! (`~/.local/share`) before the system's -- and, for menus written for
//! older KDE, under `kservices5/ServiceMenus/` and in `kservices5/` itself
//! (there only a file whose `ServiceTypes` names `KonqPopupMenu/Plugin`),
//! after every current one, as KDE reads them. A menu is known by its file
//! name, and the first file of a name shadows the rest: the user's copy the
//! system's, and `Hidden=true` in it removes the system's menu, as a desktop
//! entry's does.
//!
//! What this reads of KDE's keys: `MimeType` -- or, in a menu with none,
//! `ServiceTypes`, where older menus named their kinds -- and
//! `ExcludeServiceTypes`; `Actions` (with `_SEPARATOR_`) and each action's
//! `Name`, `Icon` and `Exec`; the menu's own `Name`, `Icon` and `Path`;
//! `X-KDE-Submenu`, `X-KDE-Priority` (`TopLevel`, `Important`), the three
//! `X-KDE-*NumberOfUrls` keys, `X-KDE-Require=Write`, `X-KDE-Protocol` and
//! `X-KDE-Protocols`. KDE reads the lists among those with commas between
//! the elements, the desktop entry specification's with semicolons; either
//! is read here, since authors write both and each means a list. `Type` is
//! not asked for: a file where menus are is a menu, as KDE has it.
//!
//! A kind is matched as KDE's file manager matches it: exactly, by
//! `type/*`, `all/all` for anything, and `all/allfiles` (or `allfiles`) or
//! `application/octet-stream` for anything but a folder -- and a kind is
//! also each kind it is a sub-class of, as the MIME database says: a menu
//! for `text/plain` is for a shell script. The rule the specification
//! states for every text type, that `text/*` is `text/plain`, is applied
//! here; the rest come from the caller, which knows its files
//! ([`Target::inherits`]).
//!
//! A menu that shows only while a D-Bus service says so --
//! `X-KDE-ShowIfRunning`, `X-KDE-ShowIfDBusCall` -- is not offered at all,
//! and says why ([`Scan::skipped`]): this desktop has no D-Bus, so the
//! condition never holds, and an item shown when its author meant it hidden
//! is worse than one missing. `X-KDE-AuthorizeAction` names permissions a
//! KDE machine grants unless its administrator withdrew them; this desktop
//! withdraws none, so it is read as granted.
//!
//! # Where the items go
//!
//! [`Scan::rows`] lays the offered menus out as KDE's file manager does: the
//! items of `TopLevel` menus in the menu itself, after the others; the rest
//! -- `Important` ones first -- in an "Actions" submenu when they come to
//! more than four rows, else in the menu too; a menu with `X-KDE-Submenu` in
//! a submenu of that name, one for every menu naming it; and between two
//! lines, items in the order of their ids.
//!
//! # The commands
//!
//! An item's `Exec` is read as KDE reads it -- a line of shell, its codes
//! expanded anywhere and quoted for where they stand, `%f` once per file
//! unless the line takes them all, the working directory the file's folder
//! -- not as the desktop entry specification reads a launcher's. The menus
//! are written for KDE's reading. See [`Command`].
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

use desktopentry::{DesktopEntry, Locale};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

mod choices;
mod command;

pub use choices::{CONFIG_NAME, Choices, ChoicesFile};
pub use command::{Command, CommandError, Context, Run, SHELL};

/// The group every service menu's keys are in.
const GROUP: &str = "Desktop Entry";

/// The largest service-menu file read. A real one is a few hundred bytes.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Where under a data directory KDE looks for service menus now.
const CURRENT_PLACE: &str = "kio/servicemenus";

/// Where older KDE looked -- every file there a menu.
const OLDER_PLACE: &str = "kservices5/ServiceMenus";

/// Where KDE still looks for menus written for its older versions: a file
/// there is one only if its `ServiceTypes` names [`POPUP_TYPE`].
const OLDEST_PLACE: &str = "kservices5";

/// The service type an older menu is marked with.
const POPUP_TYPE: &str = "KonqPopupMenu/Plugin";

/// The `Actions=` element that is a line between items rather than an item.
const SEPARATOR: &str = "_SEPARATOR_";

/// The keys naming a condition this desktop cannot meet: a D-Bus service's.
const CONDITIONS: [&str; 2] = ["X-KDE-ShowIfRunning", "X-KDE-ShowIfDBusCall"];

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

    /// Every directory to look in, with whose it is and whether only a
    /// marked menu counts there, in precedence order: every data
    /// directory's current place, then every one's older places -- the order
    /// KDE reads them in.
    fn places(&self) -> Vec<Place> {
        let bases: Vec<(&PathBuf, Origin)> = self
            .user
            .iter()
            .map(|dir| (dir, Origin::User))
            .chain(self.system.iter().map(|dir| (dir, Origin::System)))
            .collect();
        let mut out = Vec::new();
        for (sub, marked_only) in [
            (CURRENT_PLACE, false),
            (OLDER_PLACE, false),
            (OLDEST_PLACE, true),
        ] {
            for (base, origin) in &bases {
                out.push(Place {
                    dir: base.join(sub),
                    origin: *origin,
                    marked_only,
                });
            }
        }
        out
    }

    /// Every directory service menus are read from, in the order they are:
    /// what to watch for a menu installed, removed or changed.
    #[must_use]
    pub fn searched(&self) -> Vec<PathBuf> {
        self.places().into_iter().map(|place| place.dir).collect()
    }
}

/// One directory service menus are read from.
struct Place {
    dir: PathBuf,
    origin: Origin,
    /// Whether a file here is a menu only if marked with [`POPUP_TYPE`].
    marked_only: bool,
}

// ============================================================================
// A menu
// ============================================================================

/// Where a menu's items go (`X-KDE-Priority`): see [`Scan::rows`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Priority {
    /// With the others: in the "Actions" submenu when they come to more
    /// than four rows, else in the menu itself.
    #[default]
    Ordinary,
    /// `Important`: first among the others.
    Important,
    /// `TopLevel`: in the menu itself, after the others, however many.
    TopLevel,
}

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
    /// What choosing it runs: its `Exec`, read as KDE reads one.
    pub command: Command,
    /// Whether a line goes above it: an `_SEPARATOR_` came right before it
    /// in `Actions=` -- the first item's included, which draws a line only
    /// when something is above it in the menu.
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
    /// Its own `Name`, in the reader's language -- what `%c` stands for.
    pub name: Option<String>,
    /// Its own `Icon` -- what `%i` stands for.
    pub icon: Option<String>,
    /// Its `Path=`: where every item's program starts, when it gives one.
    pub working_dir: Option<PathBuf>,
    /// The kinds of thing it is for: MIME types, `type/*`, `all/all`,
    /// `all/allfiles`, `inode/directory`.
    pub mime_types: Vec<String>,
    /// The kinds it is not for, though its `mime_types` take them in
    /// (`ExcludeServiceTypes`).
    pub excluded: Vec<String>,
    /// The submenu its items go in (`X-KDE-Submenu`), in the reader's
    /// language.
    pub submenu: Option<String>,
    /// Where its items go (`X-KDE-Priority`).
    pub priority: Priority,
    /// How many things it may be offered for at once.
    pub counts: Counts,
    /// Whether every thing must be writable (`X-KDE-Require=Write`).
    pub requires_write: bool,
    /// Its items, in the order `Actions=` lists them.
    pub actions: Vec<MenuAction>,
    /// Whether a line comes after its last item: `Actions=` ends with an
    /// `_SEPARATOR_`.
    pub separator_after: bool,
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
    /// Whether this menu is for `targets`: each is a kind it names and none
    /// it excludes, there are as many as it allows, and each is writable
    /// where it requires that. Nothing is for no targets.
    #[must_use]
    pub fn applies_to(&self, targets: &[Target]) -> bool {
        !targets.is_empty()
            && self.counts.allows(targets.len())
            && targets.iter().all(|t| {
                (!self.requires_write || t.writable)
                    && self.mime_types.iter().any(|m| mime_matches(m, t))
                    && !self.excluded.iter().any(|m| mime_matches(m, t))
            })
    }
}

impl MenuAction {
    /// The programs choosing this item on `targets` starts -- one for all of
    /// them, or one each (see [`Command`]) -- `home` being what a `~` in the
    /// command means. `menu` is the menu the item is in: its name, icon, file
    /// and working directory are the command's too.
    ///
    /// # Errors
    ///
    /// As [`Command::runs`].
    pub fn runs(
        &self,
        targets: &[Target],
        menu: &ServiceMenu,
        home: Option<&Path>,
    ) -> Result<Vec<Run>, CommandError> {
        let files: Vec<&Path> = targets.iter().map(|t| t.path.as_path()).collect();
        let context = Context {
            name: menu.name.as_deref().unwrap_or_default(),
            icon: menu.icon.as_deref(),
            location: Some(&menu.path),
            working_dir: menu.working_dir.as_deref(),
            home,
        };
        self.command.runs(&files, &context)
    }
}

/// Whether the pattern `pattern` names `target`'s kind, as KDE's file
/// manager reads a pattern: its own type, a type it inherits, or a `type/*`
/// any of those is in; `all/all` anything; `all/allfiles`, `allfiles` and
/// `application/octet-stream` anything but a folder.
fn mime_matches(pattern: &str, target: &Target) -> bool {
    if pattern == "all/all" {
        return true;
    }
    if pattern == "all/allfiles"
        || pattern == "allfiles"
        || pattern.eq_ignore_ascii_case("application/octet-stream")
    {
        return !target.is_dir;
    }
    let kinds =
        || core::iter::once(target.mime.as_str()).chain(target.inherits.iter().map(String::as_str));
    match pattern.strip_suffix("/*") {
        Some(major) => kinds().any(|kind| {
            kind.split_once('/')
                .is_some_and(|(m, _)| m.eq_ignore_ascii_case(major))
        }),
        None => {
            kinds().any(|kind| kind.eq_ignore_ascii_case(pattern)) || text_is_plain(pattern, target)
        }
    }
}

/// The sub-class the shared MIME-info specification gives every text type:
/// each `text/*` is `text/plain`.
fn text_is_plain(pattern: &str, target: &Target) -> bool {
    pattern.eq_ignore_ascii_case("text/plain")
        && target
            .mime
            .split_once('/')
            .is_some_and(|(major, _)| major.eq_ignore_ascii_case("text"))
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

    /// The menu of file name `menu` and its item `action`, if both are
    /// still here -- what a file menu looks up when an item is chosen, by
    /// the names it kept rather than a reference into a scan that may have
    /// been read again since.
    #[must_use]
    pub fn find(&self, menu: &OsStr, action: &str) -> Option<(&ServiceMenu, &MenuAction)> {
        let menu = self.menus.iter().find(|m| m.id == menu)?;
        let action = menu.actions.iter().find(|a| a.id == action)?;
        Some((menu, action))
    }

    /// What the menus offered for `targets` add to a file menu, laid out as
    /// KDE's file manager lays them out (see the module docs): the rows,
    /// top to bottom.
    #[must_use]
    pub fn rows<'a>(&'a self, choices: &Choices, targets: &[Target]) -> Vec<Row<'a>> {
        let mut lists = Lists::default();
        for menu in self.offered(choices, targets) {
            let list = lists.select(menu.priority, menu.submenu.as_deref());
            for action in &menu.actions {
                if action.separator_before {
                    list.push(Entry::Line);
                }
                list.push(Entry::Item(menu, action));
            }
            if menu.separator_after {
                list.push(Entry::Line);
            }
        }
        let mut others = Vec::new();
        insert_submenus(lists.important_submenus, &mut others);
        insert_entries(lists.important, &mut others);
        insert_submenus(lists.ordinary_submenus, &mut others);
        insert_entries(lists.ordinary, &mut others);
        let mut top = Vec::new();
        insert_submenus(lists.top_submenus, &mut top);
        insert_entries(lists.top, &mut top);
        let mut rows = if others.len() > MOST_IN_MENU {
            vec![Row::Submenu {
                label: ACTIONS_LABEL.to_owned(),
                icon: Some(ACTIONS_ICON.to_owned()),
                rows: others,
            }]
        } else {
            others
        };
        rows.extend(top);
        rows
    }
}

// ============================================================================
// Where the items go
// ============================================================================

/// The label of the submenu the other items go in when they are many, as
/// KDE's is.
pub const ACTIONS_LABEL: &str = "Actions";

/// That submenu's icon, as KDE's.
pub const ACTIONS_ICON: &str = "view-more-symbolic";

/// The most rows of the other items a file menu holds itself; more go into
/// the "Actions" submenu, as KDE's do.
const MOST_IN_MENU: usize = 4;

/// One row of what service menus add to a file menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row<'a> {
    /// An item: choosing it runs `action` of `menu`.
    Item {
        /// The menu it is in.
        menu: &'a ServiceMenu,
        /// The item.
        action: &'a MenuAction,
    },
    /// A line between items.
    Separator,
    /// A submenu of more rows.
    Submenu {
        /// What it says.
        label: String,
        /// Its icon: the first of its items', as KDE takes it.
        icon: Option<String>,
        /// Its rows.
        rows: Vec<Row<'a>>,
    },
}

/// One thing a menu puts in a list: an item, or a line.
enum Entry<'a> {
    Item(&'a ServiceMenu, &'a MenuAction),
    Line,
}

/// The lists KDE sorts menus' items into, by priority and submenu.
#[derive(Default)]
struct Lists<'a> {
    top: Vec<Entry<'a>>,
    important: Vec<Entry<'a>>,
    ordinary: Vec<Entry<'a>>,
    top_submenus: BTreeMap<String, Vec<Entry<'a>>>,
    important_submenus: BTreeMap<String, Vec<Entry<'a>>>,
    ordinary_submenus: BTreeMap<String, Vec<Entry<'a>>>,
}

impl<'a> Lists<'a> {
    /// The list a menu of this priority and submenu adds to.
    fn select(&mut self, priority: Priority, submenu: Option<&str>) -> &mut Vec<Entry<'a>> {
        match (submenu, priority) {
            (None, Priority::TopLevel) => &mut self.top,
            (None, Priority::Important) => &mut self.important,
            (None, Priority::Ordinary) => &mut self.ordinary,
            (Some(name), Priority::TopLevel) => {
                self.top_submenus.entry(name.to_owned()).or_default()
            }
            (Some(name), Priority::Important) => {
                self.important_submenus.entry(name.to_owned()).or_default()
            }
            (Some(name), Priority::Ordinary) => {
                self.ordinary_submenus.entry(name.to_owned()).or_default()
            }
        }
    }
}

/// Add `entries` to `rows` as KDE adds a list to a menu: the items between
/// two lines in the order of their ids, and a line wherever one was asked
/// for with something above it that is not already a line.
fn insert_entries<'a>(entries: Vec<Entry<'a>>, rows: &mut Vec<Row<'a>>) {
    fn flush<'a>(group: &mut Vec<(&'a ServiceMenu, &'a MenuAction)>, rows: &mut Vec<Row<'a>>) {
        group.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        rows.extend(
            group
                .drain(..)
                .map(|(menu, action)| Row::Item { menu, action }),
        );
    }
    let mut group = Vec::new();
    for entry in entries {
        match entry {
            Entry::Item(menu, action) => group.push((menu, action)),
            Entry::Line => {
                flush(&mut group, rows);
                if rows.last().is_some_and(|row| *row != Row::Separator) {
                    rows.push(Row::Separator);
                }
            }
        }
    }
    flush(&mut group, rows);
}

/// Add a submenu to `rows` for each named list, in the order of the names,
/// each with the rows [`insert_entries`] makes of it. (KDE also leaves out
/// a list with no items; every menu offered has one, so none is empty.)
fn insert_submenus<'a>(submenus: BTreeMap<String, Vec<Entry<'a>>>, rows: &mut Vec<Row<'a>>) {
    for (label, entries) in submenus {
        let icon = match entries.first() {
            Some(Entry::Item(_, action)) => action.icon.clone(),
            _ => None,
        };
        let mut inner = Vec::new();
        insert_entries(entries, &mut inner);
        rows.push(Row::Submenu {
            label,
            icon,
            rows: inner,
        });
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
    for place in dirs.places() {
        // A directory that is not there has no menus in it -- the ordinary
        // state of most of them.
        let Ok(entries) = fs::read_dir(&place.dir) else {
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
            let entry = read_entry(&path);
            // Where only a marked file is a menu, anything else is some other
            // service's file and none of this desktop's business: not
            // reported, and not shadowing the name.
            if place.marked_only && !entry.as_ref().is_ok_and(is_marked) {
                continue;
            }
            seen.push(id.clone());
            match entry.and_then(|entry| parse_menu(&entry, id, place.origin, &path, locale)) {
                Ok(Some(menu)) => out.menus.push(menu),
                // Hidden: the name is removed, and that is all.
                Ok(None) => {}
                Err(why) => out.skipped.push(Skipped { path, why }),
            }
        }
    }
    out
}

/// The desktop entry in the file at `path`.
fn read_entry(path: &Path) -> Result<DesktopEntry, String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("could not be read ({e})"))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!(
            "is {size} bytes, over the {MAX_FILE_BYTES}-byte limit for a service menu"
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("could not be read ({e})"))?;
    DesktopEntry::parse(&bytes).map_err(|e| format!("is not a desktop entry ({e})"))
}

/// Whether `entry` is marked as an older KDE's service menu.
fn is_marked(entry: &DesktopEntry) -> bool {
    kde_list(entry, "ServiceTypes")
        .iter()
        .any(|kind| kind == POPUP_TYPE)
}

/// The menu `entry` describes; `None` when it says `Hidden=true`.
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
    if let Some(condition) = CONDITIONS
        .iter()
        .find(|key| entry.raw(GROUP, key).is_some())
    {
        return Err(format!(
            "shows only while a D-Bus service says so ({condition}), and this desktop has no D-Bus"
        ));
    }
    if let Some(key) = not_for_files(entry) {
        return Err(format!(
            "is only for places other than this machine's files ({key})"
        ));
    }
    // An older menu named its kinds among its service types.
    let mut mime_types = entry.list(GROUP, "MimeType");
    if mime_types.is_empty() {
        mime_types = kde_list(entry, "ServiceTypes");
        mime_types.retain(|kind| kind != POPUP_TYPE);
    }
    if mime_types.is_empty() {
        return Err(String::from("names no kind of file it is for (MimeType)"));
    }
    let mut actions: Vec<MenuAction> = Vec::new();
    let mut unusable = Vec::new();
    // A line is owed before the next item -- or after the last, if none
    // comes.
    let mut line_owed = false;
    for action_id in entry.list(GROUP, "Actions") {
        if action_id == SEPARATOR {
            line_owed = true;
            continue;
        }
        let group = format!("Desktop Action {action_id}");
        let name = entry.locale_string(&group, "Name", locale);
        let command = entry
            .string(&group, "Exec")
            .map(|line| Command::parse(&line));
        match (name, command) {
            (Some(name), Some(Ok(command))) => actions.push(MenuAction {
                id: action_id,
                name,
                icon: entry.string(&group, "Icon"),
                command,
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
        name: entry
            .locale_string(GROUP, "Name", locale)
            .filter(|s| !s.trim().is_empty()),
        icon: entry.string(GROUP, "Icon").filter(|s| !s.trim().is_empty()),
        working_dir: entry
            .string(GROUP, "Path")
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from),
        mime_types,
        submenu: entry
            .locale_string(GROUP, "X-KDE-Submenu", locale)
            .filter(|s| !s.trim().is_empty()),
        priority: match entry.string(GROUP, "X-KDE-Priority").as_deref() {
            Some("TopLevel") => Priority::TopLevel,
            Some("Important") => Priority::Important,
            _ => Priority::Ordinary,
        },
        excluded: kde_list(entry, "ExcludeServiceTypes"),
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
        separator_after: line_owed,
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
