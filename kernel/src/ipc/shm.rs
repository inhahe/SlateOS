//! Shared memory — direct physical page sharing between tasks.
//!
//! Shared memory is the fastest IPC mechanism: after a one-time kernel
//! setup, communication is just memory reads and writes — no kernel
//! involvement, no copying.
//!
//! ## Design
//!
//! A shared memory region is a set of physical frames owned by the
//! kernel.  Multiple tasks (or, in the future, processes) can map the
//! same physical frames into their address spaces.  After mapping,
//! reads and writes go directly through the hardware page tables.
//!
//! ## Synchronization
//!
//! The kernel provides the primitives; userspace builds abstractions:
//!
//! - **Futexes** (already implemented): sleep/wake on contention.
//! - **Lock-free ring buffers**: producer/consumer over shared memory
//!   (10–50 ns latency — a userspace library concern, not kernel).
//! - **Seqlocks**: one writer + many readers, readers never block the
//!   writer (also a userspace library concern).
//!
//! ## Current Limitations
//!
//! Without per-process address spaces, shared memory is mapped into
//! the kernel's single address space.  The kernel-side infrastructure
//! (region creation, physical page management, handle tracking) is
//! fully implemented and will extend naturally to multi-process mapping
//! when process contexts are added (Phase 1.6).
//!
//! ## Lock Ordering
//!
//! `SHM_TABLE` does not call into the scheduler, so no ordering
//! constraint with `SCHED`.  Frame allocation uses the frame allocator
//! lock, which is below `SHM_TABLE`.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::mm::page_table;
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum shared memory region size: 256 MiB.
///
/// Generous limit for early development.  Can be raised later with
/// capability-gated access for larger regions.
const MAX_SHM_SIZE: usize = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Region ID and Handle
// ---------------------------------------------------------------------------

/// Unique identifier for a shared memory region.
type ShmId = u64;

/// Counter for generating unique region IDs.
static NEXT_SHM_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_shm_id() -> ShmId {
    NEXT_SHM_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to a shared memory region.
///
/// Handles are used by the syscall layer to refer to regions.
/// Multiple handles can point to the same region (reference counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShmHandle(u64);

impl ShmHandle {
    /// Reconstruct a handle from its raw u64 representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw u64 representation.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Extract the region ID (the handle IS the region ID for now).
    fn region_id(self) -> ShmId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Region internals
// ---------------------------------------------------------------------------

/// A shared memory region: a contiguous set of physical frames.
struct ShmRegion {
    /// Physical frames backing this region, in order.
    frames: Vec<PhysFrame>,
    /// Total size in bytes (always a multiple of `FRAME_SIZE`).
    size: usize,
    /// Reference count — how many handles (or mappings) exist.
    /// When this reaches 0, the region is destroyed and frames freed.
    ref_count: usize,
    /// PIDs authorized to perform userspace operations (`SYS_SHM_MAP`,
    /// `SYS_SHM_SIZE`, `SYS_SHM_CLOSE`) on this region.
    ///
    /// A region is created by the kernel (kernel context has no caller PID
    /// and is the TCB — it may always operate on any region). To let a
    /// *specific* userspace process touch a region — e.g. handing the
    /// `net.stack` daemon the kernel-created TCP ring so it can
    /// `SYS_SHM_MAP` it — that process's PID must be added here via
    /// [`authorize`]. Without this list, any process holding a raw handle
    /// value (the handle *is* the small monotonic region ID, trivially
    /// guessable) could map another process's region — see the
    /// D-SHM-MAP-NOCAP issue.
    authorized: Vec<u64>,
}

// ---------------------------------------------------------------------------
// Global region table
// ---------------------------------------------------------------------------

/// Global table of all shared memory regions.
///
/// Protected by a single spinlock.
static SHM_TABLE: Mutex<BTreeMap<ShmId, ShmRegion>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new shared memory region of at least `size` bytes.
///
/// The actual size is rounded up to the nearest frame boundary
/// (16 KiB).  Returns a handle to the region.
///
/// # Errors
///
/// - `InvalidArgument` — `size` is 0.
/// - `InvalidArgument` — `size` exceeds `MAX_SHM_SIZE`.
/// - `OutOfMemory` — not enough physical memory.
#[allow(clippy::arithmetic_side_effects)]
pub fn create(size: usize) -> KernelResult<ShmHandle> {
    if size == 0 {
        return Err(KernelError::InvalidArgument);
    }
    if size > MAX_SHM_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    // Round up to frame boundary.
    let num_frames = size.div_ceil(FRAME_SIZE);
    let actual_size = num_frames * FRAME_SIZE;

    // Allocate physical frames.
    let mut frames = Vec::with_capacity(num_frames);
    for _ in 0..num_frames {
        match frame::alloc_frame() {
            Ok(f) => frames.push(f),
            Err(e) => {
                // Free already-allocated frames on failure.
                for f in frames {
                    // SAFETY: These frames were just allocated and have
                    // no other references.
                    unsafe {
                        let _ = frame::free_frame(f);
                    }
                }
                return Err(e);
            }
        }
    }

    // Zero the frames for security (don't leak previous contents).
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    for f in &frames {
        // SAFETY: f.addr() is a valid physical frame we just allocated.
        // hhdm + phys gives the kernel virtual address.
        unsafe {
            let virt = (f.addr() + hhdm) as *mut u8;
            core::ptr::write_bytes(virt, 0, FRAME_SIZE);
        }
    }

    let id = alloc_shm_id();
    let region = ShmRegion {
        frames,
        size: actual_size,
        ref_count: 1,
        authorized: Vec::new(),
    };

    let mut table = SHM_TABLE.lock();
    table.insert(id, region);

    super::stats::shm_created(actual_size as u64);
    Ok(ShmHandle(id))
}

/// Get the size of a shared memory region in bytes.
pub fn size(handle: ShmHandle) -> KernelResult<usize> {
    let table = SHM_TABLE.lock();
    let region = table
        .get(&handle.region_id())
        .ok_or(KernelError::InvalidHandle)?;
    Ok(region.size)
}

/// Get a pointer to the shared memory region's data (kernel-mode).
///
/// Returns the kernel virtual address of the first byte of the
/// region.  The region is backed by contiguous-in-virtual-space
/// frames via the HHDM.
///
/// # Safety contract
///
/// The returned pointer is valid as long as the region exists (i.e.,
/// the handle has not been closed).  When userspace is implemented,
/// this will be replaced by per-process virtual mapping.
///
/// **Note**: If the region has multiple non-contiguous physical
/// frames, this returns the address of the *first* frame only.
/// For regions larger than one frame, use [`frame_addrs`] to get
/// each frame's address.
///
/// For the self-test (single-frame regions), this is sufficient.
pub fn kernel_addr(handle: ShmHandle) -> KernelResult<*mut u8> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let table = SHM_TABLE.lock();
    let region = table
        .get(&handle.region_id())
        .ok_or(KernelError::InvalidHandle)?;

    let first_frame = region
        .frames
        .first()
        .ok_or(KernelError::InternalError)?;

    // SAFETY: first_frame.addr() is a valid physical address within
    // managed memory.  The HHDM maps all physical memory.
    #[allow(clippy::arithmetic_side_effects)]
    let virt = (first_frame.addr() + hhdm) as *mut u8;
    Ok(virt)
}

/// Get the physical frame addresses backing this region.
///
/// Returns a list of physical addresses, one per frame, in order.
/// Used by `SYS_SHM_MAP` (`sys_shm_map`) to map the region's frames into a
/// process's page table (each frame's refcount is bumped at map time so the
/// mapping outlives the SHM handle).
pub fn frame_addrs(handle: ShmHandle) -> KernelResult<Vec<u64>> {
    let table = SHM_TABLE.lock();
    let region = table
        .get(&handle.region_id())
        .ok_or(KernelError::InvalidHandle)?;
    Ok(region.frames.iter().map(|f| f.addr()).collect())
}

/// Close a shared memory handle.
///
/// Decrements the reference count.  When the count reaches zero,
/// the region's physical frames are freed.
pub fn close(handle: ShmHandle) {
    let mut table = SHM_TABLE.lock();
    let should_remove = if let Some(region) = table.get_mut(&handle.region_id()) {
        #[allow(clippy::arithmetic_side_effects)]
        {
            region.ref_count = region.ref_count.saturating_sub(1);
        }
        region.ref_count == 0
    } else {
        false
    };

    if should_remove
        && let Some(region) = table.remove(&handle.region_id())
    {
        super::stats::shm_destroyed(region.size as u64);
        // Free all physical frames.
        for f in region.frames {
            // SAFETY: We're the last handle — no other references
            // to these frames exist.
            unsafe {
                let _ = frame::free_frame(f);
            }
        }
    }
}

/// Grant a process the right to perform userspace SHM operations
/// (`SYS_SHM_MAP`/`SIZE`/`CLOSE`) on a region.
///
/// Used at each kernel→daemon SHM handoff: the kernel creates a region in
/// its own (TCB) context, then authorizes the specific daemon PID that will
/// map it. Idempotent — re-authorizing an already-listed PID is a no-op.
///
/// Returns `InvalidHandle` if the region does not exist.
pub fn authorize(handle: ShmHandle, pid: u64) -> KernelResult<()> {
    let mut table = SHM_TABLE.lock();
    let region = table
        .get_mut(&handle.region_id())
        .ok_or(KernelError::InvalidHandle)?;
    if !region.authorized.contains(&pid) {
        region.authorized.push(pid);
    }
    Ok(())
}

/// Check whether `pid` is authorized to operate on a region.
///
/// Returns `false` if the region does not exist or the PID is not in the
/// region's authorized list. Kernel-context callers (no PID) do not use
/// this — they are the TCB and may always operate on any region.
#[must_use]
pub fn is_authorized(handle: ShmHandle, pid: u64) -> bool {
    let table = SHM_TABLE.lock();
    table
        .get(&handle.region_id())
        .is_some_and(|region| region.authorized.contains(&pid))
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run shared memory self-tests.
///
/// Tests:
/// 1. Create a single-frame region, write/read through kernel address.
/// 2. Region is zeroed on creation.
/// 3. Close frees physical frames (alloc count returns to baseline).
/// 4. Invalid arguments rejected.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[shm] Running shared memory self-test...");

    test_create_write_read()?;
    test_zeroed_on_create()?;
    test_close_frees_frames()?;
    test_invalid_args()?;

    serial_println!("[shm] Shared memory self-test PASSED");
    Ok(())
}

/// Test 1: create, write, and read back through kernel address.
fn test_create_write_read() -> KernelResult<()> {
    let handle = create(64)?; // Request 64 bytes (rounds up to 1 frame).

    let sz = size(handle)?;
    if sz != FRAME_SIZE {
        serial_println!("[shm]   FAIL: size {} expected {}", sz, FRAME_SIZE);
        close(handle);
        return Err(KernelError::InternalError);
    }

    let ptr = kernel_addr(handle)?;

    // Write a pattern.
    // SAFETY: ptr is valid for FRAME_SIZE bytes (we just created it).
    unsafe {
        *ptr = 0xAB;
        *ptr.add(1) = 0xCD;
        *ptr.add(FRAME_SIZE - 1) = 0xEF;
    }

    // Read back.
    // SAFETY: Same pointer, just allocated.
    let (a, b, c) = unsafe {
        (*ptr, *ptr.add(1), *ptr.add(FRAME_SIZE - 1))
    };

    if a != 0xAB || b != 0xCD || c != 0xEF {
        serial_println!(
            "[shm]   FAIL: read back {:#x} {:#x} {:#x}",
            a, b, c
        );
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    serial_println!("[shm]   Create + write/read: OK");
    Ok(())
}

/// Test 2: region is zeroed on creation.
fn test_zeroed_on_create() -> KernelResult<()> {
    let handle = create(FRAME_SIZE)?;
    let ptr = kernel_addr(handle)?;

    // Check several offsets are zero.
    // SAFETY: ptr is valid for FRAME_SIZE bytes.
    let all_zero = unsafe {
        *ptr == 0
            && *ptr.add(1) == 0
            && *ptr.add(FRAME_SIZE / 2) == 0
            && *ptr.add(FRAME_SIZE - 1) == 0
    };

    if !all_zero {
        serial_println!("[shm]   FAIL: region not zeroed");
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    serial_println!("[shm]   Zeroed on create: OK");
    Ok(())
}

/// Test 3: closing frees frames (free count returns to baseline).
fn test_close_frees_frames() -> KernelResult<()> {
    let stats_before = frame::stats().ok_or(KernelError::NotSupported)?;

    let handle = create(FRAME_SIZE)?;
    let stats_during = frame::stats().ok_or(KernelError::NotSupported)?;

    // Should have one fewer free frame.
    if stats_during.free_frames >= stats_before.free_frames {
        serial_println!(
            "[shm]   FAIL: free frames didn't decrease ({} -> {})",
            stats_before.free_frames, stats_during.free_frames
        );
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    let stats_after = frame::stats().ok_or(KernelError::NotSupported)?;

    // Free frames should be back to (approximately) the baseline.
    // The buddy allocator may coalesce, so exact equality isn't
    // guaranteed, but it should not be less than before.
    if stats_after.free_frames < stats_before.free_frames {
        serial_println!(
            "[shm]   FAIL: free frames not restored ({} -> {})",
            stats_before.free_frames, stats_after.free_frames
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[shm]   Close frees frames: OK");
    Ok(())
}

/// Test 4: invalid arguments rejected.
fn test_invalid_args() -> KernelResult<()> {
    // Zero size.
    match create(0) {
        Err(KernelError::InvalidArgument) => {}
        other => {
            serial_println!("[shm]   FAIL: create(0) returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Exceeds maximum.
    #[allow(clippy::arithmetic_side_effects)]
    match create(MAX_SHM_SIZE + 1) {
        Err(KernelError::InvalidArgument) => {}
        other => {
            serial_println!(
                "[shm]   FAIL: create(MAX+1) returned {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[shm]   Invalid args rejected: OK");
    Ok(())
}
