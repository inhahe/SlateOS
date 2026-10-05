//! Sound themes: the sound played for an event -- a message arriving, an
//! error, the recycle bin emptied -- found by name in the theme the user
//! chose.
//!
//! # Where a sound comes from
//!
//! A sound theme is a folder of sound files named for the events of the
//! freedesktop sound naming specification (`message-new-instant.oga`,
//! `dialog-error.oga`, `trash-empty.oga`, ...) in a `stereo` directory: the
//! freedesktop sound theme specification's layout, so a theme made for
//! another desktop -- freedesktop's own, Ubuntu's Yaru -- installs here as it
//! is. Its `index.theme` may name other directories for stereo output and
//! the themes it inherits from. A theme is looked for under the SlateOS
//! theme roots first, beside a `theme.yaml` as a theme's icons and cursors
//! are ([`ThemeDirs`]), then where other desktops install sound themes:
//! `$XDG_DATA_HOME/sounds` and each of `$XDG_DATA_DIRS`' `sounds`. The first
//! root that has the file wins, so a user's copy of a theme is the one
//! heard.
//!
//! # Which file
//!
//! An event's sound is looked for in the theme, then each theme its
//! `index.theme` inherits from, then the `freedesktop` theme -- the
//! specification's order -- and failing all of them under its name cut at
//! the last hyphen, and so on: `dialog-error-serious` sounds as
//! `dialog-error`. The extensions are tried as the specification orders
//! them: `.disabled` first, which silences the event in that theme, then
//! `.oga`, `.ogg` and `.wav`. When nothing is found the event has the
//! built-in sound of its name, if there is one ([`SoundChoice::BuiltIn`]),
//! which `gui/sound` synthesizes -- so a system with no sound theme
//! installed still sounds.
//!
//! The built-in theme is the built-in sounds alone -- unless its folder has
//! been given a `stereo` directory, whose files are heard first, as an
//! edited copy of its icons is drawn. It does not fall back to the
//! `freedesktop` theme: chosen, SlateOS's sounds are what is heard, and a
//! `freedesktop` theme installed beside it would otherwise replace nearly
//! every one of them. Whoever wants freedesktop's chooses it by name.
//!
//! # Trust
//!
//! The files are the user's, or whoever installed the theme: an event's
//! name is checked before it becomes part of a path ([`is_valid_name`]), an
//! `index.theme` is read only up to a size, and inheritance stops after
//! [`MAX_THEMES_VISITED`] themes, however they inherit. What is found is
//! handed to whoever plays it, which decodes it and refuses what it cannot.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use desktopentry::DesktopEntry;
use desktopentry::scan::DataDirs;

use crate::themes::{self, ThemeDirs};

/// The axis's name, as a theme's `meta.supports` lists it and
/// `theme.sounds` chooses it.
pub const AXIS: &str = "sounds";

/// The directory a theme's stereo sounds are in, when its `index.theme`
/// names none.
pub const STEREO_DIR: &str = "stereo";

/// The largest sound file a theme should ship, in bytes: what `gui/sound`
/// reads (`sound::MAX_FILE_BYTES`) -- a larger one is refused when it is
/// played, so the theme checker says so first. The same number, written
/// twice because this crate does not depend on the player.
pub const MAX_SOUND_BYTES: u64 = 16 * 1024 * 1024;

/// The file that names a sound theme, its directories and the themes it
/// inherits from.
pub const INDEX_FILE: &str = "index.theme";

/// The theme every sound theme falls back to, as the specification says.
pub const FALLBACK_THEME: &str = "freedesktop";

/// The extensions a sound file is looked for with, in the specification's
/// order. `disabled` is a file whose presence silences the event.
pub const EXTENSIONS: [&str; 4] = ["disabled", "oga", "ogg", "wav"];

/// The largest `index.theme` read: a few lines in practice.
const MAX_INDEX_BYTES: u64 = 64 * 1024;

/// The most themes one lookup visits, the chosen one and every theme it
/// inherits from. A real chain is two or three long; this bounds a theme
/// that inherits from itself by another name.
pub const MAX_THEMES_VISITED: usize = 16;

/// What to play for an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundChoice {
    /// This sound file.
    File(PathBuf),
    /// Nothing: the theme silences the event (`.disabled`), sounds are off,
    /// or the name is not an event's.
    Silent,
    /// No theme has a sound for it: the built-in sound of this name, if
    /// `gui/sound` has one (`BuiltIn::for_event`).
    BuiltIn(String),
}

/// Whether `name` can name an event's sound: lower-case ASCII letters,
/// digits, `-` and `_`, up to 128 of them -- every name the sound naming
/// specification has, and never a path.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// `name`, then `name` cut at each hyphen from the last: the
/// specification's fallback. Shared with the user's own sounds
/// (`AppearanceSettings::sound_for`), which fall back the same way.
pub(crate) fn cuts(name: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(name), |n| n.rfind('-').and_then(|at| n.get(..at)))
        .filter(|n| !n.is_empty())
}

/// An event the desktop makes a sound for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellEvent {
    /// Its sound naming specification name: its key in `sounds.events`, and
    /// what [`crate::AppearanceSettings::sound_for`] is asked.
    pub name: &'static str,
    /// What a settings page calls it.
    pub label: &'static str,
}

/// Every event the desktop makes a sound for, in the order a settings page
/// lists them for the user to choose each one's sound. Here, not in the
/// desktop, because the page is another program's and both read this crate;
/// the desktop's tests hold its events to this list (`desktop::event_sounds`).
pub const SHELL_EVENTS: [ShellEvent; 11] = [
    ShellEvent {
        name: "message-new-instant",
        label: "Notification",
    },
    ShellEvent {
        name: "dialog-warning",
        label: "Urgent notification",
    },
    ShellEvent {
        name: "audio-volume-change",
        label: "Volume changed",
    },
    ShellEvent {
        name: "device-added",
        label: "Device connected",
    },
    ShellEvent {
        name: "device-removed",
        label: "Device removed",
    },
    ShellEvent {
        name: "screen-capture",
        label: "Screenshot taken",
    },
    ShellEvent {
        name: "battery-low",
        label: "Battery low",
    },
    ShellEvent {
        name: "network-connectivity-established",
        label: "Network connected",
    },
    ShellEvent {
        name: "network-connectivity-lost",
        label: "Network lost",
    },
    ShellEvent {
        name: "desktop-login",
        label: "Sign in",
    },
    ShellEvent {
        name: "desktop-logout",
        label: "Sign out",
    },
];

/// A sound theme, by the name of its folder.
///
/// Named as a colour theme or a cursor theme is, by a folder name that need
/// not be text (`design-decisions.md` §426). Nothing is read until a sound
/// is asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundTheme {
    id: OsString,
    /// The SlateOS theme roots, or `None` for the standard ones.
    dirs: Option<ThemeDirs>,
    /// The roots other desktops' sound themes are under, or `None` for the
    /// environment's ([`sound_dirs`]).
    sound_dirs: Option<Vec<PathBuf>>,
}

impl Default for SoundTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl SoundTheme {
    /// The built-in theme: the built-in sounds, unless the built-in theme's
    /// folder has been given sounds.
    #[must_use]
    pub fn built_in() -> Self {
        Self::load(OsStr::new(themes::BUILT_IN))
    }

    /// The theme `id`, looked for in the standard places.
    #[must_use]
    pub fn load(id: &OsStr) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: None,
            sound_dirs: None,
        }
    }

    /// The theme `id`, looked for under the SlateOS theme roots `dirs` and
    /// then the sound-theme roots `sound_dirs`, in that order.
    #[must_use]
    pub fn named(id: &OsStr, dirs: ThemeDirs, sound_dirs: Vec<PathBuf>) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: Some(dirs),
            sound_dirs: Some(sound_dirs),
        }
    }

    /// The theme's name -- its folder's.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == themes::BUILT_IN
    }

    /// What to play for the event `name` -- a sound naming specification
    /// name. See the module's "Which file".
    #[must_use]
    pub fn sound(&self, name: &str) -> SoundChoice {
        let name = name.trim();
        if !is_valid_name(name) {
            return SoundChoice::Silent;
        }
        let roots = self.roots();
        for candidate in cuts(name) {
            let mut visited = Vec::new();
            let mut found = find(&roots, &self.id, candidate, &mut visited);
            // Not for the built-in theme, which is a theme of its own rather
            // than one with gaps for freedesktop's to fill -- see the
            // module's "Which file".
            if found.is_none() && !self.is_built_in() {
                found = find(&roots, OsStr::new(FALLBACK_THEME), candidate, &mut visited);
            }
            if let Some(found) = found {
                return found;
            }
        }
        SoundChoice::BuiltIn(name.to_owned())
    }

    /// Where this theme is looked for: the SlateOS roots, then the others.
    fn roots(&self) -> Vec<PathBuf> {
        let dirs = self.dirs.clone().unwrap_or_else(ThemeDirs::standard);
        let mut roots: Vec<PathBuf> = dirs
            .roots()
            .into_iter()
            .map(|(root, _)| root.to_path_buf())
            .collect();
        match &self.sound_dirs {
            Some(sound_dirs) => roots.extend(sound_dirs.iter().cloned()),
            None => roots.extend(sound_dirs()),
        }
        roots
    }
}

/// The sound for `name` in the theme `id` or a theme it inherits from,
/// every theme visited recorded in `visited`.
fn find(
    roots: &[PathBuf],
    id: &OsStr,
    name: &str,
    visited: &mut Vec<OsString>,
) -> Option<SoundChoice> {
    if visited.len() >= MAX_THEMES_VISITED
        || visited.iter().any(|seen| seen == id)
        || !themes::is_valid_id(id)
    {
        return None;
    }
    visited.push(id.to_os_string());
    // Its first `index.theme` says where its sounds are and what it
    // inherits, as a theme's first copy is the one read.
    let index = roots.iter().find_map(|root| read_index(&root.join(id)));
    let directories = index
        .as_ref()
        .map(|index| index.directories.clone())
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| vec![STEREO_DIR.to_owned()]);
    for root in roots {
        for directory in &directories {
            for extension in EXTENSIONS {
                let file = root
                    .join(id)
                    .join(directory)
                    .join(format!("{name}.{extension}"));
                if file.is_file() {
                    return Some(if extension == "disabled" {
                        SoundChoice::Silent
                    } else {
                        SoundChoice::File(file)
                    });
                }
            }
        }
    }
    index?
        .inherits
        .iter()
        .find_map(|parent| find(roots, parent, name, visited))
}

/// The bytes of the file at `path`, if it is no larger than `cap`.
fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    // `ok()`: every failure means the same here -- this theme has no
    // readable index, and is looked in with the default directory.
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= cap).then_some(bytes)
}

/// What a theme's `index.theme` says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Index {
    /// The name it gives itself, if any.
    name: Option<String>,
    /// The directories of its stereo sounds, in order.
    directories: Vec<String>,
    /// The themes it inherits from, in order.
    inherits: Vec<OsString>,
}

/// A list in an `index.theme`: commas, as the specification separates it,
/// or semicolons, as a desktop entry's lists are.
fn list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

/// The `index.theme` in the theme folder `dir`, or `None` for one that is
/// missing, too large or not a key file.
fn read_index(dir: &Path) -> Option<Index> {
    let bytes = read_capped(&dir.join(INDEX_FILE), MAX_INDEX_BYTES)?;
    let entry = DesktopEntry::parse(&bytes).ok()?;
    const GROUP: &str = "Sound Theme";
    let inherits = entry
        .raw(GROUP, "Inherits")
        .map(|value| {
            list(value)
                .map(OsString::from)
                .filter(|parent| themes::is_valid_id(parent))
                .collect()
        })
        .unwrap_or_default();
    // A directory is stereo's if its group says so, or says nothing: a
    // theme of one directory often leaves `OutputProfile` out.
    let directories = entry
        .raw(GROUP, "Directories")
        .map(|value| {
            list(value)
                .filter(|dir| is_plain_directory(dir))
                .filter(|dir| {
                    entry
                        .raw(dir, "OutputProfile")
                        .is_none_or(|profile| profile.trim() == "stereo")
                })
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let name = entry
        .string(GROUP, "Name")
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    Some(Index {
        name,
        directories,
        inherits,
    })
}

/// Whether `dir` names a directory inside a theme and nothing more: no
/// separator, not `.` or `..` -- an `index.theme` cannot point a lookup
/// out of its theme.
fn is_plain_directory(dir: &str) -> bool {
    !dir.is_empty() && dir != "." && dir != ".." && !dir.contains(['/', '\\', '\0'])
}

/// Where other desktops' sound themes are installed, as the environment
/// says: `$XDG_DATA_HOME/sounds` (or `~/.local/share/sounds`), then each
/// `$XDG_DATA_DIRS/sounds` -- the user's before the system's.
#[must_use]
pub fn sound_dirs() -> Vec<PathBuf> {
    sound_dirs_from(|name| std::env::var_os(name))
}

/// [`sound_dirs`], with the environment as `get`.
fn sound_dirs_from(get: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    DataDirs::from_env(&get)
        .dirs()
        .iter()
        .map(|dir| dir.join("sounds"))
        .collect()
}

/// A sound theme that is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundThemeInfo {
    /// Its folder's name: what [`SoundTheme::load`] takes.
    pub id: OsString,
    /// The name it gives itself in its `index.theme`, else its folder's.
    pub name: String,
    /// Whether this is the built-in theme -- the built-in sounds.
    pub built_in: bool,
}

/// Every sound theme installed, for a picker: the built-in one first, then
/// every folder with an `index.theme` of a sound theme or a `stereo`
/// directory under any root, by name. A theme installed in two roots is
/// listed once, as the first root's copy -- the one [`SoundTheme::sound`]
/// would read.
#[must_use]
pub fn available() -> Vec<SoundThemeInfo> {
    available_in(&SoundTheme::built_in().roots())
}

/// [`available`], under the roots `roots`, in order.
#[must_use]
pub fn available_in(roots: &[PathBuf]) -> Vec<SoundThemeInfo> {
    let mut found: BTreeMap<OsString, SoundThemeInfo> = BTreeMap::new();
    for root in roots {
        // A root that does not exist has no themes in it: the ordinary state
        // of a user's data directory, not an error.
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        // An entry the listing could not describe could not be opened as a
        // theme either; skipping it loses a row nobody could have chosen.
        for entry in entries.flatten() {
            let id = entry.file_name();
            if found.contains_key(&id) || !themes::is_valid_id(&id) || id == themes::BUILT_IN {
                continue;
            }
            let index = read_index(&entry.path());
            let has_sounds = entry.path().join(STEREO_DIR).is_dir()
                || index.as_ref().is_some_and(|i| !i.directories.is_empty());
            if !has_sounds {
                continue;
            }
            let name = index
                .and_then(|index| index.name)
                .unwrap_or_else(|| pathcodec::display_os(&id));
            found.insert(
                id.clone(),
                SoundThemeInfo {
                    id,
                    name,
                    built_in: false,
                },
            );
        }
    }
    let mut list: Vec<SoundThemeInfo> = found.into_values().collect();
    list.sort_by_key(|info| info.name.to_lowercase());
    list.insert(
        0,
        SoundThemeInfo {
            id: OsString::from(themes::BUILT_IN),
            name: themes::BUILT_IN_NAME.to_owned(),
            built_in: true,
        },
    );
    list
}

#[cfg(test)]
#[path = "sounds_tests.rs"]
mod tests;
