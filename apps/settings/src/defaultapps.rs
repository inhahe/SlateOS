//! Which program opens which kind of file, read from the file the file
//! manager actually obeys.
//!
//! # Why this page exists and `StartupApps` does not
//!
//! The test both pages were put to is the same: *does a consumer exist that
//! makes the displayed claim true?* `StartupApps` fails it -- nothing in this
//! tree launches a startup entry, so a row saying "Enabled - Session" would
//! promise a program runs when nothing runs it (see the comment on the
//! placeholder arm in `main.rs`).
//!
//! This page passes it. `apps/explorer`'s `open_entry` looks the extension up
//! in exactly the file read here and spawns what it finds:
//!
//! ```text
//! settingsfile::load("fileassoc") -> ["associations", <ext>] -> a program path
//!   -> Command::new(program).arg(path).spawn()
//! ```
//!
//! So a row here reports a real setting with a real effect: change it and the
//! next double-click in the file manager runs a different program.
//!
//! # The name of the file is duplicated, deliberately
//!
//! [`ASSOC_CONFIG_NAME`] repeats a constant `apps/explorer` also declares.
//! Neither can import the other's: `apps/fileassoc`, which *writes* the file,
//! is a binary with nothing to link against, and explorer's own comment gives
//! that as the reason its association records a runnable path rather than an
//! application id. Three crates therefore agree on one string by convention,
//! which is worth stating plainly rather than leaving to be discovered: if
//! this string and explorer's ever drift, this page will show associations the
//! file manager does not use, and nothing will fail to compile.
//!
//! # Read-only here
//!
//! The associations are *set* by the File Associations application, which owns
//! the file. This page reports them. That is a deliberate split rather than a
//! missing feature -- see the note the page draws.

use yamldoc::Document;

/// The File Associations program's configuration group.
///
/// Must match `apps/explorer`'s `ASSOC_CONFIG_NAME`. See the module doc for
/// why the string is repeated instead of shared.
pub const ASSOC_CONFIG_NAME: &str = "fileassoc";

/// The mapping key under which associations live, inside that group.
const ASSOCIATIONS: &str = "associations";

/// One "files of this kind open with this program" entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Association {
    /// The extension, without a dot and lower-cased, as the file stores it.
    ///
    /// Lower-cased because that is how `apps/explorer` looks it up -- it
    /// lower-cases the extension before the query -- so an entry stored in
    /// mixed case is one the file manager will never match. Keeping the
    /// stored spelling rather than normalising it here is what lets the page
    /// show such an entry as it really is.
    pub extension: String,
    /// The program that opens it, as an executable path.
    ///
    /// May be empty: a key present with no value is a broken entry rather than
    /// an absent one, and the two are worth telling apart on screen.
    pub program: String,
}

impl Association {
    /// How the extension is written for a reader: `.txt`, not `txt`.
    #[must_use]
    pub fn label(&self) -> String {
        format!(".{}", self.extension)
    }

    /// Whether this entry names a program at all.
    ///
    /// An entry whose value is empty reaches `Command::new("")` in the file
    /// manager and fails there. Saying so here is the difference between "no
    /// association" and "an association that cannot work".
    #[must_use]
    pub fn is_runnable(&self) -> bool {
        !self.program.trim().is_empty()
    }
}

/// Every association in a loaded configuration document.
///
/// Pure, and takes the document rather than reading the file, for two reasons:
/// it can be tested without a filesystem, and the caller polls a
/// `settingsfile::Watcher` which hands back a `Document` already -- a second
/// function that re-read the file would be a second answer to the same
/// question.
///
/// Sorted by extension. The document preserves file order, which is whatever
/// order the File Associations program happened to write; a settings page that
/// reordered itself when an unrelated entry was edited would be harder to read
/// than one that is always alphabetical.
#[must_use]
pub fn associations_from(doc: &Document) -> Vec<Association> {
    let mut out: Vec<Association> = doc
        .keys(&[ASSOCIATIONS])
        .into_iter()
        .map(|extension| {
            // An absent value and an empty one both arrive as an empty
            // program. That is deliberate: the key's presence is what makes it
            // an entry, and `is_runnable` is what reports it cannot work.
            let program = doc
                .get_str(&[ASSOCIATIONS, extension.as_str()])
                .unwrap_or_default();
            Association { extension, program }
        })
        .collect();
    out.sort_by(|a, b| a.extension.cmp(&b.extension));
    out
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

    fn doc(text: &str) -> Document {
        Document::parse(text)
    }

    /// The ordinary case: extensions and their programs come back paired.
    #[test]
    fn associations_are_read_with_their_programs() {
        let d = doc("associations:\n  txt: /usr/bin/editor\n  png: /usr/bin/imageviewer\n");
        let got = associations_from(&d);
        assert_eq!(got.len(), 2, "wrong number of associations: {got:?}");
        assert_eq!(got[0].extension, "png");
        assert_eq!(got[0].program, "/usr/bin/imageviewer");
        assert_eq!(got[1].extension, "txt");
        assert_eq!(got[1].program, "/usr/bin/editor");
    }

    /// Sorted, not in file order.
    ///
    /// The point is that editing one entry must not move the others, which is
    /// what file order would do.
    #[test]
    fn the_list_is_alphabetical_whatever_the_file_order() {
        let d = doc("associations:\n  zip: /usr/bin/archivemanager\n  aaa: /usr/bin/a\n  md: /usr/bin/editor\n");
        let got: Vec<String> = associations_from(&d).into_iter().map(|a| a.extension).collect();
        assert_eq!(got, vec!["aaa", "md", "zip"], "not sorted: {got:?}");
    }

    /// A configuration with no associations block yields no rows, not a panic.
    #[test]
    fn a_document_without_associations_is_empty_not_fatal() {
        assert!(associations_from(&doc("")).is_empty());
        assert!(associations_from(&doc("something_else:\n  a: b\n")).is_empty());
    }

    /// A key with no value is kept, and reported as unable to run.
    ///
    /// Dropping it would hide a broken setting; showing it as though it worked
    /// would be worse. `apps/explorer` would reach `Command::new("")` on it.
    #[test]
    fn an_entry_with_no_program_is_kept_and_flagged() {
        let d = doc("associations:\n  txt:\n  png: /usr/bin/imageviewer\n");
        let got = associations_from(&d);
        let txt = got.iter().find(|a| a.extension == "txt").expect("the empty entry was dropped");
        assert!(!txt.is_runnable(), "an empty program was reported as runnable");
        let png = got.iter().find(|a| a.extension == "png").expect("the good entry was dropped");
        assert!(png.is_runnable(), "a real program was reported as broken");
    }

    /// Whitespace is not a program either.
    #[test]
    fn a_whitespace_only_program_is_not_runnable() {
        let a = Association { extension: "txt".into(), program: "   ".into() };
        assert!(!a.is_runnable());
    }

    /// The label is what a reader expects to see.
    #[test]
    fn the_label_carries_a_dot() {
        let a = Association { extension: "txt".into(), program: "/usr/bin/editor".into() };
        assert_eq!(a.label(), ".txt");
    }

    /// An entry stored in mixed case keeps its spelling.
    ///
    /// `apps/explorer` lower-cases before looking up, so `TXT` is an entry the
    /// file manager can never match. Normalising it here would make the page
    /// show a working association where there is a dead one.
    #[test]
    fn a_mixed_case_extension_is_shown_as_stored() {
        let d = doc("associations:\n  TXT: /usr/bin/editor\n");
        let got = associations_from(&d);
        assert_eq!(got[0].extension, "TXT", "the stored spelling was normalised away");
    }
}
