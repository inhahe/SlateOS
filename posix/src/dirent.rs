//! POSIX directory entry functions.
//!
//! Implements `opendir`, `readdir`, `closedir`, `rewinddir`,
//! `seekdir`, `telldir`, `dirfd`, `alphasort`, and `scandir` for
//! directory iteration.
//!
//! ## Every stream is a descriptor, and every listing is read through it
//!
//! A directory stream here is an open file descriptor plus a snapshot of the
//! listing `SYS_FS_GETDENTS_PINNED` (664) took *through that descriptor*.
//! `opendir` is `open(O_RDONLY|O_DIRECTORY|O_CLOEXEC)` followed by
//! `fdopendir`, exactly as glibc's is, and nothing in this file names a
//! directory by path after the descriptor exists.
//!
//! That is a correctness property, not a tidiness one.  Until 2026-09-01 both
//! entry points listed by **path** (`SYS_FS_LIST_DIR`, 603): `opendir` passed
//! the caller's path, and `fdopendir` passed the path the descriptor *had
//! when it was opened*, retrieved from `fdtable`.  A descriptor exists
//! precisely so that the object cannot be substituted underneath you, and
//! re-deriving a path from one throws that away — rename the directory and
//! the listing is of something else or of nothing; replace a component with a
//! symlink and the listing is of wherever the attacker pointed.  It also made
//! `fdopendir` fail outright on any descriptor this libc did not open.
//!
//! Three further things fall out of the switch, and are the reason the whole
//! file changed rather than one call:
//!
//! - **`d_ino` is the filesystem's inode number.**  603's record has no room
//!   for one, so `readdir` used to report the entry's *position in the
//!   listing* — a number that is never the real inode, so `find -inum`,
//!   `ls -i` against `stat`, and the hard-link detection in `tar`, `rsync`
//!   and `du` were all silently wrong rather than visibly broken.  664's
//!   record carries the same value `SYS_FS_GET_META` reports as `st_ino`,
//!   and it is passed through untouched — **including `0`**, which means the
//!   filesystem has no stable per-object identity (`procfs`, `sysfs`,
//!   `devfs`, `iso9660`, an unallocated FAT file), not that the entry is
//!   deleted.  See design-decisions.md §740.
//! - **There is no 256-entry cap.**  603 wrote fixed 264-byte records into a
//!   fixed buffer and quietly stopped at 256 of them.  664's records are
//!   `21 + name_len` bytes and it reports the size the *complete* listing
//!   needs, so [`slurp_listing`] can ask again with a big enough buffer
//!   instead of returning a directory that is missing files.
//! - **`dirfd` works on every stream**, because every stream now has a
//!   descriptor.  It used to return -1 for `opendir`-created ones, which is
//!   not a value POSIX allows on a valid `DIR *` and which broke the
//!   `dirfd(dirp)` + `*at` idiom that exists to avoid path races.
//!
//! ## Limitations
//!
//! - [`MAX_OPEN_DIRS`] concurrent open streams; `opendir` reports `EMFILE`
//!   beyond that.
//! - Entries whose name does not fit `d_name` (255 bytes plus a terminator)
//!   are skipped rather than truncated: a truncated name denotes a different
//!   file, and acting on it would be worse than not seeing it.  That, and an
//!   empty name, are the *only* reasons an entry is dropped — nothing is
//!   filtered by type, deliberately (see [`kernel_type_to_dt`]).

use crate::errno;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// The packed record 647 and 664 share
// ---------------------------------------------------------------------------

/// Bytes a packed listing record spends on everything except the name:
/// `u8 entry_type | u32 name_len | … | u64 size | u64 ino`.
const PACKED_FIXED_LEN: usize = 21;

/// Bytes of listing the first [`slurp_listing`] round asks the kernel for.
///
/// Sized so that the overwhelming majority of directories arrive in one
/// syscall and one allocation: at a 20-byte average name, 8 KiB is ~200
/// entries.  Overshooting costs untouched memory for the life of the stream;
/// undershooting costs a second syscall, never a truncated listing.
const DIR_BUF_INITIAL: usize = 8 * 1024;

/// One entry decoded from the packed listing format 647 and 664 share.
///
/// The record's `size` field is deliberately not decoded — `Dirent` has
/// nowhere to put it and `linux_dirent64` has no such field either, so
/// keeping it would be an unread copy.  It is still accounted for in
/// [`PackedEntry::record_len`], which is what the caller advances by.
struct PackedEntry<'a> {
    /// Kernel type code — one of the `KERNEL_TYPE_*` constants below, which
    /// is **not** a `DT_*` value; run it through [`kernel_type_to_dt`].
    kernel_type: u8,
    /// The entry's name, exactly as the filesystem stores it.  Not
    /// NUL-terminated and not required to be UTF-8.
    name: &'a [u8],
    /// The filesystem's inode number, or `0` where it has no stable
    /// per-object identity.  Passed to callers verbatim; see the module doc.
    ino: u64,
    /// Bytes this record occupies, i.e. how far to advance to reach the next.
    record_len: usize,
}

/// Decode the record at the front of `buf`, if a whole one is there.
///
/// Returns `None` for a buffer that is empty, that is shorter than the record
/// it announces, or whose `name_len` overflows a `usize` — all of which mean
/// "stop", because the format is not self-synchronising and there is no way
/// to find the next record without trusting the length in this one.  The
/// kernel truncates only at a record boundary, so a `None` before the end of
/// the listing indicates a corrupt buffer rather than a normal short read.
///
/// A pure function over bytes so it can be tested on the host, where the
/// syscall that produces these records answers `ENOSYS` and cannot be.
fn decode_packed_entry(buf: &[u8]) -> Option<PackedEntry<'_>> {
    let kernel_type = *buf.first()?;
    let name_len_bytes: [u8; 4] = buf.get(1..5)?.try_into().ok()?;
    let name_len = usize::try_from(u32::from_le_bytes(name_len_bytes)).ok()?;
    let record_len = PACKED_FIXED_LEN.checked_add(name_len)?;
    if record_len > buf.len() {
        return None;
    }
    let name = buf.get(5..5usize.checked_add(name_len)?)?;
    // `ino` is the last eight bytes of the record; `size` occupies the eight
    // before it and is skipped.
    let ino_at = record_len.checked_sub(8)?;
    let ino_bytes: [u8; 8] = buf.get(ino_at..record_len)?.try_into().ok()?;
    Some(PackedEntry {
        kernel_type,
        name,
        ino: u64::from_le_bytes(ino_bytes),
        record_len,
    })
}

// ---------------------------------------------------------------------------
// dirent — POSIX directory entry
// ---------------------------------------------------------------------------

/// Directory entry type constants for `Dirent::d_type` / `getdents64`.
///
/// These are the Linux `<dirent.h>` ABI values (DT_REG=8, DT_DIR=4,
/// DT_LNK=10, …), re-exported from `linux_dirent_types` which is the
/// single source of truth.  Ported programs compiled against Linux/musl
/// headers compare `d_type` against these exact numbers, so we must
/// expose them — NOT the compact kernel type codes (see below).
pub use crate::linux_dirent_types::{
    DT_BLK, DT_CHR, DT_DIR, DT_FIFO, DT_LNK, DT_REG, DT_SOCK, DT_UNKNOWN,
};

/// Kernel directory-entry / stat type codes.
///
/// `SYS_FS_READDIR_AT` (647) and `SYS_FS_GETDENTS_PINNED` (664) write one of
/// these as the first byte of each packed record, `SYS_FS_LIST_DIR` (603)
/// writes one at byte offset 260 of each of its fixed 264-byte entries, and
/// `SYS_FS_STAT` uses the same encoding in its `entry_type` field.  These
/// are an internal kernel ABI and are deliberately NOT the same as the Linux
/// `DT_*` values above — every consumer that reads the raw kernel byte must
/// translate via [`kernel_type_to_dt`] before exposing it as a `d_type`.
///
/// **Read the serializers, not the comments, when checking these.**  647's
/// own ABI note in `kernel/src/syscall/number.rs` documented the table as
/// `2=symlink, 3=volume_label` until 2026-08-31 — the reverse of what all
/// three handlers emit, and the *only* documentation of the byte for 664,
/// whose note defers to it.  It was reported as
/// `requests/b-a-the-647-664-entry-type-table-has-symlink-and-volume-label-swapped.md`
/// and lane A corrected the note; the values below have always been taken
/// from the emitters rather than from it, which is why nothing here changed
/// when it was fixed.  Keep it that way: a symlink decoded as a volume label
/// is a *plausible* value rather than a fault, so a recursive copy built from
/// the wrong table would dereference every link it walked and no return code
/// would say so.
pub(crate) const KERNEL_TYPE_FILE: u8 = 0;
pub(crate) const KERNEL_TYPE_DIR: u8 = 1;
pub(crate) const KERNEL_TYPE_VOLLABEL: u8 = 2;
pub(crate) const KERNEL_TYPE_SYMLINK: u8 = 3;
pub(crate) const KERNEL_TYPE_CHARDEV: u8 = 4;
pub(crate) const KERNEL_TYPE_BLOCKDEV: u8 = 5;

/// Translate a kernel directory-entry type byte into a POSIX `DT_*` value.
///
/// Every code the kernel defines is translated; only a code it does not
/// define reaches `DT_UNKNOWN`.  That matters more than it looks, because
/// `DT_UNKNOWN` is not a neutral answer — it tells the caller "stat this
/// entry to find out", so a wrongly-unknown entry costs a syscall per
/// listing at best and a wrong decision at worst.
///
/// This function used to carry the comment "`SYS_FS_LIST_DIR` only ever
/// emits file/dir/symlink (volume labels are filtered out kernel-side)" and
/// had no arms for `CharDevice` or `BlockDevice` — with `DT_CHR` and
/// `DT_BLK` imported a dozen lines above.  The first half of that sentence
/// was false when it was written: 603 has emitted 4 and 5 since it gained
/// device support, `devfs` produces both, and so every device node in
/// `readdir("/dev")` came back `DT_UNKNOWN`.  The arms below are exhaustive
/// over the kernel's codes precisely so that the next code the kernel adds
/// shows up as a compile-time gap here rather than as another silent
/// `DT_UNKNOWN`.
pub(crate) fn kernel_type_to_dt(kernel_type: u8) -> u8 {
    match kernel_type {
        KERNEL_TYPE_DIR => DT_DIR,
        KERNEL_TYPE_SYMLINK => DT_LNK,
        KERNEL_TYPE_FILE => DT_REG,
        KERNEL_TYPE_CHARDEV => DT_CHR,
        KERNEL_TYPE_BLOCKDEV => DT_BLK,
        // A volume label genuinely has no `DT_*`: it is not a file, has no
        // inode, and cannot be opened or statted.  `DT_UNKNOWN` is the
        // honest answer even though the follow-up stat it invites will
        // fail.  In practice this arm is unreachable from a listing — the
        // VFS drops labels on every route (`Vfs::drop_volume_labels`) and
        // the kernel documents `2` as reserved and never emitted — but
        // `SYS_FS_STAT` shares this encoding, so the code has a meaning and
        // deserves an answer rather than falling into the catch-all.
        //
        // Translating it is deliberately all we do: `readdir` and
        // `fill_dirent64_batch` do *not* filter type 2 out.  A second filter
        // in libc would not add a defence, it would add a place for the
        // kernel's to fail invisibly — a regression in `drop_volume_labels`
        // would fail lane A's `fat::mkfs_self_test` loudly while every real
        // program on the system saw a listing we had quietly repaired.  If a
        // label ever does arrive, it should show up as an odd entry nothing
        // can open, which is a bug report; hiding it makes it a mystery.
        // Lane A asked for exactly this in
        // `requests/a-b-the-volume-label-divergence-did-not-exist-and-now-it-cannot.md`.
        KERNEL_TYPE_VOLLABEL => DT_UNKNOWN,
        _ => DT_UNKNOWN,
    }
}

/// POSIX directory entry.
#[repr(C)]
pub struct Dirent {
    /// The filesystem's inode number for the object this name refers to, as
    /// the kernel reported it — the same value `stat` puts in `st_ino`.
    ///
    /// `0` is a real answer and not an error: it means the filesystem has no
    /// stable per-object identity to report (`procfs`, `sysfs`, `devfs`,
    /// `iso9660`, a FAT file with no allocated cluster).  It is passed
    /// through rather than replaced with an invented number; see
    /// design-decisions.md §740 for why, and for the one Linux idiom
    /// (treating `d_ino == 0` as a deleted entry) that the choice trades
    /// against.
    pub d_ino: InoT,
    /// Cookie identifying the *next* entry — the value [`telldir`] would
    /// return after this one, and the value [`seekdir`] takes to come back
    /// to it.  Opaque; do not do arithmetic on it.
    pub d_off: OffT,
    /// Length of this record.
    pub d_reclen: u16,
    /// File type (DT_REG, DT_DIR, etc.).
    pub d_type: u8,
    /// Null-terminated filename.
    pub d_name: [u8; 256],
}

/// Opaque directory stream handle.
///
/// A descriptor, plus the snapshot of the listing that
/// `SYS_FS_GETDENTS_PINNED` took through it, plus a cursor into that
/// snapshot.  The snapshot is taken once at [`opendir`]/[`fdopendir`] time
/// and re-taken by [`rewinddir`], which is what POSIX requires of those three
/// and nothing more: entries created after the stream was opened may or may
/// not appear, and that latitude is what lets one syscall serve a whole walk.
pub struct Dir {
    /// Packed 664 records, `malloc`'d and owned by this stream.
    ///
    /// Null only between [`alloc_dir`] and the fill that follows it, and
    /// again after [`Dir::release`].  Read through [`snapshot`], which is
    /// the one place that turns it into a slice.
    buf: *mut u8,
    /// Bytes of records in `buf`.
    len: usize,
    /// Byte offset of the next record to decode.
    ///
    /// A byte offset rather than an entry index because that is what
    /// [`telldir`] hands out and [`seekdir`] takes back, and a variable-length
    /// format has no cheaper way to seek: an index would have to be walked
    /// from the start on every `seekdir`.
    pos: usize,
    /// Scratch space for the dirent we return.
    current: Dirent,
    /// The descriptor this stream reads through; [`closedir`] closes it.
    ///
    /// Always a real descriptor on a live stream — [`opendir`] opens one of
    /// its own — so [`dirfd`] never has to answer -1 for a valid `DIR *`.
    owned_fd: i32,
}

/// A snapshot's records as a slice; empty for a stream not yet filled.
///
/// Takes the pointer and length by value rather than `&self` so that the
/// borrow produced is of the *buffer*, not of the [`Dir`] that owns it:
/// [`readdir`] decodes an entry out of the snapshot and copies it into
/// `Dir::current` in the same breath, and a `&self` method would make that a
/// borrow conflict even though the two touch unrelated memory.
///
/// # Safety
///
/// `buf`/`len` must describe a live snapshot — either null and 0, or the
/// allocation a `Dir` currently owns and has not yet passed to
/// [`Dir::release`].  The returned lifetime is unconstrained, so the caller
/// must not hold the slice across a `release` or a re-fill.
unsafe fn snapshot<'a>(buf: *const u8, len: usize) -> &'a [u8] {
    if buf.is_null() {
        return &[];
    }
    // SAFETY: the caller guarantees `buf` is a live `slurp_listing`
    // allocation of at least `len` bytes.  A `DIR *` is owned by one caller
    // at a time — POSIX leaves concurrent use of a single stream undefined —
    // so no other thread is writing it.
    unsafe { core::slice::from_raw_parts(buf, len) }
}

impl Dir {
    /// Drop the snapshot and rewind.  Idempotent.
    fn release(&mut self) {
        if !self.buf.is_null() {
            // SAFETY: `buf` is non-null here, came from `crate::malloc::malloc`
            // in `slurp_listing`, and is nulled immediately so no second free
            // can reach it.
            unsafe { crate::malloc::free(self.buf) };
            self.buf = core::ptr::null_mut();
        }
        self.len = 0;
        self.pos = 0;
    }
}

// ---------------------------------------------------------------------------
// Reading a listing through a descriptor
// ---------------------------------------------------------------------------

/// Read the complete listing of the directory `handle` denotes.
///
/// Returns a `malloc`'d buffer the caller owns and the number of bytes of
/// packed records in it, or `None` with `errno` set.
///
/// # Why the loop has no trip count
///
/// 664 is unpaginated: it answers with the size the **whole** listing
/// occupies, which may exceed the buffer it was handed, and the only way to
/// get the rest is to ask again with a bigger one.  A directory that grows
/// between the two calls can overflow the second buffer too, so this has to
/// be a loop rather than a retry.
///
/// It has no iteration limit because every plausible limit is wrong in one
/// direction or the other, and because the loop already terminates for a
/// reason: each round asks for at least **twice** what the last one did, so a
/// directory would have to double in size every round to stay ahead of it,
/// and long before that the doubling exhausts memory and `malloc` fails —
/// which is reported as `ENOMEM` rather than papered over.
///
/// The alternative, accepting a truncated listing, is what the old
/// `SYS_FS_LIST_DIR` route did at 256 entries.  A listing that is short by a
/// few names is not a degraded answer; it is `rm -r` reporting success over a
/// directory it did not empty and `du` under-reporting a tree.
fn slurp_listing(handle: u64) -> Option<(*mut u8, usize)> {
    let mut cap = DIR_BUF_INITIAL;
    loop {
        let buf = crate::malloc::malloc(cap);
        if buf.is_null() {
            errno::set_errno(errno::ENOMEM);
            return None;
        }
        let ret = syscall3(SYS_FS_GETDENTS_PINNED, handle, buf as u64, cap as u64);
        if ret < 0 {
            // SAFETY: `buf` is the non-null pointer `malloc` just returned
            // and nothing else has seen it.
            unsafe { crate::malloc::free(buf) };
            let _ = errno::translate(ret); // Called for its side effect: sets errno.
            return None;
        }
        // The return is the size of the *complete* listing, not the bytes
        // written — that is the whole reason a directory which exactly filled
        // the buffer is distinguishable from one that overflowed it.
        let Ok(need) = usize::try_from(ret) else {
            errno::set_errno(errno::ENOMEM);
            // SAFETY: as above.
            unsafe { crate::malloc::free(buf) };
            return None;
        };
        if need <= cap {
            return Some((buf, need));
        }
        // SAFETY: as above; `buf` has not been handed to anyone.
        unsafe { crate::malloc::free(buf) };
        let Some(next) = need.max(cap).checked_mul(2) else {
            errno::set_errno(errno::ENOMEM);
            return None;
        };
        cap = next;
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Open a directory stream.
///
/// Returns a pointer to a Dir, or NULL on error.
///
/// This is `open` followed by [`fdopendir`], which is how glibc's is built
/// too, and it is why the errors below are `open`'s errors: `ENOENT` for a
/// name that does not exist (including the empty one), `ENOTDIR` for a name
/// that is not a directory, `ENAMETOOLONG`, `EACCES`, `ELOOP`.  Doing the
/// resolution here as well would be a second place that has to agree with
/// `open` about all of them.
///
/// `O_CLOEXEC` is not optional decoration: POSIX requires the descriptor
/// underlying a `DIR *` to have `FD_CLOEXEC` set, so that a stream held open
/// across a `posix_spawn` does not leak a directory handle into the child.
///
/// The `Dir` itself comes from a static pool — we are `no_std` and a `DIR *`
/// must outlive this call — so at most [`MAX_OPEN_DIRS`] streams can be open
/// at once, beyond which this reports `EMFILE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn opendir(name: *const u8) -> *mut Dir {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }

    let fd = crate::file::open(
        name,
        crate::fcntl::O_RDONLY | crate::fcntl::O_DIRECTORY | crate::fcntl::O_CLOEXEC,
        0,
    );
    if fd < 0 {
        return core::ptr::null_mut(); // errno already set by `open`.
    }

    let dirp = fdopendir(fd);
    if dirp.is_null() {
        // A failed `fdopendir` deliberately leaves the descriptor alone,
        // because POSIX says a caller's fd must survive one.  This one is
        // ours, so it has to be closed here — and the diagnosis carried
        // across the close, which sets `errno` on its own failures.
        let failure = errno::get_errno();
        crate::file::close(fd);
        errno::set_errno(failure);
    }
    dirp
}

/// Read the next directory entry.
///
/// Returns a pointer to a `Dirent`, or NULL when the directory
/// is exhausted (end of listing).
///
/// Two kinds of record are stepped over rather than returned.  A **volume
/// label** (kernel type 2) is filesystem metadata that FAT stores in a
/// root-directory slot, not a name the directory contains; the VFS drops
/// them on every listing route and the type byte is documented as reserved,
/// so this arm should be unreachable — it is here because a label surfacing
/// as an unopenable entry in `/` would be a puzzling bug rather than a loud
/// one.  A **name too long for `d_name`** is skipped for a stronger reason:
/// `d_name` is 256 bytes including the terminator, and a truncated name
/// denotes a *different file*, so returning one invites the caller to unlink
/// or overwrite something it never saw.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readdir(dirp: *mut Dir) -> *mut Dirent {
    if dirp.is_null() {
        errno::set_errno(errno::EBADF);
        return core::ptr::null_mut();
    }

    // SAFETY: `dirp` is non-null and, per this function's C contract, a
    // pointer `opendir`/`fdopendir` returned and `closedir` has not taken.
    let dir = unsafe { &mut *dirp };

    loop {
        // SAFETY: `dir` is live, so its snapshot is too — nothing between
        // here and the `return` releases or re-fills it.
        let records = unsafe { snapshot(dir.buf, dir.len) };
        let Some(rest) = records.get(dir.pos..) else {
            return core::ptr::null_mut();
        };
        if rest.is_empty() {
            return core::ptr::null_mut(); // End of directory.
        }
        let Some(entry) = decode_packed_entry(rest) else {
            // A partial record can only mean a corrupt buffer: the kernel
            // truncates at record boundaries.  Stop rather than resynchronise
            // on bytes we cannot interpret.
            dir.pos = dir.len;
            return core::ptr::null_mut();
        };
        let next = dir.pos.wrapping_add(entry.record_len);
        let name_len = entry.name.len();
        // The only entries dropped are ones that cannot be *represented*: a
        // name with no room for its terminator would have to be truncated, and
        // a truncated name denotes a different file.  Nothing is dropped for
        // being the wrong kind of thing — see `kernel_type_to_dt` for why a
        // volume label is not filtered here even though it has no POSIX type.
        let fits = name_len < dir.current.d_name.len();
        if !fits || name_len == 0 {
            dir.pos = next;
            continue;
        }

        dir.current.d_name = [0u8; 256];
        if let Some(dst) = dir.current.d_name.get_mut(..name_len) {
            dst.copy_from_slice(entry.name);
        }
        dir.current.d_type = kernel_type_to_dt(entry.kernel_type);
        // The kernel's inode number, verbatim — including a `0`, which says
        // this filesystem has no stable identity for the object rather than
        // that the entry is gone.  See design-decisions.md §740.
        dir.current.d_ino = entry.ino;
        dir.current.d_reclen = core::mem::size_of::<Dirent>() as u16;
        dir.pos = next;
        // `d_off` is the cookie for the *next* entry, which is what Linux
        // puts there and what makes `seekdir(telldir())` land on the entry
        // after this one rather than repeating it.
        let Ok(off) = i64::try_from(dir.pos) else {
            return core::ptr::null_mut();
        };
        dir.current.d_off = off;

        return core::ptr::addr_of_mut!(dir.current);
    }
}

/// Close a directory stream.
///
/// Returns 0 on success, -1 on error.
///
/// A NULL `dirp` gives `EINVAL`, not `EBADF`: glibc's Linux `closedir`
/// (`sysdeps/unix/sysv/linux/closedir.c`, checked against 2.39) opens
/// with `if (dirp == NULL) { __set_errno (EINVAL); return -1; }` before
/// it reaches the descriptor, so a Linux caller never sees `EBADF` for
/// this case — the descriptor is not consulted at all.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn closedir(dirp: *mut Dir) -> i32 {
    if dirp.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: dirp is valid (checked above) and is a `DIR *` this module
    // handed out, so it points into `DIR_POOL` and nobody else holds it.
    let dir = unsafe { &mut *dirp };
    dir.release();
    let owned_fd = dir.owned_fd;
    dir.owned_fd = -1;
    if owned_fd >= 0 {
        crate::file::close(owned_fd);
    }

    free_dir(dirp);
    0
}

/// Reset a directory stream to the beginning **and re-read the directory**.
///
/// The second half is not an extra: POSIX says `rewinddir` "shall also cause
/// the directory stream to refer to the current state of the corresponding
/// directory, as a call to `opendir()` would have done".  This used to reset
/// the cursor alone, so a caller that created files and rewound to find them
/// — the ordinary way to poll a spool or a lock directory — saw the same
/// stale snapshot forever.
///
/// It cannot report a failure, because the function returns `void` and POSIX
/// defines no errors for it.  So a re-read that fails leaves the previous
/// snapshot in place and rewinds within it, which is the behaviour of the
/// version that never re-read at all: strictly no worse than before, and the
/// only alternative — discarding the listing — would turn a transient error
/// into an empty directory.  `errno` is restored across the attempt for the
/// same reason: a caller checking it after an unrelated call must not find it
/// carrying a diagnosis this function chose not to report.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rewinddir(dirp: *mut Dir) {
    if dirp.is_null() {
        return;
    }
    // SAFETY: dirp is valid (caller contract).
    let dir = unsafe { &mut *dirp };
    dir.pos = 0;

    let saved = errno::get_errno();
    if let Some(handle) = crate::file::pinned_base(dir.owned_fd) {
        if let Some((buf, len)) = slurp_listing(handle) {
            dir.release();
            dir.buf = buf;
            dir.len = len;
        }
    }
    errno::set_errno(saved);
}

/// Return the current position in the directory stream.
///
/// The value is an opaque cookie — in this implementation the byte offset of
/// the next record in the buffered listing — and its only defined use is to
/// be handed back to [`seekdir`] on the *same* stream before it is closed.
/// POSIX says as much; it is spelled out here because the value used to be a
/// plain entry index, which was tempting to do arithmetic on.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn telldir(dirp: *mut Dir) -> i64 {
    if dirp.is_null() {
        return -1;
    }
    // SAFETY: dirp is valid.
    let pos = unsafe { (*dirp).pos };
    i64::try_from(pos).unwrap_or(-1)
}

/// Set the position of the directory stream.
///
/// `loc` must be a value [`telldir`] returned for this stream.  A value from
/// anywhere else lands mid-record, and the decoder then reads the record's
/// own bytes as a header — which yields nonsense entries, not memory unsafety
/// (every read is bounds-checked against the buffer), and stops at the first
/// length that does not fit.  Out-of-range values are clamped to end-of-
/// directory rather than rejected, because the function cannot report an
/// error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn seekdir(dirp: *mut Dir, loc: i64) {
    if dirp.is_null() || loc < 0 {
        return;
    }
    let Ok(want) = usize::try_from(loc) else {
        return;
    };
    // SAFETY: dirp is valid.
    unsafe {
        let max = (*dirp).len;
        (*dirp).pos = if want > max { max } else { want };
    }
}

/// Get the file descriptor associated with a directory stream.
///
/// Every live stream has one — [`opendir`] opens its own — so this answers -1
/// only for a null `dirp`.  It used to answer -1 for any `opendir`-created
/// stream, which POSIX does not allow and which silently broke the
/// `dirfd(dirp)` + `openat`/`unlinkat` idiom that exists precisely to keep a
/// walk anchored to a descriptor instead of a path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dirfd(dirp: *mut Dir) -> i32 {
    if dirp.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: dirp is valid (caller contract).
    unsafe { (*dirp).owned_fd }
}

/// Compare two directory entries alphabetically by name.
///
/// Suitable as a comparator for `scandir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn alphasort(a: *const *const Dirent, b: *const *const Dirent) -> i32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    // SAFETY: a and b point to valid Dirent pointers.
    let da = unsafe { &**a };
    let db = unsafe { &**b };

    // Compare d_name byte by byte.
    let mut i: usize = 0;
    loop {
        let ca = da.d_name.get(i).copied().unwrap_or(0);
        let cb = db.d_name.get(i).copied().unwrap_or(0);
        if ca != cb {
            return i32::from(ca).wrapping_sub(i32::from(cb));
        }
        if ca == 0 {
            return 0;
        }
        i = i.wrapping_add(1);
    }
}

/// Insertion sort a `*mut Dirent` array of `count` elements using `cmp`.
///
/// # Safety
///
/// `arr` must point to `count` valid, writable `*mut Dirent` entries.
/// `cmp` must be a valid function pointer that never returns from a panic.
///
/// # Alignment note
///
/// `arr` is cast from a `*mut u8` returned by `malloc`.  Our `malloc`
/// implementation uses `mmap`, which returns page-aligned memory (≥ 4096
/// bytes), far exceeding the 8-byte alignment required for `*mut Dirent`.
#[allow(clippy::cast_ptr_alignment)]
unsafe fn scandir_sort(
    arr: *mut u8,
    count: usize,
    cmp: extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32,
) {
    // SAFETY: caller guarantees arr is page-aligned and has count entries.
    let arr_typed = arr.cast::<*mut Dirent>();
    let mut i: usize = 1;
    while i < count {
        let mut j = i;
        while j > 0 {
            // SAFETY: j and j-1 are valid indices within [0, count).
            let a = unsafe { arr_typed.add(j.wrapping_sub(1)) };
            let b = unsafe { arr_typed.add(j) };
            if cmp(a.cast::<*const Dirent>(), b.cast::<*const Dirent>()) > 0 {
                // SAFETY: a and b are valid, non-overlapping, aligned pointers.
                unsafe {
                    core::ptr::swap(a, b);
                }
                j = j.wrapping_sub(1);
            } else {
                break;
            }
        }
        i = i.wrapping_add(1);
    }
}

/// Scan a directory and return a sorted array of matching entries.
///
/// If `filter` is non-null, only entries for which `filter(entry)` returns
/// non-zero are included.  The resulting array is sorted using `compar`
/// (if non-null).
///
/// On success, `*namelist` is set to a `malloc`'d array of `malloc`'d
/// `Dirent` pointers, and the function returns the number of entries.
/// The caller must `free()` each entry and the array itself.
///
/// On failure, returns -1 with errno set.
///
/// # Safety
///
/// `dirname` must be a valid null-terminated path.
/// `namelist` must point to a valid `*mut *mut Dirent` location.
///
/// # Alignment note
///
/// Pointer casts from `*mut u8` (returned by `malloc`) to `*mut *mut Dirent`
/// and `*mut Dirent` are safe because our `malloc` uses `mmap`, which
/// returns page-aligned memory (≥ 4096 bytes).
#[allow(clippy::cast_ptr_alignment)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scandir(
    dirname: *const u8,
    namelist: *mut *mut *mut Dirent,
    filter: Option<extern "C" fn(*const Dirent) -> i32>,
    compar: Option<extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32>,
) -> i32 {
    if dirname.is_null() || namelist.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Open the directory.
    let dirp = opendir(dirname);
    if dirp.is_null() {
        return -1; // errno already set by opendir.
    }

    // First pass: count matching entries.  Two-pass approach avoids
    // over-allocating when a filter rejects many entries.
    let mut count: usize = 0;
    loop {
        let entry = readdir(dirp);
        if entry.is_null() {
            break;
        }
        if filter.is_none_or(|f| f(entry) != 0) {
            count = count.wrapping_add(1);
        }
    }

    if count == 0 {
        closedir(dirp);
        // Allocate an empty array (POSIX allows returning 0 with a non-null
        // but empty namelist).
        let arr = crate::malloc::malloc(core::mem::size_of::<*mut Dirent>());
        if arr.is_null() {
            errno::set_errno(errno::ENOMEM);
            return -1;
        }
        // SAFETY: arr is page-aligned (mmap), so align ≥ 8.
        unsafe {
            *namelist = arr.cast::<*mut Dirent>();
        }
        return 0;
    }

    // Allocate the output array.
    let arr_size = count.wrapping_mul(core::mem::size_of::<*mut Dirent>());
    let arr = crate::malloc::malloc(arr_size);
    if arr.is_null() {
        closedir(dirp);
        errno::set_errno(errno::ENOMEM);
        return -1;
    }
    // SAFETY: arr is page-aligned (mmap), align ≥ 8.
    let arr_typed = arr.cast::<*mut Dirent>();

    // Second pass: collect matching entries into the array.
    //
    // The cursor is reset directly rather than with `rewinddir`, which now
    // re-reads the directory.  The two passes must walk the *same* snapshot:
    // the array was sized from the first pass's count, so a second pass over
    // a directory that gained entries would have to drop the surplus, and one
    // over a directory that lost entries would return a short array whose
    // length disagrees with what the caller was told.
    // SAFETY: `dirp` is a live stream this function opened.
    unsafe {
        (*dirp).pos = 0;
    }
    let mut idx: usize = 0;
    loop {
        let entry = readdir(dirp);
        if entry.is_null() {
            break;
        }
        if filter.is_none_or(|f| f(entry) != 0) && idx < count {
            let dup = crate::malloc::malloc(core::mem::size_of::<Dirent>());
            if dup.is_null() {
                // OOM: free everything allocated so far then bail.
                let mut j: usize = 0;
                while j < idx {
                    // SAFETY: valid pointers written at indices < idx.
                    unsafe {
                        crate::malloc::free((*arr_typed.add(j)).cast::<u8>());
                    }
                    j = j.wrapping_add(1);
                }
                // SAFETY: arr allocated by malloc above.
                unsafe {
                    crate::malloc::free(arr);
                }
                closedir(dirp);
                errno::set_errno(errno::ENOMEM);
                return -1;
            }
            // SAFETY: entry → dir.current (valid Dirent); dup has correct size.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    entry.cast::<u8>(),
                    dup,
                    core::mem::size_of::<Dirent>(),
                );
                // SAFETY: arr_typed is page-aligned; idx < count.
                *arr_typed.add(idx) = dup.cast::<Dirent>();
            }
            idx = idx.wrapping_add(1);
        }
    }

    closedir(dirp);

    // Sort if a comparator was provided.
    if let Some(cmp) = compar {
        // SAFETY: arr is page-aligned; idx entries have been written.
        unsafe {
            scandir_sort(arr, idx, cmp);
        }
    }

    // SAFETY: arr is page-aligned (align ≥ 8).
    unsafe {
        *namelist = arr_typed;
    }
    idx as i32
}

// ---------------------------------------------------------------------------
// versionsort — GNU extension
// ---------------------------------------------------------------------------

/// Compare two directory entries using version-number sorting.
///
/// Uses `strverscmp` on the `d_name` fields.  Like `alphasort`, this is
/// intended as a comparator for `scandir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn versionsort(a: *const *const Dirent, b: *const *const Dirent) -> i32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    // SAFETY: a and b point to valid Dirent pointers.
    let da = unsafe { &**a };
    let db = unsafe { &**b };
    // SAFETY: d_name arrays are valid null-terminated strings within
    // allocated Dirent structs.
    unsafe { crate::string::strverscmp(da.d_name.as_ptr(), db.d_name.as_ptr()) }
}

// ---------------------------------------------------------------------------
// readdir_r — thread-safe readdir (POSIX, deprecated in POSIX.1-2008)
// ---------------------------------------------------------------------------

/// Thread-safe version of `readdir`.
///
/// Reads the next directory entry into caller-supplied `entry`, and
/// stores a pointer to `entry` in `*result` on success, or sets
/// `*result` to NULL when the directory is exhausted.
///
/// Returns 0 on success, or an error number on failure.
///
/// Note: deprecated in POSIX.1-2008 (readdir is thread-safe if each
/// thread uses its own Dir*), but still needed for legacy code.
///
/// # Safety
///
/// `dirp`, `entry`, and `result` must all be valid, non-null pointers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readdir_r(dirp: *mut Dir, entry: *mut Dirent, result: *mut *mut Dirent) -> i32 {
    if dirp.is_null() || entry.is_null() || result.is_null() {
        return errno::EFAULT;
    }

    let ent = readdir(dirp);
    if ent.is_null() {
        // End of directory — not an error.
        // SAFETY: result verified non-null.
        unsafe {
            *result = core::ptr::null_mut();
        }
        return 0;
    }

    // Copy the entry into caller's buffer.
    // SAFETY: ent points to a valid Dirent (inside Dir.current),
    // and entry is caller-supplied valid storage.
    unsafe {
        core::ptr::copy_nonoverlapping(
            ent.cast::<u8>(),
            entry.cast::<u8>(),
            core::mem::size_of::<Dirent>(),
        );
        *result = entry;
    }

    0
}

// ---------------------------------------------------------------------------
// fdopendir — open directory stream from file descriptor
// ---------------------------------------------------------------------------

/// Open a directory stream from a file descriptor.
///
/// The listing is read through `fd` itself, via `SYS_FS_GETDENTS_PINNED`
/// (664), which resolves the *handle* rather than a name.  The `Dir` takes
/// ownership of `fd`: [`closedir`] closes it, and the caller must not.
///
/// This used to look up the path `fd` had when it was opened and list *that*,
/// which gave up everything the descriptor was for.  Renaming the directory
/// after the open made the stream list a path that no longer existed;
/// replacing a component with a symlink made it list wherever the symlink
/// pointed; and a descriptor this libc did not open — one inherited across an
/// exec, or produced by a raw `openat` — had no stored path at all and was
/// rejected as `ENOTDIR` despite being a perfectly good directory.
///
/// On failure the descriptor is left open and unchanged, as POSIX requires:
/// the caller still owns it and may close it or try something else.
/// [`opendir`], whose descriptor is its own, closes it itself.
///
/// # Errors
///
/// - `EBADF` — `fd` is not a valid open file descriptor.
/// - `ENOTDIR` — `fd` is not a file handle at all (a pipe, a socket), or is
///   not a directory.  The second half is the kernel's verdict, not ours.
/// - `EMFILE` — directory pool exhausted ([`MAX_OPEN_DIRS`] streams already
///   open).
/// - `ENOMEM` — no memory for the listing buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdopendir(fd: i32) -> *mut Dir {
    // The two questions are asked separately because their answers differ:
    // an fd that is not in the table at all is `EBADF`, while one that is
    // there but is not a file handle is a pipe or a socket — `ENOTDIR`.
    // `pinned_base` deliberately sets no errno and cannot tell them apart.
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return core::ptr::null_mut();
    }
    let Some(handle) = crate::file::pinned_base(fd) else {
        errno::set_errno(errno::ENOTDIR);
        return core::ptr::null_mut();
    };

    // Read the listing *before* claiming a pool slot, so a slow syscall never
    // holds one of the eight streams hostage and a failure needs no unwind.
    let Some((buf, len)) = slurp_listing(handle) else {
        return core::ptr::null_mut(); // errno set by `slurp_listing`.
    };

    let dir_ptr = alloc_dir();
    if dir_ptr.is_null() {
        // SAFETY: `buf` came from `slurp_listing`'s `malloc` and has not been
        // published anywhere.
        unsafe { crate::malloc::free(buf) };
        errno::set_errno(errno::EMFILE);
        return core::ptr::null_mut();
    }

    // SAFETY: alloc_dir returned a valid, exclusively-owned Dir pointer.
    let dir = unsafe { &mut *dir_ptr };
    dir.buf = buf;
    dir.len = len;
    dir.pos = 0;
    // Take ownership of the fd — closedir() will close it.
    dir.owned_fd = fd;

    dir_ptr
}

// ---------------------------------------------------------------------------
// Static Dir pool (no heap allocator)
// ---------------------------------------------------------------------------

/// Maximum concurrent open directory streams.
///
/// This was 8, and the reason was arithmetic: a `Dir` used to carry its
/// listing inline as 256 fixed 264-byte entries, so each slot cost ~68 KiB
/// and eight of them cost 544 KiB of `.bss`.  Eight is far too few for a
/// recursive walk — `fts` and `nftw` hold one stream per level — and the cap
/// is what forced [`crate::fts`] to buffer children eagerly instead of
/// keeping the parent's stream open.
///
/// The listing now lives on the heap, so a slot is a few hundred bytes and
/// the whole pool is well under 32 KiB.  64 is chosen to comfortably exceed
/// the deepest tree anything here walks while still being a fixed cost that
/// cannot be exhausted by a leak.
pub(crate) const MAX_OPEN_DIRS: usize = 64;

/// Static pool of Dir structs.
///
/// We can't heap-allocate in `no_std` without a global allocator, and a
/// `DIR *` must outlive the call that made it, so the stream *objects* come
/// from a fixed pool.  Their listings do not — those are `malloc`'d per
/// stream and sized to the directory (see [`slurp_listing`]).
static mut DIR_POOL: [DirSlot; MAX_OPEN_DIRS] = [const { DirSlot::EMPTY }; MAX_OPEN_DIRS];

/// Serialises the scans of [`DIR_POOL`] in [`alloc_dir`] and [`free_dir`].
///
/// A plain `static`, not a [`crate::perprocess::process_global!`] one,
/// because `DIR_POOL` is itself a plain `static mut`: the lock has to be
/// shared exactly as widely as the table it guards.  See
/// [`crate::perprocess::PoolLock`].
static DIR_POOL_LOCK: crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();

struct DirSlot {
    in_use: bool,
    dir: Dir,
}

impl DirSlot {
    const EMPTY: Self = Self {
        in_use: false,
        dir: Dir {
            buf: core::ptr::null_mut(),
            len: 0,
            pos: 0,
            current: Dirent {
                d_ino: 0,
                d_off: 0,
                d_reclen: 0,
                d_type: 0,
                d_name: [0u8; 256],
            },
            owned_fd: -1,
        },
    };
}

/// Allocate a Dir from the static pool.
///
/// Returns a raw pointer to an available Dir slot, or null if the pool
/// is exhausted.  Uses `addr_of_mut!` to avoid creating `&mut` references
/// to `static mut` (which is UB in Rust 2024).
///
/// The scan runs under [`DIR_POOL_LOCK`], so two threads calling `opendir`
/// at once cannot both be handed the same slot.  The lock covers only the
/// claim: once a caller holds its `DIR *`, `readdir` on it is unsynchronised,
/// which is where POSIX puts the obligation anyway.
fn alloc_dir() -> *mut Dir {
    // SAFETY: `DIR_POOL_LOCK` is a `static`, so it outlives the guard.
    let _guard = unsafe { crate::perprocess::lock_pool((&raw const DIR_POOL_LOCK).cast_mut()) };
    // SAFETY: the guard is held, and every scan of `DIR_POOL` takes it, so
    // this is the only live view of the table.
    unsafe {
        let pool = core::ptr::addr_of_mut!(DIR_POOL).cast::<DirSlot>();
        let mut i: usize = 0;
        while i < MAX_OPEN_DIRS {
            let slot = pool.add(i);
            if !(*slot).in_use {
                (*slot).in_use = true;
                // `closedir` released the buffer before returning the slot,
                // so this is a re-assertion rather than a leak being papered
                // over — but it is cheap and it is what keeps a slot that
                // some future error path abandons from handing its stale
                // pointer to the next caller.
                (*slot).dir.buf = core::ptr::null_mut();
                (*slot).dir.len = 0;
                (*slot).dir.pos = 0;
                (*slot).dir.owned_fd = -1;
                return core::ptr::addr_of_mut!((*slot).dir);
            }
            i = i.wrapping_add(1);
        }
    }
    core::ptr::null_mut()
}

/// Return a Dir to the static pool.
///
/// Uses raw pointer comparison to find the matching slot.
///
/// Takes [`DIR_POOL_LOCK`] for the same reason [`alloc_dir`] does — and not
/// only to order the two against each other: clearing `in_use` outside the
/// lock would be a plain data race with a concurrent `alloc_dir` reading it,
/// and could let the slot be reissued before this release was visible.
fn free_dir(dir: *mut Dir) {
    // SAFETY: `DIR_POOL_LOCK` is a `static`, so it outlives the guard.
    let _guard = unsafe { crate::perprocess::lock_pool((&raw const DIR_POOL_LOCK).cast_mut()) };
    // SAFETY: the guard is held, so this is the only live view of the table.
    unsafe {
        let pool = core::ptr::addr_of_mut!(DIR_POOL).cast::<DirSlot>();
        let mut i: usize = 0;
        while i < MAX_OPEN_DIRS {
            let slot = pool.add(i);
            let slot_dir = core::ptr::addr_of_mut!((*slot).dir);
            if core::ptr::eq(dir, slot_dir) {
                (*slot).in_use = false;
                return;
            }
            i = i.wrapping_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// LFS64 aliases — our off_t/ino_t are already 64-bit
// ---------------------------------------------------------------------------

/// `readdir64` — LFS64 alias for `readdir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readdir64(dirp: *mut Dir) -> *mut Dirent {
    readdir(dirp)
}

/// `readdir_r64` — LFS64 alias for `readdir_r`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn readdir_r64(
    dirp: *mut Dir,
    entry: *mut Dirent,
    result: *mut *mut Dirent,
) -> i32 {
    readdir_r(dirp, entry, result)
}

/// `scandir64` — LFS64 alias for `scandir`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn scandir64(
    dirname: *const u8,
    namelist: *mut *mut *mut Dirent,
    filter: Option<extern "C" fn(*const Dirent) -> i32>,
    compar: Option<extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32>,
) -> i32 {
    scandir(dirname, namelist, filter, compar)
}

// ---------------------------------------------------------------------------
// getdents / getdents64 — raw Linux directory entry syscalls
// ---------------------------------------------------------------------------

/// Linux kernel directory entry for `getdents64`.
///
/// Programs normally use `readdir()` instead.  This struct exists for
/// low-level compatibility with programs that use the raw syscall.
#[repr(C)]
pub struct LinuxDirent64 {
    /// Inode number.
    pub d_ino: u64,
    /// Offset to next entry.
    pub d_off: i64,
    /// Length of this `linux_dirent64`.
    pub d_reclen: u16,
    /// File type (DT_* constant).
    pub d_type: u8,
    /// Filename (null-terminated, variable length).
    pub d_name: [u8; 256],
}

// ---------------------------------------------------------------------------
// Per-fd getdents iterator cache
// ---------------------------------------------------------------------------
//
// `getdents64` is stateful: each call returns the next batch of entries
// for a given fd, with no caller-visible position object.  We therefore
// keep a small static pool of per-fd snapshot caches.  On the first call
// for an fd we snapshot the directory through the fd itself, via
// `SYS_FS_GETDENTS_PINNED`; subsequent calls walk the snapshot until
// exhausted, at which point the slot is freed and 0 returned.
//
// The snapshot used to be taken by *path* — `SYS_FS_LIST_DIR` on whatever
// path the fd had when it was opened — and via `syscall3`, which omitted the
// buffer-capacity argument the kernel reads from arg3.  With no capacity the
// kernel computes `max_entries = 0`, so **every `getdents64` returned an
// empty directory**.  That is the same defect as
// BUG-OPENDIR-MISSING-BUFCAP-ARG3 (known-issues.md, 2026-07-22), which was
// fixed in `opendir`/`fdopendir` and missed here because nothing in-tree
// calls the raw syscall wrapper.
//
// Everything below is done under GETDENTS_POOL_LOCK, including the walk that
// fills the caller's buffer.  That is stricter than it used to be, and it has
// to be: the snapshot is now heap-allocated, so a thread reaching end-of-
// directory frees the buffer, and a second thread mid-walk on the same fd
// would have been reading freed memory rather than merely stale bytes.  The
// only work the lock does not cover is the syscall and allocation that take
// the snapshot, which happen before any slot is claimed.
//
// LIMITATION: the cache is keyed by fd number, so a program that closes a
// directory fd mid-iteration and gets the same number back from a later
// `open` sees the old snapshot until it reports EOF.  Real programs do not
// close-and-reuse mid-iteration (see todo.txt).

/// Directories that can be under `getdents64` iteration at once.
///
/// Raised from 4 when the snapshot moved to the heap: a slot used to carry a
/// 68 KiB inline buffer, so four of them were 270 KiB of `.bss`, and it is
/// now a handful of words.
const MAX_GETDENTS_CACHES: usize = 16;

struct GetdentsCache {
    in_use: bool,
    fd: i32,
    /// Packed 664 records, `malloc`'d; null until the snapshot is installed.
    buf: *mut u8,
    /// Bytes of records in `buf`.
    len: usize,
    /// Byte offset of the next record to emit.
    pos: usize,
}

impl GetdentsCache {
    const EMPTY: Self = Self {
        in_use: false,
        fd: -1,
        buf: core::ptr::null_mut(),
        len: 0,
        pos: 0,
    };
}

static mut GETDENTS_POOL: [GetdentsCache; MAX_GETDENTS_CACHES] =
    [const { GetdentsCache::EMPTY }; MAX_GETDENTS_CACHES];

/// Serialises every scan of [`GETDENTS_POOL`].
///
/// Plain `static` to match `GETDENTS_POOL`'s own scope; see
/// [`crate::perprocess::PoolLock`].
static GETDENTS_POOL_LOCK: crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();

/// Install a freshly-taken snapshot for `fd`, taking ownership of `buf`.
///
/// Returns `false` — having freed `buf` — only when the pool is full.
///
/// A snapshot may already have appeared for `fd` while this one was being
/// taken, because the syscall runs with the lock released.  That is a race
/// between two threads iterating the same descriptor, which is undefined
/// anyway; what matters is that it cannot produce *two* slots for one fd, so
/// the loser here discards its own buffer and lets the winner's stand.
fn install_getdents_cache(fd: i32, buf: *mut u8, len: usize) -> bool {
    let mut discard = core::ptr::null_mut();
    let installed = {
        // SAFETY: `GETDENTS_POOL_LOCK` is a `static`, so it outlives the guard.
        let _guard =
            unsafe { crate::perprocess::lock_pool((&raw const GETDENTS_POOL_LOCK).cast_mut()) };
        // SAFETY: the guard is held, and every scan of the pool takes it, so
        // this is the only live view of the table.
        unsafe {
            let base = core::ptr::addr_of_mut!(GETDENTS_POOL).cast::<GetdentsCache>();
            let mut found = false;
            let mut free_slot: *mut GetdentsCache = core::ptr::null_mut();
            let mut i: usize = 0;
            while i < MAX_GETDENTS_CACHES {
                let slot = base.add(i);
                if (*slot).in_use && (*slot).fd == fd {
                    found = true;
                    break;
                }
                if !(*slot).in_use && free_slot.is_null() {
                    free_slot = slot;
                }
                i = i.wrapping_add(1);
            }
            if found {
                discard = buf;
                true
            } else if free_slot.is_null() {
                discard = buf;
                false
            } else {
                (*free_slot).in_use = true;
                (*free_slot).fd = fd;
                (*free_slot).buf = buf;
                (*free_slot).len = len;
                (*free_slot).pos = 0;
                true
            }
        }
    };
    if !discard.is_null() {
        // SAFETY: `discard` is `buf`, which the caller handed over and which
        // was not stored in any slot on the paths that set it.  Freed outside
        // the guard so an `munmap` never runs under the pool lock.
        unsafe { crate::malloc::free(discard) };
    }
    installed
}

/// Header size of a `linux_dirent64` record (everything before `d_name`).
const LINUX_DIRENT64_HEADER: usize = 19;

/// Emit one `linux_dirent64` record into `out`.
///
/// Returns `Some(reclen)` on success (number of bytes written, padded to
/// 8-byte alignment) or `None` if `out` is too small to hold the record.
// `slot` is taken as `out[..reclen]` after the `reclen > out.len()` check,
// so `reclen` bytes (>= LINUX_DIRENT64_HEADER = 19) are guaranteed in
// scope.  `name_len` is bounded by `reclen - LINUX_DIRENT64_HEADER`.
// `reclen + 7` cannot overflow because the prior `checked_add` chain
// returned None if it would.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn emit_linux_dirent64(
    out: &mut [u8],
    ino: u64,
    off: i64,
    dtype: u8,
    name: &[u8],
) -> Option<usize> {
    // Name must include space for the trailing NUL terminator and the
    // whole record must be rounded up to 8-byte alignment so the next
    // record's u64 fields stay aligned.
    let name_len = name.len();
    let unpadded = LINUX_DIRENT64_HEADER
        .checked_add(name_len)?
        .checked_add(1)?;
    let reclen = unpadded.checked_add(7)? & !7usize;
    if reclen > out.len() || reclen > u16::MAX as usize {
        return None;
    }
    let slot = out.get_mut(..reclen)?;
    slot[0..8].copy_from_slice(&ino.to_le_bytes());
    slot[8..16].copy_from_slice(&off.to_le_bytes());
    let reclen_u16 = reclen as u16;
    slot[16..18].copy_from_slice(&reclen_u16.to_le_bytes());
    slot[18] = dtype;
    if let Some(name_dst) = slot.get_mut(LINUX_DIRENT64_HEADER..LINUX_DIRENT64_HEADER + name_len) {
        name_dst.copy_from_slice(name);
    }
    // Zero the NUL terminator and any tail padding.
    if let Some(tail) = slot.get_mut(LINUX_DIRENT64_HEADER + name_len..reclen) {
        for b in tail {
            *b = 0;
        }
    }
    Some(reclen)
}

/// Translate packed 664 records into `linux_dirent64`s, filling `out`.
///
/// `pos` is the byte offset into `records` to resume from; the return is
/// `(bytes written, byte offset to resume from next time)`.  Emission stops
/// at the first record that does not fit in what is left of `out`, which is
/// exactly how `getdents64` batches: that record is *not* consumed, so the
/// caller's next call re-offers it against a fresh buffer.
///
/// The only entries skipped are ones that cannot be represented: an empty
/// name, or one 256 bytes or longer, which could not be null-terminated
/// inside [`LinuxDirent64::d_name`] and whose truncation would denote a
/// different file.  Nothing is skipped for its *type* — the same rule
/// [`readdir`] follows, for the reason in [`kernel_type_to_dt`].
///
/// A pure function over two byte slices, so the batching boundary — the
/// case where `out` fills mid-listing — is testable on the host, where the
/// syscall that produces these records answers `ENOSYS`.
fn fill_dirent64_batch(records: &[u8], pos: usize, out: &mut [u8]) -> (usize, usize) {
    let mut cursor = pos;
    let mut written: usize = 0;
    loop {
        let Some(rest) = records.get(cursor..) else {
            // `pos` was past the end, or a record length ran off it.  Report
            // the listing as exhausted rather than re-reading a partial one.
            cursor = records.len();
            break;
        };
        if rest.is_empty() {
            break;
        }
        let Some(entry) = decode_packed_entry(rest) else {
            cursor = records.len();
            break;
        };
        let next = cursor.wrapping_add(entry.record_len);
        let name_len = entry.name.len();
        if name_len == 0 || name_len >= 256 {
            cursor = next;
            continue;
        }
        let Some(remaining) = out.get_mut(written..) else {
            break;
        };
        // `d_off` is the cookie for the *next* entry, matching what
        // `telldir` hands back on the stream side.
        let Ok(off) = i64::try_from(next) else {
            break;
        };
        let Some(reclen) = emit_linux_dirent64(
            remaining,
            entry.ino,
            off,
            kernel_type_to_dt(entry.kernel_type),
            entry.name,
        ) else {
            // Out of room in the caller's buffer; leave `cursor` on this
            // record so the next call retries it.
            break;
        };
        written = written.wrapping_add(reclen);
        cursor = next;
    }
    (written, cursor)
}

/// Emit the next batch of entries for `fd` out of its cached snapshot.
///
/// Returns `None` when no snapshot exists for `fd`, which is the caller's
/// signal to take one.  Otherwise the value is what `getdents64` should
/// return: bytes written, `0` at end-of-directory (the slot is released and
/// its buffer freed), or `-1` with errno set when the caller's buffer cannot
/// hold even the next record.
///
/// # Safety
///
/// `dirp` must be writable for `count` bytes.
unsafe fn drain_getdents_cache(fd: i32, dirp: *mut u8, count: usize) -> Option<i64> {
    let mut release: *mut u8 = core::ptr::null_mut();
    let result = {
        // SAFETY: `GETDENTS_POOL_LOCK` is a `static`, so it outlives the guard.
        let _guard =
            unsafe { crate::perprocess::lock_pool((&raw const GETDENTS_POOL_LOCK).cast_mut()) };
        // SAFETY: the guard is held, and every scan of the pool takes it, so
        // this is the only live view of the table — including of the snapshot
        // buffers it owns, which is why the walk itself is in here.
        unsafe {
            let base = core::ptr::addr_of_mut!(GETDENTS_POOL).cast::<GetdentsCache>();
            let mut slot: *mut GetdentsCache = core::ptr::null_mut();
            let mut i: usize = 0;
            while i < MAX_GETDENTS_CACHES {
                let cand = base.add(i);
                if (*cand).in_use && (*cand).fd == fd {
                    slot = cand;
                    break;
                }
                i = i.wrapping_add(1);
            }
            if slot.is_null() {
                None
            } else {
                let records: &[u8] = if (*slot).buf.is_null() {
                    &[]
                } else {
                    core::slice::from_raw_parts((*slot).buf, (*slot).len)
                };
                // SAFETY (inner): the caller guarantees `dirp` is writable for
                // `count` bytes, and `dirp` was null-checked before we ran.
                let out = core::slice::from_raw_parts_mut(dirp, count);
                let (written, next) = fill_dirent64_batch(records, (*slot).pos, out);
                (*slot).pos = next;
                if written > 0 {
                    // `written <= count`, so this only fails for a buffer
                    // larger than `i64::MAX`, which no address space holds.
                    match i64::try_from(written) {
                        Ok(n) => Some(n),
                        Err(_) => Some(i64::MAX),
                    }
                } else if next >= (*slot).len {
                    // End of directory: hand the slot back.
                    release = (*slot).buf;
                    (*slot).in_use = false;
                    (*slot).fd = -1;
                    (*slot).buf = core::ptr::null_mut();
                    (*slot).len = 0;
                    (*slot).pos = 0;
                    Some(0)
                } else {
                    // Entries remain but none fit; POSIX and Linux both say
                    // EINVAL for a buffer too small for the next record.
                    crate::errno::set_errno(crate::errno::EINVAL);
                    Some(-1)
                }
            }
        }
    };
    if !release.is_null() {
        // SAFETY: the slot owned `release` and gave it up under the lock, so
        // no other thread can reach it.  Freed outside the guard so the
        // `munmap` inside `free` never runs with the pool held.
        unsafe { crate::malloc::free(release) };
    }
    result
}

/// Read directory entries via the raw Linux `getdents64` syscall.
///
/// Programs normally use `readdir()`; this exists for low-level
/// compatibility with code that calls the raw syscall (e.g. `ls -f`
/// implementations and language runtimes that bypass libc's dir
/// streams).
///
/// On the first call for a given fd we snapshot the directory *through that
/// fd*, via `SYS_FS_GETDENTS_PINNED`; subsequent calls drain the snapshot.
/// Returns the number of bytes written into `dirp`, 0 at end-of-directory,
/// or -1 with errno set on error.
///
/// `d_ino` is the filesystem's own inode number, passed through from the
/// kernel record — including `0`, which means the filesystem has no stable
/// per-object identity, not that the entry is deleted (design-decisions.md
/// §740).  It used to be the entry's index in the listing, which is never a
/// real inode and quietly broke every caller that compared it to `st_ino`.
///
/// # Errors
///
/// - `EBADF`  — `fd` is negative or not a valid open fd.
/// - `EFAULT` — `dirp` is null and `count` is non-zero.
/// - `EINVAL` — `count` is zero or too small to hold any single entry.
/// - `ENOTDIR` — `fd` does not refer to a directory.
/// - `ENFILE` — the per-fd snapshot cache pool is exhausted.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getdents64(fd: i32, dirp: *mut u8, count: usize) -> i64 {
    if fd < 0 {
        crate::errno::set_errno(crate::errno::EBADF);
        return -1;
    }
    if count == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    if dirp.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }

    // An iteration already in progress is served from its snapshot.
    // SAFETY: `dirp` is non-null and, per this function's contract, writable
    // for `count` bytes.
    if let Some(ret) = unsafe { drain_getdents_cache(fd, dirp, count) } {
        return ret;
    }

    // First call for this fd: take the snapshot through the descriptor.
    if crate::fdtable::get_fd(fd).is_none() {
        crate::errno::set_errno(crate::errno::EBADF);
        return -1;
    }
    let Some(handle) = crate::file::pinned_base(fd) else {
        // No kernel handle behind this fd, so it cannot be a directory the
        // kernel can list — a pipe, a socket, or an emulated fd.
        crate::errno::set_errno(crate::errno::ENOTDIR);
        return -1;
    };
    let Some((buf, len)) = slurp_listing(handle) else {
        // errno already set by `slurp_listing`.
        return -1;
    };
    if !install_getdents_cache(fd, buf, len) {
        // `install_getdents_cache` freed the buffer.
        crate::errno::set_errno(crate::errno::ENFILE);
        return -1;
    }

    // A `None` here means the slot we just installed is already gone, which
    // can only be another thread draining the same fd to end-of-directory in
    // between — where this call would have ended up too, so report EOF.
    // SAFETY: as above.
    unsafe { drain_getdents_cache(fd, dirp, count) }.unwrap_or(0)
}

/// Read directory entries via the legacy Linux `getdents` syscall.
///
/// The legacy `struct linux_dirent` has a 32-bit inode field which
/// cannot represent our 64-bit inodes safely, so we never actually
/// produce records here — the function returns `ENOSYS` on valid
/// calls and callers should switch to `getdents64` or libc's
/// `readdir()`.  glibc and musl do not export a wrapper for either
/// raw syscall, so portable code already uses one of those.
///
/// However, an unimplemented sentinel is not a license to skip
/// argument-domain validation.  A buggy caller — for example a
/// language runtime that bypasses libc — passing a closed fd or a
/// NULL buffer must see the same errno values Linux would produce,
/// so the failure is diagnosed correctly even though the underlying
/// directory walk is not performed.  Validation order matches
/// `getdents64` above and Linux's `fs/readdir.c::sys_getdents`:
///
/// 1. `fd < 0`                   -> `EBADF`
/// 2. `count == 0`               -> `EINVAL`
/// 3. `dirp.is_null()`           -> `EFAULT`
/// 4. `fd` not in fdtable        -> `EBADF`
/// 5. all valid                  -> `ENOSYS`
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getdents(fd: i32, dirp: *mut u8, count: usize) -> i64 {
    if fd < 0 {
        crate::errno::set_errno(crate::errno::EBADF);
        return -1;
    }
    if count == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    if dirp.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        crate::errno::set_errno(crate::errno::EBADF);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// scandirat — scan directory relative to a directory fd
// ---------------------------------------------------------------------------

/// `scandirat` — scan a directory relative to a directory fd.
///
/// Like `scandir`, but the directory is specified relative to `dirfd`.
/// If `dirfd` is `AT_FDCWD` or `dirname` is absolute, this behaves
/// identically to `scandir`.
///
/// Returns the number of matching entries on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scandirat(
    dirfd: i32,
    dirname: *const u8,
    namelist: *mut *mut *mut Dirent,
    filter: Option<extern "C" fn(*const Dirent) -> i32>,
    compar: Option<extern "C" fn(*const *const Dirent, *const *const Dirent) -> i32>,
) -> i32 {
    if dirname.is_null() || namelist.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // An empty relative name is ENOENT, as everywhere else in the `*at`
    // family.  Without this the textual join below silently turns `""` into
    // `dirfd` itself, so `scandirat(fd, "", …)` would list the directory the
    // caller happened to hold rather than reporting that no name was given.
    if crate::file::is_empty_path(dirname) {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    // Resolve relative to dirfd if needed.
    if dirfd == crate::file::AT_FDCWD || crate::file::is_absolute_path(dirname) {
        return scandir(dirname, namelist, filter, compar);
    }

    // Build full path from dirfd + relative dirname.
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = crate::file::resolve_dirfd_path(dirfd, dirname, &mut full);
    if len == 0 {
        return -1; // errno set by resolve_dirfd_path
    }
    scandir(full.as_ptr(), namelist, filter, compar)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- DT_* type constants --

    #[test]
    fn test_dt_constants() {
        // d_type uses the Linux <dirent.h> ABI values (re-exported from
        // linux_dirent_types) so ported programs interpret them correctly.
        assert_eq!(DT_UNKNOWN, 0);
        assert_eq!(DT_FIFO, 1);
        assert_eq!(DT_CHR, 2);
        assert_eq!(DT_DIR, 4);
        assert_eq!(DT_BLK, 6);
        assert_eq!(DT_REG, 8);
        assert_eq!(DT_LNK, 10);
        assert_eq!(DT_SOCK, 12);
    }

    #[test]
    fn test_kernel_type_to_dt() {
        // The kernel's compact type code must translate to the matching
        // POSIX DT_* value, NOT pass through unchanged.
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_FILE), DT_REG);
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_DIR), DT_DIR);
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_SYMLINK), DT_LNK);
        // Volume labels and any unexpected code → unknown.
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_VOLLABEL), DT_UNKNOWN);
        assert_eq!(kernel_type_to_dt(99), DT_UNKNOWN);
    }

    #[test]
    fn device_nodes_get_a_device_dtype_not_unknown() {
        // Regression: these two arms were missing, so `readdir("/dev")`
        // reported DT_UNKNOWN for every device node that devfs produces.
        // DT_UNKNOWN is not a neutral answer — it tells the caller to stat
        // the entry, which costs a syscall per entry in the one directory
        // where the type is most often the thing being looked for.
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_CHARDEV), DT_CHR);
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_BLOCKDEV), DT_BLK);
    }

    #[test]
    fn the_kernel_type_codes_match_the_serializers() {
        // These six constants are the wire values three kernel handlers
        // emit (`SYS_FS_LIST_DIR` 603, `SYS_FS_READDIR_AT` 647,
        // `SYS_FS_GETDENTS_PINNED` 664) and the one `SYS_FS_STAT` writes at
        // byte 8 of its record.  Pinned literally, not derived, because the
        // kernel's *documentation* of this table used to have 2 and 3
        // transposed (see the note on KERNEL_TYPE_FILE); it has since been
        // corrected, and nothing here moved when it was, which is the
        // property this test exists to keep true.
        assert_eq!(KERNEL_TYPE_FILE, 0);
        assert_eq!(KERNEL_TYPE_DIR, 1);
        assert_eq!(KERNEL_TYPE_VOLLABEL, 2);
        assert_eq!(KERNEL_TYPE_SYMLINK, 3);
        assert_eq!(KERNEL_TYPE_CHARDEV, 4);
        assert_eq!(KERNEL_TYPE_BLOCKDEV, 5);
    }

    #[test]
    fn every_kernel_type_except_the_volume_label_has_a_real_dtype() {
        // The property that matters is not any single mapping but that
        // DT_UNKNOWN is reached only where the kernel has nothing to say.
        // A volume label is the sole defined code with no POSIX analogue.
        for code in [
            KERNEL_TYPE_FILE,
            KERNEL_TYPE_DIR,
            KERNEL_TYPE_SYMLINK,
            KERNEL_TYPE_CHARDEV,
            KERNEL_TYPE_BLOCKDEV,
        ] {
            assert_ne!(
                kernel_type_to_dt(code),
                DT_UNKNOWN,
                "kernel type {code} translated to DT_UNKNOWN"
            );
        }
        assert_eq!(kernel_type_to_dt(KERNEL_TYPE_VOLLABEL), DT_UNKNOWN);
    }

    #[test]
    fn test_dt_types_distinct() {
        let types = [
            DT_UNKNOWN, DT_REG, DT_DIR, DT_LNK, DT_CHR, DT_BLK, DT_FIFO, DT_SOCK,
        ];
        for (i, &a) in types.iter().enumerate() {
            for &b in &types[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    // -- Dirent struct layout --

    #[test]
    fn test_dirent_d_name_size() {
        // d_name must be at least 256 bytes for POSIX compliance.
        let d = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        assert_eq!(d.d_name.len(), 256);
    }

    #[test]
    fn test_dirent_fields() {
        let d = Dirent {
            d_ino: 42,
            d_off: 100,
            d_reclen: 280,
            d_type: DT_REG,
            d_name: [0u8; 256],
        };
        assert_eq!(d.d_ino, 42);
        assert_eq!(d.d_off, 100);
        assert_eq!(d.d_reclen, 280);
        assert_eq!(d.d_type, DT_REG);
    }

    // -- The packed record 647 and 664 share --

    /// Write one packed record into `out`, returning the bytes it used.
    ///
    /// Deliberately literal — offsets spelled out rather than derived from
    /// [`PACKED_FIXED_LEN`] — so that a change to the decoder cannot silently
    /// change the encoder these tests check it against.
    #[allow(clippy::indexing_slicing)]
    fn packed_record(out: &mut [u8], kernel_type: u8, name: &[u8], size: u64, ino: u64) -> usize {
        let n = name.len();
        out[0] = kernel_type;
        out[1..5].copy_from_slice(&(n as u32).to_le_bytes());
        out[5..5 + n].copy_from_slice(name);
        out[5 + n..13 + n].copy_from_slice(&size.to_le_bytes());
        out[13 + n..21 + n].copy_from_slice(&ino.to_le_bytes());
        21 + n
    }

    #[test]
    fn a_packed_record_is_twenty_one_bytes_plus_the_name() {
        // The kernel computes this length in one place (`entry_record_len`)
        // for both 647 and 664 so the two cannot drift; this is our copy of
        // the same arithmetic, and it has to agree.
        assert_eq!(PACKED_FIXED_LEN, 21);
        let mut buf = [0u8; 64];
        let used = packed_record(&mut buf, KERNEL_TYPE_FILE, b"hello", 0, 0);
        assert_eq!(used, 21 + 5);
    }

    #[test]
    fn decode_packed_entry_reads_type_name_and_inode() {
        let mut buf = [0u8; 64];
        let used = packed_record(
            &mut buf,
            KERNEL_TYPE_DIR,
            b"sub",
            4096,
            0x0123_4567_89ab_cdef,
        );
        let entry = decode_packed_entry(&buf).expect("a whole record decodes");
        assert_eq!(entry.kernel_type, KERNEL_TYPE_DIR);
        assert_eq!(entry.name, b"sub");
        // The inode is the last eight bytes, *after* the size — reading them
        // in the other order is the mistake this asserts against.
        assert_eq!(entry.ino, 0x0123_4567_89ab_cdef);
        assert_eq!(entry.record_len, used);
    }

    #[test]
    fn decode_packed_entry_refuses_a_record_that_runs_off_the_buffer() {
        // A truncated listing must stop the walk, not read past the end of
        // the allocation.  664 truncates only at record boundaries, so this
        // can only come from a corrupt or hostile buffer — which is exactly
        // when returning None matters.
        let mut buf = [0u8; 64];
        let used = packed_record(&mut buf, KERNEL_TYPE_FILE, b"name", 0, 7);
        assert!(decode_packed_entry(&buf[..used]).is_some());
        for short in 0..used {
            assert!(
                decode_packed_entry(&buf[..short]).is_none(),
                "a {short}-byte prefix of a {used}-byte record decoded"
            );
        }
    }

    #[test]
    fn decode_packed_entry_tolerates_an_empty_name() {
        // Not something the kernel emits, but the decoder must not confuse a
        // zero-length name with a malformed record: the callers skip it, and
        // they can only skip what they can measure.
        let mut buf = [0u8; 32];
        let used = packed_record(&mut buf, KERNEL_TYPE_FILE, b"", 0, 0);
        let entry = decode_packed_entry(&buf).expect("decodes");
        assert_eq!(entry.name.len(), 0);
        assert_eq!(entry.record_len, used);
    }

    // -- getdents64 batching, as a pure function over bytes --

    #[test]
    fn fill_dirent64_batch_emits_the_kernels_inode_not_a_position() {
        let mut records = [0u8; 128];
        let n = packed_record(&mut records, KERNEL_TYPE_FILE, b"a", 1, 4242);
        let mut out = [0u8; 128];
        let (written, next) = fill_dirent64_batch(&records[..n], 0, &mut out);
        assert!(written > 0);
        assert_eq!(next, n);
        let ino = u64::from_le_bytes(out[0..8].try_into().expect("8 bytes"));
        assert_eq!(ino, 4242, "d_ino must be the filesystem's, not the index");
        assert_eq!(out[18], DT_REG);
        assert_eq!(&out[19..20], b"a");
        assert_eq!(out[20], 0, "the name must be NUL-terminated");
    }

    #[test]
    fn fill_dirent64_batch_passes_a_zero_inode_through() {
        // 0 means "this filesystem has no stable per-object identity"
        // (procfs, devfs, an unallocated FAT file), not "deleted".  Inventing
        // a replacement would make `find -inum` and `tar`'s hard-link
        // detection confidently wrong instead of visibly unavailable.
        // See design-decisions.md §740.
        let mut records = [0u8; 128];
        let n = packed_record(&mut records, KERNEL_TYPE_FILE, b"proc-ish", 0, 0);
        let mut out = [0u8; 128];
        let (written, _) = fill_dirent64_batch(&records[..n], 0, &mut out);
        assert!(written > 0);
        assert_eq!(
            u64::from_le_bytes(out[0..8].try_into().expect("8 bytes")),
            0
        );
    }

    #[test]
    fn fill_dirent64_batch_stops_without_consuming_the_record_that_did_not_fit() {
        // This is the whole contract of a batched getdents64: a record that
        // does not fit is re-offered next call.  Consuming it would drop an
        // entry from the directory for any caller whose buffer happens to
        // end on a record boundary.
        let mut records = [0u8; 256];
        let mut n = packed_record(&mut records, KERNEL_TYPE_FILE, b"first", 0, 11);
        n += packed_record(&mut records[n..], KERNEL_TYPE_FILE, b"second", 0, 22);
        // Room for exactly one linux_dirent64 with a five-byte name.
        let one = 19 + 5 + 1usize;
        let one_padded = (one + 7) & !7usize;
        let mut out = [0u8; 256];
        let (written, next) = fill_dirent64_batch(&records[..n], 0, &mut out[..one_padded]);
        assert_eq!(written, one_padded);
        assert_eq!(
            u64::from_le_bytes(out[0..8].try_into().expect("8 bytes")),
            11
        );

        // Resuming from `next` yields the second entry and nothing else.
        let mut out2 = [0u8; 256];
        let (written2, next2) = fill_dirent64_batch(&records[..n], next, &mut out2);
        assert!(written2 > 0);
        assert_eq!(next2, n);
        assert_eq!(
            u64::from_le_bytes(out2[0..8].try_into().expect("8 bytes")),
            22
        );

        // And a third call at end-of-listing writes nothing.
        let (written3, next3) = fill_dirent64_batch(&records[..n], next2, &mut out2);
        assert_eq!(written3, 0);
        assert_eq!(next3, n);
    }

    #[test]
    fn fill_dirent64_batch_skips_only_names_it_cannot_represent() {
        // An empty name is dropped because there is nothing to hand a caller.
        // Everything else is passed on whatever its type — see the label test
        // below for why that is a policy and not an omission.
        let mut records = [0u8; 256];
        let mut n = packed_record(&mut records, KERNEL_TYPE_FILE, b"", 0, 2);
        n += packed_record(&mut records[n..], KERNEL_TYPE_FILE, b"real", 0, 3);
        let mut out = [0u8; 256];
        let (written, next) = fill_dirent64_batch(&records[..n], 0, &mut out);
        assert!(written > 0);
        assert_eq!(next, n);
        assert_eq!(
            u64::from_le_bytes(out[0..8].try_into().expect("8 bytes")),
            3
        );
        let reclen = u16::from_le_bytes(out[16..18].try_into().expect("2 bytes")) as usize;
        assert_eq!(
            written, reclen,
            "only the one nameable entry should have been emitted"
        );
    }

    #[test]
    fn a_volume_label_is_reported_as_unknown_rather_than_hidden() {
        // The kernel drops labels on every listing route and reserves type 2,
        // so this record cannot arrive in practice.  What is pinned here is
        // the *policy*: libc does not add a second filter, because a filter
        // here would turn a regression in the kernel's into an invisible one
        // — lane A's `fat::mkfs_self_test` would fail loudly while every real
        // program saw a listing we had quietly repaired.  A label must surface
        // as an entry nothing can open, which is a bug report, rather than as
        // a gap, which is a mystery.  See `kernel_type_to_dt`.
        let mut records = [0u8; 128];
        let n = packed_record(&mut records, KERNEL_TYPE_VOLLABEL, b"MYDISK", 0, 1);
        let mut out = [0u8; 128];
        let (written, next) = fill_dirent64_batch(&records[..n], 0, &mut out);
        assert!(written > 0, "the label must not be filtered out");
        assert_eq!(next, n);
        assert_eq!(out[18], DT_UNKNOWN);
        assert_eq!(&out[19..25], b"MYDISK");
    }

    #[test]
    fn fill_dirent64_batch_treats_a_corrupt_record_as_end_of_listing() {
        // Better to end the directory early than to walk off a buffer.  The
        // cursor is left at the end so the next call reports EOF rather than
        // re-deriving the same bad offset forever.
        let mut records = [0u8; 128];
        let n = packed_record(&mut records, KERNEL_TYPE_FILE, b"ok", 0, 5);
        let truncated = n - 1;
        let mut out = [0u8; 128];
        let (written, next) = fill_dirent64_batch(&records[..truncated], 0, &mut out);
        assert_eq!(written, 0);
        assert_eq!(next, truncated);
    }

    // -- alphasort (pure function — can test without kernel) --

    #[test]
    fn test_alphasort_equal() {
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'f';
        a.d_name[1] = b'o';
        a.d_name[2] = b'o';
        b.d_name[0] = b'f';
        b.d_name[1] = b'o';
        b.d_name[2] = b'o';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert_eq!(alphasort(&pa, &pb), 0);
    }

    #[test]
    fn test_alphasort_less() {
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'a';
        a.d_name[1] = b'b';
        a.d_name[2] = b'c';
        b.d_name[0] = b'x';
        b.d_name[1] = b'y';
        b.d_name[2] = b'z';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert!(alphasort(&pa, &pb) < 0);
    }

    #[test]
    fn test_alphasort_greater() {
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'z';
        b.d_name[0] = b'a';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert!(alphasort(&pa, &pb) > 0);
    }

    #[test]
    fn test_alphasort_null_outer() {
        // Null outer pointer (the *const *const Dirent itself).
        let d = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let pd: *const Dirent = &d;
        assert_eq!(alphasort(core::ptr::null(), &pd), 0);
        assert_eq!(alphasort(&pd, core::ptr::null()), 0);
        assert_eq!(alphasort(core::ptr::null(), core::ptr::null()), 0);
    }

    #[test]
    fn test_alphasort_prefix() {
        // "ab" < "abc"
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'a';
        a.d_name[1] = b'b';
        b.d_name[0] = b'a';
        b.d_name[1] = b'b';
        b.d_name[2] = b'c';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert!(alphasort(&pa, &pb) < 0); // "ab\0" < "abc\0"
    }

    // -- Dirent struct size and offsets --

    #[test]
    fn test_dirent_struct_size() {
        // d_ino(8) + d_off(8) + d_reclen(2) + d_type(1) + padding(5) + d_name(256)
        // = 280 bytes on x86_64.
        let size = core::mem::size_of::<Dirent>();
        assert!(size >= 275, "Dirent too small: {size}"); // at least the fields
        // d_ino must be at offset 0.
        assert_eq!(core::mem::offset_of!(Dirent, d_ino), 0);
        // d_name must be inside the struct and fit 256 bytes.
        let name_offset = core::mem::offset_of!(Dirent, d_name);
        assert!(name_offset + 256 <= size, "d_name doesn't fit in Dirent");
    }

    // -- versionsort (pure function — can test without kernel) --

    #[test]
    fn test_versionsort_numeric_ordering() {
        // "file2" < "file10" under version sorting.
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let name_a = b"file2\0";
        let name_b = b"file10\0";
        a.d_name[..name_a.len()].copy_from_slice(name_a);
        b.d_name[..name_b.len()].copy_from_slice(name_b);
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        // Under version sort, file2 < file10.
        assert!(versionsort(&pa, &pb) < 0, "file2 should sort before file10");
    }

    #[test]
    fn test_versionsort_equal() {
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        a.d_name[0] = b'x';
        a.d_name[1] = b'1';
        b.d_name[0] = b'x';
        b.d_name[1] = b'1';
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;
        assert_eq!(versionsort(&pa, &pb), 0);
    }

    #[test]
    fn test_versionsort_null_outer() {
        let d = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let pd: *const Dirent = &d;
        assert_eq!(versionsort(core::ptr::null(), &pd), 0);
        assert_eq!(versionsort(&pd, core::ptr::null()), 0);
    }

    #[test]
    fn test_versionsort_vs_alphasort() {
        // alphasort("file10", "file2") > 0 (lexicographic: '1' < '2')
        // versionsort("file10", "file2") > 0 (numeric: 10 > 2)
        // Both agree on ordering direction here, but the reason differs.
        let mut a = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut b = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let name_a = b"file10\0";
        let name_b = b"file2\0";
        a.d_name[..name_a.len()].copy_from_slice(name_a);
        b.d_name[..name_b.len()].copy_from_slice(name_b);
        let pa: *const Dirent = &a;
        let pb: *const Dirent = &b;

        // versionsort: file10 > file2 (numeric 10 > 2)
        assert!(versionsort(&pa, &pb) > 0, "versionsort: file10 > file2");

        // alphasort: "file10" < "file2" (lexicographic: '1' < '2')
        assert!(
            alphasort(&pa, &pb) < 0,
            "alphasort: file10 < file2 (lexicographic)"
        );
    }

    // -- Dir pool constants --

    #[test]
    fn test_max_open_dirs() {
        // Raised from 8 when the listing moved to the heap.  A slot used to
        // carry a 68 KiB inline buffer, which is what made 8 the ceiling; it
        // is now a few words plus whatever the directory actually needs.
        // Eight was low enough that `fts` had to read every child directory
        // eagerly to avoid running out mid-walk.
        assert_eq!(MAX_OPEN_DIRS, 64);
    }

    // -- Null pointer handling --

    #[test]
    fn test_readdir_null() {
        let ret = readdir(core::ptr::null_mut());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_closedir_null_einval_not_ebadf() {
        // glibc's Linux closedir rejects a NULL stream with EINVAL before
        // it ever looks at a descriptor, so EBADF is unreachable here.
        // (sysdeps/unix/sysv/linux/closedir.c, glibc 2.39.)
        errno::set_errno(0);
        let ret = closedir(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_rewinddir_null_is_noop() {
        // Should not crash.
        rewinddir(core::ptr::null_mut());
    }

    #[test]
    fn test_telldir_null() {
        let ret = telldir(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_seekdir_null_is_noop() {
        // Should not crash.
        seekdir(core::ptr::null_mut(), 5);
    }

    #[test]
    fn test_seekdir_negative_loc_is_noop() {
        // Negative loc should be silently ignored.
        // We can't test this without a real Dir, but we can test null+negative.
        seekdir(core::ptr::null_mut(), -1);
    }

    #[test]
    fn test_dirfd_null() {
        let ret = dirfd(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_opendir_null() {
        let ret = opendir(core::ptr::null());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- readdir_r null pointer handling --

    #[test]
    fn test_readdir_r_null_dirp() {
        let mut entry = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = readdir_r(core::ptr::null_mut(), &raw mut entry, &raw mut result);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_readdir_r_null_entry() {
        // Use a fake non-null dirp to test the entry null check.
        let fake_dirp = 0x1000 as *mut Dir;
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = readdir_r(fake_dirp, core::ptr::null_mut(), &raw mut result);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_readdir_r_null_result() {
        let fake_dirp = 0x1000 as *mut Dir;
        let mut entry = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; 256],
        };
        let ret = readdir_r(fake_dirp, &raw mut entry, core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
    }

    // -- scandir null pointer handling --

    #[test]
    fn test_scandir_null_dirname() {
        let mut list: *mut *mut Dirent = core::ptr::null_mut();
        let ret = scandir(core::ptr::null(), &raw mut list, None, None);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_scandir_null_namelist() {
        let ret = scandir(b"/tmp\0".as_ptr(), core::ptr::null_mut(), None, None);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- fdopendir error handling --

    #[test]
    fn test_fdopendir_invalid_fd() {
        // fd 999 is not open.
        let ret = fdopendir(999);
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fdopendir_negative_fd() {
        let ret = fdopendir(-1);
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- DirSlot layout --

    #[test]
    fn test_dir_slot_empty_state() {
        let slot = DirSlot::EMPTY;
        assert!(!slot.in_use);
        assert!(slot.dir.buf.is_null());
        assert_eq!(slot.dir.len, 0);
        assert_eq!(slot.dir.pos, 0);
        assert_eq!(slot.dir.owned_fd, -1);
    }

    // -- Listing buffer sizing --

    #[test]
    fn the_first_slurp_round_asks_for_a_whole_page_of_listing() {
        // There is no fixed capacity any more: 664 reports the size the
        // complete listing needs, so a first round that comes up short is
        // retried at that size rather than truncated.  What is fixed is only
        // the opening guess, which should cover an ordinary directory in one
        // syscall — 8 KiB is roughly 250 entries at typical name lengths.
        assert_eq!(DIR_BUF_INITIAL, 8 * 1024);
        assert!(
            DIR_BUF_INITIAL > PACKED_FIXED_LEN + 255,
            "the first round must fit at least one maximum-length record"
        );
    }

    // -- LP64 aliases --

    #[test]
    fn test_readdir64_null_returns_null() {
        let result = readdir64(core::ptr::null_mut());
        assert!(result.is_null());
    }

    #[test]
    fn test_readdir_r64_null_dirp() {
        let mut entry = Dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0; 256],
        };
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = unsafe { readdir_r64(core::ptr::null_mut(), &mut entry, &mut result) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_readdir_r64_null_entry() {
        let mut result: *mut Dirent = core::ptr::null_mut();
        let ret = unsafe { readdir_r64(core::ptr::null_mut(), core::ptr::null_mut(), &mut result) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_scandir64_null_dirname() {
        let mut namelist: *mut *mut Dirent = core::ptr::null_mut();
        let ret = scandir64(core::ptr::null(), &mut namelist, None, None);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_scandir64_null_namelist() {
        let ret = scandir64(b"/tmp\0".as_ptr(), core::ptr::null_mut(), None, None);
        assert_eq!(ret, -1);
    }

    // -- getdents / getdents64 stubs --

    #[test]
    fn test_getdents64_null_buf_returns_efault() {
        // getdents64 is now implemented; a null buffer with non-zero
        // count must report EFAULT before touching the cache.
        crate::errno::set_errno(0);
        assert_eq!(getdents64(3, core::ptr::null_mut(), 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_getdents_still_enosys() {
        // Phase 67: getdents (legacy 32-bit-ino variant) is still
        // unimplemented, but now validates arguments first.  NULL dirp
        // with non-zero count now produces EFAULT (matching Linux),
        // not ENOSYS — this test was previously checking the pre-
        // validator behaviour.  Updated to call with valid args that
        // reach the ENOSYS sentinel (negative fd would short-circuit
        // with EBADF; use AT_FDCWD-style sentinel value of -100 is
        // also negative, so we have to use a real open fd).  Since
        // we cannot easily open a fd in this test, we assert the new
        // EFAULT semantics directly to keep regression coverage.
        crate::errno::set_errno(0);
        assert_eq!(getdents(3, core::ptr::null_mut(), 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_linux_dirent64_size() {
        let size = core::mem::size_of::<LinuxDirent64>();
        // d_ino(8) + d_off(8) + d_reclen(2) + d_type(1) + d_name(256) + padding
        assert!(
            size >= 275,
            "LinuxDirent64 should be at least 275 bytes, got {size}"
        );
    }

    #[test]
    fn test_linux_dirent64_alignment() {
        assert!(core::mem::align_of::<LinuxDirent64>() >= 8);
    }

    // -----------------------------------------------------------------------
    // scandirat
    // -----------------------------------------------------------------------

    #[test]
    fn test_scandirat_null_dirname() {
        crate::errno::set_errno(0);
        let mut list: *mut *mut Dirent = core::ptr::null_mut();
        let ret = scandirat(
            crate::file::AT_FDCWD,
            core::ptr::null(),
            &raw mut list,
            None,
            None,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_scandirat_null_namelist() {
        crate::errno::set_errno(0);
        let ret = scandirat(
            crate::file::AT_FDCWD,
            b"/tmp\0".as_ptr(),
            core::ptr::null_mut(),
            None,
            None,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_scandirat_with_at_fdcwd() {
        // AT_FDCWD delegates to scandir — result depends on test host.
        let mut list: *mut *mut Dirent = core::ptr::null_mut();
        let _ret = scandirat(
            crate::file::AT_FDCWD,
            b"/nonexistent_scandirat\0".as_ptr(),
            &raw mut list,
            None,
            None,
        );
        // Just verify no crash.
    }

    // -- getdents / getdents64 --

    #[test]
    fn test_getdents_returns_enosys() {
        // Phase 67: fd 3 is not an open fd in the test environment, so
        // the new validator now reports EBADF before reaching the
        // ENOSYS sentinel.  Test updated to verify EBADF for this
        // closed-fd case.  A separate test below
        // (`test_getdents_valid_args_reach_enosys`) covers the actual
        // ENOSYS path with a real open fd.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(3, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents64_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents64(-1, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents64_zero_count_einval() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents64(3, buf.as_mut_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdents64_null_buf_efault() {
        crate::errno::set_errno(0);
        let ret = getdents64(3, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_getdents64_invalid_fd_ebadf() {
        // fd 9999 is far above any allocated test fd, so get_fd() returns None.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents64(9999, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_emit_linux_dirent64_layout() {
        // Verify the header layout: ino|off|reclen|type|name|NUL|pad.
        let mut out = [0u8; 64];
        let name = b"hi";
        let reclen = emit_linux_dirent64(&mut out, 0xCAFEBABE, 7, DT_REG, name)
            .expect("emit should succeed");
        // 19 (header) + 2 (name) + 1 (NUL) = 22 → rounded up to 24.
        assert_eq!(reclen, 24);
        // ino
        let ino = u64::from_le_bytes(out[0..8].try_into().unwrap());
        assert_eq!(ino, 0xCAFEBABE);
        // off
        let off = i64::from_le_bytes(out[8..16].try_into().unwrap());
        assert_eq!(off, 7);
        // reclen
        let rl = u16::from_le_bytes(out[16..18].try_into().unwrap());
        assert_eq!(rl as usize, reclen);
        // type
        assert_eq!(out[18], DT_REG);
        // name + NUL
        assert_eq!(&out[19..21], b"hi");
        assert_eq!(out[21], 0);
        // Padding bytes are zero.
        assert_eq!(out[22], 0);
        assert_eq!(out[23], 0);
    }

    #[test]
    fn test_emit_linux_dirent64_too_small() {
        let mut out = [0u8; 8];
        assert!(emit_linux_dirent64(&mut out, 1, 1, DT_DIR, b"abc").is_none());
    }

    #[test]
    fn test_emit_linux_dirent64_alignment() {
        // Every record must be a multiple of 8 bytes so consecutive
        // records keep their u64 fields aligned.
        let mut out = [0u8; 128];
        for name in [&b"a"[..], &b"abc"[..], &b"longer"[..], &b"exactly7"[..]] {
            let r = emit_linux_dirent64(&mut out, 0, 0, DT_REG, name).expect("emit should succeed");
            assert_eq!(r % 8, 0, "reclen {r} not 8-aligned for name {name:?}");
        }
    }

    #[test]
    fn fill_dirent64_batch_translates_the_type_code() {
        // The DT_* value must be the translated one, never the kernel's raw
        // code: they collide (kernel 1 = directory, DT_REG = 8, DT_DIR = 4),
        // so passing one through as the other is silently wrong.
        let mut records = [0u8; 128];
        let n = packed_record(&mut records, KERNEL_TYPE_DIR, b"foo", 0, 9);
        let mut out = [0u8; 128];
        let (written, _) = fill_dirent64_batch(&records[..n], 0, &mut out);
        assert!(written > 0);
        assert_eq!(out[18], DT_DIR);
        assert_ne!(out[18], KERNEL_TYPE_DIR);
    }

    #[test]
    fn test_getdents_cache_pool_constants() {
        // Raised from 4 when the snapshot moved to the heap: a slot used to
        // hold a 68 KiB inline buffer, so four of them were 270 KiB of .bss
        // and the constant was a memory decision.  It is now a handful of
        // words per slot, and the number is just how many directories may be
        // under raw `getdents64` iteration at once.
        assert_eq!(MAX_GETDENTS_CACHES, 16);
    }

    #[test]
    fn a_negative_fd_never_reaches_the_getdents_cache() {
        // The pool is keyed by fd and every free slot carries `fd == -1`, so
        // a negative fd must be rejected by `getdents64` before any lookup —
        // otherwise it would match an empty slot.  `in_use` is what actually
        // guards that, and this is the end-to-end check of it.
        let mut buf = [0u8; 512];
        errno::set_errno(0);
        assert_eq!(getdents64(-1, buf.as_mut_ptr(), buf.len()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_linux_dirent64_header_size() {
        // 8 (ino) + 8 (off) + 2 (reclen) + 1 (type) = 19.
        assert_eq!(LINUX_DIRENT64_HEADER, 19);
    }

    // -----------------------------------------------------------------
    // Phase 67 — getdents argument-domain validators
    // -----------------------------------------------------------------
    //
    // The legacy `getdents` stub remains policy-driven (returns ENOSYS
    // on valid calls because the 32-bit-ino record format can't
    // represent our 64-bit inodes safely).  But invalid calls must
    // produce the same errno values Linux would, so a buggy caller is
    // not misled by ENOSYS into thinking the function never exists.

    // --- per-error-class ---

    #[test]
    fn test_getdents_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(-1, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_very_negative_fd_ebadf() {
        // Even an "AT_FDCWD-like" -100 fd is rejected with EBADF here.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(-100, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_zero_count_einval() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(3, buf.as_mut_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdents_null_buf_efault() {
        crate::errno::set_errno(0);
        let ret = getdents(3, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_getdents_closed_fd_ebadf() {
        // fd 9999 is far beyond any allocated fd in tests, so fdtable
        // returns None and we report EBADF.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(9999, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_valid_args_reach_enosys() {
        // Open a real fd via pipe() and pass it to getdents.  The fd
        // is not a directory, but getdents's stub does not check kind
        // — it only verifies the fd is open, then returns ENOSYS.
        // A future refinement (when kind tracking lands for directories)
        // would refine non-directory fds to ENOTDIR.
        let mut pf = [-1i32; 2];
        let r = crate::pipe::pipe(pf.as_mut_ptr());
        assert_eq!(r, 0, "pipe() must succeed to set up test");
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(pf[0], buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        // Clean up pipe fds.
        let _ = crate::fdtable::close_fd(pf[0]);
        let _ = crate::fdtable::close_fd(pf[1]);
    }

    // --- ordering ---

    #[test]
    fn test_getdents_negative_fd_beats_zero_count() {
        // fd<0 check fires before count==0 check.
        crate::errno::set_errno(0);
        let ret = getdents(-1, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_negative_fd_beats_null_buf() {
        crate::errno::set_errno(0);
        let ret = getdents(-1, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_getdents_zero_count_beats_null_buf() {
        // count==0 check fires before NULL-buf check.
        crate::errno::set_errno(0);
        let ret = getdents(3, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdents_null_buf_beats_closed_fd() {
        // NULL-buf check (with non-zero count) fires before the
        // fdtable lookup, so a closed fd plus NULL buf produces EFAULT,
        // not EBADF.
        crate::errno::set_errno(0);
        let ret = getdents(9999, core::ptr::null_mut(), 256);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- ordering parity with getdents64 ---

    #[test]
    fn test_getdents_and_getdents64_share_validation_order() {
        // Both validators check fd<0 first, then count==0, then NULL
        // buf, then fdtable.  This test pins that parity so a future
        // refactor doesn't diverge them silently.
        crate::errno::set_errno(0);
        let r1 = getdents(-1, core::ptr::null_mut(), 0);
        let e1 = crate::errno::get_errno();
        crate::errno::set_errno(0);
        let r2 = getdents64(-1, core::ptr::null_mut(), 0);
        let e2 = crate::errno::get_errno();
        assert_eq!(r1, -1);
        assert_eq!(r2, -1);
        assert_eq!(e1, e2);
        assert_eq!(e1, crate::errno::EBADF);
    }

    // --- real-world workflows ---

    #[test]
    fn test_workflow_legacy_program_calling_raw_getdents() {
        // A 32-bit-era program (or test harness emulating one) calls
        // the raw getdents syscall directly with a valid open fd.
        // Modern kernels would happily return records; we return
        // ENOSYS because we don't support the 32-bit-ino layout, but
        // the call must not be confused with "fd was bad".
        let mut pf = [-1i32; 2];
        assert_eq!(crate::pipe::pipe(pf.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let mut buf = [0u8; 1024];
        let ret = getdents(pf[0], buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        let _ = crate::fdtable::close_fd(pf[0]);
        let _ = crate::fdtable::close_fd(pf[1]);
    }

    // --- buggy callers ---

    #[test]
    fn test_buggy_caller_passes_closed_fd() {
        // A caller forgot to check the return value of open() and
        // passes the -1 sentinel through.  Linux returns EBADF; we
        // must too — not ENOSYS, which would suggest the function
        // doesn't exist at all.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(-1, buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_caller_zero_size_buffer() {
        // A caller miscomputes the buffer size as 0.  Linux returns
        // EINVAL; we must too.
        crate::errno::set_errno(0);
        let mut buf = [0u8; 256];
        let ret = getdents(3, buf.as_mut_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_caller_uninitialised_buffer_pointer() {
        // A caller forgets to allocate the buffer and passes NULL.
        // Linux returns EFAULT; we must too.
        crate::errno::set_errno(0);
        let ret = getdents(3, core::ptr::null_mut(), 4096);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }
}
