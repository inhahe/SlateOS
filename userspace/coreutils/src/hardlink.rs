//! A hard link that takes a name already in use — gnulib's `force_linkat`
//! (`lib/force-link.c`) under GNU's `create_hard_link` (`copy.c:2122`).
//!
//! The companion to [`rename`](crate::rename), and the mirror image of it.
//! There the caller had checked a name was free and needed the kernel to hold
//! it free; here the caller has decided the name is *theirs* and needs whatever
//! is standing on it to be replaced without a window in which the name does not
//! exist at all. Both are one-line operations that `link(2)` and `rename(2)`
//! decline to provide, and both are shared for the same reason: two utilities
//! that disagree about what "replace" means disagree about a file.
//!
//! # Why it is not one `link(2)`
//!
//! `link(2)` fails with `EEXIST` and has no force flag. So gnulib links to a
//! fresh name in the destination's own directory and `rename`s that over the
//! destination — which is atomic, and is what makes `cp --preserve=links a b d`
//! work when `d/b` was already something else. Unlinking the destination first
//! would be the obvious alternative and is wrong in a way that only shows up
//! under a concurrent reader: between the unlink and the link the name does not
//! exist, so a `d/b` that was going to be replaced by an equivalent link can be
//! observed missing instead.
//!
//! The temporary must be in the **same directory** as the destination, because
//! `rename` cannot cross a filesystem and the destination's directory is the
//! only place guaranteed to be on the right one.
//!
//! # Who asks for it
//!
//! Both utilities that reproduce a file rather than move it, and both for the
//! same rule: a source that is a second name for an inode already written must
//! become a second name for the *result*, not a second copy of it.
//!
//! * `cp --preserve=links` (and `-a`), where the two names are two operands.
//! * `mv` across a filesystem boundary, where the copy is standing in for a
//!   rename and a rename would have kept the names together.
//!
//! # Following
//!
//! GNU passes `AT_SYMLINK_FOLLOW` when its caller dereferences; [`fs::hard_link`]
//! is `linkat` with no flags and cannot. The difference is unreachable rather
//! than unimplemented, and the reachable case needs the flag *off*: what is
//! being linked *from* is a destination this same command created, and a command
//! that dereferences creates no symlinks to link to. `cp -P --preserve=links l1
//! l2 d`, where `l1` and `l2` are two hard links to one symlink, must give
//! `d/l1` and `d/l2` one inode that is still a symlink — measured, and what this
//! produces.

use crate::errmsg::strerror;
use crate::fileid::split_entry;
use crate::quote::quoteaf_os;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;

/// Link `earlier` to `target`, replacing whatever is at `target`, and report a
/// failure the way GNU's `create_hard_link` does.
///
/// `program` is the diagnostic's prefix — `cp` or `mv` — and is the only thing
/// that differs between the two callers' messages. `verbose` is `-v`.
///
/// Returns whether the link is now there. A `false` has already been reported on
/// `err`; the caller's remaining job is whatever it does about a failed operand,
/// which for both of them is putting a `-b` backup back.
///
/// # The `removed` line, and where it lands
///
/// `-v` prints `removed 'target'` **only** when something was actually replaced,
/// and it prints it *here* — after the caller's own arrow line rather than
/// before it, because gnulib emits it from inside `force_linkat` after the
/// rename rather than in a pre-copy unlink. Measured with `cp`, with `d/b` a
/// dangling symlink:
///
/// ```text
/// 'a' -> 'd/a'
/// 'b' -> 'd/b'
/// removed 'd/b'
/// ```
///
/// It goes to `out` and not `err`: it is `--verbose` output, not a diagnostic.
pub fn force_link(
    program: &str,
    earlier: &Path,
    target: &Path,
    verbose: bool,
    out: &mut dyn io::Write,
    err: &mut dyn io::Write,
) -> bool {
    // gnulib's three-valued return, which is what the `removed` line is keyed
    // on: 0 for a link made on a free name, negative for one made after
    // replacing something, an errno for a failure.
    let existed = match fs::hard_link(earlier, target) {
        Ok(()) => false,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match link_over(earlier, target) {
            Ok(()) => true,
            Err(e) => return report_failure(program, &e, earlier, target, err),
        },
        Err(e) => return report_failure(program, &e, earlier, target, err),
    };
    if existed && verbose {
        let _ = writeln!(out, "removed {}", quoteaf_os(target));
    }
    true
}

/// gnulib's `cannot create hard link %s to %s`, destination first.
fn report_failure(
    program: &str,
    e: &io::Error,
    earlier: &Path,
    target: &Path,
    err: &mut dyn io::Write,
) -> bool {
    let why = strerror(e);
    let _ = writeln!(
        err,
        "{program}: cannot create hard link {} to {}: {why}",
        quoteaf_os(target),
        quoteaf_os(earlier)
    );
    false
}

/// The replace half of `force_linkat`: link into a temporary name beside
/// `target`, rename it over, and remove the temporary either way.
///
/// The name is gnulib's `CuXXXXXX` pattern with the random part supplied by the
/// only two things available without a dependency — the process id and a
/// counter — and retried, because a collision must not be reported as the
/// caller's failure. `O_EXCL` semantics come free: `link(2)` fails with
/// `EEXIST` rather than clobbering, so a name that loses the race is simply
/// tried again.
///
/// The unlink of the temporary is unconditional, and gnulib's comment says why:
/// if `dsttmp` and `target` were already the same link, `renameat` is a no-op
/// that leaves both names, so the cleanup cannot be skipped on success
/// (`force-link.c:117`).
fn link_over(earlier: &Path, target: &Path) -> io::Result<()> {
    let (dir, base) = split_entry(target);
    for attempt in 0..PLACE_TEMP_TRIES {
        let mut name = OsString::from("Cu");
        name.push(format!("{:x}{attempt:x}", std::process::id()));
        // Beside the destination and not in `/tmp`: `rename` cannot cross a
        // filesystem, and the destination's directory is the only place
        // guaranteed to be on the same one.
        let tmp = dir.join(&name);
        match fs::hard_link(earlier, &tmp) {
            Ok(()) => {
                let result = fs::rename(&tmp, target);
                let _ = fs::remove_file(&tmp);
                return result;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    // Every candidate name was taken, which needs `PLACE_TEMP_TRIES`
    // simultaneous copies in one directory. Reported as an `EEXIST` rather than
    // as a panic.
    let _ = base;
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

/// How many temporary names [`link_over`] tries before giving up. gnulib's
/// `try_tempname_len` uses six random characters and the whole space; this
/// walks a counter instead, and the bound is what stops an unlucky directory
/// from spinning.
const PLACE_TEMP_TRIES: u32 = 64;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A directory that removes itself, so a failing test does not leave a name
    /// behind for the next run to trip over.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "coreutils-hardlink-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A free name is linked and *nothing* is said about it, because nothing was
    /// removed. The `removed` line being conditional is the whole reason
    /// [`force_link`] reports the two cases apart.
    #[test]
    fn a_free_name_takes_the_link_silently() {
        let dir = TempDir::new("free");
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"payload").unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert!(force_link("cp", &a, &b, true, &mut out, &mut err));
        assert_eq!(fs::read(&b).unwrap(), b"payload");
        assert!(out.is_empty(), "nothing was removed, so nothing is said");
        assert!(err.is_empty());
    }

    /// The case `link(2)` cannot do at all: a destination that is already
    /// something else. Under `-v` the replacement is announced.
    #[test]
    fn an_occupied_name_is_replaced_and_announced() {
        let dir = TempDir::new("taken");
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert!(force_link("cp", &a, &b, true, &mut out, &mut err));
        assert_eq!(fs::read(&b).unwrap(), b"new");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("removed {}\n", quoteaf_os(&b))
        );
        assert!(err.is_empty());
        // And no `CuXXXX` left behind: the temporary is removed on the success
        // path too, because a rename of a name onto its own link is a no-op that
        // would otherwise leave both.
        let leftovers: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .filter(|n| n.to_string_lossy().starts_with("Cu"))
            .collect();
        assert!(leftovers.is_empty(), "temporary left behind: {leftovers:?}");
    }

    /// Without `-v` the replacement is silent, which is what separates the
    /// `verbose` argument from the return value.
    #[test]
    fn the_removed_line_is_verbose_only() {
        let dir = TempDir::new("quiet");
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert!(force_link("cp", &a, &b, false, &mut out, &mut err));
        assert!(out.is_empty());
        assert!(err.is_empty());
    }

    /// The diagnostic names the destination first and carries the caller's own
    /// program name — the one thing the two callers do not share.
    #[test]
    fn a_failure_is_reported_destination_first_under_the_callers_name() {
        let dir = TempDir::new("fail");
        let missing = dir.path("nothing-here");
        let target = dir.path("b");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert!(!force_link(
            "mv", &missing, &target, true, &mut out, &mut err
        ));
        let said = String::from_utf8(err).unwrap();
        assert!(said.starts_with("mv: cannot create hard link "), "{said}");
        assert!(
            said.contains(&format!(
                "{} to {}",
                quoteaf_os(&target),
                quoteaf_os(&missing)
            )),
            "{said}"
        );
        assert!(out.is_empty(), "a failure says nothing on stdout");
    }
}
