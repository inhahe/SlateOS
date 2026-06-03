//! System V shared memory — `<sys/shm.h>`.
//!
//! A real (single-process) implementation of `shmget` / `shmat` /
//! `shmdt` / `shmctl`, replacing the four ENOSYS stubs.
//!
//! ## Design
//!
//! Each segment is backed by a fixed slot in a static [`SegmentStorage`]
//! pool — [`MAX_SEGMENTS`] segments of [`SEGMENT_SIZE`] bytes each,
//! 16 KiB-aligned (our page size) so `shmat` returns a properly
//! page-aligned address.  The pool sits in BSS; unused slots cost
//! nothing at run time on demand-paged systems.
//!
//! A single global spinlock ([`SHM_LOCK`]) serialises all mutations of
//! the per-slot metadata.  The data buffer itself is *not* under the
//! lock — that's the whole point of shared memory.
//!
//! Segment IDs use the same generation-tagged encoding as `sysv_sem` /
//! `sysv_msg`: `(generation << 16) | (slot + 1)`.
//!
//! ## Lifecycle
//!
//! Linux semantics: `shmctl(IPC_RMID)` marks the segment for deletion
//! but **does not** free it until the last attached caller detaches.
//! We follow that contract — `IPC_RMID` flips a `marked_for_rmid`
//! flag, and the slot is only returned to the free pool when
//! `nattch` drops to zero.  After `IPC_RMID`, the segment can no
//! longer be looked up by key (so a future `shmget` with the same key
//! always creates a new slot), but existing attached pointers stay
//! valid for as long as any caller still holds one.
//!
//! ## Multiple attaches
//!
//! `shmat` is called once per attaching caller; classically each call
//! returns a *different* mapping (because each process has its own
//! address space).  In our single-process world the same backing
//! buffer is shared by everyone, so every `shmat` call returns the
//! same pointer and `shm_nattch` is just a reference count.  `shmdt`
//! decrements the count.
//!
//! ## Limitations
//!
//! * Single-process only — there's no kernel-side namespace yet, so a
//!   second process wouldn't see segments created by the first.  When
//!   we add cross-process IPC, this layer will need rework to back
//!   segments with kernel-managed virtual mappings.
//! * `SHM_REMAP`, `SHM_RND`, caller-supplied `shmaddr` — accepted but
//!   ignored (we always return our own pool address).
//! * `SHM_RDONLY` — accepted but unenforced (we have no per-mapping
//!   permission machinery).
//! * `SHM_LOCK` / `SHM_UNLOCK` (in `shmctl`) — accepted as no-ops; our
//!   memory is never swapped.
//! * Maximum segment size is fixed at [`SEGMENT_SIZE`] (64 KiB).
//!   Programs requesting bigger get `EINVAL`.
//! * `shm_atime` / `shm_dtime` / `shm_ctime` / `shm_cpid` / `shm_lpid`
//!   stay 0 — we don't have process IDs or per-segment clocks.

use crate::errno;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Create if key doesn't exist.
pub const IPC_CREAT: i32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: i32 = 0o2000;

/// Remove identifier.
pub const IPC_RMID: i32 = 0;
/// Set options.
pub const IPC_SET: i32 = 1;
/// Get options.
pub const IPC_STAT: i32 = 2;

/// Private key.
pub const IPC_PRIVATE: i32 = 0;

/// Attach read-only.
pub const SHM_RDONLY: i32 = 0o10000;
/// Round attach address down to `SHMLBA`.
pub const SHM_RND: i32 = 0o20000;
/// Take-over region on attach (remove on last detach).
pub const SHM_REMAP: i32 = 0o40000;
/// Executable mapping.
pub const SHM_EXEC: i32 = 0o100000;

/// Lock pages in memory.
pub const SHM_LOCK: i32 = 11;
/// Unlock pages.
pub const SHM_UNLOCK: i32 = 12;

/// Segment low boundary address multiple (page size).
pub const SHMLBA: usize = 16384; // 16 KiB pages

// ---------------------------------------------------------------------------
// Pool sizing
// ---------------------------------------------------------------------------

const MAX_SEGMENTS: usize = 4;
const SEGMENT_SIZE: usize = 65536;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct shmid_ds` — shared memory segment data structure.
#[repr(C)]
pub struct ShmidDs {
    /// Owner's UID.
    pub shm_perm_uid: u32,
    /// Owner's GID.
    pub shm_perm_gid: u32,
    /// Creator's UID.
    pub shm_perm_cuid: u32,
    /// Creator's GID.
    pub shm_perm_cgid: u32,
    /// Permissions mode.
    pub shm_perm_mode: u16,
    /// Padding.
    pub _pad: u16,
    /// Segment size in bytes.
    pub shm_segsz: usize,
    /// PID of last shmat/shmdt.
    pub shm_lpid: i32,
    /// PID of creator.
    pub shm_cpid: i32,
    /// Number of current attaches.
    pub shm_nattch: usize,
    /// Last attach time.
    pub shm_atime: i64,
    /// Last detach time.
    pub shm_dtime: i64,
    /// Last change time.
    pub shm_ctime: i64,
}

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

/// Backing storage for a single segment.
///
/// On Windows COFF the maximum section alignment is 8192 bytes
/// (`IMAGE_SCN_ALIGN_8192BYTES`), so we cannot use a static aligned
/// directly to [`SHMLBA`] (16 KiB). Instead we allocate an extra
/// `SHMLBA - 1` slack bytes and round the pointer up at access time
/// in [`segment_ptr`].
#[repr(C, align(8192))]
struct SegmentStorage {
    bytes: UnsafeCell<[u8; SEGMENT_SIZE + SHMLBA]>,
}

// SAFETY: callers synchronise access externally — the segment buffer is
// shared memory by design and access discipline is the user's problem.
// The metadata (in `Segment`) is protected by SHM_LOCK.
unsafe impl Sync for SegmentStorage {}

#[derive(Clone, Copy)]
struct Segment {
    in_use: bool,
    /// Flipped by `IPC_RMID`; when nattch reaches 0 the slot is freed.
    marked_for_rmid: bool,
    key: i32,
    /// Logical size requested by `shmget` (≤ SEGMENT_SIZE).
    size: usize,
    mode: u16,
    nattch: usize,
    /// Bumped on every slot reuse to invalidate stale shmids.
    generation: u32,
}

impl Segment {
    const EMPTY: Self = Self {
        in_use: false,
        marked_for_rmid: false,
        key: 0,
        size: 0,
        mode: 0,
        nattch: 0,
        generation: 0,
    };
}

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

static SHM_TABLE_LOCK: AtomicBool = AtomicBool::new(false);
static mut SHM_META: [Segment; MAX_SEGMENTS] = [const { Segment::EMPTY }; MAX_SEGMENTS];
static SHM_STORAGE: [SegmentStorage; MAX_SEGMENTS] = [
    SegmentStorage {
        bytes: UnsafeCell::new([0u8; SEGMENT_SIZE + SHMLBA]),
    },
    SegmentStorage {
        bytes: UnsafeCell::new([0u8; SEGMENT_SIZE + SHMLBA]),
    },
    SegmentStorage {
        bytes: UnsafeCell::new([0u8; SEGMENT_SIZE + SHMLBA]),
    },
    SegmentStorage {
        bytes: UnsafeCell::new([0u8; SEGMENT_SIZE + SHMLBA]),
    },
];

fn lock_acquire() {
    while SHM_TABLE_LOCK
        .compare_exchange_weak(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn lock_release() {
    SHM_TABLE_LOCK.store(false, Ordering::Release);
}

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        lock_release();
    }
}

fn lock() -> Guard {
    lock_acquire();
    Guard
}

// ---------------------------------------------------------------------------
// shmid encoding
// ---------------------------------------------------------------------------

fn encode_shmid(slot: usize, generation: u32) -> i32 {
    let s = ((slot as u32) & 0xFFFF).wrapping_add(1);
    let g = generation & 0x7FFF;
    ((g << 16) | s) as i32
}

fn decode_shmid(shmid: i32) -> Option<(usize, u32)> {
    if shmid <= 0 {
        return None;
    }
    let u = shmid as u32;
    let s = (u & 0xFFFF) as usize;
    if s == 0 {
        return None;
    }
    let slot = s - 1;
    if slot >= MAX_SEGMENTS {
        return None;
    }
    let generation = (u >> 16) & 0x7FFF;
    Some((slot, generation))
}

/// Pointer to the (SHMLBA-aligned) backing buffer for slot `slot`.
///
/// We round the raw static address up to the next [`SHMLBA`] multiple
/// so callers always see a properly page-aligned mapping — necessary
/// because Windows COFF caps static alignment below `SHMLBA`.
fn segment_ptr(slot: usize) -> *mut u8 {
    // Bounded by caller.
    let raw = SHM_STORAGE[slot].bytes.get().cast::<u8>() as usize;
    // SHMLBA is a power of two — round up.
    let aligned = raw.wrapping_add(SHMLBA - 1) & !(SHMLBA - 1);
    aligned as *mut u8
}

// ---------------------------------------------------------------------------
// Helpers (all callers hold the lock)
// ---------------------------------------------------------------------------

/// SAFETY: caller holds the lock.
unsafe fn meta_ptr() -> *mut Segment {
    core::ptr::addr_of_mut!(SHM_META).cast::<Segment>()
}

/// SAFETY: caller holds the lock.
unsafe fn find_by_key(key: i32) -> Option<usize> {
    if key == IPC_PRIVATE {
        return None;
    }
    let meta = unsafe { meta_ptr() };
    let mut i: usize = 0;
    while i < MAX_SEGMENTS {
        let m = unsafe { meta.add(i) };
        // Only match segments that haven't been marked for deletion.
        if unsafe { (*m).in_use } && !unsafe { (*m).marked_for_rmid } && unsafe { (*m).key } == key
        {
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// SAFETY: caller holds the lock.
unsafe fn alloc_segment(key: i32, size: usize, mode: u16) -> Option<usize> {
    let meta = unsafe { meta_ptr() };
    let mut i: usize = 0;
    while i < MAX_SEGMENTS {
        let m = unsafe { meta.add(i) };
        if !unsafe { (*m).in_use } {
            unsafe {
                (*m).in_use = true;
                (*m).marked_for_rmid = false;
                (*m).key = key;
                (*m).size = size;
                (*m).mode = mode;
                (*m).nattch = 0;
            }
            // Zero the backing buffer so a reused slot doesn't leak
            // stale data to the next caller.
            // SAFETY: lock held; no live attachments since refcount is 0.
            unsafe {
                core::ptr::write_bytes(segment_ptr(i), 0, SEGMENT_SIZE);
            }
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// SAFETY: caller holds the lock.
unsafe fn resolve_shmid(shmid: i32) -> Option<usize> {
    let (slot, gen_) = decode_shmid(shmid)?;
    let meta = unsafe { meta_ptr() };
    let m = unsafe { meta.add(slot) };
    if !unsafe { (*m).in_use } {
        return None;
    }
    if unsafe { (*m).generation } & 0x7FFF != gen_ {
        return None;
    }
    Some(slot)
}

/// Free the slot and bump the generation counter.
///
/// SAFETY: caller holds the lock; `slot` must be in-use.
unsafe fn free_segment(slot: usize) {
    let meta = unsafe { meta_ptr() };
    let m = unsafe { meta.add(slot) };
    unsafe {
        (*m).in_use = false;
        (*m).marked_for_rmid = false;
        (*m).key = 0;
        (*m).size = 0;
        (*m).nattch = 0;
        (*m).generation = (*m).generation.wrapping_add(1);
    }
}

/// Try to look up a slot by its attached buffer pointer.
fn slot_for_ptr(ptr: *const u8) -> Option<usize> {
    let mut i: usize = 0;
    while i < MAX_SEGMENTS {
        if ptr == segment_ptr(i).cast_const() {
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

// ---------------------------------------------------------------------------
// shmget
// ---------------------------------------------------------------------------

/// `shmget` — get a shared memory identifier.
///
/// Returns the shmid on success or -1 with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmget(key: i32, size: usize, shmflg: i32) -> i32 {
    // Linux: size > SHMMAX → EINVAL.  Our SHMMAX is SEGMENT_SIZE.
    if size > SEGMENT_SIZE {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mode = (shmflg & 0o777) as u16;
    let _g = lock();
    if key == IPC_PRIVATE {
        if size == 0 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        let Some(slot) = (unsafe { alloc_segment(IPC_PRIVATE, size, mode) }) else {
            errno::set_errno(errno::ENOSPC);
            return -1;
        };
        let gen_ = unsafe {
            let meta = meta_ptr();
            (*meta.add(slot)).generation & 0x7FFF
        };
        return encode_shmid(slot, gen_);
    }
    if let Some(slot) = unsafe { find_by_key(key) } {
        if shmflg & IPC_CREAT != 0 && shmflg & IPC_EXCL != 0 {
            errno::set_errno(errno::EEXIST);
            return -1;
        }
        // Linux: existing segment with size > requested → EINVAL.
        // (POSIX permits the caller to pass size == 0 to mean "any size
        // is fine".)
        if size > 0 {
            let cur = unsafe {
                let meta = meta_ptr();
                (*meta.add(slot)).size
            };
            if size > cur {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
        }
        let gen_ = unsafe {
            let meta = meta_ptr();
            (*meta.add(slot)).generation & 0x7FFF
        };
        return encode_shmid(slot, gen_);
    }
    if shmflg & IPC_CREAT == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    if size == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let Some(slot) = (unsafe { alloc_segment(key, size, mode) }) else {
        errno::set_errno(errno::ENOSPC);
        return -1;
    };
    let gen_ = unsafe {
        let meta = meta_ptr();
        (*meta.add(slot)).generation & 0x7FFF
    };
    encode_shmid(slot, gen_)
}

// ---------------------------------------------------------------------------
// shmat
// ---------------------------------------------------------------------------

/// `shmat` — attach shared memory segment.
///
/// Returns the segment's address on success, or `(void *)-1` with
/// errno set on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmat(shmid: i32, _shmaddr: *const u8, _shmflg: i32) -> *mut u8 {
    let _g = lock();
    let Some(slot) = (unsafe { resolve_shmid(shmid) }) else {
        errno::set_errno(errno::EINVAL);
        return usize::MAX as *mut u8;
    };
    // Disallow attaching to segments marked for deletion (Linux: still
    // works for already-attached callers, but a new attach gets EIDRM).
    let meta = unsafe { meta_ptr() };
    let m = unsafe { meta.add(slot) };
    if unsafe { (*m).marked_for_rmid } {
        errno::set_errno(errno::EIDRM);
        return usize::MAX as *mut u8;
    }
    unsafe {
        (*m).nattch = (*m).nattch.wrapping_add(1);
    }
    segment_ptr(slot)
}

// ---------------------------------------------------------------------------
// shmdt
// ---------------------------------------------------------------------------

/// `shmdt` — detach shared memory segment.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmdt(shmaddr: *const u8) -> i32 {
    if shmaddr.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let _g = lock();
    let Some(slot) = slot_for_ptr(shmaddr) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let meta = unsafe { meta_ptr() };
    let m = unsafe { meta.add(slot) };
    if !unsafe { (*m).in_use } || unsafe { (*m).nattch } == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    unsafe {
        (*m).nattch = (*m).nattch.wrapping_sub(1);
    }
    // If marked for deletion and refcount dropped to 0, free now.
    if unsafe { (*m).marked_for_rmid } && unsafe { (*m).nattch } == 0 {
        unsafe { free_segment(slot) };
    }
    0
}

// ---------------------------------------------------------------------------
// shmctl
// ---------------------------------------------------------------------------

/// `shmctl` — shared memory control operations.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmctl(shmid: i32, cmd: i32, buf: *mut ShmidDs) -> i32 {
    let _g = lock();
    let Some(slot) = (unsafe { resolve_shmid(shmid) }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let meta = unsafe { meta_ptr() };
    let m = unsafe { meta.add(slot) };
    match cmd {
        IPC_RMID => {
            unsafe {
                (*m).marked_for_rmid = true;
            }
            // Free immediately if no current attachments.
            if unsafe { (*m).nattch } == 0 {
                unsafe { free_segment(slot) };
            }
            0
        }
        IPC_STAT => {
            if buf.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // SAFETY: caller contract.
            unsafe {
                (*buf) = ShmidDs {
                    shm_perm_uid: 0,
                    shm_perm_gid: 0,
                    shm_perm_cuid: 0,
                    shm_perm_cgid: 0,
                    shm_perm_mode: (*m).mode,
                    _pad: 0,
                    shm_segsz: (*m).size,
                    shm_lpid: 0,
                    shm_cpid: 0,
                    shm_nattch: (*m).nattch,
                    shm_atime: 0,
                    shm_dtime: 0,
                    shm_ctime: 0,
                };
            }
            0
        }
        IPC_SET => {
            if buf.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // SAFETY: caller contract.
            let new_mode = unsafe { (*buf).shm_perm_mode };
            unsafe {
                (*m).mode = new_mode;
            }
            0
        }
        SHM_LOCK | SHM_UNLOCK => {
            // Our memory never swaps; accept and no-op.
            0
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Test-only helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
fn test_reset_all() {
    let _g = lock();
    let meta = unsafe { meta_ptr() };
    let mut i: usize = 0;
    while i < MAX_SEGMENTS {
        unsafe {
            (*meta.add(i)).in_use = false;
            (*meta.add(i)).marked_for_rmid = false;
            (*meta.add(i)).key = 0;
            (*meta.add(i)).size = 0;
            (*meta.add(i)).nattch = 0;
            (*meta.add(i)).generation = (*meta.add(i)).generation.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean<F: FnOnce()>(f: F) {
        let _g = TEST_LOCK.lock().unwrap();
        test_reset_all();
        f();
    }

    // -- Constants --

    #[test]
    fn test_constants() {
        assert_eq!(IPC_CREAT, 0o1000);
        assert_eq!(IPC_EXCL, 0o2000);
        assert_eq!(SHM_RDONLY, 0o10000);
        assert_eq!(SHM_LOCK, 11);
        assert_eq!(SHM_UNLOCK, 12);
        assert_eq!(SHMLBA, 16384);
    }

    #[test]
    fn test_shmid_ds_layout() {
        let ds = ShmidDs {
            shm_perm_uid: 1000,
            shm_perm_gid: 1000,
            shm_perm_cuid: 0,
            shm_perm_cgid: 0,
            shm_perm_mode: 0o666,
            _pad: 0,
            shm_segsz: 65536,
            shm_lpid: 42,
            shm_cpid: 1,
            shm_nattch: 2,
            shm_atime: 1000,
            shm_dtime: 0,
            shm_ctime: 500,
        };
        assert_eq!(ds.shm_segsz, 65536);
        assert_eq!(ds.shm_nattch, 2);
        assert_eq!(ds.shm_perm_uid, 1000);
    }

    // -- shmid encoding --

    #[test]
    fn test_shmid_encode_decode_roundtrip() {
        for slot in 0..MAX_SEGMENTS {
            for gen_ in [0u32, 7, 0x7FFF] {
                let id = encode_shmid(slot, gen_);
                let (s, g) = decode_shmid(id).unwrap();
                assert_eq!(s, slot);
                assert_eq!(g, gen_);
            }
        }
    }

    #[test]
    fn test_decode_rejects_zero_and_negative() {
        assert!(decode_shmid(0).is_none());
        assert!(decode_shmid(-1).is_none());
    }

    // -- shmget --

    #[test]
    fn test_shmget_private_creates_new() {
        with_clean(|| {
            let a = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let b = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            assert_ne!(a, -1);
            assert_ne!(b, -1);
            assert_ne!(a, b);
        });
    }

    #[test]
    fn test_shmget_keyed_lookup() {
        with_clean(|| {
            let a = shmget(0x1234, 4096, IPC_CREAT | 0o600);
            let b = shmget(0x1234, 0, 0);
            assert_eq!(a, b);
        });
    }

    #[test]
    fn test_shmget_missing_enoent() {
        with_clean(|| {
            errno::set_errno(0);
            let id = shmget(0x9876, 0, 0);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::ENOENT);
        });
    }

    #[test]
    fn test_shmget_excl_eexist() {
        with_clean(|| {
            let _ = shmget(0xABCD, 4096, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let id = shmget(0xABCD, 4096, IPC_CREAT | IPC_EXCL | 0o600);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EEXIST);
        });
    }

    #[test]
    fn test_shmget_zero_size_private_einval() {
        with_clean(|| {
            errno::set_errno(0);
            let id = shmget(IPC_PRIVATE, 0, IPC_CREAT);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmget_oversized_einval() {
        with_clean(|| {
            errno::set_errno(0);
            let id = shmget(IPC_PRIVATE, SEGMENT_SIZE + 1, IPC_CREAT | 0o600);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmget_lookup_size_too_large_einval() {
        with_clean(|| {
            let _ = shmget(0x55, 4096, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let id = shmget(0x55, 8192, 0);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmget_pool_exhaustion_enospc() {
        with_clean(|| {
            for _ in 0..MAX_SEGMENTS {
                let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
                assert_ne!(id, -1);
            }
            errno::set_errno(0);
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::ENOSPC);
        });
    }

    // -- shmat / shmdt --

    #[test]
    fn test_shmat_returns_aligned_pointer() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let p = shmat(id, core::ptr::null(), 0);
            assert!(!p.is_null());
            assert_ne!(p, usize::MAX as *mut u8);
            assert_eq!(p as usize & (SHMLBA - 1), 0);
        });
    }

    #[test]
    fn test_shmat_then_shmdt() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let p = shmat(id, core::ptr::null(), 0);
            assert_ne!(p, usize::MAX as *mut u8);
            assert_eq!(shmdt(p), 0);
        });
    }

    #[test]
    fn test_shmat_bad_shmid_einval() {
        with_clean(|| {
            errno::set_errno(0);
            let p = shmat(0xCAFE, core::ptr::null(), 0);
            assert_eq!(p, usize::MAX as *mut u8);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmat_after_rmid_when_no_attaches_eidrm_or_einval() {
        // After IPC_RMID with nattch == 0, the slot is freed
        // immediately, so subsequent shmat sees EINVAL (id is stale).
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            assert_eq!(shmctl(id, IPC_RMID, core::ptr::null_mut()), 0);
            errno::set_errno(0);
            let p = shmat(id, core::ptr::null(), 0);
            assert_eq!(p, usize::MAX as *mut u8);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmat_pending_rmid_with_attachments_eidrm() {
        // After IPC_RMID while an attach is live, the slot stays alive
        // but new attaches fail with EIDRM.
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let p = shmat(id, core::ptr::null(), 0);
            assert_eq!(shmctl(id, IPC_RMID, core::ptr::null_mut()), 0);
            errno::set_errno(0);
            let p2 = shmat(id, core::ptr::null(), 0);
            assert_eq!(p2, usize::MAX as *mut u8);
            assert_eq!(errno::get_errno(), errno::EIDRM);
            // The original attach should still be detachable.
            assert_eq!(shmdt(p), 0);
        });
    }

    #[test]
    fn test_shmdt_null_einval() {
        with_clean(|| {
            errno::set_errno(0);
            assert_eq!(shmdt(core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmdt_unknown_ptr_einval() {
        with_clean(|| {
            errno::set_errno(0);
            assert_eq!(shmdt(0xDEADBEEF as *const u8), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmdt_unattached_einval() {
        with_clean(|| {
            // Get a segment but never attach — shmdt should refuse.
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let (slot, _) = decode_shmid(id).unwrap();
            let p = segment_ptr(slot);
            errno::set_errno(0);
            assert_eq!(shmdt(p), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmat_refcount_via_stat() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let _ = shmat(id, core::ptr::null(), 0);
            let _ = shmat(id, core::ptr::null(), 0);
            let _ = shmat(id, core::ptr::null(), 0);
            let mut ds = ShmidDs {
                shm_perm_uid: 0,
                shm_perm_gid: 0,
                shm_perm_cuid: 0,
                shm_perm_cgid: 0,
                shm_perm_mode: 0,
                _pad: 0,
                shm_segsz: 0,
                shm_lpid: 0,
                shm_cpid: 0,
                shm_nattch: 0,
                shm_atime: 0,
                shm_dtime: 0,
                shm_ctime: 0,
            };
            assert_eq!(shmctl(id, IPC_STAT, &raw mut ds), 0);
            assert_eq!(ds.shm_nattch, 3);
        });
    }

    #[test]
    fn test_writes_to_attached_buffer_visible_on_second_attach() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 32, IPC_CREAT | 0o600);
            let p1 = shmat(id, core::ptr::null(), 0);
            assert_ne!(p1, usize::MAX as *mut u8);
            unsafe {
                core::ptr::write(p1, 0xAB);
                core::ptr::write(p1.add(1), 0xCD);
            }
            let p2 = shmat(id, core::ptr::null(), 0);
            assert_eq!(p1, p2); // single-process: same address
            unsafe {
                assert_eq!(core::ptr::read(p2), 0xAB);
                assert_eq!(core::ptr::read(p2.add(1)), 0xCD);
            }
            assert_eq!(shmdt(p1), 0);
            assert_eq!(shmdt(p2), 0);
        });
    }

    // -- shmctl --

    #[test]
    fn test_shmctl_stat_populates() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 8192, IPC_CREAT | 0o644);
            let mut ds = ShmidDs {
                shm_perm_uid: 99,
                shm_perm_gid: 99,
                shm_perm_cuid: 99,
                shm_perm_cgid: 99,
                shm_perm_mode: 0,
                _pad: 0,
                shm_segsz: 0,
                shm_lpid: 0,
                shm_cpid: 0,
                shm_nattch: 0,
                shm_atime: 0,
                shm_dtime: 0,
                shm_ctime: 0,
            };
            assert_eq!(shmctl(id, IPC_STAT, &raw mut ds), 0);
            assert_eq!(ds.shm_segsz, 8192);
            assert_eq!(ds.shm_perm_mode, 0o644);
            assert_eq!(ds.shm_nattch, 0);
        });
    }

    #[test]
    fn test_shmctl_set_updates_mode() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let mut ds = ShmidDs {
                shm_perm_uid: 0,
                shm_perm_gid: 0,
                shm_perm_cuid: 0,
                shm_perm_cgid: 0,
                shm_perm_mode: 0o744,
                _pad: 0,
                shm_segsz: 0,
                shm_lpid: 0,
                shm_cpid: 0,
                shm_nattch: 0,
                shm_atime: 0,
                shm_dtime: 0,
                shm_ctime: 0,
            };
            assert_eq!(shmctl(id, IPC_SET, &raw mut ds), 0);
            let mut out = ds;
            out.shm_perm_mode = 0;
            assert_eq!(shmctl(id, IPC_STAT, &raw mut out), 0);
            assert_eq!(out.shm_perm_mode, 0o744);
        });
    }

    #[test]
    fn test_shmctl_rmid_no_attach_frees_immediately() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            assert_eq!(shmctl(id, IPC_RMID, core::ptr::null_mut()), 0);
            // Slot should be freed — STAT now fails.
            errno::set_errno(0);
            assert_eq!(shmctl(id, IPC_STAT, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmctl_rmid_with_attach_defers_free() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            let p = shmat(id, core::ptr::null(), 0);
            assert_ne!(p, usize::MAX as *mut u8);
            // RMID should succeed but slot lives until detach.
            assert_eq!(shmctl(id, IPC_RMID, core::ptr::null_mut()), 0);
            // The id still resolves (slot in_use, generation unchanged).
            let mut ds = ShmidDs {
                shm_perm_uid: 0,
                shm_perm_gid: 0,
                shm_perm_cuid: 0,
                shm_perm_cgid: 0,
                shm_perm_mode: 0,
                _pad: 0,
                shm_segsz: 0,
                shm_lpid: 0,
                shm_cpid: 0,
                shm_nattch: 0,
                shm_atime: 0,
                shm_dtime: 0,
                shm_ctime: 0,
            };
            assert_eq!(shmctl(id, IPC_STAT, &raw mut ds), 0);
            assert_eq!(ds.shm_nattch, 1);
            // Detach — now slot must be freed.
            assert_eq!(shmdt(p), 0);
            errno::set_errno(0);
            assert_eq!(shmctl(id, IPC_STAT, &raw mut ds), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmctl_rmid_then_shmget_creates_new() {
        // After RMID, the key should be re-usable for a brand-new segment.
        with_clean(|| {
            let key = 0x42;
            let id1 = shmget(key, 4096, IPC_CREAT | 0o600);
            assert_eq!(shmctl(id1, IPC_RMID, core::ptr::null_mut()), 0);
            let id2 = shmget(key, 4096, IPC_CREAT | 0o600);
            assert_ne!(id2, -1);
            assert_ne!(id1, id2);
        });
    }

    #[test]
    fn test_shmctl_lock_unlock_noop() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            assert_eq!(shmctl(id, SHM_LOCK, core::ptr::null_mut()), 0);
            assert_eq!(shmctl(id, SHM_UNLOCK, core::ptr::null_mut()), 0);
        });
    }

    #[test]
    fn test_shmctl_bad_cmd_einval() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(shmctl(id, 9999, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_shmctl_stat_null_buf_efault() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(shmctl(id, IPC_STAT, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        });
    }

    #[test]
    fn test_shmctl_bad_shmid_einval() {
        with_clean(|| {
            errno::set_errno(0);
            assert_eq!(shmctl(0xDEAD, IPC_STAT, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    // -- workflow --

    #[test]
    fn test_full_workflow() {
        with_clean(|| {
            let id = shmget(IPC_PRIVATE, 1024, IPC_CREAT | 0o600);
            assert_ne!(id, -1);
            let p = shmat(id, core::ptr::null(), 0);
            assert_ne!(p, usize::MAX as *mut u8);
            unsafe {
                core::ptr::write(p, 0x55);
                assert_eq!(core::ptr::read(p), 0x55);
            }
            assert_eq!(shmdt(p), 0);
            assert_eq!(shmctl(id, IPC_RMID, core::ptr::null_mut()), 0);
        });
    }
}
