//! The Rust half of the Ada/SPARK FFI bridge.
//!
//! `design.txt` (lines 78-100) puts safety-critical driver logic in Ada/SPARK
//! and everything else in Rust: *Application → syscall → Rust kernel → FFI →
//! SPARK driver logic → device → DMA → IOMMU*. This module is the "FFI" arrow.
//! It is the only place in the kernel that declares an Ada symbol, so the whole
//! surface of the boundary is one file you can read in a sitting.
//!
//! # What lives on which side, and why
//!
//! The split is not "hard parts in Ada". It is **indices and topology in Ada,
//! addresses and memory in Rust**:
//!
//! | Ada owns | Rust owns |
//! |---|---|
//! | which descriptor index is free | where the descriptor table is mapped |
//! | which index chains to which | the volatile writes into it |
//! | whether a device-supplied index is real | MMIO, DMA, the IOMMU |
//!
//! The Ada side never dereferences anything and has no notion of an address.
//! That is what makes it provable: `gnatprove` discharges absence of run-time
//! errors over the whole index space, and there is no pointer for it to be
//! wrong about. Rust keeps every unsafe operation, but each one is now
//! justified by a *proved* bound rather than by a comment asserting one — see
//! `known-issues.md` → `B-VIRTIO-UNVALIDATED-USED-ID` for what happened when it
//! was only a comment.
//!
//! # The unconstrained boundary is deliberate
//!
//! Every Ada entry point takes plain `u16`, never a constrained subtype, and
//! validates in its own body. A constrained parameter would push the range
//! check onto the *caller*, and SPARK would then discharge the callee's proof
//! by assuming the caller complies — an assumption this Rust code never agreed
//! to and a hostile device has no reason to honour. Taking the wide type puts
//! the check on the side that can be proved to perform it, which means the
//! signatures below need no preconditions and cannot be misused from here.
//!
//! # Runtime contract
//!
//! The Ada objects are compiled against a Zero-FootPrint profile whose entire
//! runtime is one `system.ads` (`kernel/ada/rts/`), so they leave exactly one
//! symbol undefined: `__gnat_last_chance_handler`, exported below. See
//! `design-decisions.md` §205 for why ZFP rather than a light runtime, and
//! `kernel/build.rs` for how the objects reach the link.

#![allow(dead_code)] // Entry points land as drivers are migrated to them.

use core::ffi::c_char;

/// Result of an Ada operation that reports a status rather than a value.
///
/// Mirrors the `Status_*` constants in `virtqueue_descriptors.ads`. The
/// discriminants are pinned to the Ada values, and [`selftest`] round-trips
/// every one of them through the Ada side so drift between the two definitions
/// fails the boot rather than silently remapping errors onto each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VqStatus {
    /// The operation succeeded.
    Ok = 0,
    /// Queue id out of range, or the queue was never initialised.
    BadQueue = 1,
    /// Size was 0, above the compiled-in maximum, or not a power of two.
    BadSize = 2,
    /// Descriptor index is at or beyond the queue's size.
    BadIndex = 3,
    /// The index names a descriptor that is not currently in use.
    NotAllocated = 4,
    /// No free descriptors remain.
    Exhausted = 5,
    /// The Ada side returned a status this enum does not know about. Reaching
    /// this means the two definitions have diverged; it is a distinct variant
    /// rather than a panic so a driver can fail its request instead of taking
    /// the machine down, and [`selftest`] is what turns the divergence into a
    /// boot-time failure where it belongs.
    Unknown = 0xFF,
}

impl VqStatus {
    fn from_raw(v: u8) -> Self {
        match v {
            0 => Self::Ok,
            1 => Self::BadQueue,
            2 => Self::BadSize,
            3 => Self::BadIndex,
            4 => Self::NotAllocated,
            5 => Self::Exhausted,
            _ => Self::Unknown,
        }
    }

    /// `Ok` as a `Result`, so call sites can use `?`.
    ///
    /// # Errors
    /// Returns `self` unchanged when it is not [`VqStatus::Ok`].
    pub fn ok(self) -> Result<(), Self> {
        if self == Self::Ok { Ok(()) } else { Err(self) }
    }
}

/// Returned by [`allocate`] and used by the Ada side as the chain terminator.
///
/// Equal to the virtio specification's terminator, and outside any queue's
/// index range by construction, so it can never collide with a real descriptor.
pub const NO_DESCRIPTOR: u16 = 0xFFFF;

/// Largest queue size the Ada side is compiled for. Kept in step with
/// `Max_Descriptors` by [`selftest`], which asks the Ada side to initialise a
/// queue of exactly this size and rejects the build if it refuses.
pub const MAX_DESCRIPTORS: u16 = 256;

/// Number of queues the Ada side is compiled for (`Max_Queues`).
pub const MAX_QUEUES: u16 = 16;

// The Ada externs are only present when linking for the kernel target, because
// only that build links the x86-64 ELF objects (see `kernel/build.rs`). A host
// build of this crate would otherwise fail at link time with an undefined
// symbol that says nothing about the cause.
#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Zero the Ada package's state. Emitted by GNAT as the package's
    /// elaboration body.
    ///
    /// The table is in `.bss` and the loader already zeroes it, so this is
    /// redundant today — and is called anyway, because "redundant" is a
    /// property of the current loader and not of this package. A component
    /// that silently depends on someone else's zeroing breaks when that
    /// someone changes.
    #[link_name = "virtqueue_descriptors___elabb"]
    fn vqd_elaborate();

    fn vqd_size_of(queue: u16) -> u16;
    fn vqd_free_count(queue: u16) -> u16;
    fn vqd_is_allocated(queue: u16, index: u16) -> u8;
    fn vqd_initialize(queue: u16, size: u16, status: *mut u8);
    fn vqd_reset(queue: u16);
    fn vqd_allocate(queue: u16, index: *mut u16);
    fn vqd_link(queue: u16, from: u16, to: u16, status: *mut u8);
    fn vqd_terminate_chain(queue: u16, index: u16, status: *mut u8);
    fn vqd_free_chain(queue: u16, head: u16, freed: *mut u16);
}

// ---------------------------------------------------------------------------
// Safe wrappers.
//
// Each is a thin call plus a type change. They are `unsafe` blocks only because
// FFI is unsafe by declaration, not because anything here can go wrong: the Ada
// side takes unconstrained scalars, writes exactly one `out` scalar through the
// pointer we hand it, touches no memory of ours, and cannot fail. There is no
// aliasing to reason about (the pointers are to our own stack locals, live for
// the duration of the call, and are not retained), and no allocation.
// ---------------------------------------------------------------------------

/// Number of descriptors in `queue`; 0 if the id is invalid or the queue has
/// not been initialised.
///
/// 0 makes every other operation reject, so an uninitialised queue fails
/// closed. Callers can therefore treat this as "how many, and is it usable"
/// in one question.
#[must_use]
pub fn size_of(queue: u16) -> u16 {
    // SAFETY: `vqd_size_of` is a pure read of Ada-private state. It takes an
    // unconstrained u16 and validates internally (proved), so no value of
    // `queue` is out of contract.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_size_of(queue)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = queue;
        0
    }
}

/// How many descriptors of `queue` are currently free.
///
/// A statistic for backpressure decisions, not a value that becomes an index —
/// the Ada side deliberately proves only `<= Max_Descriptors` for it.
#[must_use]
pub fn free_count(queue: u16) -> u16 {
    // SAFETY: as `size_of` — a validated pure read of Ada-private state.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_free_count(queue)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = queue;
        0
    }
}

/// Is `index` a currently-allocated descriptor of `queue`?
#[must_use]
pub fn is_allocated(queue: u16, index: u16) -> bool {
    // SAFETY: as `size_of`. The Ada postcondition proves the result is 0 or 1.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_is_allocated(queue, index) != 0
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (queue, index);
        false
    }
}

/// Initialise `queue` with `size` descriptors, all free.
///
/// Also the reset path: virtio drivers reset a queue after a request times
/// out, because a timed-out request leaves descriptors owned by the device.
/// Idempotent.
///
/// # Errors
/// [`VqStatus::BadQueue`] if `queue` is out of range;
/// [`VqStatus::BadSize`] if `size` is 0, above [`MAX_DESCRIPTORS`], or not a
/// power of two. The power-of-two requirement is virtio's, and the ring-index
/// masking on this side depends on it — rejecting it here is what lets the
/// rest of the driver stop wondering.
pub fn initialize(queue: u16, size: u16) -> Result<(), VqStatus> {
    let mut status: u8 = VqStatus::Unknown as u8;
    // SAFETY: `&mut status` is a live, aligned, exclusively-owned `u8` for the
    // duration of the call; the Ada side writes it once as an `out` parameter
    // and does not retain it.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_initialize(queue, size, &raw mut status);
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (queue, size);
    }
    VqStatus::from_raw(status).ok()
}

/// Return `queue` to the uninitialised state, in which every operation on it
/// rejects.
///
/// This is device teardown, not "initialise with size 0" — that is refused,
/// because a driver asking for a zero-sized queue has a bug. Call it when a
/// device goes away, so a completion naming one of its stale descriptor
/// indices is answered as the nonsense it now is rather than against a queue
/// that still looks live.
///
/// Idempotent, and accepts any `u16`: resetting a queue id that never existed
/// is a no-op rather than an error, because every caller of this is on a
/// teardown path where there is nothing useful to do with a failure.
pub fn reset(queue: u16) {
    // SAFETY: takes an unconstrained u16, validates internally (proved), and
    // writes only Ada-private state.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_reset(queue);
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = queue;
    }
}

/// Take a descriptor off `queue`'s free list.
///
/// Returns `None` if the queue id is invalid, the queue is uninitialised, or
/// nothing is free. The caller cannot distinguish those and does not need to:
/// all three mean "do not submit".
///
/// **The returned index is proved to be a genuine index into this queue.** It
/// is what justifies the pointer arithmetic in `virtio::queue`.
#[must_use]
pub fn allocate(queue: u16) -> Option<u16> {
    let mut index: u16 = NO_DESCRIPTOR;
    // SAFETY: as `initialize` — one `out` scalar into our own live local.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_allocate(queue, &raw mut index);
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = queue;
    }
    if index == NO_DESCRIPTOR {
        None
    } else {
        Some(index)
    }
}

/// Record that descriptor `from` chains to descriptor `to`.
///
/// The link is stored in Ada-private memory, *not* in the device-visible
/// descriptor table. That is the substantive difference from the code this
/// replaces: the structure deciding which memory to touch next is no longer
/// somewhere a device can edit it.
///
/// # Errors
/// [`VqStatus::BadQueue`], [`VqStatus::BadIndex`] if either index is beyond
/// the queue, or [`VqStatus::NotAllocated`] if either end is not in use.
pub fn link(queue: u16, from: u16, to: u16) -> Result<(), VqStatus> {
    let mut status: u8 = VqStatus::Unknown as u8;
    // SAFETY: as `initialize`.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_link(queue, from, to, &raw mut status);
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (queue, from, to);
    }
    VqStatus::from_raw(status).ok()
}

/// Mark `index` as the last descriptor of its chain.
///
/// # Errors
/// [`VqStatus::BadQueue`], [`VqStatus::BadIndex`] or
/// [`VqStatus::NotAllocated`].
pub fn terminate_chain(queue: u16, index: u16) -> Result<(), VqStatus> {
    let mut status: u8 = VqStatus::Unknown as u8;
    // SAFETY: as `initialize`.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_terminate_chain(queue, index, &raw mut status);
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (queue, index);
    }
    VqStatus::from_raw(status).ok()
}

/// Free the chain whose head is `head`, returning its descriptors to the free
/// list. Returns how many were freed.
///
/// **`0` means the chain was rejected and nothing changed.**
///
/// This is the entry point fed device-controlled data: `head` comes from the
/// used ring, which the device writes. Each way it can be wrong is answered by
/// the Ada side rather than by the caller — out of range, in range but not
/// allocated (a double completion), or a chain longer than the queue (a
/// cycle). All three return 0 having mutated nothing, because the Ada side
/// validates the whole chain before freeing any of it; a partial free would
/// leave the queue in a state that is neither the old one nor a good one,
/// after being handed input the device chose.
#[must_use]
pub fn free_chain(queue: u16, head: u16) -> u16 {
    let mut freed: u16 = 0;
    // SAFETY: as `initialize`.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_free_chain(queue, head, &raw mut freed);
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (queue, head);
    }
    freed
}

/// Run the Ada package's elaboration.
///
/// Must be called once, before any other function here. Calling it again
/// resets every queue to uninitialised, which is harmless but pointless.
pub fn init() {
    // SAFETY: GNAT's elaboration body for this package only zeroes the
    // package's own `.bss` state. It takes no arguments, touches nothing else,
    // and is idempotent.
    #[cfg(target_os = "none")]
    unsafe {
        vqd_elaborate();
    }
}

// ---------------------------------------------------------------------------
// The one symbol the Ada side needs from us.
// ---------------------------------------------------------------------------

/// GNAT's last-chance handler: where a failed run-time check lands.
///
/// # Why this exists when the code is proved
///
/// `gnatprove` discharges every run-time check in the Ada sources, so on paper
/// nothing can reach here. The checks are compiled in *anyway* and routed to a
/// kernel panic, because the proof is a statement about the code and this is a
/// kernel — an unrelated wild write can land in the middle of the Ada package's
/// arrays, and then a check that "cannot fail" is the only thing between a
/// corrupted index and a corrupted machine. Suppressing the checks
/// (`-gnatp`) would buy a few compares and throw that away.
///
/// So reaching this handler does not mean the proof was wrong. It means memory
/// that the proof assumed only this package writes was written by something
/// else — which is exactly the kind of failure a kernel must stop on rather
/// than continue through.
///
/// # Panics
/// Always. That is its entire job.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn __gnat_last_chance_handler(source_location: *const c_char, line: i32) -> ! {
    // The Ada runtime passes a NUL-terminated source file name and a line
    // number. Read it defensively: we are already in a failure path, and a
    // corrupt pointer here would turn a diagnosable panic into a triple fault.
    let file = if source_location.is_null() {
        "<null>"
    } else {
        // SAFETY: GNAT emits this pointer as a pointer to a static
        // NUL-terminated string in .rodata. The length scan is bounded at 128
        // so a corrupted pointer cannot walk the address space, and the result
        // is only used for a diagnostic.
        unsafe {
            let mut len = 0usize;
            while len < 128 && *source_location.add(len) != 0 {
                len = len.saturating_add(1);
            }
            let bytes = core::slice::from_raw_parts(source_location.cast::<u8>(), len);
            core::str::from_utf8(bytes).unwrap_or("<non-utf8>")
        }
    };

    // clippy::panic is denied tree-wide because a panic is normally a way of
    // not handling an error. Here it *is* the handling: GNAT calls this when a
    // run-time check has already failed, the function is `-> !` by the Ada
    // runtime's contract, and there is no caller to return an error to. Routing
    // it into the Rust panic path is what gets the operator a message and a
    // backtrace instead of an immediate triple fault.
    #[allow(clippy::panic)]
    {
        panic!("Ada run-time check failed at {file}:{line} (see kernel/src/ada.rs)");
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise the FFI boundary at boot.
///
/// # What this is actually for
///
/// Not to test the Ada logic — `gnatprove` covers that far better than any test
/// could, over inputs no test supplies. This checks the things a proof cannot
/// see, all of which are properties of the *boundary*:
///
/// * that the objects linked at all, and the symbols resolve;
/// * that the calling convention agrees, including `out` parameters written
///   through pointers;
/// * that [`VqStatus`]'s discriminants still match the Ada `Status_*`
///   constants — the failure mode being silent remapping of one error onto
///   another, which no compiler catches across an FFI boundary;
/// * that [`MAX_DESCRIPTORS`] and [`MAX_QUEUES`] still match the Ada
///   compile-time bounds.
///
/// Every check below is one of those. Where it exercises behaviour, it does so
/// because the behaviour is the only observable that pins a constant.
///
/// # Errors
/// A static description of the first check that failed.
pub fn selftest() -> Result<(), &'static str> {
    init();

    // --- Status round-trip. Each status must be reachable and must arrive as
    // the variant this side names, or errors are being silently remapped.
    if initialize(MAX_QUEUES, 16) != Err(VqStatus::BadQueue) {
        return Err("Ada: queue id at Max_Queues was not rejected as BadQueue");
    }
    if initialize(0, 0) != Err(VqStatus::BadSize) {
        return Err("Ada: size 0 was not rejected as BadSize");
    }
    if initialize(0, 3) != Err(VqStatus::BadSize) {
        return Err("Ada: non-power-of-two size was not rejected as BadSize");
    }
    if initialize(0, MAX_DESCRIPTORS + 1) != Err(VqStatus::BadSize) {
        return Err("Ada: size above Max_Descriptors was not rejected as BadSize");
    }

    // --- The compile-time bounds. MAX_DESCRIPTORS must be accepted and one
    // more rejected (above); MAX_QUEUES-1 must be a usable id and MAX_QUEUES
    // must not be. Together these pin both constants exactly.
    initialize(0, MAX_DESCRIPTORS).map_err(|_| "Ada: Max_Descriptors was rejected as a size")?;
    if size_of(0) != MAX_DESCRIPTORS {
        return Err("Ada: MAX_DESCRIPTORS disagrees with the Ada Max_Descriptors");
    }
    initialize(MAX_QUEUES - 1, 8).map_err(|_| "Ada: highest queue id was rejected")?;
    if size_of(MAX_QUEUES - 1) != 8 {
        return Err("Ada: MAX_QUEUES disagrees with the Ada Max_Queues");
    }

    // --- A working queue, to pin the remaining statuses and prove `out`
    // parameters come back at all.
    const Q: u16 = 1;
    initialize(Q, 8).map_err(|_| "Ada: initialize of a valid queue failed")?;
    if size_of(Q) != 8 || free_count(Q) != 8 {
        return Err("Ada: a freshly initialised queue is not fully free");
    }

    let a = allocate(Q).ok_or("Ada: allocate returned nothing from a full free list")?;
    let b = allocate(Q).ok_or("Ada: second allocate returned nothing")?;
    if a >= 8 || b >= 8 || a == b {
        return Err("Ada: allocate returned an out-of-range or duplicate index");
    }
    if !is_allocated(Q, a) || free_count(Q) != 6 {
        return Err("Ada: allocate did not update the free list");
    }
    if link(Q, a, 8) != Err(VqStatus::BadIndex) {
        return Err("Ada: link to an out-of-range index was not BadIndex");
    }
    // `b + 1` is in range but was never allocated, unless b is the last index,
    // in which case wrap to 0. `b < 8` holds from the range check above.
    let unallocated = if b < 7 { b.saturating_add(1) } else { 0 };
    if unallocated != a && unallocated != b && !is_allocated(Q, unallocated) {
        if link(Q, a, unallocated) != Err(VqStatus::NotAllocated) {
            return Err("Ada: link to a free descriptor was not NotAllocated");
        }
    }

    // --- The bug this component exists to fix. A device-supplied head that is
    // out of range, or in range but never issued, must be rejected *totally*:
    // rejected means nothing moved, so free_count is unchanged afterwards.
    link(Q, a, b).map_err(|_| "Ada: link of two allocated descriptors failed")?;
    terminate_chain(Q, b).map_err(|_| "Ada: terminate_chain failed")?;

    let before = free_count(Q);
    if free_chain(Q, NO_DESCRIPTOR) != 0 {
        return Err("Ada: an out-of-range chain head was not rejected");
    }
    if free_chain(Q, 8) != 0 {
        return Err("Ada: a head at the queue size was not rejected");
    }
    if free_count(Q) != before {
        return Err("Ada: a rejected free_chain still mutated the queue");
    }

    // The real chain frees, and frees exactly its own length.
    if free_chain(Q, a) != 2 {
        return Err("Ada: freeing a two-descriptor chain did not report 2");
    }
    if free_count(Q) != 8 {
        return Err("Ada: freeing the chain did not return both descriptors");
    }
    // ...and freeing it again is a double completion, which must be refused.
    if free_chain(Q, a) != 0 {
        return Err("Ada: a double free_chain of the same head was not rejected");
    }
    if free_count(Q) != 8 {
        return Err("Ada: a rejected double free still mutated the free count");
    }

    // --- Reset returns a queue to fail-closed, which is the only way back.
    reset(Q);
    if size_of(Q) != 0 {
        return Err("Ada: reset left the queue with a non-zero size");
    }
    if allocate(Q).is_some() {
        return Err("Ada: a reset queue still allocated a descriptor");
    }
    if free_chain(Q, 0) != 0 {
        return Err("Ada: a reset queue still accepted a chain head");
    }
    reset(MAX_QUEUES); // out of range: must be a no-op, not a fault

    // Leave no state behind. Every id this test touched goes back to
    // uninitialised, so a driver that later forgets to initialise one gets a
    // rejection rather than inheriting a queue the self-test half-used.
    for q in [0u16, Q, MAX_QUEUES - 1] {
        reset(q);
    }
    Ok(())
}
