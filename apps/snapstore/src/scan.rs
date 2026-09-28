//! Walking a source directory: what is there, and what could not be read.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use pathcodec::os_string_from_bytes;

use crate::glob::is_excluded;

/// One file or link found under the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where it is, to read it.
    pub full: PathBuf,
    /// Where it is relative to the source, `/`-separated: the manifest's name.
    pub rel: PathBuf,
    /// Its size.
    pub size: u64,
    /// Its modification time, seconds since the epoch.
    pub mtime: u64,
    /// Its permission bits, where the platform has them.
    pub mode: Option<u32>,
    /// The link's target, if it is a link.
    pub link_target: Option<PathBuf>,
}

/// Everything a walk found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Walk {
    /// Files and links, in the order found.
    pub files: Vec<Found>,
    /// Every directory under the source, relative.
    pub dirs: Vec<PathBuf>,
    /// What was there and could not be read, relative, with why.
    ///
    /// This used to be a warning on stderr and nothing else, and the file was
    /// simply not in the backup. For a restore that makes a folder match the
    /// snapshot, "not in the snapshot" means "delete it" -- so a file this
    /// walk could not read would have been destroyed by the restore meant to
    /// protect it. Recorded instead, so the restore can leave it be.
    pub unread: Vec<(PathBuf, String)>,
}

/// Walk `source`, leaving out what `excludes` match.
///
/// `follow_symlinks` walks into the directories links point at, and records a
/// link to a file as that file. A link loop is not followed twice: each
/// directory's resolved path is entered once.
#[must_use]
pub fn walk(source: &Path, excludes: &[String], follow_symlinks: bool) -> Walk {
    let mut out = Walk::default();
    let mut entered = HashSet::new();
    if follow_symlinks && let Ok(real) = fs::canonicalize(source) {
        entered.insert(real);
    }
    walk_dir(
        source,
        source,
        excludes,
        follow_symlinks,
        &mut entered,
        &mut out,
    );
    out
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    excludes: &[String],
    follow: bool,
    entered: &mut HashSet<PathBuf>,
    out: &mut Walk,
) {
    let listing = match fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(e) => {
            out.unread.push((relative_path(dir, root), e.to_string()));
            return;
        }
    };
    for entry in listing {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                // The listing broke part-way: what it would have named next is
                // unknown, so the directory itself is what could not be read.
                out.unread.push((relative_path(dir, root), e.to_string()));
                return;
            }
        };
        let path = entry.path();
        let rel = relative_path(&path, root);
        if is_excluded(&rel, excludes) {
            continue;
        }
        let meta = if follow {
            fs::metadata(&path)
        } else {
            fs::symlink_metadata(&path)
        };
        let meta = match meta {
            Ok(meta) => meta,
            Err(e) => {
                out.unread.push((rel, e.to_string()));
                continue;
            }
        };
        if meta.is_dir() {
            if follow {
                match fs::canonicalize(&path) {
                    Ok(real) => {
                        if !entered.insert(real) {
                            // Already walked: a loop, or two links to one
                            // place. Its contents are in the snapshot once.
                            continue;
                        }
                    }
                    Err(e) => {
                        out.unread.push((rel, e.to_string()));
                        continue;
                    }
                }
            }
            out.dirs.push(rel);
            walk_dir(root, &path, excludes, follow, entered, out);
        } else if meta.is_file() {
            out.files.push(Found {
                full: path,
                rel,
                size: meta.len(),
                mtime: mtime_of(&meta),
                mode: mode_of(&meta),
                link_target: None,
            });
        } else if meta.file_type().is_symlink() {
            // A link's target is an arbitrary byte string too, and it is what
            // a restore recreates, so it must not be flattened -- nor made up:
            // this read `unwrap_or_default()`, so a link whose target could not
            // be read went in as a link to nothing.
            match fs::read_link(&path) {
                Ok(target) => out.files.push(Found {
                    full: path,
                    rel,
                    size: 0,
                    mtime: 0,
                    mode: None,
                    link_target: Some(target),
                }),
                Err(e) => out.unread.push((rel, e.to_string())),
            }
        }
        // Anything else -- a device, a pipe, a socket -- is not content a
        // snapshot can hold. It is not "unread" either: there was nothing to
        // read. A restore that makes a folder match leaves such a thing be,
        // because it only ever removes files and links.
    }
}

/// Seconds since the epoch, 0 where the platform gives no time.
fn mtime_of(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// The permission bits, where the platform has them.
#[cfg(unix)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "one signature on every platform; elsewhere there are no bits to give"
)]
fn mode_of(meta: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode() & 0o7777)
}

/// No permission bits on this platform.
#[cfg(not(unix))]
fn mode_of(_meta: &fs::Metadata) -> Option<u32> {
    None
}

/// `full` relative to `base`, `/`-separated.
///
/// Byte-exact: an earlier version went through `to_string_lossy`, which is
/// where a name that is not UTF-8 was destroyed -- before the manifest writer
/// ever saw it. Components are rejoined with `/` rather than by
/// `PathBuf::push` so the manifest records the same text on every host; `/`
/// cannot occur inside a component, so the join is reversible. The root and
/// prefix components are handled apart because their text is already a
/// separator, and joining them like the rest would double it.
#[must_use]
pub fn relative_path(full: &Path, base: &Path) -> PathBuf {
    let rel = full.strip_prefix(base).unwrap_or(full);
    let mut out: Vec<u8> = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Prefix(prefix) => {
                out.extend_from_slice(prefix.as_os_str().as_encoded_bytes());
            }
            Component::RootDir => out.push(b'/'),
            _ => {
                if !out.is_empty() && !out.ends_with(b"/") {
                    out.push(b'/');
                }
                out.extend_from_slice(comp.as_os_str().as_encoded_bytes());
            }
        }
    }
    PathBuf::from(os_string_from_bytes(out))
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did it
    // -- that is the diagnosis. The defensive lints keep panics out of code
    // that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

    /// `relative_path` produces every `FileEntry.path`, so its output is what
    /// the manifest records. It must strip the base and normalise separators
    /// without going anywhere near a string conversion.
    #[test]
    fn relative_path_strips_the_base_and_joins_with_forward_slashes() {
        let base = PathBuf::from("/srv/data");
        assert_eq!(
            relative_path(Path::new("/srv/data/a/b/c.txt"), &base),
            PathBuf::from("a/b/c.txt")
        );
        // A path that is not under the base is kept whole rather than silently
        // becoming empty — an empty relative path would collide with every
        // other such file in the manifest. The root component must survive as a
        // single leading `/`, not as a doubled or host-flavoured separator.
        assert_eq!(
            relative_path(Path::new("/elsewhere/x.txt"), &base),
            PathBuf::from("/elsewhere/x.txt")
        );
        // Host separators are normalised to `/` so the manifest reads the same
        // whichever machine wrote it.
        assert_eq!(
            relative_path(Path::new(r"a\b\c.txt"), Path::new("")),
            PathBuf::from(if cfg!(windows) {
                "a/b/c.txt"
            } else {
                r"a\b\c.txt"
            })
        );
        // The base itself is relative to nothing.
        assert_eq!(relative_path(&base, &base), PathBuf::new());
    }

    /// The defect this whole change exists for, at the point it happened:
    /// `relative_path` produced the name the manifest recorded, and it went
    /// through `to_string_lossy`, so a byte the filesystem allowed became
    /// U+FFFD before anything could escape it.
    ///
    /// Unix-only because a Windows `OsString` cannot hold such a path at all —
    /// and our target is `target-family = ["unix"]`, so this is the platform
    /// that matters.
    #[cfg(unix)]
    #[test]
    fn relative_path_does_not_mangle_a_non_utf8_name() {
        use std::os::unix::ffi::OsStrExt;
        let full = Path::new(std::ffi::OsStr::from_bytes(b"/srv/data/photos/caf\xE9.jpg"));
        let rel = relative_path(full, Path::new("/srv/data"));
        assert_eq!(
            rel.as_os_str().as_bytes(),
            b"photos/caf\xE9.jpg",
            "the 0xE9 must reach the manifest writer intact"
        );
    }
}
