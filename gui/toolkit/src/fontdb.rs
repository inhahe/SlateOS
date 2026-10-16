//! Finding the font file behind a family name.
//!
//! # Why this is in the toolkit and not in `osfont`
//!
//! [`osfont`] does no I/O — it takes bytes and gives back glyphs — because it
//! has to run against the SlateOS VFS as readily as against a host
//! filesystem. Walking directories and reading files is the caller's job, and
//! this is that caller.
//!
//! It lives in the *toolkit* specifically, rather than in each program that
//! needs it, because the compositor depends on the toolkit and both of them
//! need the answer. The toolkit measures text to decide how wide a button is;
//! the compositor draws that text. If they resolved `"Inter"` to two
//! different files — different versions installed in different directories,
//! or the same directories walked in a different order — every centred label
//! in the system would be off by the difference between two fonts' metrics,
//! and nothing in either program would look wrong on its own. One resolver,
//! used by both, is what makes that class of bug impossible rather than
//! unlikely.
//!
//! # What it costs
//!
//! Finding a family means opening font files, because a file's name does not
//! reliably tell you the family inside it (`seguisb.ttf` is Segoe UI
//! Semibold, `ARLRDBD.TTF` is Arial Rounded MT Bold). The scan parses each
//! candidate's tables and keeps the few fields it needs, then drops the file
//! contents — an index of several hundred faces is tens of kilobytes, not the
//! hundreds of megabytes the files themselves occupy.
//!
//! Faces that fail to parse are skipped rather than reported as errors. A
//! font directory is not curated: it accumulates broken files, formats this
//! crate does not read, and the occasional non-font that got the extension by
//! accident. Failing the whole scan because one file is bad would mean one
//! bad font costs the user every font.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use osfont::select::{self, Query};
use osfont::sfnt::{Face, Style};

/// The extensions of the files the index reads ([`is_font_file`]). Checked
/// case-insensitively, since Windows font directories are full of `.TTF` and
/// Unix ones of `.ttf`. Public so a program offering to install a font offers
/// what the index will read, from this one list.
pub const FONT_EXTENSIONS: [&str; 4] = ["ttf", "ttc", "otf", "otc"];

/// How deep to recurse. Font trees nest by foundry or by script
/// (`/usr/share/fonts/truetype/dejavu/`), but not deeply; a bound stops a
/// symlink cycle or a misconfigured path from walking the whole disk.
const MAX_DEPTH: u32 = 6;

/// The weight at which a face counts as bold for [`FontDb::has_bold`]: CSS's
/// semibold. A family whose heaviest upright face is 600 draws its bold text
/// in that face -- CSS matching for 700 takes the heaviest below when nothing
/// is heavier -- so it has a bold.
pub const BOLD_WEIGHT: u16 = 600;

/// The weights `face` draws ([`FaceInfo::weights`]): its `wght` axis's range
/// if it has one, else `weight` -- its `OS/2` weight -- twice.
fn weight_range(face: &Face, weight: u16) -> (u16, u16) {
    face.variation_axes()
        .and_then(|axes| axes.axes().iter().find(|axis| axis.tag == *b"wght"))
        .map_or((weight, weight), |axis| {
            (css_weight(axis.min, weight), css_weight(axis.max, weight))
        })
}

/// An axis value as a CSS weight: rounded and held to 1 to 1000, or
/// `otherwise` for a value that is not a number -- a damaged `fvar` is one
/// face's problem, not a reason to drop it.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded and clamped to 1..=1000 first"
)]
fn css_weight(value: f32, otherwise: u16) -> u16 {
    if value.is_finite() {
        value.round().clamp(1.0, 1000.0) as u16
    } else {
        otherwise
    }
}

/// One installed face, as much of it as selection needs.
///
/// Deliberately does not hold the file contents. The point of the index is to
/// answer "which file?" cheaply enough to build at startup; keeping the bytes
/// would make it as expensive as loading every font on the system.
#[derive(Clone, Debug)]
pub struct FaceInfo {
    /// The file this face came from.
    pub path: PathBuf,
    /// Every name this face answers to, lowercased for matching.
    ///
    /// Plural because a face has up to two family names and they are both
    /// legitimate answers. `arialn.ttf` calls itself "Arial Narrow" in the
    /// legacy name and "Arial" in the typographic one; a user who types
    /// either should find it. Which of the two a *request* means is settled
    /// by width class, not by the name.
    pub families: Vec<String>,
    /// The same names as the face spells them, for showing to a person.
    ///
    /// [`families`](Self::families) is lower-cased because it is the *key* an
    /// index is searched by, and a test pins that: "the index is what the rest
    /// of the system trusts". But a font picker that listed `arial` and
    /// `segoe ui` would look broken, and there is nowhere to recover the
    /// capitals from afterwards short of reopening every font. So the spelling
    /// is kept here at scan time, when the name table is already open.
    ///
    /// Not necessarily the same length as `families`: two spellings that
    /// differ only in case are one name to the index and one entry here.
    pub display: Vec<String>,
    /// The family a font menu groups this face under, as the face spells it:
    /// its typographic family (name id 16) where it has one, else its legacy
    /// family (name id 1). Noto Sans's files call themselves "Noto Sans"
    /// typographically and "Noto Sans SemiBold", "Noto Sans Light" and so on
    /// in the legacy name, which named one style each; grouped by those, a
    /// list shows a family per style. The face still answers to both
    /// ([`families`](Self::families)): a document naming "Noto Sans SemiBold"
    /// still finds it.
    pub family: String,
    /// Weight, slant and width, from the face's own `OS/2` table.
    pub style: Style,
    /// The lightest and the heaviest weight the face draws, on the CSS
    /// scale: its `OS/2` weight twice for a face of one weight, its `wght`
    /// axis's range for a variable one -- Open Sans's single file draws 300
    /// to 800.
    ///
    /// Kept beside [`style`](Self::style) because the two answer different
    /// questions: a variable file's `OS/2` weight is its default instance's,
    /// 400, which is right for choosing the file for regular text and wrong
    /// for "can this family be drawn bold" ([`FontDb::has_bold`]).
    pub weights: (u16, u16),
    /// Whether every glyph advances by the same width.
    ///
    /// Recorded here rather than asked for later because the scan already
    /// parses each face to read the two fields above -- it discards the
    /// *bytes*, not the parse -- so this costs one bool per face. Asking
    /// afterwards would mean opening every font on the system a second time,
    /// which is the expense this struct's own doc comment exists to avoid.
    pub monospaced: bool,
}

impl FaceInfo {
    /// Whether this face answers to `family` (case-insensitively).
    #[must_use]
    pub fn matches(&self, family: &str) -> bool {
        let wanted = family.trim().to_lowercase();
        self.families.contains(&wanted)
    }
}

/// An index of the faces installed on this system.
#[derive(Clone, Debug, Default)]
pub struct FontDb {
    faces: Vec<FaceInfo>,
}

impl FontDb {
    /// An empty index, for a caller that will add directories itself.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Index every font in the system's usual font directories.
    #[must_use]
    pub fn scan_system() -> Self {
        let mut db = Self::new();
        for dir in system_font_dirs() {
            db.scan_dir(&dir);
        }
        db.finish();
        db
    }

    /// Index every font under `dir`, recursively.
    ///
    /// Call [`finish`](Self::finish) once after the last directory.
    pub fn scan_dir(&mut self, dir: &Path) {
        self.walk(dir, 0);
    }

    /// Put the index in a defined order.
    ///
    /// Directory iteration order is not specified and differs between
    /// filesystems, so without this the face chosen from a set of equally
    /// good candidates could change between runs — and the toolkit and the
    /// compositor, scanning independently, could disagree. Sorting by path
    /// makes the tie-break in [`select::best`] deterministic.
    pub fn finish(&mut self) {
        self.faces.sort_by(|a, b| a.path.cmp(&b.path));
    }

    fn walk(&mut self, dir: &Path, depth: u32) {
        if depth > MAX_DEPTH {
            return;
        }
        // A font directory that does not exist is the normal case on any
        // given platform — this probes several — so it is not an error.
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // `file_type` rather than `path.is_dir()`: the latter follows
            // symlinks, and a link pointing at an ancestor would recurse
            // until the depth bound caught it.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                self.walk(&path, depth.saturating_add(1));
            } else if is_font_file(&path) {
                self.add_file(&path);
            }
        }
    }

    /// Parse one file and record what it contains, if anything usable.
    fn add_file(&mut self, path: &Path) {
        let Ok(data) = fs::read(path) else { return };
        let Ok(face) = Face::parse(data) else { return };
        // A face with no family name cannot be asked for by name, so it has
        // no place in an index keyed by name. It is still perfectly
        // renderable if a caller finds it some other way.
        let mut families = BTreeSet::new();
        let mut display = BTreeSet::new();
        // The first name found, in this order: the typographic one.
        let mut family = None;
        for id in [
            osfont::sfnt::name_id::TYPOGRAPHIC_FAMILY,
            osfont::sfnt::name_id::FAMILY,
        ] {
            if let Some(name) = face.name(id) {
                let name = name.trim();
                if !name.is_empty() {
                    families.insert(name.to_lowercase());
                    display.insert(name.to_string());
                    family.get_or_insert_with(|| name.to_string());
                }
            }
        }
        let Some(family) = family else {
            return;
        };
        let style = face.style();
        self.faces.push(FaceInfo {
            path: path.to_path_buf(),
            families: families.into_iter().collect(),
            display: display.into_iter().collect(),
            family,
            weights: weight_range(&face, style.weight),
            style,
            monospaced: face.is_monospaced(),
        });
    }

    /// Every face indexed.
    #[must_use]
    pub fn faces(&self) -> &[FaceInfo] {
        &self.faces
    }

    /// How many faces are indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Every family, sorted -- what a font picker lists: each face's
    /// [`family`](FaceInfo::family), the name a font menu groups it under,
    /// not each name it answers to, so Noto Sans is one entry and not one per
    /// style. A face's other name still finds it ([`find`](Self::find)).
    #[must_use]
    pub fn families(&self) -> Vec<String> {
        Self::spellings(self.faces.iter())
    }

    /// The families of `faces`, one per name, sorted.
    ///
    /// Keyed on the lower-cased name and valued by the spelling, so that two
    /// faces disagreeing about capitalisation ("DejaVu Sans" and "DejaVu
    /// sans") become one entry rather than two that look like duplicates to a
    /// reader and are duplicates to nobody else. First spelling seen wins,
    /// which is stable because `finish` sorts the index by path.
    ///
    /// Ordering comes from the key, so the list sorts case-insensitively --
    /// what a picker wants, and not what sorting the spellings would give
    /// (every capitalised name before every lower-case one).
    fn spellings<'a>(faces: impl Iterator<Item = &'a FaceInfo>) -> Vec<String> {
        let mut seen: BTreeMap<String, &str> = BTreeMap::new();
        for face in faces {
            seen.entry(face.family.to_lowercase())
                .or_insert(face.family.as_str());
        }
        seen.into_values().map(str::to_string).collect()
    }

    /// The families with at least one fixed-pitch face.
    ///
    /// What a "Terminal Font" picker lists, as distinct from [`families`] for
    /// the interface one. Before this existed nothing could make the
    /// distinction, and `guitk::text::set_mono_family`'s own note records the
    /// consequence: it installs whatever it is handed, so a caller pointing it
    /// at a proportional face "gets a terminal with a broken grid, and that is
    /// the caller's decision to have made". This is how a caller stops making
    /// that decision by accident.
    ///
    /// *At least one* face rather than all of them: a family's bold or italic
    /// member sometimes omits the declaration its regular member makes, and
    /// dropping the whole family over one under-described face would hide
    /// fonts a terminal can use perfectly well.
    ///
    /// [`families`]: Self::families
    #[must_use]
    pub fn monospaced_families(&self) -> Vec<String> {
        Self::spellings(self.faces.iter().filter(|f| f.monospaced))
    }

    /// Whether a face answers to `family`, whatever its capitals -- whether
    /// [`find`](Self::find) has a file to give. What a choice among families
    /// asks before choosing: a theme's recommendations, tried in order.
    #[must_use]
    pub fn has_family(&self, family: &str) -> bool {
        self.faces.iter().any(|face| face.matches(family))
    }

    /// Whether `family` has a fixed-pitch face, as
    /// [`monospaced_families`](Self::monospaced_families) counts one: whether
    /// a terminal drawn in it keeps its grid. `false` for a family that is
    /// not installed.
    #[must_use]
    pub fn is_fixed_pitch(&self, family: &str) -> bool {
        self.faces
            .iter()
            .any(|face| face.monospaced && face.matches(family))
    }

    /// Whether `family` has an upright face that draws bold -- a bold file,
    /// or a variable one whose weight axis reaches [`BOLD_WEIGHT`]. What a
    /// font picker asks before offering "Bold": a family without one draws
    /// its bold text in its regular face, since nothing here emboldens a
    /// face that has no bold. `false` for a family that is not installed.
    ///
    /// Upright only: the render tree draws no italic, so an italic-only bold
    /// would never be drawn.
    #[must_use]
    pub fn has_bold(&self, family: &str) -> bool {
        let wanted = family.trim().to_lowercase();
        self.faces.iter().any(|face| {
            !face.style.italic && face.weights.1 >= BOLD_WEIGHT && face.families.contains(&wanted)
        })
    }

    /// The best file for `family` at `want`, or `None` if the family is not
    /// installed.
    ///
    /// Returns `None` only when *nothing* in the family matched. A family
    /// that exists always yields a file, however badly it fits the query,
    /// because the alternative is drawing no text — see [`select`].
    #[must_use]
    pub fn find(&self, family: &str, want: Query) -> Option<&FaceInfo> {
        let matching = self.faces.iter().filter(|f| f.matches(family));
        select::best(matching, want, |f| f.style)
    }

    /// Read the best file for `family` at `want` and parse it.
    ///
    /// # Errors
    ///
    /// [`LoadError::NotInstalled`] if no indexed face answers to `family`,
    /// [`LoadError::Unreadable`] if the file it chose could not be read, and
    /// [`LoadError::Unparsable`] if it could be read but not parsed.
    ///
    /// The three are distinguished because they call for different responses:
    /// the first is a configuration mistake to report to the user, and the
    /// other two mean the font directory changed underneath the index and a
    /// rescan may fix it.
    pub fn load(&self, family: &str, want: Query) -> Result<Face, LoadError> {
        let info = self.find(family, want).ok_or(LoadError::NotInstalled)?;
        let data = fs::read(&info.path).map_err(|e| LoadError::Unreadable {
            path: info.path.clone(),
            source: e,
        })?;
        Face::parse(data).map_err(|e| LoadError::Unparsable {
            path: info.path.clone(),
            source: e,
        })
    }
}

/// Why a family could not be turned into a face.
#[derive(Debug)]
pub enum LoadError {
    /// No indexed face answers to that family name.
    NotInstalled,
    /// The chosen file could not be read.
    Unreadable {
        /// The file the index pointed at.
        path: PathBuf,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// The chosen file was read but is not a font this crate can open.
    Unparsable {
        /// The file the index pointed at.
        path: PathBuf,
        /// Why the parse failed.
        source: osfont::sfnt::SfntError,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => write!(f, "no such font family is installed"),
            Self::Unreadable { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::Unparsable { path, source } => {
                write!(f, "cannot parse {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotInstalled => None,
            Self::Unreadable { source, .. } => Some(source),
            Self::Unparsable { source, .. } => Some(source),
        }
    }
}

/// Whether `path` is a file the index reads: one of [`FONT_EXTENSIONS`].
#[must_use]
pub fn is_font_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        FONT_EXTENSIONS
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

/// Where fonts live.
///
/// SlateOS's own path comes first so that a system font always wins over a
/// host one when this is built for development. The rest are the host paths,
/// present so that the toolkit renders correctly when run on a development
/// machine — without them, every test and every locally-run app falls back to
/// the built-in bitmap face and looks nothing like the real thing.
#[must_use]
pub fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/fonts")];
    // Per-user fonts. The order of these folders decides nothing about which
    // file wins a tie between two equally good faces: `FontDb::finish` sorts
    // the faces by path, and the tie goes to the path that sorts first -- one
    // rule, so the toolkit and the compositor, scanning apart, agree. That
    // happens to put the user's face first on every system this runs on
    // (`/home` before `/usr`; `C:\Users` before `C:\Windows`), which is the
    // winner wanted, but by the paths, not by this list. (Overriding by *name*
    // is the caller's business; this only decides which file a tie resolves
    // to.)
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join(".local/share/fonts"));
        dirs.push(Path::new(&home).join(".fonts"));
    }
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(Path::new(&windir).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(Path::new(&local).join("Microsoft/Windows/Fonts"));
    }
    dirs.push(PathBuf::from("/System/Library/Fonts"));
    dirs.push(PathBuf::from("/Library/Fonts"));
    dirs.retain(|d| d.is_dir());
    dirs
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// An index built by hand, so the tests do not depend on what is
    /// installed.
    fn db(entries: &[(&str, &[&str], u16, bool, u8)]) -> FontDb {
        let mut db = FontDb::new();
        for (path, families, weight, italic, width) in entries {
            db.faces.push(FaceInfo {
                path: PathBuf::from(path),
                families: families.iter().map(|f| f.to_lowercase()).collect(),
                display: families.iter().map(|f| (*f).to_string()).collect(),
                // The first name given is the typographic one, as a fixture
                // lists them.
                family: families
                    .first()
                    .map(|f| (*f).to_string())
                    .unwrap_or_default(),
                style: Style {
                    weight: *weight,
                    italic: *italic,
                    width: *width,
                },
                // One weight, as a static file draws; a test of a variable
                // face sets the range itself.
                weights: (*weight, *weight),
                // Proportional unless a test says otherwise, so the existing
                // entries keep their shape. `db_mono` is how a test says
                // otherwise.
                monospaced: false,
            });
        }
        db.finish();
        db
    }

    /// An index whose every face is fixed-pitch.
    ///
    /// Separate from [`db`] rather than a sixth tuple field, so that the
    /// twenty-odd existing fixtures do not all have to grow a `false` to say
    /// something they were never about.
    fn db_mono(entries: &[(&str, &[&str], u16, bool, u8)]) -> FontDb {
        let mut db = db(entries);
        for face in &mut db.faces {
            face.monospaced = true;
        }
        db.finish();
        db
    }

    /// The four-file family every desktop has.
    fn arial() -> FontDb {
        db(&[
            ("arial.ttf", &["arial"], 400, false, 5),
            ("arialbd.ttf", &["arial"], 700, false, 5),
            ("ariali.ttf", &["arial"], 400, true, 5),
            ("arialbi.ttf", &["arial"], 700, true, 5),
            // The condensed member, which calls itself "Arial" typographically
            // and "Arial Narrow" legacy — the case width class exists for.
            ("arialn.ttf", &["arial", "arial narrow"], 400, false, 3),
        ])
    }

    fn found(db: &FontDb, family: &str, want: Query) -> String {
        db.find(family, want)
            .expect("family is in the index")
            .path
            .display()
            .to_string()
    }

    #[test]
    fn a_family_resolves_to_the_file_for_the_style_asked_for() {
        let db = arial();
        assert_eq!(found(&db, "Arial", Query::regular()), "arial.ttf");
        assert_eq!(found(&db, "Arial", Query::bold()), "arialbd.ttf");
        assert_eq!(found(&db, "Arial", Query::regular().italic()), "ariali.ttf");
        assert_eq!(found(&db, "Arial", Query::bold().italic()), "arialbi.ttf");
    }

    #[test]
    fn matching_a_family_name_ignores_case_and_surrounding_space() {
        // A family name comes from a settings file a human typed into.
        let db = arial();
        for spelling in ["arial", "ARIAL", "Arial", "  Arial  ", "aRiAl"] {
            assert_eq!(found(&db, spelling, Query::regular()), "arial.ttf");
        }
    }

    #[test]
    fn a_request_for_a_family_is_not_answered_with_its_condensed_member() {
        // Arial Narrow answers to "Arial" typographically, so it is a genuine
        // candidate; width class is the only thing keeping it from being
        // chosen. Without that check, a UI asking for Arial would get a face
        // that measures noticeably narrower.
        assert_eq!(found(&arial(), "Arial", Query::regular()), "arial.ttf");
    }

    #[test]
    fn the_condensed_member_can_still_be_asked_for_by_its_own_name() {
        let db = arial();
        assert_eq!(found(&db, "Arial Narrow", Query::regular()), "arialn.ttf");
        // And by "Arial" plus a condensed width, which is what a style
        // selector rather than a name would ask.
        let narrow = Query {
            weight: 400,
            italic: false,
            width: 3,
        };
        assert_eq!(found(&db, "Arial", narrow), "arialn.ttf");
    }

    #[test]
    fn a_family_with_only_one_file_answers_every_query() {
        // The common case for a downloaded font. Refusing would mean the UI
        // draws nothing rather than drawing the one face that exists.
        let db = db(&[("solo.ttf", &["solo"], 400, false, 5)]);
        for want in [
            Query::regular(),
            Query::bold(),
            Query::regular().italic(),
            Query::bold().italic(),
        ] {
            assert_eq!(found(&db, "Solo", want), "solo.ttf");
        }
    }

    #[test]
    fn a_family_that_is_not_installed_is_reported_as_such() {
        let db = arial();
        assert!(db.find("Comic Sans MS", Query::regular()).is_none());
        assert!(db.find("", Query::regular()).is_none());
        assert!(FontDb::new().find("Arial", Query::regular()).is_none());
    }

    #[test]
    fn resolution_does_not_depend_on_directory_order() {
        // Two runs that discovered the same files in different orders must
        // choose the same file, or the toolkit and the compositor — which
        // scan independently — can disagree about what "Inter" means.
        let forward = db(&[
            ("a/x.ttf", &["dup"], 400, false, 5),
            ("b/y.ttf", &["dup"], 400, false, 5),
        ]);
        let mut reverse = FontDb::new();
        reverse.faces = forward.faces.clone();
        reverse.faces.reverse();
        reverse.finish();
        assert_eq!(
            found(&forward, "dup", Query::regular()),
            found(&reverse, "dup", Query::regular())
        );
    }

    #[test]
    fn the_family_list_is_deduplicated_and_sorted() {
        let db = arial();
        assert_eq!(db.families(), vec!["arial"]);
        assert!(FontDb::new().families().is_empty());
    }

    /// **A family is listed once, by the name a font menu groups it under**:
    /// Noto Sans's styles, each also calling itself by a legacy name of one
    /// style, are one entry -- and each still answers to its other name.
    #[test]
    fn a_family_is_listed_by_its_typographic_name_and_found_by_either() {
        let db = db(&[
            ("NotoSans-Regular.ttf", &["Noto Sans"], 400, false, 5),
            (
                "NotoSans-SemiBold.ttf",
                &["Noto Sans", "Noto Sans SemiBold"],
                600,
                false,
                5,
            ),
            (
                "NotoSans-Light.ttf",
                &["Noto Sans", "Noto Sans Light"],
                300,
                false,
                5,
            ),
            ("Solo.ttf", &["Solo"], 400, false, 5),
        ]);
        assert_eq!(db.families(), vec!["Noto Sans", "Solo"]);
        assert_eq!(
            found(&db, "Noto Sans SemiBold", Query::regular()),
            "NotoSans-SemiBold.ttf"
        );
    }

    /// **The files the index reads are the extensions it lists**, in any case.
    #[test]
    fn a_font_file_is_one_of_the_listed_extensions_in_any_case() {
        for name in ["a.ttf", "a.TTF", "a.otc", "dir/b.Otf", "c.ttc"] {
            assert!(is_font_file(Path::new(name)), "{name}");
        }
        for name in ["a.woff2", "a.txt", "ttf", "a.ttf.bak"] {
            assert!(!is_font_file(Path::new(name)), "{name}");
        }
    }

    /// The list a picker shows keeps the font's own capitals.
    ///
    /// The index lower-cases for matching and a neighbouring test pins that;
    /// this pins the other half. Without it the change is invisible to the
    /// suite: every other fixture here spells its families in lower case
    /// already, so they pass whether or not the spelling survives -- which is
    /// exactly how a picker came to list `arial` and `segoe ui` with a green
    /// suite.
    #[test]
    fn the_family_list_keeps_the_fonts_own_capitals() {
        let db = db(&[
            ("s.ttf", &["Segoe UI"], 400, false, 5),
            ("d.ttf", &["DejaVu Sans"], 400, false, 5),
        ]);
        assert_eq!(db.families(), vec!["DejaVu Sans", "Segoe UI"]);

        // The match key is untouched, so lookups still work and the invariant
        // the other test asserts still holds.
        assert!(
            db.faces()
                .iter()
                .all(|f| f.families.iter().all(|n| *n == n.to_lowercase())),
            "the match key stopped being lower-cased"
        );
        assert!(
            db.faces().iter().any(|f| f.matches("SEGOE ui")),
            "a differently-cased request stopped matching"
        );
    }

    /// Two faces spelling one family differently are one entry, not two.
    ///
    /// A picker showing "DejaVu Sans" above "DejaVu sans" looks like two fonts
    /// and is one, and the index has always treated them as one.
    #[test]
    fn spellings_that_differ_only_in_case_collapse() {
        let db = db(&[
            ("a.ttf", &["DejaVu Sans"], 400, false, 5),
            ("b.ttf", &["DejaVu sans"], 700, false, 5),
        ]);
        assert_eq!(db.families().len(), 1, "got {:?}", db.families());
    }

    /// A terminal picker is offered fixed-pitch families and nothing else.
    ///
    /// The same entries twice, differing only in the flag, so what is asserted
    /// is the flag rather than the fixture: a `monospaced_families` that
    /// forgot to filter would return `consolas` from both halves and pass a
    /// test that only checked the first.
    #[test]
    fn only_fixed_pitch_families_are_offered_to_a_terminal() {
        let entries: &[(&str, &[&str], u16, bool, u8)] = &[("c.ttf", &["consolas"], 400, false, 5)];

        assert_eq!(db_mono(entries).monospaced_families(), vec!["consolas"]);
        assert!(
            db(entries).monospaced_families().is_empty(),
            "a proportional face was offered as a terminal font"
        );

        // The ordinary list is the same either way: this filters a picker, it
        // does not remove a family from the system.
        assert_eq!(db(entries).families(), vec!["consolas"]);
        assert_eq!(db_mono(entries).families(), vec!["consolas"]);
    }

    /// **A family is installed when a face answers to it**, whatever its
    /// capitals, **and fixed-pitch when one of its faces is** -- the same
    /// entries twice, differing in the flag alone, so it is the flag being
    /// read and not the name.
    #[test]
    fn a_family_is_installed_and_fixed_pitch_as_its_faces_say() {
        let entries: &[(&str, &[&str], u16, bool, u8)] = &[("c.ttf", &["Consolas"], 400, false, 5)];
        let proportional = db(entries);
        assert!(proportional.has_family("Consolas"));
        assert!(
            proportional.has_family("cONSOLAS"),
            "names match in any case"
        );
        assert!(!proportional.has_family("Inter"));
        assert!(!proportional.is_fixed_pitch("Consolas"));

        let fixed = db_mono(entries);
        assert!(fixed.is_fixed_pitch("consolas"));
        assert!(
            !fixed.is_fixed_pitch("Inter"),
            "a family not installed is not fixed-pitch"
        );
        assert!(!FontDb::new().has_family("Consolas"));
    }

    // ---- the fallback faces (`crate::text::DEFAULT_FALLBACK_FAMILIES`) ----

    /// A regular face of each of `families`, and nothing else.
    fn installed(families: &[&str]) -> FontDb {
        let entries: Vec<(String, Vec<&str>)> = families
            .iter()
            .map(|family| (format!("{family}.ttf"), vec![*family]))
            .collect();
        let rows: Vec<(&str, &[&str], u16, bool, u8)> = entries
            .iter()
            .map(|(path, names)| (path.as_str(), names.as_slice(), 400, false, 5))
            .collect();
        db(&rows)
    }

    /// **Each group gives the first of its members that is installed, and
    /// the groups keep their order** -- broad coverage, then emoji, then
    /// symbols, then scripts -- whatever order the faces were found in.
    #[test]
    fn the_fallbacks_are_the_first_installed_member_of_each_group_in_order() {
        let db = installed(&[
            "Noto Sans Hebrew",
            "Segoe UI Emoji",
            "Symbola",
            "DejaVu Sans",
            "Noto Sans",
        ]);
        assert_eq!(
            crate::text::resolve_fallback_families(&db),
            ["Noto Sans", "Segoe UI Emoji", "Symbola", "Noto Sans Hebrew"]
        );
    }

    /// The OS's own emoji face is preferred to a host's, and one face per
    /// group is used even when more are installed: two emoji faces would be
    /// two faces parsed in every process for characters the first draws.
    #[test]
    fn one_face_answers_for_each_group() {
        let db = installed(&["Segoe UI Emoji", "Noto Color Emoji", "Twemoji"]);
        assert_eq!(
            crate::text::resolve_fallback_families(&db),
            ["Noto Color Emoji"]
        );
    }

    /// One CJK face serves all four regional forms -- each is tens of
    /// megabytes -- and the first listed of those installed is the one.
    #[test]
    fn one_cjk_face_serves_all_four() {
        let db = installed(&["Noto Sans CJK KR", "Noto Sans CJK JP"]);
        assert_eq!(
            crate::text::resolve_fallback_families(&db),
            ["Noto Sans CJK JP"]
        );
    }

    /// Nothing installed is no fallbacks -- characters the UI face lacks stay
    /// boxes, as they were -- and a family not on the list is never used.
    #[test]
    fn only_listed_installed_families_are_fallbacks() {
        assert!(crate::text::resolve_fallback_families(&FontDb::new()).is_empty());
        let db = installed(&["Comic Sans MS", "Wingdings"]);
        assert!(crate::text::resolve_fallback_families(&db).is_empty());
    }

    /// Names are matched as the rest of the index matches them, whatever
    /// capitals the font file uses.
    #[test]
    fn a_fallback_is_found_whatever_its_capitals() {
        let db = installed(&["noto color emoji", "NOTO SANS"]);
        assert_eq!(
            crate::text::resolve_fallback_families(&db),
            ["Noto Sans", "Noto Color Emoji"]
        );
    }

    #[test]
    fn an_empty_index_reports_itself_empty() {
        let db = FontDb::new();
        assert!(db.is_empty());
        assert_eq!(db.len(), 0);
        assert!(db.faces().is_empty());
        assert_eq!(arial().len(), 5);
        assert!(!arial().is_empty());
    }

    #[test]
    fn only_font_extensions_are_opened() {
        // A font directory holds licence files, `.uninstall` markers and
        // whatever else; opening and parsing each would waste the scan.
        for good in ["a.ttf", "a.TTF", "a.otf", "a.ttc", "a.OTC", "dir/b.Ttf"] {
            assert!(is_font_file(Path::new(good)), "{good} should be indexed");
        }
        for bad in ["a.txt", "a", "a.ttf.bak", "fonts.dir", ".ttf/x"] {
            assert!(!is_font_file(Path::new(bad)), "{bad} should be skipped");
        }
    }

    #[test]
    fn scanning_a_directory_that_is_not_there_is_not_an_error() {
        // Several of the probed paths are absent on any given platform.
        let mut db = FontDb::new();
        db.scan_dir(Path::new("/no/such/font/directory"));
        db.finish();
        assert!(db.is_empty());
    }

    /// Scan the development host's real font directories.
    ///
    /// Everything above runs against a hand-built index, which proves the
    /// matching rules but not that the *scan* produces an index at all — that
    /// the directories are the right ones, that the extensions are the right
    /// ones, that a real file's family names come out where the matcher looks
    /// for them. This is `#[ignore]`d because it depends on what is installed;
    /// run it deliberately:
    ///
    /// ```text
    /// cargo test -p guitk --target x86_64-pc-windows-gnu fontdb -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "depends on the host's installed fonts"]
    fn the_host_system_scans_into_a_usable_index() {
        let db = FontDb::scan_system();
        println!("directories: {:?}", system_font_dirs());
        println!("faces: {}, families: {}", db.len(), db.families().len());
        assert!(!db.is_empty(), "no fonts found in {:?}", system_font_dirs());

        // Whatever is installed, asking for a family that is there must give
        // back a file that is there and that parses.
        let mut checked = 0usize;
        for family in ["Arial", "Segoe UI", "DejaVu Sans", "Times New Roman"] {
            let Some(info) = db.find(family, Query::regular()) else {
                continue;
            };
            println!("{family} regular -> {}", info.path.display());
            let face = db.load(family, Query::regular()).expect("load the file");
            assert!(
                face.family()
                    .is_some_and(|f| f.eq_ignore_ascii_case(family)
                        || info.families.contains(&f.to_lowercase())),
                "{family} resolved to {:?}, which calls itself {:?}",
                info.path,
                face.family()
            );
            // The bold of a family must be a different file from its regular,
            // wherever the family ships one — resolving both to the same file
            // is how a UI ends up with no bold text at all.
            if let Some(bold) = db.find(family, Query::bold()) {
                println!("{family} bold    -> {}", bold.path.display());
                assert!(
                    bold.style.weight >= info.style.weight,
                    "{family}: bold resolved lighter than regular"
                );
            }
            checked += 1;
        }
        assert!(
            checked > 0,
            "none of the well-known families are installed — the scan was \
             never checked against a known answer"
        );

        // Every indexed face must be loadable, since the index is what the
        // rest of the system trusts.
        for face in db.faces().iter().take(50) {
            assert!(face.path.is_file(), "{} is not a file", face.path.display());
            assert!(!face.families.is_empty());
            assert!(
                face.families.iter().all(|f| *f == f.to_lowercase()),
                "{:?} was indexed with a name that is not lowercased",
                face.families
            );
        }
    }

    #[test]
    fn a_load_failure_says_which_of_the_three_it_was() {
        // The index can point at a file that has since gone, and the three
        // failures call for different responses.
        let db = db(&[("/no/such/file.ttf", &["ghost"], 400, false, 5)]);
        assert!(matches!(
            db.load("nothing", Query::regular()),
            Err(LoadError::NotInstalled)
        ));
        let err = db
            .load("ghost", Query::regular())
            .expect_err("file is absent");
        assert!(matches!(err, LoadError::Unreadable { .. }), "{err}");
        // The message names the file, so a user can act on it.
        assert!(err.to_string().contains("file.ttf"), "{err}");
    }

    /// **A family has a bold when an upright face of it reaches semibold** --
    /// a bold file, or a variable one whose axis does -- whatever the
    /// capitals asked with; not for an italic-only bold, nor a family that is
    /// not installed.
    #[test]
    fn a_family_has_a_bold_when_an_upright_face_reaches_semibold() {
        let mut db = db(&[
            ("arial.ttf", &["Arial"], 400, false, 5),
            ("arialbd.ttf", &["Arial"], 700, false, 5),
            ("semi.ttf", &["Semi"], 400, false, 5),
            ("semisb.ttf", &["Semi"], 600, false, 5),
            ("light.ttf", &["Light Only"], 300, false, 5),
            ("medium.ttf", &["Light Only"], 500, false, 5),
            ("slanted.ttf", &["Slanted"], 400, false, 5),
            ("slantedbi.ttf", &["Slanted"], 700, true, 5),
            ("open.ttf", &["Open Variable"], 400, false, 5),
            ("thin.ttf", &["Thin Variable"], 400, false, 5),
        ]);
        for face in &mut db.faces {
            if face.path.ends_with("open.ttf") {
                face.weights = (300, 800);
            }
            if face.path.ends_with("thin.ttf") {
                face.weights = (100, 500);
            }
        }
        assert!(db.has_bold("Arial"));
        assert!(db.has_bold(" arial "), "trimmed, any capitals");
        assert!(db.has_bold("Semi"), "a semibold is the bold");
        assert!(!db.has_bold("Light Only"), "500 is not bold");
        assert!(!db.has_bold("Slanted"), "its only bold is italic");
        assert!(db.has_bold("Open Variable"), "the axis reaches bold");
        assert!(!db.has_bold("Thin Variable"), "the axis stops short");
        assert!(!db.has_bold("Nowhere"));
    }

    /// **A face's weights are its axis's range when it is variable, and its
    /// own weight otherwise** -- read from files the host has, when it has
    /// them: Windows' Bahnschrift is one variable file, 300 to 700.
    #[test]
    fn a_variable_face_draws_its_axis_range() {
        let fonts = std::env::var_os("WINDIR").map(|w| Path::new(&w).join("Fonts"));
        let Some(fonts) = fonts.filter(|d| d.is_dir()) else {
            return;
        };
        let mut db = FontDb::new();
        for file in ["bahnschrift.ttf", "arial.ttf"] {
            db.add_file(&fonts.join(file));
        }
        for face in db.faces() {
            if face.matches("Bahnschrift") {
                assert_eq!(face.weights, (300, 700), "{}", face.path.display());
                assert_eq!(face.style.weight, 400, "the default instance's");
            } else {
                assert_eq!(face.weights, (face.style.weight, face.style.weight));
            }
        }
        if db.faces().iter().any(|f| f.matches("Bahnschrift")) {
            assert!(db.has_bold("Bahnschrift"));
        }
    }

    /// **An axis value becomes a CSS weight**, rounded and held to 1 to
    /// 1000, and one that is not a number gives way to the face's own.
    #[test]
    fn an_axis_value_becomes_a_css_weight() {
        assert_eq!(css_weight(299.6, 400), 300);
        assert_eq!(css_weight(0.0, 400), 1);
        assert_eq!(css_weight(5000.0, 400), 1000);
        assert_eq!(css_weight(f32::NAN, 400), 400);
        assert_eq!(css_weight(f32::INFINITY, 700), 700);
    }
}
