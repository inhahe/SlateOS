//! Which program opens which kind of file.
//!
//! The one model behind the file-type associations: `apps/fileassoc` writes
//! them, `apps/explorer` obeys them on every open, and `apps/settings` reports
//! them. Before this crate existed all three named the group with a bare
//! string of their own, each carrying a doc comment warning that if the
//! spellings ever drifted the file manager would read a file nobody wrote and
//! *nothing would fail to compile*. Three warnings about the same hazard is
//! the tree asking for a crate.
//!
//! `apps/explorer` put the reason for the old arrangement plainly: "named here
//! rather than imported because `apps/fileassoc` is a *binary*: there is
//! nothing to link against". That was true, and it is the thing this fixes --
//! there is something to link against now.
//!
//! # What a consumer is
//!
//! The rule these types are shaped by is design-decisions 856: a setting is
//! real when something obeys it. The consumer here is concrete --
//! `apps/explorer`'s `open_entry` looks the extension up in this file and
//! spawns what it finds:
//!
//! ```text
//! settingsfile::load("fileassoc") -> ["associations", <ext>] -> a program path
//!   -> Command::new(program).arg(path).spawn()
//! ```
//!
//! # Extensions and categories
//!
//! Extensions are what is stored, because they are what the file manager
//! resolves. A [`Category`] is a *view* over them -- "Music" is defined as its
//! extension set, not stored as a name -- so choosing a program for a category
//! writes each extension in it. See design-decisions 857, which also records
//! why there is no "web browser", "email" or "documents" category.

use guitk::filetypes::FileCategory;
use yamldoc::Document;

/// The settings group the associations live in.
///
/// The single definition. Every consumer imports this rather than spelling the
/// string again, which is what the crate is for.
pub const CONFIG_NAME: &str = "fileassoc";

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

/// The program set for `extension`, or `None` if nothing usable is.
///
/// The lookup itself, so that a consumer does not have to spell the key path.
/// `apps/explorer` used to hold its own copy of it, which meant the string
/// `"associations"` appeared in three binaries as well as the group name.
///
/// Lower-cases before looking up, because that is how the entries are written.
/// An entry whose value is blank answers `None`: a key present with no program
/// reaches `Command::new("")` in the caller and fails there, so reporting it as
/// found would hand the caller something it cannot run.
#[must_use]
pub fn program_for(doc: &Document, extension: &str) -> Option<String> {
    let extension = extension.to_lowercase();
    let program = doc.get_str(&[ASSOCIATIONS, extension.as_str()])?;
    if program.trim().is_empty() {
        return None;
    }
    Some(program)
}

/// A named group of file kinds that share one program.
///
/// See design-decisions 857. A category is *defined* as a
/// [`FileCategory`] over the toolkit's table rather than as a list of its own:
/// the tree already holds one mapping of extension to kind, and a second copy
/// here would drift the moment a format was added to one and not the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Category {
    /// How the group is written on the row.
    pub name: &'static str,
    /// The kind it covers, in the toolkit's vocabulary.
    pub kind: FileCategory,
}

impl Category {
    /// The extensions this group covers, without dots.
    pub fn extensions(&self) -> impl Iterator<Item = &'static str> {
        guitk::filetypes::extensions_in(self.kind)
    }
}

/// The groups offered, in the order they are shown.
///
/// Three, not the six the shell's dead panel drew. "Web browser" and "email"
/// are protocol categories with no consumer anywhere in the tree, and
/// "documents" is not one kind of file -- the toolkit's table files both `txt`
/// and `pdf` under `Document`, and those do not share a program, so the row
/// could only impose a wrong association. 857 has the evidence for each, and
/// `guitk::filetypes`' own tests check the `Document` half of it against the
/// table rather than trusting this comment.
pub const CATEGORIES: &[Category] = &[
    Category {
        name: "Music",
        kind: FileCategory::Audio,
    },
    Category {
        name: "Video",
        kind: FileCategory::Video,
    },
    Category {
        name: "Images",
        kind: FileCategory::Image,
    },
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
    for extension in category.extensions() {
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
    for extension in category.extensions() {
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

    /// The lookup finds what was written, whatever case it is asked in.
    #[test]
    fn a_program_is_found_by_extension_in_any_case() {
        let mut d = Document::parse("");
        d.set_str(&[ASSOCIATIONS, "mp3"], "/usr/bin/musicplayer");
        assert_eq!(
            program_for(&d, "mp3").as_deref(),
            Some("/usr/bin/musicplayer")
        );
        assert_eq!(
            program_for(&d, "MP3").as_deref(),
            Some("/usr/bin/musicplayer")
        );
        assert_eq!(program_for(&d, "flac"), None);
    }

    /// A key present with no program is not a find.
    ///
    /// The caller would reach `Command::new("")` and fail. Answering `None`
    /// lets it say "nothing is set to open this" instead of reporting a
    /// program that cannot start.
    #[test]
    fn a_blank_program_is_not_found() {
        let mut d = Document::parse("");
        d.set_str(&[ASSOCIATIONS, "txt"], "   ");
        assert_eq!(program_for(&d, "txt"), None);
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
            for extension in category.extensions() {
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
        let second = category.extensions().nth(1).expect("the group has two");
        d.set_str(&[ASSOCIATIONS, second], "/usr/bin/two");
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
        let first = category
            .extensions()
            .next()
            .expect("the group is not empty");
        d.set_str(&[ASSOCIATIONS, first], "/usr/bin/one");
        assert_eq!(
            category_default(&d, category),
            CategoryDefault::Mixed,
            "a category with one of {} extensions set claimed to be settled",
            category.extensions().count()
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
            for extension in category.extensions() {
                assert_eq!(
                    *extension,
                    extension.to_lowercase(),
                    "{} lists .{extension} in mixed case",
                    category.name
                );
            }
        }
    }

    /// A group is a group: one extension would add nothing over its own row.
    #[test]
    fn every_category_groups_more_than_one_extension() {
        for category in CATEGORIES {
            assert!(
                category.extensions().count() > 1,
                "{} covers {} extension(s)",
                category.name,
                category.extensions().count()
            );
        }
    }
}
