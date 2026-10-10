//! The fonts on this machine, as the Font Manager shows them.
//!
//! Every family the toolkit's font index finds -- the very index every
//! program draws from (`guitk::fontdb`), walked over the same folders -- with
//! its faces, which of them are the user's own, a font file installed into
//! the user's own fonts folder, and the user's own fonts removed.
//!
//! # Whose fonts
//!
//! A face is the user's own when its file is in their fonts folder
//! ([`FontPlaces::own`], `~/.local/share/fonts`): what an install writes, and
//! the only fonts this program removes. The system's are SlateOS's, shared by
//! every account, and are not this program's to delete.
//!
//! # When programs see a change
//!
//! Each program builds its font index once, when it starts (`guitk::text`'s
//! `font_db`), so a font installed or removed here is seen by programs started
//! afterwards -- the page that says so is the window's.
//!
//! Until 2026-10-10 this program listed nineteen fonts written into its
//! source, whatever the machine had, and its Install added a line to that list
//! and copied no file (known issue
//! `E-the-font-manager-lists-invented-fonts-and-installs-nothing`).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use guitk::fontdb::FontDb;
use osfont::select::Query;
use osfont::sfnt::{Face, Style, name_id};

/// The largest font file installed, in bytes. A font runs to a few
/// megabytes, a collection of a CJK family to some tens; a file past this is
/// not a font anyone meant to install, and is not read into memory to find
/// out.
pub const MAX_FONT_BYTES: usize = 128 * 1024 * 1024;

/// The extensions of the files the font index reads, as it checks them: in
/// any case.
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];

/// How many names an install tries -- `Inter.ttf`, `Inter (2).ttf`, ... --
/// before it gives up: a folder holding ninety-nine files of one name is not
/// one an install should add a hundredth to.
const NAME_TRIES: u32 = 99;

/// Where fonts are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontPlaces {
    /// Every folder walked for fonts, in the index's order.
    pub dirs: Vec<PathBuf>,
    /// The user's own fonts folder: where an install writes, and the only
    /// folder a font is removed from. `None` when there is no home folder.
    pub own: Option<PathBuf>,
}

impl FontPlaces {
    /// The folders every program's index walks
    /// (`guitk::fontdb::system_font_dirs`), and the user's own,
    /// `~/.local/share/fonts` -- walked whether or not it exists yet, since
    /// an install makes it.
    #[must_use]
    pub fn standard() -> Self {
        let own = std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share/fonts"));
        let mut dirs = guitk::fontdb::system_font_dirs();
        if let Some(own) = &own
            && !dirs.contains(own)
        {
            dirs.push(own.clone());
        }
        Self { dirs, own }
    }
}

/// One face of a family: one file's, in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceEntry {
    /// The file it is in.
    pub path: PathBuf,
    /// Its weight, width and slant.
    pub style: Style,
    /// Whether every glyph advances the same width.
    pub monospaced: bool,
    /// Whether it is the user's own: its file is in their fonts folder.
    pub own: bool,
}

/// A family, as the index names it, and its faces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    /// Its name, as the font spells it.
    pub name: String,
    /// Its faces, in the index's order.
    pub faces: Vec<FaceEntry>,
}

impl Family {
    /// Whether it has a fixed-pitch face: what a terminal can be drawn in.
    #[must_use]
    pub fn is_fixed_pitch(&self) -> bool {
        self.faces.iter().any(|face| face.monospaced)
    }

    /// Whether any of its faces are the user's own.
    #[must_use]
    pub fn has_own(&self) -> bool {
        self.faces.iter().any(|face| face.own)
    }

    /// Whether every one of its faces is the user's own.
    #[must_use]
    pub fn all_own(&self) -> bool {
        !self.faces.is_empty() && self.faces.iter().all(|face| face.own)
    }

    /// Its user's own files, each once: what removing it deletes.
    #[must_use]
    pub fn own_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        for face in self.faces.iter().filter(|face| face.own) {
            if !files.contains(&face.path) {
                files.push(face.path.clone());
            }
        }
        files
    }

    /// Its styles by name, in weight order, each once: "Regular", "Bold
    /// Italic".
    #[must_use]
    pub fn style_names(&self) -> Vec<String> {
        let mut styles: Vec<Style> = Vec::new();
        for face in &self.faces {
            if !styles.contains(&face.style) {
                styles.push(face.style);
            }
        }
        styles.sort_by_key(|style| (style.width, style.weight, style.italic));
        styles.into_iter().map(style_name).collect()
    }
}

/// A face's style in words: its width if not normal, its weight, and
/// whether it is italic -- "Regular", "Italic", "Bold", "Condensed Light
/// Italic".
#[must_use]
pub fn style_name(style: Style) -> String {
    let width = match style.width {
        1 => Some("Ultra Condensed"),
        2 => Some("Extra Condensed"),
        3 => Some("Condensed"),
        4 => Some("Semi Condensed"),
        6 => Some("Semi Expanded"),
        7 => Some("Expanded"),
        8 => Some("Extra Expanded"),
        9 => Some("Ultra Expanded"),
        _ => None,
    };
    let weight = match style.weight {
        0..=149 => "Thin",
        150..=249 => "Extra Light",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "Semibold",
        650..=749 => "Bold",
        750..=849 => "Extra Bold",
        _ => "Black",
    };
    let mut words: Vec<&str> = Vec::new();
    words.extend(width);
    // "Italic" alone, not "Regular Italic"; "Condensed", not "Condensed
    // Regular": regular is what is left unsaid when anything else is said.
    if weight != "Regular" || (width.is_none() && !style.italic) {
        words.push(weight);
    }
    if style.italic {
        words.push("Italic");
    }
    words.join(" ")
}

/// A font file installed: the family it is, and where it now lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The family, as the file names it.
    pub family: String,
    /// Its style, in words.
    pub style: String,
    /// Where the file was written, in the user's fonts folder.
    pub path: PathBuf,
}

/// Why a font file was not installed.
#[derive(Debug)]
pub enum InstallError {
    /// There is no home folder, and so no fonts folder of the user's.
    NoFontsFolder,
    /// The file's name is not a font file's: not `.ttf`, `.otf`, `.ttc` or
    /// `.otc`.
    NotAFontName,
    /// The file could not be read.
    Read(io::Error),
    /// It is larger than [`MAX_FONT_BYTES`].
    TooLarge,
    /// It could be read, and is not a font this system can draw.
    NotAFont,
    /// It is a font, and names no family to be found by.
    NoFamilyName,
    /// The user has this family in this style already.
    AlreadyInstalled {
        /// The family.
        family: String,
        /// The style, in words.
        style: String,
    },
    /// It could not be written into the fonts folder.
    Write(io::Error),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFontsFolder => write!(f, "there is no home folder to keep your fonts in"),
            Self::NotAFontName => {
                write!(
                    f,
                    "it is not a font file: a font's name ends .ttf, .otf, .ttc or .otc"
                )
            }
            Self::Read(e) => write!(f, "it could not be read: {e}"),
            Self::TooLarge => write!(
                f,
                "it is larger than {} MB, more than any font",
                MAX_FONT_BYTES / (1024 * 1024)
            ),
            Self::NotAFont => write!(f, "it is not a font this system can draw"),
            Self::NoFamilyName => write!(f, "the font names no family to be found by"),
            Self::AlreadyInstalled { family, style } => {
                write!(f, "you have {family} {style} installed already")
            }
            Self::Write(e) => write!(f, "it could not be written to your fonts folder: {e}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Why a family's fonts were not removed, or not all of them.
#[derive(Debug)]
pub enum RemoveError {
    /// No family of that name is installed.
    NotInstalled,
    /// None of its fonts is the user's: they are the system's.
    SystemFonts,
    /// Some of its files could not be deleted: how many were, and the first
    /// failure.
    Delete {
        /// How many files were deleted before or after the failure.
        deleted: usize,
        /// The file that was not.
        path: PathBuf,
        /// Why.
        error: io::Error,
    },
}

impl fmt::Display for RemoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => write!(f, "no such family is installed"),
            Self::SystemFonts => write!(
                f,
                "its fonts are the system's, shared by every account, not yours to remove"
            ),
            Self::Delete {
                deleted,
                path,
                error,
            } => write!(
                f,
                "{} could not be deleted ({error}); {deleted} other file{} {} deleted",
                pathtext::ShowPath::shown(path.as_path()),
                if *deleted == 1 { "" } else { "s" },
                if *deleted == 1 { "was" } else { "were" }
            ),
        }
    }
}

impl std::error::Error for RemoveError {}

/// The fonts on this machine: the index, and its families.
pub struct Library {
    places: FontPlaces,
    index: FontDb,
    families: Vec<Family>,
}

impl Library {
    /// The fonts in `places`, read now.
    #[must_use]
    pub fn scan(places: FontPlaces) -> Self {
        let mut library = Self {
            places,
            index: FontDb::new(),
            families: Vec::new(),
        };
        library.rescan();
        library
    }

    /// Read the folders again: after an install or a removal, or when the
    /// user asks.
    pub fn rescan(&mut self) {
        let mut index = FontDb::new();
        for dir in &self.places.dirs {
            index.scan_dir(dir);
        }
        index.finish();
        let own = self.places.own.as_deref();
        self.families = index
            .families()
            .into_iter()
            .map(|name| Family {
                faces: index
                    .faces()
                    .iter()
                    .filter(|face| face.matches(&name))
                    .map(|face| FaceEntry {
                        path: face.path.clone(),
                        style: face.style,
                        monospaced: face.monospaced,
                        own: own.is_some_and(|own| face.path.starts_with(own)),
                    })
                    .collect(),
                name,
            })
            .collect();
        self.index = index;
    }

    /// Every family, sorted by name in any case.
    #[must_use]
    pub fn families(&self) -> &[Family] {
        &self.families
    }

    /// The family named `name`, in any case.
    #[must_use]
    pub fn family(&self, name: &str) -> Option<&Family> {
        self.families
            .iter()
            .find(|family| family.name.eq_ignore_ascii_case(name))
    }

    /// The upright, regular-weight face of `family` -- or its nearest -- read
    /// and parsed, for a preview; `None` if it cannot be.
    #[must_use]
    pub fn load(&self, family: &str) -> Option<Face> {
        self.index.load(family, Query::regular()).ok()
    }

    /// Every other family with a face in one of `files`: what deleting them
    /// takes away besides `family`, since a collection file holds several.
    #[must_use]
    pub fn sharing(&self, family: &str, files: &[PathBuf]) -> Vec<String> {
        self.families
            .iter()
            .filter(|other| !other.name.eq_ignore_ascii_case(family))
            .filter(|other| other.faces.iter().any(|face| files.contains(&face.path)))
            .map(|other| other.name.clone())
            .collect()
    }

    /// Install the font file at `from` into the user's fonts folder, and read
    /// the folders again.
    ///
    /// Read, and parsed as the index parses a font, before anything is
    /// written: a file that is not a font is refused rather than left in the
    /// folder for every program to skip. Written under its own name -- or
    /// `Name (2).ttf` where that is taken -- and never over another file.
    ///
    /// # Errors
    ///
    /// [`InstallError`]: no fonts folder, not a font file, unreadable, too
    /// large, not a font, no family name, the user's already, or not written.
    pub fn install(&mut self, from: &Path) -> Result<Installed, InstallError> {
        let own = self.places.own.clone().ok_or(InstallError::NoFontsFolder)?;
        let name = from
            .file_name()
            .filter(|_| is_font_name(from))
            .ok_or(InstallError::NotAFontName)?;
        let read = safeio::read_capped(from, MAX_FONT_BYTES).map_err(InstallError::Read)?;
        if read.truncated {
            return Err(InstallError::TooLarge);
        }
        let face = Face::parse(read.bytes.clone()).map_err(|_| InstallError::NotAFont)?;
        let names = family_names(&face);
        let family = face
            .family()
            .or_else(|| names.first().cloned())
            .ok_or(InstallError::NoFamilyName)?;
        let style = face.style();
        let already = self.families.iter().any(|installed| {
            names
                .iter()
                .any(|name| installed.name.eq_ignore_ascii_case(name))
                && installed
                    .faces
                    .iter()
                    .any(|face| face.own && face.style == style)
        });
        if already {
            return Err(InstallError::AlreadyInstalled {
                family,
                style: style_name(style),
            });
        }
        std::fs::create_dir_all(&own).map_err(InstallError::Write)?;
        let path = write_beside(&own, Path::new(name), &read.bytes)?;
        self.rescan();
        Ok(Installed {
            family,
            style: style_name(style),
            path,
        })
    }

    /// Delete the user's own files of `family` and read the folders again:
    /// how many were deleted. A family that is partly the system's keeps its
    /// system faces.
    ///
    /// Every file is tried, a failure not stopping the rest, and the first
    /// failure reported with how many were deleted.
    ///
    /// # Errors
    ///
    /// [`RemoveError`]: no such family, its fonts all the system's, or a file
    /// not deleted.
    pub fn remove(&mut self, family: &str) -> Result<usize, RemoveError> {
        let files = self
            .family(family)
            .ok_or(RemoveError::NotInstalled)?
            .own_files();
        if files.is_empty() {
            return Err(RemoveError::SystemFonts);
        }
        let mut deleted = 0_usize;
        let mut failed: Option<(PathBuf, io::Error)> = None;
        for file in files {
            match std::fs::remove_file(&file) {
                Ok(()) => deleted = deleted.saturating_add(1),
                Err(error) => {
                    if failed.is_none() {
                        failed = Some((file, error));
                    }
                }
            }
        }
        self.rescan();
        match failed {
            None => Ok(deleted),
            Some((path, error)) => Err(RemoveError::Delete {
                deleted,
                path,
                error,
            }),
        }
    }
}

/// Whether `path` names a font file, as the index decides which files to
/// open: by its extension, in any case.
#[must_use]
pub fn is_font_name(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        FONT_EXTENSIONS
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

/// The family names a face answers to: its typographic family and its
/// legacy one, as the index keys it.
fn family_names(face: &Face) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for id in [name_id::TYPOGRAPHIC_FAMILY, name_id::FAMILY] {
        if let Some(name) = face.name(id) {
            let name = name.trim().to_owned();
            if !name.is_empty() && !names.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
                names.push(name);
            }
        }
    }
    names
}

/// Write `bytes` into `dir` as `name`, or the first of `name (2)`, `name
/// (3)`, ... that is free: never over a file. Where it was written.
fn write_beside(dir: &Path, name: &Path, bytes: &[u8]) -> Result<PathBuf, InstallError> {
    let stem = name.file_stem().unwrap_or(name.as_os_str());
    let extension = name.extension();
    for n in 1..=NAME_TRIES {
        let mut file = stem.to_os_string();
        if n > 1 {
            file.push(format!(" ({n})"));
        }
        if let Some(extension) = extension {
            file.push(".");
            file.push(extension);
        }
        let path = dir.join(file);
        match safeio::write_new_atomically(&path, bytes) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(InstallError::Write(e)),
        }
    }
    Err(InstallError::Write(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{NAME_TRIES} files of that name are there already"),
    )))
}

#[cfg(test)]
#[path = "library_tests.rs"]
pub(crate) mod tests;
