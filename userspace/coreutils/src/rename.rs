//! A rename that must not destroy an existing destination — gnulib's
//! `renameatu` (`lib/renameatu.c`), narrowed to the one case the coreutils ask
//! for.
//!
//! Two utilities need it, for what look like different reasons and are the same
//! reason. [`backup`](crate::backup) renames `f` to `f.~4~` after scanning the
//! directory and finding `.~4~` free; `mv` renames the source onto the
//! destination *speculatively*, before it has decided whether the destination
//! is a directory, precisely so that a successful rename can stand as proof the
//! name was free. Both are statements of the form "I checked that this name was
//! free" — and a plain `rename(2)` cannot carry that statement, because the
//! check and the rename are two operations with a window between them. A name
//! created inside that window is silently overwritten, and in `backup`'s case
//! what is overwritten is another process's backup: the file the option exists
//! to preserve.
//!
//! `RENAME_NOREPLACE` closes the window by doing both under the kernel's own
//! lock. Ours honours it as of `6ea052654`
//! (`requests/b-a-rename-cannot-be-told-to-refuse-an-existing-target.md`), and
//! `posix::file::renameat2` forwards the flag word rather than refusing it, so
//! the atomic path is the one taken on the target.
//!
//! The fallback below is upstream's, for a host whose kernel does not know the
//! flag — a pre-3.15 Linux, and the Windows development host, where there is no
//! `renameat2` to call at all. It is `lstat` and then rename, with the race
//! back, and gnulib's comment says as much. Keeping it is not a hedge: without
//! it the utilities would not build on the host they are tested on.
//!
//! **This does not reach `SYS_FS_RENAMEAT_PINNED` (670), and that is a property
//! of `AT_FDCWD` rather than a gap here.** On the target, libc's `renameat`
//! prefers the pinned call — the kernel re-verifies the directory *handle*
//! instead of trusting the path a descriptor once had — but only when both ends
//! are a real directory fd and a name with no slashes in it. `AT_FDCWD` is
//! neither, so both callers take the path route, exactly as gnulib's
//! `renameatu` does on Linux. Reaching the pinned call would mean opening each
//! parent directory and splitting each operand into (parent, final component),
//! which moves `ENOENT` and `ENOTDIR` from the rename to the open and changes
//! the wording of the resulting diagnostic. It would also buy little: the pin
//! pays in recursive walks, which re-derive a directory by name between deciding
//! to descend into it and acting on it, and neither of these callers derives
//! anything — each is one rename of a name the caller was given.
//!
//! Sharing this between the two callers is the point of the module. They had a
//! copy each, and the copies had already diverged — `backup`'s called
//! `renameat2` and fell back, `mv`'s only ever emulated, so `mv` kept the race
//! on a kernel that no longer had one. The bug was not in either copy; it was
//! in there being two.

use std::fs;
use std::io;
use std::path::Path;

/// Rename `from` to `to`, refusing rather than replacing an existing `to`.
///
/// A *dangling* symlink at `to` counts as existing: it occupies the name, which
/// is the only question being asked.
///
/// # Errors
///
/// [`io::ErrorKind::AlreadyExists`] when `to` is taken — which is the answer
/// callers act on, and is why they must compare the *kind*: the atomic path
/// receives `EEXIST` from the kernel and the fallback synthesises it, and the
/// two have to be indistinguishable. Otherwise whatever the rename failed with.
pub fn noreplace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(unix)]
    if let Some(answer) = try_renameat2_noreplace(from, to) {
        return answer;
    }
    // The emulation, with the race upstream also has: something can create `to`
    // between this test and the rename below.
    match fs::symlink_metadata(to) {
        Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::rename(from, to),
        // Upstream treats `EOVERFLOW` as "it is there": a destination whose
        // metadata does not fit in a `stat` is still a destination. Every error
        // other than "not found" is read that way here for the same reason —
        // the one thing that licenses the rename is having *seen* the name free.
        Err(e) => Err(e),
    }
}

/// Ask the kernel, returning `None` when it turns out not to know how to answer.
///
/// `None` rather than an error because "this kernel has no such flag" is not a
/// fact about the rename; it is a fact about where we are running, and the
/// caller's response to it is to try a different way rather than to report a
/// failure.
#[cfg(unix)]
fn try_renameat2_noreplace(from: &Path, to: &Path) -> Option<io::Result<()>> {
    /// `AT_FDCWD`. Names relative to it are resolved from the working
    /// directory, which is where this module's callers' names already are.
    const AT_FDCWD: i32 = -100;
    /// `RENAME_NOREPLACE`.
    const NOREPLACE: u32 = 1;
    /// Upstream's three "the flag is not supported here" codes.
    const EINVAL: i32 = 22;
    /// See [`EINVAL`].
    const ENOSYS: i32 = 38;
    /// See [`EINVAL`].
    const ENOTSUP: i32 = 95;

    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const u8,
            newdirfd: i32,
            newpath: *const u8,
            flags: u32,
        ) -> i32;
    }

    // A name that cannot be made into a C string cannot be passed to the
    // syscall, but it can still be handed to `std`, which will produce the
    // right complaint about it. That is the fallback's job, not this one's.
    let (Ok(old), Ok(new)) = (crate::pathname::c_path(from), crate::pathname::c_path(to)) else {
        return None;
    };
    // SAFETY: both are NUL-terminated byte strings that outlive the call, and
    // `renameat2` does not retain either.
    let rc = unsafe { renameat2(AT_FDCWD, old.as_ptr(), AT_FDCWD, new.as_ptr(), NOREPLACE) };
    if rc == 0 {
        return Some(Ok(()));
    }
    let err = io::Error::last_os_error();
    if matches!(err.raw_os_error(), Some(EINVAL | ENOSYS | ENOTSUP)) {
        return None;
    }
    // Anything else is the rename's real answer and stands as it is.
    Some(Err(err))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory that removes itself, so a failing test does not leave a name
    /// behind for the next run to trip over.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "coreutils-rename-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
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

    #[test]
    fn a_free_name_is_taken() {
        let dir = TempDir::new("free");
        let (src, dst) = (dir.path("src"), dir.path("dst"));
        fs::write(&src, b"payload").expect("write src");
        noreplace(&src, &dst).expect("rename onto a free name");
        assert!(!src.exists());
        assert_eq!(fs::read(&dst).expect("read dst"), b"payload");
    }

    #[test]
    fn an_occupied_name_is_refused_and_neither_file_moves() {
        let dir = TempDir::new("taken");
        let (src, dst) = (dir.path("src"), dir.path("dst"));
        fs::write(&src, b"new").expect("write src");
        fs::write(&dst, b"old").expect("write dst");
        let err = noreplace(&src, &dst).expect_err("must refuse an occupied name");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        // The refusal is only worth anything if it left the destination alone.
        assert_eq!(fs::read(&src).expect("read src"), b"new");
        assert_eq!(fs::read(&dst).expect("read dst"), b"old");
    }

    /// The case the whole module exists for: a name that is *there* but that
    /// `metadata` cannot see, because following it leads nowhere. A check
    /// written with `Path::exists` — which follows — would call this free and
    /// then fail the rename with `EEXIST` anyway, or worse, succeed and destroy
    /// the link.
    #[test]
    #[cfg(unix)]
    fn a_dangling_symlink_occupies_the_name() {
        let dir = TempDir::new("dangling");
        let (src, dst) = (dir.path("src"), dir.path("dst"));
        fs::write(&src, b"new").expect("write src");
        std::os::unix::fs::symlink(dir.path("nowhere"), &dst).expect("symlink");
        assert!(
            !dst.exists(),
            "the link must be dangling for this to test it"
        );
        let err = noreplace(&src, &dst).expect_err("a dangling link still holds the name");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(fs::symlink_metadata(&dst).is_ok(), "link must survive");
    }

    #[test]
    fn a_missing_source_reports_the_missing_source() {
        let dir = TempDir::new("nosrc");
        let err =
            noreplace(&dir.path("absent"), &dir.path("dst")).expect_err("nothing to rename from");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
