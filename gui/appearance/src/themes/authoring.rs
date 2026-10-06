//! Making themes: deriving one from another, changing its colours and what
//! describes it, installing one from a folder or a file, copying one out to
//! share, and removing one -- the model under a theme editor
//! (`roadmap-detailed.md` §4.6, *Theme Editor in Settings App*: "Derive from
//! existing", "Import/Export") and under the `theme` program.
//!
//! # Only the user's own themes change
//!
//! A theme is changed where the user's themes are,
//! `~/.local/share/slateos/themes`. One installed with the system is changed
//! by deriving a copy ([`derive()`]): under its own name the copy stands in for
//! it -- the user's copy wins, as [`crate::themes`] has it -- and under a new
//! name it is a theme of its own. The built-in theme is derived from its
//! template and its icons, both compiled in with it, so it can be derived
//! from on a machine with no theme installed at all.
//!
//! # An edit keeps the author's text
//!
//! A [`ThemeDraft`] holds a theme's file as text and edits it through
//! `yamldoc`: setting a colour rewrites the one line that holds it, and every
//! comment, blank line and spelling elsewhere comes out as it went in. What a
//! draft *means* is always the desktop's own reading of that text
//! ([`ThemeDraft::file`]), so a preview drawn from it is what the desktop
//! draws once it is saved -- there is no second reader to disagree with the
//! first.
//!
//! # Whole or not at all
//!
//! A theme's file is replaced through a temporary renamed over it. A theme's
//! folder -- derived, installed or exported -- is built inside a hidden
//! folder beside where it goes and renamed into place when it is complete,
//! and a theme being removed is renamed into one before it is deleted. A
//! crash leaves the old theme or the new one, and at worst a hidden folder
//! that no list shows: never half a theme in the list.
//!
//! # What a copy takes
//!
//! Everything a theme is, as the checker ([`crate::themecheck`]) reads a
//! folder: its files and folders, and its links that lead to a file inside it
//! -- a cursor theme is made of them -- each of which becomes a second name
//! for the copied file. What is not part of a theme is left out, and said: a
//! hidden file or folder, a link leading out of the folder, to a folder or to
//! nothing, a device or a pipe, a file larger than anything a theme holds.
//! A copy reads nothing outside the folder it copies, so a theme cannot carry
//! one of the user's own files out with it when it is shared. Nor does it
//! carry a mark that a file may be run: what is copied is bytes.
//!
//! # After a save
//!
//! Nothing more is needed for the desktop to show a theme saved while it is
//! in use: the desktop's settings watcher follows the chosen themes' folders
//! ([`crate::dependency_paths`]) and announces the change as one to the
//! appearance settings, and every window's watcher compares the theme's file
//! (design-decisions §1483).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use guitk::color::Color;
use guitk::palette::{TERMINAL_ROLES, THEME_ROLES, ThemeColors, syntax_roles};
use yamldoc::Document;

use super::{
    BUILT_IN, BUILT_IN_NAME, DARK_SECTION, FILE_NAME, LIGHT_SECTION, MAX_FILE_BYTES, META_SECTION,
    Origin, SYNTAX_DARK_SECTION, SYNTAX_LIGHT_SECTION, TERMINAL_DARK_SECTION,
    TERMINAL_LIGHT_SECTION, ThemeDirs, ThemeError, ThemeFile, confined, is_valid_id, parse, quoted,
    read_theme_bytes,
};
use crate::cursors::CURSORS_DIR;
use crate::icons::{self, ICONS_DIR};
use crate::sounds::STEREO_DIR;
use crate::themecheck::{
    self, Finding, MAX_DEPTH, MAX_ENTRIES, MAX_WALLPAPER_BYTES, Report, Severity,
};

/// The built-in theme's file as it is shipped: where a theme made from the
/// built-in one starts.
const BUILT_IN_TEMPLATE: &str = include_str!("../../themes/aero/theme.yaml");

/// The largest file a copy takes: the largest the checker lets a theme hold,
/// a wallpaper. Anything larger is no part of a theme, and copying it whole
/// first to find that out would cost the disk it fills.
const MAX_COPIED_BYTES: u64 = MAX_WALLPAPER_BYTES;

/// The longest name [`id_for`] makes, in bytes: room left under the 255 a
/// file name may be for the `-2` that keeps it apart from another.
const MAX_ID_BYTES: usize = 200;

/// The highest number [`id_for`] adds to a name to make it free. Bounded, as
/// a name can be refused for a reason no number cures -- there being no
/// user's directory at all -- and a search that cannot end must not be one
/// that never does.
const MAX_ID_SUFFIX: u32 = 10_000;

/// How many names [`Holder::new`] tries before giving up -- each taken only
/// by another holder made at the same moment, so a handful is plenty.
const HOLDER_TRIES: u32 = 64;

// ============================================================================
// Sections and keys
// ============================================================================

/// One of a theme file's colour sections: the desktop's colours, a
/// terminal's, or code's, each for dark or for light mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColorSection {
    /// `colors`: the desktop's colours in dark mode.
    Dark,
    /// `colors-light`: the desktop's colours in light mode.
    Light,
    /// `terminal`: a terminal's colours in dark mode.
    TerminalDark,
    /// `terminal-light`: a terminal's colours in light mode.
    TerminalLight,
    /// `syntax`: the colours of code in dark mode.
    SyntaxDark,
    /// `syntax-light`: the colours of code in light mode.
    SyntaxLight,
}

impl ColorSection {
    /// Every section, in the order the built-in theme's file has them.
    pub const ALL: [Self; 6] = [
        Self::Dark,
        Self::Light,
        Self::TerminalDark,
        Self::TerminalLight,
        Self::SyntaxDark,
        Self::SyntaxLight,
    ];

    /// The section's key in a theme's file.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Dark => DARK_SECTION,
            Self::Light => LIGHT_SECTION,
            Self::TerminalDark => TERMINAL_DARK_SECTION,
            Self::TerminalLight => TERMINAL_LIGHT_SECTION,
            Self::SyntaxDark => SYNTAX_DARK_SECTION,
            Self::SyntaxLight => SYNTAX_LIGHT_SECTION,
        }
    }

    /// The section whose key is `key`, if one is.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.key() == key)
    }

    /// The colours the section can set, by the names the desktop reads them
    /// by, in the palette's order: the palette's roles but the accent (the
    /// user's own), a terminal's, or a highlighter's kinds of code.
    #[must_use]
    pub fn roles(self) -> Vec<&'static str> {
        match self {
            Self::Dark | Self::Light => THEME_ROLES.to_vec(),
            Self::TerminalDark | Self::TerminalLight => TERMINAL_ROLES.to_vec(),
            Self::SyntaxDark | Self::SyntaxLight => syntax_roles().to_vec(),
        }
    }

    /// The colours this section of `colors` sets, by role -- what a theme's
    /// file gave it ([`ThemeFile::colors`]).
    #[must_use]
    pub fn of(self, colors: &ThemeColors) -> &BTreeMap<String, Color> {
        match self {
            Self::Dark => &colors.dark,
            Self::Light => &colors.light,
            Self::TerminalDark => &colors.terminal_dark,
            Self::TerminalLight => &colors.terminal_light,
            Self::SyntaxDark => &colors.syntax_dark,
            Self::SyntaxLight => &colors.syntax_light,
        }
    }
}

/// What describes a theme and is a line of text: the keys of its `meta`
/// block that are not lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetaText {
    /// Its name as shown.
    Name,
    /// Who made it.
    Author,
    /// Its version, as its author spells it.
    Version,
    /// Its licence, as its author spells it.
    License,
}

impl MetaText {
    /// Every one, in the order a theme's file has them.
    pub const ALL: [Self; 4] = [Self::Name, Self::Author, Self::Version, Self::License];

    /// Its key in the `meta` block.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Author => "author",
            Self::Version => "version",
            Self::License => "license",
        }
    }

    /// The one whose key is `key`, if one is.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.key() == key)
    }
}

/// The key of the `meta` block's list of tags.
const TAGS_KEY: &str = "tags";

/// The key of the `meta` block's list of screenshots.
const SCREENSHOTS_KEY: &str = "screenshots";

// ============================================================================
// What can go wrong
// ============================================================================

/// Why a theme could not be made, changed, installed, copied or removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoringError {
    /// The name cannot name a new theme: it is not one folder's name
    /// ([`is_valid_id`]), it begins with a dot -- a hidden folder, which no
    /// list shows -- or it is the built-in theme's.
    InvalidName,
    /// There is no user's themes directory: neither `XDG_DATA_HOME` nor
    /// `HOME` is set.
    NoUserDirectory,
    /// Something of that name is already there: an installed theme, or
    /// whatever is at the place a copy was to go.
    Exists,
    /// No theme of that name is installed.
    NotInstalled,
    /// The theme is not the user's own -- the built-in one, or one installed
    /// with the system -- and so is neither changed nor removed: a copy of it
    /// ([`derive()`]) is what changes.
    NotTheUsers,
    /// The section sets no colour of that name.
    NoSuchRole(ColorSection, String),
    /// The colour is see-through, and a theme's colours are opaque: text on a
    /// ground with no colour of its own has no contrast to hold it to.
    Translucent(Color),
    /// The theme's file would be this many bytes, over [`MAX_FILE_BYTES`],
    /// and so would not be read.
    TooLarge(u64),
    /// The theme's file could not be read.
    Unreadable(ThemeError),
    /// A file or folder could not be read, written, renamed or removed: what,
    /// and why.
    Io(String),
    /// The theme to install did not pass its check: everything the check
    /// found, the errors first.
    Refused(Box<Report>),
}

impl fmt::Display for AuthoringError {
    /// A sentence for the person who asked, with no subject to supply.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => write!(
                f,
                "that cannot be a theme's name: a theme's name is one folder's name, does not begin with a dot, and is not `{BUILT_IN}`, the built-in theme's"
            ),
            Self::NoUserDirectory => f.write_str(
                "there is nowhere to keep your themes: neither XDG_DATA_HOME nor HOME is set",
            ),
            Self::Exists => f.write_str("something of that name is already there"),
            Self::NotInstalled => f.write_str("no theme of that name is installed"),
            Self::NotTheUsers => f.write_str(
                "that theme is not one of yours -- it is built in, or installed with the system -- so it is not changed here: derive a copy of it, and change that",
            ),
            Self::NoSuchRole(section, role) => write!(
                f,
                "`{}` has no colour called `{}`",
                section.key(),
                quoted(role)
            ),
            Self::Translucent(color) => write!(
                f,
                "`{}` is see-through, and a theme's colours are opaque",
                color.hex_text()
            ),
            Self::TooLarge(bytes) => write!(
                f,
                "the theme's file would be {bytes} bytes, over the {MAX_FILE_BYTES} a theme's file may be"
            ),
            Self::Unreadable(err) => write!(f, "the theme {err}"),
            Self::Io(what) => f.write_str(what),
            Self::Refused(report) => {
                let errors = report.count(Severity::Error);
                write!(f, "the theme did not pass its check: {errors} error")?;
                if errors != 1 {
                    f.write_str("s")?;
                }
                if let Some(first) = report
                    .findings
                    .iter()
                    .find(|finding| finding.severity == Severity::Error)
                {
                    write!(f, ", the first: {first}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AuthoringError {}

/// An [`AuthoringError::Io`]: what was being done to `path`, and why it
/// failed.
fn io_error(doing: &str, path: &Path, err: &io::Error) -> AuthoringError {
    AuthoringError::Io(format!(
        "could not {doing} {} ({err})",
        pathcodec::display_path(path)
    ))
}

// ============================================================================
// A theme open for changing
// ============================================================================

/// One of the user's themes, open for changing: its file as text, edited in
/// place, and the folder it is saved in.
///
/// Nothing reaches the disk until [`save`](Self::save). What the text means
/// as it stands -- for a preview, or to show what the desktop would ignore --
/// is [`file`](Self::file): the desktop's own reading of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeDraft {
    id: OsString,
    dir: PathBuf,
    doc: Document,
    /// The file as it is on disk -- empty where there is none yet, as in an
    /// icon pack -- to tell whether anything is unsaved.
    saved: String,
}

impl ThemeDraft {
    /// The user's own theme `id`, as its file stands. A theme of the user's
    /// with no file yet -- an icon pack -- opens empty, and saving gives it
    /// one.
    ///
    /// # Errors
    ///
    /// [`AuthoringError::NotTheUsers`] for the built-in theme or one installed
    /// with the system; [`AuthoringError::NotInstalled`] for a name nothing
    /// is installed under; [`AuthoringError::NoUserDirectory`] when there is
    /// no user; [`AuthoringError::Unreadable`] for a file that is too large,
    /// not text, or cannot be read.
    pub fn open(dirs: &ThemeDirs, id: &OsStr) -> Result<Self, AuthoringError> {
        let dir = users_theme(dirs, id)?;
        let saved = read_text(&dir.join(FILE_NAME))?;
        Ok(Self {
            id: id.to_owned(),
            dir,
            doc: Document::parse(&saved),
            saved,
        })
    }

    /// The theme's name: its folder's, and what `appearance.yaml` chooses it
    /// by.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// The folder it is saved in.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What the desktop reads from the text as it stands: its colours, its
    /// description, and what in it would be ignored.
    #[must_use]
    pub fn file(&self) -> ThemeFile {
        parse(&self.doc.to_text())
    }

    /// Whether the text differs from the file on disk.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.doc.to_text() != self.saved
    }

    /// Set the colour `role` of `section`, written `"#rrggbb"` -- in quotes,
    /// as a colour has to be -- on the line that holds it, or a new line at
    /// the section's end.
    ///
    /// # Errors
    ///
    /// [`AuthoringError::NoSuchRole`] for a name the section has no colour
    /// by, and [`AuthoringError::Translucent`] for a colour that is not
    /// opaque. Either way the text is unchanged.
    pub fn set_color(
        &mut self,
        section: ColorSection,
        role: &str,
        color: Color,
    ) -> Result<(), AuthoringError> {
        if !section.roles().contains(&role) {
            return Err(AuthoringError::NoSuchRole(section, role.to_owned()));
        }
        if color.a != u8::MAX {
            return Err(AuthoringError::Translucent(color));
        }
        self.doc.set_str(&[section.key(), role], &color.hex_text());
        Ok(())
    }

    /// Leave the colour `role` of `section` to the built-in theme: its line
    /// goes, and the section with it once nothing is left in it. Whether
    /// there was one to remove.
    pub fn clear_color(&mut self, section: ColorSection, role: &str) -> bool {
        let removed = self.doc.remove(&[section.key(), role]);
        if removed {
            remove_if_empty(&mut self.doc, section.key());
        }
        removed
    }

    /// Set what describes the theme: `value`, trimmed, or -- when that is
    /// empty -- nothing, the key removed. A theme with no name is shown by
    /// its folder's.
    pub fn set_meta(&mut self, field: MetaText, value: &str) {
        set_or_remove(&mut self.doc, &[META_SECTION, field.key()], value);
        remove_if_empty(&mut self.doc, META_SECTION);
    }

    /// Set the words the theme is searched by: each trimmed, the empty ones
    /// dropped, and the list removed when none is left.
    pub fn set_tags(&mut self, tags: &[&str]) {
        let tags: Vec<&str> = tags
            .iter()
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
            .collect();
        if tags.is_empty() {
            self.doc.remove(&[META_SECTION, TAGS_KEY]);
            remove_if_empty(&mut self.doc, META_SECTION);
        } else {
            self.doc.set_seq(&[META_SECTION, TAGS_KEY], &tags);
        }
    }

    /// Write the text to the theme's file -- whole or not at all, through a
    /// temporary renamed over it.
    ///
    /// # Errors
    ///
    /// [`AuthoringError::TooLarge`] for a text the desktop would not read,
    /// and [`AuthoringError::Io`] when the write or the rename fails; either
    /// way the file on disk is as it was.
    pub fn save(&mut self) -> Result<(), AuthoringError> {
        let text = self.doc.to_text();
        write_theme_file(&self.dir, &text)?;
        self.saved = text;
        Ok(())
    }
}

/// Set `path` to `value` trimmed, or remove it when that is empty.
fn set_or_remove(doc: &mut Document, path: &[&str], value: &str) {
    let value = value.trim();
    if value.is_empty() {
        doc.remove(path);
    } else {
        doc.set_str(path, value);
    }
}

/// Remove the top-level `section` when nothing is left in it.
fn remove_if_empty(doc: &mut Document, section: &str) {
    if doc.contains(&[section]) && doc.keys(&[section]).is_empty() {
        doc.remove(&[section]);
    }
}

/// The text of the theme file at `path`, within [`MAX_FILE_BYTES`] -- or
/// nothing, when there is no file.
fn read_text(path: &Path) -> Result<String, AuthoringError> {
    if !path.is_file() {
        return Ok(String::new());
    }
    let bytes = read_theme_bytes(path).map_err(AuthoringError::Unreadable)?;
    String::from_utf8(bytes).map_err(|_| AuthoringError::Unreadable(ThemeError::NotText))
}

/// Write `text` as the theme file in `dir`, whole or not at all.
fn write_theme_file(dir: &Path, text: &str) -> Result<(), AuthoringError> {
    let size = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if size > MAX_FILE_BYTES {
        return Err(AuthoringError::TooLarge(size));
    }
    let path = dir.join(FILE_NAME);
    settingsfile::write_atomic(&path, text.as_bytes()).map_err(|err| io_error("write", &path, &err))
}

// ============================================================================
// Finding themes
// ============================================================================

/// Whether `dir` holds a theme: a theme file, or one of the folders a theme
/// may be alone -- icons, cursors, sounds -- as the checker counts one.
fn holds_a_theme(dir: &Path) -> bool {
    dir.join(FILE_NAME).is_file()
        || [ICONS_DIR, CURSORS_DIR, STEREO_DIR]
            .iter()
            .any(|axis| dir.join(axis).is_dir())
}

/// Whether `id` is hidden: begins with a dot.
fn is_hidden(id: &OsStr) -> bool {
    themecheck::is_hidden(Path::new(id))
}

/// The folder of the user's own theme `id`.
fn users_theme(dirs: &ThemeDirs, id: &OsStr) -> Result<PathBuf, AuthoringError> {
    if id == OsStr::new(BUILT_IN) {
        return Err(AuthoringError::NotTheUsers);
    }
    if !is_valid_id(id) || is_hidden(id) {
        return Err(AuthoringError::NotInstalled);
    }
    let root = dirs
        .user
        .as_deref()
        .ok_or(AuthoringError::NoUserDirectory)?;
    let dir = root.join(id);
    if holds_a_theme(&dir) {
        Ok(dir)
    } else if holds_a_theme(&dirs.system.join(id)) {
        Err(AuthoringError::NotTheUsers)
    } else {
        Err(AuthoringError::NotInstalled)
    }
}

/// Where a new theme `id` goes in the user's directory, if it can: a name
/// that can be a theme's, visible, not the built-in theme's, and held by
/// nothing there -- nor in the system's directory unless `may_stand_in`, a
/// user's theme of a system theme's name standing in for it.
fn new_theme_place(
    dirs: &ThemeDirs,
    id: &OsStr,
    may_stand_in: bool,
) -> Result<PathBuf, AuthoringError> {
    if !is_valid_id(id) || is_hidden(id) || id == OsStr::new(BUILT_IN) {
        return Err(AuthoringError::InvalidName);
    }
    let root = dirs
        .user
        .as_deref()
        .ok_or(AuthoringError::NoUserDirectory)?;
    let place = root.join(id);
    if fs::symlink_metadata(&place).is_ok()
        || (!may_stand_in && fs::symlink_metadata(dirs.system.join(id)).is_ok())
    {
        return Err(AuthoringError::Exists);
    }
    Ok(place)
}

/// A name for a new theme called `name` that nothing in `dirs` has yet --
/// the name its folder gets, and that `appearance.yaml` chooses it by.
///
/// `name` itself wherever it can be: a theme's name may hold any character,
/// so "Nord, warmer" is a folder's name as it stands. Each `/`, `\` and
/// control character becomes `-`; leading dots and spaces go (a hidden
/// folder is no theme); an empty result is `theme`; a long one is cut to 200
/// bytes; and then `-2`, `-3` ... is added until the name is free -- of the
/// user's themes and of the system's, so a new theme never stands in for one
/// the user did not mean to hide.
///
/// When no name is free -- there is no user's directory at all, or ten
/// thousand of them are taken -- the cleaned name is given as it is, and
/// whatever is then asked of it ([`derive()`], [`install`]) says why it cannot
/// be had.
#[must_use]
pub fn id_for(dirs: &ThemeDirs, name: &str) -> OsString {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut base = cleaned
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .trim_end()
        .to_owned();
    if base.len() > MAX_ID_BYTES {
        let cut = (0..=MAX_ID_BYTES)
            .rev()
            .find(|&at| base.is_char_boundary(at))
            .unwrap_or(0);
        base.truncate(cut);
        base.truncate(base.trim_end().len());
    }
    if base.is_empty() {
        "theme".clone_into(&mut base);
    }
    let free = |id: &str| new_theme_place(dirs, OsStr::new(id), false).is_ok();
    if free(&base) {
        return OsString::from(base);
    }
    (2..=MAX_ID_SUFFIX)
        .map(|n| format!("{base}-{n}"))
        .find(|id| free(id))
        .map_or_else(|| OsString::from(&base), OsString::from)
}

// ============================================================================
// Making, installing, copying out and removing themes
// ============================================================================

/// A theme a copy is made from.
enum Source {
    /// The built-in theme: its template and its icons, compiled in.
    BuiltIn,
    /// An installed theme's folder, and where it was found.
    Folder(PathBuf, Origin),
}

impl Source {
    /// The installed theme `id`, the user's copy before the system's -- the
    /// one the desktop would read.
    fn find(dirs: &ThemeDirs, id: &OsStr) -> Result<Self, AuthoringError> {
        if id == OsStr::new(BUILT_IN) {
            return Ok(Self::BuiltIn);
        }
        if !is_valid_id(id) || is_hidden(id) {
            return Err(AuthoringError::NotInstalled);
        }
        dirs.roots()
            .into_iter()
            .find_map(|(root, origin)| {
                let dir = root.join(id);
                holds_a_theme(&dir).then_some(Self::Folder(dir, origin))
            })
            .ok_or(AuthoringError::NotInstalled)
    }
}

/// The file of the installed theme `id` as the desktop reads it -- the user's
/// copy before the system's -- or the built-in theme's template for the
/// built-in one: what a theme sets, to show before it is derived from or
/// chosen. A theme that is installed but has no file -- an icon pack -- sets
/// nothing.
///
/// # Errors
///
/// [`AuthoringError::NotInstalled`] for a name nothing is installed under,
/// and [`AuthoringError::Unreadable`] for a file that is too large, not
/// text, or cannot be read.
pub fn read_theme(dirs: &ThemeDirs, id: &OsStr) -> Result<ThemeFile, AuthoringError> {
    match Source::find(dirs, id)? {
        Source::BuiltIn => Ok(parse(BUILT_IN_TEMPLATE)),
        Source::Folder(dir, _) => Ok(parse(&read_text(&dir.join(FILE_NAME))?)),
    }
}

/// Make a new theme of the user's, `id`, named `name`, from the installed
/// theme `from`: a copy of its folder, which then changes as the user likes
/// while `from` stays as it was.
///
/// The copy's `meta.name` is `name` (none, when `name` is blank: the folder's
/// name shows). Its screenshots are left out: they picture `from`, and a
/// theme browser would show them as pictures of this one. Its author and
/// licence stay, as a licence asking for credit asks. A copy of the built-in
/// theme is its template, with an opening comment saying what it is, and
/// each of its icons as a file.
///
/// `id` must be free ([`id_for`] finds a free one) -- except that a theme
/// installed with the system may be copied under its own name, for the copy
/// to stand in for it.
///
/// Returns the new theme, open, and what the copy left out (see the module
/// documentation): nothing a theme could use, but said, since its author may
/// have meant something by it.
///
/// # Errors
///
/// [`AuthoringError::NotInstalled`] when there is no `from`;
/// [`AuthoringError::InvalidName`] or [`AuthoringError::Exists`] when `id`
/// cannot be had; [`AuthoringError::Unreadable`] when `from`'s file cannot be
/// read; [`AuthoringError::Io`] when the copy cannot be written. On any
/// error nothing is left behind.
pub fn derive(
    dirs: &ThemeDirs,
    from: &OsStr,
    id: &OsStr,
    name: &str,
) -> Result<(ThemeDraft, Vec<Finding>), AuthoringError> {
    let source = Source::find(dirs, from)?;
    let stands_in = id == from && matches!(source, Source::Folder(_, Origin::System));
    let place = new_theme_place(dirs, id, stands_in)?;
    let staging = Staging::new(&users_root(dirs)?, id)?;
    let (text, left_out) = match &source {
        Source::BuiltIn => {
            write_built_in_icons(staging.dir())?;
            (template_for_a_copy(), Vec::new())
        }
        Source::Folder(dir, _) => {
            let text = read_text(&dir.join(FILE_NAME))?;
            // The file is written below, as edited -- not copied and then
            // overwritten, which would write through a second name a link in
            // the folder had given it.
            let mut leave = screenshots_alone(&text);
            leave.insert(PathBuf::from(FILE_NAME));
            let left_out = copy_folder(dir, staging.dir(), &leave)?;
            (text, left_out)
        }
    };
    let mut doc = Document::parse(&text);
    set_or_remove(&mut doc, &[META_SECTION, MetaText::Name.key()], name);
    doc.remove(&[META_SECTION, SCREENSHOTS_KEY]);
    remove_if_empty(&mut doc, META_SECTION);
    write_theme_file(staging.dir(), &doc.to_text())?;
    staging.place(&place)?;
    Ok((ThemeDraft::open(dirs, id)?, left_out))
}

/// Install the theme in `from` as the user's theme `id`: a theme's folder,
/// or a theme's file alone -- which becomes the new folder's `theme.yaml`.
///
/// It is copied first and the copy checked ([`themecheck::check`]), and only
/// a copy that passes is put in place: what is checked is what is installed,
/// so nothing can change between the two. What the copy left out is part of
/// the check -- a link leading out of the folder is an error, as the checker
/// says of one. `id` may be a theme installed with the system, for this one
/// to stand in for it.
///
/// Returns what the check found, for the user to see: its warnings and
/// notes.
///
/// # Errors
///
/// [`AuthoringError::Refused`] with the whole report when the check finds an
/// error; [`AuthoringError::InvalidName`] or [`AuthoringError::Exists`] when
/// `id` cannot be had; [`AuthoringError::TooLarge`] for a file over
/// [`MAX_FILE_BYTES`]; [`AuthoringError::Io`] when `from` cannot be read or
/// the copy cannot be written. On any error nothing is installed.
pub fn install(dirs: &ThemeDirs, from: &Path, id: &OsStr) -> Result<Report, AuthoringError> {
    let place = new_theme_place(dirs, id, true)?;
    let kind = fs::metadata(from).map_err(|err| io_error("read", from, &err))?;
    let staging = Staging::new(&users_root(dirs)?, id)?;
    let left_out = if kind.is_dir() {
        copy_folder(from, staging.dir(), &BTreeSet::new())?
    } else {
        let to = staging.dir().join(FILE_NAME);
        match copy_file(from, &to, MAX_FILE_BYTES) {
            Ok(Copied::Whole) => {}
            Ok(Copied::TooLarge(size)) => return Err(AuthoringError::TooLarge(size)),
            Err(CopyFailure::Read(err)) => return Err(io_error("read", from, &err)),
            Err(CopyFailure::Write(err)) => return Err(io_error("write", &to, &err)),
        }
        Vec::new()
    };
    let report = with_left_out(themecheck::check(staging.dir()), left_out);
    if !report.passes() {
        return Err(AuthoringError::Refused(Box::new(report)));
    }
    staging.place(&place)?;
    Ok(report)
}

/// Copy the installed theme `id` out to the new folder `to`, to share: its
/// folder as the desktop has it -- the built-in theme's template and icons
/// for the built-in one -- then the copy checked.
///
/// Returns what the check found, with what the copy left out: for the user
/// to see before sharing it, since a theme repository runs the same check.
/// Nothing found stops the copy -- it is the user's own, to fix or not.
///
/// # Errors
///
/// [`AuthoringError::NotInstalled`] when there is no `id`;
/// [`AuthoringError::Exists`] when something is at `to` already -- nothing
/// there is replaced or merged into; [`AuthoringError::Io`] when `to` names
/// no folder that can be made, or the copy cannot be written. On any error
/// nothing is left at `to`.
pub fn export(dirs: &ThemeDirs, id: &OsStr, to: &Path) -> Result<Report, AuthoringError> {
    let source = Source::find(dirs, id)?;
    let (Some(parent), Some(name)) = (to.parent(), to.file_name()) else {
        return Err(AuthoringError::Io(format!(
            "{} cannot be a folder's name",
            pathcodec::display_path(to)
        )));
    };
    if fs::symlink_metadata(to).is_ok() {
        return Err(AuthoringError::Exists);
    }
    let beside = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let staging = Staging::new(beside, name)?;
    let left_out = match &source {
        Source::BuiltIn => {
            write_built_in_icons(staging.dir())?;
            write_theme_file(staging.dir(), BUILT_IN_TEMPLATE)?;
            Vec::new()
        }
        Source::Folder(dir, _) => copy_folder(dir, staging.dir(), &BTreeSet::new())?,
    };
    let report = with_left_out(themecheck::check(staging.dir()), left_out);
    staging.place(to)?;
    Ok(report)
}

/// Remove the user's own theme `id`: out of the list at once -- renamed into
/// a hidden folder -- and then deleted. A link to a theme kept elsewhere is
/// removed as a link, and what it leads to stays.
///
/// Whatever chose the theme keeps its choice, and shows the built-in theme
/// with a problem saying why, as for any theme that is not there
/// ([`ColorTheme::load`](super::ColorTheme::load)) -- so a caller may want
/// to ask first, or to choose another.
///
/// # Errors
///
/// [`AuthoringError::NotTheUsers`] for the built-in theme or one installed
/// with the system; [`AuthoringError::NotInstalled`] for a name nothing is
/// installed under; [`AuthoringError::Io`] when it cannot be moved or
/// deleted. If deleting fails after the move, the theme is out of the list
/// and what is left of it is in a hidden folder.
pub fn remove(dirs: &ThemeDirs, id: &OsStr) -> Result<(), AuthoringError> {
    let dir = users_theme(dirs, id)?;
    let kind = fs::symlink_metadata(&dir).map_err(|err| io_error("read", &dir, &err))?;
    if kind.file_type().is_symlink() {
        // A link to a folder is a file to some systems and a folder to others.
        return fs::remove_file(&dir)
            .or_else(|_| fs::remove_dir(&dir))
            .map_err(|err| io_error("remove", &dir, &err));
    }
    let holder = Holder::new(&users_root(dirs)?)?;
    let aside = holder.path().join(id);
    fs::rename(&dir, &aside).map_err(|err| io_error("move", &dir, &err))?;
    fs::remove_dir_all(holder.path()).map_err(|err| io_error("remove", &aside, &err))
}

/// The user's themes directory, made if it is not there yet -- as on a new
/// account, where nothing has been installed.
fn users_root(dirs: &ThemeDirs) -> Result<PathBuf, AuthoringError> {
    let root = dirs.user.clone().ok_or(AuthoringError::NoUserDirectory)?;
    fs::create_dir_all(&root).map_err(|err| io_error("make", &root, &err))?;
    Ok(root)
}

/// The screenshots `text` names, as the paths a copy of its folder walks:
/// left out of a copy, since they picture the theme copied and not the one
/// it becomes -- all but a picture it also recommends as a wallpaper, which
/// the copy uses.
fn screenshots_alone(text: &str) -> BTreeSet<PathBuf> {
    let file = parse(text);
    let wallpapers: BTreeSet<PathBuf> = file
        .wallpapers
        .iter()
        .flat_map(|names| [&names.dark, &names.light])
        .flatten()
        .filter_map(|name| inside_path(name))
        .collect();
    file.meta
        .screenshots
        .iter()
        .filter_map(|name| inside_path(name))
        .filter(|path| !wallpapers.contains(path))
        .collect()
}

/// `name` -- a path a theme's file gives, inside its folder -- as a copy's
/// walk names the same file: its ordinary parts alone, without `.`. `None`
/// for one that is not inside ([`confined`]).
fn inside_path(name: &str) -> Option<PathBuf> {
    confined(Path::new(""), name)?;
    Some(
        Path::new(name)
            .components()
            .filter(|part| matches!(part, Component::Normal(_)))
            .collect(),
    )
}

/// The built-in theme's file as a copy of it starts: without its opening
/// comment -- which is about the built-in theme itself, compiled in and held
/// to its file by tests, none of which is true of a copy -- and with one that
/// says what the copy is.
fn template_for_a_copy() -> String {
    let body: String = BUILT_IN_TEMPLATE
        .split_inclusive('\n')
        .skip_while(|line| {
            let line = line.trim();
            line.is_empty() || line.starts_with('#')
        })
        .collect();
    format!(
        "# A theme made from {BUILT_IN_NAME}, SlateOS's built-in theme.\n\
         #\n\
         # A colour this file leaves out keeps {BUILT_IN_NAME}'s. Colours are opaque,\n\
         # written \"#rrggbb\" -- in quotes, because an unquoted `#` after a space\n\
         # begins a comment. `themecheck <this folder>` says what in it the\n\
         # desktop would refuse, ignore or adjust.\n\
         \n\
         {body}"
    )
}

/// Write each of the built-in theme's icons into `dir`'s icons folder, a file
/// for each name it draws.
fn write_built_in_icons(dir: &Path) -> Result<(), AuthoringError> {
    let icons_dir = dir.join(ICONS_DIR);
    fs::create_dir(&icons_dir).map_err(|err| io_error("make", &icons_dir, &err))?;
    for (name, svg) in icons::built_in_icons() {
        let path = icons_dir.join(format!("{name}.svg"));
        fs::write(&path, svg).map_err(|err| io_error("write", &path, &err))?;
    }
    Ok(())
}

/// `report`, with what the copy it judged left out: the copy's findings
/// first within each severity -- they were found first -- and the most
/// severe first, as the checker orders its own.
fn with_left_out(mut report: Report, mut left_out: Vec<Finding>) -> Report {
    left_out.append(&mut report.findings);
    left_out.sort_by_key(|finding| Reverse(finding.severity));
    report.findings = left_out;
    report
}

// ============================================================================
// Building a folder beside where it goes
// ============================================================================

/// How many holders this process has made: part of each one's name, with
/// the process's own number, so two never collide.
static HOLDERS: AtomicU32 = AtomicU32::new(0);

/// A hidden folder -- `.theme-<process>-<n>` -- beside where a theme goes,
/// holding one being built or removed, and deleted with whatever it holds
/// when dropped. No list shows it, and a failure leaves nothing of it.
struct Holder {
    path: PathBuf,
}

impl Holder {
    /// A new, empty holder in `beside`.
    fn new(beside: &Path) -> Result<Self, AuthoringError> {
        let process = std::process::id();
        for _ in 0..HOLDER_TRIES {
            let n = HOLDERS.fetch_add(1, Ordering::Relaxed);
            let path = beside.join(format!(".theme-{process}-{n}"));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                // Left by an earlier process of the same number, which a
                // crash can do: the next number.
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(io_error("make", &path, &err)),
            }
        }
        Err(AuthoringError::Io(format!(
            "could not make a folder to build the theme in, in {}",
            pathcodec::display_path(beside)
        )))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        // What is in it is no theme -- not yet one, or not one any more --
        // and it is hidden from every list. Failing to delete it leaves
        // litter no list shows, which is not worth an error that would
        // displace the one the caller is already returning.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A theme's folder being built in a [`Holder`], under the name it will have
/// -- so the checker judges the name it will be installed under -- and
/// renamed into place when it is complete.
struct Staging {
    holder: Holder,
    dir: PathBuf,
}

impl Staging {
    /// An empty folder named `name`, in a new holder in `beside`.
    fn new(beside: &Path, name: &OsStr) -> Result<Self, AuthoringError> {
        let holder = Holder::new(beside)?;
        let dir = holder.path().join(name);
        fs::create_dir(&dir).map_err(|err| io_error("make", &dir, &err))?;
        Ok(Self { holder, dir })
    }

    fn dir(&self) -> &Path {
        &self.dir
    }

    /// Put the folder in place at `to`, in one rename. The holder, empty
    /// now, goes when this does.
    fn place(self, to: &Path) -> Result<(), AuthoringError> {
        fs::rename(&self.dir, to).map_err(|err| io_error("move into place", to, &err))?;
        drop(self.holder);
        Ok(())
    }
}

// ============================================================================
// Copying a theme's folder
// ============================================================================

/// Copy the theme folder `from` into the empty folder `to`: everything in it
/// but `leave` (paths inside `from`) and what is not part of a theme, which
/// is left out and said -- see the module documentation.
///
/// Fails only for what goes wrong in `to`, which is the copy's own; what
/// cannot be read in `from` is a finding, as the checker would make it.
fn copy_folder(
    from: &Path,
    to: &Path,
    leave: &BTreeSet<PathBuf>,
) -> Result<Vec<Finding>, AuthoringError> {
    let root = fs::canonicalize(from).map_err(|err| io_error("read", from, &err))?;
    let mut copier = Copier {
        from: from.to_path_buf(),
        root,
        to: to.to_path_buf(),
        leave,
        found: Vec::new(),
        links: Vec::new(),
    };
    copier.walk()?;
    copier.make_links()?;
    Ok(copier.found)
}

/// One copy of a theme's folder, under way.
struct Copier<'a> {
    from: PathBuf,
    /// `from` as the system resolves it, every link on the way followed:
    /// where a link must end for the copy to follow it.
    root: PathBuf,
    to: PathBuf,
    leave: &'a BTreeSet<PathBuf>,
    /// What was left out, and why.
    found: Vec<Finding>,
    /// Each link that leads to a file inside the folder, and that file, both
    /// as paths inside it: made once every file is there.
    links: Vec<(PathBuf, PathBuf)>,
}

impl Copier<'_> {
    fn left_out(&mut self, severity: Severity, rel: &Path, why: impl Into<String>) {
        self.found.push(Finding {
            severity,
            place: themecheck::place(rel),
            message: why.into(),
        });
    }

    /// Copy every folder and file, in name order, within the checker's
    /// bounds; note each link for [`make_links`](Self::make_links).
    fn walk(&mut self) -> Result<(), AuthoringError> {
        let mut pending: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];
        let mut seen = 0_usize;
        while let Some((dir, depth)) = pending.pop() {
            let listing = match fs::read_dir(self.from.join(&dir)) {
                Ok(listing) => listing,
                Err(err) => {
                    self.left_out(
                        Severity::Error,
                        &dir,
                        format!("could not be listed ({err}), and nothing in it was copied"),
                    );
                    continue;
                }
            };
            let mut names: Vec<OsString> = Vec::new();
            for item in listing {
                match item {
                    Ok(item) => names.push(item.file_name()),
                    Err(err) => self.left_out(
                        Severity::Error,
                        &dir,
                        format!("could not be listed whole ({err}), and was copied in part"),
                    ),
                }
            }
            names.sort();
            for name in names {
                seen = seen.saturating_add(1);
                if seen > MAX_ENTRIES {
                    self.left_out(
                        Severity::Error,
                        Path::new(""),
                        format!(
                            "holds more than {MAX_ENTRIES} files and folders, more than any theme; the rest were not copied"
                        ),
                    );
                    return Ok(());
                }
                self.visit(dir.join(name), depth, &mut pending)?;
            }
        }
        Ok(())
    }

    /// One entry, `depth` folders below the top.
    fn visit(
        &mut self,
        rel: PathBuf,
        depth: usize,
        pending: &mut Vec<(PathBuf, usize)>,
    ) -> Result<(), AuthoringError> {
        if self.leave.contains(&rel) {
            return Ok(());
        }
        let path = self.from.join(&rel);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(err) => {
                self.left_out(
                    Severity::Error,
                    &rel,
                    format!("could not be read ({err}), and was not copied"),
                );
                return Ok(());
            }
        };
        let kind = meta.file_type();
        if themecheck::is_hidden(&rel) {
            self.left_out(
                Severity::Warning,
                &rel,
                "is hidden, which no part of a theme is, and was not copied",
            );
        } else if kind.is_symlink() {
            self.note_link(rel);
        } else if kind.is_dir() {
            let below = depth.saturating_add(1);
            if below >= MAX_DEPTH {
                self.left_out(
                    Severity::Error,
                    &rel,
                    format!(
                        "is {MAX_DEPTH} folders down, deeper than anything in a theme, and was not copied"
                    ),
                );
            } else {
                let made = self.to.join(&rel);
                fs::create_dir(&made).map_err(|err| io_error("make", &made, &err))?;
                pending.push((rel, below));
            }
        } else if kind.is_file() {
            self.copy(&path, &rel)?;
        } else {
            self.left_out(
                Severity::Error,
                &rel,
                "is neither a file nor a folder -- a device, a pipe or a socket -- and was not copied",
            );
        }
        Ok(())
    }

    /// Copy the file at `source` to `rel` in the copy, or say why not.
    fn copy(&mut self, source: &Path, rel: &Path) -> Result<(), AuthoringError> {
        let target = self.to.join(rel);
        match copy_file(source, &target, MAX_COPIED_BYTES) {
            Ok(Copied::Whole) => Ok(()),
            Ok(Copied::TooLarge(size)) => {
                self.left_out(
                    Severity::Error,
                    rel,
                    format!(
                        "is {size} bytes, larger than any file in a theme ({MAX_COPIED_BYTES}), and was not copied"
                    ),
                );
                Ok(())
            }
            Err(CopyFailure::Read(err)) => {
                self.left_out(
                    Severity::Error,
                    rel,
                    format!("could not be read ({err}), and was not copied"),
                );
                Ok(())
            }
            Err(CopyFailure::Write(err)) => Err(io_error("write", &target, &err)),
        }
    }

    /// A link: kept when it leads to a file inside the folder, as the system
    /// resolves it -- every link on the way followed -- and left out, and
    /// said, otherwise. Nothing outside the folder is read to find out.
    fn note_link(&mut self, rel: PathBuf) {
        let Ok(end) = fs::canonicalize(self.from.join(&rel)) else {
            self.left_out(
                Severity::Error,
                &rel,
                "is a link to nothing, and was not copied",
            );
            return;
        };
        let Ok(inside) = end.strip_prefix(&self.root) else {
            self.left_out(
                Severity::Error,
                &rel,
                "is a link leading out of the folder, which was not followed, and was not copied",
            );
            return;
        };
        if !end.is_file() {
            self.left_out(
                Severity::Error,
                &rel,
                "is a link to a folder, which nothing in a theme is, and was not copied",
            );
            return;
        }
        let inside = inside.to_path_buf();
        self.links.push((rel, inside));
    }

    /// Make each link kept: a second name for the copy of the file it leads
    /// to -- a hard link, so the dozen older names a cursor answers to cost
    /// no more room than one -- or, where the system cannot make one or that
    /// file was itself not copied, a copy of its own.
    fn make_links(&mut self) -> Result<(), AuthoringError> {
        for (rel, end) in std::mem::take(&mut self.links) {
            let copied = self.to.join(&end);
            if copied.is_file() && fs::hard_link(&copied, self.to.join(&rel)).is_ok() {
                continue;
            }
            let source = self.root.join(&end);
            self.copy(&source, &rel)?;
        }
        Ok(())
    }
}

/// What copying a file came to.
enum Copied {
    /// Every byte.
    Whole,
    /// Nothing: the file is this many bytes, over the limit.
    TooLarge(u64),
}

/// Why a file could not be copied: its side or the copy's.
enum CopyFailure {
    /// The file being copied could not be read.
    Read(io::Error),
    /// The copy could not be written.
    Write(io::Error),
}

/// Copy the file `from` to the new file `to`, within `limit` bytes. Its
/// bytes and not its permissions: a theme's files are data, and a mark
/// saying one may be run is not carried into a copy.
fn copy_file(from: &Path, to: &Path, limit: u64) -> Result<Copied, CopyFailure> {
    let source = fs::File::open(from).map_err(CopyFailure::Read)?;
    let size = source.metadata().map_err(CopyFailure::Read)?.len();
    if size > limit {
        return Ok(Copied::TooLarge(size));
    }
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(CopyFailure::Write)?;
    // Bounded again while copying: the size above was true when it was
    // asked, and a file can grow between the two.
    let mut reader = source.take(limit.saturating_add(1));
    let mut buf = vec![0_u8; 64 * 1024];
    let mut copied: u64 = 0;
    loop {
        let read = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(CopyFailure::Read(err)),
        };
        let chunk = buf.get(..read).unwrap_or_default();
        target.write_all(chunk).map_err(CopyFailure::Write)?;
        copied = copied.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
    if copied > limit {
        drop(target);
        // The copy is half a file nobody wants; failing to delete it leaves
        // it in a holder that is deleted whole.
        let _ = fs::remove_file(to);
        return Ok(Copied::TooLarge(copied));
    }
    Ok(Copied::Whole)
}

#[cfg(test)]
#[path = "authoring_tests.rs"]
mod tests;
