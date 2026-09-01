//! The directory side of a recursive copy: making a destination directory,
//! reading a source one in the order GNU walks it, and the bookkeeping for the
//! permission bits that are deliberately withheld while a copy is in flight.
//!
//! Split out of `cp` so that `mv` can reach it. The two are one program
//! upstream — there is a single `copy_internal` in `copy.c`, parameterised by
//! `struct cp_options`, and `mv` is that engine with `move_mode` set
//! (`mv.c:119`) — so a second walk written for `mv` would be a second home for
//! every bug this one has. This module is the first instalment of putting them
//! back together; see `known-issues.md` →
//! `B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED` for the staged plan and for
//! why the stages are separated the way they are.
//!
//! Nothing here consults an option struct. That is the property that makes the
//! module shareable at all, and it is worth keeping as the rest of the engine
//! arrives: [`ModeDebt::new`] takes the single flag it needs as a `bool`, and
//! the other three functions take none.

use std::fs;
use std::io;
use std::path::Path;

/// The ways a destination's permission bits are deliberately not the ones it is
/// to end with, and what has to be done about each once it is safe.
///
/// GNU's four locals `omitted_permissions`, `restore_dst_mode`, `dst_mode`
/// (`copy.c:2211`) and `extra_permissions` (`copy.c:1246`), carried together
/// because they are one fact in four pieces: a destination is created with a
/// mode that is not its final one on purpose, and something has to remember how
/// it differs.
///
/// Two different reasons put bits in here, and they are the two branches of
/// GNU's expression at `copy.c:2900`:
///
/// * **The ownership is about to change.** Under `--preserve=ownership` the
///   destination is created with no group or other permissions at all
///   (`S_IRWXG | S_IRWXO`, i.e. `0o077`), because between the creation and the
///   `chown` it belongs to the *copying* user. A source that is group-readable
///   by its own owner's group would, for that window, be readable by the
///   copier's group instead — a different set of people.
/// * **It is a directory whose contents are not written yet.** Group- and
///   other-*write* (`0o022`) are withheld so that nobody can slip a file into a
///   directory that is about to look like a faithful copy.
///
/// The first subsumes the second, which is why GNU's expression is an
/// `if`/`else` rather than a union — and why a *regular file* has a debt at all
/// under `-p`, where before `-p` existed only directories did.
///
/// [`Self::extra`] goes the other way — a bit that is on the destination and
/// must come *off* — and is here rather than beside it because the settle-up is
/// one step for both: whichever of the two is non-zero, the answer is the same
/// chmod to the mode the file was always meant to have.
#[derive(Clone, Copy, Default)]
pub struct ModeDebt {
    /// GNU's `omitted_permissions`: the bits withheld at creation.
    pub omitted: u32,
    /// GNU's `restore_dst_mode` and `dst_mode` in one value. `Some(mode)` means
    /// the destination's mode has already been read and must be written back —
    /// either because a directory was forced owner-rwx so it could be filled,
    /// or because the settle-up stat showed the withheld bits genuinely absent.
    pub forced: Option<u32>,
    /// GNU's `extra_permissions` (`copy.c:1453`): owner-write, granted to a
    /// destination that is not meant to have it, so that its extended
    /// attributes can be written.
    ///
    /// Linux's `xattr_permission` (`fs/xattr.c`) requires write access to the
    /// *inode* before it will set an attribute on it, so a copy of a read-only
    /// file — mode `0444` — is a file no `setxattr` can reach. The bit is added
    /// at creation and taken off by the settle-up, and it costs no exposure
    /// while it is on: the owner at that instant is the process doing the
    /// copying, which already holds a writable descriptor to the file it just
    /// created.
    ///
    /// Zero unless the destination is newly created, extended attributes are
    /// being carried, and the caller is not root — root's `setxattr` is not
    /// subject to the check, which is why GNU's condition is
    /// `preserve_xattr && !x->owner_privileges`.
    pub extra: u32,
}

impl ModeDebt {
    /// GNU's `omitted_permissions = dst_mode_bits & (…)` (`copy.c:2899`).
    ///
    /// Takes the one flag it reads rather than an options struct, so that a
    /// caller with no such struct — `mv`, whose `preserve_ownership` is
    /// unconditionally true (`mv.c:134`) — can reach it too.
    #[must_use]
    pub fn new(preserve_ownership: bool, src_mode: u32, is_dir: bool) -> Self {
        let withhold = if preserve_ownership {
            0o077
        } else if is_dir {
            0o022
        } else {
            0
        };
        ModeDebt {
            omitted: src_mode & withhold,
            forced: None,
            // Not decided here. Whether the extra owner-write bit is wanted
            // depends on whether the destination turns out to be newly created,
            // which is not known until the open; GNU sets it in the same
            // expression as the open mode (`copy.c:1451`) for that reason.
            extra: 0,
        }
    }
}

/// Every entry of `src`, read in one go and put in the order GNU walks them.
///
/// This is gnulib's `savedir (dir, SAVEDIR_SORT_FASTREAD)`, which is what
/// `copy.c`'s `copy_dir` calls, reproduced for two reasons that are the same
/// reason twice.
///
/// **The order is observable now.** Until `--verbose` there was no way to tell
/// what order a tree was walked in — the copy it leaves is the same either way
/// — and `fs::read_dir`'s raw `readdir` order was as good as any. `cp -rv` puts
/// that order on stdout, and on ext4 the two disagree: a directory holding
/// `a.txt`, `sub` and `link` created in the order `sub`, `a.txt`, `link` is
/// named by GNU in creation order and by an unsorted `readdir` in hash order.
/// Neither is more correct, but only one of them is GNU's, and this program's
/// job is to be indistinguishable from GNU.
///
/// **And the order GNU picked is the fast one**, which is why gnulib calls it
/// `FASTREAD` rather than `SORTED`. Inode number is roughly on-disk position on
/// every filesystem that allocates inodes in tables, so walking a directory in
/// inode order turns the scattered reads of a `stat` per entry into a forward
/// scan. That is a real win on a cold cache and costs one sort of a list that
/// had to be materialised anyway.
///
/// The eager read is gnulib's too, and it changes one thing besides order: a
/// `readdir` that fails part-way through now abandons the whole directory
/// rather than copying the entries it had already seen. `savedir` returns
/// `NULL` in exactly that case, and `copy_dir` reports it as the one
/// `cannot access` diagnostic — so this is not a new behaviour so much as the
/// one GNU always had.
///
/// # Errors
///
/// Opening the directory, or any `readdir` within it.
pub fn read_dir_fastread(src: &Path) -> io::Result<Vec<fs::DirEntry>> {
    // `mut` is written only by the `#[cfg(unix)]` arm below. Off Unix there is
    // no inode to sort by, so the binding is never assigned to and the compiler
    // would rightly say so.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(src)?.collect::<io::Result<Vec<_>>>()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirEntryExt as _;
        // `d_ino` straight out of the `dirent`, not a `stat` — the sort must
        // not cost what it is there to save. Unstable sort because gnulib's
        // `qsort_r` is unstable too, and the only way to get a tie is two hard
        // links to one inode in one directory, where the two orders differ in
        // which of two names is copied first and in nothing else.
        entries.sort_unstable_by_key(fs::DirEntry::ino);
    }
    // Off Unix there is nothing to sort by, which is also gnulib's answer:
    // `SAVEDIR_SORT_FASTREAD` degrades to `SAVEDIR_SORT_NONE` where
    // `D_INO_IN_DIRENT` is not defined. See the `#[cfg(unix)]` arm above.
    Ok(entries)
}

/// Create `dest` as a directory with mode `mode`, before the umask is applied.
///
/// `Ok(true)` if it was created, `Ok(false)` if a directory was already there —
/// a distinction the caller needs, because an existing directory's mode is left
/// alone. Plain `create_dir` and not `create_dir_all`: GNU's single `mkdirat`
/// does not invent missing parents either, and `cp -r a no/such/dir` must fail
/// rather than quietly build the path.
pub fn make_dir(dest: &Path, mode: u32) -> io::Result<bool> {
    match create_dir_with_mode(dest, mode) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // Only "already there" if it is a *directory*. A regular file under
            // that name is a failure, and reporting it as one is what stops the
            // walk from writing a directory's contents into whatever it found.
            if fs::metadata(dest).is_ok_and(|m| m.is_dir()) {
                Ok(false)
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}

/// `mkdir(path, mode)`; the kernel narrows `mode` by the umask.
#[cfg(unix)]
pub fn create_dir_with_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(mode).create(path)
}

/// Windows has no mode to create a directory with, so the mode is dropped
/// rather than approximated — the same answer every other non-unix arm in
/// this crate gives. The target OS is the `#[cfg(unix)]` branch above.
#[cfg(not(unix))]
pub fn create_dir_with_mode(path: &Path, _mode: u32) -> io::Result<()> {
    fs::create_dir(path)
}
