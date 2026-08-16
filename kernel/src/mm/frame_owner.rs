//! Physical frame ownership tracker — "who allocated this frame?"
//!
//! Records the subsystem or call site that allocated each physical frame.
//! When memory usage is high and you need to understand *where* it went,
//! this module can answer: which subsystem owns the most frames?
//!
//! ## Design
//!
//! Each frame index (0..MAX_FRAMES) gets a compact 8-bit owner tag stored
//! in a flat array.  The tag identifies the subsystem that allocated the
//! frame (page tables, kernel stacks, DMA buffers, user pages, etc.).
//!
//! Tags are set at allocation time and cleared on free.  The overhead is
//! exactly 1 byte per frame, and the array is sized to *all* installed RAM:
//! it is carved from the frame allocator's metadata region in `frame::init`
//! (alongside `page_info` / `refcount` / the per-frame cgroup id) and
//! published here via [`init_storage`].  It is therefore not a fixed window
//! — a frame at any physical address is tracked.
//!
//! ## Owner Tags
//!
//! Tags are defined in [`Owner`] and cover all major allocation sources.
//! Unknown or untracked allocations get `Owner::Unknown`.
//!
//! ## Querying
//!
//! - `get(frame_idx)` → which subsystem owns this frame
//! - `summary()` → per-tag frame counts
//! - `find_by_owner(tag)` → iterator over frame indices with that tag
//!
//! ## Integration
//!
//! The frame allocator calls [`set`] after every successful allocation and
//! [`clear`] on every free.  The allocator has no idea *who* is asking, so
//! the tag comes from an ambient **owner context**: a subsystem wraps its
//! allocations in [`OwnerScope`], and every frame allocated while that guard
//! is alive is tagged with it.  Untagged allocations get [`Owner::Unknown`].
//!
//! [`OwnerScope`] saves and restores the previous tag on drop, so it nests
//! correctly — including when an interrupt handler allocates in the middle of
//! another subsystem's scope.
//!
//! ## References
//!
//! - Linux `mm/page_owner.c` — per-page allocation tracking
//! - Linux `include/linux/page_owner.h` — page_owner API

use crate::serial_println;
use crate::smp::MAX_CPUS;
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// The ownership array is sized dynamically to the machine's frame count in
// `frame::init` — see `init_storage`.  There is deliberately no MAX_FRAMES
// ceiling here; a fixed one used to silently drop every tag above 1 GiB
// (known-issues: TD-FRAME-OWNER-1GIB).

// ---------------------------------------------------------------------------
// Owner tag
// ---------------------------------------------------------------------------

/// Identifies which subsystem allocated a frame.
///
/// Each variant corresponds to a major allocation source in the kernel.
/// 8-bit representation keeps the per-frame overhead to 1 byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Owner {
    /// Frame is free (not allocated).
    Free = 0,
    /// Unknown or untagged allocation.
    Unknown = 1,
    /// Kernel page table structures (PML4, PDPT, PD, PT pages).
    PageTable = 2,
    /// Kernel heap slab allocator backing frames.
    HeapSlab = 3,
    /// Kernel stack frames (per-task stacks with guard pages).
    KernelStack = 4,
    /// DMA buffer allocation (physically contiguous, device-accessible).
    Dma = 5,
    /// User-space anonymous pages (demand paging, mmap).
    UserAnon = 6,
    /// User-space file-backed pages (page cache).
    UserFile = 7,
    /// Copy-on-Write source/destination frames.
    Cow = 8,
    /// Shared memory regions (IPC).
    SharedMem = 9,
    /// VMA metadata or internal bookkeeping.
    VmaMeta = 10,
    /// vmalloc backing frames (virtually-contiguous kernel allocations).
    Vmalloc = 11,
    /// Memory pool (mempool) reserved frames.
    Mempool = 12,
    /// Swap cache (frames awaiting or completing swap I/O).
    SwapCache = 13,
    /// Zero-page pool (pre-zeroed frames for fast demand paging).
    ZeroPool = 14,
    /// Huge page allocation (2 MiB / 128 frames).
    HugePage = 15,
    /// Filesystem buffer cache backing.
    FsCache = 16,
    /// Network buffer frames (packet data).
    NetBuffer = 17,
    /// Crypto / RNG scratch buffers.
    Crypto = 18,
    /// Boot-time allocations (before subsystems are initialized).
    Boot = 19,
    /// Self-test / benchmark temporary allocations.
    SelfTest = 20,
    /// Frame used for ACPI tables or firmware data.
    Firmware = 21,
    /// Framebuffer / display memory.
    Framebuffer = 22,
    /// Compressed page backing (zswap/zram).
    Compressed = 23,
}

impl Owner {
    /// Convert from raw u8 tag.
    #[inline]
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Free,
            1 => Self::Unknown,
            2 => Self::PageTable,
            3 => Self::HeapSlab,
            4 => Self::KernelStack,
            5 => Self::Dma,
            6 => Self::UserAnon,
            7 => Self::UserFile,
            8 => Self::Cow,
            9 => Self::SharedMem,
            10 => Self::VmaMeta,
            11 => Self::Vmalloc,
            12 => Self::Mempool,
            13 => Self::SwapCache,
            14 => Self::ZeroPool,
            15 => Self::HugePage,
            16 => Self::FsCache,
            17 => Self::NetBuffer,
            18 => Self::Crypto,
            19 => Self::Boot,
            20 => Self::SelfTest,
            21 => Self::Firmware,
            22 => Self::Framebuffer,
            23 => Self::Compressed,
            _ => Self::Unknown,
        }
    }

    /// Human-readable name for display.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Unknown => "unknown",
            Self::PageTable => "page_table",
            Self::HeapSlab => "heap_slab",
            Self::KernelStack => "kstack",
            Self::Dma => "dma",
            Self::UserAnon => "user_anon",
            Self::UserFile => "user_file",
            Self::Cow => "cow",
            Self::SharedMem => "shm",
            Self::VmaMeta => "vma_meta",
            Self::Vmalloc => "vmalloc",
            Self::Mempool => "mempool",
            Self::SwapCache => "swap_cache",
            Self::ZeroPool => "zero_pool",
            Self::HugePage => "hugepage",
            Self::FsCache => "fs_cache",
            Self::NetBuffer => "net_buf",
            Self::Crypto => "crypto",
            Self::Boot => "boot",
            Self::SelfTest => "selftest",
            Self::Firmware => "firmware",
            Self::Framebuffer => "framebuf",
            Self::Compressed => "compressed",
        }
    }

    /// Total number of defined owner tags.
    pub const COUNT: usize = 24;
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Per-frame owner tags.  Index = frame index, value = `Owner as u8`.
///
/// A flat byte array for O(1) lookup, carved from the frame allocator's
/// metadata region by `frame::init` and published through [`init_storage`].
/// `OWNERS_PTR` is its HHDM virtual base and `OWNERS_LEN` its length in
/// frames; both are written once during early boot and never change.
///
/// Before they are set every access no-ops (and [`get`] reports
/// [`Owner::Unknown`]), which is correct: nothing can have been recorded
/// yet.  Access needs no lock because alloc/free for a given frame index is
/// serialised by the allocator itself — a frame is either free or
/// exclusively owned, so only one CPU ever writes a given slot at a time.
/// Diagnostic reads are inherently racy, which is acceptable for statistics.
static OWNERS_PTR: AtomicU64 = AtomicU64::new(0);
static OWNERS_LEN: AtomicU64 = AtomicU64::new(0);

/// Publish the per-frame owner array.
///
/// Called once from `frame::init` with a pointer into the frame-allocator
/// metadata region, which has already been zeroed (`0` == [`Owner::Free`]).
///
/// # Safety
///
/// - `base` must point to at least `len` writable bytes that live for the
///   rest of the kernel's lifetime (the metadata region never moves).
/// - The region must already be zero-initialised.
/// - Must be called exactly once, during early single-threaded boot.
pub unsafe fn init_storage(base: *mut u8, len: usize) {
    // Store the base before the length so any reader that observes a
    // non-zero length is guaranteed to also observe a valid pointer.
    OWNERS_PTR.store(base as u64, Ordering::Release);
    OWNERS_LEN.store(len as u64, Ordering::Release);
}

/// Number of frames covered by the ownership array (0 before `init_storage`).
#[inline]
#[must_use]
pub fn tracked_frames() -> usize {
    #[allow(clippy::cast_possible_truncation)]
    let len = OWNERS_LEN.load(Ordering::Relaxed) as usize;
    len
}

/// Resolve the array base for `frame_idx`, or `None` if out of range or
/// the storage has not been published yet.
#[inline]
fn slot(frame_idx: usize) -> Option<*mut u8> {
    if frame_idx >= tracked_frames() {
        return None;
    }
    let base = OWNERS_PTR.load(Ordering::Relaxed);
    if base == 0 {
        return None;
    }
    // SAFETY: `frame_idx` is less than the published length, so `base +
    // frame_idx` lies inside the `len`-byte array carved in `frame::init`.
    Some(unsafe { (base as *mut u8).add(frame_idx) })
}

/// Whether frame ownership tracking is enabled.
///
/// Can be disabled at runtime to eliminate the overhead of set/clear
/// operations on every alloc/free.
static ENABLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Ambient owner context
// ---------------------------------------------------------------------------

/// One CPU's current owner tag, padded to its own cache line.
///
/// Padding matters because this is written on the allocation hot path: an
/// unpadded `[AtomicU8; MAX_CPUS]` would put every CPU's tag in one or two
/// cache lines and turn each `OwnerScope` into a false-sharing storm.
#[repr(align(64))]
struct CpuOwner(core::sync::atomic::AtomicU8);

/// Per-CPU ambient owner tag, consulted by the allocator on every alloc.
///
/// Defaults to [`Owner::Unknown`] (1) so untagged allocations are reported
/// as such rather than masquerading as `Free`.
static CURRENT_OWNER: [CpuOwner; MAX_CPUS] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: CpuOwner = CpuOwner(core::sync::atomic::AtomicU8::new(Owner::Unknown as u8));
    [INIT; MAX_CPUS]
};

/// The owner tag that frames allocated on this CPU are currently attributed to.
#[inline]
#[must_use]
pub fn current_owner() -> Owner {
    let cpu = crate::smp::fast_cpu_index();
    match CURRENT_OWNER.get(cpu) {
        Some(c) => Owner::from_u8(c.0.load(Ordering::Relaxed)),
        None => Owner::Unknown,
    }
}

/// Overwrite this CPU's ambient owner tag, returning the previous one.
#[inline]
fn swap_current_owner(owner: Owner) -> Owner {
    let cpu = crate::smp::fast_cpu_index();
    match CURRENT_OWNER.get(cpu) {
        Some(c) => Owner::from_u8(c.0.swap(owner as u8, Ordering::Relaxed)),
        None => Owner::Unknown,
    }
}

/// RAII guard that attributes every frame allocated in its scope to `owner`.
///
/// ```ignore
/// let _own = OwnerScope::new(Owner::PageTable);
/// let frame = frame::alloc_frame()?;   // tagged PageTable
/// ```
///
/// The guard saves the previous tag and restores it on drop, so scopes nest
/// correctly — including an interrupt handler that opens its own scope in the
/// middle of another subsystem's.
///
/// **Accuracy caveat.** The tag is per-CPU, so if a task is preempted and
/// migrated to another CPU mid-scope, the restore lands on the new CPU and
/// the old CPU keeps the tag until its next scope. That can mis-attribute a
/// handful of frames. This is accepted deliberately: ownership tracking is
/// diagnostic-only, and the alternative (a lock or a per-task field reachable
/// from boot and IRQ contexts) would cost more on the allocation hot path than
/// the precision is worth.
pub struct OwnerScope {
    previous: Owner,
}

impl OwnerScope {
    /// Begin attributing allocations on this CPU to `owner`.
    #[inline]
    #[must_use]
    pub fn new(owner: Owner) -> Self {
        Self {
            previous: swap_current_owner(owner),
        }
    }
}

impl Drop for OwnerScope {
    #[inline]
    fn drop(&mut self) {
        swap_current_owner(self.previous);
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// One CPU's diagnostic counters, padded to its own cache line.
///
/// These were a single pair of global `AtomicU64`s incremented with
/// `fetch_add` on every `set`/`clear` — that is, on **every frame allocation
/// and free in the system**. Two problems, and the file already knew about
/// both: `CURRENT_OWNER` a few lines up is padded per-CPU with a comment
/// explaining that an unpadded shared array on this path "would turn each
/// `OwnerScope` into a false-sharing storm". The counters sat on exactly the
/// same path, unpadded, in one cache line, and every CPU wrote them.
///
/// 1. **Cross-CPU contention.** A `lock`-prefixed RMW on one shared line from
///    every CPU on every alloc is the cache-line ping-pong CLAUDE.md's
///    performance rules name explicitly — for a counter nothing reads except
///    a diagnostic dump.
/// 2. **Emulation cost.** TCG cannot always lower a guest atomic RMW inline;
///    the fallback stops the world and re-executes the instruction, which is
///    thousands of cycles. Under the boot-test emulator these two increments
///    dominated the measured cost of ownership tagging.
///
/// Both go away by giving each CPU its own line.
#[repr(align(64))]
struct CpuStats {
    /// `set()` calls made by this CPU.
    sets: AtomicU64,
    /// `clear()` calls made by this CPU.
    clears: AtomicU64,
}

/// Per-CPU `set`/`clear` counters, summed on read by [`summary`].
static PER_CPU_STATS: [CpuStats; MAX_CPUS] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: CpuStats = CpuStats {
        sets: AtomicU64::new(0),
        clears: AtomicU64::new(0),
    };
    [INIT; MAX_CPUS]
};

/// Add one to a counter that only the running CPU writes.
///
/// This is deliberately a relaxed load-add-store rather than a `fetch_add`.
/// A read-modify-write would be atomic against an interrupt on this CPU, but
/// atomicity is not what is wanted here: the slot is per-CPU, so there is no
/// other *writer* to be atomic against, and the RMW's only remaining effect
/// is its cost (a bus lock on hardware, a potential stop-the-world re-execute
/// under TCG).
///
/// The accepted trade is that an interrupt landing between the load and the
/// store loses that increment. These are diagnostic counters for a `frame`
/// ownership dump; an occasional missed count is invisible, and paying an
/// atomic RMW on every frame allocation to avoid it is not a trade worth
/// making. Concurrent *readers* already tolerate staleness — `summary()` sums
/// the per-CPU slots without any snapshot guarantee.
#[inline]
fn bump(counter: &AtomicU64) {
    let prev = counter.load(Ordering::Relaxed);
    counter.store(prev.wrapping_add(1), Ordering::Relaxed);
}

/// Sum one per-CPU counter across every CPU.
///
/// Diagnostic-only: CPUs are read one at a time with no snapshot, so the
/// total can straddle concurrent updates. That is fine for a statistics dump
/// and is the reason the hot path gets to stay lock-free.
fn sum_per_cpu(select: fn(&CpuStats) -> &AtomicU64) -> u64 {
    let mut total: u64 = 0;
    for cpu in &PER_CPU_STATS {
        total = total.wrapping_add(select(cpu).load(Ordering::Relaxed));
    }
    total
}

/// Total `set()` calls across all CPUs since boot.
fn total_sets() -> u64 {
    sum_per_cpu(|c| &c.sets)
}

/// Total `clear()` calls across all CPUs since boot.
fn total_clears() -> u64 {
    sum_per_cpu(|c| &c.clears)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record ownership of a frame.
///
/// Called by the frame allocator after a successful allocation.  Out-of-range
/// indices and the pre-`init_storage` window are silently ignored, and this is
/// a no-op while tracking is disabled.
#[inline]
pub fn set(frame_idx: usize, owner: Owner) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(p) = slot(frame_idx) else { return };
    // SAFETY: `slot` returned an in-bounds pointer into the live ownership
    // array, and the caller has just allocated this frame, so no other CPU
    // writes this slot concurrently.
    unsafe {
        p.write(owner as u8);
    }
    if let Some(stats) = PER_CPU_STATS.get(crate::smp::fast_cpu_index()) {
        bump(&stats.sets);
    }
}

/// Clear ownership of a frame (mark as free).
///
/// Called by the frame allocator when a frame is freed.
#[inline]
pub fn clear(frame_idx: usize) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(p) = slot(frame_idx) else { return };
    // SAFETY: `slot` returned an in-bounds pointer into the live ownership
    // array, and the caller holds the frame exclusively while freeing it.
    unsafe {
        p.write(Owner::Free as u8);
    }
    if let Some(stats) = PER_CPU_STATS.get(crate::smp::fast_cpu_index()) {
        bump(&stats.clears);
    }
}

/// Query the owner of a specific frame.
///
/// Returns [`Owner::Unknown`] for an out-of-range index or before the
/// ownership array has been published.
#[inline]
#[must_use]
pub fn get(frame_idx: usize) -> Owner {
    let Some(p) = slot(frame_idx) else {
        return Owner::Unknown;
    };
    // SAFETY: `slot` returned an in-bounds pointer into the live ownership
    // array.  Concurrent writes by an allocating CPU are possible, but a
    // single-byte read is atomic on x86 and a torn/stale tag is acceptable
    // for a diagnostic.
    let raw = unsafe { p.read() };
    Owner::from_u8(raw)
}

/// Enable ownership tracking.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable ownership tracking (for performance-critical periods).
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether tracking is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Summary / reporting
// ---------------------------------------------------------------------------

/// Per-owner frame count summary.
#[derive(Debug, Clone)]
pub struct OwnerSummary {
    /// Frame count per owner tag.
    pub counts: [u32; Owner::COUNT],
    /// Total allocated (non-free) frames.
    pub total_allocated: u32,
    /// Total free frames (according to ownership tracking).
    pub total_free: u32,
    /// Total set() calls since boot.
    pub total_sets: u64,
    /// Total clear() calls since boot.
    pub total_clears: u64,
}

/// Compute a summary of frame ownership across all physical memory.
///
/// Scans the entire ownership array.  O(`tracked_frames`) — not for hot
/// paths, but fine for diagnostics (a byte scan of one array).
#[must_use]
pub fn summary() -> OwnerSummary {
    let mut counts = [0u32; Owner::COUNT];
    let mut total_free: u32 = 0;
    let mut total_allocated: u32 = 0;

    let len = tracked_frames();
    let base = OWNERS_PTR.load(Ordering::Relaxed) as *const u8;
    if base.is_null() {
        return OwnerSummary {
            counts,
            total_allocated: 0,
            total_free: 0,
            total_sets: total_sets(),
            total_clears: total_clears(),
        };
    }

    for i in 0..len {
        // SAFETY: `i < len`, the published length of the array at `base`.
        let raw = unsafe { base.add(i).read() };
        // An out-of-range byte can only mean a corrupted record; skip it
        // rather than indexing blindly.
        if let Some(slot) = counts.get_mut(raw as usize) {
            *slot = slot.saturating_add(1);
        }
        if raw == 0 {
            total_free = total_free.saturating_add(1);
        } else {
            total_allocated = total_allocated.saturating_add(1);
        }
    }

    OwnerSummary {
        counts,
        total_allocated,
        total_free,
        total_sets: total_sets(),
        total_clears: total_clears(),
    }
}

/// Find up to `limit` frame indices owned by the given tag.
///
/// Returns the actual count found (may be less than limit).
/// Useful for targeted investigation of a specific subsystem's usage.
pub fn find_by_owner(owner: Owner, buf: &mut [usize]) -> usize {
    let target = owner as u8;
    let mut found = 0;

    let len = tracked_frames();
    let base = OWNERS_PTR.load(Ordering::Relaxed) as *const u8;
    if base.is_null() {
        return 0;
    }

    for i in 0..len {
        if found >= buf.len() {
            break;
        }
        // SAFETY: `i < len`, the published length of the array at `base`.
        let raw = unsafe { base.add(i).read() };
        if raw == target {
            if let Some(dst) = buf.get_mut(found) {
                *dst = i;
            }
            found = found.saturating_add(1);
        }
    }
    found
}

/// Get the top N owners by frame count (sorted descending).
///
/// Returns an array of (Owner, count) pairs.  Useful for the kshell
/// command to show "who is using the most memory?"
#[must_use]
pub fn top_owners() -> [(Owner, u32); Owner::COUNT] {
    let s = summary();
    let mut result = [(Owner::Free, 0u32); Owner::COUNT];

    // `result` and `s.counts` are both `Owner::COUNT` long, so zip pairs
    // them exactly — no indexing needed.
    for (i, (dst, &count)) in result.iter_mut().zip(s.counts.iter()).enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let tag = Owner::from_u8(i as u8);
        *dst = (tag, count);
    }

    // Sort by count descending (simple insertion sort, N=24 is tiny).
    for i in 1..Owner::COUNT {
        let mut j = i;
        // `j` and `j - 1` are both < Owner::COUNT == result.len() here, but
        // read them through `get` so a future change to the bounds cannot
        // turn this into a panic.
        while j > 0 {
            let (Some(cur), Some(prev)) = (result.get(j), result.get(j.wrapping_sub(1))) else {
                break;
            };
            if cur.1 <= prev.1 {
                break;
            }
            result.swap(j, j.wrapping_sub(1));
            j = j.wrapping_sub(1);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for frame ownership tracking.
///
/// Note on methodology: the ownership array is now **live** — the frame
/// allocator writes it on every alloc/free — so this test must never scribble
/// on an arbitrary frame index, which would corrupt a real frame's record.
/// Every index it writes is either a frame it allocated itself (and frees
/// again) or is saved and restored around the check.
// `expect_used` is suppressed because this is a boot self-test: an allocation
// failing here means the frame allocator is broken, and panicking loudly at
// boot is the correct response — propagating the error would let a broken
// allocator boot silently.  The surrounding `assert!`s have the same intent.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::expect_used
)]
pub fn self_test() {
    use crate::mm::frame;

    serial_println!("[frame_owner] Running self-test...");

    // Test 1: storage was published by frame::init and covers ALL installed
    // RAM.  This is the regression test for TD-FRAME-OWNER-1GIB: the array
    // used to be a fixed 65536 frames (= 1 GiB), silently dropping every tag
    // above that.
    let tracked = tracked_frames();
    assert!(
        tracked > 0,
        "ownership array must be published by frame::init"
    );
    if let Some(st) = frame::stats() {
        assert_eq!(
            tracked, st.total_frames,
            "ownership array must cover every frame the allocator manages"
        );
        serial_println!(
            "[frame_owner]   Covers all {} frames ({} MiB): OK",
            tracked,
            (tracked * frame::FRAME_SIZE) / (1024 * 1024)
        );
    }

    // Test 2: a frame index beyond the old fixed 1 GiB window round-trips.
    // Skipped on machines with <= 1 GiB, where there is no such index.
    const OLD_CEILING: usize = 65536; // the historical MAX_FRAMES
    if tracked > OLD_CEILING {
        let high = tracked - 1;
        let saved = get(high);
        set(high, Owner::Dma);
        assert_eq!(
            get(high),
            Owner::Dma,
            "frame above the old 1 GiB ceiling must be tracked"
        );
        set(high, saved); // restore the real record
        assert_eq!(get(high), saved);
        serial_println!(
            "[frame_owner]   High frame {} (> old {}-frame window): OK",
            high,
            OLD_CEILING
        );
    } else {
        serial_println!(
            "[frame_owner]   High-frame test skipped ({tracked} frames <= {OLD_CEILING})"
        );
    }

    // Test 3: out-of-bounds is inert — no panic, no wild write.
    assert_eq!(get(tracked), Owner::Unknown, "OOB get must report Unknown");
    assert_eq!(get(tracked + 1_000_000), Owner::Unknown);
    set(tracked, Owner::Dma); // must be a no-op
    clear(tracked); // must be a no-op
    serial_println!("[frame_owner]   Out-of-bounds safety: OK");

    // Test 4: end-to-end wiring — a frame allocated inside an OwnerScope is
    // tagged with that owner, and freeing it clears the tag.  This is the
    // test that actually proves `set`/`clear` are reached from the allocator
    // (they used to have no callers at all).
    {
        let frame_a = {
            let _scope = OwnerScope::new(Owner::SelfTest);
            frame::alloc_frame().expect("self-test frame alloc must succeed")
        };
        let idx = (frame_a.addr() / frame::FRAME_SIZE as u64) as usize;
        assert_eq!(
            get(idx),
            Owner::SelfTest,
            "allocation inside an OwnerScope must be tagged with it"
        );
        // SAFETY: `frame_a` was just allocated here and no mapping or
        // reference to its memory was ever created, so freeing it is sound.
        unsafe {
            frame::free_frame(frame_a).expect("self-test frame free must succeed");
        }
        assert_eq!(get(idx), Owner::Free, "freeing a frame must clear its tag");
        serial_println!("[frame_owner]   Alloc/free tagging round-trip: OK");
    }

    // Test 5: OwnerScope nests and restores the previous tag on drop.
    assert_eq!(current_owner(), Owner::Unknown, "default tag is Unknown");
    {
        let _outer = OwnerScope::new(Owner::PageTable);
        assert_eq!(current_owner(), Owner::PageTable);
        {
            let _inner = OwnerScope::new(Owner::KernelStack);
            assert_eq!(current_owner(), Owner::KernelStack);
        }
        assert_eq!(
            current_owner(),
            Owner::PageTable,
            "inner scope must restore the outer tag"
        );
    }
    assert_eq!(
        current_owner(),
        Owner::Unknown,
        "outermost scope must restore the default tag"
    );
    serial_println!("[frame_owner]   OwnerScope nesting: OK");

    // Test 6: summary() and find_by_owner() see a live tag.
    {
        let frame_b = {
            let _scope = OwnerScope::new(Owner::Crypto);
            frame::alloc_frame().expect("self-test frame alloc must succeed")
        };
        let idx = (frame_b.addr() / frame::FRAME_SIZE as u64) as usize;

        let s = summary();
        assert!(
            s.counts[Owner::Crypto as usize] >= 1,
            "summary must count the Crypto-tagged frame"
        );
        assert!(
            s.total_allocated >= 1,
            "summary must report allocated frames"
        );

        let mut buf = [0usize; 8];
        let found = find_by_owner(Owner::Crypto, &mut buf);
        assert!(found >= 1, "find_by_owner must locate the tagged frame");
        assert!(
            buf[..found].contains(&idx),
            "find_by_owner must return the frame we tagged"
        );

        // SAFETY: `frame_b` was just allocated here and is unmapped and
        // unreferenced, so freeing it is sound.
        unsafe {
            frame::free_frame(frame_b).expect("self-test frame free must succeed");
        }
        serial_println!("[frame_owner]   summary/find_by_owner: OK");
    }

    // Test 7: top_owners is sorted descending.  Most frames are unallocated,
    // so `Free` should lead.
    let top = top_owners();
    for w in top.windows(2) {
        assert!(w[0].1 >= w[1].1, "top_owners must be sorted descending");
    }
    assert_eq!(top[0].0, Owner::Free, "Free should have the most frames");
    serial_println!("[frame_owner]   top_owners sorted: OK");

    // Test 8: disable() suppresses updates; enable() restores them.  Use a
    // frame we own so a suppressed update cannot corrupt a live record.
    {
        let frame_c = frame::alloc_frame().expect("self-test frame alloc must succeed");
        let idx = (frame_c.addr() / frame::FRAME_SIZE as u64) as usize;
        let before = get(idx);

        disable();
        set(idx, Owner::Boot);
        assert_eq!(get(idx), before, "set must be a no-op while disabled");
        enable();
        set(idx, Owner::Boot);
        assert_eq!(get(idx), Owner::Boot, "set must work once re-enabled");

        // SAFETY: `frame_c` was just allocated here and is unmapped and
        // unreferenced, so freeing it is sound.  The free also restores the
        // tag to `Free`.
        unsafe {
            frame::free_frame(frame_c).expect("self-test frame free must succeed");
        }
        serial_println!("[frame_owner]   Enable/disable toggle: OK");
    }

    // Test 9: statistics moved, proving the allocator hit set() and clear().
    let s2 = summary();
    assert!(
        s2.total_sets > 0,
        "allocator must have recorded set() calls"
    );
    assert!(
        s2.total_clears > 0,
        "allocator must have recorded clear() calls"
    );
    serial_println!(
        "[frame_owner]   Stats: sets={}, clears={}",
        s2.total_sets,
        s2.total_clears
    );

    serial_println!("[frame_owner] Self-test PASSED");
}
