//! The programs SlateOS knows, as one list: what each is called, what it
//! opens, which jobs it can be the default for, and what opens what until a
//! person chooses.
//!
//! # Why this exists
//!
//! `design-decisions.md` §1425, the operator's answer to C-Q20. Fourteen parts
//! of the system each kept some of this -- nine kernel modules, the shell's own
//! program table, the File Associations program, the toolkit's file types, and
//! a deleted default-programs panel -- and none could read another's, so they
//! disagreed about where the text editor was (`/usr/bin/editor`,
//! `/usr/bin/text-editor`, `/usr/bin/edit`) and about programs that do not
//! exist. `INVENTORY.md`, beside this file, lists every item any of them held
//! and where it now lives; `tests/inventory.rs` holds that to the code.
//!
//! # The format is the desktop entry
//!
//! A program here is a freedesktop desktop entry
//! (`applications/org.slateos.<Program>.desktop`), parsed by
//! [`desktopentry`] -- the format the start menu already reads for programs
//! installed on the machine. So SlateOS's own programs and installed ones are
//! one kind of thing: an installed entry with the same id replaces the
//! built-in one, and the same files install unchanged as
//! `/usr/share/applications/*.desktop` the day the image ships them.
//! `design-decisions.md` §1429 records why this and not a table of structs.
//!
//! The defaults -- what opens a type before anyone chooses -- are
//! `defaults.list`, in the freedesktop `mimeapps.list` format for the same
//! reason. What a person *chooses* is not here: that is `gui/associations`,
//! which a default here stands behind.
//!
//! # What is here
//!
//! - [`built_in`]: SlateOS's own programs.
//! - [`default_for`]: the built-in default for a type.
//! - [`Role`]: the jobs a program can be the default for -- web browser, text
//!   editor, terminal and the rest -- and [`Role::filled_by`], which of a list
//!   of programs does each.

use desktopentry::{App, DesktopEntry, Locale};

/// SlateOS's own programs' desktop entries: the file name each installs
/// under, and its text.
///
/// Embedded rather than read from disk because the image does not install
/// them yet, and a program list that is empty until it does is the menu this
/// crate exists to keep. Adding a program is a file in `applications/` and a
/// line here; `tests/inventory.rs` fails if a file is not listed.
pub const BUILT_IN: &[(&str, &str)] = &[
    (
        "org.slateos.ArchiveManager.desktop",
        include_str!("../applications/org.slateos.ArchiveManager.desktop"),
    ),
    (
        "org.slateos.Calculator.desktop",
        include_str!("../applications/org.slateos.Calculator.desktop"),
    ),
    (
        "org.slateos.Calendar.desktop",
        include_str!("../applications/org.slateos.Calendar.desktop"),
    ),
    (
        "org.slateos.Editor.desktop",
        include_str!("../applications/org.slateos.Editor.desktop"),
    ),
    (
        "org.slateos.Explorer.desktop",
        include_str!("../applications/org.slateos.Explorer.desktop"),
    ),
    (
        "org.slateos.HexEditor.desktop",
        include_str!("../applications/org.slateos.HexEditor.desktop"),
    ),
    (
        "org.slateos.ImageViewer.desktop",
        include_str!("../applications/org.slateos.ImageViewer.desktop"),
    ),
    (
        "org.slateos.MusicPlayer.desktop",
        include_str!("../applications/org.slateos.MusicPlayer.desktop"),
    ),
    (
        "org.slateos.PdfViewer.desktop",
        include_str!("../applications/org.slateos.PdfViewer.desktop"),
    ),
    (
        "org.slateos.ProcessExplorer.desktop",
        include_str!("../applications/org.slateos.ProcessExplorer.desktop"),
    ),
    (
        "org.slateos.Screenshot.desktop",
        include_str!("../applications/org.slateos.Screenshot.desktop"),
    ),
    (
        "org.slateos.Settings.desktop",
        include_str!("../applications/org.slateos.Settings.desktop"),
    ),
    (
        "org.slateos.SystemInformation.desktop",
        include_str!("../applications/org.slateos.SystemInformation.desktop"),
    ),
    (
        "org.slateos.Terminal.desktop",
        include_str!("../applications/org.slateos.Terminal.desktop"),
    ),
    (
        "org.slateos.VideoPlayer.desktop",
        include_str!("../applications/org.slateos.VideoPlayer.desktop"),
    ),
];

/// The built-in defaults, in the `mimeapps.list` format: what opens each type
/// until a person chooses otherwise.
pub const DEFAULTS: &str = include_str!("../defaults.list");

/// The group of [`DEFAULTS`] that names defaults.
const DEFAULT_APPLICATIONS: &str = "Default Applications";

/// SlateOS's own programs, in `locale` (or untranslated), in the order of
/// [`BUILT_IN`].
///
/// An entry that does not parse or validate is left out rather than failing
/// the list: the entries are compiled in, so one that fails is a defect in
/// this crate, which `tests/inventory.rs` checks entry by entry -- at run time
/// the rest of the list is still worth having.
#[must_use]
pub fn built_in(locale: Option<&Locale>) -> Vec<App> {
    BUILT_IN
        .iter()
        .filter_map(|(id, text)| {
            let entry = DesktopEntry::parse(text.as_bytes()).ok()?;
            App::from_entry(&entry, id, locale).ok()
        })
        .collect()
}

/// The built-in default for the type `mime`: a desktop file id, such as
/// `org.slateos.Editor.desktop`, or `None` when nothing opens it by default.
///
/// Matched exactly and case-insensitively, as MIME types are compared. The
/// `mimeapps.list` value may name several ids, most preferred first; the
/// first is the default.
#[must_use]
pub fn default_for(mime: &str) -> Option<&'static str> {
    defaults()
        .find(|(listed, _)| listed.eq_ignore_ascii_case(mime))
        .map(|(_, id)| id)
}

/// Every built-in default, as `(type, desktop file id)`, in file order.
pub fn defaults() -> impl Iterator<Item = (&'static str, &'static str)> {
    read_defaults(DEFAULTS)
}

/// The defaults `text` names, in the `mimeapps.list` format: each line of the
/// `[Default Applications]` group, as its type and the first desktop file id
/// its value lists. Comments, blank lines, other groups and lines that are
/// not `type=ids` are passed over -- a file a person edited is read for what
/// it can say, as `gui/desktopentry` reads an entry.
fn read_defaults(text: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut in_group = false;
    text.lines().filter_map(move |line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|g| g.strip_suffix(']')) {
            in_group = group == DEFAULT_APPLICATIONS;
            return None;
        }
        if !in_group {
            return None;
        }
        let (mime, ids) = line.split_once('=')?;
        let first = ids.split(';').map(str::trim).find(|id| !id.is_empty())?;
        Some((mime.trim(), first))
    })
}

/// A job a program can be the default for.
///
/// The union of the two role lists the system had: the kernel's
/// `fs::defaultapps` (fourteen) and the deleted `default_apps.rs` panel's
/// (twelve, all among the fourteen once its "document reader" and the
/// kernel's "PDF viewer" are seen to be one job). `INVENTORY.md` section 3.
///
/// A role with no program is kept, and [`Role::filled_by`] says so with
/// `None`: "no web browser is installed" is an answer a settings page can
/// show, where leaving the role out would hide the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    WebBrowser,
    Email,
    FileManager,
    TextEditor,
    Terminal,
    ImageViewer,
    VideoPlayer,
    MusicPlayer,
    DocumentReader,
    ArchiveManager,
    Calculator,
    Calendar,
    Maps,
    SystemMonitor,
}

/// What makes a program able to do a [`Role`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Definition {
    /// It opens these types, the first being the one whose default fills the
    /// role. A scheme is a type here as in `mimeapps.list`:
    /// `x-scheme-handler/http` is "opens web links".
    Types(&'static [&'static str]),
    /// It carries this desktop-entry category: for a job that is a kind of
    /// program rather than a kind of file -- a terminal, a calculator.
    Category(&'static str),
}

impl Role {
    /// Every role, in the order a settings page lists them.
    pub const ALL: [Self; 14] = [
        Self::WebBrowser,
        Self::Email,
        Self::FileManager,
        Self::TextEditor,
        Self::Terminal,
        Self::ImageViewer,
        Self::VideoPlayer,
        Self::MusicPlayer,
        Self::DocumentReader,
        Self::ArchiveManager,
        Self::Calculator,
        Self::Calendar,
        Self::Maps,
        Self::SystemMonitor,
    ];

    /// The role's name, as a settings page shows it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WebBrowser => "Web browser",
            Self::Email => "Email",
            Self::FileManager => "File manager",
            Self::TextEditor => "Text editor",
            Self::Terminal => "Terminal",
            Self::ImageViewer => "Image viewer",
            Self::VideoPlayer => "Video player",
            Self::MusicPlayer => "Music player",
            Self::DocumentReader => "Document reader",
            Self::ArchiveManager => "Archive manager",
            Self::Calculator => "Calculator",
            Self::Calendar => "Calendar",
            Self::Maps => "Maps",
            Self::SystemMonitor => "System monitor",
        }
    }

    /// What a program needs to do this job.
    ///
    /// The types are the kernel's (`fs::defaultapps`), in the toolkit's
    /// spelling where the two differed. Three roles are a category rather than
    /// a type: the kernel gave the terminal `x-scheme-handler/terminal`, a
    /// scheme registered nowhere, and gave the calculator and the system
    /// monitor no type at all -- they are kinds of program, not openers of a
    /// kind of file. The calendar is one too: the calendar program does not
    /// open a `.ics` file named on its command line, so "opens
    /// `text/calendar`" would find nothing.
    #[must_use]
    pub const fn definition(self) -> Definition {
        match self {
            Self::WebBrowser => Definition::Types(&[
                "x-scheme-handler/http",
                "x-scheme-handler/https",
                "text/html",
                "application/xhtml+xml",
            ]),
            Self::Email => Definition::Types(&["x-scheme-handler/mailto", "message/rfc822"]),
            Self::FileManager => Definition::Types(&["inode/directory"]),
            Self::TextEditor => Definition::Types(&[
                "text/plain",
                "text/x-c",
                "text/x-python",
                "application/json",
            ]),
            Self::Terminal => Definition::Category("TerminalEmulator"),
            Self::ImageViewer => Definition::Types(&[
                "image/png",
                "image/jpeg",
                "image/gif",
                "image/bmp",
                "image/svg+xml",
                "image/webp",
            ]),
            Self::VideoPlayer => Definition::Types(&[
                "video/mp4",
                "video/x-matroska",
                "video/webm",
                "video/x-msvideo",
            ]),
            Self::MusicPlayer => Definition::Types(&[
                "audio/mpeg",
                "audio/ogg",
                "audio/flac",
                "audio/wav",
                "audio/aac",
            ]),
            Self::DocumentReader => Definition::Types(&["application/pdf", "application/epub+zip"]),
            Self::ArchiveManager => Definition::Types(&[
                "application/zip",
                "application/x-tar",
                "application/gzip",
                "application/x-7z-compressed",
            ]),
            Self::Calculator => Definition::Category("Calculator"),
            Self::Calendar => Definition::Category("Calendar"),
            Self::Maps => Definition::Types(&["x-scheme-handler/geo"]),
            Self::SystemMonitor => Definition::Category("Monitor"),
        }
    }

    /// The program among `programs` that does this job, or `None` when none
    /// can.
    ///
    /// For a role defined by types: the built-in default for its first type,
    /// when that program is among `programs`; otherwise the first program that
    /// opens that type. For a role defined by a category: the first program
    /// carrying it. `programs` is whatever list the caller has -- the built-in
    /// one, or the installed programs with the built-in ones behind them -- and
    /// its order is the tie-break.
    #[must_use]
    pub fn filled_by(self, programs: &[App]) -> Option<&App> {
        match self.definition() {
            Definition::Types(types) => {
                let primary = types.first()?;
                default_for(primary)
                    .and_then(|id| programs.iter().find(|app| app.id == id))
                    .or_else(|| {
                        programs.iter().find(|app| {
                            app.mime_types
                                .iter()
                                .any(|m| m.eq_ignore_ascii_case(primary))
                        })
                    })
            }
            Definition::Category(category) => programs
                .iter()
                .find(|app| app.categories.iter().any(|c| c == category)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_default_is_found_whatever_the_case_of_the_type() {
        assert_eq!(
            default_for("text/plain"),
            Some("org.slateos.Editor.desktop")
        );
        assert_eq!(
            default_for("TEXT/Plain"),
            Some("org.slateos.Editor.desktop")
        );
        assert_eq!(default_for("application/x-nothing"), None);
    }

    /// The first id of a list is the default; a group other than
    /// `[Default Applications]` names none; a line that says nothing is passed
    /// over.
    #[test]
    fn the_defaults_reader_takes_the_first_id_and_only_its_group() {
        let text = "# c\n[Added Associations]\ntext/plain=a.desktop\n\
                    [Default Applications]\nimage/png= b.desktop ; c.desktop;\n\
                    audio/ogg=;;d.desktop\nbroken line\nvideo/mp4=;\n\
                    [Removed Associations]\nimage/gif=e.desktop\n";
        let read: Vec<(&str, &str)> = read_defaults(text).collect();
        assert_eq!(
            read,
            [("image/png", "b.desktop"), ("audio/ogg", "d.desktop")]
        );
    }

    #[test]
    fn a_role_nothing_can_do_is_kept_and_answers_none() {
        let programs = built_in(None);
        assert!(Role::ALL.contains(&Role::WebBrowser));
        assert!(Role::WebBrowser.filled_by(&programs).is_none());
        assert!(Role::Maps.filled_by(&programs).is_none());
    }

    /// A program installed later that opens the role's type fills it when the
    /// built-in default is not in the list -- a machine with a browser has a
    /// web browser.
    #[test]
    fn an_installed_program_fills_a_role_the_defaults_leave_open() {
        let entry = DesktopEntry::parse(
            b"[Desktop Entry]\nType=Application\nName=Web\nExec=web %u\n\
              MimeType=x-scheme-handler/http;text/html;\n",
        )
        .unwrap();
        let web = App::from_entry(&entry, "org.example.Web.desktop", None).unwrap();
        let mut programs = built_in(None);
        programs.push(web);
        assert_eq!(
            Role::WebBrowser
                .filled_by(&programs)
                .map(|app| app.id.as_str()),
            Some("org.example.Web.desktop")
        );
    }

    /// The built-in default wins over an installed program that also opens the
    /// type, when the default is in the list: order is only the tie-break.
    #[test]
    fn the_built_in_default_fills_its_role_ahead_of_list_order() {
        let entry = DesktopEntry::parse(
            b"[Desktop Entry]\nType=Application\nName=Other\nExec=other %f\n\
              MimeType=text/plain;\n",
        )
        .unwrap();
        let other = App::from_entry(&entry, "org.example.Other.desktop", None).unwrap();
        let mut programs = vec![other];
        programs.extend(built_in(None));
        assert_eq!(
            Role::TextEditor
                .filled_by(&programs)
                .map(|app| app.id.as_str()),
            Some("org.slateos.Editor.desktop")
        );
    }
}
