//! A hand-arranged order for the files in one folder.
//!
//! `roadmap-detailed.md` §4.1: *"The user can drag files/folders into an
//! arbitrary order within a directory and that ordering is remembered — it
//! persists across navigations, refreshes, and reboots (stored per-directory
//! in user-side state, keyed by directory identity, not baked into the
//! filesystem)."*
//!
//! Stored in the same `explorer` settings document as
//! [`crate::columnprefs`], under its own key, because it is the same kind of
//! thing -- a per-folder view preference -- and a second file would be a
//! second thing to keep in step for no gain.
//!
//! # The precedence rule, which is the part that is easy to get wrong
//!
//! The spec is explicit that a column sort **temporarily overrides** a manual
//! order without discarding it: *"switching back to Custom restores the user's
//! hand-arranged sequence intact"*. That falls out of where the two live
//! rather than from any code that has to remember it: the arrangement is on
//! disk against the folder, and the sort mode is view state. Sorting by Name
//! does not write anything, so the arrangement is still there when Custom
//! comes back.
//!
//! This is worth stating because the obvious implementation -- reordering
//! `entries` in place and calling that the arrangement -- loses it the first
//! time anything else sorts, and would look correct until the user tried
//! precisely the thing the spec calls out.
//!
//! # What cannot be stored, and why it is not silently wrong
//!
//! The arrangement is a list of names, and this file is YAML, whose scalars
//! are text. A file whose name is not valid UTF-8 therefore cannot have its
//! position written down -- the same wall [`crate::columnprefs::set_for_folder`]
//! meets for the folder's own path, and the subject of **C-Q24** in
//! `open-questions.md`.
//!
//! What this does about it is the honest option rather than the tidy one: such
//! a file is **left out of the saved order and sorts with the newcomers**, at
//! the end, by name. It is never dropped from the listing, and its name is
//! never guessed at by a lossy conversion -- which would file its position
//! under a *different* name and move some other file instead.

use std::ffi::OsStr;
use std::path::Path;
use yamldoc::Document;

/// Where the per-folder arrangements live in the settings document.
const ORDER: [&str; 2] = ["manual_order", "folders"];

/// The saved arrangement for `folder`, as names in the order chosen.
///
/// `None` for a folder that has never been arranged, and for one whose path
/// cannot be written down -- the two are the same answer here, because neither
/// can produce an arrangement to apply.
#[must_use]
pub fn for_folder(doc: &Document, folder: &Path) -> Option<Vec<String>> {
    let key = folder.to_str()?;
    doc.get_seq(&[ORDER[0], ORDER[1], key])
}

/// Save `names` as the arrangement for `folder`. Answers whether it was saved.
///
/// `false` when the folder's path is not UTF-8, for the reason
/// [`crate::columnprefs::set_for_folder`] gives at length: inventing a key by
/// lossy conversion would save the arrangement against a *different* folder,
/// and the user would find their arrangement applied somewhere they never
/// touched.
pub fn set_for_folder(doc: &mut Document, folder: &Path, names: &[&str]) -> bool {
    let Some(key) = folder.to_str() else {
        return false;
    };
    doc.set_seq(&[ORDER[0], ORDER[1], key], names);
    true
}

/// Where `name` sits in `order`, or `None` if it is not in it.
///
/// Takes an `OsStr` rather than a `&str` so the caller does not have to decode
/// first: a name with no text form is simply in no saved position, which is
/// the right answer and needs no conversion to reach.
#[must_use]
pub fn position_of(order: &[String], name: &OsStr) -> Option<usize> {
    let text = name.to_str()?;
    order.iter().position(|n| n == text)
}

/// Compare two entries by their place in `order`.
///
/// Anything in the arrangement comes before anything that is not, and the
/// newcomers keep whatever order the caller had them in -- which is the
/// "appended, then draggable" the spec asks for, since the caller sorts by
/// name first.
#[must_use]
pub fn compare(order: &[String], a: &OsStr, b: &OsStr) -> core::cmp::Ordering {
    match (position_of(order, a), position_of(order, b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        // Arranged before unarranged: a file the user placed should not be
        // pushed below one they have never seen.
        (Some(_), None) => core::cmp::Ordering::Less,
        (None, Some(_)) => core::cmp::Ordering::Greater,
        (None, None) => core::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that indexes out of range should fail loudly at the line that did it"
)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doc() -> Document {
        Document::new()
    }

    /// An arrangement survives the round trip.
    #[test]
    fn an_arrangement_round_trips() {
        let mut d = doc();
        let folder = PathBuf::from("/home/user/pics");
        assert!(set_for_folder(
            &mut d,
            &folder,
            &["c.png", "a.png", "b.png"]
        ));
        assert_eq!(
            for_folder(&d, &folder),
            Some(vec![
                "c.png".to_string(),
                "a.png".to_string(),
                "b.png".to_string()
            ])
        );
    }

    /// Two folders keep separate arrangements.
    #[test]
    fn arrangements_are_per_folder() {
        let mut d = doc();
        let one = PathBuf::from("/a");
        let two = PathBuf::from("/b");
        set_for_folder(&mut d, &one, &["x"]);
        set_for_folder(&mut d, &two, &["y"]);
        assert_eq!(for_folder(&d, &one), Some(vec!["x".to_string()]));
        assert_eq!(for_folder(&d, &two), Some(vec!["y".to_string()]));
    }

    /// A folder nobody has arranged has no arrangement.
    #[test]
    fn an_unarranged_folder_has_none() {
        let d = doc();
        assert_eq!(for_folder(&d, Path::new("/never/touched")), None);
    }

    /// The arranged come before the unarranged.
    #[test]
    fn arranged_files_sort_before_new_ones() {
        let order = vec!["b.txt".to_string(), "a.txt".to_string()];
        assert_eq!(
            compare(&order, OsStr::new("b.txt"), OsStr::new("a.txt")),
            core::cmp::Ordering::Less,
            "the arrangement is not being honoured"
        );
        assert_eq!(
            compare(&order, OsStr::new("a.txt"), OsStr::new("new.txt")),
            core::cmp::Ordering::Less,
            "a newcomer displaced an arranged file"
        );
        assert_eq!(
            compare(&order, OsStr::new("new.txt"), OsStr::new("a.txt")),
            core::cmp::Ordering::Greater
        );
    }

    /// Two newcomers tie, so the caller's existing order decides.
    ///
    /// That is what makes "appended, then draggable" come out in name order
    /// rather than in whatever order the filesystem listed them.
    #[test]
    fn two_newcomers_tie() {
        let order = vec!["a".to_string()];
        assert_eq!(
            compare(&order, OsStr::new("y"), OsStr::new("z")),
            core::cmp::Ordering::Equal
        );
    }

    /// A name with no text form is a newcomer rather than an error.
    ///
    /// It cannot be written into a YAML list, so it can never be *in* the
    /// arrangement; sorting it with the newcomers is the only answer that
    /// neither drops it from the listing nor guesses at its name.
    #[cfg(windows)]
    #[test]
    fn a_name_that_is_not_text_sorts_with_the_newcomers() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let odd = OsString::from_wide(&[u16::from(b'z'), 0xD800]);
        assert!(odd.to_str().is_none(), "the fixture decodes");

        let order = vec!["a.txt".to_string()];
        assert_eq!(position_of(&order, &odd), None);
        assert_eq!(
            compare(&order, OsStr::new("a.txt"), &odd),
            core::cmp::Ordering::Less,
            "an arranged file lost its place to one that cannot be arranged"
        );
    }

    /// A folder whose path is not text cannot be arranged, and says so.
    #[cfg(windows)]
    #[test]
    fn a_folder_that_cannot_be_written_down_reports_false() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let mut d = doc();
        let folder = PathBuf::from(OsString::from_wide(&[
            u16::from(b'/'),
            u16::from(b'z'),
            0xD800,
        ]));
        assert!(
            !set_for_folder(&mut d, &folder, &["x"]),
            "claimed to save an arrangement it cannot key"
        );
        assert_eq!(for_folder(&d, &folder), None);
    }
}
