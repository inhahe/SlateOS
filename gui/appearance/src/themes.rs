//! Themes: the colour sets a user can choose between, read from files anyone
//! can write.
//!
//! `design.txt` asks for "desktop theme - light, dark, anything else" and
//! "individual desktop colors? (make your own theme)" (lines 1249-1250), and
//! `roadmap-detailed.md` → *Themes and Appearance* sets out the format: one
//! declarative YAML file per theme, a `meta` block describing it, and one
//! section per *axis* -- colours, window decorations, icons, cursors and so on
//! -- each of which a user may take from a different theme. This module is the
//! colours axis, together with the parts every axis will share: where themes
//! are installed, how one is named, and how its file is read.
//!
//! # A theme on disk
//!
//! A directory named for the theme, holding `theme.yaml` and whatever the file
//! refers to (screenshots, so far), and its icons in an `icons` folder beside
//! it -- the icons axis, [`crate::icons`]. A folder with icons and no
//! `theme.yaml` is an icon pack, a theme for that axis alone:
//!
//! ```text
//! /usr/share/slateos/themes/<name>/theme.yaml          installed with the system
//! ~/.local/share/slateos/themes/<name>/theme.yaml      installed by this user
//! ```
//!
//! The user's copy wins where both have a theme of that name, so a system theme
//! is adjusted by copying it. The *data* directory and not the configuration
//! one, because a theme is a thing the user has -- like a font, which lives in
//! `~/.local/share/fonts` for the same reason -- rather than a preference they
//! set. The preference, *which* theme, is one line of `appearance.yaml`:
//! `theme.colors`.
//!
//! # The file
//!
//! ```yaml
//! meta:
//!   name: Nord
//!   author: Someone
//!   version: "1.2"
//!   license: MIT
//!   tags: [cool, blue]
//!   screenshots: [dark.png, light.png]
//!   supports: [colors]
//! colors:              # dark mode
//!   base: "#2e3440"
//!   text: "#eceff4"
//! colors-light:        # light mode
//!   base: "#eceff4"
//!   text: "#2e3440"
//! ```
//!
//! The colour names are the palette's roles, [`THEME_ROLES`] -- the names every
//! application already reads its colours by. So a theme redefines exactly the
//! tokens everything is drawn from, and there is no second vocabulary to map
//! onto them and fall out of step with. A role a theme leaves out keeps its
//! built-in value, which makes a theme that changes only the page and the text
//! a whole theme. `gui/appearance/themes/aero/theme.yaml` is the built-in
//! theme written out in this format with every role present: the template to
//! copy.
//!
//! A theme with only one of the two sections is shown in that mode whichever
//! the user has chosen ([`ThemeColors::variant`]): its colours were chosen
//! against one set of grounds, and mixing them into the other mode's would be
//! neither.
//!
//! # What a theme cannot do
//!
//! - **Choose the accent.** That is the user's own choice, made in Settings; a
//!   theme's `accent` is reported and ignored.
//! - **Make text unreadable.** The text inks are held to the same contrast floor
//!   as the built-in ones ([`Palette::for_theme`](crate::Palette::for_theme)): a
//!   theme chooses where the text starts, not whether it can be read.
//! - **Override high contrast.** A high-contrast scheme is chosen for need,
//!   not taste, and replaces the palette whole -- theme and all.
//! - **Run code.** A theme is data (`roadmap-detailed.md`: "Themes are pure
//!   data -- never executable code").
//! - **Reach outside its directory.** Its name must be one path component, and
//!   a screenshot that names a path elsewhere is dropped.
//! - **Cost more than a page of reading.** A file over [`MAX_FILE_BYTES`] is
//!   refused unread.
//!
//! # When it is read
//!
//! When `appearance.yaml` is: [`AppearanceSettings::read_from`] loads the
//! chosen theme into a [`ColorTheme`], so every reader of the settings -- the
//! shell, the compositor, and every application through `oswindow` -- gets the
//! theme's colours with them, and none has a second step to forget. Never per
//! frame: `Palette::from_settings` runs per frame and must not touch a file,
//! which is why the colours travel already parsed.
//!
//! What is *not* noticed is a change to the theme's own file while the same
//! theme stays chosen: the settings watchers watch `appearance.yaml`. See
//! `known-issues.md` `TD-C-AN-EDITED-THEME-FILE-IS-NOT-NOTICED-UNTIL-THE-SETTINGS-CHANGE`.
//!
//! [`AppearanceSettings::read_from`]: crate::AppearanceSettings::read_from

use crate::color_from_hex;
use guitk::color::Color;
use guitk::palette::{THEME_ROLES, ThemeColors};
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use yamldoc::Document;

/// The built-in theme's name: what `theme.colors` holds when the user has
/// chosen nothing else.
///
/// Its colours are compiled in -- they are the palette's own constants -- so it
/// needs no file and cannot fail to load. `aero` because the roadmap names the
/// default theme after the demo the desktop's look follows (`Aero Desktop
/// (offline).html`, `design-decisions.md` §815), and a theme needs a name a
/// user can pick it back by.
pub const BUILT_IN: &str = "aero";

/// The built-in theme's name as shown, when no installed file says otherwise.
pub const BUILT_IN_NAME: &str = "Aero";

/// The file inside a theme's directory.
pub const FILE_NAME: &str = "theme.yaml";

/// Where the system's themes are installed.
pub const SYSTEM_DIR: &str = "/usr/share/slateos/themes";

/// The section holding a theme's dark-mode colours.
///
/// The unmarked one, because dark is this desktop's default mode.
pub const DARK_SECTION: &str = "colors";

/// The section holding a theme's light-mode colours.
pub const LIGHT_SECTION: &str = "colors-light";

/// The largest theme file that is read.
///
/// A complete theme is under two kilobytes; a quarter of a megabyte is a
/// hundred times that, and still small enough that reading one costs nothing a
/// user would notice. The bound exists because a theme is data from strangers
/// -- the roadmap has a theme repository -- and a file of any size would
/// otherwise be read whole, on the path that starts the desktop.
pub const MAX_FILE_BYTES: u64 = 256 * 1024;

/// How many problems with one file are listed before the rest are counted.
const MAX_WARNINGS: usize = 20;

/// How much of a value a warning quotes.
const MAX_QUOTED_CHARS: usize = 40;

// ============================================================================
// Where themes are installed
// ============================================================================

/// Where a theme was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// Compiled into the desktop: the built-in theme.
    BuiltIn,
    /// Installed with the system, under [`SYSTEM_DIR`].
    System,
    /// Installed by this user, under their data directory.
    User,
}

/// The directories themes are looked for in.
///
/// A value rather than a pair of constants so that a test can point it at a
/// scratch directory and know nothing on the machine running it is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeDirs {
    /// This user's themes, searched first. `None` when the environment names
    /// no user -- neither `XDG_DATA_HOME` nor `HOME` -- as in early boot.
    pub user: Option<PathBuf>,
    /// The system's themes.
    pub system: PathBuf,
}

impl ThemeDirs {
    /// The directories this user's desktop reads: `$XDG_DATA_HOME` (or
    /// `~/.local/share`) `/slateos/themes`, then [`SYSTEM_DIR`].
    ///
    /// Both variables are honoured for the reason `settingsfile::config_dir`
    /// honours both of its own: a user who set `XDG_DATA_HOME` said plainly
    /// where their data lives.
    #[must_use]
    pub fn standard() -> Self {
        Self {
            user: user_dir(),
            system: PathBuf::from(SYSTEM_DIR),
        }
    }

    /// The directories in the order they are searched: the user's first.
    pub(crate) fn roots(&self) -> Vec<(&Path, Origin)> {
        let mut roots = Vec::with_capacity(2);
        if let Some(user) = self.user.as_deref() {
            roots.push((user, Origin::User));
        }
        roots.push((self.system.as_path(), Origin::System));
        roots
    }

    /// The directory of the installed theme `id`, and where it was found --
    /// the first root holding `<id>/theme.yaml`. `None` if no root does.
    ///
    /// `id` must already have passed [`is_valid_id`]; joining an unchecked
    /// name to a root is how a name leaves it.
    fn find(&self, id: &OsStr) -> Option<(PathBuf, Origin)> {
        self.roots().into_iter().find_map(|(root, origin)| {
            let dir = root.join(id);
            dir.join(FILE_NAME).is_file().then_some((dir, origin))
        })
    }
}

/// `$XDG_DATA_HOME/slateos/themes`, or `$HOME/.local/share/slateos/themes`.
fn user_dir() -> Option<PathBuf> {
    let data = match env::var_os("XDG_DATA_HOME") {
        Some(xdg) if !xdg.is_empty() => PathBuf::from(xdg),
        _ => {
            let home = env::var_os("HOME")?;
            if home.is_empty() {
                return None;
            }
            PathBuf::from(home).join(".local").join("share")
        }
    };
    Some(data.join("slateos").join("themes"))
}

/// Whether `id` can name a theme: a single ordinary path component, so that
/// joining it to a themes directory cannot leave that directory.
///
/// The name comes from `appearance.yaml`, which a user -- or anything able to
/// write their files -- can edit, so `../../somewhere` is not hypothetical.
/// Every other byte is allowed, as in any name on this system (`design.txt`
/// allows all but `/` and NUL). On the development host a backslash is a
/// separator too, and is refused there for the same reason.
#[must_use]
pub fn is_valid_id(id: &OsStr) -> bool {
    let bytes = id.as_encoded_bytes();
    if bytes.is_empty() || bytes.contains(&b'/') || bytes.contains(&0) {
        return false;
    }
    if cfg!(windows) && bytes.contains(&b'\\') {
        return false;
    }
    let mut parts = Path::new(id).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    )
}

// ============================================================================
// Why a theme could not be used
// ============================================================================

/// Why a theme could not be read, or could be read but not used for colours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeError {
    /// The name is not a single path component; see [`is_valid_id`].
    InvalidName,
    /// No themes directory holds a theme of this name.
    NotInstalled,
    /// The file is larger than [`MAX_FILE_BYTES`]; the size it was found to be.
    TooLarge(u64),
    /// The file is not UTF-8, so it is not YAML.
    NotText,
    /// Reading the file failed, for the reason given.
    Unreadable(String),
    /// The file was read but sets no colours, in either mode.
    NoColors,
}

impl fmt::Display for ThemeError {
    /// A clause that follows the theme's name: `"nord" is not installed`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("is not the name of a folder in a themes directory"),
            Self::NotInstalled => f.write_str("is not installed"),
            Self::TooLarge(bytes) => write!(
                f,
                "has a {bytes}-byte file, over the {MAX_FILE_BYTES}-byte limit for a theme"
            ),
            Self::NotText => f.write_str("has a file that is not text"),
            Self::Unreadable(why) => write!(f, "could not be read ({why})"),
            Self::NoColors => f.write_str("sets no colours"),
        }
    }
}

// ============================================================================
// Reading a theme file
// ============================================================================

/// A theme's `meta` block: what describes it, as against what it sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeMeta {
    /// Its name as shown. The directory's name is used when this is absent.
    pub name: Option<String>,
    /// Who made it.
    pub author: Option<String>,
    /// Its version, as the author spells it.
    pub version: Option<String>,
    /// Its licence, as the author spells it.
    pub license: Option<String>,
    /// Words to search it by.
    pub tags: Vec<String>,
    /// Pictures of it, as the file names them: paths inside the theme's
    /// directory. [`ThemeInfo::screenshots`] has them resolved.
    pub screenshots: Vec<String>,
    /// The axes it claims to cover (`colors`, `icons`, `cursors`, ...), as
    /// written. A claim, for a theme browser to show: which axes a theme
    /// *does* cover is what its sections say, and nothing here trusts this
    /// list over them.
    pub supports: Vec<String>,
}

/// A theme file, read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeFile {
    /// What describes it.
    pub meta: ThemeMeta,
    /// The colours it sets.
    pub colors: ThemeColors,
    /// What in it was not understood and so was ignored -- a colour this
    /// desktop has no role for, a value that is not a colour. For the theme's
    /// author, and for a theme browser to show them: the theme is used without
    /// these, not refused for them.
    pub warnings: Vec<String>,
}

/// Read a theme from its text. Never fails: what cannot be understood is
/// ignored and listed in [`ThemeFile::warnings`], the rule every settings
/// reader here follows -- one mistyped colour should cost that colour, not
/// the theme.
#[must_use]
pub fn parse(text: &str) -> ThemeFile {
    let doc = Document::parse(text);
    let mut warnings = Warnings::default();
    let meta = ThemeMeta {
        name: scalar(&doc, &["meta", "name"]),
        author: scalar(&doc, &["meta", "author"]),
        version: scalar(&doc, &["meta", "version"]),
        license: scalar(&doc, &["meta", "license"]),
        tags: list(&doc, &["meta", "tags"]),
        screenshots: list(&doc, &["meta", "screenshots"]),
        supports: list(&doc, &["meta", "supports"]),
    };
    let colors = ThemeColors {
        dark: read_colors(&doc, DARK_SECTION, &mut warnings),
        light: read_colors(&doc, LIGHT_SECTION, &mut warnings),
    };
    ThemeFile {
        meta,
        colors,
        warnings: warnings.finish(),
    }
}

/// A non-empty scalar, trimmed.
fn scalar(doc: &Document, path: &[&str]) -> Option<String> {
    doc.get_str(path)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// A list, written either as a block (`- a` lines) or in flow style
/// (`[a, b]`) -- the file format's own example uses the second, and `yamldoc`
/// reads only the first. A bare scalar is a list of one.
fn list(doc: &Document, path: &[&str]) -> Vec<String> {
    let block = doc.get_seq(path).unwrap_or_default();
    if !block.is_empty() {
        return block
            .into_iter()
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect();
    }
    doc.get_str(path)
        .map(|value| flow_items(&value))
        .unwrap_or_default()
}

/// The items of a flow sequence, `[a, "b, c", 'd''s']`, unquoted.
///
/// Enough of YAML's flow syntax for the words and file names a `meta` block
/// holds: commas separate unless quoted, a single-quoted `''` is one quote,
/// and a backslash in double quotes takes the next character literally.
fn flow_items(text: &str) -> Vec<String> {
    let text = text.trim();
    let Some(inner) = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![text.to_owned()]
        };
    };
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') if c == '\'' => {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    current.push('\'');
                } else {
                    quote = None;
                }
            }
            Some('"') if c == '\\' => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            Some(open) if c == open => quote = None,
            Some(_) => current.push(c),
            None => match c {
                '"' | '\'' => quote = Some(c),
                ',' => {
                    push_item(&mut items, &current);
                    current.clear();
                }
                _ => current.push(c),
            },
        }
    }
    push_item(&mut items, &current);
    items
}

fn push_item(items: &mut Vec<String>, raw: &str) {
    let item = raw.trim();
    if !item.is_empty() {
        items.push(item.to_owned());
    }
}

/// The colours one section sets, by role.
fn read_colors(doc: &Document, section: &str, warnings: &mut Warnings) -> BTreeMap<String, Color> {
    let mut out = BTreeMap::new();
    for role in doc.keys(&[section]) {
        let at = format!("{section}.{role}");
        if role == "accent" {
            warnings.push(format!(
                "`{at}` is ignored: the accent is the user's to choose, in Settings"
            ));
            continue;
        }
        if !THEME_ROLES.contains(&role.as_str()) {
            warnings.push(format!(
                "`{at}` is ignored: this desktop has no colour called `{}`",
                quoted(&role)
            ));
            continue;
        }
        let Some(value) = doc.get_str(&[section, &role]) else {
            // The usual cause, and the reason this says more than "missing":
            // `base: #2e3440` is a key followed by a *comment*, because an
            // unquoted `#` after a space begins one.
            warnings.push(format!(
                "`{at}` has no value: write a colour in quotes, like \"#2e3440\" -- \
                 without them, `#` begins a comment"
            ));
            continue;
        };
        let value = value.trim();
        match color_from_hex(value) {
            Some(color) if color.a == 255 => {
                out.insert(role, color);
            }
            // A translucent ground has no contrast of its own until something
            // is behind it, so neither the floor nor anything else here could
            // say whether text on it can be read.
            Some(_) => warnings.push(format!(
                "`{at}` is ignored: a theme's colours must be opaque, and `{}` is not",
                quoted(value)
            )),
            None => warnings.push(format!(
                "`{at}` is ignored: `{}` is not a colour (expected `#rrggbb`)",
                quoted(value)
            )),
        }
    }
    out
}

/// A value as a warning quotes it: whole if short, cut and marked if not, so
/// a quarter-megabyte value cannot become a quarter-megabyte message.
fn quoted(value: &str) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(MAX_QUOTED_CHARS).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// The warnings for one file, bounded.
#[derive(Default)]
struct Warnings {
    kept: Vec<String>,
    dropped: usize,
}

impl Warnings {
    fn push(&mut self, warning: String) {
        if self.kept.len() < MAX_WARNINGS {
            self.kept.push(warning);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    fn finish(mut self) -> Vec<String> {
        if self.dropped > 0 {
            self.kept
                .push(format!("... and {} more like these", self.dropped));
        }
        self.kept
    }
}

/// Read and parse the theme file at `path`, within [`MAX_FILE_BYTES`].
fn read_theme_file(path: &Path) -> Result<ThemeFile, ThemeError> {
    let bytes = read_theme_bytes(path)?;
    let text = String::from_utf8(bytes).map_err(|_| ThemeError::NotText)?;
    Ok(parse(&text))
}

/// The bytes of the theme file at `path`, within [`MAX_FILE_BYTES`].
fn read_theme_bytes(path: &Path) -> Result<Vec<u8>, ThemeError> {
    let unreadable = |e: std::io::Error| ThemeError::Unreadable(e.to_string());
    let file = fs::File::open(path).map_err(unreadable)?;
    let size = file.metadata().map_err(unreadable)?.len();
    if size > MAX_FILE_BYTES {
        return Err(ThemeError::TooLarge(size));
    }
    // Bounded again while reading: the size above was true when it was asked,
    // and a file can grow between the two.
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read > MAX_FILE_BYTES {
        return Err(ThemeError::TooLarge(read));
    }
    Ok(bytes)
}

/// What the colours read from `doc` depend on besides the document itself:
/// the chosen theme's file -- where it was found, and what it holds. The
/// dependency fingerprint of [`crate::watcher`].
///
/// Empty for the built-in theme, whose colours are compiled in, and for a
/// name that cannot be a theme. A theme that is not installed, or cannot be
/// read, contributes that fact, so one appearing, disappearing or moving
/// between the user's directory and the system's is a change as much as an
/// edit is.
pub(crate) fn fingerprint(doc: &Document) -> Vec<u8> {
    let Some(id) = crate::color_theme_name(doc) else {
        return Vec::new();
    };
    if id == OsStr::new(BUILT_IN) || !is_valid_id(&id) {
        return Vec::new();
    }
    let Some((dir, _)) = ThemeDirs::standard().find(&id) else {
        return b"not installed".to_vec();
    };
    let file = dir.join(FILE_NAME);
    let mut out = file.as_os_str().as_encoded_bytes().to_vec();
    out.push(0);
    match read_theme_bytes(&file) {
        Ok(bytes) => out.extend_from_slice(&bytes),
        Err(err) => out.extend_from_slice(format!("{err:?}").as_bytes()),
    }
    out
}

/// Find, read and check the installed theme `id` for its colours.
fn read_for_colors(dirs: &ThemeDirs, id: &OsStr) -> Result<ThemeFile, ThemeError> {
    if !is_valid_id(id) {
        return Err(ThemeError::InvalidName);
    }
    let (dir, _) = dirs.find(id).ok_or(ThemeError::NotInstalled)?;
    let file = read_theme_file(&dir.join(FILE_NAME))?;
    if file.colors.dark.is_empty() && file.colors.light.is_empty() {
        return Err(ThemeError::NoColors);
    }
    Ok(file)
}

// ============================================================================
// The colour theme in use
// ============================================================================

/// The colour theme in use: which one was chosen, and what reading it gave.
///
/// One value rather than a name beside a set of colours, so the two cannot
/// disagree. The only ways to have a `ColorTheme` named "nord" are to read
/// "nord" ([`load`](Self::load)) or to be handed its colours
/// ([`from_colors`](Self::from_colors)); there is no way to change the name
/// and keep the old colours.
///
/// The colours are behind an `Arc` because the settings they belong to are
/// cloned freely -- the shell compares a fresh copy against the one it holds
/// on every change -- and a theme is two maps of up to twenty-six entries
/// that nothing ever changes once read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorTheme {
    id: OsString,
    colors: Option<Arc<ThemeColors>>,
    warnings: Vec<String>,
    problem: Option<String>,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl ColorTheme {
    /// The built-in theme, whose colours are the palette's own.
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            id: OsString::from(BUILT_IN),
            colors: None,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme named `id`, read from the standard directories.
    ///
    /// Never fails. A theme that cannot be used -- not installed, unreadable,
    /// setting no colours -- keeps its name, so that saving the settings does
    /// not quietly forget the user's choice, and shows the built-in colours
    /// with a [`problem`](Self::problem) saying why.
    #[must_use]
    pub fn load(id: &OsStr) -> Self {
        Self::load_from(&ThemeDirs::standard(), id)
    }

    /// [`load`](Self::load), from the given directories.
    #[must_use]
    pub fn load_from(dirs: &ThemeDirs, id: &OsStr) -> Self {
        if id == OsStr::new(BUILT_IN) {
            return Self::built_in();
        }
        match read_for_colors(dirs, id) {
            Ok(file) => Self {
                id: id.to_owned(),
                colors: Some(Arc::new(file.colors)),
                warnings: file.warnings,
                problem: None,
            },
            Err(err) => Self {
                id: id.to_owned(),
                colors: None,
                warnings: Vec::new(),
                problem: Some(format!(
                    "\"{}\" {err}, so the built-in colours are shown.",
                    pathcodec::display_os(id)
                )),
            },
        }
    }

    /// A theme whose colours are already in hand -- a preview of one being
    /// edited, or a test's.
    #[must_use]
    pub fn from_colors(id: impl Into<OsString>, colors: ThemeColors) -> Self {
        Self {
            id: id.into(),
            colors: Some(Arc::new(colors)),
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme's name, as `theme.colors` holds it.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == OsStr::new(BUILT_IN)
    }

    /// The colours to lay over the built-in palette; `None` for the built-in
    /// theme, and for one that could not be used.
    #[must_use]
    pub fn colors(&self) -> Option<&ThemeColors> {
        self.colors.as_deref()
    }

    /// What in the theme's file was ignored; see [`ThemeFile::warnings`].
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Why the chosen theme is not in use, as a sentence for the user; `None`
    /// when it is.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

// ============================================================================
// Listing themes
// ============================================================================

/// An installed theme, described for a list to choose from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeInfo {
    /// Its name, as `theme.colors` would hold it: the directory's name.
    pub id: OsString,
    /// Its name as shown: `meta.name`, or the directory's name drawn as text.
    pub name: String,
    /// Where it was found.
    pub origin: Origin,
    /// Its directory; `None` for the built-in theme when no file for it is
    /// installed.
    pub dir: Option<PathBuf>,
    /// What describes it.
    pub meta: ThemeMeta,
    /// Its screenshots, resolved inside [`dir`](Self::dir). One that would
    /// resolve outside is dropped, with a warning.
    pub screenshots: Vec<PathBuf>,
    /// Whether it sets dark-mode colours.
    pub has_dark: bool,
    /// Whether it sets light-mode colours.
    pub has_light: bool,
    /// What in its file was ignored.
    pub warnings: Vec<String>,
    /// Why it could not be read, if it could not. A theme that cannot be read
    /// is listed anyway, so the list can say why it cannot be chosen rather
    /// than leave its author wondering where it went.
    pub problem: Option<ThemeError>,
}

impl ThemeInfo {
    /// Whether it can be chosen for the colours axis: it was read, and sets
    /// colours for at least one mode.
    #[must_use]
    pub fn provides_colors(&self) -> bool {
        self.problem.is_none() && (self.has_dark || self.has_light)
    }

    /// Whether it can be chosen for the icons axis: its folder holds an
    /// [`icons`](crate::icons::ICONS_DIR) directory -- or it is the built-in
    /// theme, whose icons are compiled in. A folder with icons and no
    /// `theme.yaml`, an icon pack, is a theme for this axis alone.
    ///
    /// Asked of the folder when called rather than kept from the listing, as a
    /// field the listing filled and nothing in the desktop read: its one reader
    /// is the Settings app's picker, which asks it once for each row it draws.
    #[must_use]
    pub fn provides_icons(&self) -> bool {
        self.origin == Origin::BuiltIn
            || self
                .dir
                .as_ref()
                .is_some_and(|dir| dir.join(crate::icons::ICONS_DIR).is_dir())
    }
}

/// Every installed theme, from the standard directories.
#[must_use]
pub fn available() -> Vec<ThemeInfo> {
    available_in(&ThemeDirs::standard())
}

/// Every theme in `dirs`: the built-in one first, then the rest by name.
///
/// A name installed in both directories is listed once, as the user's copy --
/// the one [`ColorTheme::load`] would read. The built-in theme is always
/// listed, whether or not a file for it is installed; when one is, only its
/// description is taken from it (the colours are compiled in), and only from
/// the system directory, since a user's `aero` could not be chosen apart from
/// the built-in one and listing it as if it could would be a lie.
#[must_use]
pub fn available_in(dirs: &ThemeDirs) -> Vec<ThemeInfo> {
    let mut found: BTreeMap<OsString, ThemeInfo> = BTreeMap::new();
    for (root, origin) in dirs.roots() {
        // A directory that does not exist has no themes in it, which is the
        // ordinary state of a user's data directory: not an error to report.
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        // An entry the directory listing could not describe is one that could
        // not be opened as a theme either; skipping it costs a list entry
        // nobody could have chosen.
        for entry in entries.flatten() {
            let id = entry.file_name();
            if id == OsStr::new(BUILT_IN) || found.contains_key(&id) || !is_valid_id(&id) {
                continue;
            }
            let dir = entry.path();
            let file = dir.join(FILE_NAME);
            // A directory with neither a theme file nor icons is not a theme --
            // a half-copied download, or something else entirely -- and is
            // left out. One with icons alone is an icon pack: a theme for the
            // icons axis, with no colours and nothing wrong with it.
            let read = if file.is_file() {
                read_theme_file(&file)
            } else if dir.join(crate::icons::ICONS_DIR).is_dir() {
                Ok(ThemeFile::default())
            } else {
                continue;
            };
            let info = describe(&id, dir, origin, read);
            found.insert(id, info);
        }
    }

    let mut themes: Vec<ThemeInfo> = found.into_values().collect();
    themes.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    themes.insert(0, built_in_info(dirs));
    themes
}

/// The built-in theme's entry, described by its installed file if the system
/// has one.
fn built_in_info(dirs: &ThemeDirs) -> ThemeInfo {
    let dir = dirs.system.join(BUILT_IN);
    let file = dir.join(FILE_NAME);
    let mut info = if file.is_file() {
        describe(
            OsStr::new(BUILT_IN),
            dir,
            Origin::BuiltIn,
            read_theme_file(&file),
        )
    } else {
        ThemeInfo {
            id: OsString::from(BUILT_IN),
            name: BUILT_IN_NAME.to_owned(),
            origin: Origin::BuiltIn,
            dir: None,
            meta: ThemeMeta::default(),
            screenshots: Vec::new(),
            has_dark: true,
            has_light: true,
            warnings: Vec::new(),
            problem: None,
        }
    };
    // Whatever its file says, the built-in theme's colours and icons are
    // compiled in: it covers both modes, draws every icon, and cannot fail to
    // load.
    info.has_dark = true;
    info.has_light = true;
    info.problem = None;
    info
}

/// A list entry for the theme `id` in `dir`, from reading its file.
fn describe(
    id: &OsStr,
    dir: PathBuf,
    origin: Origin,
    read: Result<ThemeFile, ThemeError>,
) -> ThemeInfo {
    let shown = pathcodec::display_os(id);
    match read {
        Ok(file) => {
            let mut warnings = file.warnings;
            let mut screenshots = Vec::new();
            for name in &file.meta.screenshots {
                match confined(&dir, name) {
                    Some(path) => screenshots.push(path),
                    None => warnings.push(format!(
                        "screenshot `{}` is ignored: it must be a path inside the theme's folder",
                        quoted(name)
                    )),
                }
            }
            ThemeInfo {
                id: id.to_owned(),
                name: file.meta.name.clone().unwrap_or(shown),
                origin,
                dir: Some(dir),
                has_dark: !file.colors.dark.is_empty(),
                has_light: !file.colors.light.is_empty(),
                meta: file.meta,
                screenshots,
                warnings,
                problem: None,
            }
        }
        Err(err) => ThemeInfo {
            id: id.to_owned(),
            name: shown,
            origin,
            dir: Some(dir),
            meta: ThemeMeta::default(),
            screenshots: Vec::new(),
            has_dark: false,
            has_light: false,
            warnings: Vec::new(),
            problem: Some(err),
        },
    }
}

/// `name` resolved inside `dir`, if it stays there: relative, and made only
/// of ordinary components (and `.`).
fn confined(dir: &Path, name: &str) -> Option<PathBuf> {
    let relative = Path::new(name);
    let inside = !name.is_empty()
        && relative
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    inside.then(|| dir.join(relative))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::{AppearanceSettings, HighContrastScheme, Palette, PaletteSource, SurfaceStyle};
    use scratchdir::ScratchDir;

    /// A pair of theme directories inside a scratch directory, and a way to
    /// install a theme in either.
    struct Fixture {
        scratch: ScratchDir,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            Self {
                scratch: ScratchDir::new(&format!("slateos-themes-{tag}")),
            }
        }

        fn dirs(&self) -> ThemeDirs {
            ThemeDirs {
                user: Some(self.scratch.dir().join("user")),
                system: self.scratch.dir().join("system"),
            }
        }

        fn install(&self, origin: Origin, id: &str, text: &str) -> PathBuf {
            let root = match origin {
                Origin::User => "user",
                Origin::System | Origin::BuiltIn => "system",
            };
            let dir = self.scratch.dir().join(root).join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(FILE_NAME), text).unwrap();
            dir
        }
    }

    const NORD: &str = "\
meta:
  name: Nord
  author: Someone
  version: \"1.2\"
  license: MIT
  tags: [cool, 'blue''s', \"a, b\"]
  screenshots:
    - dark.png
    - shots/light.png
  supports: [colors]
colors:
  base: \"#2e3440\"
  text: \"#eceff4\"
  red: \"#bf616a\"
colors-light:
  base: \"#eceff4\"
  text: \"#2e3440\"
";

    fn rgb(hex: u32) -> Color {
        Color::from_hex(hex)
    }

    // ---- the file ----

    #[test]
    fn a_theme_file_gives_its_colours_by_mode_and_its_description() {
        let file = parse(NORD);
        assert_eq!(file.warnings, Vec::<String>::new());
        assert_eq!(file.meta.name.as_deref(), Some("Nord"));
        assert_eq!(file.meta.author.as_deref(), Some("Someone"));
        assert_eq!(file.meta.version.as_deref(), Some("1.2"));
        assert_eq!(file.meta.license.as_deref(), Some("MIT"));
        assert_eq!(file.meta.tags, ["cool", "blue's", "a, b"]);
        assert_eq!(file.meta.screenshots, ["dark.png", "shots/light.png"]);
        assert_eq!(file.meta.supports, ["colors"]);
        assert_eq!(file.colors.dark.len(), 3);
        assert_eq!(file.colors.dark["base"], rgb(0x2E3440));
        assert_eq!(file.colors.dark["red"], rgb(0xBF616A));
        assert_eq!(file.colors.light.len(), 2);
        assert_eq!(file.colors.light["text"], rgb(0x2E3440));
    }

    /// Everything wrong with a colour costs that colour and is said, and the
    /// rest of the theme is kept.
    #[test]
    fn what_is_not_understood_is_ignored_and_listed() {
        let file = parse(
            "\
colors:
  base: \"#101010\"
  accent: \"#ff0000\"
  bse: \"#202020\"
  text: \"#80808080\"
  red: \"crimson\"
  green: #00ff00
",
        );
        assert_eq!(file.colors.dark.len(), 1, "{:?}", file.colors.dark);
        assert_eq!(file.colors.dark["base"], rgb(0x101010));
        let said = file.warnings.join("\n");
        assert_eq!(file.warnings.len(), 5, "{said}");
        for expected in [
            "`colors.accent` is ignored: the accent is the user's",
            "no colour called `bse`",
            "must be opaque, and `#80808080` is not",
            "`crimson` is not a colour",
            "`colors.green` has no value: write a colour in quotes",
        ] {
            assert!(said.contains(expected), "missing {expected:?} in:\n{said}");
        }
    }

    /// A file of nothing but mistakes cannot produce a message of any length:
    /// the list stops and counts, and a long value is cut.
    #[test]
    fn the_warnings_are_bounded() {
        use core::fmt::Write as _;
        // The long value first, so its warning is one of those kept.
        let mut text = String::from("colors:\n");
        writeln!(text, "  base: \"{}\"", "x".repeat(5000)).unwrap();
        for i in 0..100 {
            writeln!(text, "  nope{i}: \"#000000\"").unwrap();
        }
        let file = parse(&text);
        assert_eq!(file.warnings.len(), MAX_WARNINGS + 1);
        assert_eq!(file.warnings[MAX_WARNINGS], "... and 81 more like these");
        assert!(
            file.warnings.iter().all(|w| w.len() < 200),
            "{:?}",
            file.warnings
        );
    }

    #[test]
    fn a_list_may_be_a_block_a_flow_or_a_single_word() {
        let file = parse("meta:\n  tags: dark\n  supports: []\n");
        assert_eq!(file.meta.tags, ["dark"]);
        assert_eq!(file.meta.supports, Vec::<String>::new());
        assert_eq!(flow_items("[ a ,b,, \"c\\\"d\" ]"), ["a", "b", "c\"d"]);
    }

    // ---- names ----

    #[test]
    fn a_theme_name_is_one_path_component() {
        for good in ["nord", "Nord Dark", "100%", "ça"] {
            assert!(is_valid_id(OsStr::new(good)), "{good:?}");
        }
        for bad in ["", ".", "..", "../x", "a/b", "/abs", "x/", "a\0b"] {
            assert!(!is_valid_id(OsStr::new(bad)), "{bad:?}");
        }
        #[cfg(windows)]
        for bad in ["a\\b", "..\\x", "C:"] {
            assert!(!is_valid_id(OsStr::new(bad)), "{bad:?}");
        }
    }

    // ---- loading ----

    #[test]
    fn an_installed_theme_is_loaded_with_its_colours() {
        let fx = Fixture::new("load");
        fx.install(Origin::System, "nord", NORD);
        let theme = ColorTheme::load_from(&fx.dirs(), OsStr::new("nord"));
        assert_eq!(theme.problem(), None);
        assert_eq!(theme.id(), "nord");
        assert!(!theme.is_built_in());
        assert_eq!(theme.colors(), Some(&parse(NORD).colors));
    }

    /// The user's copy of a theme is the one read: that is how a user adjusts
    /// one the system installed.
    #[test]
    fn the_users_copy_of_a_theme_wins() {
        let fx = Fixture::new("shadow");
        fx.install(Origin::System, "nord", NORD);
        fx.install(Origin::User, "nord", "colors:\n  base: \"#000001\"\n");
        let theme = ColorTheme::load_from(&fx.dirs(), OsStr::new("nord"));
        assert_eq!(theme.colors().unwrap().dark["base"], rgb(0x000001));
    }

    /// The built-in theme is not read from anywhere, so no file -- present,
    /// broken or absent -- can change it.
    #[test]
    fn the_built_in_theme_reads_no_file() {
        let fx = Fixture::new("builtin");
        fx.install(Origin::User, BUILT_IN, "colors:\n  base: \"#ff0000\"\n");
        let theme = ColorTheme::load_from(&fx.dirs(), OsStr::new(BUILT_IN));
        assert_eq!(theme, ColorTheme::built_in());
        assert!(theme.is_built_in());
        assert_eq!(theme.colors(), None);
    }

    /// Every way a theme can fail keeps its name -- so saving the settings
    /// does not forget the choice -- and says why in words.
    #[test]
    fn a_theme_that_cannot_be_used_says_why_and_keeps_its_name() {
        let fx = Fixture::new("fail");
        fx.install(Origin::System, "empty", "meta:\n  name: Empty\n");
        let big = fx.install(Origin::System, "big", "");
        fs::write(
            big.join(FILE_NAME),
            vec![b'#'; usize::try_from(MAX_FILE_BYTES).unwrap() + 1],
        )
        .unwrap();
        let binary = fx.install(Origin::System, "binary", "");
        fs::write(binary.join(FILE_NAME), b"colors:\n  base: \"\xff\"\n").unwrap();

        for (id, why) in [
            (
                "missing",
                "\"missing\" is not installed, so the built-in colours are shown.",
            ),
            (
                "empty",
                "\"empty\" sets no colours, so the built-in colours are shown.",
            ),
            ("big", "over the 262144-byte limit for a theme"),
            ("binary", "\"binary\" has a file that is not text"),
            (
                "../system/empty",
                "is not the name of a folder in a themes directory",
            ),
        ] {
            let theme = ColorTheme::load_from(&fx.dirs(), OsStr::new(id));
            assert_eq!(theme.id(), id);
            assert_eq!(theme.colors(), None, "{id}");
            let problem = theme
                .problem()
                .unwrap_or_else(|| panic!("{id} has no problem"));
            assert!(problem.contains(why), "{id}: {problem}");
        }
    }

    // ---- listing ----

    #[test]
    fn the_list_has_the_built_in_theme_first_then_the_rest_by_name() {
        let fx = Fixture::new("list");
        fx.install(Origin::System, "nord", NORD);
        fx.install(
            Origin::System,
            "zz",
            "meta:\n  name: alpha\ncolors-light:\n  base: \"#ffffff\"\n",
        );
        fx.install(
            Origin::User,
            "nord",
            "meta:\n  name: My Nord\ncolors:\n  base: \"#000000\"\n",
        );
        fx.install(Origin::User, "broken", "");
        fs::write(
            fx.dirs().user.unwrap().join("broken").join(FILE_NAME),
            b"\xff",
        )
        .unwrap();
        // Not themes: a directory without the file, and a stray file.
        fs::create_dir_all(fx.dirs().system.join("half")).unwrap();
        fs::write(fx.dirs().system.join("notes.txt"), "hi").unwrap();

        let list = available_in(&fx.dirs());
        let names: Vec<&str> = list.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Aero", "alpha", "broken", "My Nord"]);

        assert_eq!(list[0].origin, Origin::BuiltIn);
        assert!(list[0].provides_colors());
        assert_eq!(list[0].dir, None);

        let alpha = &list[1];
        assert_eq!((alpha.has_dark, alpha.has_light), (false, true));
        assert!(alpha.provides_colors());

        let broken = &list[2];
        assert_eq!(broken.problem, Some(ThemeError::NotText));
        assert!(!broken.provides_colors());

        let nord = &list[3];
        assert_eq!(nord.origin, Origin::User);
        assert_eq!((nord.has_dark, nord.has_light), (true, false));
    }

    /// A folder of icons without a theme file is an icon pack: listed, for the
    /// icons axis alone -- no colours, and nothing wrong with it. A theme with
    /// both says so; the built-in theme always draws icons.
    #[test]
    fn an_icon_pack_is_listed_for_its_icons_alone() {
        let fx = Fixture::new("icon-packs");
        let pack = fx.dirs().system.join("lines");
        fs::create_dir_all(pack.join(crate::icons::ICONS_DIR)).unwrap();
        let both = fx.install(Origin::User, "nord", "colors:\n  base: \"#000000\"\n");
        fs::create_dir_all(both.join(crate::icons::ICONS_DIR)).unwrap();
        fx.install(Origin::User, "plain", "colors:\n  base: \"#101010\"\n");

        let list = available_in(&fx.dirs());
        let by_id = |id: &str| {
            list.iter()
                .find(|t| t.id == OsStr::new(id))
                .unwrap_or_else(|| panic!("{id} is not listed"))
        };
        let lines = by_id("lines");
        assert!(lines.provides_icons());
        assert!(!lines.provides_colors());
        assert_eq!(lines.problem, None, "an icon pack is not a broken theme");
        assert!(by_id("nord").provides_icons() && by_id("nord").provides_colors());
        assert!(!by_id("plain").provides_icons());
        assert!(list[0].provides_icons(), "the built-in theme draws icons");
    }

    /// A screenshot is a path inside the theme's folder or it is nothing: a
    /// theme is data from strangers, and one naming `/etc/shadow` as its
    /// picture should not have the desktop open it.
    #[test]
    fn a_screenshot_outside_the_themes_folder_is_dropped() {
        let fx = Fixture::new("shots");
        let dir = fx.install(
            Origin::System,
            "nord",
            "meta:\n  screenshots: [a.png, ./b.png, ../x.png, /etc/shadow]\ncolors:\n  base: \"#000000\"\n",
        );
        let list = available_in(&fx.dirs());
        let nord = list.iter().find(|t| t.id == "nord").unwrap();
        assert_eq!(nord.screenshots, [dir.join("a.png"), dir.join("./b.png")]);
        assert_eq!(nord.warnings.len(), 2, "{:?}", nord.warnings);
    }

    /// The built-in theme's list entry takes its description from the
    /// system's file for it, when there is one, and never its colours or its
    /// failures: those are compiled in.
    #[test]
    fn the_built_in_entry_is_described_by_its_installed_file() {
        let fx = Fixture::new("builtin-list");
        fx.install(
            Origin::System,
            BUILT_IN,
            "meta:\n  name: Aero Glass\n  author: SlateOS\n",
        );
        let list = available_in(&fx.dirs());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Aero Glass");
        assert_eq!(list[0].meta.author.as_deref(), Some("SlateOS"));
        assert!(list[0].provides_colors());
    }

    // ---- the palette a theme gives ----

    fn themed(id: &str, colors: ThemeColors, light: bool) -> AppearanceSettings {
        AppearanceSettings {
            color_theme: ColorTheme::from_colors(id, colors),
            theme_mode: if light {
                crate::ThemeMode::Light
            } else {
                crate::ThemeMode::Dark
            },
            ..AppearanceSettings::default()
        }
    }

    #[test]
    fn a_theme_replaces_the_roles_it_sets_and_keeps_the_rest() {
        let colors = parse(NORD).colors;
        let settings = themed("nord", colors, false);
        let p = Palette::from_settings(&settings);
        let built_in = Palette::for_mode(false);
        assert_eq!(p.base, rgb(0x2E3440));
        assert_eq!(p.red, rgb(0xBF616A));
        assert_eq!(p.text, rgb(0xECEFF4), "readable already, so unchanged");
        assert_eq!(p.mantle, built_in.mantle);
        assert_eq!(p.teal, built_in.teal);
        assert_eq!(
            p.accent,
            settings.accent(),
            "a theme does not choose the accent"
        );
        assert!(!p.light);

        let light = Palette::from_settings(&themed("nord", parse(NORD).colors, true));
        assert_eq!(light.base, rgb(0xECEFF4));
        assert!(light.light);
    }

    /// A theme cannot make its text unreadable: text the theme sets too close
    /// to its ground is moved until it can be read, as the built-in inks are.
    #[test]
    fn a_themes_text_is_held_to_the_contrast_floor() {
        let colors = parse("colors:\n  base: \"#202020\"\n  text: \"#303030\"\n  subtext0: \"#282828\"\n  link: \"#252525\"\n").colors;
        let p = Palette::from_settings(&themed("murk", colors, false));
        for (name, ink) in [("text", p.text), ("subtext0", p.subtext0), ("link", p.link)] {
            let ratio = crate::contrast_ratio(ink, p.base);
            assert!(ratio >= 4.5, "{name} reads at {ratio:.2}:1");
        }
    }

    /// The contrast floor starts from the theme's inks every time, so changing
    /// the box style keeps the theme's text rather than putting the built-in
    /// text back.
    #[test]
    fn a_themes_inks_survive_a_change_of_surface_style() {
        let colors = parse("colors:\n  text: \"#ffd0d0\"\n  subtext1: \"#d0ffd0\"\n").colors;
        let mut settings = themed("pink", colors, false);
        for style in [
            SurfaceStyle::Borders,
            SurfaceStyle::Cards,
            SurfaceStyle::Borders,
        ] {
            settings.surface_style = style;
            let p = Palette::from_settings(&settings);
            assert_eq!(p.text, rgb(0xFFD0D0), "{style:?}");
            assert_eq!(p.subtext1, rgb(0xD0FFD0), "{style:?}");
        }
    }

    /// A theme with only a dark section is dark in light mode too, and the
    /// palette says so.
    #[test]
    fn a_theme_with_one_mode_is_shown_in_that_mode() {
        let colors = parse("colors:\n  base: \"#101418\"\n").colors;
        let p = Palette::from_settings(&themed("night", colors, true));
        assert_eq!(p.base, rgb(0x101418));
        assert!(!p.light, "built on the dark palette, so it must say dark");
        assert_eq!(p.surface0, Palette::for_mode(false).surface0);

        let only_light = parse("colors-light:\n  base: \"#fafafa\"\n").colors;
        let p = Palette::from_settings(&themed("day", only_light, false));
        assert_eq!(p.base, rgb(0xFAFAFA));
        assert!(p.light);
    }

    /// A theme with only dark colours, chosen in light mode, is drawn dark --
    /// and its accent is the dark-background one, matching the grounds it
    /// sits on rather than the switch in Settings. Asking the mode put the
    /// light-background accent (a deep blue meant for pale pages) on the
    /// theme's dark ones.
    #[test]
    fn a_one_mode_themes_accent_matches_its_grounds() {
        let colors = parse("colors:\n  base: \"#101418\"\n").colors;
        let settings = themed("night", colors, true);
        assert!(!settings.is_light(), "the theme can only be drawn dark");
        assert!(!Palette::from_settings(&settings).light);
        assert_eq!(
            settings.effective_accent(),
            settings.accent_color.color(),
            "the dark-background accent"
        );
        // And the ordinary case is untouched: no theme, light mode, the light
        // accent.
        let plain = AppearanceSettings {
            theme_mode: crate::ThemeMode::Light,
            ..AppearanceSettings::default()
        };
        assert!(plain.is_light());
        assert_eq!(plain.effective_accent(), plain.accent_color.color_light());
    }

    /// High contrast replaces the palette whole, theme and all.
    #[test]
    fn high_contrast_overrides_a_theme() {
        let mut settings = themed("nord", parse(NORD).colors, false);
        let plain = AppearanceSettings {
            high_contrast: Some(HighContrastScheme::YellowOnBlack),
            ..AppearanceSettings::default()
        };
        settings.high_contrast = plain.high_contrast;
        assert_eq!(
            Palette::from_settings(&settings),
            Palette::from_settings(&plain)
        );
    }

    /// A high-contrast palette keeps its own inks through the style setters --
    /// they re-run the contrast floor, which starts from the scheme's colours.
    #[test]
    fn a_high_contrast_palette_keeps_its_inks_through_the_setters() {
        for scheme in HighContrastScheme::ALL {
            let settings = AppearanceSettings {
                high_contrast: Some(scheme),
                ..AppearanceSettings::default()
            };
            let mut p = Palette::from_settings(&settings);
            let fg = p.text;
            for style in [SurfaceStyle::Cards, SurfaceStyle::Borders] {
                p.set_surface_style(style);
                assert_eq!(
                    (p.text, p.subtext0, p.subtext1),
                    (fg, fg, fg),
                    "{scheme:?} {style:?}"
                );
            }
        }
    }

    /// Every colour a palette has but the accent is a role a theme can set, so
    /// a role added to the palette is one a theme reaches -- or a decision, on
    /// the record here, that it is not.
    #[test]
    fn every_palette_role_but_the_accent_is_a_theme_role() {
        let roles: Vec<&str> = Palette::for_mode(false)
            .roles()
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| *name != "accent")
            .collect();
        let mut theme: Vec<&str> = THEME_ROLES.to_vec();
        let mut palette = roles.clone();
        theme.sort_unstable();
        palette.sort_unstable();
        assert_eq!(theme, palette);
    }

    /// Setting every role a theme can set reaches every role: none is read
    /// but dropped on the way into the palette.
    #[test]
    fn every_theme_role_reaches_the_palette() {
        for (i, role) in THEME_ROLES.iter().enumerate() {
            // A dark ground and a light ink, so the floor has nothing to move
            // whichever kind of role this is.
            let value = if matches!(*role, "text" | "subtext0" | "subtext1" | "link") {
                Color::rgb(250, 250, u8::try_from(200 + i).unwrap())
            } else {
                Color::rgb(u8::try_from(i).unwrap(), 1, 2)
            };
            let mut colors = ThemeColors::default();
            colors.dark.insert((*role).to_owned(), value);
            let p = Palette::for_theme(false, &colors);
            let got = p.roles().iter().find(|(name, _)| name == role).unwrap().1;
            assert_eq!(got, value, "{role}");
        }
    }

    /// The shipped file for the built-in theme is the built-in palette,
    /// exactly, in both modes, and names every role. It is the template a
    /// theme's author copies, so a role missing from it is a role nobody
    /// learns exists, and a colour that differs is a template that lies.
    #[test]
    fn the_shipped_built_in_theme_is_the_built_in_palette() {
        let file = parse(include_str!("../themes/aero/theme.yaml"));
        assert_eq!(file.warnings, Vec::<String>::new());
        assert_eq!(file.meta.name.as_deref(), Some(BUILT_IN_NAME));
        for light in [false, true] {
            let set = file.colors.roles(light);
            let missing: Vec<&str> = THEME_ROLES
                .iter()
                .copied()
                .filter(|role| !set.contains_key(*role))
                .collect();
            assert_eq!(missing, Vec::<&str>::new(), "light = {light}");
            let themed = Palette::for_theme(light, &file.colors);
            assert_eq!(
                themed.roles(),
                Palette::for_mode(light).roles(),
                "light = {light}"
            );
        }
    }

    // ---- where a user's themes are ----

    /// The scratch user has a scratch data directory, so a test that names a
    /// theme cannot find the developer's own.
    #[test]
    fn a_scratch_user_has_its_own_theme_directory() {
        settingsfile::testing::with_scratch_config("themes-dir", |root| {
            let dirs = ThemeDirs::standard();
            let expected = settingsfile::testing::scratch_data_dir(root)
                .join("slateos")
                .join("themes");
            assert_eq!(dirs.user, Some(expected));
            assert_eq!(dirs.system, PathBuf::from(SYSTEM_DIR));
        });
    }
}
