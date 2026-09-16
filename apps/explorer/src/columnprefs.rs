//! Which detail columns the user chose, remembered between sessions.
//!
//! Two scopes, as `roadmap-detailed.md` §4.1 specifies: a default used for any
//! folder without a preference of its own, and a per-folder override saved
//! against the folder's path.
//!
//! # Columns are named, never numbered
//!
//! Every entry here is a [`ColumnDef::key`] -- a stable string that is never
//! displayed and never renumbered. `ColumnId` is a position in a hand-written
//! list, so a file full of integers would silently mean different columns the
//! first time anyone inserted one; `label` is display text that the column
//! table does not even populate until runtime. A saved preference has to
//! survive both, so it names columns the way this file does.
//!
//! An unknown key is **skipped, not guessed at**. A preference written by a
//! later version, or naming a column since removed, loses that one column and
//! keeps the rest, rather than shifting every later column along by one.

use std::path::Path;
use yamldoc::Document;

/// The settings group these live in.
pub const CONFIG_NAME: &str = "explorer";

/// The default column set, used by any folder with no preference of its own.
const GLOBAL: [&str; 2] = ["columns", "default"];

/// The mapping of folder path to column set.
const PER_FOLDER: [&str; 2] = ["columns", "folders"];

/// The saved default column set, if the user has saved one.
#[must_use]
pub fn global(doc: &Document) -> Option<Vec<String>> {
    doc.get_seq(&GLOBAL)
}

/// Save `keys` as the default for folders with no preference of their own.
pub fn set_global(doc: &mut Document, keys: &[&str]) {
    doc.set_seq(&GLOBAL, keys);
}

/// The column set saved for `folder`, if any.
///
/// `None` for a folder with no preference *and* for one whose path cannot be
/// written down -- see [`set_for_folder`].
#[must_use]
pub fn for_folder(doc: &Document, folder: &Path) -> Option<Vec<String>> {
    let key = folder.to_str()?;
    doc.get_seq(&[PER_FOLDER[0], PER_FOLDER[1], key])
}

/// Save `keys` for `folder`. Answers whether it could be written down.
///
/// `false` when the folder's path is not UTF-8. Our paths may hold any byte
/// but `/` and NUL, and this file is YAML, whose keys are text -- so such a
/// folder cannot have a preference saved against it. Answering `false` lets
/// the caller say so; inventing a key by lossy conversion would save the
/// preference against a *different* folder, and the user would find their
/// choice applied to a directory they never configured.
pub fn set_for_folder(doc: &mut Document, folder: &Path, keys: &[&str]) -> bool {
    let Some(key) = folder.to_str() else {
        return false;
    };
    doc.set_seq(&[PER_FOLDER[0], PER_FOLDER[1], key], keys);
    true
}

/// The thumbnail size the user chose, in pixels.
const THUMB_SIZE: [&str; 2] = ["thumbnails", "size"];

/// The sizes offered, smallest first.
///
/// A fixed list rather than a free number, for the reason the screen-lock
/// delays are a list: the value is a choice the user makes from a few sensible
/// options, and a text field would invite "300px" and "huge" and other things
/// this would then have to refuse. `DiskCache` keys its entries on this, so a
/// new size misses the cache and regenerates rather than serving one made at
/// the old size.
pub const THUMB_SIZES: [u32; 4] = [64, 96, 128, 192];

/// The saved thumbnail size, if the user has chosen one.
///
/// A size that is not one of [`THUMB_SIZES`] answers `None` rather than being
/// honoured: the list is what the menu can show a tick beside, and a value
/// from outside it would leave every row unticked with no way to tell why.
#[must_use]
pub fn thumb_size(doc: &Document) -> Option<u32> {
    let saved = doc.get_i64(&THUMB_SIZE)?;
    let saved = u32::try_from(saved).ok()?;
    THUMB_SIZES.contains(&saved).then_some(saved)
}

/// Remember `size` as the thumbnail size.
pub fn set_thumb_size(doc: &mut Document, size: u32) {
    doc.set_i64(&THUMB_SIZE, i64::from(size));
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that indexes out of range should fail loudly and point at the line that did it"
)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_size_round_trips() {
        let mut doc = Document::new();
        assert_eq!(thumb_size(&doc), None, "an unset machine claimed a size");
        set_thumb_size(&mut doc, 192);
        assert_eq!(thumb_size(&doc), Some(192));
    }

    /// A size outside the offered list is not honoured.
    ///
    /// The menu ticks the row matching the saved size, so a value from outside
    /// the list would leave every row unticked and the user unable to see what
    /// is in force. Falling back to the default is the honest answer.
    #[test]
    fn a_size_we_do_not_offer_is_ignored() {
        let mut doc = Document::new();
        doc.set_i64(&THUMB_SIZE, 300);
        assert_eq!(thumb_size(&doc), None);
        doc.set_i64(&THUMB_SIZE, -64);
        assert_eq!(thumb_size(&doc), None, "a negative size was honoured");
    }

    #[test]
    fn a_default_set_round_trips() {
        let mut doc = Document::new();
        set_global(&mut doc, &["name", "size", "date_modified"]);
        assert_eq!(
            global(&doc).as_deref(),
            Some(
                ["name", "size", "date_modified"]
                    .map(String::from)
                    .as_slice()
            )
        );
    }

    #[test]
    fn a_folder_keeps_its_own_set_without_disturbing_the_default() {
        let mut doc = Document::new();
        set_global(&mut doc, &["name", "size"]);
        assert!(set_for_folder(
            &mut doc,
            Path::new("/home/me/pictures"),
            &["name", "dimensions"]
        ));

        assert_eq!(
            for_folder(&doc, Path::new("/home/me/pictures")).as_deref(),
            Some(["name", "dimensions"].map(String::from).as_slice())
        );
        assert_eq!(
            global(&doc).as_deref(),
            Some(["name", "size"].map(String::from).as_slice()),
            "saving a folder's columns changed the default"
        );
        assert!(
            for_folder(&doc, Path::new("/home/me/music")).is_none(),
            "a folder with no preference claimed one"
        );
    }

    /// A folder whose path cannot be written down is refused, not mangled.
    ///
    /// Windows-only because that is where a non-UTF-8 path is constructible.
    /// The point is not platform-specific: a lossy key would save the choice
    /// against a different folder, and the user would meet their preference in
    /// a directory they never configured.
    #[cfg(windows)]
    #[test]
    fn a_folder_that_cannot_be_named_is_refused() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::path::PathBuf;

        let mut doc = Document::new();
        let folder = PathBuf::from(OsString::from_wide(&[0xD800_u16]));
        assert!(folder.to_str().is_none(), "the fixture is not the case");
        assert!(
            !set_for_folder(&mut doc, &folder, &["name"]),
            "a path with no text form was written down anyway"
        );
        assert!(for_folder(&doc, &folder).is_none());
    }
}
