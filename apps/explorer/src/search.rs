//! Finding files by name, underneath the folder being viewed.
//!
//! This is the first of the search modes `roadmap-detailed.md` lists, and the
//! only one with no prerequisite: *"Filename/path (always available, no
//! indexer needed)"*. The others -- file contents, OCR text, image
//! descriptions -- all require the file indexer, and one requires the ML
//! option on top of it.
//!
//! # Why there are no checkboxes yet
//!
//! The roadmap item is "search feature with checkboxes for what to search",
//! and this ships the box that works rather than five boxes of which one does.
//! design-decisions 856 is explicit about the trade: *a settings page is built
//! when something obeys it, not when something stores it.* A "file contents"
//! checkbox with no indexer behind it is a control that stores a preference
//! nothing honours -- it would make the feature look finished and search
//! quietly worse, because a user who ticks it gets fewer results than they
//! would have got by leaving it alone. The boxes arrive with their backing.
//!
//! # Why the match is on bytes
//!
//! `design.txt` allows every byte but `/` and NUL in a name, so a filename
//! need not be text. `apps/filesearch` meets the same problem and answers it
//! honestly -- it takes `to_str()`, skips a name that is not text, and reports
//! how many it skipped. That is not corrupt, but it is a file you cannot find.
//!
//! Matching raw bytes does better for no extra cost. A file called
//! `report<FF>.txt` is found by "report" and by ".txt", because those bytes
//! are in the name whatever the rest of it is. Nothing has to be decoded, so
//! nothing can be decoded wrongly, and no file is unfindable.
//!
//! The query is text, because it is typed. That asymmetry is fine and is the
//! same one `apps/backup`'s exclude patterns rest on: a user cannot type a
//! byte that is not text, so a query can only ever name the decodable parts of
//! a name -- which is exactly the part this compares.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

/// How many matches a search collects before it stops looking.
///
/// A bound rather than a complete answer, because this runs on the UI thread:
/// an unbounded walk of a large tree freezes the window, and a file manager
/// that stops responding is worse than one that says it found a lot. The
/// result reports [`Found::truncated`] so the count on screen can never be
/// read as "this is all of them".
pub const MAX_RESULTS: usize = 2000;

/// How far below the starting folder a search descends.
///
/// Guards against a pathological tree rather than against depth as such; the
/// result is reported as truncated if this is what stopped it, for the same
/// reason as the count.
pub const MAX_DEPTH: usize = 16;

/// What a search found, and what it did not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    /// Matching paths, in breadth-first order: the shallowest first, which is
    /// usually the one the user meant.
    pub paths: Vec<PathBuf>,
    /// Whether the walk stopped before exhausting the tree.
    ///
    /// Separate from `paths.len() == MAX_RESULTS` on purpose: a tree that
    /// happens to hold exactly the cap is complete, and saying otherwise would
    /// be a lie in the one case a user is most likely to check.
    pub truncated: bool,
    /// Directories that could not be read, usually for want of permission.
    ///
    /// Counted rather than silently dropped: "no results" and "no results in
    /// the half of the tree I was allowed to look at" are different answers,
    /// and only one of them means the file is not there.
    pub unreadable: usize,
}

/// Whether `name` contains `query`, comparing bytes and ignoring ASCII case.
///
/// Non-ASCII bytes compare exactly. That is deliberate rather than a
/// limitation: case folding outside ASCII is language-dependent (Turkish
/// dotless i, Greek final sigma), and a file manager's find-as-you-type is not
/// the place to pick a locale. ASCII case-insensitivity is what a user expects
/// from a filename search and is the same rule the rest of the toolkit uses.
#[must_use]
pub fn name_matches(name: &OsStr, query: &str) -> bool {
    let needle = query.as_bytes();
    if needle.is_empty() {
        return false;
    }
    let hay = name.as_encoded_bytes();
    if needle.len() > hay.len() {
        return false;
    }
    hay.windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Whether a name begins with `.`, tested as a byte.
///
/// The leading dot is ASCII, so this agrees with the text version for every
/// name that has one -- but it also answers for a name that has no text form,
/// where the text version would have had to decode first.
fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// Find everything under `root` whose name contains `query`.
///
/// Breadth-first, so results arrive shallowest-first and a truncated search
/// still returns the near matches rather than whatever a depth-first walk
/// happened to reach.
///
/// An empty query finds nothing rather than everything: a search box that has
/// just been opened should not replace the listing with a recursive dump of
/// the disk.
#[must_use]
pub fn find(root: &Path, query: &str, show_hidden: bool) -> Found {
    let mut found = Found::default();
    if query.is_empty() {
        return found;
    }

    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        let read = match fs::read_dir(&dir) {
            Ok(read) => read,
            Err(_) => {
                found.unreadable = found.unreadable.saturating_add(1);
                continue;
            }
        };

        for entry in read.flatten() {
            let name = entry.file_name();
            if !show_hidden && is_hidden(&name) {
                continue;
            }

            let path = entry.path();
            // `file_type()` off the entry rather than a `metadata()` call:
            // it is already known from the directory read on every platform
            // that has it, and it does not follow symlinks -- so a link
            // pointing at its own ancestor cannot turn this walk into a loop.
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());

            if name_matches(&name, query) {
                if found.paths.len() >= MAX_RESULTS {
                    found.truncated = true;
                    return found;
                }
                found.paths.push(path.clone());
            }

            if is_dir {
                if depth < MAX_DEPTH {
                    queue.push_back((path, depth.saturating_add(1)));
                } else {
                    found.truncated = true;
                }
            }
        }
    }

    found
}

/// A one-line summary of a search, for the status bar.
///
/// Says what was not looked at as well as what was found. A bare count invites
/// the reading "that is all of them", which after a truncated walk is wrong in
/// the direction that matters -- the user concludes the file is absent.
#[must_use]
pub fn describe(found: &Found, query: &str) -> String {
    let n = found.paths.len();
    let head = match n {
        0 => format!("No matches for \"{query}\""),
        1 => format!("1 match for \"{query}\""),
        _ => format!("{n} matches for \"{query}\""),
    };
    let mut out = head;
    if found.truncated {
        out.push_str(" (stopped early; narrow the search to see the rest)");
    }
    match found.unreadable {
        0 => {}
        1 => out.push_str(" — 1 folder could not be read"),
        n => out.push_str(&format!(" — {n} folders could not be read")),
    }
    out
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

    use scratchdir::ScratchDir;

    /// Build `root/<rel>` files, creating parents as needed.
    fn make(root: &Path, rels: &[&str]) {
        for rel in rels {
            let path = root.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            std::fs::write(&path, b"x").expect("write");
        }
    }

    /// The walk descends, and finds a match in a subfolder.
    #[test]
    fn a_match_below_the_starting_folder_is_found() {
        let scratch = ScratchDir::new("explorer-search-nested");
        let root = scratch.dir();
        make(
            root,
            &[
                "top-report.txt",
                "sub/deep/inner-report.txt",
                "sub/other.dat",
            ],
        );

        let found = find(root, "report", false);
        assert_eq!(found.paths.len(), 2, "{:?}", found.paths);
        assert!(!found.truncated);
        assert_eq!(found.unreadable, 0);
    }

    /// Shallow matches come first, because a truncated walk should return the
    /// near ones.
    #[test]
    fn results_arrive_shallowest_first() {
        let scratch = ScratchDir::new("explorer-search-order");
        let root = scratch.dir();
        make(root, &["a/b/c/match-deep.txt", "match-top.txt"]);

        let found = find(root, "match", false);
        assert_eq!(found.paths.len(), 2, "{:?}", found.paths);
        assert!(
            found.paths[0].ends_with("match-top.txt"),
            "breadth-first should put the shallow match first: {:?}",
            found.paths
        );
    }

    /// A hidden file is skipped unless hidden files are being shown.
    #[test]
    fn hidden_files_follow_the_view_setting() {
        let scratch = ScratchDir::new("explorer-search-hidden");
        let root = scratch.dir();
        make(root, &[".hidden-report.txt", "plain-report.txt"]);

        assert_eq!(find(root, "report", false).paths.len(), 1);
        assert_eq!(find(root, "report", true).paths.len(), 2);
    }

    /// A hidden *folder* is not descended into either.
    ///
    /// Worth its own test: skipping a hidden file but walking a hidden folder
    /// would surface its children, which is the opposite of what the setting
    /// asks for and is invisible in a listing of the folder itself.
    #[test]
    fn a_hidden_folder_is_not_descended() {
        let scratch = ScratchDir::new("explorer-search-hiddendir");
        let root = scratch.dir();
        make(root, &[".git/config-report.txt", "visible-report.txt"]);

        let found = find(root, "report", false);
        assert_eq!(found.paths.len(), 1, "{:?}", found.paths);
        assert!(found.paths[0].ends_with("visible-report.txt"));
    }

    /// An empty query walks nothing at all.
    #[test]
    fn an_empty_query_does_not_walk() {
        let scratch = ScratchDir::new("explorer-search-empty");
        let root = scratch.dir();
        make(root, &["a.txt", "b.txt"]);

        assert_eq!(find(root, "", false), Found::default());
    }

    /// A folder that is not there is reported as unreadable, not as empty.
    #[test]
    fn a_missing_root_is_unreadable_rather_than_empty() {
        let scratch = ScratchDir::new("explorer-search-missing");
        let found = find(&scratch.dir().join("no-such-folder"), "x", false);
        assert_eq!(found.unreadable, 1);
        assert!(found.paths.is_empty());
    }

    /// A plain substring, found.
    #[test]
    fn a_substring_of_the_name_matches() {
        assert!(name_matches(OsStr::new("annual-report.txt"), "report"));
        assert!(name_matches(OsStr::new("annual-report.txt"), ".txt"));
    }

    /// Case is ignored for ASCII.
    #[test]
    fn ascii_case_is_ignored() {
        assert!(name_matches(OsStr::new("Annual-REPORT.TXT"), "report"));
        assert!(name_matches(OsStr::new("annual-report.txt"), "REPORT"));
    }

    /// An empty query matches nothing, rather than everything.
    ///
    /// The distinction is the whole behaviour of a search box that has just
    /// been opened: matching everything would replace the folder listing with
    /// a recursive dump the moment the box appears.
    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(!name_matches(OsStr::new("anything"), ""));
    }

    /// A query longer than the name cannot match, and does not panic.
    ///
    /// `windows()` on a slice shorter than the window yields nothing, which is
    /// the right answer, but the guard is explicit because the panic it would
    /// otherwise risk is on a zero window and that is easy to conflate.
    #[test]
    fn a_query_longer_than_the_name_is_not_a_match() {
        assert!(!name_matches(OsStr::new("ab"), "abcdef"));
    }

    /// A name with no text form is still findable by its ASCII parts.
    ///
    /// The point of matching bytes. `apps/filesearch` skips such a name --
    /// honestly, and it says how many it skipped -- but skipped is still a
    /// file the user cannot find. Here the undecodable byte simply is not one
    /// of the bytes being looked for.
    #[cfg(windows)]
    #[test]
    fn a_name_that_is_not_text_is_still_findable() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        // `report<D800>.txt`: a real name, with no text form.
        let odd = OsString::from_wide(&[
            u16::from(b'r'),
            u16::from(b'e'),
            u16::from(b'p'),
            u16::from(b'o'),
            u16::from(b'r'),
            u16::from(b't'),
            0xD800,
            u16::from(b'.'),
            u16::from(b't'),
            u16::from(b'x'),
            u16::from(b't'),
        ]);
        assert!(
            odd.to_str().is_none(),
            "the fixture decodes, so this test proves nothing"
        );
        assert!(
            name_matches(&odd, "report"),
            "the ASCII stem is still there"
        );
        assert!(name_matches(&odd, ".txt"), "and so is the extension");
    }

    /// Ordinary text queries still work on ordinary names.
    #[test]
    fn a_non_ascii_query_matches_its_own_bytes() {
        assert!(name_matches(OsStr::new("café-menu.pdf"), "café"));
        assert!(!name_matches(OsStr::new("cafe-menu.pdf"), "café"));
    }

    /// Nothing is described as complete when it was not.
    #[test]
    fn a_truncated_search_says_so() {
        let found = Found {
            paths: vec![PathBuf::from("/a")],
            truncated: true,
            unreadable: 0,
        };
        let text = describe(&found, "a");
        assert!(text.contains("1 match"), "{text}");
        assert!(text.contains("stopped early"), "{text}");
    }

    /// Unreadable folders are reported, because "not found" and "not looked
    /// at" are different answers.
    #[test]
    fn folders_that_could_not_be_read_are_counted_out_loud() {
        let found = Found {
            paths: Vec::new(),
            truncated: false,
            unreadable: 3,
        };
        let text = describe(&found, "x");
        assert!(text.contains("No matches"), "{text}");
        assert!(text.contains("3 folders could not be read"), "{text}");
    }

    /// One folder reads as "1 folder", not "1 folders".
    #[test]
    fn the_singular_reads_as_english() {
        let found = Found {
            paths: Vec::new(),
            truncated: false,
            unreadable: 1,
        };
        assert!(describe(&found, "x").contains("1 folder could not be read"));
    }
}
