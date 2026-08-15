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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use osfont::select::{self, Query};
use osfont::sfnt::{Face, Style};

/// Extensions worth opening. Checked case-insensitively, since Windows font
/// directories are full of `.TTF` and Unix ones of `.ttf`.
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "ttc", "otf", "otc"];

/// How deep to recurse. Font trees nest by foundry or by script
/// (`/usr/share/fonts/truetype/dejavu/`), but not deeply; a bound stops a
/// symlink cycle or a misconfigured path from walking the whole disk.
const MAX_DEPTH: u32 = 6;

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
    /// Weight, slant and width, from the face's own `OS/2` table.
    pub style: Style,
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
        for id in [
            osfont::sfnt::name_id::TYPOGRAPHIC_FAMILY,
            osfont::sfnt::name_id::FAMILY,
        ] {
            if let Some(name) = face.name(id) {
                let name = name.trim().to_lowercase();
                if !name.is_empty() {
                    families.insert(name);
                }
            }
        }
        if families.is_empty() {
            return;
        }
        self.faces.push(FaceInfo {
            path: path.to_path_buf(),
            families: families.into_iter().collect(),
            style: face.style(),
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

    /// Every distinct family name, sorted — what a font picker lists.
    #[must_use]
    pub fn families(&self) -> Vec<String> {
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for face in &self.faces {
            for f in &face.families {
                names.insert(f.as_str());
            }
        }
        names.into_iter().map(str::to_string).collect()
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

/// Whether `path` has an extension worth opening.
fn is_font_file(path: &Path) -> bool {
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
    // Per-user fonts, which on SlateOS and on Unix alike override system ones
    // — hence pushed after, since a later directory's faces sort later and
    // ties go to the earlier candidate. (Overriding by *name* is the caller's
    // business; this only decides which file a tie resolves to.)
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
                style: Style {
                    weight: *weight,
                    italic: *italic,
                    width: *width,
                },
            });
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
        assert_eq!(db.families(), vec!["arial", "arial narrow"]);
        assert!(FontDb::new().families().is_empty());
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
}
