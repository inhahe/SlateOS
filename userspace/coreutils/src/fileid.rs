//! Which file, and which name for it: the two identity questions every utility
//! that writes over an existing path has to answer.
//!
//! # The two questions, and why they are two
//!
//! **"Are these the same file?"** — [`FileId`], the `(device, inode)` pair. It
//! is the only answer that survives the file being reached by a different name,
//! and reaching a file by a different name is the entire subject: `a`, `./a`,
//! `d/../a` and a symlink pointing at `a` are four spellings of one file, and a
//! utility that compares the four strings finds four different files and
//! destroys one of them.
//!
//! **"Are these the same directory entry?"** — [`EntryId`], the directory's
//! `FileId` paired with the final component of the name. Two hard links to one
//! file share a `FileId` and have different `EntryId`s, which is exactly the
//! distinction GNU draws in `same_nameat` (gnulib `lib/same.c`): `cp a hard-a d`
//! is a request for two copies rather than a repeat, while `cp a ./a d` is a
//! repeat.
//!
//! Both are needed and neither substitutes for the other. `cp` asks the first to
//! refuse `cp a d/../a`, and the second to notice a repeated operand;
//! `mv` asks the first to refuse `mv link file` (which would destroy the file
//! the link points at) and the second to allow `mv link other-link`.
//!
//! # Why the byte split rather than `Path::file_name`
//!
//! [`split_entry`] works on the bytes because `Path::file_name` answers `None`
//! for a name whose last component is `.` or `..` — so `a/.` and `b/..` would
//! both come back as "no name" and look like one entry named twice. GNU's
//! `last_component` keeps them, and both `cp` and `mv` depend on it keeping
//! them: `cp -r a/. dst` targets `dst/.`, which *is* `dst`, and that is why the
//! idiom copies a directory's contents rather than the directory. `mv a/.. dst`
//! targets `dst/..`, which is how GNU comes to report the two as the same file
//! rather than moving the user's current directory somewhere they never named.
//!
//! # Where the portable arm gives up, and why that is stated rather than hidden
//!
//! A host without inode numbers — Windows, the development machine — has no
//! cheaper answer than the resolved path, so [`file_id`] canonicalises there.
//! That agrees with the pair on every question except hard links, which such a
//! host does not meaningfully have either. The guarantees below are therefore
//! stated on the `#[cfg(unix)]` arm, which is the arm the target OS uses:
//! `toolchain/x86_64-slateos.json` sets `"target-family": ["unix"]`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// What tells one file from another.
///
/// The `(device, inode)` pair. See the module docs for why nothing textual will
/// do.
#[cfg(unix)]
pub type FileId = (u64, u64);

/// The portable stand-in: a host with no inode numbers has no cheaper answer
/// than the resolved path.
#[cfg(not(unix))]
pub type FileId = PathBuf;

/// Identify the file `meta` describes.
///
/// `path` is unused on a POSIX host and is the whole answer on the other, which
/// is why both are taken: a caller that already holds the metadata should not
/// have to know which arm it is compiled against.
///
/// `None` only on the portable arm, and only when the path cannot be resolved.
#[cfg(unix)]
#[must_use]
pub fn file_id(_path: &Path, meta: &fs::Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
#[must_use]
pub fn file_id(path: &Path, _meta: &fs::Metadata) -> Option<FileId> {
    fs::canonicalize(path).ok()
}

/// Do two already-stat'd paths describe one file? GNU's `SAME_INODE`.
///
/// Both paths are passed because the portable arm has no inode number and must
/// fall back on resolving the name; on the POSIX arm they are ignored. "Cannot
/// answer" is `false`, so a caller that treats `true` as "refuse" errs toward
/// letting the operation proceed and being caught by the next check, and a
/// caller that treats `true` as "allow" errs toward refusing.
///
/// One caveat, and it is confined to the development host: the portable arm
/// *follows* the name it resolves, so two distinct symlinks to one target
/// compare equal there where a POSIX host calls them two different files. Every
/// caller here reaches the wrong answer in the refusing direction, so the cost
/// on that host is a diagnostic rather than a destroyed file.
#[must_use]
pub fn same_inode(a: (&Path, &fs::Metadata), b: (&Path, &fs::Metadata)) -> bool {
    match (file_id(a.0, a.1), file_id(b.0, b.1)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// How many directory entries point at this file — `st_nlink`.
///
/// The count is what separates "the destination is the last name this file has"
/// from "there is another name and the data survives", which is the distinction
/// GNU's `same_file_ok` turns on when deciding whether `mv link file` destroys
/// something. A host without hard links answers 1, which is the truth there.
#[cfg(unix)]
#[must_use]
pub fn nlink(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.nlink()
}

#[cfg(not(unix))]
#[must_use]
pub fn nlink(_meta: &fs::Metadata) -> u64 {
    1
}

/// What tells one *directory entry* from another: which directory it is in, and
/// the final component of the name.
pub type EntryId = (FileId, OsString);

/// The entry `path` names, or `None` if the directory holding it cannot be
/// identified.
///
/// `None` means "cannot answer", and every caller treats that as "not the same
/// entry" — the same conclusion GNU reaches when its `fstatat` of the parent
/// fails.
#[must_use]
pub fn entry_id(path: &Path) -> Option<EntryId> {
    let (dir, name) = split_entry(path);
    let meta = fs::metadata(&dir).ok()?;
    Some((file_id(&dir, &meta)?, name))
}

/// Do two paths name the same directory entry? GNU's `same_nameat`.
///
/// `false` when either side cannot be identified, which is GNU's answer too. The
/// callers all treat "cannot answer" as "assume they are different", which is
/// the safe direction: it costs a refusal rather than a silent overwrite.
#[must_use]
pub fn same_entry(a: &Path, b: &Path) -> bool {
    match (entry_id(a), entry_id(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Do two paths name the same file?
///
/// Both are *followed* unless `nofollow` is set, so a destination that is a
/// symlink to the source counts — writing through it truncates the source
/// exactly as surely as naming the source directly.
///
/// The one exception is GNU's, and it is the reason `nofollow` is a parameter:
/// when the source is *not* being followed, two names that are both symlinks are
/// the same file only when they are the same *link*, because replacing one link
/// with a copy of another does not touch what either points at. `cp -P linkA
/// linkB` where both point at one file is therefore allowed, while `cp -P link
/// file` — where `link` resolves to `file` — is not, and GNU makes exactly that
/// distinction in `same_file_ok` (`copy.c:1764`), keyed on `x->dereference ==
/// DEREF_NEVER` and not on `-r`.
///
/// `false` when either side cannot be stat'd. A source that is a dangling
/// symlink is not the same file as anything, and a destination that cannot be
/// reached will produce its own diagnostic a moment later.
#[cfg(unix)]
#[must_use]
pub fn is_same_file(src: &Path, dst: &Path, nofollow: bool) -> bool {
    use std::os::unix::fs::MetadataExt;
    fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    if nofollow {
        let (sl, dl) = (fs::symlink_metadata(src), fs::symlink_metadata(dst));
        if let (Ok(sl), Ok(dl)) = (&sl, &dl)
            && sl.file_type().is_symlink()
            && dl.file_type().is_symlink()
        {
            return same(sl, dl);
        }
    }
    match (fs::metadata(src), fs::metadata(dst)) {
        (Ok(a), Ok(b)) => same(&a, &b),
        _ => false,
    }
}

/// Windows exposes a file's identity only through `windows_by_handle`, which is
/// unstable, so the development host compares resolved paths instead. That still
/// catches a repeated operand and a `.` or `..` in the middle of one; it misses
/// a hard link, which is why the guarantee is stated on the `#[cfg(unix)]` arm
/// above — the arm the target OS and the certification harnesses both use.
#[cfg(not(unix))]
#[must_use]
pub fn is_same_file(src: &Path, dst: &Path, _nofollow: bool) -> bool {
    match (fs::canonicalize(src), fs::canonicalize(dst)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// A path's directory and final component, GNU's `dir_name`/`base_name` pair.
///
/// Trailing slashes belong to neither — `tree/` names the entry `tree` — and a
/// path with no slash at all names an entry in the current directory. See the
/// module docs for why this is done on the bytes rather than through
/// `Path::file_name`.
#[cfg(unix)]
#[must_use]
pub fn split_entry(path: &Path) -> (PathBuf, OsString) {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    // Everything after the last byte that is not a separator is decoration.
    let end = bytes
        .iter()
        .rposition(|&b| b != b'/')
        .map_or(bytes.len(), |i| i + 1);
    let head = bytes.get(..end).unwrap_or(bytes);
    match head.iter().rposition(|&b| b == b'/') {
        Some(cut) => {
            // An empty directory half means the name was rooted: `/etc` is the
            // entry `etc` in `/`, not in the current directory.
            let dir = head.get(..cut).filter(|d| !d.is_empty()).unwrap_or(b"/");
            let name = head.get(cut.saturating_add(1)..).unwrap_or_default();
            (
                PathBuf::from(OsStr::from_bytes(dir)),
                OsStr::from_bytes(name).to_os_string(),
            )
        }
        None => (PathBuf::from("."), OsStr::from_bytes(head).to_os_string()),
    }
}

/// The same split for the only non-POSIX host this builds on, Windows, where it
/// exists so that `cargo test` on the development machine exercises the same
/// code shape rather than a weaker stand-in. `OsStr` is not bytes there, so the
/// units are UTF-16 and both separators count.
#[cfg(not(unix))]
#[must_use]
pub fn split_entry(path: &Path) -> (PathBuf, OsString) {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    // `b'/' as u16` in a pattern position is not const-evaluable, and the two
    // code units are fixed by ASCII, so they are written out.
    const SLASH: u16 = 0x2F;
    const BACKSLASH: u16 = 0x5C;
    let sep = |c: u16| c == SLASH || c == BACKSLASH;

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let end = wide
        .iter()
        .rposition(|&c| !sep(c))
        .map_or(wide.len(), |i| i + 1);
    let head = wide.get(..end).unwrap_or(&wide);
    match head.iter().rposition(|&c| sep(c)) {
        Some(cut) => {
            // An empty directory half means the name was rooted, and the
            // separator itself is then the directory.
            let dir = if cut == 0 {
                head.get(..1)
            } else {
                head.get(..cut)
            };
            let name = head.get(cut.saturating_add(1)..).unwrap_or_default();
            (
                PathBuf::from(OsString::from_wide(dir.unwrap_or_default())),
                OsString::from_wide(name),
            )
        }
        None => (PathBuf::from("."), OsString::from_wide(head)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn split_entry_keeps_dot_and_dotdot_apart() {
        assert_eq!(split_entry(Path::new("a/.")).1, OsString::from("."));
        assert_eq!(split_entry(Path::new("a/..")).1, OsString::from(".."));
        assert_eq!(split_entry(Path::new("a/b/")).1, OsString::from("b"));
        assert_eq!(
            split_entry(Path::new("b")),
            (PathBuf::from("."), "b".into())
        );
    }

    /// Trailing slashes are decoration on the *whole* path, however many, and a
    /// path that is nothing but separators names the entry with the empty name
    /// in the root — which is how `mv / dst` comes to append nothing.
    #[test]
    fn split_entry_strips_every_trailing_separator() {
        assert_eq!(split_entry(Path::new("a/b//")).1, OsString::from("b"));
        assert_eq!(split_entry(Path::new("a//b")).0, PathBuf::from("a"));
        assert_eq!(split_entry(Path::new("a//b")).1, OsString::from("b"));
    }

    /// A rooted name's directory is the root itself, not the current directory:
    /// the entry `etc` lives in `/`, and saying `.` would make `/etc` and `etc`
    /// the same entry.
    #[cfg(unix)]
    #[test]
    fn split_entry_roots_a_rooted_name() {
        assert_eq!(
            split_entry(Path::new("/etc")),
            (PathBuf::from("/"), "etc".into())
        );
    }

    /// Two names for one file are one file and two entries, which is the whole
    /// reason both identities exist. Runs only where hard links do.
    #[cfg(unix)]
    #[test]
    fn a_hard_link_is_one_file_and_two_entries() {
        let dir = std::env::temp_dir().join(format!(
            "coreutils-fileid-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"x").unwrap();
        fs::hard_link(&a, &b).unwrap();

        assert!(is_same_file(&a, &b, false), "one inode under two names");
        assert!(!same_entry(&a, &b), "two entries");
        assert!(same_entry(&a, &dir.join("./a")), "one entry, two spellings");

        let _ = fs::remove_dir_all(&dir);
    }
}
