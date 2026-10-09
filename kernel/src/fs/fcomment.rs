//! File comments: free-form text a person attaches to a file or directory,
//! which the file explorer shows in its Properties dialog. Unlike tags (short
//! labels) or queryable attributes (typed key-value metadata), a comment is
//! for a human to read.
//!
//! **A comment is the file's `user.xdg.comment` extended attribute** -- where
//! `design.txt` puts it ("Arbitrary string/comment ... Stored in extended
//! attributes", lines 377 and 391-392), under the name freedesktop.org's
//! shared file metadata gives it, so KDE's Dolphin, `getfattr -n
//! user.xdg.comment` and `setfattr` read and write the same one. The file's
//! filesystem keeps it, which settles everything a table of comments here
//! could not:
//!
//! - it is the file's, not a name's: every hard link shows the one comment,
//!   and a rename carries it;
//! - it goes with the file, so a file that later reuses the inode does not
//!   begin with a stranger's comment;
//! - on ext4 it is on disk, and survives a reboot;
//! - `cp -a`, `tar --xattrs` and `rsync -X` carry it to and from Linux.
//!
//! Programs reach it with the extended-attribute calls they already have
//! (`getxattr(2)` and its family, through either ABI), under the `user.`
//! namespace's rules (`fs::xattr_policy`): regular files and directories
//! only, read as the file's read permission allows and written as its write
//! permission does. What this module adds is for the kernel shell: [`search`]
//! and [`list`], which walk a subtree reading the attribute -- bounded, since
//! nothing indexes comments (`design.txt`: "user-facing tagging is better done
//! at the GUI/search-index level, not in filesystem metadata").
//!
//! Until 2026-10-02 the comments lived in a table here, in memory and keyed by
//! name: two names of one file had two comments, a rename lost one, a reboot
//! lost them all, and no program could reach any of it (design-decisions
//! §1533). `/proc/fcomment` listed every one of them to any reader, whether or
//! not it could read the files; it now counts operations only.
//!
//! ## Limits
//!
//! - A comment is at most [`MAX_COMMENT_SIZE`] bytes (64 KiB, Linux's
//!   `XATTR_SIZE_MAX` and the figure `design.txt` gives). A filesystem may keep
//!   less, and says so when asked to keep more: ext4 keeps a file's attributes
//!   in one block.
//! - A comment is bytes. Text is what a person writes, but the attribute is
//!   anyone's to set, so nothing here assumes UTF-8.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::fswalk::{self, WalkAction, WalkOptions};
use super::path::{Path, PathBuf};
use super::{EntryType, Vfs};
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The extended attribute a comment is kept in.
pub const XATTR: &[u8] = b"user.xdg.comment";

/// The longest comment, in bytes: Linux's `XATTR_SIZE_MAX`.
pub const MAX_COMMENT_SIZE: usize = 65536;

/// The most entries one [`search`] or [`list`] visits: each visit is a path
/// lookup and a read of the attribute, under the filesystem's lock, in the
/// caller's time.
const MAX_VISITED: u64 = 65536;

/// The most comments one [`search`] or [`list`] returns.
const MAX_RESULTS: usize = 4096;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static SET_COUNT: AtomicU64 = AtomicU64::new(0);
static GET_COUNT: AtomicU64 = AtomicU64::new(0);
static SEARCH_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// One file's comment
// ---------------------------------------------------------------------------

/// Give the file at `path` the comment `comment`, replacing any it has; an
/// empty one removes it, and removing none is not an error. Asked as the
/// caller: `setxattr`'s rules decide who may.
///
/// # Errors
///
/// `InvalidArgument` for a comment over [`MAX_COMMENT_SIZE`]; the path's, the
/// `user.` namespace's (`PermissionDenied`, and `PermissionDenied` again for
/// anything but a regular file or a directory) and the filesystem's -- one
/// that keeps no attributes (FAT) answers `NotSupported`, one with no room
/// left for this one says so.
pub fn set(path: &Path, comment: &[u8]) -> KernelResult<()> {
    if comment.len() > MAX_COMMENT_SIZE {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    if comment.is_empty() {
        return match Vfs::remove_xattr(path, XATTR) {
            Ok(()) | Err(KernelError::NoAttribute) => Ok(()),
            Err(e) => Err(e),
        };
    }
    Vfs::set_xattr(path, XATTR, comment)
}

/// The comment on the file at `path`, or `None` when it has none. Asked as the
/// caller, through a trailing symlink, as `getxattr` asks.
///
/// # Errors
///
/// The path's, the `user.` namespace's and the filesystem's.
pub fn get(path: &Path) -> KernelResult<Option<Vec<u8>>> {
    GET_COUNT.fetch_add(1, Ordering::Relaxed);
    match Vfs::get_xattr(path, XATTR) {
        Ok(comment) => Ok(Some(comment)),
        Err(KernelError::NoAttribute) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remove the comment from the file at `path`.
///
/// # Errors
///
/// `NoAttribute` when it has none; otherwise as [`set`].
pub fn remove(path: &Path) -> KernelResult<()> {
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    Vfs::remove_xattr(path, XATTR)
}

// ---------------------------------------------------------------------------
// Many files' comments
// ---------------------------------------------------------------------------

/// What a [`search`] or a [`list`] found under its root.
#[derive(Debug)]
pub struct Found {
    /// Each commented file's path and comment, in the order the walk met them.
    pub comments: Vec<(PathBuf, Vec<u8>)>,
    /// Whether it stopped short of seeing everything under the root:
    /// `MAX_VISITED` entries or `MAX_RESULTS` comments reached, a
    /// directory or an attribute that could not be read, or a directory the
    /// walk left unread (`fswalk::WalkStats::unwalked`). Without it, "nothing
    /// found" and "not all looked at" would read the same.
    pub incomplete: bool,
}

/// The files and directories under `root`, `root` included, whose comment
/// contains `needle` with ASCII letters' case ignored; an empty `needle`
/// matches every comment. `/proc`, `/sys` and `/dev` are not walked (their
/// files keep no attributes), and symlinks are not followed.
///
/// # Errors
///
/// `root`'s: `NotFound`, `NotADirectory`.
pub fn search(needle: &[u8], root: &Path) -> KernelResult<Found> {
    SEARCH_COUNT.fetch_add(1, Ordering::Relaxed);
    collect(root, |comment| {
        contains_ignoring_ascii_case(comment, needle)
    })
}

/// Every commented file and directory under `root`, `root` included, walked
/// as [`search`] walks.
///
/// # Errors
///
/// As [`search`].
pub fn list(root: &Path) -> KernelResult<Found> {
    collect(root, |_| true)
}

/// Walk `root` reading each regular file's and directory's comment, keeping
/// those `wanted` takes.
fn collect(root: &Path, wanted: impl Fn(&[u8]) -> bool) -> KernelResult<Found> {
    let opts = WalkOptions {
        show_hidden: true,
        include_root: true,
        ..WalkOptions::default()
    };
    let mut found = Found {
        comments: Vec::new(),
        incomplete: false,
    };
    let mut visited: u64 = 0;
    let stats = fswalk::walk_visit(root, &opts, |entry| {
        visited = visited.saturating_add(1);
        if visited > MAX_VISITED {
            found.incomplete = true;
            return WalkAction::Stop;
        }
        // Only these two carry `user.` attributes.
        if !matches!(entry.entry_type, EntryType::File | EntryType::Directory) {
            return WalkAction::Continue;
        }
        match Vfs::get_xattr_no_follow(&entry.path, XATTR) {
            Ok(comment) if wanted(comment.as_slice()) => {
                if found.comments.len() >= MAX_RESULTS {
                    found.incomplete = true;
                    return WalkAction::Stop;
                }
                found.comments.push((entry.path.clone(), comment));
            }
            // No comment, or a filesystem that keeps no attributes and so no
            // comments: nothing missed.
            Ok(_) | Err(KernelError::NoAttribute | KernelError::NotSupported) => {}
            // Anything else -- the entry gone mid-walk, a read that failed --
            // is a comment that may exist and was not seen.
            Err(_) => found.incomplete = true,
        }
        WalkAction::Continue
    })?;
    if stats.errors > 0 || stats.unwalked > 0 {
        found.incomplete = true;
    }
    Ok(found)
}

/// Whether `needle` occurs in `haystack`, ASCII letters' case ignored.
fn contains_ignoring_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// How many comments were set or removed, read, and searched for, since boot
/// or [`reset_stats`].
#[must_use]
pub fn stats() -> (u64, u64, u64) {
    (
        SET_COUNT.load(Ordering::Relaxed),
        GET_COUNT.load(Ordering::Relaxed),
        SEARCH_COUNT.load(Ordering::Relaxed),
    )
}

/// Zero the counters.
pub fn reset_stats() {
    SET_COUNT.store(0, Ordering::Relaxed);
    GET_COUNT.store(0, Ordering::Relaxed);
    SEARCH_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comments as the file's attribute, on a scratch tree in `/tmp` (memfs):
///
/// 1. set and read back, and what a program's `getxattr` sees is the same
///    comment, listed under its name;
/// 2. bytes, not text: a comment that is not UTF-8 and a name that is not
///    either;
/// 3. the file's, not a name's: a hard link shows the comment, a rename
///    carries it, and a file made at the name of a deleted one has none;
/// 4. an empty comment removes it, removing none is `NoAttribute`, and one
///    over the limit is refused with nothing changed;
/// 5. search ignores ASCII case and keeps to its root, which must exist; a
///    listing finds every comment, the root's own included, and says it saw
///    everything.
///
/// The counters are put back as they were: this is reachable from the shell,
/// and a test run is not the user's activity.
///
/// # Errors
///
/// `InternalError` naming the first step that answered wrongly; the setup's.
pub fn self_test() -> KernelResult<()> {
    const DIR: &str = "/tmp/_fcomment";
    let saved = stats();
    // Best effort, both ends: this test's own scratch tree.
    let _ = Vfs::remove_recursive(DIR);
    let result = self_test_on(DIR);
    let _ = Vfs::remove_recursive(DIR);
    SET_COUNT.store(saved.0, Ordering::Relaxed);
    GET_COUNT.store(saved.1, Ordering::Relaxed);
    SEARCH_COUNT.store(saved.2, Ordering::Relaxed);
    result?;
    crate::serial_println!(
        "[fcomment] Self-test passed (5 tests): the file's attribute, bytes, links and renames, \
         removal and limits, search"
    );
    Ok(())
}

fn self_test_on(dir: &str) -> KernelResult<()> {
    use crate::serial_println;

    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("[fcomment]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    let at = |name: &[u8]| {
        let mut p = PathBuf::from(dir);
        p.push(Path::new(name));
        p
    };
    let report = at(b"report.pdf");
    let alias = at(b"alias.pdf");
    let moved = at(b"moved.pdf");
    let odd = at(b"\xFFnote.txt");
    let sub = at(b"sub");
    let deep = at(b"sub/deep.txt");

    Vfs::mkdir(dir)?;
    Vfs::mkdir(&sub)?;
    Vfs::write_file(&report, b"pdf")?;
    Vfs::write_file(&odd, b"odd")?;
    Vfs::write_file(&deep, b"deep")?;

    // 1: set, read back, and the attribute a program reads.
    set(&report, b"Q3 Quarterly report, needs review")?;
    let read = get(&report)?;
    let program = Vfs::get_xattr(&report, XATTR)?;
    let listed = Vfs::list_xattrs(&report)?;
    if read.as_deref() != Some(&b"Q3 Quarterly report, needs review"[..])
        || program != b"Q3 Quarterly report, needs review"
        || !listed.iter().any(|n| n.as_slice() == XATTR)
    {
        return fail("a comment did not read back as the file's user.xdg.comment");
    }
    serial_println!("[fcomment]   1: set and read back, as the file's attribute: ok");

    // 2: bytes, not text.
    set(&odd, b"caf\xE9 \xFF")?;
    if get(&odd)?.as_deref() != Some(&b"caf\xE9 \xFF"[..]) {
        return fail("a comment that is not UTF-8, on a name that is not either, changed");
    }
    serial_println!("[fcomment]   2: bytes in the comment and the name: ok");

    // 3: the file's, not a name's.
    Vfs::link(&report, &alias)?;
    let through_link = get(&alias)?;
    Vfs::rename(&report, &moved)?;
    let after_rename = get(&moved)?;
    Vfs::remove(&moved)?;
    Vfs::remove(&alias)?;
    Vfs::write_file(&report, b"new")?;
    let newcomer = get(&report)?;
    if through_link != read || after_rename != read || newcomer.is_some() {
        serial_println!(
            "[fcomment]     link {:?}, renamed {:?}, a new file at the old name {:?}",
            through_link,
            after_rename,
            newcomer
        );
        return fail("a comment did not follow its file");
    }
    serial_println!("[fcomment]   3: through a link, across a rename, gone with the file: ok");

    // 4: removal and limits.
    set(&report, b"temporary")?;
    set(&report, b"")?;
    let emptied = get(&report)?;
    let absent = remove(&report);
    let quiet = set(&report, b"");
    set(&report, b"kept")?;
    let oversized = alloc::vec![b'x'; MAX_COMMENT_SIZE.saturating_add(1)];
    let refused = set(&report, &oversized);
    let survivor = get(&report)?;
    if emptied.is_some()
        || absent != Err(KernelError::NoAttribute)
        || quiet.is_err()
        || refused != Err(KernelError::InvalidArgument)
        || survivor.as_deref() != Some(&b"kept"[..])
    {
        serial_println!(
            "[fcomment]     emptied {:?}, removed none {:?}, emptied none {:?}, oversized {:?}, after {:?}",
            emptied,
            absent,
            quiet,
            refused,
            survivor
        );
        return fail("removing or refusing a comment went wrong");
    }
    serial_println!("[fcomment]   4: removal, removing none, the size limit: ok");

    // 5: search and list.
    set(&deep, b"Internal memo about Q4 PLANNING")?;
    set(&sub, b"a directory's own comment")?;
    let planning = search(b"planning", Path::new(dir))?;
    let outside = search(b"planning", Path::new("/tmp/_fcomment/sub/none"));
    let in_sub = list(&sub)?;
    let everything = list(Path::new(dir))?;
    let planning_ok = planning.comments.len() == 1
        && planning.comments.first().is_some_and(|(p, _)| *p == deep)
        && !planning.incomplete;
    if !planning_ok
        || outside.is_ok()
        || in_sub.comments.len() != 2
        || everything.comments.len() != 4
        || everything.incomplete
    {
        serial_println!(
            "[fcomment]     planning {:?}, a missing root {:?}, under sub {}, everything {}",
            planning,
            outside.map(|f| f.comments.len()),
            in_sub.comments.len(),
            everything.comments.len()
        );
        return fail("a search or a listing found the wrong files");
    }
    serial_println!("[fcomment]   5: search ignoring case, within its root, and list: ok");
    Ok(())
}
