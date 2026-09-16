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

/// A named group of file kinds that share one program.
///
/// See design-decisions 857. A category is *defined* as its extension set
/// rather than stored as a name: the file the file manager obeys has no idea
/// what "Music" is, so a category that were stored separately would be a claim
/// with nothing behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Category {
    /// How the group is written on the row.
    pub name: &'static str,
    /// The extensions it covers, without dots, lower-cased.
    ///
    /// Lower-cased because `apps/explorer` lower-cases before it looks up, so
    /// an upper-case entry here would write a key it can never match.
    pub extensions: &'static [&'static str],
}

/// The groups offered, in the order they are shown.
///
/// Three, not the six the shell's dead panel drew. "Web browser" and "email"
/// are protocol categories with no consumer anywhere in the tree, and
/// "documents" is not one kind of file -- a text file and a PDF do not share a
/// program, so the row could only impose a wrong association. 857 has the
/// evidence for each.
pub const CATEGORIES: &[Category] = &[
    Category { name: "Music", extensions: &["aac", "flac", "m4a", "mp3", "ogg", "wav"] },
    Category { name: "Video", extensions: &["avi", "mkv", "mov", "mp4", "webm"] },
    Category { name: "Images", extensions: &["bmp", "gif", "jpeg", "jpg", "png", "webp"] },
];

/// What a category currently resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryDefault {
    /// No extension in the group names a runnable program.
    Unset,
    /// Every extension in the group names the same program.
    Agreed(String),
    /// They do not agree, or only some of them are set.
    ///
    /// Partly-set counts as disagreement on purpose. Reporting the majority
    /// program would say "Music opens with this" while some music file on the
    /// machine opened with nothing -- the page would be making a claim that is
    /// false for part of what it names, which is the whole failure 856 is
    /// about. Choosing a program from the row repairs it, because setting a
    /// category writes every extension in it.
    Mixed,
}

/// What `category` resolves to in `doc`.
///
/// Pure, for `associations_from`'s reasons: testable without a filesystem, and
/// the caller already holds the document.
#[must_use]
pub fn category_default(doc: &Document, category: &Category) -> CategoryDefault {
    let mut agreed: Option<&str> = None;
    let mut any_unset = false;
    let mut programs: Vec<String> = Vec::new();
    for extension in category.extensions {
        let program = doc.get_str(&[ASSOCIATIONS, extension]).unwrap_or_default();
        if program.trim().is_empty() {
            any_unset = true;
        } else {
            programs.push(program);
        }
    }
    for program in &programs {
        match agreed {
            None => agreed = Some(program.as_str()),
            Some(first) if first == program.as_str() => {}
            Some(_) => return CategoryDefault::Mixed,
        }
    }
    match agreed {
        None => CategoryDefault::Unset,
        Some(_) if any_unset => CategoryDefault::Mixed,
        Some(program) => CategoryDefault::Agreed(program.to_string()),
    }
}

/// Point every extension in `category` at `program`.
///
/// Writes each extension rather than a category key, which is the whole of
/// 857: the file manager resolves an extension, so an extension is the only
/// thing worth writing.
pub fn set_category(doc: &mut Document, category: &Category, program: &str) {
    for extension in category.extensions {
        doc.set_str(&[ASSOCIATIONS, extension], program);
    }
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
        let d = doc(
            "associations:\n  zip: /usr/bin/archivemanager\n  aaa: /usr/bin/a\n  md: /usr/bin/editor\n",
        );
        let got: Vec<String> = associations_from(&d)
            .into_iter()
            .map(|a| a.extension)
            .collect();
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
        let txt = got
            .iter()
            .find(|a| a.extension == "txt")
            .expect("the empty entry was dropped");
        assert!(
            !txt.is_runnable(),
            "an empty program was reported as runnable"
        );
        let png = got
            .iter()
            .find(|a| a.extension == "png")
            .expect("the good entry was dropped");
        assert!(png.is_runnable(), "a real program was reported as broken");
    }

    /// Whitespace is not a program either.
    #[test]
    fn a_whitespace_only_program_is_not_runnable() {
        let a = Association {
            extension: "txt".into(),
            program: "   ".into(),
        };
        assert!(!a.is_runnable());
    }

    /// The label is what a reader expects to see.
    #[test]
    fn the_label_carries_a_dot() {
        let a = Association {
            extension: "txt".into(),
            program: "/usr/bin/editor".into(),
        };
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
        assert_eq!(
            got[0].extension, "TXT",
            "the stored spelling was normalised away"
        );
    }

    /// A machine nobody has configured has no default for a group.
    #[test]
    fn an_unconfigured_category_is_unset() {
        let d = Document::parse("");
        for category in CATEGORIES {
            assert_eq!(
                category_default(&d, category),
                CategoryDefault::Unset,
                "{} on an empty document",
                category.name
            );
        }
    }

    /// Choosing a program for a group writes every extension in it.
    ///
    /// This is the whole of 857: the file manager resolves an extension, so a
    /// category that wrote a category key would change nothing at all.
    #[test]
    fn setting_a_category_writes_every_extension_in_it() {
        for category in CATEGORIES {
            let mut d = Document::parse("");
            set_category(&mut d, category, "/usr/bin/chosen");
            for extension in category.extensions {
                assert_eq!(
                    d.get_str(&[ASSOCIATIONS, extension]).as_deref(),
                    Some("/usr/bin/chosen"),
                    "{} left .{extension} unwritten",
                    category.name
                );
            }
            assert_eq!(
                category_default(&d, category),
                CategoryDefault::Agreed("/usr/bin/chosen".to_string())
            );
        }
    }

    /// Members pointing at different programs have no single answer.
    #[test]
    fn members_that_disagree_are_mixed() {
        let category = &CATEGORIES[0];
        let mut d = Document::parse("");
        set_category(&mut d, category, "/usr/bin/one");
        d.set_str(&[ASSOCIATIONS, category.extensions[1]], "/usr/bin/two");
        assert_eq!(category_default(&d, category), CategoryDefault::Mixed);
    }

    /// And so does a group only half of which is set.
    ///
    /// The tempting answer is Agreed: every extension that *names* a program
    /// names the same one. It is also a false claim -- the row would read
    /// "Music opens with this" while some music file on the machine opened
    /// with nothing. Pinned because it is the one a future simplification
    /// would quietly get wrong.
    #[test]
    fn a_partly_set_category_is_mixed_not_agreed() {
        let category = &CATEGORIES[0];
        let mut d = Document::parse("");
        d.set_str(&[ASSOCIATIONS, category.extensions[0]], "/usr/bin/one");
        assert_eq!(
            category_default(&d, category),
            CategoryDefault::Mixed,
            "a category with one of {} extensions set claimed to be settled",
            category.extensions.len()
        );
    }

    /// Every listed extension is lower-case, because the lookup is.
    ///
    /// `apps/explorer` lower-cases an extension before it queries, so an
    /// upper-case entry here would write a key nothing can ever match -- a
    /// setting that silently does nothing, which is the shape this page exists
    /// to stop shipping.
    #[test]
    fn every_listed_extension_is_lower_case() {
        for category in CATEGORIES {
            for extension in category.extensions {
                assert_eq!(
                    *extension,
                    extension.to_lowercase(),
                    "{} lists .{extension} in mixed case",
                    category.name
                );
            }
        }
    }

    /// No extension belongs to two groups.
    ///
    /// A hand-written table drifts: an overlap would mean choosing a video
    /// player silently changed the music default, and the page would show one
    /// group reverting for no reason the user could see.
    #[test]
    fn no_extension_is_claimed_by_two_categories() {
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for category in CATEGORIES {
            for extension in category.extensions {
                if let Some((other, _)) = seen.iter().find(|(_, e)| e == extension) {
                    panic!(".{extension} is in both {other} and {}", category.name);
                }
                seen.push((category.name, extension));
            }
        }
    }

    /// A group is a group: one extension would add nothing over its own row.
    #[test]
    fn every_category_groups_more_than_one_extension() {
        for category in CATEGORIES {
            assert!(
                category.extensions.len() > 1,
                "{} covers {} extension(s)",
                category.name,
                category.extensions.len()
            );
        }
    }
}
