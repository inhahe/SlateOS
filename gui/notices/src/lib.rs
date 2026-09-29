//! The third-party notices this system carries (design-decisions §1433).
//!
//! Code copied or translated from other people's projects is ours to ship only
//! if their licence notices ship with it -- the BSD licences ask for their
//! notice "in the documentation and/or other materials provided with the
//! distribution", MIT for its notice "in all copies or substantial portions",
//! and libjpeg-turbo's for one exact sentence. `scripts/gather-notices.py`
//! collects every such notice the source tree carries into one folder when the
//! image is built, [`SYSTEM_DIR`]: the ported libraries a
//! `licenses/notices.yaml` names, the vendored Rust crates, and the crates.io
//! libraries. This crate reads that folder, for whatever screen shows the
//! notices to a person.
//!
//! # The folder
//!
//! `index.yaml` lists the notices in order; each one's texts sit beside it,
//! under its key:
//!
//! ```text
//! notices:
//!   libjpeg-turbo-3.1.1:
//!     component: 'libjpeg-turbo'
//!     version: '3.1.1'
//!     licence: 'IJG AND BSD-3-Clause AND Zlib'
//!     attribution: 'This software is based in part on the work of the Independent JPEG Group.'
//!     from: 'gui/imagecodec'
//!     texts:
//!       - 'libjpeg-turbo-3.1.1/libjpeg-turbo-LICENSE.md'
//! ```
//!
//! `attribution` is a sentence a licence requires to be *shown*, word for
//! word; a screen listing the notices must display it, not bury it in the
//! text. The folder also holds `NOTICES.txt`, everything in one file, for a
//! person reading without a screen.
//!
//! # Why a folder read at run time, and not text compiled in
//!
//! The image carries code no single program links -- the video decoder is in
//! the video player, the crates.io libraries are wherever they are used -- so
//! a list compiled into the program that shows it would cover that program and
//! not the system. §1433 has the alternatives.
//!
//! # What this refuses
//!
//! A text path that leaves the folder -- absolute, or climbing out with `..` --
//! is an error, not a file to read. The index is data, and a program that
//! shows licences must not be steerable into showing any other file.

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use yamldoc::Document;

/// Where the notices are in a SlateOS image.
pub const SYSTEM_DIR: &str = "/usr/share/licenses";

/// The list, inside the notices folder.
pub const INDEX: &str = "index.yaml";

/// One component whose notice the system carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Unique within the folder, and the name of the directory its texts are
    /// in: the component and its version, lower-cased (`libjpeg-turbo-3.1.1`).
    pub key: String,
    /// The component's own name (`libjpeg-turbo`).
    pub component: String,
    /// Its version, when the source names one.
    pub version: Option<String>,
    /// The licence it is used under, as its source states it -- an SPDX
    /// expression (`MIT OR Apache-2.0`) for a Rust crate.
    pub licence: String,
    /// A sentence the licence requires to be shown, word for word.
    pub attribution: Option<String>,
    /// Where in the source tree it came from (`gui/imagecodec`), or
    /// `crates.io`.
    pub origin: String,
    /// The texts that must travel with it, in the order given.
    pub texts: Vec<Text>,
}

impl Notice {
    /// The heading a list shows: `libjpeg-turbo 3.1.1`, or the name alone.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.version {
            Some(version) => format!("{} {version}", self.component),
            None => self.component.clone(),
        }
    }
}

/// One licence text of a notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    /// The file's name (`LICENSE-MIT`), which is how a person tells two texts
    /// of one notice apart.
    pub name: String,
    /// Where the text is.
    pub path: PathBuf,
}

impl Text {
    /// The text, byte for byte as its authors wrote it.
    ///
    /// Bytes rather than a `String`: a licence text is someone else's file,
    /// shown as it is, and nothing here decides what to do with a byte that is
    /// not UTF-8 -- the screen showing it does, visibly.
    ///
    /// # Errors
    ///
    /// [`NoticesError::Unreadable`] if the file cannot be read.
    pub fn read(&self) -> Result<Vec<u8>, NoticesError> {
        std::fs::read(&self.path).map_err(|source| NoticesError::Unreadable {
            path: self.path.clone(),
            source,
        })
    }
}

/// Why the notices could not be read.
#[derive(Debug)]
pub enum NoticesError {
    /// The folder has no index: this system was built without its notices.
    NotInstalled {
        /// The folder that was looked in.
        dir: PathBuf,
    },
    /// A file the notices need could not be read.
    Unreadable {
        /// The file.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The index says something this reader cannot make sense of.
    Malformed {
        /// The index.
        path: PathBuf,
        /// What is wrong with it, naming the entry.
        what: String,
    },
}

impl fmt::Display for NoticesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled { dir } => write!(
                f,
                "the third-party notices are not installed: {} has no {INDEX}",
                dir.display()
            ),
            Self::Unreadable { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::Malformed { path, what } => write!(f, "{}: {what}", path.display()),
        }
    }
}

impl std::error::Error for NoticesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } => Some(source),
            Self::NotInstalled { .. } | Self::Malformed { .. } => None,
        }
    }
}

/// The notices in `dir`, in the index's order.
///
/// The texts are not read here -- a list of forty components does not need
/// forty licences in memory -- so a missing text is reported when it is read
/// ([`Text::read`]), not by refusing every other notice.
///
/// # Errors
///
/// [`NoticesError::NotInstalled`] if `dir` has no index;
/// [`NoticesError::Unreadable`] if the index cannot be read;
/// [`NoticesError::Malformed`] if an entry lacks a required field, has a key
/// that is not a plain name, or names a text outside `dir`.
pub fn load(dir: &Path) -> Result<Vec<Notice>, NoticesError> {
    let index = dir.join(INDEX);
    let bytes = match std::fs::read(&index) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(NoticesError::NotInstalled {
                dir: dir.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(NoticesError::Unreadable {
                path: index,
                source,
            });
        }
    };
    let malformed = |what: String| NoticesError::Malformed {
        path: index.clone(),
        what,
    };
    let text = String::from_utf8(bytes).map_err(|e| malformed(format!("not UTF-8 ({e})")))?;
    let doc = Document::parse(&text);
    if !doc.contains(&["notices"]) {
        return Err(malformed("no `notices:` list".to_owned()));
    }
    doc.keys(&["notices"])
        .into_iter()
        .map(|key| entry(&doc, dir, &key).map_err(|what| malformed(format!("{key}: {what}"))))
        .collect()
}

/// One entry of the index, or what is wrong with it.
fn entry(doc: &Document, dir: &Path, key: &str) -> Result<Notice, String> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
    {
        return Err("a key is lower-case letters, digits, `-` and `.`".to_owned());
    }
    let field = |name: &str| {
        doc.get_str(&["notices", key, name])
            .filter(|v| !v.is_empty())
    };
    let required = |name: &str| field(name).ok_or_else(|| format!("no `{name}`"));
    let texts = doc
        .get_seq(&["notices", key, "texts"])
        .unwrap_or_default()
        .into_iter()
        .map(|rel| text_in(dir, &rel))
        .collect::<Result<Vec<_>, _>>()?;
    if texts.is_empty() {
        return Err("no `texts`".to_owned());
    }
    Ok(Notice {
        key: key.to_owned(),
        component: required("component")?,
        version: field("version"),
        licence: required("licence")?,
        attribution: field("attribution"),
        origin: required("from")?,
        texts,
    })
}

/// `rel`, inside `dir`, or why not.
fn text_in(dir: &Path, rel: &str) -> Result<Text, String> {
    let path = Path::new(rel);
    let inside = !rel.is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)));
    let name = path.file_name().and_then(|n| n.to_str());
    match (inside, name) {
        (true, Some(name)) => Ok(Text {
            name: name.to_owned(),
            path: dir.join(path),
        }),
        _ => Err(format!(
            "the text {rel:?} is not a path inside the notices folder"
        )),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    /// A folder holding `index` as its index.
    fn bundle(tag: &str, index: &str) -> ScratchDir {
        let dir = ScratchDir::new(&format!("notices-{tag}"));
        std::fs::write(dir.path(INDEX), index).expect("write the index");
        dir
    }

    const ONE: &str = "notices:\n  spin-0.9.8:\n    component: 'spin'\n    version: '0.9.8'\n    \
                       licence: 'MIT'\n    from: 'crates.io'\n    texts:\n      - 'spin-0.9.8/LICENSE'\n";

    #[test]
    fn a_folder_without_an_index_is_not_installed_rather_than_empty() {
        let dir = ScratchDir::new("notices-none");
        match load(dir.dir()) {
            Err(NoticesError::NotInstalled { dir: at }) => assert_eq!(at, dir.dir()),
            other => panic!("expected NotInstalled, got {other:?}"),
        }
    }

    #[test]
    fn an_index_with_no_entries_is_a_system_with_nothing_to_credit() {
        let dir = bundle("empty", "notices:\n");
        assert_eq!(
            load(dir.dir()).expect("an empty list is a list"),
            Vec::new()
        );
    }

    #[test]
    fn a_text_path_is_resolved_inside_the_folder_and_named_by_its_file() {
        let dir = bundle("one", ONE);
        let notices = load(dir.dir()).expect("load");
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title(), "spin 0.9.8");
        assert_eq!(
            notices[0].texts,
            vec![Text {
                name: "LICENSE".to_owned(),
                path: dir.dir().join("spin-0.9.8").join("LICENSE"),
            }]
        );
    }

    #[test]
    fn a_missing_text_is_reported_when_read_and_does_not_hide_the_list() {
        let dir = bundle("missing", ONE);
        let notices = load(dir.dir()).expect("the list loads without its texts");
        match notices[0].texts[0].read() {
            Err(NoticesError::Unreadable { path, .. }) => {
                assert_eq!(path, dir.dir().join("spin-0.9.8").join("LICENSE"));
            }
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn a_text_outside_the_folder_is_refused_not_read() {
        for escape in [
            "../../etc/passwd",
            "/etc/passwd",
            "spin-0.9.8/../../x",
            "./LICENSE",
            "",
        ] {
            let index = ONE.replace("spin-0.9.8/LICENSE", escape);
            let dir = bundle("escape", &index);
            match load(dir.dir()) {
                Err(NoticesError::Malformed { what, .. }) => {
                    assert!(what.contains("not a path inside"), "{escape:?}: {what}");
                }
                other => panic!("{escape:?}: expected Malformed, got {other:?}"),
            }
        }
    }

    #[test]
    fn every_required_field_is_required() {
        for (field, line) in [
            ("component", "    component: 'spin'\n"),
            ("licence", "    licence: 'MIT'\n"),
            ("from", "    from: 'crates.io'\n"),
        ] {
            let dir = bundle("required", &ONE.replace(line, ""));
            match load(dir.dir()) {
                Err(NoticesError::Malformed { what, .. }) => {
                    assert_eq!(what, format!("spin-0.9.8: no `{field}`"));
                }
                other => panic!("without {field}: expected Malformed, got {other:?}"),
            }
        }
        let dir = bundle(
            "no-texts",
            &ONE.replace("      - 'spin-0.9.8/LICENSE'\n", ""),
        );
        assert!(
            matches!(load(dir.dir()), Err(NoticesError::Malformed { what, .. }) if what.ends_with("no `texts`")),
            "a notice with no text to show"
        );
    }

    #[test]
    fn the_optional_fields_are_optional() {
        let dir = bundle("optional", &ONE.replace("    version: '0.9.8'\n", ""));
        let notices = load(dir.dir()).expect("load");
        assert_eq!(notices[0].version, None);
        assert_eq!(notices[0].attribution, None);
        assert_eq!(notices[0].title(), "spin");
    }

    #[test]
    fn a_key_that_could_name_another_directory_is_refused() {
        for key in ["Spin", "spin/../x", "spin 0.9"] {
            let dir = bundle("key", &ONE.replace("spin-0.9.8:", &format!("'{key}':")));
            assert!(
                matches!(load(dir.dir()), Err(NoticesError::Malformed { .. })),
                "{key:?} was accepted as a key"
            );
        }
    }

    #[test]
    fn an_index_that_is_not_utf8_or_has_no_list_is_malformed() {
        let dir = ScratchDir::new("notices-bytes");
        std::fs::write(dir.path(INDEX), b"notices:\n  \xff:\n").expect("write");
        assert!(matches!(
            load(dir.dir()),
            Err(NoticesError::Malformed { .. })
        ));
        let dir = bundle("no-list", "credits:\n");
        assert!(
            matches!(load(dir.dir()), Err(NoticesError::Malformed { what, .. }) if what == "no `notices:` list")
        );
    }
}
