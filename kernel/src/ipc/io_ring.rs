//! io_uring-style submission queue for batch I/O.
//!
//! Allows userspace to submit multiple I/O operations in a single syscall
//! and retrieve completions in bulk.  This reduces per-operation syscall
//! overhead for high-throughput workloads (file I/O, network servers, IPC
//! message pumps).
//!
//! ## Design
//!
//! Each io_ring has two ring buffers:
//!
//! - **Submission Queue (SQ)**: userspace writes [`SqEntry`] descriptors,
//!   kernel reads and processes them.
//! - **Completion Queue (CQ)**: kernel writes [`CqEntry`] results,
//!   userspace reads them.
//!
//! Both rings are backed by kernel-allocated memory that is mapped into
//! the calling process's address space.  Head/tail pointers use atomic
//! operations for lock-free coordination.
//!
//! ## Supported Opcodes
//!
//! Each SQ entry specifies an opcode (what operation to perform) plus
//! arguments.  The initial opcode set covers the most common operations:
//!
//! | Opcode | Operation | fd/handle | addr | len |
//! |--------|-----------|-----------|------|-----|
//! | NOP    | No-op (test) | — | — | — |
//! | CONSOLE_WRITE | Write to console | — | buf ptr | buf len |
//! | CHANNEL_SEND  | Send on channel | ch handle | msg ptr | msg len |
//! | CHANNEL_RECV  | Recv from channel | ch handle | buf ptr | buf cap |
//! | PIPE_WRITE    | Write to pipe | pipe handle | buf ptr | buf len |
//! | PIPE_READ     | Read from pipe | pipe handle | buf ptr | buf cap |
//! | FS_READ       | Read file | — | path ptr | path len |
//! | FS_WRITE      | Write file | — | path ptr | path len |
//!
//! ## Syscall Interface
//!
//! - `SYS_IO_RING_SETUP(sq_entries, cq_entries)` → (ring_handle, ring_addr)
//!   Allocates ring memory, maps into user space, returns handle and
//!   the virtual address of the [`IoRingHeader`].
//!
//! - `SYS_IO_RING_ENTER(ring_handle, to_submit, min_complete, flags)` → completed
//!   Processes up to `to_submit` entries from the SQ.  If `min_complete`
//!   is > 0, blocks until that many CQEs are available.
//!
//! - `SYS_IO_RING_DESTROY(ring_handle)` → 0
//!   Unmaps ring memory and frees resources.
//!
//! ## Performance Target
//!
//! < 200ns per SQE submission (Linux io_uring: 100-200ns).  Achieved by:
//! - No memory allocation in the submission path.
//! - O(1) ring buffer operations.
//! - Direct dispatch to existing kernel handlers.
//!
//! ## References
//!
//! - Linux io_uring (Jens Axboe, 2019)
//! - Design spec: "io_uring-style submission queue" in design desicions.txt

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Ring entry structures
// ---------------------------------------------------------------------------

/// Submission Queue Entry — describes one I/O operation.
///
/// 64 bytes, matching Linux io_uring SQE size for familiarity.
/// Must be repr(C) for user-kernel shared memory layout.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SqEntry {
    /// Operation code — see `IO_OP_*` constants.
    pub opcode: u8,
    /// Per-entry flags (reserved, must be 0).
    pub flags: u8,
    /// Padding.
    pub _pad0: [u8; 2],
    /// Reserved for future priority/personality features.
    pub _pad1: u32,
    /// Arbitrary user data — returned in the corresponding CQE.
    /// Allows the application to dispatch results without lookup tables.
    pub user_data: u64,
    /// Target handle (channel, pipe, etc.).  Meaning depends on opcode.
    pub handle: u64,
    /// Buffer address in userspace.  Meaning depends on opcode.
    pub addr: u64,
    /// Buffer length / capacity.  Meaning depends on opcode.
    pub len: u32,
    /// Additional argument (e.g., file offset).
    pub _pad2: u32,
    /// Additional 64-bit argument (e.g., path length for FS ops,
    /// or data pointer for FS write).
    pub arg1: u64,
    /// Another 64-bit argument for operations that need more params.
    pub arg2: u64,
}

/// Completion Queue Entry — result of one I/O operation.
///
/// 16 bytes, matching Linux io_uring CQE size.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CqEntry {
    /// User data from the corresponding SQE.
    pub user_data: u64,
    /// Result value.  >= 0 on success (operation-specific), < 0 on
    /// error (negative [`KernelError`] code).
    pub result: i64,
}

/// Shared header at the start of the ring buffer memory region.
///
/// Both userspace and kernel read/write these fields using atomic
/// operations.  The header is followed by the SQ entries, then the
/// CQ entries.
///
/// Memory layout:
/// ```text
/// [IoRingHeader]                    offset 0, 64 bytes
/// [SqEntry; sq_entries]             offset 64
/// [CqEntry; cq_entries]             offset 64 + sq_entries * 64
/// ```
#[repr(C)]
pub struct IoRingHeader {
    /// SQ head — consumer index (kernel reads, advances after processing).
    pub sq_head: AtomicU32,
    /// SQ tail — producer index (user writes, advances after enqueuing).
    pub sq_tail: AtomicU32,
    /// SQ ring mask (`sq_entries - 1`).  Power-of-two ring size.
    pub sq_mask: u32,
    /// Number of SQ entries.
    pub sq_entries: u32,

    /// CQ head — consumer index (user reads, advances after consuming).
    pub cq_head: AtomicU32,
    /// CQ tail — producer index (kernel writes, advances after posting).
    pub cq_tail: AtomicU32,
    /// CQ ring mask (`cq_entries - 1`).
    pub cq_mask: u32,
    /// Number of CQ entries.
    pub cq_entries: u32,

    /// Ring state flags.
    pub flags: AtomicU32,
    /// Padding to 64 bytes (cache line alignment).
    pub _pad: [u32; 7],
}

// ---------------------------------------------------------------------------
// Opcodes
// ---------------------------------------------------------------------------

/// No-op.  Used for testing ring buffer correctness.
pub const IO_OP_NOP: u8 = 0;

/// Write bytes to the framebuffer console.
/// `addr` = buffer pointer, `len` = byte count.
pub const IO_OP_CONSOLE_WRITE: u8 = 1;

/// Send a message on an IPC channel.
/// `handle` = channel endpoint, `addr` = message pointer, `len` = message length.
pub const IO_OP_CHANNEL_SEND: u8 = 2;

/// Receive a message from an IPC channel (non-blocking try_recv).
/// `handle` = channel endpoint, `addr` = buffer pointer, `len` = buffer capacity.
pub const IO_OP_CHANNEL_RECV: u8 = 3;

/// Write bytes to a pipe.
/// `handle` = write-end pipe handle, `addr` = buffer pointer, `len` = byte count.
pub const IO_OP_PIPE_WRITE: u8 = 4;

/// Read bytes from a pipe (non-blocking).
/// `handle` = read-end pipe handle, `addr` = buffer pointer, `len` = buffer capacity.
pub const IO_OP_PIPE_READ: u8 = 5;

/// Read an entire file.
/// `addr` = path pointer, `len` = path length, `arg1` = dest buffer pointer,
/// `arg2` = dest buffer capacity.
pub const IO_OP_FS_READ: u8 = 6;

/// Write data to a file (create/overwrite).
/// `addr` = path pointer, `len` = path length, `arg1` = data pointer,
/// `arg2` = data length.
pub const IO_OP_FS_WRITE: u8 = 7;

/// Read from a file handle at the current offset.
/// `handle` = file handle, `addr` = buffer pointer, `len` = buffer capacity.
/// Returns: bytes read (0 = EOF).
pub const IO_OP_FH_READ: u8 = 8;

/// Write to a file handle at the current offset.
/// `handle` = file handle, `addr` = data pointer, `len` = data length.
/// Returns: bytes written.
pub const IO_OP_FH_WRITE: u8 = 9;

/// Read from a file handle at a specific offset (positional read).
/// `handle` = file handle, `addr` = buffer pointer, `len` = buffer capacity,
/// `arg1` = file offset.  Does NOT move the handle's cursor.
/// Returns: bytes read (0 = EOF).
pub const IO_OP_FH_PREAD: u8 = 10;

/// Write to a file handle at a specific offset (positional write).
/// `handle` = file handle, `addr` = data pointer, `len` = data length,
/// `arg1` = file offset.  Does NOT move the handle's cursor.
/// Returns: bytes written.
pub const IO_OP_FH_PWRITE: u8 = 11;

/// Signal an eventfd (add value to counter).
/// `handle` = eventfd handle, `arg1` = value to add.
/// Returns: 0 on success.
pub const IO_OP_EVENTFD_SIGNAL: u8 = 12;

/// Signal a semaphore (increment count).
/// `handle` = semaphore handle, `arg1` = count to add.
/// Returns: 0 on success.
pub const IO_OP_SEM_SIGNAL: u8 = 13;

/// Timeout — completes after a specified duration.
/// `arg1` = timeout in nanoseconds (0 = immediate).
/// Returns: 0 on timeout (normal), or -ECANCELLED if cancelled.
///
/// This is a synchronous sleep (blocks the io_ring processing thread
/// for the duration).  Useful for:
/// - Rate limiting batch submissions
/// - Adding delays between sequential linked SQEs
/// - Implementing periodic timers via repeated submission
pub const IO_OP_TIMEOUT: u8 = 14;

/// Cancel a pending timeout (by user_data match).
/// `arg1` = user_data of the timeout SQE to cancel.
/// Returns: 0 if found and cancelled, -ENOENT if not found.
///
/// Note: Since the current io_ring is synchronous (processes SQEs
/// in order), this is only useful for linked SQEs or future async
/// mode.  Included for API completeness with Linux io_uring.
pub const IO_OP_TIMEOUT_CANCEL: u8 = 15;

/// Service connect — connect to a named service.
/// `addr` = pointer to service name bytes.
/// `len` = service name length.
/// Returns: raw channel handle (>= 0) on success.
///
/// Equivalent to the SYS_SERVICE_CONNECT syscall but submittable
/// via io_uring for batch service discovery.
pub const IO_OP_SERVICE_CONNECT: u8 = 16;

/// Sleep (nanosecond-precision delay using hrtimer).
/// `arg1` = duration in nanoseconds.
/// Returns: 0 on completion, -EINTR if interrupted.
///
/// Unlike IO_OP_TIMEOUT, this always sleeps the full duration
/// (no cancel mechanism).  Used for precise delays in I/O sequences.
pub const IO_OP_SLEEP: u8 = 17;

// ---------------------------------------------------------------------------
// Ring management
// ---------------------------------------------------------------------------

/// Maximum number of concurrent io_rings.
const MAX_RINGS: usize = 64;

/// Maximum SQ/CQ entries per ring (must be power of 2).
const MAX_RING_ENTRIES: u32 = 256;

/// Minimum SQ/CQ entries per ring.
const MIN_RING_ENTRIES: u32 = 4;

/// An io_ring instance.
///
/// Stores the kernel-side view of the ring buffers.  The actual data
/// lives in separately allocated memory (accessible via HHDM pointers).
struct IoRing {
    /// Ring handle (unique ID).
    handle: u64,
    /// Pointer to the IoRingHeader in kernel virtual memory (HHDM).
    header_ptr: *mut IoRingHeader,
    /// Pointer to the start of the SQ entry array.
    sq_ptr: *mut SqEntry,
    /// Pointer to the start of the CQ entry array.
    cq_ptr: *mut CqEntry,
    /// Number of SQ entries.
    sq_entries: u32,
    /// Number of CQ entries.
    cq_entries: u32,
    /// Owning task ID (for cleanup).
    owner_task: u64,
    /// Owning **process**, or `None` for a ring created by a kernel thread.
    ///
    /// The unit of ownership is the process, not the task: the SQ/CQ are a
    /// shared mapping in one address space, so every thread of that process is
    /// a legitimate driver of the ring, and no thread outside it is.  Handles
    /// come from a small global counter and are therefore trivially guessable,
    /// so without this check any process could name another's ring.
    owner_process: Option<u64>,
    /// Set while [`enter`] is processing this ring's SQ with the table lock
    /// released.
    ///
    /// Serves two purposes: it makes `enter` non-reentrant per ring (two
    /// threads of the owning process would otherwise both consume from the same
    /// SQ head), and it blocks [`destroy`], which would otherwise free the
    /// frames the processing loop is still reading SQEs out of.
    busy: bool,
    /// Physical frame addresses backing this ring (for cleanup).
    phys_frames: alloc::vec::Vec<u64>,
    /// Completion port to notify when CQEs are posted (0 = none).
    cp_handle: u64,
    /// Base of the owning process's mapping of these frames, or 0 if the ring
    /// has no user mapping (a kernel-thread ring).
    ///
    /// Recorded so [`destroy`] can tear the mapping down; see
    /// [`attach_user_mapping`].
    user_base: u64,
    /// Byte length of the user mapping at `user_base`.
    user_bytes: u64,
}

/// The process that owns rings created by, and usable from, the current task.
///
/// `None` means "a kernel thread with no owning process" — a legitimate owner
/// value that only matches other kernel threads, which is what the in-kernel
/// self-tests and benchmarks need.
fn current_owner_process() -> Option<u64> {
    crate::proc::thread::owner_process(crate::sched::current_task_id())
}

// SAFETY: IoRing's raw pointers point to HHDM-mapped memory that is
// only accessed under the RING_TABLE lock.  We never dereference them
// from ISR context or across CPUs without synchronization.
unsafe impl Send for IoRing {}

/// Global table of active io_rings.
static RING_TABLE: Mutex<alloc::collections::BTreeMap<u64, IoRing>> =
    Mutex::new(alloc::collections::BTreeMap::new());

/// Counter for ring handle generation.
static NEXT_RING_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new io_ring.
///
/// Allocates memory for the ring header, SQ, and CQ.  Returns the
/// ring handle and the kernel virtual address of the header (HHDM).
///
/// The caller is responsible for mapping these frames into the user
/// address space if needed.
///
/// # Arguments
///
/// - `sq_entries` — number of submission queue entries (rounded up to
///   power of 2, clamped to [`MIN_RING_ENTRIES`]..=[`MAX_RING_ENTRIES`]).
/// - `cq_entries` — number of completion queue entries (same clamping).
///
/// # Returns
///
/// `(ring_handle, header_hhdm_addr, phys_frames)` on success.
///
/// **The returned address is an HHDM (kernel direct-map) address and must not
/// be handed to userspace.**  It is here for in-kernel callers (the self-tests
/// and benchmarks) that drive a ring without an address space of their own.
/// A syscall must instead map `phys_frames` into the calling process and
/// register the result with [`attach_user_mapping`], returning *that* address:
/// leaking an HHDM pointer to ring 3 discloses the direct-map base and thus
/// defeats kernel address-space randomisation for every subsequent attack.
pub fn setup(sq_entries: u32, cq_entries: u32) -> KernelResult<(u64, u64, alloc::vec::Vec<u64>)> {
    use crate::mm::frame::{self, FRAME_SIZE};
    use crate::mm::page_table;

    // Clamp and round up to power of 2.
    let sq = sq_entries
        .clamp(MIN_RING_ENTRIES, MAX_RING_ENTRIES)
        .next_power_of_two();
    let cq = cq_entries
        .clamp(MIN_RING_ENTRIES, MAX_RING_ENTRIES)
        .next_power_of_two();

    // Check ring table capacity.
    {
        let table = RING_TABLE.lock();
        if table.len() >= MAX_RINGS {
            return Err(KernelError::OutOfMemory);
        }
    }

    // Calculate total memory needed.
    let header_size = core::mem::size_of::<IoRingHeader>();
    let sq_size = (sq as usize) * core::mem::size_of::<SqEntry>();
    let cq_size = (cq as usize) * core::mem::size_of::<CqEntry>();
    #[allow(clippy::arithmetic_side_effects)]
    let total_size = header_size + sq_size + cq_size;
    #[allow(clippy::arithmetic_side_effects)]
    let frames_needed = total_size.div_ceil(FRAME_SIZE);

    // Allocate physical frames.
    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    let mut phys_frames = alloc::vec::Vec::new();
    for _ in 0..frames_needed {
        let frame = frame::alloc_frame()?;
        let virt = frame.to_virt(hhdm);
        // Zero the frame.
        // SAFETY: freshly allocated, exclusively owned.
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE);
        }
        phys_frames.push(frame.addr());
    }

    // The ring data starts at the HHDM virtual address of the first frame.
    #[allow(clippy::arithmetic_side_effects)]
    let base_virt = phys_frames[0] + hhdm;

    // Set up the header.
    let header_ptr = base_virt as *mut IoRingHeader;
    // SAFETY: We just allocated and zeroed this memory.
    unsafe {
        (*header_ptr).sq_mask = sq.wrapping_sub(1);
        (*header_ptr).sq_entries = sq;
        (*header_ptr).cq_mask = cq.wrapping_sub(1);
        (*header_ptr).cq_entries = cq;
        // Head and tail start at 0 (already zeroed).
    }

    // Compute pointers to the SQ and CQ arrays.
    #[allow(clippy::arithmetic_side_effects)]
    let sq_ptr = (base_virt + header_size as u64) as *mut SqEntry;
    #[allow(clippy::arithmetic_side_effects)]
    let cq_ptr = (base_virt + header_size as u64 + sq_size as u64) as *mut CqEntry;

    let handle = NEXT_RING_ID.fetch_add(1, Ordering::Relaxed);
    let owner = crate::sched::current_task_id();

    let ring = IoRing {
        handle,
        header_ptr,
        sq_ptr,
        cq_ptr,
        sq_entries: sq,
        cq_entries: cq,
        owner_task: owner,
        owner_process: current_owner_process(),
        busy: false,
        phys_frames: phys_frames.clone(),
        cp_handle: 0,
        user_base: 0,
        user_bytes: 0,
    };

    {
        let mut table = RING_TABLE.lock();
        table.insert(handle, ring);
    }

    serial_println!(
        "[io_ring] Created ring {} (sq={}, cq={}, frames={})",
        handle,
        sq,
        cq,
        frames_needed
    );

    Ok((handle, base_virt, phys_frames))
}

/// Process submissions on an io_ring.
///
/// Reads up to `to_submit` SQEs from the submission queue, executes
/// each operation, and posts results to the completion queue.
///
/// Returns the number of SQEs processed.
///
/// # Arguments
///
/// - `ring_handle` — the ring to process.
/// - `to_submit` — maximum number of SQEs to process (0 = drain all available).
pub fn enter(ring_handle: u64, to_submit: u32) -> KernelResult<u32> {
    // Claim the ring under the table lock, then release it for the duration of
    // processing.  Executing SQEs with `RING_TABLE` held was a system-wide
    // hazard: `IO_OP_SLEEP` parks for up to 60 seconds, `IO_OP_TIMEOUT` for up
    // to 10, and every buffer-moving opcode can block on a demand-paging fault
    // or on filesystem I/O — all with a global spinlock held, which also
    // deadlocks any other thread touching *any* ring.  The `busy` flag keeps
    // the exclusion the lock used to provide (one entrant per ring, and no
    // `destroy` freeing the frames underneath us) without holding it.
    let (header_ptr, sq_ptr, cq_ptr) = {
        let mut table = RING_TABLE.lock();
        let ring = table
            .get_mut(&ring_handle)
            .ok_or(KernelError::InvalidArgument)?;
        if ring.owner_process != current_owner_process() {
            return Err(KernelError::PermissionDenied);
        }
        if ring.busy {
            return Err(KernelError::WouldBlock);
        }
        ring.busy = true;
        (ring.header_ptr, ring.sq_ptr, ring.cq_ptr)
    };

    let processed = enter_locked(header_ptr, sq_ptr, cq_ptr, to_submit);

    // Release the claim and pick up the completion port in one pass.
    let cp = {
        let mut table = RING_TABLE.lock();
        match table.get_mut(&ring_handle) {
            Some(ring) => {
                ring.busy = false;
                ring.cp_handle
            }
            // Unreachable: `destroy` refuses a busy ring.  Nothing to notify.
            None => 0,
        }
    };

    // Lock ordering: CP_TABLE → RING_TABLE, so `notify` must run with the ring
    // table released.
    if processed > 0 && cp != 0 {
        use super::completion::{self, CpHandle, WaitSource};
        completion::notify(
            CpHandle::from_raw(cp),
            WaitSource::IoCompletion(ring_handle),
        );
    }

    Ok(processed)
}

/// Drain the SQ and post CQEs, with the ring already claimed.
///
/// Split out of [`enter`] so the claim/release bookkeeping is impossible to
/// skip: this function does no locking and must only be called between setting
/// and clearing a ring's `busy` flag.
fn enter_locked(
    header_ptr: *mut IoRingHeader,
    sq_ptr: *mut SqEntry,
    cq_ptr: *mut CqEntry,
    to_submit: u32,
) -> u32 {
    // SAFETY: the ring's pointers are HHDM addresses of frames allocated in
    // `setup` and freed only by `destroy`, which refuses a busy ring — so they
    // stay valid for this whole call.  The `busy` claim also makes this the
    // only thread advancing this ring's SQ head / CQ tail.
    let header = unsafe { &*header_ptr };

    let sq_head = header.sq_head.load(Ordering::Acquire);
    let sq_tail = header.sq_tail.load(Ordering::Acquire);
    let sq_mask = header.sq_mask;

    let cq_tail = header.cq_tail.load(Ordering::Acquire);
    let cq_head = header.cq_head.load(Ordering::Acquire);
    let cq_mask = header.cq_mask;
    let cq_entries = header.cq_entries;

    // How many SQEs are pending?
    let pending = sq_tail.wrapping_sub(sq_head);
    let to_process = if to_submit == 0 {
        pending
    } else {
        pending.min(to_submit)
    };

    let mut processed: u32 = 0;
    let mut new_sq_head = sq_head;
    let mut new_cq_tail = cq_tail;

    for _ in 0..to_process {
        // Check CQ has space.
        let cq_used = new_cq_tail.wrapping_sub(cq_head);
        if cq_used >= cq_entries {
            // CQ is full — stop processing.
            break;
        }

        // Read the SQE.  Copied out by value before dispatch: the SQ is a
        // shared mapping the owning process can keep writing to, so reading a
        // field twice could see two different values (a classic double-fetch —
        // validate one pointer, act on another).
        let sq_idx = (new_sq_head & sq_mask) as usize;
        // SAFETY: `sq_idx` is masked by `sq_mask == sq_entries - 1`, so it
        // indexes within the SQ array allocated in `setup`.
        let sqe = unsafe { *sq_ptr.add(sq_idx) };

        // Execute the operation.
        let result = execute_sqe(&sqe);

        // Write the CQE.
        let cq_idx = (new_cq_tail & cq_mask) as usize;
        // SAFETY: `cq_idx` is masked by `cq_mask == cq_entries - 1`, so it
        // indexes within the CQ array allocated in `setup`.  The `busy` claim
        // makes this thread the only writer.
        unsafe {
            let cqe = &mut *cq_ptr.add(cq_idx);
            cqe.user_data = sqe.user_data;
            cqe.result = result;
        }

        new_sq_head = new_sq_head.wrapping_add(1);
        new_cq_tail = new_cq_tail.wrapping_add(1);
        processed = processed.saturating_add(1);
    }

    // Update the head/tail pointers.
    header.sq_head.store(new_sq_head, Ordering::Release);
    header.cq_tail.store(new_cq_tail, Ordering::Release);

    processed
}

/// Destroy an io_ring and free its resources.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] — no such ring.
/// - [`KernelError::PermissionDenied`] — the ring belongs to another process.
/// - [`KernelError::WouldBlock`] — the ring is mid-[`enter`]; retry after it
///   completes.
pub fn destroy(ring_handle: u64) -> KernelResult<()> {
    use crate::mm::frame::{self, PhysFrame};

    let ring = {
        let mut table = RING_TABLE.lock();
        // Checked before the removal: handles come from a global counter, so
        // without this any process could free another's ring — and the frames
        // stay mapped in the victim's address space, which turns this into a
        // use-after-free of physical memory across a security boundary.
        let ring = table
            .get(&ring_handle)
            .ok_or(KernelError::InvalidArgument)?;
        if ring.owner_process != current_owner_process() {
            return Err(KernelError::PermissionDenied);
        }
        if ring.busy {
            // `enter` is walking the SQ in these frames right now.
            return Err(KernelError::WouldBlock);
        }
        table
            .remove(&ring_handle)
            .ok_or(KernelError::InvalidArgument)?
    };

    // Tear down the owner's window onto these frames first.  Freeing them with
    // the mapping still live would hand the process a dangling view of memory
    // the allocator is free to hand to somebody else.  Each unmap drops the
    // reference taken when the mapping was made; the loop below drops the
    // ring's own reference, and the frame is released when both are gone.
    if ring.user_base != 0 {
        use crate::mm::page_table::{self, VirtAddr};
        let frame_size = crate::mm::frame::FRAME_SIZE as u64;
        if let Some(pml4) = current_owner_process().and_then(crate::proc::pcb::get_pml4) {
            let mut va = ring.user_base;
            let end = ring.user_base.saturating_add(ring.user_bytes);
            while va < end {
                // SAFETY: `pml4` is the owning process's page table (the owner
                // check above ran on this same process) and `va` walks the
                // range recorded by `attach_user_mapping`.  A range that is
                // already gone — the process munmap'd it — simply reports an
                // error, which is why the result is only used to release the
                // reference on success.
                if let Ok(phys) = unsafe { page_table::unmap_frame(pml4, VirtAddr::new(va)) } {
                    // SAFETY: refcount-aware; drops the mapping's reference.
                    unsafe {
                        let _ = frame::free_frame(phys);
                    }
                }
                va = va.saturating_add(frame_size);
            }
            crate::proc::pcb::remove_vma(current_owner_process().unwrap_or(0), ring.user_base);
        }
    }

    // Free the physical frames.
    for &phys_addr in &ring.phys_frames {
        if let Some(frame) = PhysFrame::from_addr(phys_addr) {
            // SAFETY: the ring was removed from the table under the lock and
            // `busy` was clear, so no `enter` is walking these frames.  This
            // drops the ring's own reference; a still-live user mapping holds
            // its own, so the frame is released only when both are gone.
            unsafe {
                let _ = frame::free_frame(frame);
            }
        }
    }

    serial_println!(
        "[io_ring] Destroyed ring {} ({} frames freed)",
        ring_handle,
        ring.phys_frames.len()
    );

    Ok(())
}

/// Record the owning process's mapping of a ring's frames.
///
/// Called by `SYS_IO_RING_SETUP` once it has mapped the frames returned by
/// [`setup`] into the caller's address space, so [`destroy`] can tear that
/// mapping down instead of leaving the process with a live window onto frames
/// the ring no longer owns.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] — no such ring, or a mapping is already
///   recorded (a ring is mapped exactly once, at creation).
/// - [`KernelError::PermissionDenied`] — the ring belongs to another process.
pub fn attach_user_mapping(ring_handle: u64, base: u64, bytes: u64) -> KernelResult<()> {
    let mut table = RING_TABLE.lock();
    let ring = table
        .get_mut(&ring_handle)
        .ok_or(KernelError::InvalidArgument)?;
    if ring.owner_process != current_owner_process() {
        return Err(KernelError::PermissionDenied);
    }
    if ring.user_base != 0 {
        return Err(KernelError::InvalidArgument);
    }
    ring.user_base = base;
    ring.user_bytes = bytes;
    Ok(())
}

/// Associate an io_ring with a completion port.
///
/// When CQEs are posted via `enter()`, the completion port is notified.
/// Pass 0 to clear the association.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] — no such ring.
/// - [`KernelError::PermissionDenied`] — the ring belongs to another process.
///   Without this check, pointing a foreign ring at handle 0 would silently
///   detach it from its owner's completion port, so the owner's event loop
///   would never wake again — a quiet denial of service.
pub fn set_cp(ring_handle: u64, cp_raw: u64) -> KernelResult<()> {
    let mut table = RING_TABLE.lock();
    let ring = table
        .get_mut(&ring_handle)
        .ok_or(KernelError::InvalidArgument)?;
    if ring.owner_process != current_owner_process() {
        return Err(KernelError::PermissionDenied);
    }
    ring.cp_handle = cp_raw;
    Ok(())
}

/// Check whether an io_ring has pending completion entries.
///
/// Returns `true` if CQ tail > CQ head (user hasn't consumed all CQEs).
/// Used by completion port polling.
///
/// Deliberately *not* owner-checked, unlike the mutating entry points: this
/// runs on whichever task is polling a completion port, which need not be the
/// task that created the ring, and it discloses nothing but "this ring has
/// completions" for a handle the caller already had to name.
pub fn has_completions_ready(ring_handle: u64) -> bool {
    let table = RING_TABLE.lock();
    let Some(ring) = table.get(&ring_handle) else {
        return false;
    };

    // SAFETY: ring pointers are valid HHDM addresses, we hold RING_TABLE lock.
    let header = unsafe { &*ring.header_ptr };
    let cq_head = header.cq_head.load(Ordering::Acquire);
    let cq_tail = header.cq_tail.load(Ordering::Acquire);

    cq_tail != cq_head
}

/// Get the number of pending completions in a ring.
#[allow(dead_code)]
pub fn pending_completions(ring_handle: u64) -> KernelResult<u32> {
    let table = RING_TABLE.lock();
    let ring = table
        .get(&ring_handle)
        .ok_or(KernelError::InvalidArgument)?;

    // SAFETY: header_ptr was set during io_ring_create and points to the
    // IoRingHeader at the start of the shared ring buffer frame.
    let header = unsafe { &*ring.header_ptr };
    let cq_head = header.cq_head.load(Ordering::Acquire);
    let cq_tail = header.cq_tail.load(Ordering::Acquire);

    Ok(cq_tail.wrapping_sub(cq_head))
}

// ---------------------------------------------------------------------------
// SQE execution
// ---------------------------------------------------------------------------

/// Largest payload an SQE may move in one operation.
///
/// This is a *rejection* threshold for the all-or-nothing opcodes and a
/// short-transfer cap for the ones that report a byte count; see the note on
/// each `exec_*` below.  It also bounds the kernel bounce allocation, so a
/// hostile `len` costs an `InvalidArgument`, not 4 GiB of kernel heap.
const MAX_SQE_PAYLOAD: usize = 64 * 1024;

/// Largest path an SQE may name.
const SQE_PATH_MAX: usize = 4096;

/// Copy an SQE-named path out of user memory.
///
/// `addr`/`len` in an SQE are *userspace* values — the SQ is a shared mapping
/// that the submitting process fills in — so they get the same treatment as a
/// syscall argument: bounce through kernel memory, reject rather than truncate.
fn sqe_path(addr: u64, len: usize) -> Result<crate::fs::path::PathBuf, KernelError> {
    let bytes = crate::mm::user::read_user_vec(addr, len, SQE_PATH_MAX)?;
    // No decode step: a path is an uninterpreted byte string, and
    // `PathBuf::from` takes the `Vec` by value, so this is one allocation.
    // This used to `String::from_utf8(..)` and return `InvalidArgument` on
    // failure, mirroring the VFS's old `&str` API — which rejected legal
    // names and so made a file with a non-UTF-8 byte in its path
    // unopenable through io_uring specifically, while other paths to the
    // same file worked.  Fixed with D-VFS-PATHS-ARE-STR-NOT-BYTES.
    Ok(crate::fs::path::PathBuf::from(bytes))
}

/// Execute a single submission queue entry and return the result.
///
/// Dispatches to the appropriate kernel subsystem based on the opcode.
/// Returns >= 0 on success, < 0 on error (negative KernelError code).
fn execute_sqe(sqe: &SqEntry) -> i64 {
    match sqe.opcode {
        IO_OP_NOP => 0,
        IO_OP_CONSOLE_WRITE => exec_console_write(sqe),
        IO_OP_CHANNEL_SEND => exec_channel_send(sqe),
        IO_OP_CHANNEL_RECV => exec_channel_recv(sqe),
        IO_OP_PIPE_WRITE => exec_pipe_write(sqe),
        IO_OP_PIPE_READ => exec_pipe_read(sqe),
        IO_OP_FS_READ => exec_fs_read(sqe),
        IO_OP_FS_WRITE => exec_fs_write(sqe),
        IO_OP_FH_READ => exec_fh_read(sqe),
        IO_OP_FH_WRITE => exec_fh_write(sqe),
        IO_OP_FH_PREAD => exec_fh_pread(sqe),
        IO_OP_FH_PWRITE => exec_fh_pwrite(sqe),
        IO_OP_EVENTFD_SIGNAL => exec_eventfd_signal(sqe),
        IO_OP_SEM_SIGNAL => exec_sem_signal(sqe),
        IO_OP_TIMEOUT => exec_timeout(sqe),
        IO_OP_TIMEOUT_CANCEL => exec_timeout_cancel(sqe),
        IO_OP_SERVICE_CONNECT => exec_service_connect(sqe),
        IO_OP_SLEEP => exec_sleep(sqe),
        _ => KernelError::NotSupported.code() as i64,
    }
}

fn exec_console_write(sqe: &SqEntry) -> i64 {
    let len = sqe.len as usize;

    if sqe.addr == 0 || len == 0 {
        return 0;
    }

    // Short write: the CQE result is the number of bytes consumed, so a caller
    // handing over more than the cap loops on the remainder — the same contract
    // as `write(2)`, and the only shape in which a length cap is honest.
    let n = len.min(MAX_SQE_PAYLOAD);
    let bytes = match crate::mm::user::read_user_vec(sqe.addr, n, MAX_SQE_PAYLOAD) {
        Ok(b) => b,
        Err(e) => return e.code() as i64,
    };

    if let Ok(s) = core::str::from_utf8(&bytes) {
        crate::console::write_str(s);
    } else {
        for &b in &bytes {
            crate::console::putchar(b);
        }
    }

    bytes.len() as i64
}

fn exec_channel_send(sqe: &SqEntry) -> i64 {
    use crate::ipc::channel::{self, ChannelHandle, Message};

    let handle = ChannelHandle::from_raw(sqe.handle);
    let len = sqe.len as usize;

    if sqe.addr == 0 || len == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // All-or-nothing: the CQE result is 0/error, not a byte count, so an
    // over-long message must be *rejected*.  Clamping would send a truncated
    // message and report success.
    let data = match crate::mm::user::read_user_vec(sqe.addr, len, MAX_SQE_PAYLOAD) {
        Ok(d) => d,
        Err(e) => return e.code() as i64,
    };
    let msg = match Message::from_bytes(&data) {
        Ok(m) => m,
        Err(e) => return e.code() as i64,
    };

    match channel::send(handle, msg) {
        Ok(()) => 0,
        Err(e) => e.code() as i64,
    }
}

fn exec_channel_recv(sqe: &SqEntry) -> i64 {
    use crate::ipc::channel::{self, ChannelHandle};

    let handle = ChannelHandle::from_raw(sqe.handle);
    let buf_cap = sqe.len as usize;

    if sqe.addr == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // The destination is validated *before* the dequeue, so a bad buffer costs
    // an error rather than a message that has already left the channel with
    // nowhere to put it.  `with_user_out_buf` then re-validates at the copy.
    let r = crate::mm::user::with_user_out_buf(
        sqe.addr,
        buf_cap.min(MAX_SQE_PAYLOAD),
        MAX_SQE_PAYLOAD,
        |buf| match channel::try_recv(handle) {
            Ok(Some(msg)) => {
                let n = msg.data().len().min(buf.len());
                match (buf.get_mut(..n), msg.data().get(..n)) {
                    (Some(dst), Some(src)) => {
                        dst.copy_from_slice(src);
                        Ok(n)
                    }
                    _ => Err(KernelError::InternalError),
                }
            }
            Ok(None) => Err(KernelError::WouldBlock),
            Err(e) => Err(e),
        },
    );
    match r {
        Ok(n) => n as i64,
        Err(e) => e.code() as i64,
    }
}

fn exec_pipe_write(sqe: &SqEntry) -> i64 {
    use crate::ipc::pipe::{self, PipeHandle};

    let handle = PipeHandle::from_raw(sqe.handle);
    let len = sqe.len as usize;

    if sqe.addr == 0 || len == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // Short write: the result is the byte count `pipe::try_write` accepted, so
    // capping what we hand it is a partial write the caller already has to
    // handle (a pipe can accept less than offered regardless).
    let n = len.min(MAX_SQE_PAYLOAD);
    let data = match crate::mm::user::read_user_vec(sqe.addr, n, MAX_SQE_PAYLOAD) {
        Ok(d) => d,
        Err(e) => return e.code() as i64,
    };

    match pipe::try_write(handle, &data) {
        Ok(written) => written as i64,
        Err(e) => e.code() as i64,
    }
}

fn exec_pipe_read(sqe: &SqEntry) -> i64 {
    use crate::ipc::pipe::{self, PipeHandle};

    let handle = PipeHandle::from_raw(sqe.handle);
    let cap = sqe.len as usize;

    if sqe.addr == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // `pipe::try_read` only ever sees kernel scratch: the pipe lock is taken
    // inside it, and a user mapping handed straight in could fault (or go stale
    // under a peer's `munmap`) with that lock held.
    let r = crate::mm::user::with_user_out_buf(
        sqe.addr,
        cap.min(MAX_SQE_PAYLOAD),
        MAX_SQE_PAYLOAD,
        |buf| pipe::try_read(handle, buf),
    );
    match r {
        Ok(n) => n as i64,
        Err(e) => e.code() as i64,
    }
}

fn exec_fs_read(sqe: &SqEntry) -> i64 {
    let path_len = sqe.len as usize;
    let buf_cap = sqe.arg2 as usize;

    if sqe.addr == 0 || path_len == 0 || sqe.arg1 == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    let path = match sqe_path(sqe.addr, path_len) {
        Ok(p) => p,
        Err(e) => return e.code() as i64,
    };

    // Short read: the result is the byte count delivered.  The whole file is
    // read into kernel memory first (that is what `Vfs::read_file` returns),
    // then as much of it as fits is copied out in one go.
    let r = crate::mm::user::with_user_out_buf(
        sqe.arg1,
        buf_cap.min(MAX_SQE_PAYLOAD),
        MAX_SQE_PAYLOAD,
        |buf| {
            let data = crate::fs::Vfs::read_file(&path)?;
            let n = data.len().min(buf.len());
            match (buf.get_mut(..n), data.get(..n)) {
                (Some(dst), Some(src)) => {
                    dst.copy_from_slice(src);
                    Ok(n)
                }
                _ => Err(KernelError::InternalError),
            }
        },
    );
    match r {
        Ok(n) => n as i64,
        Err(e) => e.code() as i64,
    }
}

fn exec_fs_write(sqe: &SqEntry) -> i64 {
    let path_len = sqe.len as usize;
    let data_len = sqe.arg2 as usize;

    if sqe.addr == 0 || path_len == 0 || sqe.arg1 == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    let path = match sqe_path(sqe.addr, path_len) {
        Ok(p) => p,
        Err(e) => return e.code() as i64,
    };

    // All-or-nothing: `write_file` replaces the file's contents and the result
    // is 0/error, so an over-long payload is rejected rather than clamped —
    // clamping would silently truncate the file and report success.
    let data = match crate::mm::user::read_user_vec(sqe.arg1, data_len, MAX_SQE_PAYLOAD) {
        Ok(d) => d,
        Err(e) => return e.code() as i64,
    };

    match crate::fs::Vfs::write_file(&path, &data) {
        Ok(()) => 0,
        Err(e) => e.code() as i64,
    }
}

// ---------------------------------------------------------------------------
// File handle operations (opcodes 8–11)
// ---------------------------------------------------------------------------

fn exec_fh_read(sqe: &SqEntry) -> i64 {
    let fh = sqe.handle;
    let cap = sqe.len as usize;

    if sqe.addr == 0 || cap == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // Short read; the result is the byte count.  The scratch buffer also keeps
    // the filesystem from ever seeing a user address — it can block on I/O, at
    // which point a user mapping would be both SMAP-illegal and revocable.
    let r = crate::mm::user::with_user_out_buf(
        sqe.addr,
        cap.min(MAX_SQE_PAYLOAD),
        MAX_SQE_PAYLOAD,
        |buf| crate::fs::handle::read(fh, buf),
    );
    match r {
        Ok(n) => n as i64,
        Err(e) => e.code() as i64,
    }
}

fn exec_fh_write(sqe: &SqEntry) -> i64 {
    let fh = sqe.handle;
    let len = sqe.len as usize;

    if sqe.addr == 0 || len == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // Short write: the result is the count `handle::write` accepted.
    let n = len.min(MAX_SQE_PAYLOAD);
    let data = match crate::mm::user::read_user_vec(sqe.addr, n, MAX_SQE_PAYLOAD) {
        Ok(d) => d,
        Err(e) => return e.code() as i64,
    };

    match crate::fs::handle::write(fh, &data) {
        Ok(n) => n as i64,
        Err(e) => e.code() as i64,
    }
}

fn exec_fh_pread(sqe: &SqEntry) -> i64 {
    let fh = sqe.handle;
    let cap = sqe.len as usize;
    let offset = sqe.arg1;

    if sqe.addr == 0 || cap == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // Validated before the read so a bad destination fails without having
    // touched the file at all.
    if let Err(e) = crate::mm::user::validate_user_write(sqe.addr, cap.min(MAX_SQE_PAYLOAD)) {
        return e.code() as i64;
    }

    // `read_at` is the whole point of `pread`: it takes the handle table's lock
    // once and reads at `offset` without ever consulting or moving the stored
    // cursor.  This opcode used to emulate that with seek/read/seek, which was
    // neither cursor-preserving nor safe against a peer sharing the handle —
    // the restore could clobber a position the peer had legitimately advanced.
    // See B-FS-HANDLE-PREAD-PWRITE-ARE-NOT-ATOMIC in known-issues.md.
    match crate::mm::user::with_user_out_buf(
        sqe.addr,
        cap.min(MAX_SQE_PAYLOAD),
        MAX_SQE_PAYLOAD,
        |buf| crate::fs::handle::read_at(fh, offset, buf),
    ) {
        Ok(n) => n as i64,
        Err(e) => e.code() as i64,
    }
}

fn exec_fh_pwrite(sqe: &SqEntry) -> i64 {
    let fh = sqe.handle;
    let len = sqe.len as usize;
    let offset = sqe.arg1;

    if sqe.addr == 0 || len == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // Copied out of user memory *before* the write: the copy can fail, and a
    // failure that reached the filesystem first would have to be undone.  Short
    // write — the result is the count accepted.
    let n = len.min(MAX_SQE_PAYLOAD);
    let data = match crate::mm::user::read_user_vec(sqe.addr, n, MAX_SQE_PAYLOAD) {
        Ok(d) => d,
        Err(e) => return e.code() as i64,
    };

    // `write_at` writes at `offset` under a single acquisition of the handle
    // table's lock and leaves the stored cursor alone — the POSIX `pwrite`
    // contract, and (unlike the seek/write/seek this replaced) safe to issue
    // concurrently with a peer's sequential I/O on the same handle.  It also
    // ignores `O_APPEND`, as Linux's `pwrite` does.  See
    // B-FS-HANDLE-PREAD-PWRITE-ARE-NOT-ATOMIC in known-issues.md.
    match crate::fs::handle::write_at(fh, offset, &data) {
        Ok(n) => n as i64,
        Err(e) => e.code() as i64,
    }
}

// ---------------------------------------------------------------------------
// Eventfd / semaphore operations (opcodes 12–13)
// ---------------------------------------------------------------------------

fn exec_eventfd_signal(sqe: &SqEntry) -> i64 {
    use crate::ipc::eventfd::{self, EventFdHandle};

    let handle = EventFdHandle::from_raw(sqe.handle);
    let value = sqe.arg1;

    if value == 0 {
        return 0; // No-op signal.
    }

    match eventfd::write(handle, value) {
        Ok(()) => 0,
        Err(e) => e.code() as i64,
    }
}

fn exec_sem_signal(sqe: &SqEntry) -> i64 {
    use crate::ipc::semaphore::{self, SemHandle};

    let handle = SemHandle::from_raw(sqe.handle);
    let count = sqe.arg1;

    match semaphore::signal(handle, count) {
        Ok(()) => 0,
        Err(e) => e.code() as i64,
    }
}

// ---------------------------------------------------------------------------
// Timeout / Sleep / Service connect
// ---------------------------------------------------------------------------

/// Execute IO_OP_TIMEOUT — sleep for the specified nanosecond duration.
///
/// This is a synchronous timeout: the io_ring processing blocks for
/// the specified duration, then completes with result 0.  Useful for
/// rate-limiting batch submissions or inserting delays in sequences.
fn exec_timeout(sqe: &SqEntry) -> i64 {
    let timeout_ns = sqe.arg1;

    if timeout_ns == 0 {
        // Zero timeout = immediate completion (no sleep).
        return 0;
    }

    // Cap at 10 seconds to prevent accidental multi-minute hangs.
    let capped_ns = timeout_ns.min(10_000_000_000);
    crate::sched::sleep_ns(capped_ns);
    0
}

/// Execute IO_OP_TIMEOUT_CANCEL — cancel a pending timeout.
///
/// In the current synchronous io_ring model, timeouts execute inline
/// and cannot be cancelled after submission.  This returns -ENOENT
/// (NotFound) always, but exists for API compatibility with future
/// async io_ring modes.
fn exec_timeout_cancel(_sqe: &SqEntry) -> i64 {
    // In synchronous mode, by the time we see a cancel SQE, the target
    // timeout has already completed (SQEs are processed in order).
    KernelError::NotFound.code() as i64
}

/// Execute IO_OP_SERVICE_CONNECT — connect to a named service.
///
/// `addr` points to the service name bytes, `len` is the name length.
/// Returns the raw channel handle on success (>= 0).
fn exec_service_connect(sqe: &SqEntry) -> i64 {
    /// A service name longer than this is rejected, never clipped: a clipped
    /// name is a *different* service, and connecting to the wrong one is a
    /// failure in exactly the wrong direction.
    const NAME_MAX: usize = 256;

    let len = sqe.len as usize;

    if sqe.addr == 0 || len == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    let name = match crate::mm::user::read_user_vec(sqe.addr, len, NAME_MAX) {
        Ok(n) => n,
        Err(e) => return e.code() as i64,
    };

    match crate::ipc::service::connect(&name) {
        Ok(handle) => handle.raw() as i64,
        Err(e) => e.code() as i64,
    }
}

/// Execute IO_OP_SLEEP — nanosecond-precision delay via hrtimer.
///
/// Always sleeps the full duration (no cancellation).
fn exec_sleep(sqe: &SqEntry) -> i64 {
    let duration_ns = sqe.arg1;

    if duration_ns == 0 {
        crate::sched::yield_now();
        return 0;
    }

    // Cap at 60 seconds.
    let capped_ns = duration_ns.min(60_000_000_000);
    crate::sched::sleep_ns(capped_ns);
    0
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run io_ring self-tests (early boot — no filesystem available).
pub fn self_test() -> KernelResult<()> {
    test_ring_create_destroy()?;
    test_nop_submission()?;
    test_console_write_batch()?;
    test_fh_read_write()?;
    test_fh_positioned_io_leaves_the_cursor_alone()?;
    test_timeout_and_service()?;

    Ok(())
}

/// Run io_ring file handle self-tests (after filesystem is mounted).
///
/// This is called separately from main self_test() because it requires
/// /tmp to be mounted, which happens later in the boot sequence.
pub fn self_test_fh() -> KernelResult<()> {
    test_fh_read_write()?;
    test_fh_positioned_io_leaves_the_cursor_alone()
}

/// Test 1: Create and destroy a ring.
fn test_ring_create_destroy() -> KernelResult<()> {
    let (handle, base_virt, frames) = setup(8, 16)?;

    // Verify the header is readable.
    // SAFETY: base_virt returned by setup() points to a freshly allocated
    // io_ring buffer; the first bytes are the IoRingHeader.
    let header = unsafe { &*(base_virt as *const IoRingHeader) };
    if header.sq_entries != 8 {
        serial_println!(
            "[io_ring]   FAIL: sq_entries should be 8, got {}",
            header.sq_entries
        );
        destroy(handle)?;
        return Err(KernelError::InternalError);
    }
    if header.cq_entries != 16 {
        serial_println!(
            "[io_ring]   FAIL: cq_entries should be 16, got {}",
            header.cq_entries
        );
        destroy(handle)?;
        return Err(KernelError::InternalError);
    }
    if header.sq_mask != 7 {
        serial_println!(
            "[io_ring]   FAIL: sq_mask should be 7, got {}",
            header.sq_mask
        );
        destroy(handle)?;
        return Err(KernelError::InternalError);
    }

    destroy(handle)?;
    serial_println!(
        "[io_ring]   Create/destroy ring (sq=8, cq=16, {} frames): OK",
        frames.len()
    );
    Ok(())
}

/// Test 2: Submit NOP entries and verify CQEs.
fn test_nop_submission() -> KernelResult<()> {
    let (handle, base_virt, _frames) = setup(8, 16)?;

    // SAFETY: base_virt from setup() points to a valid io_ring buffer.
    let header = unsafe { &mut *(base_virt as *mut IoRingHeader) };
    #[allow(clippy::arithmetic_side_effects)]
    let sq_base = (base_virt + core::mem::size_of::<IoRingHeader>() as u64) as *mut SqEntry;
    #[allow(clippy::arithmetic_side_effects)]
    let cq_base = (base_virt
        + core::mem::size_of::<IoRingHeader>() as u64
        + 8 * core::mem::size_of::<SqEntry>() as u64) as *const CqEntry;

    // Write 3 NOP SQEs.
    for i in 0u32..3 {
        let sqe = SqEntry {
            opcode: IO_OP_NOP,
            flags: 0,
            _pad0: [0; 2],
            _pad1: 0,
            user_data: (i as u64).wrapping_add(100),
            handle: 0,
            addr: 0,
            len: 0,
            _pad2: 0,
            arg1: 0,
            arg2: 0,
        };
        // SAFETY: sq_base points to valid SQ array, i < sq_entries.
        unsafe {
            *sq_base.add(i as usize) = sqe;
        }
    }

    // Advance the SQ tail.
    header.sq_tail.store(3, Ordering::Release);

    // Process.
    let processed = enter(handle, 0)?;
    if processed != 3 {
        serial_println!("[io_ring]   FAIL: processed {} SQEs, expected 3", processed);
        destroy(handle)?;
        return Err(KernelError::InternalError);
    }

    // Verify CQEs.
    let cq_tail = header.cq_tail.load(Ordering::Acquire);
    if cq_tail != 3 {
        serial_println!("[io_ring]   FAIL: cq_tail should be 3, got {}", cq_tail);
        destroy(handle)?;
        return Err(KernelError::InternalError);
    }

    for i in 0..3u32 {
        // SAFETY: cq_base points to the CQ array and i < cq_entries.
        let cqe = unsafe { &*cq_base.add(i as usize) };
        let expected_ud = (i as u64).wrapping_add(100);
        if cqe.user_data != expected_ud {
            serial_println!(
                "[io_ring]   FAIL: CQE[{}] user_data={}, expected {}",
                i,
                cqe.user_data,
                expected_ud
            );
            destroy(handle)?;
            return Err(KernelError::InternalError);
        }
        if cqe.result != 0 {
            serial_println!(
                "[io_ring]   FAIL: CQE[{}] result={}, expected 0",
                i,
                cqe.result
            );
            destroy(handle)?;
            return Err(KernelError::InternalError);
        }
    }

    destroy(handle)?;
    serial_println!("[io_ring]   NOP submission (3 entries): OK");
    Ok(())
}

/// Test 3: Batch console write via io_ring.
fn test_console_write_batch() -> KernelResult<()> {
    let (handle, base_virt, _frames) = setup(8, 16)?;

    // SAFETY: base_virt from setup() points to a valid io_ring buffer.
    let header = unsafe { &mut *(base_virt as *mut IoRingHeader) };
    #[allow(clippy::arithmetic_side_effects)]
    let sq_base = (base_virt + core::mem::size_of::<IoRingHeader>() as u64) as *mut SqEntry;
    #[allow(clippy::arithmetic_side_effects)]
    let cq_base = (base_virt
        + core::mem::size_of::<IoRingHeader>() as u64
        + 8 * core::mem::size_of::<SqEntry>() as u64) as *const CqEntry;

    let msg1 = b"[io_ring]   Batch write 1\n";
    let msg2 = b"[io_ring]   Batch write 2\n";

    // Write 2 CONSOLE_WRITE SQEs.
    let sqe1 = SqEntry {
        opcode: IO_OP_CONSOLE_WRITE,
        flags: 0,
        _pad0: [0; 2],
        _pad1: 0,
        user_data: 1,
        handle: 0,
        addr: msg1.as_ptr() as u64,
        len: msg1.len() as u32,
        _pad2: 0,
        arg1: 0,
        arg2: 0,
    };
    let sqe2 = SqEntry {
        opcode: IO_OP_CONSOLE_WRITE,
        flags: 0,
        _pad0: [0; 2],
        _pad1: 0,
        user_data: 2,
        handle: 0,
        addr: msg2.as_ptr() as u64,
        len: msg2.len() as u32,
        _pad2: 0,
        arg1: 0,
        arg2: 0,
    };

    // SAFETY: sq_base points to the SQ array; indices 0 and 1 < sq_entries (8).
    unsafe {
        *sq_base.add(0) = sqe1;
        *sq_base.add(1) = sqe2;
    }
    header.sq_tail.store(2, Ordering::Release);

    let processed = enter(handle, 0)?;
    if processed != 2 {
        serial_println!("[io_ring]   FAIL: processed {} SQEs, expected 2", processed);
        destroy(handle)?;
        return Err(KernelError::InternalError);
    }

    // Verify both CQEs report success.
    for i in 0..2u32 {
        // SAFETY: cq_base points to the CQ array; i < cq_entries.
        let cqe = unsafe { &*cq_base.add(i as usize) };
        if cqe.result < 0 {
            serial_println!("[io_ring]   FAIL: CQE[{}] result={} (error)", i, cqe.result);
            destroy(handle)?;
            return Err(KernelError::InternalError);
        }
    }

    destroy(handle)?;
    serial_println!("[io_ring]   Console write batch (2 entries): OK");
    Ok(())
}

/// Test 4: File handle read/write via io_ring.
///
/// Opens a temp file, writes data via IO_OP_FH_WRITE, reads it back
/// via IO_OP_FH_READ, and verifies correctness.
///
/// Skipped if no filesystem is mounted yet (io_ring self-test runs
/// before VFS init in the boot sequence).
fn test_fh_read_write() -> KernelResult<()> {
    // Check if /tmp is available.  If no filesystem is mounted yet
    // (io_ring self-test runs early in boot), skip gracefully.
    let test_path = "/tmp/io_ring_test";
    let test_data = b"io_ring file handle test data 1234567890";
    if crate::fs::Vfs::write_file(test_path, test_data).is_err() {
        serial_println!("[io_ring]   File handle read/write: SKIPPED (no FS)");
        return Ok(());
    }

    // Open the file for read.
    let fh = crate::fs::handle::open(test_path, crate::fs::handle::OpenFlags::READ)?;

    // Create io_ring.
    let (ring_handle, base_virt, _frames) = setup(8, 16)?;
    // SAFETY: base_virt from setup() points to a valid io_ring buffer.
    let header = unsafe { &mut *(base_virt as *mut IoRingHeader) };
    #[allow(clippy::arithmetic_side_effects)]
    let sq_base = (base_virt + core::mem::size_of::<IoRingHeader>() as u64) as *mut SqEntry;
    #[allow(clippy::arithmetic_side_effects)]
    let cq_base = (base_virt
        + core::mem::size_of::<IoRingHeader>() as u64
        + 8u64 * core::mem::size_of::<SqEntry>() as u64) as *const CqEntry;

    // Set up a read buffer in kernel memory (simulating user buffer).
    let mut read_buf = [0u8; 64];

    // Submit a FH_READ SQE.
    let sqe = SqEntry {
        opcode: IO_OP_FH_READ,
        flags: 0,
        _pad0: [0; 2],
        _pad1: 0,
        user_data: 500,
        handle: fh,
        addr: read_buf.as_mut_ptr() as u64,
        len: read_buf.len() as u32,
        _pad2: 0,
        arg1: 0,
        arg2: 0,
    };
    // SAFETY: sq_base points to the SQ array; index 0 < sq_entries (8).
    unsafe {
        *sq_base.add(0) = sqe;
    }
    header.sq_tail.store(1, Ordering::Release);

    let processed = enter(ring_handle, 0)?;
    if processed != 1 {
        serial_println!(
            "[io_ring]   FAIL: fh_read processed {} SQEs, expected 1",
            processed
        );
        let _ = crate::fs::handle::close(fh);
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // Verify CQE.
    // SAFETY: cq_base points to the CQ array; index 0 is valid.
    let cqe = unsafe { &*cq_base.add(0) };
    if cqe.result != test_data.len() as i64 {
        serial_println!(
            "[io_ring]   FAIL: fh_read CQE result={}, expected {}",
            cqe.result,
            test_data.len()
        );
        let _ = crate::fs::handle::close(fh);
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }
    if cqe.user_data != 500 {
        serial_println!(
            "[io_ring]   FAIL: fh_read CQE user_data={}, expected 500",
            cqe.user_data
        );
        let _ = crate::fs::handle::close(fh);
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // Verify the data read matches.
    if &read_buf[..test_data.len()] != test_data {
        serial_println!("[io_ring]   FAIL: fh_read data mismatch");
        let _ = crate::fs::handle::close(fh);
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // Clean up.
    let _ = crate::fs::handle::close(fh);
    destroy(ring_handle)?;
    let _ = crate::fs::Vfs::remove(test_path);
    serial_println!("[io_ring]   File handle read/write (1 entry): OK");
    Ok(())
}

/// Test 4b: `IO_OP_FH_PREAD` / `IO_OP_FH_PWRITE` are positioned I/O.
///
/// The contract these opcodes have to keep is the POSIX one: read or write at
/// an explicit offset **without disturbing the handle's cursor**.  They used to
/// be emulated as seek/read/seek, which moved the cursor three times and only
/// happened to put it back — so a peer sharing the handle saw it wander, and a
/// failure mid-sandwich left it parked at the wrong place
/// (B-KNOWN-ISSUES: B-FS-HANDLE-PREAD-PWRITE-ARE-NOT-ATOMIC).
///
/// Regression: this test interleaves a sequential read with a pread and a
/// pwrite and asserts the sequential stream is completely unaffected — which is
/// exactly what the seek sandwich could not guarantee.
///
/// Skipped if no filesystem is mounted yet.
fn test_fh_positioned_io_leaves_the_cursor_alone() -> KernelResult<()> {
    use crate::fs::handle::{OpenFlags, SeekFrom};

    let test_path = "/tmp/io_ring_pio_test";
    // 26 bytes: byte i is b'A' + i, so any offset is self-identifying.
    let mut test_data = [0u8; 26];
    for (i, b) in test_data.iter_mut().enumerate() {
        // 26 elements, so the index always fits; `unwrap_or(0)` rather than an
        // `expect` only because this is the kernel and a panic here would be a
        // far worse outcome than a test that then fails its own assertion.
        *b = b'A'.wrapping_add(u8::try_from(i).unwrap_or(0));
    }
    if crate::fs::Vfs::write_file(test_path, &test_data).is_err() {
        serial_println!("[io_ring]   Positioned I/O (pread/pwrite): SKIPPED (no FS)");
        return Ok(());
    }

    let fh = crate::fs::handle::open(test_path, OpenFlags::READ.union(OpenFlags::WRITE))?;
    let (ring_handle, base_virt, _frames) = setup(8, 16)?;
    // SAFETY: base_virt from setup() points to a valid io_ring buffer.
    let header = unsafe { &mut *(base_virt as *mut IoRingHeader) };
    #[allow(clippy::arithmetic_side_effects)]
    let sq_base = (base_virt + core::mem::size_of::<IoRingHeader>() as u64) as *mut SqEntry;
    #[allow(clippy::arithmetic_side_effects)]
    let cq_base = (base_virt
        + core::mem::size_of::<IoRingHeader>() as u64
        + 8u64 * core::mem::size_of::<SqEntry>() as u64) as *const CqEntry;

    // A tiny helper so each step is one submission on a freshly-rewound ring.
    // Returns the CQE result, or the failing `KernelError` code as a negative.
    let submit = |sqe: SqEntry| -> KernelResult<i64> {
        header.sq_head.store(0, Ordering::Release);
        header.sq_tail.store(0, Ordering::Release);
        header.cq_head.store(0, Ordering::Release);
        header.cq_tail.store(0, Ordering::Release);
        // SAFETY: sq_base points to the SQ array; index 0 < sq_entries (8).
        unsafe {
            *sq_base.add(0) = sqe;
        }
        header.sq_tail.store(1, Ordering::Release);
        if enter(ring_handle, 0)? != 1 {
            return Err(KernelError::InternalError);
        }
        // SAFETY: cq_base points to the CQ array; index 0 is valid and the
        // completion for the single SQE above was just posted there.
        Ok(unsafe { (*cq_base.add(0)).result })
    };

    // Bail-out that always closes both objects, so no early `?` leaks a ring.
    macro_rules! fail {
        ($($arg:tt)*) => {{
            serial_println!($($arg)*);
            let _ = crate::fs::handle::close(fh);
            destroy(ring_handle)?;
            let _ = crate::fs::Vfs::remove(test_path);
            return Err(KernelError::InternalError);
        }};
    }

    let mut buf = [0u8; 8];
    let sqe = |opcode: u8, addr: u64, len: u32, offset: u64| SqEntry {
        opcode,
        flags: 0,
        _pad0: [0; 2],
        _pad1: 0,
        user_data: 600,
        handle: fh,
        addr,
        len,
        _pad2: 0,
        arg1: offset,
        arg2: 0,
    };

    // 1. Sequential read of 4 bytes leaves the cursor at 4.
    let r = submit(sqe(IO_OP_FH_READ, buf.as_mut_ptr() as u64, 4, 0))?;
    if r != 4 || buf.get(..4) != Some(b"ABCD".as_slice()) {
        fail!(
            "[io_ring]   FAIL: pio setup read result={} buf={:?}",
            r,
            &buf[..4]
        );
    }

    // 2. A pread at a far offset returns that offset's bytes...
    buf = [0; 8];
    let r = submit(sqe(IO_OP_FH_PREAD, buf.as_mut_ptr() as u64, 4, 20))?;
    if r != 4 || buf.get(..4) != Some(b"UVWX".as_slice()) {
        fail!(
            "[io_ring]   FAIL: pread result={} buf={:?}, expected 4/UVWX",
            r,
            &buf[..4]
        );
    }
    // ...and does not move the cursor.
    let pos = crate::fs::handle::seek(fh, SeekFrom::Current(0))?;
    if pos != 4 {
        fail!(
            "[io_ring]   FAIL: pread moved the cursor to {}, expected 4",
            pos
        );
    }

    // 3. A pwrite at another offset lands there and also leaves the cursor.
    let payload = *b"zzzz";
    let r = submit(sqe(IO_OP_FH_PWRITE, payload.as_ptr() as u64, 4, 10))?;
    if r != 4 {
        fail!("[io_ring]   FAIL: pwrite result={}, expected 4", r);
    }
    let pos = crate::fs::handle::seek(fh, SeekFrom::Current(0))?;
    if pos != 4 {
        fail!(
            "[io_ring]   FAIL: pwrite moved the cursor to {}, expected 4",
            pos
        );
    }

    // 4. The sequential stream picks up exactly where step 1 left it — the
    //    property the seek sandwich could not offer a concurrent peer.
    buf = [0; 8];
    let r = submit(sqe(IO_OP_FH_READ, buf.as_mut_ptr() as u64, 4, 0))?;
    if r != 4 || buf.get(..4) != Some(b"EFGH".as_slice()) {
        fail!(
            "[io_ring]   FAIL: post-pio sequential read={} buf={:?}, expected 4/EFGH",
            r,
            &buf[..4]
        );
    }

    // 5. The pwrite really reached the file, at the offset asked for.
    match crate::fs::Vfs::read_file(test_path) {
        Ok(data) if data.get(10..14) == Some(b"zzzz".as_slice()) => {}
        Ok(data) => fail!(
            "[io_ring]   FAIL: pwrite landed wrong: {:?}",
            data.get(8..16)
        ),
        Err(e) => fail!("[io_ring]   FAIL: re-reading the pwrite target: {:?}", e),
    }

    let _ = crate::fs::handle::close(fh);
    destroy(ring_handle)?;
    let _ = crate::fs::Vfs::remove(test_path);
    serial_println!("[io_ring]   Positioned I/O (pread/pwrite preserve the cursor): OK");
    Ok(())
}

/// Test 5: Timeout and service connect opcodes.
///
/// Verifies:
/// - IO_OP_TIMEOUT with short duration completes successfully (result 0).
/// - IO_OP_TIMEOUT with 0 ns completes immediately (result 0).
/// - IO_OP_TIMEOUT_CANCEL always returns NotFound in sync mode.
/// - IO_OP_SERVICE_CONNECT connects to a registered service.
fn test_timeout_and_service() -> KernelResult<()> {
    use crate::ipc::{channel, service};

    let (ring_handle, base_virt, _frames) = setup(8, 16)?;

    // SAFETY: base_virt from setup() points to a valid io_ring buffer.
    let header = unsafe { &mut *(base_virt as *mut IoRingHeader) };
    #[allow(clippy::arithmetic_side_effects)]
    let sq_base = (base_virt + core::mem::size_of::<IoRingHeader>() as u64) as *mut SqEntry;
    #[allow(clippy::arithmetic_side_effects)]
    let cq_base = (base_virt
        + core::mem::size_of::<IoRingHeader>() as u64
        + 8u64 * core::mem::size_of::<SqEntry>() as u64) as *const CqEntry;

    // --- Sub-test A: IO_OP_TIMEOUT with 0ns (immediate) ---
    let sqe_timeout_zero = SqEntry {
        opcode: IO_OP_TIMEOUT,
        flags: 0,
        _pad0: [0; 2],
        _pad1: 0,
        user_data: 1000,
        handle: 0,
        addr: 0,
        len: 0,
        _pad2: 0,
        arg1: 0, // 0 ns = immediate
        arg2: 0,
    };
    // SAFETY: sq_base points to the SQ array; index 0 < sq_entries (8).
    unsafe {
        *sq_base.add(0) = sqe_timeout_zero;
    }
    header.sq_tail.store(1, Ordering::Release);

    let processed = enter(ring_handle, 0)?;
    if processed != 1 {
        serial_println!(
            "[io_ring]   FAIL: timeout(0ns) processed {}, expected 1",
            processed
        );
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // SAFETY: cq_base points to the CQ array; index 0 is valid.
    let cqe = unsafe { &*cq_base.add(0) };
    if cqe.user_data != 1000 || cqe.result != 0 {
        serial_println!(
            "[io_ring]   FAIL: timeout(0ns) CQE ud={} result={}, expected ud=1000 result=0",
            cqe.user_data,
            cqe.result
        );
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // Reset ring pointers for next sub-test.
    header.sq_head.store(0, Ordering::Release);
    header.sq_tail.store(0, Ordering::Release);
    header.cq_head.store(0, Ordering::Release);
    header.cq_tail.store(0, Ordering::Release);

    // --- Sub-test B: IO_OP_TIMEOUT_CANCEL (always NotFound in sync mode) ---
    let sqe_cancel = SqEntry {
        opcode: IO_OP_TIMEOUT_CANCEL,
        flags: 0,
        _pad0: [0; 2],
        _pad1: 0,
        user_data: 2000,
        handle: 0,
        addr: 0,
        len: 0,
        _pad2: 0,
        arg1: 1000, // try to cancel the previous timeout's user_data
        arg2: 0,
    };
    // SAFETY: sq_base points to the SQ array; index 0 < sq_entries.
    unsafe {
        *sq_base.add(0) = sqe_cancel;
    }
    header.sq_tail.store(1, Ordering::Release);

    let processed = enter(ring_handle, 0)?;
    if processed != 1 {
        serial_println!(
            "[io_ring]   FAIL: timeout_cancel processed {}, expected 1",
            processed
        );
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // SAFETY: cq_base points to the CQ array; index 0 is valid.
    let cqe = unsafe { &*cq_base.add(0) };
    let expected_cancel_result = KernelError::NotFound.code() as i64;
    if cqe.result != expected_cancel_result {
        serial_println!(
            "[io_ring]   FAIL: timeout_cancel result={}, expected {}",
            cqe.result,
            expected_cancel_result
        );
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // Reset ring pointers.
    header.sq_head.store(0, Ordering::Release);
    header.sq_tail.store(0, Ordering::Release);
    header.cq_head.store(0, Ordering::Release);
    header.cq_tail.store(0, Ordering::Release);

    // --- Sub-test C: IO_OP_SERVICE_CONNECT ---
    // Register a test service, then connect to it via io_ring.
    let svc_name = b"io_ring_test_svc";
    let listener = service::register(svc_name)?;

    let sqe_connect = SqEntry {
        opcode: IO_OP_SERVICE_CONNECT,
        flags: 0,
        _pad0: [0; 2],
        _pad1: 0,
        user_data: 3000,
        handle: 0,
        addr: svc_name.as_ptr() as u64,
        len: svc_name.len() as u32,
        _pad2: 0,
        arg1: 0,
        arg2: 0,
    };
    // SAFETY: sq_base points to the SQ array; index 0 < sq_entries.
    unsafe {
        *sq_base.add(0) = sqe_connect;
    }
    header.sq_tail.store(1, Ordering::Release);

    let processed = enter(ring_handle, 0)?;
    if processed != 1 {
        serial_println!(
            "[io_ring]   FAIL: service_connect processed {}, expected 1",
            processed
        );
        let _ = service::unregister(listener);
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // SAFETY: cq_base points to the CQ array; index 0 is valid.
    let cqe = unsafe { &*cq_base.add(0) };
    if cqe.result < 0 {
        serial_println!(
            "[io_ring]   FAIL: service_connect result={} (error)",
            cqe.result
        );
        let _ = service::unregister(listener);
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // The result is the raw channel handle.  Accept the server side
    // and verify we can send a message across.
    let client_handle = channel::ChannelHandle::from_raw(cqe.result as u64);
    let server_handle = service::try_accept(listener)?.ok_or(KernelError::InternalError)?;

    // Send from client → server.
    let test_msg = b"hello from io_ring";
    let msg = channel::Message::from_bytes(test_msg).map_err(|_| KernelError::InternalError)?;
    channel::send(client_handle, msg)?;

    // Receive on server side.
    let received = channel::try_recv(server_handle)?.ok_or(KernelError::InternalError)?;
    if received.data() != test_msg {
        serial_println!("[io_ring]   FAIL: service message data mismatch");
        channel::close(client_handle);
        channel::close(server_handle);
        let _ = service::unregister(listener);
        destroy(ring_handle)?;
        return Err(KernelError::InternalError);
    }

    // Clean up.
    channel::close(client_handle);
    channel::close(server_handle);
    let _ = service::unregister(listener);
    destroy(ring_handle)?;

    serial_println!("[io_ring]   Timeout + service connect: OK");
    Ok(())
}
