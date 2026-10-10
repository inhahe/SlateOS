//! File tags: short labels a person attaches to files and directories
//! ("work", "taxes 2026", "Projects/SlateOS"), and finds them by. Unlike a
//! comment (free text for a person to read, `fs::fcomment`) a tag is matched;
//! unlike a queryable attribute (`fs::queryable`) it has no type and no value
//! beyond itself.
//!
//! **A file's tags are its `user.xdg.tags` extended attribute**: a
//! comma-separated list, the name and the format freedesktop.org's shared file
//! metadata and KDE's Baloo use, so Dolphin, `getfattr -n user.xdg.tags` and
//! `setfattr` read and write the same tags. The file's filesystem keeps them,
//! which settles what an index here could not:
//!
//! - they are the file's, not a name's: every hard link shows them, and a
//!   rename carries them;
//! - they go with the file, so a file that later reuses the inode does not
//!   begin with a stranger's tags;
//! - on ext4 they are on disk, and survive a reboot;
//! - `cp -a`, `tar --xattrs` and `rsync -X` carry them to and from Linux.
//!
//! Programs reach them with the extended-attribute calls they already have
//! (`getxattr(2)` and its family, through either ABI), under the `user.`
//! namespace's rules (`fs::xattr_policy`): regular files and directories only,
//! read as the file's read permission allows, written as its write permission
//! does. That is the door design-decisions §978 asked for. What this module
//! adds is for the kernel shell: reading and changing one file's list, and
//! [`search`], [`search_multi`] and [`list_tags`], which walk a subtree --
//! bounded, and saying when they could not see all of it, since nothing
//! indexes tags (`design.txt`: "user-facing tagging is better done at the
//! GUI/search-index level, not in filesystem metadata"; a search index is a
//! userspace service's to keep).
//!
//! Until 2026-10-09 the tags were in a `user.tags` attribute, and an index
//! here mapped each tag to the *names* of its files, in memory: a rename left
//! the file out of every search until the index was rebuilt by hand, a reboot
//! emptied it, and `/proc/tags` listed every tag in use to any reader, whether
//! or not it could read the files (design-decisions §1560). Nothing outside the
//! kernel shell and this module's self-test ever set one -- the self-test on
//! `/tmp`, in memory -- so no file carries the old name to carry over.
//!
//! ## A tag
//!
//! - At most [`MAX_TAG_LEN`] bytes of text: a label a person types, so UTF-8,
//!   as Baloo writes it. An attribute that is not UTF-8 reads as
//!   `CorruptedData` rather than as some other list.
//! - No comma (the separator), no control character, and no space at either
//!   end; spaces inside, `/` (Baloo's nesting) and any letter are fine.
//! - Matched with ASCII letters' case ignored, kept as written.
//! - At most [`MAX_TAGS_PER_FILE`] on a file.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::fswalk::{self, WalkAction, WalkOptions};
use super::path::{Path, PathBuf};
use super::{EntryType, Vfs};
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The extended attribute a file's tags are kept in.
pub const XATTR: &[u8] = b"user.xdg.tags";

/// The most tags one file may carry.
pub const MAX_TAGS_PER_FILE: usize = 32;

/// The longest tag, in bytes.
pub const MAX_TAG_LEN: usize = 64;

/// The most entries one walk visits: each visit is a path lookup and a read
/// of the attribute, under the filesystem's lock, in the caller's time.
const MAX_VISITED: u64 = 65536;

/// The most files one search returns.
const MAX_RESULTS: usize = 4096;

/// The most distinct tags one [`list_tags`] counts.
const MAX_DISTINCT: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A file and its tags, as a search found them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedFile {
    /// The path the walk reached it by. A path is bytes, not text: a file
    /// whose name is not UTF-8 is still found, and openable from the result.
    pub path: PathBuf,
    /// Its tags, as written.
    pub tags: Vec<String>,
}

/// What a [`search`] or [`search_multi`] found under its root.
#[derive(Debug)]
pub struct Found {
    /// Each matching file, in the order the walk met them.
    pub files: Vec<TaggedFile>,
    /// Whether it stopped short of seeing everything under the root: too
    /// many entries or results, a directory or an attribute that could not be
    /// read, or a directory the walk left unread. Without it, "nothing found"
    /// and "not all looked at" would read the same.
    pub incomplete: bool,
}

/// Every tag in use under a root, with how many files carry it.
#[derive(Debug)]
pub struct Census {
    /// Each tag as first met, with its count; ordered by the tag with ASCII
    /// letters' case ignored.
    pub tags: Vec<(String, usize)>,
    /// As [`Found::incomplete`].
    pub incomplete: bool,
}

/// How many tag changes, removals and searches there have been since boot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagStats {
    /// Lists set, and tags added.
    pub adds: u64,
    /// Tags removed, and lists cleared.
    pub removes: u64,
    /// Searches and listings.
    pub searches: u64,
}

static ADDS: AtomicU64 = AtomicU64::new(0);
static REMOVES: AtomicU64 = AtomicU64::new(0);
static SEARCHES: AtomicU64 = AtomicU64::new(0);

/// The counters.
#[must_use]
pub fn stats() -> TagStats {
    TagStats {
        adds: ADDS.load(Ordering::Relaxed),
        removes: REMOVES.load(Ordering::Relaxed),
        searches: SEARCHES.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// A tag, and a list of them
// ---------------------------------------------------------------------------

/// Whether `tag` may be stored: see the module documentation's "A tag".
fn validate(tag: &str) -> KernelResult<()> {
    if tag.is_empty()
        || tag.len() > MAX_TAG_LEN
        || tag.contains(',')
        || tag.chars().any(char::is_control)
        || tag.trim() != tag
    {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

/// Whether two tags are the same one: ASCII letters' case ignored.
fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The tags an attribute's value holds: comma-separated, empty pieces
/// skipped, as Baloo reads them.
fn parse(raw: &[u8]) -> KernelResult<Vec<String>> {
    let text = core::str::from_utf8(raw).map_err(|_| KernelError::CorruptedData)?;
    Ok(text
        .split(',')
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect())
}

/// Keep `tags` as the file's list: the attribute removed when it is empty,
/// and removing none is not an error.
fn store(path: &Path, tags: &[String]) -> KernelResult<()> {
    if tags.is_empty() {
        return match Vfs::remove_xattr(path, XATTR) {
            Ok(()) | Err(KernelError::NoAttribute) => Ok(()),
            Err(e) => Err(e),
        };
    }
    Vfs::set_xattr(path, XATTR, tags.join(",").as_bytes())
}

// ---------------------------------------------------------------------------
// One file's tags
// ---------------------------------------------------------------------------

/// The tags on the file at `path`, as written; none is an empty list. Asked as
/// the caller, through a trailing symlink, as `getxattr` asks.
///
/// # Errors
///
/// `CorruptedData` for an attribute that is not UTF-8; the path's, the `user.`
/// namespace's and the filesystem's.
pub fn get<P: AsRef<Path> + ?Sized>(path: &P) -> KernelResult<Vec<String>> {
    match Vfs::get_xattr(path.as_ref(), XATTR) {
        Ok(raw) => parse(&raw),
        // Carrying no tags is an ordinary answer. `NotFound` -- the path did
        // not resolve -- is not, and goes on to the caller.
        Err(KernelError::NoAttribute) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Make `tags` the file's whole list, in that order, a second spelling of
/// one tag dropped; an empty list clears it. Asked as the caller: `setxattr`'s
/// rules decide who may.
///
/// # Errors
///
/// `InvalidArgument` for a tag that may not be stored, or more than
/// [`MAX_TAGS_PER_FILE`], with nothing changed; otherwise as [`get`], and a
/// filesystem that keeps no attributes (FAT) answers `NotSupported`.
pub fn set<P: AsRef<Path> + ?Sized>(path: &P, tags: &[&str]) -> KernelResult<()> {
    let mut list: Vec<String> = Vec::new();
    list.try_reserve(tags.len())
        .map_err(|_| KernelError::OutOfMemory)?;
    for &tag in tags {
        validate(tag)?;
        if !list.iter().any(|t| same(t, tag)) {
            list.push(String::from(tag));
        }
    }
    if list.len() > MAX_TAGS_PER_FILE {
        return Err(KernelError::InvalidArgument);
    }
    store(path.as_ref(), &list)?;
    ADDS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Add `tag` to the file's list; one it carries already, in any case, leaves
/// the list as it is.
///
/// # Errors
///
/// `InvalidArgument` for a tag that may not be stored, or a file at
/// [`MAX_TAGS_PER_FILE`] already; otherwise as [`set`].
pub fn add<P: AsRef<Path> + ?Sized>(path: &P, tag: &str) -> KernelResult<()> {
    let path = path.as_ref();
    validate(tag)?;
    let mut list = get(path)?;
    if list.iter().any(|t| same(t, tag)) {
        return Ok(());
    }
    if list.len() >= MAX_TAGS_PER_FILE {
        return Err(KernelError::InvalidArgument);
    }
    list.push(String::from(tag));
    store(path, &list)?;
    ADDS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Take `tag`, in any case, off the file's list.
///
/// # Errors
///
/// `NotFound` when the file does not carry it; otherwise as [`set`].
pub fn remove<P: AsRef<Path> + ?Sized>(path: &P, tag: &str) -> KernelResult<()> {
    let path = path.as_ref();
    let mut list = get(path)?;
    let before = list.len();
    list.retain(|t| !same(t, tag));
    if list.len() == before {
        return Err(KernelError::NotFound);
    }
    store(path, &list)?;
    REMOVES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Take every tag off the file; one with none is not an error.
///
/// # Errors
///
/// As [`set`].
pub fn clear<P: AsRef<Path> + ?Sized>(path: &P) -> KernelResult<()> {
    store(path.as_ref(), &[])?;
    REMOVES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Many files' tags
// ---------------------------------------------------------------------------

/// The files and directories under `root`, `root` included, that carry `tag`,
/// ASCII letters' case ignored. `/proc`, `/sys` and `/dev` are not walked
/// (their files keep no attributes), and symlinks are not followed.
///
/// # Errors
///
/// `root`'s: `NotFound`, `NotADirectory`.
pub fn search<R: AsRef<Path> + ?Sized>(tag: &str, root: &R) -> KernelResult<Found> {
    search_multi(&[tag], root)
}

/// The files and directories under `root` that carry every one of `tags`,
/// walked as [`search`] walks; no tags at all matches nothing.
///
/// # Errors
///
/// As [`search`].
pub fn search_multi<R: AsRef<Path> + ?Sized>(tags: &[&str], root: &R) -> KernelResult<Found> {
    SEARCHES.fetch_add(1, Ordering::Relaxed);
    let mut files = Vec::new();
    if tags.is_empty() {
        return Ok(Found {
            files,
            incomplete: false,
        });
    }
    let incomplete = each_tagged(root.as_ref(), |path, carried| {
        if tags
            .iter()
            .all(|wanted| carried.iter().any(|t| same(t, wanted)))
        {
            if files.len() >= MAX_RESULTS {
                return false;
            }
            files.push(TaggedFile {
                path: path.to_path_buf(),
                tags: carried,
            });
        }
        true
    })?;
    Ok(Found { files, incomplete })
}

/// Every tag in use under `root`, `root` included, with how many files and
/// directories carry it, walked as [`search`] walks.
///
/// # Errors
///
/// As [`search`].
pub fn list_tags<R: AsRef<Path> + ?Sized>(root: &R) -> KernelResult<Census> {
    SEARCHES.fetch_add(1, Ordering::Relaxed);
    // Keyed by the tag with ASCII letters lowered: one entry per tag however
    // its files spell it, shown as the first file spelled it.
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let incomplete = each_tagged(root.as_ref(), |_, carried| {
        let mut seen_here: Vec<String> = Vec::new();
        for tag in carried {
            let key = tag.to_ascii_lowercase();
            if seen_here.contains(&key) {
                continue;
            }
            if !counts.contains_key(&key) && counts.len() >= MAX_DISTINCT {
                return false;
            }
            let entry = counts.entry(key.clone()).or_insert((tag, 0));
            entry.1 = entry.1.saturating_add(1);
            seen_here.push(key);
        }
        true
    })?;
    Ok(Census {
        tags: counts.into_values().collect(),
        incomplete,
    })
}

/// Walk `root` handing each regular file and directory that carries tags to
/// `found`, which answers whether to go on. Answers whether the walk stopped
/// short of seeing everything (see [`Found::incomplete`]).
fn each_tagged(
    root: &Path,
    mut found: impl FnMut(&Path, Vec<String>) -> bool,
) -> KernelResult<bool> {
    let opts = WalkOptions {
        show_hidden: true,
        include_root: true,
        ..WalkOptions::default()
    };
    let mut incomplete = false;
    let mut visited: u64 = 0;
    let stats = fswalk::walk_visit(root, &opts, |entry| {
        visited = visited.saturating_add(1);
        if visited > MAX_VISITED {
            incomplete = true;
            return WalkAction::Stop;
        }
        // Only these two carry `user.` attributes.
        if !matches!(entry.entry_type, EntryType::File | EntryType::Directory) {
            return WalkAction::Continue;
        }
        match Vfs::get_xattr_no_follow(&entry.path, XATTR).and_then(|raw| parse(&raw)) {
            Ok(carried) if carried.is_empty() => {}
            Ok(carried) => {
                if !found(&entry.path, carried) {
                    incomplete = true;
                    return WalkAction::Stop;
                }
            }
            // No tags, or a filesystem that keeps no attributes and so no
            // tags: nothing missed.
            Err(KernelError::NoAttribute | KernelError::NotSupported) => {}
            // Anything else -- the entry gone mid-walk, a read that failed, a
            // list that is not text -- may be a match that was not seen.
            Err(_) => incomplete = true,
        }
        WalkAction::Continue
    })?;
    if stats.errors > 0 || stats.unwalked > 0 {
        incomplete = true;
    }
    Ok(incomplete)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Tags as the file's attribute, on a scratch tree in `/tmp` (memfs):
///
/// 1. set and read back, and what a program's `getxattr` sees is the same
///    list, under the attribute's name;
/// 2. add and remove with case ignored, removing one not there `NotFound`,
///    clearing removes the attribute;
/// 3. what may be a tag -- refusals change nothing, and spaces inside,
///    nesting and letters beyond ASCII are fine;
/// 4. the file's, not a name's: a hard link shows the tags, a rename carries
///    them, and a file made at the name of a deleted one has none;
/// 5. search ignores case, keeps to its root (which must exist), finds a
///    directory's own tags and files whose names are not UTF-8, and says it
///    saw everything; all-of searches; a census counts each tag once per file;
/// 6. a list that is not text reads as `CorruptedData`, and a search that met
///    one says it may have missed something.
///
/// The counters are put back as they were: this is reachable from the shell,
/// and a test run is not the user's activity.
///
/// # Errors
///
/// `InternalError` naming the first step that answered wrongly; the setup's.
pub fn self_test() -> KernelResult<()> {
    const DIR: &str = "/tmp/_tags";
    let saved = stats();
    // Best effort, both ends: this test's own scratch tree.
    let _ = Vfs::remove_recursive(DIR);
    let result = self_test_on(DIR);
    let _ = Vfs::remove_recursive(DIR);
    ADDS.store(saved.adds, Ordering::Relaxed);
    REMOVES.store(saved.removes, Ordering::Relaxed);
    SEARCHES.store(saved.searches, Ordering::Relaxed);
    result?;
    crate::serial_println!(
        "[tags] Self-test passed (6 tests): the file's user.xdg.tags, case, what a tag may be, \
         links and renames, search and census, a list that is not text"
    );
    Ok(())
}

#[allow(clippy::too_many_lines)] // one linear script
fn self_test_on(dir: &str) -> KernelResult<()> {
    use crate::serial_println;

    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("[tags]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    let at = |name: &[u8]| {
        let mut p = PathBuf::from(dir);
        p.push(Path::new(name));
        p
    };
    let doc = at(b"doc.txt");
    let alias = at(b"alias.txt");
    let moved = at(b"moved.txt");
    let odd = at(b"\xFFodd.txt");
    let sub = at(b"sub");
    let deep = at(b"sub/deep.txt");
    let strange = at(b"strange.txt");

    Vfs::mkdir(dir)?;
    Vfs::mkdir(&sub)?;
    Vfs::write_file(&doc, b"doc")?;
    Vfs::write_file(&odd, b"odd")?;
    Vfs::write_file(&deep, b"deep")?;
    Vfs::write_file(&strange, b"strange")?;

    // 1: set, read back, and the attribute a program reads.
    set(&doc, &["work", "Important"])?;
    let read = get(&doc)?;
    let program = Vfs::get_xattr(&doc, XATTR)?;
    let listed = Vfs::list_xattrs(&doc)?;
    if read != ["work", "Important"]
        || program != b"work,Important"
        || !listed.iter().any(|n| n.as_slice() == XATTR)
    {
        return fail("a list did not read back as the file's user.xdg.tags");
    }
    serial_println!("[tags]   1: set and read back, as the file's attribute: ok");

    // 2: case, removal, clearing.
    add(&doc, "WORK")?;
    add(&doc, "draft")?;
    let added = get(&doc)?;
    remove(&doc, "IMPORTANT")?;
    let removed = get(&doc)?;
    let absent = remove(&doc, "never-there");
    clear(&doc)?;
    let cleared = Vfs::get_xattr(&doc, XATTR);
    let cleared_again = clear(&doc);
    if added != ["work", "Important", "draft"]
        || removed != ["work", "draft"]
        || absent != Err(KernelError::NotFound)
        || cleared != Err(KernelError::NoAttribute)
        || cleared_again.is_err()
    {
        serial_println!(
            "[tags]     added {:?}, removed {:?}, absent {:?}, cleared {:?}",
            added,
            removed,
            absent,
            cleared.map(|v| v.len())
        );
        return fail("adding or removing with case ignored, or clearing, went wrong");
    }
    serial_println!("[tags]   2: case ignored, removing one not there, clearing: ok");

    // 3: what may be a tag.
    set(&doc, &["kept"])?;
    let long = "t".repeat(MAX_TAG_LEN.saturating_add(1));
    let many: Vec<String> = (0..=MAX_TAGS_PER_FILE)
        .map(|i| alloc::format!("t{i}"))
        .collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let refused = [
        add(&doc, ""),
        add(&doc, "a,b"),
        add(&doc, "tab\there"),
        add(&doc, " padded"),
        add(&doc, long.as_str()),
        set(&doc, &many),
    ];
    let unchanged = get(&doc)?;
    let fine = set(&doc, &["two words", "Projects/SlateOS", "Résumé"]);
    let fine_read = get(&doc)?;
    if refused
        .iter()
        .any(|r| *r != Err(KernelError::InvalidArgument))
        || unchanged != ["kept"]
        || fine.is_err()
        || fine_read != ["two words", "Projects/SlateOS", "Résumé"]
    {
        serial_println!(
            "[tags]     refusals {:?}, after them {:?}, the fine ones {:?} -> {:?}",
            refused,
            unchanged,
            fine,
            fine_read
        );
        return fail("what may be a tag was judged wrongly");
    }
    serial_println!("[tags]   3: what may be a tag, refusals changing nothing: ok");

    // 4: the file's, not a name's.
    Vfs::link(&doc, &alias)?;
    let through_link = get(&alias)?;
    Vfs::rename(&doc, &moved)?;
    let after_rename = get(&moved)?;
    Vfs::remove(&moved)?;
    Vfs::remove(&alias)?;
    Vfs::write_file(&doc, b"new")?;
    let newcomer = get(&doc)?;
    if through_link != fine_read || after_rename != fine_read || !newcomer.is_empty() {
        serial_println!(
            "[tags]     link {:?}, renamed {:?}, a new file at the old name {:?}",
            through_link,
            after_rename,
            newcomer
        );
        return fail("tags did not follow their file");
    }
    serial_println!("[tags]   4: through a link, across a rename, gone with the file: ok");

    // 5: search and census.
    set(&doc, &["work", "urgent"])?;
    set(&deep, &["Work"])?;
    set(&odd, &["work", "URGENT"])?;
    set(&sub, &["folder"])?;
    let work = search("WORK", Path::new(dir))?;
    let both = search_multi(&["work", "urgent"], Path::new(dir))?;
    let in_sub = search("work", &sub)?;
    let folder = search("folder", &sub)?;
    let missing_root = search("work", Path::new("/tmp/_tags/none"));
    let census = list_tags(Path::new(dir))?;
    let count_of = |tag: &str| {
        census
            .tags
            .iter()
            .find(|(t, _)| same(t, tag))
            .map_or(0, |(_, n)| *n)
    };
    let found_odd = work.files.iter().any(|f| f.path == odd);
    if work.files.len() != 3
        || work.incomplete
        || !found_odd
        || both.files.len() != 2
        || in_sub.files.len() != 1
        || in_sub.files.first().map(|f| &f.path) != Some(&deep)
        || folder.files.len() != 1
        || missing_root.is_ok()
        || census.tags.len() != 3
        || count_of("work") != 3
        || count_of("urgent") != 2
        || count_of("folder") != 1
        || census.incomplete
    {
        serial_println!(
            "[tags]     work {} (odd name found {}), both {}, under sub {}, sub's own {}, a missing root {:?}, census {:?}",
            work.files.len(),
            found_odd,
            both.files.len(),
            in_sub.files.len(),
            folder.files.len(),
            missing_root.map(|f| f.files.len()),
            census
        );
        return fail("a search or the census found the wrong files");
    }
    serial_println!("[tags]   5: search with case ignored, within its root, all-of, census: ok");

    // 6: a list that is not text.
    Vfs::set_xattr(&strange, XATTR, b"ok,\xFF\xFE")?;
    let unreadable = get(&strange);
    let after = search("work", Path::new(dir))?;
    if unreadable != Err(KernelError::CorruptedData) || !after.incomplete {
        serial_println!(
            "[tags]     read {:?}, the search after it incomplete {}",
            unreadable,
            after.incomplete
        );
        return fail("a list that is not text was read as something, or passed unremarked");
    }
    serial_println!("[tags]   6: a list that is not text, reported: ok");
    Ok(())
}
