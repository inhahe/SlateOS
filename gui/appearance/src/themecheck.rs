//! Checking a theme before it is installed or shared: what in its folder
//! this desktop would refuse, ignore or adjust, said to its author.
//!
//! `roadmap-detailed.md` §4.6 (*Theme Repository*) asks that a theme offered
//! to others pass automated checks first -- that its file matches the format,
//! that its pictures are pictures within a size, that it holds nothing that
//! runs, and that its text can be read -- and that the contrast check warn
//! rather than refuse. [`check`] is those checks, run on a theme's folder: by
//! its author, by the `themecheck` program from a terminal or a repository's
//! CI, or by whatever installs a theme, before it copies one in.
//!
//! # The desktop's own judgement
//!
//! Every check asks the code the desktop itself uses. The file is read by
//! [`themes::parse`], an icon is drawn by the renderer that draws it on the
//! desktop, a cursor is read by the cursor reader, a size limit is the
//! reader's own, and contrast is measured by the palette against its own
//! floor. So "passes" means "is used as written", and the checker and the
//! desktop cannot drift into disagreeing: a theme the checker passes and the
//! desktop refuses is a bug in one of the two, not a second opinion.
//!
//! # How much each finding matters
//!
//! - **Error** -- the theme should not be installed or shared as it is: it
//!   cannot be read, sets nothing, holds a program or a script, reaches
//!   outside its folder, or holds a file the desktop refuses.
//! - **Warning** -- something in it is ignored, or used other than as written:
//!   a colour that is not one, a section this desktop does not read, text the
//!   desktop has to darken before it can be read, a file nothing reads -- or
//!   something a theme offered to others should say and this one does not.
//! - **Note** -- what it leaves to the built-in theme, the colours darkened
//!   only under the card style a user may choose, and the like.
//!
//! [`Report::passes`] is "no errors". A repository can ask more of the themes
//! it takes than an installer asks of a user's own: `themecheck --strict`
//! fails on warnings too.
//!
//! # What is not checked
//!
//! How a theme *looks*. The roadmap leaves that to a repository's maintainers,
//! looking at pictures its CI draws; nothing here has an opinion on taste.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use guitk::color::Color;
use guitk::svg::SvgDocument;
use yamldoc::Document;

use crate::cursors::{self, xcursor};
use crate::icons;
use crate::themes::{self, ThemeError, ThemeFile, ThemeMeta};
use crate::{
    Palette, StripStyle, SurfaceStyle, TEXT_CONTRAST_FLOOR, THEME_ROLES, ThemeColors,
    contrast_ratio,
};

/// The largest screenshot a theme may carry.
///
/// Not a limit the desktop enforces -- nothing in it draws a theme's
/// screenshots yet -- but the one a theme browser needs, fetching every
/// theme's pictures to show a page of them. Four megabytes holds a lossless
/// picture of a whole 4K screen with room to spare.
pub const MAX_SCREENSHOT_BYTES: u64 = 4 * 1024 * 1024;

/// The most files and folders a theme's folder is looked through for; past
/// it the rest go unchecked, which is an error. A theme with every icon the
/// desktop asks for and a set of cursors is a few hundred.
pub const MAX_ENTRIES: usize = 10_000;

/// How far down a theme's folder is looked through: a folder this many
/// folders below its top is reported, and not looked inside. The desktop
/// reads one level down -- `icons/`, `cursors/` -- and a folder of
/// screenshots is the same.
pub const MAX_DEPTH: usize = 8;

/// Every axis a theme can cover, by the name `meta.supports` gives it: the
/// sections that carry one and the folders that do.
pub const AXES: [&str; 9] = [
    themes::DARK_SECTION,
    themes::TERMINAL_DARK_SECTION,
    themes::SYNTAX_DARK_SECTION,
    themes::WIDGET_SECTION,
    themes::ANIMATION_SECTION,
    themes::DECORATIONS_SECTION,
    themes::PANEL_SECTION,
    icons::ICONS_DIR,
    cursors::CURSORS_DIR,
];

/// Axes the theme format names that this desktop does not have yet --
/// `roadmap-detailed.md`'s sounds, wallpapers and fonts. A theme listing one
/// is ahead of the desktop, not wrong.
const PLANNED_AXES: [&str; 3] = ["sounds", "wallpapers", "fonts"];

/// The sections the theme file is read for.
const SECTIONS: [&str; 11] = [
    themes::META_SECTION,
    themes::DARK_SECTION,
    themes::LIGHT_SECTION,
    themes::TERMINAL_DARK_SECTION,
    themes::TERMINAL_LIGHT_SECTION,
    themes::SYNTAX_DARK_SECTION,
    themes::SYNTAX_LIGHT_SECTION,
    themes::WIDGET_SECTION,
    themes::ANIMATION_SECTION,
    themes::DECORATIONS_SECTION,
    themes::PANEL_SECTION,
];

/// The four colours text is drawn in, which the palette holds to the text
/// floor: the theme's value is where each starts, not what is drawn.
const INKS: [&str; 4] = ["text", "subtext1", "subtext0", "link"];

/// File name extensions that say a file is a program or a script. A theme
/// holding one is refused whatever its bytes: a script is code whether or not
/// it is marked to run. Compared without regard to case.
const PROGRAM_EXTENSIONS: [&str; 33] = [
    "appimage", "bash", "bat", "cjs", "class", "cmd", "com", "csh", "desktop", "dll", "dylib",
    "exe", "fish", "jar", "js", "ksh", "lua", "mjs", "msi", "php", "pl", "ps1", "py", "pyc", "rb",
    "run", "scr", "service", "sh", "so", "vbs", "wasm", "zsh",
];

/// How a program's file begins, and what such a file is.
const PROGRAM_SIGNATURES: [(&[u8], &str); 9] = [
    (b"\x7fELF", "an ELF program"),
    (b"MZ", "a Windows program"),
    (b"#!", "a script"),
    (b"\0asm", "a WebAssembly program"),
    (b"\xca\xfe\xba\xbe", "a Mach-O or Java program"),
    (b"\xfe\xed\xfa\xce", "a Mach-O program"),
    (b"\xfe\xed\xfa\xcf", "a Mach-O program"),
    (b"\xce\xfa\xed\xfe", "a Mach-O program"),
    (b"\xcf\xfa\xed\xfe", "a Mach-O program"),
];

/// How much of a file is read to see whether it is a program: the longest of
/// [`PROGRAM_SIGNATURES`].
const SIGNATURE_BYTES: u64 = 4;

/// Ways an SVG refers to something outside itself, in lower case. A picture
/// that fetches from the network, or reads a file of the user's, is not
/// self-contained data -- and a theme gallery showing it in a web page would
/// be making the request for it.
const EXTERNAL_REFERENCES: [&str; 15] = [
    "href=\"http",
    "href='http",
    "href=\"//",
    "href='//",
    "href=\"file:",
    "href='file:",
    "src=\"http",
    "src='http",
    "url(http",
    "url('http",
    "url(\"http",
    "url(//",
    "url('//",
    "url(\"//",
    "@import",
];

/// What a theme's folder may hold for people rather than for the desktop: a
/// file at its top whose name, before any extension, is one of these, in any
/// case, is not reported as one nothing reads.
const DOCUMENTS: [&str; 8] = [
    "AUTHORS",
    "CHANGELOG",
    "COPYING",
    "CREDITS",
    "LICENCE",
    "LICENSE",
    "NOTICE",
    "README",
];

/// The size an icon is drawn at to see that it draws something.
const ICON_TEST_PX: u32 = 48;

/// The size a cursor is read at to see that it can be.
const CURSOR_TEST_PX: u32 = 24;

// ============================================================================
// The report
// ============================================================================

/// How much a finding matters; ordered from least to most.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// For the author's information.
    Note,
    /// Something is ignored or adjusted: used, but not as written.
    Warning,
    /// The theme should not be installed or shared as it is.
    Error,
}

impl Severity {
    /// The word a report prints before a finding.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One thing the checker has to say about a theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// How much it matters.
    pub severity: Severity,
    /// Where, as a path inside the theme's folder, `/`-separated --
    /// `icons/folder.svg`, `theme.yaml` -- or empty for the folder as a
    /// whole. A finding about one key of the theme file is placed at the file
    /// and names the key.
    pub place: String,
    /// What it is, in a sentence for the theme's author.
    pub message: String,
}

impl fmt::Display for Finding {
    /// `warning: theme.yaml: ...`, or `error: ...` for the folder itself.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.place.is_empty() {
            write!(f, "{}: {}", self.severity.label(), self.message)
        } else {
            write!(
                f,
                "{}: {}: {}",
                self.severity.label(),
                self.place,
                self.message
            )
        }
    }
}

/// What checking a theme found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Every finding: the errors first, then the warnings, then the notes,
    /// each in the order they were found.
    pub findings: Vec<Finding>,
    /// The axes the theme covers -- that it sets something usable for, not
    /// that it claims -- in [`AXES`] order.
    pub covers: Vec<&'static str>,
    /// How many files its folder holds.
    pub files: usize,
    /// How many bytes they come to.
    pub bytes: u64,
}

impl Report {
    /// How many findings are of `severity`.
    #[must_use]
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    }

    /// Whether the theme may be installed or shared as it is: nothing found
    /// is an error.
    #[must_use]
    pub fn passes(&self) -> bool {
        self.count(Severity::Error) == 0
    }
}

/// Check the theme in the folder `dir`.
///
/// Never fails: what cannot be read is itself a finding. Reads the folder and
/// nothing outside it -- a link leading out is reported and not followed.
#[must_use]
pub fn check(dir: &Path) -> Report {
    let mut checker = Checker::new(dir);
    checker.run();
    checker.finish()
}

// ============================================================================
// The checker
// ============================================================================

/// What a folder entry is, as the walk found it -- not following links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Link,
}

/// One file, folder or link inside the theme's folder.
#[derive(Clone, Debug)]
struct Entry {
    /// Its path inside the folder.
    rel: PathBuf,
    kind: Kind,
}

struct Checker {
    root: PathBuf,
    /// The folder as the system resolves it, every link on the way followed:
    /// what a link must end inside. `None` when it could not be resolved.
    real_root: Option<PathBuf>,
    findings: Vec<Finding>,
    /// Everything in the folder, in path order.
    entries: Vec<Entry>,
    /// Each link that stays inside the folder, and the path it leads to.
    links: BTreeMap<PathBuf, PathBuf>,
    /// Files whose presence is explained: something reads them, a person
    /// does, or they have been reported already.
    accounted: BTreeSet<PathBuf>,
    /// The axes the theme sets something usable for.
    covers: BTreeSet<&'static str>,
    files: usize,
    bytes: u64,
    /// The most entries the walk looks at: [`MAX_ENTRIES`], or fewer in a
    /// test, which can then reach the bound without making ten thousand
    /// files.
    max_entries: usize,
}

impl Checker {
    fn new(dir: &Path) -> Self {
        Self {
            root: dir.to_path_buf(),
            real_root: None,
            findings: Vec::new(),
            entries: Vec::new(),
            links: BTreeMap::new(),
            accounted: BTreeSet::new(),
            covers: BTreeSet::new(),
            files: 0,
            bytes: 0,
            max_entries: MAX_ENTRIES,
        }
    }

    fn push(&mut self, severity: Severity, place: &str, message: impl Into<String>) {
        self.findings.push(Finding {
            severity,
            place: place.to_owned(),
            message: message.into(),
        });
    }

    fn error(&mut self, place: &str, message: impl Into<String>) {
        self.push(Severity::Error, place, message);
    }

    fn warning(&mut self, place: &str, message: impl Into<String>) {
        self.push(Severity::Warning, place, message);
    }

    fn note(&mut self, place: &str, message: impl Into<String>) {
        self.push(Severity::Note, place, message);
    }

    fn run(&mut self) {
        match fs::metadata(&self.root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                self.error(
                    "",
                    "is not a folder: a theme is a folder holding a `theme.yaml`, `icons/` or `cursors/`",
                );
                return;
            }
            Err(err) => {
                self.error("", format!("could not be read ({err})"));
                return;
            }
        }
        self.real_root = fs::canonicalize(&self.root).ok();
        self.check_name();
        self.walk();

        let has_file = self.is_entry(Path::new(themes::FILE_NAME), &[Kind::File, Kind::Link]);
        let has_icons = self.is_entry(Path::new(icons::ICONS_DIR), &[Kind::Dir]);
        let has_cursors = self.is_entry(Path::new(cursors::CURSORS_DIR), &[Kind::Dir]);
        if !has_file && !has_icons && !has_cursors {
            self.error(
                "",
                "holds no `theme.yaml`, no `icons/` and no `cursors/`: it is not a theme",
            );
            return;
        }

        self.check_programs();
        let file = if has_file {
            self.check_theme_file()
        } else {
            None
        };
        if has_icons {
            self.check_icons();
        }
        if has_cursors {
            self.check_cursors();
        }
        if let Some(file) = &file {
            self.check_screenshots(&file.meta);
            self.check_contrast(&file.colors);
            self.check_meta(&file.meta);
        }
        self.check_unread();
        if self.covers.is_empty() {
            self.error(
                "",
                "sets nothing this desktop can use: no colours, no section it reads, no icon it can draw and no cursor it can read",
            );
        } else if let Some(file) = &file {
            self.check_supports(&file.meta);
        }
    }

    fn finish(mut self) -> Report {
        // Most severe first; stable, so each severity keeps the order its
        // findings were made in.
        self.findings
            .sort_by_key(|finding| std::cmp::Reverse(finding.severity));
        Report {
            covers: AXES
                .iter()
                .copied()
                .filter(|axis| self.covers.contains(axis))
                .collect(),
            findings: self.findings,
            files: self.files,
            bytes: self.bytes,
        }
    }

    /// Whether the folder holds `rel` as one of `kinds`.
    fn is_entry(&self, rel: &Path, kinds: &[Kind]) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.rel == rel && kinds.contains(&entry.kind))
    }

    /// Whether the file at `rel` may be read: a file in the folder, or a link
    /// that the system resolves to one inside it. A link leading out, or to
    /// nothing, has been reported, and is not followed.
    fn readable(&self, rel: &Path) -> bool {
        self.is_entry(rel, &[Kind::File]) || self.links.contains_key(rel)
    }

    // ------------------------------------------------------------------------
    // The folder
    // ------------------------------------------------------------------------

    /// The folder's own name: what the theme is chosen by.
    fn check_name(&mut self) {
        let named = self.real_root.clone().unwrap_or_else(|| self.root.clone());
        let Some(name) = named.file_name() else {
            self.error("", "has no name of its own to be installed under");
            return;
        };
        if !themes::is_valid_id(name) {
            let shown = pathcodec::display_os(name);
            self.error(
                "",
                format!("is named `{shown}`, which cannot name a theme: it would be a path, not a folder"),
            );
        } else if name == OsStr::new(themes::BUILT_IN) {
            self.note(
                "",
                format!(
                    "is named `{}`, the built-in theme's name: the system's copy of it is read only for its description, and a user's is not listed at all",
                    themes::BUILT_IN
                ),
            );
        }
    }

    /// List everything in the folder, not following links, reporting what a
    /// theme cannot hold as it goes.
    fn walk(&mut self) {
        let mut pending: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];
        let mut seen = 0_usize;
        while let Some((dir, depth)) = pending.pop() {
            let listing = match fs::read_dir(self.root.join(&dir)) {
                Ok(listing) => listing,
                Err(err) => {
                    self.error(&place(&dir), format!("could not be listed ({err})"));
                    continue;
                }
            };
            let mut names: Vec<OsString> = Vec::new();
            for item in listing {
                match item {
                    Ok(item) => names.push(item.file_name()),
                    Err(err) => self.error(&place(&dir), format!("could not be listed ({err})")),
                }
            }
            names.sort();
            for name in names {
                seen = seen.saturating_add(1);
                if seen > self.max_entries {
                    let max = self.max_entries;
                    self.error(
                        "",
                        format!(
                            "holds more than {max} files and folders; the rest were not checked"
                        ),
                    );
                    self.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
                    return;
                }
                self.visit(dir.join(&name), depth, &mut pending);
            }
        }
        self.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    }

    /// One entry of the folder, `depth` folders below its top.
    fn visit(&mut self, rel: PathBuf, depth: usize, pending: &mut Vec<(PathBuf, usize)>) {
        let at = place(&rel);
        let meta = match fs::symlink_metadata(self.root.join(&rel)) {
            Ok(meta) => meta,
            Err(err) => {
                self.error(&at, format!("could not be read ({err})"));
                return;
            }
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            if let Some(target) = self.check_link(&rel) {
                self.links.insert(rel.clone(), target);
            }
            self.entries.push(Entry {
                rel,
                kind: Kind::Link,
            });
        } else if kind.is_dir() {
            if is_hidden(&rel) {
                self.warning(
                    &at,
                    "is a hidden folder, which no part of a theme is: leave it out when the theme is installed or shared",
                );
                return;
            }
            // `depth` folders are above this one, so it is the one `below`
            // folders down.
            let below = depth.saturating_add(1);
            if below >= MAX_DEPTH {
                self.error(
                    &at,
                    format!("is {MAX_DEPTH} folders down, deeper than anything in a theme, and what is in it was not checked"),
                );
                return;
            }
            pending.push((rel.clone(), below));
            self.entries.push(Entry {
                rel,
                kind: Kind::Dir,
            });
        } else if kind.is_file() {
            self.files = self.files.saturating_add(1);
            self.bytes = self.bytes.saturating_add(meta.len());
            check_mode(self, &at, &meta);
            if is_hidden(&rel) {
                self.warning(
                    &at,
                    "is a hidden file, which no part of a theme is: leave it out when the theme is installed or shared",
                );
                self.accounted.insert(rel.clone());
            }
            self.entries.push(Entry {
                rel,
                kind: Kind::File,
            });
        } else {
            self.error(
                &at,
                "is neither a file nor a folder -- a device, a pipe or a socket -- which nothing in a theme can be",
            );
        }
    }

    /// Where the link at `rel` ends, inside the folder -- or `None`, having
    /// said why, for one written from the root, or ending outside the
    /// folder, at a folder, or nowhere.
    ///
    /// Resolved as the system resolves it, every link on the way followed
    /// (`fs::canonicalize`), and judged by where that ends: a reading of the
    /// link's text alone passes `icons/a.svg -> sub/a.svg` when `icons/sub`
    /// is itself a link out of the folder, and reading the icon would then
    /// read outside it. Only where the path ends is asked; nothing outside
    /// the folder is read. Cursor themes are made of links -- each older name
    /// of a cursor a link to its file -- so a link ending inside is no fault.
    fn check_link(&mut self, rel: &Path) -> Option<PathBuf> {
        let at = place(rel);
        let path = self.root.join(rel);
        let target = match fs::read_link(&path) {
            Ok(target) => target,
            Err(err) => {
                self.error(&at, format!("is a link that could not be read ({err})"));
                return None;
            }
        };
        let shown = pathcodec::display_path(&target);
        let from_root = target.has_root()
            || target
                .components()
                .any(|part| matches!(part, Component::Prefix(_)));
        if from_root {
            self.error(
                &at,
                format!("is a link to `{shown}`, a path from the root: a theme's links must stay inside its folder, written from where they are"),
            );
            return None;
        }
        let Some(root) = self.real_root.clone() else {
            self.error(
                &at,
                format!("is a link to `{shown}`, which could not be followed"),
            );
            return None;
        };
        let Ok(real) = fs::canonicalize(&path) else {
            self.warning(
                &at,
                format!("is a link to `{shown}`, which leads nowhere -- to nothing, or round in a ring of links -- so nothing reads it"),
            );
            self.accounted.insert(rel.to_path_buf());
            return None;
        };
        let Ok(inside) = real.strip_prefix(&root) else {
            self.error(
                &at,
                format!("is a link to `{shown}`, which leads outside the theme's folder: a theme cannot reach outside its folder"),
            );
            return None;
        };
        if real.is_dir() {
            self.error(
                &at,
                format!(
                    "is a link to the folder `{shown}`: a theme's links may lead only to files"
                ),
            );
            return None;
        }
        if !real.is_file() {
            self.error(&at, format!("is a link to `{shown}`, which is not a file"));
            return None;
        }
        Some(inside.to_path_buf())
    }

    /// No file in the folder may be a program or a script, and no SVG in it
    /// may carry one.
    fn check_programs(&mut self) {
        let files: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|entry| entry.kind == Kind::File)
            .map(|entry| entry.rel.clone())
            .collect();
        for rel in files {
            let at = place(&rel);
            if let Some(extension) = program_extension(&rel) {
                self.error(
                    &at,
                    format!("is named as a program or a script (`.{extension}`): a theme is data, never code"),
                );
                continue;
            }
            match read_head(&self.root.join(&rel)) {
                Ok(head) => {
                    if let Some(what) = program_signature(&head) {
                        self.error(&at, format!("is {what}: a theme is data, never code"));
                        continue;
                    }
                }
                Err(err) => {
                    self.error(&at, format!("could not be read ({err})"));
                    continue;
                }
            }
            if has_extension(&rel, "svg") {
                match read_within(&self.root.join(&rel), MAX_SCREENSHOT_BYTES) {
                    Ok(Some(bytes)) => {
                        if let Some(hazard) = svg_hazard(&bytes) {
                            self.error(&at, format!("{hazard}: a theme is data, never code"));
                        }
                    }
                    // Larger than any SVG a theme may use: what it is used as
                    // says so, or it is one nothing reads.
                    Ok(None) => {}
                    Err(err) => self.error(&at, format!("could not be read ({err})")),
                }
            }
        }
    }

    // ------------------------------------------------------------------------
    // The theme file
    // ------------------------------------------------------------------------

    /// Read `theme.yaml` as the desktop does, and report what the desktop
    /// would ignore in it.
    fn check_theme_file(&mut self) -> Option<ThemeFile> {
        let rel = PathBuf::from(themes::FILE_NAME);
        self.accounted.insert(rel.clone());
        let at = themes::FILE_NAME;
        if !self.readable(&rel) {
            return None;
        }
        let bytes = match themes::read_theme_bytes(&self.root.join(&rel)) {
            Ok(bytes) => bytes,
            Err(ThemeError::TooLarge(size)) => {
                self.error(
                    at,
                    format!(
                        "is {size} bytes, over the {} the desktop reads",
                        themes::MAX_FILE_BYTES
                    ),
                );
                return None;
            }
            Err(ThemeError::Unreadable(why)) => {
                self.error(at, format!("could not be read ({why})"));
                return None;
            }
            Err(err) => {
                self.error(at, format!("cannot be used: the theme {err}"));
                return None;
            }
        };
        let Ok(text) = String::from_utf8(bytes) else {
            self.error(at, "is not text (UTF-8), so it is not YAML");
            return None;
        };
        let file = themes::parse(&text);
        for warning in &file.warnings {
            self.warning(at, warning.clone());
        }

        let doc = Document::parse(&text);
        for key in doc.keys(&[]) {
            if !SECTIONS.contains(&key.as_str()) {
                self.warning(
                    at,
                    format!(
                        "`{}` is not a section this desktop reads, and is ignored -- misspelt, or one a later desktop reads",
                        themes::quoted(&key)
                    ),
                );
            }
        }
        for key in doc.keys(&[themes::META_SECTION]) {
            if !themes::META_KEYS.contains(&key.as_str()) {
                self.warning(
                    at,
                    format!(
                        "`{}.{}` is not read, and is ignored: `{}` holds {}",
                        themes::META_SECTION,
                        themes::quoted(&key),
                        themes::META_SECTION,
                        themes::META_KEYS.join(", ")
                    ),
                );
            }
        }

        let colors = &file.colors;
        let sections = [
            (themes::DARK_SECTION, !colors.dark.is_empty()),
            (themes::LIGHT_SECTION, !colors.light.is_empty()),
            (
                themes::TERMINAL_DARK_SECTION,
                !colors.terminal_dark.is_empty(),
            ),
            (
                themes::TERMINAL_LIGHT_SECTION,
                !colors.terminal_light.is_empty(),
            ),
            (themes::SYNTAX_DARK_SECTION, !colors.syntax_dark.is_empty()),
            (
                themes::SYNTAX_LIGHT_SECTION,
                !colors.syntax_light.is_empty(),
            ),
            (themes::WIDGET_SECTION, file.widget_style.is_some()),
            (themes::ANIMATION_SECTION, file.motion.is_some()),
            (themes::DECORATIONS_SECTION, file.decorations.is_some()),
            (themes::PANEL_SECTION, file.panel.is_some()),
        ];
        for (section, sets) in sections {
            if doc.contains(&[section]) && !sets {
                self.warning(
                    at,
                    format!("`{section}` sets nothing this desktop can use, so it is as if it were not there"),
                );
            }
        }

        let axes = [
            (
                themes::DARK_SECTION,
                !colors.dark.is_empty() || !colors.light.is_empty(),
            ),
            (
                themes::TERMINAL_DARK_SECTION,
                !colors.terminal_dark.is_empty() || !colors.terminal_light.is_empty(),
            ),
            (
                themes::SYNTAX_DARK_SECTION,
                !colors.syntax_dark.is_empty() || !colors.syntax_light.is_empty(),
            ),
            (themes::WIDGET_SECTION, file.widget_style.is_some()),
            (themes::ANIMATION_SECTION, file.motion.is_some()),
            (themes::DECORATIONS_SECTION, file.decorations.is_some()),
            (themes::PANEL_SECTION, file.panel.is_some()),
        ];
        for (axis, covered) in axes {
            if covered {
                self.covers.insert(axis);
            }
        }
        Some(file)
    }

    /// What a theme offered to others should say about itself.
    fn check_meta(&mut self, meta: &ThemeMeta) {
        let at = themes::FILE_NAME;
        let section = themes::META_SECTION;
        if meta.name.is_none() {
            let shown = self
                .real_root
                .as_deref()
                .and_then(Path::file_name)
                .map(pathcodec::display_os)
                .unwrap_or_default();
            self.note(
                at,
                format!("no `{section}.name`: the theme is shown by its folder's name, `{shown}`"),
            );
        }
        if meta.author.is_none() {
            self.warning(
                at,
                format!("no `{section}.author`: a theme offered to others should say who made it"),
            );
        }
        if meta.version.is_none() {
            self.warning(
                at,
                format!("no `{section}.version`: a theme offered to others needs one for its updates to be told apart"),
            );
        }
        if meta.license.is_none() {
            self.warning(
                at,
                format!(
                    "no `{section}.license`: a theme offered to others should say on what terms"
                ),
            );
        }
        if meta.screenshots.is_empty() {
            self.note(
                at,
                format!(
                    "no `{section}.screenshots`: a theme browser will have no picture of it to show"
                ),
            );
        }
        if meta.tags.is_empty() {
            self.note(
                at,
                format!(
                    "no `{section}.tags`: nothing to find it by in a theme browser but its name"
                ),
            );
        }
    }

    /// `meta.supports` against what the theme covers.
    fn check_supports(&mut self, meta: &ThemeMeta) {
        let at = themes::FILE_NAME;
        let covered: Vec<&'static str> = AXES
            .iter()
            .copied()
            .filter(|axis| self.covers.contains(axis))
            .collect();
        if meta.supports.is_empty() {
            self.warning(
                at,
                format!(
                    "no `{}.supports`: a theme browser cannot say what it covers -- {}",
                    themes::META_SECTION,
                    covered.join(", ")
                ),
            );
            return;
        }
        for claim in &meta.supports {
            if let Some(axis) = AXES.iter().find(|axis| **axis == claim.as_str()) {
                if !self.covers.contains(axis) {
                    self.warning(
                        at,
                        format!("`meta.supports` lists `{axis}`, but the theme sets no {axis} this desktop can use"),
                    );
                }
            } else if PLANNED_AXES.contains(&claim.as_str()) {
                self.note(
                    at,
                    format!("`meta.supports` lists `{claim}`, which this desktop has no axis for yet: nothing reads it"),
                );
            } else {
                self.warning(
                    at,
                    format!(
                        "`meta.supports` lists `{}`, which is not an axis: they are {}",
                        themes::quoted(claim),
                        AXES.join(", ")
                    ),
                );
            }
        }
        for axis in covered {
            if !meta.supports.iter().any(|claim| claim == axis) {
                self.warning(
                    at,
                    format!("sets `{axis}`, which `meta.supports` does not list, so a theme browser will not say so"),
                );
            }
        }
    }

    /// Each screenshot `meta` names: inside the folder, there, a picture,
    /// and within [`MAX_SCREENSHOT_BYTES`].
    fn check_screenshots(&mut self, meta: &ThemeMeta) {
        let at = themes::FILE_NAME;
        for name in &meta.screenshots {
            let shown = themes::quoted(name);
            let Some(path) = themes::confined(&self.root, name) else {
                self.error(
                    at,
                    format!("screenshot `{shown}` is outside the theme's folder: a theme cannot reach outside its folder"),
                );
                continue;
            };
            let rel: PathBuf = path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .components()
                .filter(|part| matches!(part, Component::Normal(_)))
                .collect();
            self.accounted.insert(rel.clone());
            if !self.readable(&rel) {
                // A link that leads out has been reported where it is.
                if self.is_entry(&rel, &[Kind::Dir]) {
                    self.warning(
                        at,
                        format!("screenshot `{shown}` is a folder, not a picture"),
                    );
                } else if !self.is_entry(&rel, &[Kind::Link]) {
                    self.warning(
                        at,
                        format!("screenshot `{shown}` is not in the theme's folder"),
                    );
                }
                continue;
            }
            let shot = place(&rel);
            let bytes = match read_within(&path, MAX_SCREENSHOT_BYTES) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    self.error(
                        &shot,
                        format!(
                            "is larger than the {MAX_SCREENSHOT_BYTES} bytes a screenshot may be"
                        ),
                    );
                    continue;
                }
                Err(err) => {
                    self.error(&shot, format!("could not be read ({err})"));
                    continue;
                }
            };
            if has_extension(&rel, "svg") {
                let parsed = String::from_utf8(bytes)
                    .map_err(|_| "it is not text".to_owned())
                    .and_then(|text| {
                        SvgDocument::parse(&text)
                            .map(|_| ())
                            .map_err(|err| err.to_string())
                    });
                if let Err(why) = parsed {
                    self.error(
                        &shot,
                        format!("is not a picture this desktop can show ({why})"),
                    );
                }
            } else if let Err(err) = imagecodec::decode(&bytes, imagecodec::Limits::default()) {
                self.error(
                    &shot,
                    format!("is not a picture this desktop can show ({err})"),
                );
            }
        }
    }

    // ------------------------------------------------------------------------
    // Icons and cursors
    // ------------------------------------------------------------------------

    /// Each icon, drawn as the desktop draws it.
    fn check_icons(&mut self) {
        let dir = Path::new(icons::ICONS_DIR);
        let inside: Vec<Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.rel.starts_with(dir) && entry.rel != dir)
            .cloned()
            .collect();
        let mut drawn = 0_usize;
        for entry in inside {
            self.accounted.insert(entry.rel.clone());
            // Inside a folder inside `icons/`: reported with that folder.
            if entry.rel.parent() != Some(dir) {
                continue;
            }
            if entry.kind == Kind::Dir {
                self.warning(
                    &place(&entry.rel),
                    format!("is a folder inside `{}/`, which the desktop does not look in: an icon is `{}/<name>.svg`", icons::ICONS_DIR, icons::ICONS_DIR),
                );
            } else if self.check_icon(&entry.rel) {
                drawn = drawn.saturating_add(1);
            }
        }
        if drawn > 0 {
            self.covers.insert(icons::ICONS_DIR);
        }
    }

    /// One icon: an SVG under an icon's name, within the size the desktop
    /// reads, that draws something. Whether it does.
    fn check_icon(&mut self, rel: &Path) -> bool {
        let at = place(rel);
        let stem = rel
            .file_stem()
            .and_then(OsStr::to_str)
            .filter(|_| rel.extension() == Some(OsStr::new("svg")));
        let Some(stem) = stem else {
            self.warning(
                &at,
                format!("is not read: the desktop reads `<name>.svg` files from `{}/`, SVG and named in lower case, and this is not one", icons::ICONS_DIR),
            );
            return false;
        };
        if !icons::is_valid_name(stem) {
            self.warning(
                &at,
                "is not named as an icon is asked for -- lower-case letters, digits, `-` and `_`, as freedesktop's icon names are -- so nothing asks for it",
            );
            return false;
        }
        if !self.readable(rel) {
            return false;
        }
        let text = match read_within(&self.root.join(rel), icons::MAX_ICON_BYTES) {
            Ok(Some(bytes)) => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    self.error(&at, "is not text, so it is not an SVG");
                    return false;
                }
            },
            Ok(None) => {
                self.error(
                    &at,
                    format!(
                        "is larger than the {} bytes an icon may be: the desktop passes it over",
                        icons::MAX_ICON_BYTES
                    ),
                );
                return false;
            }
            Err(err) => {
                self.error(&at, format!("could not be read ({err})"));
                return false;
            }
        };
        if let Err(err) = SvgDocument::parse(&text) {
            self.error(&at, format!("is not an SVG this desktop can draw ({err})"));
            return false;
        }
        if icons::render_svg(&text, ICON_TEST_PX, Color::rgb(0, 0, 0)).is_none() {
            self.error(&at, "draws nothing the desktop can see");
            return false;
        }
        true
    }

    /// Each cursor, read as the desktop reads it.
    fn check_cursors(&mut self) {
        let dir = Path::new(cursors::CURSORS_DIR);
        // A cursor theme's `index.theme` names the themes it inherits from.
        let index = PathBuf::from(cursors::INDEX_FILE);
        if self.is_entry(&index, &[Kind::File, Kind::Link]) {
            self.accounted.insert(index);
        }
        let inside: Vec<Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.rel.starts_with(dir) && entry.rel != dir)
            .cloned()
            .collect();
        let mut read = 0_usize;
        for entry in inside {
            self.accounted.insert(entry.rel.clone());
            if entry.rel.parent() != Some(dir) {
                continue;
            }
            if entry.kind == Kind::Dir {
                self.warning(
                    &place(&entry.rel),
                    format!(
                        "is a folder inside `{}/`, which the desktop does not look in",
                        cursors::CURSORS_DIR
                    ),
                );
            } else if self.check_cursor(&entry.rel) {
                read = read.saturating_add(1);
            }
        }
        if read > 0 {
            self.covers.insert(cursors::CURSORS_DIR);
        }
    }

    /// One cursor: an XCursor file under a cursor's name, within the size the
    /// desktop reads. Whether it is.
    fn check_cursor(&mut self, rel: &Path) -> bool {
        let at = place(rel);
        let name = rel.file_name().and_then(OsStr::to_str);
        if !name.is_some_and(cursors::is_valid_name) {
            self.warning(
                &at,
                "is not named as a cursor is asked for -- letters, digits, `-` and `_` -- so nothing asks for it",
            );
            return false;
        }
        if !self.readable(rel) {
            return false;
        }
        match read_within(&self.root.join(rel), cursors::MAX_CURSOR_BYTES) {
            Ok(Some(bytes)) => {
                if xcursor::read(&bytes, CURSOR_TEST_PX).is_none() {
                    self.error(&at, "is not an XCursor file the desktop can read");
                    return false;
                }
                true
            }
            Ok(None) => {
                self.error(
                    &at,
                    format!(
                        "is larger than the {} bytes a cursor may be: the desktop passes it over",
                        cursors::MAX_CURSOR_BYTES
                    ),
                );
                false
            }
            Err(err) => {
                self.error(&at, format!("could not be read ({err})"));
                false
            }
        }
    }

    // ------------------------------------------------------------------------
    // Contrast
    // ------------------------------------------------------------------------

    /// Each mode's text against the grounds it is drawn on, as the palette
    /// built from the theme draws them.
    fn check_contrast(&mut self, colors: &ThemeColors) {
        let at = themes::FILE_NAME;
        for light in [false, true] {
            let (section, terminal, syntax, other) = if light {
                (
                    themes::LIGHT_SECTION,
                    themes::TERMINAL_LIGHT_SECTION,
                    themes::SYNTAX_LIGHT_SECTION,
                    themes::DARK_SECTION,
                )
            } else {
                (
                    themes::DARK_SECTION,
                    themes::TERMINAL_DARK_SECTION,
                    themes::SYNTAX_DARK_SECTION,
                    themes::LIGHT_SECTION,
                )
            };
            let own = colors.roles(light);
            let terminal_set = colors.terminal_roles(light);
            let syntax_set = colors.syntax_roles(light);
            if colors.variant(light).0 != light {
                // This mode is drawn in the other's colours -- and, with them,
                // in the other's terminal and code colours.
                for (name, set) in [(terminal, terminal_set), (syntax, syntax_set)] {
                    if !set.is_empty() {
                        self.warning(
                            at,
                            format!("`{name}` is never used: with no `{section}`, the theme is drawn in its `{other}` in both modes, with that mode's terminal and code colours"),
                        );
                    }
                }
                continue;
            }
            if own.is_empty() && terminal_set.is_empty() && syntax_set.is_empty() {
                continue;
            }

            // The grounds every desktop draws text on -- its styles as they
            // come -- and those only the card style adds: its raised
            // surfaces, which a user chooses.
            let mut palette = Palette::for_theme(light, colors);
            palette.set_surface_style(SurfaceStyle::default());
            palette.set_strip_style(StripStyle::default());
            let everyone = text_grounds(&palette);
            let mut carded = palette;
            carded.set_surface_style(SurfaceStyle::Cards);
            carded.set_strip_style(StripStyle::Filled);
            let cards: Vec<(&'static str, Color)> = text_grounds(&carded)
                .into_iter()
                .filter(|ground| !everyone.contains(ground))
                .collect();

            let keys = INKS
                .iter()
                .filter_map(|ink| {
                    own.get(*ink)
                        .map(|&color| (format!("{section}.{ink}"), color))
                })
                .chain(
                    syntax_set
                        .iter()
                        .map(|(kind, &color)| (format!("{syntax}.{kind}"), color)),
                );
            let mut on_cards: Vec<String> = Vec::new();
            for (key, color) in keys {
                if let Some(darkened) = self.check_ink(&key, color, &palette, &everyone, &cards) {
                    on_cards.push(darkened);
                }
            }
            if !on_cards.is_empty() {
                self.note(
                    at,
                    format!(
                        "under the card style, a user's choice, the desktop darkens {} of this mode's colours to be read on its raised surfaces: {}",
                        on_cards.len(),
                        on_cards.join(", ")
                    ),
                );
            }
            let foreground = terminal_set.get("foreground").copied();
            let background = terminal_set.get("background").copied();
            if foreground.is_some() || background.is_some() {
                let foreground = foreground.unwrap_or(palette.text);
                let background = background.unwrap_or(palette.base);
                let ratio = contrast_ratio(foreground, background);
                if ratio < TEXT_CONTRAST_FLOOR {
                    let drawn = crate::legible_on(foreground, background);
                    self.warning(
                        at,
                        format!(
                            "the terminal's text, {} on {}, is {ratio:.2}:1 -- under the {TEXT_CONTRAST_FLOOR}:1 text needs to be read, so `{terminal}.foreground` is drawn as {} instead",
                            foreground.hex_text(),
                            background.hex_text(),
                            drawn.hex_text()
                        ),
                    );
                }
            }

            let left: Vec<&str> = THEME_ROLES
                .iter()
                .copied()
                .filter(|role| !own.contains_key(*role))
                .collect();
            if !own.is_empty() && !left.is_empty() {
                self.note(
                    at,
                    format!(
                        "`{section}` sets {} of the {} colours; the rest are the built-in theme's: {}",
                        own.len(),
                        THEME_ROLES.len(),
                        left.join(", ")
                    ),
                );
            }
        }
    }

    /// `key`'s colour against the grounds text is drawn on. Under the floor
    /// on one `everyone` draws text on: a warning naming the worst, and what
    /// `palette` -- those grounds' -- draws instead. Otherwise under it on one
    /// of the card style's: that ground and the ratio, for the mode's note.
    fn check_ink(
        &mut self,
        key: &str,
        color: Color,
        palette: &Palette,
        everyone: &[(&'static str, Color)],
        cards: &[(&'static str, Color)],
    ) -> Option<String> {
        if let Some((name, ground, ratio)) = under_the_floor(color, everyone) {
            self.warning(
                themes::FILE_NAME,
                format!(
                    "`{key}`, {}, is {ratio:.2}:1 on `{name}` ({}) -- under the {TEXT_CONTRAST_FLOOR}:1 text needs to be read, so the desktop draws it as {} instead",
                    color.hex_text(),
                    ground.hex_text(),
                    palette.ink(color).hex_text()
                ),
            );
            return None;
        }
        under_the_floor(color, cards)
            .map(|(name, _, ratio)| format!("`{key}` ({ratio:.2}:1 on `{name}`)"))
    }

    // ------------------------------------------------------------------------
    // What nothing reads
    // ------------------------------------------------------------------------

    /// Each file nothing reads -- not the desktop, and not a person.
    fn check_unread(&mut self) {
        // A link something reads leads to a file that is read too -- the
        // file it ends at, every link on the way followed, so a chain of
        // links counts its last file and needs no second round.
        let ends: Vec<PathBuf> = self
            .links
            .iter()
            .filter(|(link, _)| self.accounted.contains(*link))
            .map(|(_, end)| end.clone())
            .collect();
        self.accounted.extend(ends);
        let unread: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|entry| entry.kind != Kind::Dir)
            .filter(|entry| !self.accounted.contains(&entry.rel) && !is_document(&entry.rel))
            .map(|entry| entry.rel.clone())
            .collect();
        for rel in unread {
            self.warning(
                &place(&rel),
                "is a file nothing reads: not the theme file, a screenshot it names, an icon or a cursor",
            );
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// `rel` as a finding places it: `/`-separated, each name shown as
/// `design-decisions.md` §426 shows one that is not text.
fn place(rel: &Path) -> String {
    rel.iter()
        .map(pathcodec::display_os)
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether the last part of `rel` is hidden: begins with a dot.
fn is_hidden(rel: &Path) -> bool {
    rel.file_name()
        .is_some_and(|name| name.as_encoded_bytes().first() == Some(&b'.'))
}

/// Whether `rel` is a document for people at the top of the folder.
fn is_document(rel: &Path) -> bool {
    if rel.parent() != Some(Path::new("")) {
        return false;
    }
    rel.file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(|stem| DOCUMENTS.iter().any(|doc| stem.eq_ignore_ascii_case(doc)))
}

/// Whether `rel`'s extension is `extension`, in any case.
fn has_extension(rel: &Path, extension: &str) -> bool {
    rel.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|own| own.eq_ignore_ascii_case(extension))
}

/// `rel`'s extension, lower-cased, when it names a program or a script.
fn program_extension(rel: &Path) -> Option<String> {
    let extension = rel.extension()?.to_str()?.to_ascii_lowercase();
    PROGRAM_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

/// What kind of program a file beginning `head` is, if it is one.
fn program_signature(head: &[u8]) -> Option<&'static str> {
    PROGRAM_SIGNATURES
        .iter()
        .find(|(signature, _)| head.starts_with(signature))
        .map(|(_, what)| *what)
}

/// The first [`SIGNATURE_BYTES`] of the file at `path`, or fewer if it is
/// shorter.
fn read_head(path: &Path) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    fs::File::open(path)?
        .take(SIGNATURE_BYTES)
        .read_to_end(&mut head)?;
    Ok(head)
}

/// The bytes of the file at `path`, or `None` when it holds more than
/// `limit` -- found by reading one byte past it, so a file that grows while
/// it is read is still held to the limit.
fn read_within(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    Ok((read <= limit).then_some(bytes))
}

/// Whether an SVG's bytes carry something that runs or reaches outside the
/// picture, and if so, what -- as the start of a sentence.
fn svg_hazard(svg: &[u8]) -> Option<String> {
    let lower = svg.to_ascii_lowercase();
    if contains(&lower, b"<script") {
        return Some("holds a script (`<script>`)".to_owned());
    }
    if contains(&lower, b"<foreignobject") {
        return Some(
            "holds a `<foreignObject>`, which can carry a web page and its scripts".to_owned(),
        );
    }
    if contains(&lower, b"javascript:") {
        return Some("holds a `javascript:` link".to_owned());
    }
    if let Some(attribute) = event_attribute(&lower) {
        return Some(format!(
            "has an `{attribute}` attribute, which runs a script"
        ));
    }
    EXTERNAL_REFERENCES
        .iter()
        .find(|reference| contains(&lower, reference.as_bytes()))
        .map(|reference| format!("refers to something outside itself (`{reference}`)"))
}

/// Whether `needle` occurs in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The first event-handler attribute (`onload=`, `onclick=`, ...) inside a
/// tag of the lower-cased markup `lower`, if there is one. Quoted values are
/// skipped, so an attribute *value* mentioning one is not taken for one.
fn event_attribute(lower: &[u8]) -> Option<String> {
    let mut in_tag = false;
    let mut quote: Option<u8> = None;
    for (at, &byte) in lower.iter().enumerate() {
        if let Some(open) = quote {
            if byte == open {
                quote = None;
            }
            continue;
        }
        if !in_tag {
            in_tag = byte == b'<';
            continue;
        }
        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'>' => in_tag = false,
            _ if byte.is_ascii_whitespace() => {
                let rest = lower.get(at.saturating_add(1)..).unwrap_or_default();
                let length = rest
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(**c, b'-' | b'_' | b':'))
                    .count();
                let name = rest.get(..length).unwrap_or_default();
                let after = rest
                    .get(length..)
                    .unwrap_or_default()
                    .iter()
                    .find(|c| !c.is_ascii_whitespace());
                let handler = name.len() > 2
                    && name.starts_with(b"on")
                    && name
                        .get(2..)
                        .is_some_and(|tail| tail.iter().all(u8::is_ascii_alphabetic));
                if handler && after == Some(&b'=') {
                    return Some(name.iter().map(|&c| char::from(c)).collect());
                }
            }
            _ => {}
        }
    }
    None
}

/// The grounds text is drawn on in `palette`, by role, under the style
/// settings it was given -- in the order [`Palette::text_grounds`] gives them,
/// which a test holds this to.
fn text_grounds(palette: &Palette) -> Vec<(&'static str, Color)> {
    [
        ("base", palette.base),
        ("mantle", palette.mantle),
        ("surface0", palette.surface0),
        ("surface1", palette.surface1),
        ("crust", palette.crust),
    ]
    .into_iter()
    .filter(|(_, ground)| palette.text_grounds().iter().flatten().any(|g| g == ground))
    .collect()
}

/// The ground of `grounds` that `color` is hardest to read on, with its name
/// and the ratio -- when that is under the text floor.
fn under_the_floor(
    color: Color,
    grounds: &[(&'static str, Color)],
) -> Option<(&'static str, Color, f32)> {
    grounds
        .iter()
        .map(|&(name, ground)| (name, ground, contrast_ratio(color, ground)))
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .filter(|&(_, _, ratio)| ratio < TEXT_CONTRAST_FLOOR)
}

/// A file marked to run is reported: nothing in a theme is run, and a mark
/// that says otherwise invites something to.
#[cfg(unix)]
fn check_mode(checker: &mut Checker, at: &str, meta: &fs::Metadata) {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o111 != 0 {
        checker.warning(
            at,
            "is marked as a program that may be run, and nothing in a theme is: clear the mark (`chmod -x`)",
        );
    }
}

/// Elsewhere a file has no mark that says it may be run.
#[cfg(not(unix))]
fn check_mode(_checker: &mut Checker, _at: &str, _meta: &fs::Metadata) {}

#[cfg(test)]
#[path = "themecheck_tests.rs"]
mod tests;
