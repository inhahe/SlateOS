//! The parts of "delete a tree" that `rm` and `mv` must agree on.
//!
//! `mv` acquired a recursive delete on 2026-09-03, when a move across a disk
//! boundary stopped being refused: such a move is a copy followed by a
//! recursive delete of the original, and there was nothing to call. That left
//! two walks in the zone — `rm.rs`'s `Rm::remove_tree` and `mv.rs`'s
//! `remove_tree` — which is what `known-issues.md` →
//! `TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS` is about. GNU has one,
//! `remove.c`, linked by both `rm.c` and `mv.c`; this module is the beginning
//! of the same arrangement here.
//!
//! It starts with the rule the two provably disagreed about rather than with
//! the walk, because the disagreement is what proved the entry's point. `mv`
//! reproduced upstream's substitution of an uninformative `rmdir` errno;
//! `rm` did not, and as a result `rm -r` refused to remove an empty directory
//! it could not *read*, where GNU removes it. Sharing [`is_uninformative`] is
//! what makes that one rule, in one place, for both.

use std::io;

/// Whether a failed `rmdir`'s errno is one upstream throws away in favour of an
/// earlier `opendir` failure (`remove.c:424`).
///
/// # Why a directory that cannot be read is still worth an `rmdir`
///
/// Reading a directory needs `r`; removing an empty one needs only `w`+`x` on
/// its *parent*. So `chmod 300 d` on an empty `d` is a directory nobody can
/// list and anybody can delete, and GNU deletes it: `fts` hands the entry over
/// as `FTS_DNR` and `remove.c:571` calls `excise` on it anyway. Measured
/// against GNU tar's sibling `rm` (coreutils 9.x) on 2026-09-03:
///
/// ```text
/// $ mkdir d && chmod 300 d && rm -rv d
/// removed directory 'd'
/// ```
///
/// The read error is therefore *held*, not reported — it only becomes the
/// diagnostic if the `rmdir` also fails, and then only when the `rmdir`'s own
/// errno says less than the read's did. `ENOTEMPTY` on a directory nobody could
/// open says less: it is the mechanical consequence of the entry that could not
/// be enumerated, and upstream's comment is that such errnos "would be
/// meaningless in a diagnostic" (`remove.c:420`). So `rm -r` on an unreadable
/// *non*-empty directory says `Permission denied`, not `Directory not empty`.
///
/// # The list
///
/// Upstream's verbatim, oddities included: `EISDIR` and `ENOTDIR` are there
/// because kernels have been observed to return them from `rmdir` on an
/// unreadable directory, and `EEXIST` because Solaris 10 spells `ENOTEMPTY`
/// that way.
///
/// The numbers are open-coded rather than taken from a `libc` binding because
/// this crate has none, and they are Linux's — which is also SlateOS's, since
/// `posix/src/errno.rs` is derived from the same table. The `ErrorKind` arm
/// below is what answers on the Windows development host, where the raw numbers
/// are the C runtime's and mean other things entirely.
#[must_use]
pub fn is_uninformative(err: &io::Error) -> bool {
    /// `ENOTEMPTY`, `EISDIR`, `ENOTDIR`, `EEXIST` — in the order `remove.c`
    /// lists them.
    const UNINFORMATIVE_CODES: &[i32] = &[
        39, // ENOTEMPTY
        21, // EISDIR
        20, // ENOTDIR
        17, // EEXIST
    ];
    if cfg!(unix)
        && err
            .raw_os_error()
            .is_some_and(|n| UNINFORMATIVE_CODES.contains(&n))
    {
        return true;
    }
    matches!(
        err.kind(),
        io::ErrorKind::DirectoryNotEmpty
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::NotADirectory
            | io::ErrorKind::AlreadyExists
    )
}

/// Which error to print when an `rmdir` failed and an earlier read had too.
///
/// The whole of the substitution rule in one place, so that neither caller has
/// to remember which way round it goes. `held` is the read failure, if there
/// was one; `failure` is what the `rmdir` answered.
///
/// The `rmdir` error wins whenever it says anything specific — `EACCES`,
/// `EBUSY`, `EROFS` are all more informative than a stale `EACCES` from the
/// listing — and loses only to [`is_uninformative`].
#[must_use]
pub fn blame<'a>(held: Option<&'a io::Error>, failure: &'a io::Error) -> &'a io::Error {
    match held {
        Some(earlier) if is_uninformative(failure) => earlier,
        _ => failure,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// The four codes, by number, which is the arm that runs on the target.
    #[cfg(unix)]
    #[test]
    fn the_four_upstream_codes_are_uninformative_by_number() {
        for code in [39, 21, 20, 17] {
            assert!(
                is_uninformative(&io::Error::from_raw_os_error(code)),
                "errno {code} should be uninformative"
            );
        }
    }

    /// ...and the ones that are not, which is what stops the rule swallowing a
    /// diagnostic that had something to say. `EACCES` is the important one: an
    /// `rmdir` that fails with it has found a real obstacle, and substituting
    /// the identical-looking earlier error would be harmless, but `EBUSY` and
    /// `EROFS` name obstacles the listing never saw.
    #[cfg(unix)]
    #[test]
    fn a_specific_errno_is_not_thrown_away() {
        for code in [13, 16, 30, 2] {
            assert!(
                !is_uninformative(&io::Error::from_raw_os_error(code)),
                "errno {code} should be kept"
            );
        }
    }

    /// The host arm, which has no errno numbers to match on. Portable, so it is
    /// the one test in here that runs on both targets.
    #[test]
    fn the_kinds_answer_where_the_numbers_cannot() {
        assert!(is_uninformative(&io::Error::from(
            io::ErrorKind::DirectoryNotEmpty
        )));
        assert!(is_uninformative(&io::Error::from(
            io::ErrorKind::AlreadyExists
        )));
        assert!(!is_uninformative(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!is_uninformative(&io::Error::from(io::ErrorKind::NotFound)));
    }

    /// `blame` substitutes only when there is something to substitute *and* the
    /// failure is one of the empty ones. Four combinations, all four asserted:
    /// a test that only checked the substituting case would pass against a
    /// `blame` that always substituted.
    #[test]
    fn blame_substitutes_only_the_uninformative_failure() {
        let held = io::Error::from(io::ErrorKind::PermissionDenied);
        let empty = io::Error::from(io::ErrorKind::DirectoryNotEmpty);
        let busy = io::Error::from(io::ErrorKind::ResourceBusy);

        assert_eq!(
            blame(Some(&held), &empty).kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            blame(Some(&held), &busy).kind(),
            io::ErrorKind::ResourceBusy
        );
        assert_eq!(blame(None, &empty).kind(), io::ErrorKind::DirectoryNotEmpty);
        assert_eq!(blame(None, &busy).kind(), io::ErrorKind::ResourceBusy);
    }
}
