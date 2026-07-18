//! memfd — anonymous in-memory file.
//!
//! Backs the Linux `memfd_create(2)` syscall.  A memfd is an anonymous
//! file with no on-disk presence — its bytes live in a kernel-owned
//! `Vec<u8>` until the last fd referencing the memfd is closed.
//!
//! ## Use Cases
//!
//! - Anonymous shared memory across processes (typically combined with
//!   `mmap`, though mmap on a memfd is not yet wired in this kernel).
//! - File sealing for IPC integrity (`F_ADD_SEALS` / `F_GET_SEALS`).
//! - Sandboxed shared buffers (Vulkan / Wayland / shm_open replacement).
//!
//! ## Semantics
//!
//! - A memfd is a *regular file* with `S_IFREG | 0o777` mode.  Reads and
//!   writes are byte-streams against an in-kernel `Vec<u8>` with a
//!   per-memfd current offset.  All callers sharing the same memfd
//!   (via `dup`, `dup2`, `dup3`, or `pidfd_getfd`) share both the bytes
//!   AND the offset — matching Linux's "open file description" sharing
//!   semantics across dup, and matching the behaviour of any other
//!   POSIX regular file dup'd within a process.
//!
//! - `read(buf)` copies `min(buf.len(), size - off)` bytes starting at
//!   the current offset and advances the offset by that many.  EOF at
//!   end-of-data returns 0.
//!
//! - `write(buf)` extends the data if the offset is past the current
//!   end, copies `buf.len()` bytes at the current offset, and advances
//!   the offset.  Returns the byte count written.
//!
//! - `pread_at(buf, off)` / `pwrite_at(buf, off)` are positional and
//!   do not modify the current offset.  pwrite_at may extend the data.
//!
//! - `lseek(off, whence)` updates the offset.  Seeking past end-of-data
//!   is permitted (a subsequent write creates a hole filled with zeros,
//!   matching Linux's sparse-file semantics for regular files).  The
//!   memfd is **not** sparse in storage — bytes are always materialised.
//!
//! - `truncate(new_size)` resizes the backing data, filling newly added
//!   bytes with zeros.  Shrinking truncates discarded bytes.
//!
//! - `close()` decrements the refcount; the entry is removed only when
//!   the refcount reaches 0 (matching the Pipe / EventFd dup pattern).
//!
//! ## Sealing (deferred)
//!
//! `MFD_ALLOW_SEALING` is accepted at create time and the seal flag is
//! stored, but `F_ADD_SEALS` / `F_GET_SEALS` are not yet plumbed through
//! the Linux ABI fcntl path.  See `todo.txt` for the follow-up batch.
//!
//! ## Lock Ordering
//!
//! `MEMFD_TABLE` is the only lock taken inside this module; it does not
//! call back into the scheduler (no blocking).  Reads and writes are
//! pure memory copies under the mutex.

use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for a memfd.
type MemFdId = u64;

/// Counter for generating unique IDs.
static NEXT_MEMFD_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_memfd_id() -> MemFdId {
    NEXT_MEMFD_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to a memfd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemFdHandle(u64);

impl MemFdHandle {
    /// Reconstruct from raw u64 (used by the Linux ABI fd-table dispatch
    /// layer which stores the handle as `FdEntry::raw_handle`).
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw u64 representation.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    fn id(self) -> MemFdId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// memfd internals
// ---------------------------------------------------------------------------

/// MemFd seal bit values — match Linux uapi `linux/fcntl.h`:
///   * F_SEAL_SEAL   = 0x0001 — no more seals may be added.
///   * F_SEAL_SHRINK = 0x0002 — file may not shrink.
///   * F_SEAL_GROW   = 0x0004 — file may not grow.
///   * F_SEAL_WRITE  = 0x0008 — no further writes.
///   * F_SEAL_FUTURE_WRITE = 0x0010 — no NEW write mappings (older mappings
///     may still write); we do not yet honour mmap so this collapses to
///     the same enforcement as F_SEAL_WRITE for our current dispatch.
///
/// Stored but not yet enforced (see module docs).
pub const F_SEAL_SEAL: u32 = 0x0001;
pub const F_SEAL_SHRINK: u32 = 0x0002;
pub const F_SEAL_GROW: u32 = 0x0004;
pub const F_SEAL_WRITE: u32 = 0x0008;
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;

/// In-kernel memfd record.
struct MemFd {
    /// Backing bytes.  `len()` is the file size.
    data: Vec<u8>,
    /// Current offset shared across all fds referencing this memfd —
    /// matches Linux open-file-description sharing semantics for dup'd
    /// fds.
    offset: u64,
    /// Display name (memfd_create's `name` argument, capped to 249
    /// bytes per Linux semantics — the `/memfd:` prefix accounts for
    /// the remaining seven).  Stored only for /proc/self/fd display
    /// (we don't yet expose that path).
    name: Vec<u8>,
    /// Seal mask.  See `F_SEAL_*` constants.
    seals: u32,
    /// Whether the caller set `MFD_ALLOW_SEALING` at create time.  When
    /// `false`, the implicit `F_SEAL_SEAL` is set so no other seals may
    /// ever be added.
    allow_sealing: bool,
    /// Reference count.  Each successful `create_with_flags()` or
    /// `dup()` adds 1; each `close()` subtracts 1.  The entry is
    /// removed from the global table when this drops to 0.
    refcount: u32,
}

impl MemFd {
    fn new(name: Vec<u8>, allow_sealing: bool) -> Self {
        let seals = if allow_sealing { 0 } else { F_SEAL_SEAL };
        Self {
            data: Vec::new(),
            offset: 0,
            name,
            seals,
            allow_sealing,
            refcount: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

static MEMFD_TABLE: Mutex<BTreeMap<MemFdId, MemFd>> =
    Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new memfd with the given display name and creation flags.
///
/// `allow_sealing` indicates `MFD_ALLOW_SEALING` was set: when `false`,
/// `F_SEAL_SEAL` is set implicitly so `F_ADD_SEALS` always fails with
/// EPERM.  The returned handle starts with refcount = 1 and offset = 0.
///
/// The display name is stored verbatim; the caller is responsible for
/// any size cap (Linux truncates to 249 bytes).
#[must_use]
pub fn create_with_flags(name: Vec<u8>, allow_sealing: bool) -> MemFdHandle {
    let id = alloc_memfd_id();
    let mf = MemFd::new(name, allow_sealing);
    MEMFD_TABLE.lock().insert(id, mf);
    MemFdHandle(id)
}

/// Duplicate a memfd handle reference — increments the refcount and
/// returns the same handle.
///
/// Used by fork (per-process refcount bump so a child holds its own
/// reference) and by `pidfd_getfd` (cross-process handle copy).
///
/// # Errors
///
/// - `InvalidHandle` — handle not found (already fully closed) or
///   refcount would overflow `u32::MAX`.
pub fn dup(handle: MemFdHandle) -> KernelResult<MemFdHandle> {
    let mut table = MEMFD_TABLE.lock();
    let mf = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    mf.refcount = mf
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to a memfd handle.  Removes the entry when the
/// refcount reaches 0.
pub fn close(handle: MemFdHandle) {
    let mut table = MEMFD_TABLE.lock();
    if let Some(mf) = table.get_mut(&handle.id()) {
        mf.refcount = mf.refcount.saturating_sub(1);
        if mf.refcount == 0 {
            table.remove(&handle.id());
        }
    }
}

/// Current size of the backing data (the "file size").
///
/// Returns `Err(InvalidHandle)` if the handle is not in the table.
pub fn size(handle: MemFdHandle) -> KernelResult<u64> {
    let table = MEMFD_TABLE.lock();
    let mf = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    Ok(mf.data.len() as u64)
}

/// Current shared offset of the memfd (across all dup'd fds).
///
/// Used by `stat`/`statx` to report `st_size` independently of the
/// offset, and by self-tests to verify offset state.
pub fn offset(handle: MemFdHandle) -> KernelResult<u64> {
    let table = MEMFD_TABLE.lock();
    let mf = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    Ok(mf.offset)
}

/// Caller-supplied display name for this memfd (the `name` argument to
/// `memfd_create`).  Linux exposes this in `/proc/self/fd/<N>` as
/// `/memfd:<name> (deleted)`.  We don't have `/proc` yet, but any
/// caller building such a path label (or a debug log line) reads the
/// name through this getter.
///
/// Returns `Err(InvalidHandle)` if the handle has been closed.
pub fn name(handle: MemFdHandle) -> KernelResult<Vec<u8>> {
    let table = MEMFD_TABLE.lock();
    let mf = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    Ok(mf.name.clone())
}

/// Read up to `buf.len()` bytes starting at the current offset.  Returns
/// the number of bytes copied (0 at EOF).  Advances the offset by the
/// number of bytes read.
pub fn read(handle: MemFdHandle, buf: &mut [u8]) -> KernelResult<usize> {
    let mut table = MEMFD_TABLE.lock();
    let mf = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    let size = mf.data.len() as u64;
    if mf.offset >= size {
        return Ok(0);
    }
    let remaining = size - mf.offset;
    let to_read = (buf.len() as u64).min(remaining) as usize;
    let start = mf.offset as usize;
    let end = start.saturating_add(to_read);
    // SAFETY (logical): `start` < `data.len()` and `end <= data.len()`
    // since `to_read <= remaining = data.len() - offset`.
    buf[..to_read].copy_from_slice(&mf.data[start..end]);
    mf.offset = mf.offset.saturating_add(to_read as u64);
    Ok(to_read)
}

/// Positional read from `offset`.  Does **not** modify the shared
/// current offset.  Returns bytes copied (0 if `offset >= size`).
pub fn read_at(handle: MemFdHandle, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
    let table = MEMFD_TABLE.lock();
    let mf = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    let size = mf.data.len() as u64;
    if offset >= size {
        return Ok(0);
    }
    let remaining = size - offset;
    let to_read = (buf.len() as u64).min(remaining) as usize;
    let start = offset as usize;
    let end = start.saturating_add(to_read);
    buf[..to_read].copy_from_slice(&mf.data[start..end]);
    Ok(to_read)
}

/// Write `buf.len()` bytes at the current offset.  Extends the backing
/// data if the write goes past current EOF (newly added bytes are
/// zero-initialised first, then overwritten).  Advances the offset by
/// `buf.len()`.  Returns the byte count written (always `buf.len()`
/// unless a seal blocks the operation).
///
/// Honours `F_SEAL_WRITE` (returns `PermissionDenied`) and
/// `F_SEAL_GROW` (returns `PermissionDenied` if the write would extend
/// the file).
pub fn write(handle: MemFdHandle, buf: &[u8]) -> KernelResult<usize> {
    let mut table = MEMFD_TABLE.lock();
    let mf = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    if mf.seals & F_SEAL_WRITE != 0 {
        return Err(KernelError::PermissionDenied);
    }
    let end = mf.offset.saturating_add(buf.len() as u64);
    let cur_len = mf.data.len() as u64;
    if end > cur_len {
        if mf.seals & F_SEAL_GROW != 0 {
            return Err(KernelError::PermissionDenied);
        }
        // Zero-fill any hole between cur_len and offset; the write
        // itself overlays bytes from offset..end.
        mf.data.resize(end as usize, 0);
    }
    let start = mf.offset as usize;
    let end_usize = end as usize;
    mf.data[start..end_usize].copy_from_slice(buf);
    mf.offset = end;
    Ok(buf.len())
}

/// Positional write at `offset`.  Does **not** modify the shared
/// current offset.  May extend the backing data (subject to
/// `F_SEAL_GROW`).
pub fn write_at(handle: MemFdHandle, offset: u64, buf: &[u8]) -> KernelResult<usize> {
    let mut table = MEMFD_TABLE.lock();
    let mf = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    if mf.seals & F_SEAL_WRITE != 0 {
        return Err(KernelError::PermissionDenied);
    }
    let end = offset.saturating_add(buf.len() as u64);
    let cur_len = mf.data.len() as u64;
    if end > cur_len {
        if mf.seals & F_SEAL_GROW != 0 {
            return Err(KernelError::PermissionDenied);
        }
        mf.data.resize(end as usize, 0);
    }
    let start = offset as usize;
    let end_usize = end as usize;
    mf.data[start..end_usize].copy_from_slice(buf);
    Ok(buf.len())
}

/// Whence values for `seek`.  Match the Linux `SEEK_*` constants.
pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

/// Update the shared current offset.  Returns the new offset.
///
/// `whence`:
///   * 0 (SEEK_SET) — set offset to `pos` (rejected if `pos < 0`).
///   * 1 (SEEK_CUR) — add `pos` to current offset (signed).
///   * 2 (SEEK_END) — set offset to `data.len() + pos` (signed).
///
/// Linux allows seeking past EOF (subsequent writes create a hole).
///
/// # Errors
///
/// - `InvalidArgument` — invalid `whence` or a result < 0.
/// - `InvalidHandle` — handle closed.
pub fn seek(handle: MemFdHandle, pos: i64, whence: u32) -> KernelResult<u64> {
    let mut table = MEMFD_TABLE.lock();
    let mf = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    let new_off: i64 = match whence {
        SEEK_SET => pos,
        SEEK_CUR => {
            let cur = i64::try_from(mf.offset)
                .map_err(|_| KernelError::InvalidArgument)?;
            cur.checked_add(pos).ok_or(KernelError::InvalidArgument)?
        }
        SEEK_END => {
            let end = i64::try_from(mf.data.len() as u64)
                .map_err(|_| KernelError::InvalidArgument)?;
            end.checked_add(pos).ok_or(KernelError::InvalidArgument)?
        }
        _ => return Err(KernelError::InvalidArgument),
    };
    if new_off < 0 {
        return Err(KernelError::InvalidArgument);
    }
    #[allow(clippy::cast_sign_loss)]
    let new_off_u = new_off as u64;
    mf.offset = new_off_u;
    Ok(new_off_u)
}

/// Resize the backing data to `new_size`.  Newly added bytes are zero
/// filled; shrinking discards data past the new size.  Does not modify
/// the current offset (an offset past the new size simply becomes a
/// past-EOF offset — Linux behaviour).
///
/// Honours `F_SEAL_SHRINK` (returns `PermissionDenied` for size <
/// current) and `F_SEAL_GROW` (returns `PermissionDenied` for size >
/// current).
pub fn truncate(handle: MemFdHandle, new_size: u64) -> KernelResult<()> {
    let mut table = MEMFD_TABLE.lock();
    let mf = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    let cur = mf.data.len() as u64;
    if new_size < cur && mf.seals & F_SEAL_SHRINK != 0 {
        return Err(KernelError::PermissionDenied);
    }
    if new_size > cur && mf.seals & F_SEAL_GROW != 0 {
        return Err(KernelError::PermissionDenied);
    }
    let new_usize = usize::try_from(new_size)
        .map_err(|_| KernelError::InvalidArgument)?;
    mf.data.resize(new_usize, 0);
    Ok(())
}

/// Read the current seal mask.
pub fn get_seals(handle: MemFdHandle) -> KernelResult<u32> {
    let table = MEMFD_TABLE.lock();
    let mf = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    Ok(mf.seals)
}

/// Add seals to the memfd.  Fails with `PermissionDenied` if
/// `F_SEAL_SEAL` is already set (sealing is closed off) or the memfd
/// was created without `MFD_ALLOW_SEALING`.
///
/// `add` is a bitmask of `F_SEAL_*` values.  Unknown bits are rejected
/// with `InvalidArgument` (matching Linux's `EINVAL`).
pub fn add_seals(handle: MemFdHandle, add: u32) -> KernelResult<()> {
    const KNOWN: u32 =
        F_SEAL_SEAL | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE | F_SEAL_FUTURE_WRITE;
    if add & !KNOWN != 0 {
        return Err(KernelError::InvalidArgument);
    }
    let mut table = MEMFD_TABLE.lock();
    let mf = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    if !mf.allow_sealing {
        return Err(KernelError::PermissionDenied);
    }
    if mf.seals & F_SEAL_SEAL != 0 {
        return Err(KernelError::PermissionDenied);
    }
    mf.seals |= add;
    Ok(())
}

/// Linux `poll(2)`-style readiness bits — a memfd is a regular file
/// and always returns POLLIN | POLLOUT (no blocking I/O).
///
/// Returns 0 if the handle has been closed (the caller will see EBADF
/// on read/write).
#[must_use]
pub fn poll_status(handle: MemFdHandle) -> u16 {
    let table = MEMFD_TABLE.lock();
    if table.contains_key(&handle.id()) {
        // POLLIN = 0x0001, POLLOUT = 0x0004
        0x0001 | 0x0004
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run memfd self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[memfd] Running memfd self-test...");
    test_create_close()?;
    test_read_write_offset()?;
    test_read_at_pwrite_at()?;
    test_seek_set_cur_end()?;
    test_truncate_grow_shrink()?;
    test_dup_refcount()?;
    test_seal_write_blocks_write()?;
    test_seal_seal_blocks_add_seals()?;
    test_no_allow_sealing_rejects_add()?;
    test_poll_status_ready()?;
    test_name_round_trip()?;
    serial_println!("[memfd] Memfd self-test PASSED");
    Ok(())
}

fn test_create_close() -> KernelResult<()> {
    let h = create_with_flags(b"test1".to_vec(), false);
    if size(h)? != 0 {
        serial_println!("[memfd]   FAIL: fresh memfd size != 0");
        close(h);
        return Err(KernelError::InternalError);
    }
    close(h);
    // After close (refcount 1 → 0) the handle should be invalid.
    if size(h).is_ok() {
        serial_println!("[memfd]   FAIL: size after close should be InvalidHandle");
        return Err(KernelError::InternalError);
    }
    serial_println!("[memfd]   create / close: OK");
    Ok(())
}

/// Verify the caller-supplied name round-trips through create →
/// [`name()`] and that `name()` returns `InvalidHandle` after close.
fn test_name_round_trip() -> KernelResult<()> {
    let h = create_with_flags(b"display-name".to_vec(), false);
    let got = name(h)?;
    if got.as_slice() != b"display-name" {
        serial_println!("[memfd]   FAIL: name() mismatch");
        close(h);
        return Err(KernelError::InternalError);
    }
    close(h);
    if name(h).is_ok() {
        serial_println!("[memfd]   FAIL: name() after close should be InvalidHandle");
        return Err(KernelError::InternalError);
    }
    serial_println!("[memfd]   name round-trip: OK");
    Ok(())
}

fn test_read_write_offset() -> KernelResult<()> {
    let h = create_with_flags(b"rw".to_vec(), false);
    let n = write(h, b"hello")?;
    if n != 5 {
        serial_println!("[memfd]   FAIL: write returned {} want 5", n);
        close(h);
        return Err(KernelError::InternalError);
    }
    if size(h)? != 5 || offset(h)? != 5 {
        serial_println!("[memfd]   FAIL: post-write size/offset mismatch");
        close(h);
        return Err(KernelError::InternalError);
    }
    // Seek back and read.
    seek(h, 0, SEEK_SET)?;
    let mut rbuf = [0u8; 5];
    let n = read(h, &mut rbuf)?;
    if n != 5 || &rbuf != b"hello" {
        serial_println!("[memfd]   FAIL: read got {:?} ({} bytes)", rbuf, n);
        close(h);
        return Err(KernelError::InternalError);
    }
    // Reading past EOF returns 0.
    if read(h, &mut rbuf)? != 0 {
        serial_println!("[memfd]   FAIL: read past EOF should return 0");
        close(h);
        return Err(KernelError::InternalError);
    }
    close(h);
    serial_println!("[memfd]   read/write/offset: OK");
    Ok(())
}

fn test_read_at_pwrite_at() -> KernelResult<()> {
    let h = create_with_flags(b"pio".to_vec(), false);
    write_at(h, 4, b"WXYZ")?;
    // Offset should still be 0 (write_at does not advance).
    if offset(h)? != 0 {
        serial_println!("[memfd]   FAIL: write_at changed offset");
        close(h);
        return Err(KernelError::InternalError);
    }
    // Size should be 8 (4 zero hole + 4 bytes data).
    if size(h)? != 8 {
        serial_println!("[memfd]   FAIL: write_at size {} want 8", size(h)?);
        close(h);
        return Err(KernelError::InternalError);
    }
    let mut buf = [0u8; 8];
    let n = read_at(h, 0, &mut buf)?;
    if n != 8 || buf[..4] != [0; 4] || &buf[4..] != b"WXYZ" {
        serial_println!("[memfd]   FAIL: read_at got {:?}", buf);
        close(h);
        return Err(KernelError::InternalError);
    }
    if offset(h)? != 0 {
        serial_println!("[memfd]   FAIL: read_at changed offset");
        close(h);
        return Err(KernelError::InternalError);
    }
    close(h);
    serial_println!("[memfd]   read_at / write_at: OK");
    Ok(())
}

fn test_seek_set_cur_end() -> KernelResult<()> {
    let h = create_with_flags(b"sk".to_vec(), false);
    write(h, b"0123456789")?; // size 10, offset 10
    let p = seek(h, 3, SEEK_SET)?;
    if p != 3 || offset(h)? != 3 {
        serial_println!("[memfd]   FAIL: SEEK_SET 3 -> {}", p);
        close(h);
        return Err(KernelError::InternalError);
    }
    let p = seek(h, 2, SEEK_CUR)?; // 3 + 2 = 5
    if p != 5 {
        serial_println!("[memfd]   FAIL: SEEK_CUR +2 -> {} want 5", p);
        close(h);
        return Err(KernelError::InternalError);
    }
    let p = seek(h, -1, SEEK_END)?; // 10 - 1 = 9
    if p != 9 {
        serial_println!("[memfd]   FAIL: SEEK_END -1 -> {} want 9", p);
        close(h);
        return Err(KernelError::InternalError);
    }
    // Negative result must fail.
    if seek(h, -100, SEEK_SET).is_ok() {
        serial_println!("[memfd]   FAIL: SEEK_SET negative should error");
        close(h);
        return Err(KernelError::InternalError);
    }
    // Invalid whence.
    if seek(h, 0, 99).is_ok() {
        serial_println!("[memfd]   FAIL: bogus whence should error");
        close(h);
        return Err(KernelError::InternalError);
    }
    close(h);
    serial_println!("[memfd]   seek: OK");
    Ok(())
}

fn test_truncate_grow_shrink() -> KernelResult<()> {
    let h = create_with_flags(b"tr".to_vec(), false);
    write(h, b"abcdef")?; // size 6
    truncate(h, 3)?; // shrink to 3
    if size(h)? != 3 {
        serial_println!("[memfd]   FAIL: shrink size {} want 3", size(h)?);
        close(h);
        return Err(KernelError::InternalError);
    }
    truncate(h, 10)?; // grow to 10 (zero-fill)
    if size(h)? != 10 {
        serial_println!("[memfd]   FAIL: grow size {} want 10", size(h)?);
        close(h);
        return Err(KernelError::InternalError);
    }
    let mut buf = [0xFFu8; 10];
    seek(h, 0, SEEK_SET)?;
    read(h, &mut buf)?;
    if &buf[..3] != b"abc" || buf[3..] != [0u8; 7] {
        serial_println!("[memfd]   FAIL: post-truncate bytes {:?}", buf);
        close(h);
        return Err(KernelError::InternalError);
    }
    close(h);
    serial_println!("[memfd]   truncate grow/shrink: OK");
    Ok(())
}

fn test_dup_refcount() -> KernelResult<()> {
    let h = create_with_flags(b"dup".to_vec(), false);
    let h2 = dup(h)?;
    if h2 != h {
        serial_println!("[memfd]   FAIL: dup returned different handle");
        close(h);
        close(h2);
        return Err(KernelError::InternalError);
    }
    write(h, b"shared")?;
    // h2 sees the same bytes (and the same offset, post-write).
    if offset(h2)? != 6 || size(h2)? != 6 {
        serial_println!("[memfd]   FAIL: dup'd offset/size mismatch");
        close(h);
        close(h2);
        return Err(KernelError::InternalError);
    }
    // Close once — refcount 2 → 1.  Handle still valid.
    close(h);
    if size(h2).is_err() {
        serial_println!("[memfd]   FAIL: handle invalid after first close");
        close(h2);
        return Err(KernelError::InternalError);
    }
    // Final close.
    close(h2);
    if size(h2).is_ok() {
        serial_println!("[memfd]   FAIL: handle still valid after final close");
        return Err(KernelError::InternalError);
    }
    // dup on a fully closed handle fails.
    if dup(h2).is_ok() {
        serial_println!("[memfd]   FAIL: dup on closed handle should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[memfd]   dup refcount: OK");
    Ok(())
}

fn test_seal_write_blocks_write() -> KernelResult<()> {
    let h = create_with_flags(b"sw".to_vec(), true); // allow sealing
    write(h, b"abc")?;
    add_seals(h, F_SEAL_WRITE)?;
    match write(h, b"xx") {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[memfd]   FAIL: write after F_SEAL_WRITE: {:?}", other);
            close(h);
            return Err(KernelError::InternalError);
        }
    }
    match write_at(h, 0, b"xx") {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[memfd]   FAIL: write_at after F_SEAL_WRITE: {:?}", other);
            close(h);
            return Err(KernelError::InternalError);
        }
    }
    close(h);
    serial_println!("[memfd]   F_SEAL_WRITE blocks writes: OK");
    Ok(())
}

fn test_seal_seal_blocks_add_seals() -> KernelResult<()> {
    let h = create_with_flags(b"ss".to_vec(), true);
    add_seals(h, F_SEAL_SEAL)?;
    // Further seal attempts must be rejected.
    match add_seals(h, F_SEAL_WRITE) {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[memfd]   FAIL: add_seals after F_SEAL_SEAL: {:?}", other);
            close(h);
            return Err(KernelError::InternalError);
        }
    }
    close(h);
    serial_println!("[memfd]   F_SEAL_SEAL blocks add_seals: OK");
    Ok(())
}

fn test_no_allow_sealing_rejects_add() -> KernelResult<()> {
    let h = create_with_flags(b"ns".to_vec(), false); // implicit F_SEAL_SEAL
    let s = get_seals(h)?;
    if s & F_SEAL_SEAL == 0 {
        serial_println!("[memfd]   FAIL: implicit F_SEAL_SEAL missing");
        close(h);
        return Err(KernelError::InternalError);
    }
    match add_seals(h, F_SEAL_WRITE) {
        Err(KernelError::PermissionDenied) => {}
        other => {
            serial_println!("[memfd]   FAIL: add_seals w/o ALLOW_SEALING: {:?}", other);
            close(h);
            return Err(KernelError::InternalError);
        }
    }
    close(h);
    serial_println!("[memfd]   no ALLOW_SEALING: OK");
    Ok(())
}

fn test_poll_status_ready() -> KernelResult<()> {
    let h = create_with_flags(b"po".to_vec(), false);
    let s = poll_status(h);
    if s & 0x0001 == 0 || s & 0x0004 == 0 {
        serial_println!("[memfd]   FAIL: poll_status missing POLLIN|POLLOUT: {:#x}", s);
        close(h);
        return Err(KernelError::InternalError);
    }
    close(h);
    if poll_status(h) != 0 {
        serial_println!("[memfd]   FAIL: poll_status after close should be 0");
        return Err(KernelError::InternalError);
    }
    serial_println!("[memfd]   poll_status: OK");
    Ok(())
}
