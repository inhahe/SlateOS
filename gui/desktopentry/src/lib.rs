//! Freedesktop desktop entries: what a program is called, which picture stands
//! for it, what kind of program it is, and how it is started.
//!
//! # Why this exists
//!
//! The start menu listed a handful of programs typed into the shell's own
//! source (`desktop::launcher::builtin_app_database`), several of which the
//! system does not have, and none of the hundred and forty it does. A program
//! installed later had no way to appear there at all. `design.txt` asks for an
//! "applications tree" in the start menu; a tree of programs needs, first, a
//! way for a program to say what it is.
//!
//! The format is the Desktop Entry Specification's (version 1.5): a small
//! text file per program, `org.example.Calculator.desktop`, installed under
//! `share/applications`. It is what every Linux desktop reads, so a program
//! ported to SlateOS brings its entry with it, and one written here can be
//! described without inventing a format nobody else reads.
//!
//! # What is here
//!
//! - [`DesktopEntry::parse`] reads a file into its groups and keys, and the
//!   typed accessors ([`DesktopEntry::string`], [`DesktopEntry::list`],
//!   [`DesktopEntry::boolean`], the localized ones) read values as the
//!   specification's value types say.
//! - [`App::from_entry`] validates an entry and gathers what a menu or a
//!   launcher needs into one struct, in the reader's language ([`Locale`]).
//! - [`Exec`] is the command line, parsed once by the specification's quoting
//!   rules and expanded per launch with the files, icon and name it asks for.
//! - [`scan`] walks the data directories in precedence order -- the user's
//!   own first -- so a user's entry overrides or hides the system's.
//! - [`menu`] decides what a menu shows and under which category.
//!
//! # What is deliberately not here
//!
//! Reading the environment and the filesystem is kept to [`scan`] and to the
//! functions that say so, so everything else runs in a test with neither. And
//! nothing here starts a program: `Exec::expand` produces the argument
//! vector, and starting it is the caller's, which knows how programs are
//! started on this system.

use std::fmt;

mod exec;
mod locale;
pub mod menu;
pub mod scan;

pub use exec::{Exec, ExecError, Invocation, Target};
pub use locale::Locale;

/// The group every desktop entry has, holding the entry's own keys.
pub const DESKTOP_ENTRY: &str = "Desktop Entry";

/// The prefix of the groups that describe an application's additional actions
/// -- `[Desktop Action new-window]` -- the jump list of a menu entry.
pub const ACTION_GROUP_PREFIX: &str = "Desktop Action ";

/// A desktop entry file, parsed: its groups in order, with their keys.
///
/// Values are kept as written, escapes and all, and decoded by the accessor
/// that knows the value's type: a list splits on `;` *before* its escapes are
/// decoded, so a decoded value would have lost `\;`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopEntry {
    groups: Vec<Group>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    name: String,
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    key: String,
    /// The `xx` of `Name[xx]`, with any `.ENCODING` part removed.
    locale: Option<String>,
    /// As written after the `=`, surrounding blanks trimmed.
    value: String,
}

/// Why a file is not a desktop entry at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The specification requires UTF-8, and the file is not.
    NotUtf8 {
        /// How many leading bytes were valid.
        valid_up_to: usize,
    },
    /// A key before the first group header.
    EntryOutsideGroup {
        /// One-based line number.
        line: usize,
    },
    /// A line that is not blank, a comment, a group header or `key=value`.
    Unrecognised {
        /// One-based line number.
        line: usize,
    },
    /// A group header whose name holds `[`, `]` or a control character, or
    /// is empty.
    BadGroupName {
        /// One-based line number.
        line: usize,
    },
    /// A key with characters other than `A-Za-z0-9-`, or a malformed locale.
    BadKey {
        /// One-based line number.
        line: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 { valid_up_to } => {
                write!(f, "not UTF-8 (after byte {valid_up_to})")
            }
            Self::EntryOutsideGroup { line } => {
                write!(f, "line {line}: a key before the first [group]")
            }
            Self::Unrecognised { line } => {
                write!(
                    f,
                    "line {line}: neither a [group], a key=value nor a comment"
                )
            }
            Self::BadGroupName { line } => write!(f, "line {line}: a malformed [group] name"),
            Self::BadKey { line } => write!(f, "line {line}: a malformed key"),
        }
    }
}

impl std::error::Error for ParseError {}

impl DesktopEntry {
    /// Parse the bytes of a desktop entry file.
    ///
    /// Strict about structure and lenient about content: a line that is not
    /// a comment, a group or a `key=value` is an error, because the reader
    /// cannot know what it meant; a key nobody knows is kept, because the
    /// specification reserves `X-` keys for exactly that. A second value for
    /// a key replaces the first, and a group written twice is one group, as
    /// the reference reader (GLib's key files) has them.
    ///
    /// # Errors
    ///
    /// [`ParseError`], naming the line.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let text = std::str::from_utf8(bytes).map_err(|e| ParseError::NotUtf8 {
            valid_up_to: e.valid_up_to(),
        })?;
        // A byte-order mark is not UTF-8's business, but editors write one.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut groups: Vec<Group> = Vec::new();
        // The group the next key belongs to: an index, because a group
        // written a second time is the same group and takes its keys.
        let mut current: Option<usize> = None;
        for (index, raw) in text.split('\n').enumerate() {
            let line = index.saturating_add(1);
            let raw = raw.strip_suffix('\r').unwrap_or(raw);
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(inner) = trimmed.strip_prefix('[') {
                let name = inner
                    .strip_suffix(']')
                    .filter(|name| valid_group_name(name))
                    .ok_or(ParseError::BadGroupName { line })?;
                current = Some(match groups.iter().position(|g| g.name == name) {
                    Some(at) => at,
                    None => {
                        groups.push(Group {
                            name: name.to_owned(),
                            entries: Vec::new(),
                        });
                        groups.len().saturating_sub(1)
                    }
                });
                continue;
            }
            let (key_part, value) = trimmed
                .split_once('=')
                .ok_or(ParseError::Unrecognised { line })?;
            let (key, locale) =
                split_key(key_part.trim_end()).ok_or(ParseError::BadKey { line })?;
            let group = current
                .and_then(|at| groups.get_mut(at))
                .ok_or(ParseError::EntryOutsideGroup { line })?;
            group.entries.push(Entry {
                key: key.to_owned(),
                locale: locale.map(str::to_owned),
                value: value.trim_start().to_owned(),
            });
        }
        Ok(Self { groups })
    }

    /// The names of the groups, in the order they were first written.
    #[must_use]
    pub fn group_names(&self) -> Vec<&str> {
        self.groups.iter().map(|g| g.name.as_str()).collect()
    }

    /// Whether the file has a group called `group`.
    #[must_use]
    pub fn has_group(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g.name == group)
    }

    /// The value of `key` in `group` as written, with no locale suffix --
    /// escapes undecoded. The last definition wins.
    #[must_use]
    pub fn raw(&self, group: &str, key: &str) -> Option<&str> {
        self.raw_localized(group, key, None)
    }

    /// The value of `key[locale]` in `group` as written.
    fn raw_localized(&self, group: &str, key: &str, locale: Option<&str>) -> Option<&str> {
        self.groups
            .iter()
            .filter(|g| g.name == group)
            .flat_map(|g| g.entries.iter())
            .rev()
            .find(|e| e.key == key && e.locale.as_deref() == locale)
            .map(|e| e.value.as_str())
    }

    /// The best-matching raw value of `key` for `locale`: the specification's
    /// match order, then the unlocalized key.
    fn raw_for(&self, group: &str, key: &str, locale: Option<&Locale>) -> Option<&str> {
        if let Some(locale) = locale {
            for candidate in locale.candidates() {
                if let Some(value) = self.raw_localized(group, key, Some(&candidate)) {
                    return Some(value);
                }
            }
        }
        self.raw(group, key)
    }

    /// A value of type *string* (or *iconstring*): escapes decoded.
    #[must_use]
    pub fn string(&self, group: &str, key: &str) -> Option<String> {
        self.raw(group, key).map(unescape)
    }

    /// A value of type *localestring*, in `locale` where the file has it.
    #[must_use]
    pub fn locale_string(&self, group: &str, key: &str, locale: Option<&Locale>) -> Option<String> {
        self.raw_for(group, key, locale).map(unescape)
    }

    /// A value of type *boolean*.
    ///
    /// `true` and `false` as the specification has them; `1` and `0`, which
    /// its earlier versions allowed and older files still carry; and either
    /// word in capitals, which the specification forbids and the files a
    /// reader meets do not all know. Anything else is `None`, as though the
    /// key were absent -- a malformed flag does not get to hide a program.
    #[must_use]
    pub fn boolean(&self, group: &str, key: &str) -> Option<bool> {
        let value = self.raw(group, key)?;
        if value.eq_ignore_ascii_case("true") || value == "1" {
            Some(true)
        } else if value.eq_ignore_ascii_case("false") || value == "0" {
            Some(false)
        } else {
            None
        }
    }

    /// A list of *strings*: split on `;` -- `\;` is a semicolon inside an
    /// element -- then each element's escapes decoded. The last element's
    /// trailing `;` is optional; empty elements are dropped.
    #[must_use]
    pub fn list(&self, group: &str, key: &str) -> Vec<String> {
        self.raw(group, key).map(split_list).unwrap_or_default()
    }

    /// A list of *localestrings*, in `locale` where the file has it.
    #[must_use]
    pub fn locale_list(&self, group: &str, key: &str, locale: Option<&Locale>) -> Vec<String> {
        self.raw_for(group, key, locale)
            .map(split_list)
            .unwrap_or_default()
    }
}

/// A group name: any printable ASCII but `[` and `]`, and not empty.
fn valid_group_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii() && !c.is_ascii_control() && c != '[' && c != ']')
}

/// Split `Key[locale]` into its key and its locale, validating both.
///
/// The locale keeps its `_COUNTRY` and `@MODIFIER` and loses any `.ENCODING`,
/// which the specification dropped from keys but older files still write.
fn split_key(text: &str) -> Option<(&str, Option<&str>)> {
    let (key, locale) = match text.split_once('[') {
        Some((key, rest)) => (key, Some(rest.strip_suffix(']')?)),
        None => (text, None),
    };
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let locale = match locale {
        None => None,
        Some(locale) => {
            if locale.is_empty()
                || !locale
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '@' | '-'))
            {
                return None;
            }
            Some(locale)
        }
    };
    Some((key, locale.map(strip_encoding)))
}

/// `de_DE.UTF-8@euro` -> `de_DE@euro`: a key's locale without its encoding.
///
/// Returns a slice when there is nothing to remove; a locale *with* an
/// encoding and a modifier cannot be expressed as one slice, and is so rare
/// in keys (the specification removed encodings from them) that the modifier
/// is kept and the encoding left in place -- matching then fails for that one
/// key, which falls back to the unlocalized value.
fn strip_encoding(locale: &str) -> &str {
    match (locale.find('.'), locale.find('@')) {
        (Some(dot), None) => locale.get(..dot).unwrap_or(locale),
        _ => locale,
    }
}

/// Decode the escapes of a *string* value: `\s` space, `\n` newline, `\t`
/// tab, `\r` carriage return, `\\` backslash. Any other backslash is kept as
/// written -- the `\$` and `` \` `` an `Exec` line uses belong to its own
/// quoting rules, which run after these.
#[must_use]
pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Split a list value on unescaped `;`, then decode each element.
fn split_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            ';' => items.push(std::mem::take(&mut current)),
            '\\' => match chars.next() {
                // An escaped separator is part of the element, and is not
                // an escape the element's own decoding knows.
                Some(';') => current.push(';'),
                Some(other) => {
                    current.push('\\');
                    current.push(other);
                }
                None => current.push('\\'),
            },
            other => current.push(other),
        }
    }
    items.push(current);
    items
        .iter()
        .map(|item| unescape(item))
        .filter(|item| !item.is_empty())
        .collect()
}

// ============================================================================
// The typed view
// ============================================================================

/// What kind of thing an entry describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A program: has an `Exec` line, or is started by D-Bus activation.
    Application,
    /// A link to a URL.
    Link,
    /// A menu directory's own description.
    Directory,
    /// A type the specification does not define. Kept, not refused: a
    /// future version may add one, and a menu simply does not show it.
    Other(String),
}

/// One of an application's additional actions: the rows of its jump list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// The action's identifier, as listed in `Actions=` and named in its
    /// `[Desktop Action <id>]` group.
    pub id: String,
    /// What the action is called, in the reader's language.
    pub name: String,
    /// The action's own icon, if it has one.
    pub icon: Option<String>,
    /// The command line. An action without one is started by D-Bus
    /// activation, which an application with `DBusActivatable` offers.
    pub exec: Option<Exec>,
}

/// An application's entry, validated and in the reader's language: what a
/// menu, a launcher or a file association needs, and no more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// The desktop file ID: the entry's path under `applications/`, with `/`
    /// replaced by `-` -- `org.example.Calc.desktop`.
    pub id: String,
    /// `Type`.
    pub kind: Kind,
    /// `Name`, localized.
    pub name: String,
    /// `GenericName` -- "Web Browser" -- localized.
    pub generic_name: Option<String>,
    /// `Comment`, a tooltip's worth, localized.
    pub comment: Option<String>,
    /// `Icon`: a name in the icon theme, or an absolute path to a picture.
    pub icon: Option<String>,
    /// `NoDisplay`: installed, and not for menus -- a helper, a handler.
    pub no_display: bool,
    /// `Hidden`: treat the entry as deleted. A user's own entry with
    /// `Hidden=true` is how a system program is removed from their menus.
    pub hidden: bool,
    /// `OnlyShowIn`: shown only in these desktops, when not empty.
    pub only_show_in: Vec<String>,
    /// `NotShowIn`: never shown in these desktops.
    pub not_show_in: Vec<String>,
    /// `TryExec`: a program that must exist for this entry to be shown.
    pub try_exec: Option<String>,
    /// `Exec`, parsed.
    pub exec: Option<Exec>,
    /// `Path`: the working directory to start the program in.
    pub path: Option<String>,
    /// `Terminal`: the program runs in a terminal.
    pub terminal: bool,
    /// `Actions`, each with its group, in the order `Actions` lists them.
    pub actions: Vec<Action>,
    /// `MimeType`: the kinds of file the program opens.
    pub mime_types: Vec<String>,
    /// `Categories`: where it belongs in a menu.
    pub categories: Vec<String>,
    /// `Keywords`, localized: more words a search should find it by.
    pub keywords: Vec<String>,
    /// `StartupWMClass`: the window class its windows will carry.
    pub startup_wm_class: Option<String>,
    /// `URL`, for a [`Kind::Link`].
    pub url: Option<String>,
    /// `DBusActivatable`: started by D-Bus rather than by `Exec`.
    pub dbus_activatable: bool,
    /// `SingleMainWindow`: the program has one main window, so a menu need
    /// not offer "new window".
    pub single_main_window: bool,
    /// `PrefersNonDefaultGPU`.
    pub prefers_non_default_gpu: bool,
}

/// Why a parsed file is not a usable entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// No `[Desktop Entry]` group.
    NoDesktopEntryGroup,
    /// No `Type`, which the specification requires.
    MissingType,
    /// No `Name`, which the specification requires.
    MissingName,
    /// An application with neither `Exec` nor `DBusActivatable`.
    MissingExec,
    /// A link with no `URL`.
    MissingUrl,
    /// An `Exec` line (the entry's, or the named action's) that does not
    /// parse.
    BadExec {
        /// The action whose line it is, or `None` for the entry's own.
        action: Option<String>,
        /// What was wrong with it.
        why: ExecError,
    },
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDesktopEntryGroup => write!(f, "no [{DESKTOP_ENTRY}] group"),
            Self::MissingType => write!(f, "no Type"),
            Self::MissingName => write!(f, "no Name"),
            Self::MissingExec => write!(f, "an application with no Exec"),
            Self::MissingUrl => write!(f, "a link with no URL"),
            Self::BadExec { action: None, why } => write!(f, "Exec: {why}"),
            Self::BadExec {
                action: Some(action),
                why,
            } => write!(f, "action {action}: Exec: {why}"),
        }
    }
}

impl std::error::Error for Invalid {}

impl App {
    /// Validate `entry`, known by the desktop file ID `id`, and read it in
    /// `locale` (`None`: the unlocalized values).
    ///
    /// An action that `Actions` lists but the file does not describe, or
    /// describes with no `Name`, is left out: the rest of the entry is
    /// usable, and a jump list with one row missing is better than a program
    /// missing from the menu. An action whose `Exec` does not parse is an
    /// error, as the entry's own is -- a row that would start the wrong
    /// thing is not a row to offer.
    ///
    /// # Errors
    ///
    /// [`Invalid`], saying what the specification requires and the file
    /// lacks.
    pub fn from_entry(
        entry: &DesktopEntry,
        id: &str,
        locale: Option<&Locale>,
    ) -> Result<Self, Invalid> {
        let g = DESKTOP_ENTRY;
        if !entry.has_group(g) {
            return Err(Invalid::NoDesktopEntryGroup);
        }
        let kind = match entry.string(g, "Type").as_deref() {
            None => return Err(Invalid::MissingType),
            Some("Application") => Kind::Application,
            Some("Link") => Kind::Link,
            Some("Directory") => Kind::Directory,
            Some(other) => Kind::Other(other.to_owned()),
        };
        let name = entry
            .locale_string(g, "Name", locale)
            .filter(|name| !name.is_empty())
            .ok_or(Invalid::MissingName)?;
        let dbus_activatable = entry.boolean(g, "DBusActivatable").unwrap_or(false);
        let exec = match entry.string(g, "Exec") {
            Some(line) => {
                Some(Exec::parse(&line).map_err(|why| Invalid::BadExec { action: None, why })?)
            }
            None => None,
        };
        if kind == Kind::Application && exec.is_none() && !dbus_activatable {
            return Err(Invalid::MissingExec);
        }
        let url = entry.string(g, "URL");
        if kind == Kind::Link && url.is_none() {
            return Err(Invalid::MissingUrl);
        }
        let mut actions = Vec::new();
        for action_id in entry.list(g, "Actions") {
            let group = format!("{ACTION_GROUP_PREFIX}{action_id}");
            // No group, or a group with no name: either way nothing a row
            // could say.
            let Some(action_name) = entry
                .locale_string(&group, "Name", locale)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let action_exec = match entry.string(&group, "Exec") {
                Some(line) => Some(Exec::parse(&line).map_err(|why| Invalid::BadExec {
                    action: Some(action_id.clone()),
                    why,
                })?),
                None => None,
            };
            actions.push(Action {
                id: action_id,
                name: action_name,
                icon: entry.string(&group, "Icon").filter(|icon| !icon.is_empty()),
                exec: action_exec,
            });
        }
        Ok(Self {
            id: id.to_owned(),
            kind,
            name,
            generic_name: entry
                .locale_string(g, "GenericName", locale)
                .filter(|s| !s.is_empty()),
            comment: entry
                .locale_string(g, "Comment", locale)
                .filter(|s| !s.is_empty()),
            icon: entry
                .locale_string(g, "Icon", locale)
                .filter(|s| !s.is_empty()),
            no_display: entry.boolean(g, "NoDisplay").unwrap_or(false),
            hidden: entry.boolean(g, "Hidden").unwrap_or(false),
            only_show_in: entry.list(g, "OnlyShowIn"),
            not_show_in: entry.list(g, "NotShowIn"),
            try_exec: entry.string(g, "TryExec").filter(|s| !s.is_empty()),
            exec,
            path: entry.string(g, "Path").filter(|s| !s.is_empty()),
            terminal: entry.boolean(g, "Terminal").unwrap_or(false),
            actions,
            mime_types: entry.list(g, "MimeType"),
            categories: entry.list(g, "Categories"),
            keywords: entry.locale_list(g, "Keywords", locale),
            startup_wm_class: entry.string(g, "StartupWMClass").filter(|s| !s.is_empty()),
            url,
            dbus_activatable,
            single_main_window: entry.boolean(g, "SingleMainWindow").unwrap_or(false),
            prefers_non_default_gpu: entry.boolean(g, "PrefersNonDefaultGPU").unwrap_or(false),
        })
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

    fn parse(text: &str) -> DesktopEntry {
        DesktopEntry::parse(text.as_bytes()).expect("parses")
    }

    const CALC: &str = "\
# A comment, and a blank line after it.

[Desktop Entry]
Type=Application
Name=Calculator
Name[de]=Taschenrechner
Name[de_CH]=Rechner
Name[sr@latin]=Kalkulator
GenericName=Calculator
Comment=Do sums
Comment[fr]=Faire des calculs
Icon=accessories-calculator
Exec=calculator %U
TryExec=calculator
Terminal=false
Categories=Utility;Calculator;
Keywords=math;arithmetic;sum;
Keywords[de]=Mathe;Rechnen;
MimeType=text/x-calc;
Actions=new;scientific;missing;
StartupWMClass=Calculator

[Desktop Action new]
Name=New Window
Name[de]=Neues Fenster
Exec=calculator --new-window

[Desktop Action scientific]
Name=Scientific
Icon=calc-scientific
Exec=calculator --scientific
";

    // ---- parsing ----

    /// **A file reads into its groups and keys, comments and blank lines
    /// ignored.**
    #[test]
    fn a_file_reads_into_groups_and_keys() {
        let entry = parse(CALC);
        assert_eq!(
            entry.group_names(),
            [
                "Desktop Entry",
                "Desktop Action new",
                "Desktop Action scientific"
            ]
        );
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Name"), Some("Calculator"));
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Exec"), Some("calculator %U"));
        assert_eq!(
            entry.raw("Desktop Action new", "Exec"),
            Some("calculator --new-window")
        );
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Nothing"), None);
    }

    /// Blanks around `=` are not part of the key or the value; CRLF line ends
    /// and a byte-order mark are tolerated.
    #[test]
    fn blanks_around_the_equals_and_windows_line_ends_are_ignored() {
        let entry = parse("\u{feff}[Desktop Entry]\r\nName = Spaced \r\nType=Application\r\n");
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Name"), Some("Spaced"));
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Type"), Some("Application"));
    }

    /// A value keeps an `=` of its own: only the first splits.
    #[test]
    fn a_value_may_hold_an_equals_sign() {
        let entry = parse("[Desktop Entry]\nExec=env A=1 prog\n");
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Exec"), Some("env A=1 prog"));
    }

    /// **Malformed structure is an error naming the line**, rather than a
    /// guess about what the line meant.
    #[test]
    fn malformed_lines_are_errors_naming_the_line() {
        let err = |text: &str| DesktopEntry::parse(text.as_bytes()).expect_err("malformed");
        assert_eq!(
            err("Name=x\n[Desktop Entry]\n"),
            ParseError::EntryOutsideGroup { line: 1 }
        );
        assert_eq!(
            err("[Desktop Entry]\njust words\n"),
            ParseError::Unrecognised { line: 2 }
        );
        assert_eq!(
            err("[Desktop Entry]\n[Bad]Group]\n"),
            ParseError::BadGroupName { line: 2 }
        );
        assert_eq!(err("[]\n"), ParseError::BadGroupName { line: 1 });
        assert_eq!(
            err("[Desktop Entry]\nNa me=x\n"),
            ParseError::BadKey { line: 2 }
        );
        assert_eq!(
            err("[Desktop Entry]\nName[de=x\n"),
            ParseError::BadKey { line: 2 }
        );
        assert_eq!(
            err("[Desktop Entry]\nName[]=x\n"),
            ParseError::BadKey { line: 2 }
        );
        assert_eq!(err("[Desktop Entry]\n=x\n"), ParseError::BadKey { line: 2 });
    }

    /// The specification requires UTF-8, and a file that is not is refused
    /// rather than read with its bytes replaced.
    #[test]
    fn a_file_that_is_not_utf8_is_refused() {
        let bytes = b"[Desktop Entry]\nName=Caf\xe9\n";
        assert_eq!(
            DesktopEntry::parse(bytes),
            Err(ParseError::NotUtf8 { valid_up_to: 24 })
        );
    }

    /// A key written twice takes its second value, and a group written twice
    /// is one group -- as GLib's reader has both.
    #[test]
    fn a_second_value_or_a_reopened_group_wins() {
        let entry = parse(
            "[Desktop Entry]\nName=First\n[Other]\nA=1\n[Desktop Entry]\nName=Second\nIcon=x\n",
        );
        assert_eq!(entry.group_names(), ["Desktop Entry", "Other"]);
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Name"), Some("Second"));
        assert_eq!(entry.raw(DESKTOP_ENTRY, "Icon"), Some("x"));
        assert_eq!(entry.raw("Other", "A"), Some("1"));
    }

    // ---- values ----

    /// **Escapes are decoded by type**: `\s \n \t \r \\` in strings, and `\;`
    /// only where a list splits.
    #[test]
    fn escapes_are_decoded_by_the_values_type() {
        let entry =
            parse("[Desktop Entry]\nComment=a\\sb\\nc\\td\\\\e\\$f\nList=one\\;two;three;;\n");
        assert_eq!(
            entry.string(DESKTOP_ENTRY, "Comment").as_deref(),
            Some("a b\nc\td\\e\\$f")
        );
        assert_eq!(entry.list(DESKTOP_ENTRY, "List"), ["one;two", "three"]);
        // Read as a string, the escaped separator is not an escape it knows.
        assert_eq!(
            entry.string(DESKTOP_ENTRY, "List").as_deref(),
            Some("one\\;two;three;;")
        );
    }

    /// Booleans as the specification has them, and as older and careless
    /// files write them; anything else reads as absent.
    #[test]
    fn booleans_read_every_spelling_files_use() {
        let entry = parse("[Desktop Entry]\nA=true\nB=false\nC=1\nD=0\nE=True\nF=FALSE\nG=yes\n");
        let b = |key| entry.boolean(DESKTOP_ENTRY, key);
        assert_eq!(
            [
                b("A"),
                b("B"),
                b("C"),
                b("D"),
                b("E"),
                b("F"),
                b("G"),
                b("H")
            ],
            [
                Some(true),
                Some(false),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
                None,
                None
            ]
        );
    }

    /// A list's trailing separator is optional.
    #[test]
    fn a_lists_trailing_separator_is_optional() {
        let entry = parse("[Desktop Entry]\nA=x;y\nB=x;y;\nC=\n");
        assert_eq!(entry.list(DESKTOP_ENTRY, "A"), ["x", "y"]);
        assert_eq!(entry.list(DESKTOP_ENTRY, "B"), ["x", "y"]);
        assert!(entry.list(DESKTOP_ENTRY, "C").is_empty());
        assert!(entry.list(DESKTOP_ENTRY, "Absent").is_empty());
    }

    // ---- locales ----

    /// **A localized value is chosen by the specification's match order**:
    /// `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`, `lang@MODIFIER`, `lang`, then
    /// the unlocalized value.
    #[test]
    fn a_localized_value_is_chosen_in_the_specifications_order() {
        let entry = parse(CALC);
        let name = |locale: &str| {
            entry
                .locale_string(DESKTOP_ENTRY, "Name", Locale::parse(locale).as_ref())
                .expect("a name")
        };
        assert_eq!(name("de_CH.UTF-8"), "Rechner");
        assert_eq!(name("de_AT.UTF-8"), "Taschenrechner");
        assert_eq!(name("de"), "Taschenrechner");
        assert_eq!(name("sr_RS@latin"), "Kalkulator");
        assert_eq!(name("sr_RS"), "Calculator");
        assert_eq!(name("fr_FR"), "Calculator");
        assert_eq!(
            entry.locale_string(DESKTOP_ENTRY, "Name", None).as_deref(),
            Some("Calculator")
        );
        assert_eq!(
            entry.locale_list(DESKTOP_ENTRY, "Keywords", Locale::parse("de_DE").as_ref()),
            ["Mathe", "Rechnen"]
        );
    }

    /// A key written with an encoding, as older files do, still matches.
    #[test]
    fn a_keys_old_encoding_suffix_is_ignored() {
        let entry = parse("[Desktop Entry]\nName=Plain\nName[de_DE.UTF-8]=Deutsch\n");
        assert_eq!(
            entry
                .locale_string(DESKTOP_ENTRY, "Name", Locale::parse("de_DE.UTF-8").as_ref())
                .as_deref(),
            Some("Deutsch")
        );
    }

    // ---- the typed view ----

    /// **An application entry becomes an [`App`]**, localized, with its
    /// actions in `Actions`' order -- and a listed action the file never
    /// describes left out.
    #[test]
    fn an_application_entry_becomes_an_app() {
        let app = App::from_entry(
            &parse(CALC),
            "calculator.desktop",
            Locale::parse("de_DE").as_ref(),
        )
        .expect("valid");
        assert_eq!(app.id, "calculator.desktop");
        assert_eq!(app.kind, Kind::Application);
        assert_eq!(app.name, "Taschenrechner");
        assert_eq!(app.comment.as_deref(), Some("Do sums"));
        assert_eq!(app.icon.as_deref(), Some("accessories-calculator"));
        assert_eq!(app.categories, ["Utility", "Calculator"]);
        assert_eq!(app.keywords, ["Mathe", "Rechnen"]);
        assert_eq!(app.mime_types, ["text/x-calc"]);
        assert_eq!(app.try_exec.as_deref(), Some("calculator"));
        assert_eq!(app.startup_wm_class.as_deref(), Some("Calculator"));
        assert!(!app.terminal && !app.no_display && !app.hidden);
        let actions: Vec<(&str, &str, Option<&str>)> = app
            .actions
            .iter()
            .map(|a| (a.id.as_str(), a.name.as_str(), a.icon.as_deref()))
            .collect();
        assert_eq!(
            actions,
            [
                ("new", "Neues Fenster", None),
                ("scientific", "Scientific", Some("calc-scientific"))
            ]
        );
        assert!(app.actions.iter().all(|a| a.exec.is_some()));
    }

    /// **What the specification requires is required**: a group, a type, a
    /// name, and for an application a way to start it.
    #[test]
    fn what_the_specification_requires_is_required() {
        let invalid =
            |text: &str| App::from_entry(&parse(text), "x.desktop", None).expect_err("invalid");
        assert_eq!(invalid("[Other]\nName=x\n"), Invalid::NoDesktopEntryGroup);
        assert_eq!(invalid("[Desktop Entry]\nName=x\n"), Invalid::MissingType);
        assert_eq!(
            invalid("[Desktop Entry]\nType=Application\nExec=x\n"),
            Invalid::MissingName
        );
        assert_eq!(
            invalid("[Desktop Entry]\nType=Application\nName=\nExec=x\n"),
            Invalid::MissingName
        );
        assert_eq!(
            invalid("[Desktop Entry]\nType=Application\nName=x\n"),
            Invalid::MissingExec
        );
        assert_eq!(
            invalid("[Desktop Entry]\nType=Link\nName=x\n"),
            Invalid::MissingUrl
        );
        assert!(matches!(
            invalid("[Desktop Entry]\nType=Application\nName=x\nExec=\"unterminated\n"),
            Invalid::BadExec { action: None, .. }
        ));
        assert!(matches!(
            invalid("[Desktop Entry]\nType=Application\nName=x\nExec=x\nActions=a;\n[Desktop Action a]\nName=A\nExec=x %z\n"),
            Invalid::BadExec { action: Some(a), .. } if a == "a"
        ));
    }

    /// A program started by D-Bus needs no `Exec`; a link, a directory and a
    /// type from the future are entries too.
    #[test]
    fn other_kinds_and_dbus_programs_are_valid() {
        let valid = |text: &str| App::from_entry(&parse(text), "x.desktop", None).expect("valid");
        assert!(
            valid("[Desktop Entry]\nType=Application\nName=x\nDBusActivatable=true\n")
                .exec
                .is_none()
        );
        assert_eq!(
            valid("[Desktop Entry]\nType=Link\nName=x\nURL=https://example.org\n").kind,
            Kind::Link
        );
        assert_eq!(
            valid("[Desktop Entry]\nType=Directory\nName=x\n").kind,
            Kind::Directory
        );
        assert_eq!(
            valid("[Desktop Entry]\nType=Widget\nName=x\n").kind,
            Kind::Other("Widget".to_owned())
        );
    }
}
