//! Virtio split virtqueue implementation.
//!
//! A virtqueue consists of three regions in physically contiguous memory:
//!
//! 1. **Descriptor table** — array of `VirtqDesc` (16 bytes each)
//! 2. **Available ring** — header + array of descriptor indices
//! 3. **Used ring** — header + array of (id, len) completion entries
//!
//! The legacy transport uses a page-frame-number register to tell the
//! device where the queue lives (physical address >> 12).  Since our
//! frame allocator provides 16 KiB physically contiguous frames (which
//! are 4 KiB aligned), a single frame allocation satisfies the
//! contiguity and alignment requirements.

use core::sync::atomic::{AtomicU32, Ordering, fence};

use crate::ada;
use crate::error::KernelResult;
use crate::mm::frame::{self, PhysFrame};

// ---------------------------------------------------------------------------
// Ada descriptor-pool slots
// ---------------------------------------------------------------------------
//
// The SPARK component in kernel/ada/ owns a fixed array of queue records, and
// a live `Virtqueue` owns exactly one of them for as long as it exists.  This
// bitmap is the allocator for that array: bit N set means slot N is spoken for.
//
// It is a plain atomic bitmap rather than anything cleverer because the whole
// population is 16 and the operation happens at driver-probe time, never on an
// I/O path.

/// One bit per slot in the Ada queue pool.
static QUEUE_SLOTS: AtomicU32 = AtomicU32::new(0);

/// Mask of the bits in [`QUEUE_SLOTS`] that correspond to real pool slots.
const SLOT_MASK: u32 = 0xFFFF;

// The mask above is written as a literal so it is obvious at a glance, which
// makes it something that could silently stop matching the Ada side.  Tie the
// two together at compile time instead: raising Max_Queues in the Ada spec
// without widening this mask would otherwise leave the extra slots permanently
// unreachable, which is a bug that shows up only as an unexplained
// ResourceExhausted at probe time.
const _: () = assert!(
    ada::MAX_QUEUES == 16,
    "SLOT_MASK must cover exactly ada::MAX_QUEUES bits"
);

/// Claim a free slot in the Ada queue pool, or `None` if all are in use.
fn acquire_queue_id() -> Option<u16> {
    let mut cur = QUEUE_SLOTS.load(Ordering::Relaxed);
    loop {
        let free = !cur & SLOT_MASK;
        if free == 0 {
            return None;
        }
        let bit = free.trailing_zeros();
        match QUEUE_SLOTS.compare_exchange_weak(
            cur,
            cur | (1u32 << bit),
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            // `bit` indexes a set bit of SLOT_MASK, so it is below 16 and the
            // cast cannot truncate.
            #[allow(clippy::cast_possible_truncation)]
            Ok(_) => return Some(bit as u16),
            Err(actual) => cur = actual,
        }
    }
}

/// Return a slot to the pool.  The caller must have reset it in Ada first.
fn release_queue_id(id: u16) {
    QUEUE_SLOTS.fetch_and(!(1u32 << id), Ordering::AcqRel);
}

// ---------------------------------------------------------------------------
// Virtqueue descriptor
// ---------------------------------------------------------------------------

/// Descriptor flags.
pub const VRING_DESC_F_NEXT: u16 = 1; // Descriptor chains to next.
pub const VRING_DESC_F_WRITE: u16 = 2; // Device writes (vs. reads).

/// Available-ring flag: "do not interrupt me when you consume a buffer".
///
/// Set in the avail ring's `flags` field (offset 0).  The field is zero after
/// the frame is zeroed, and zero means *interrupts wanted* — so a driver that
/// never registers a handler has to say so explicitly, or the device will
/// assert its IRQ line for completions nobody will ever service.
pub const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;

/// A single virtqueue descriptor (16 bytes, repr(C) for device compatibility).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqDesc {
    /// Physical address of the buffer.
    pub addr: u64,
    /// Length of the buffer in bytes.
    pub len: u32,
    /// Flags (NEXT, WRITE, INDIRECT).
    pub flags: u16,
    /// Index of the next descriptor if NEXT flag is set.
    pub next: u16,
}

// ---------------------------------------------------------------------------
// Available ring
// ---------------------------------------------------------------------------

/// Available ring header (4 bytes) + entries.
///
/// Layout in memory:
/// ```text
/// offset 0: flags (u16)
/// offset 2: idx (u16) — incremented by driver after adding entries
/// offset 4: ring[0..queue_size] (u16 each) — descriptor head indices
/// ```
#[repr(C)]
#[allow(dead_code)]
pub struct VirtqAvailHeader {
    pub flags: u16,
    pub idx: u16,
}

// ---------------------------------------------------------------------------
// Used ring
// ---------------------------------------------------------------------------

/// One entry in the used ring.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqUsedElem {
    /// Index of the head descriptor of the completed chain.
    pub id: u32,
    /// Total bytes written by the device.
    pub len: u32,
}

/// Used ring header (4 bytes) + entries.
#[repr(C)]
#[allow(dead_code)]
pub struct VirtqUsedHeader {
    pub flags: u16,
    pub idx: u16,
}

// ---------------------------------------------------------------------------
// Virtqueue
// ---------------------------------------------------------------------------

/// A virtio split virtqueue.
pub struct Virtqueue {
    /// Physical frame backing the queue memory.
    phys_frame: PhysFrame,
    /// Virtual base address (via HHDM).
    virt_base: *mut u8,
    /// Number of descriptors.
    queue_size: u16,
    /// Byte offset of the available ring from virt_base.
    avail_offset: usize,
    /// Byte offset of the used ring from virt_base.
    used_offset: usize,
    /// Slot in the Ada descriptor pool that owns this queue's free list and
    /// chain topology.
    ///
    /// The free list used to live in the `next` fields of the descriptor
    /// table — that is, in memory the device can write.  A device that
    /// scribbled there could redirect our next allocation anywhere.  The
    /// authoritative copy now lives in the SPARK component, which the device
    /// cannot reach, and the descriptor table became a write-only rendering
    /// of it.  See known-issues.md B-VIRTIO-UNVALIDATED-USED-ID.
    queue_id: u16,
    /// Driver's copy of the available ring index (what we've submitted).
    avail_idx: u16,
    /// Last used ring index we've seen.
    last_used_idx: u16,
    /// Whether this queue has asked the device not to raise interrupts.
    ///
    /// Kept here, rather than being written once at setup, because [`reset`]
    /// zeroes the whole backing frame — including the avail ring's flags
    /// field.  Without a remembered preference a device reset would silently
    /// re-arm interrupts on a queue whose driver has no handler for them.
    suppress_interrupts: bool,
}

impl Virtqueue {
    /// Return the number of descriptors in this queue.
    pub fn queue_size(&self) -> u16 {
        self.queue_size
    }

    /// Return the physical base address of this queue's backing memory.
    ///
    /// Needed by the modern virtio transport to set descriptor/avail/used
    /// ring addresses separately.
    pub fn phys_addr(&self) -> u64 {
        self.phys_frame.addr()
    }

    /// Allocate and initialize a virtqueue.
    ///
    /// Allocates physically contiguous memory from the frame allocator,
    /// zeroes it, and sets up the free descriptor list.
    ///
    /// Returns the queue and its physical page frame number (for the
    /// legacy transport's Queue Address register).
    // Queue layout arithmetic uses small values that fit in usize.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn new(queue_size: u16, hhdm_offset: u64) -> KernelResult<(Self, u32)> {
        let qs = queue_size as usize;

        // Compute offsets per the virtio spec.
        let desc_size = qs * 16; // 16 bytes per descriptor
        let avail_size = 4 + (qs * 2) + 2; // header + ring + used_event
        let avail_end = desc_size + avail_size;
        let used_start = align_up(avail_end, 4096); // Used ring is page-aligned
        let used_size = 4 + (qs * 8); // header + used_elem array
        let total = used_start + used_size;

        // Verify it fits in a single 16 KiB frame.
        if total > frame::FRAME_SIZE {
            return Err(crate::error::KernelError::InvalidArgument);
        }

        // Claim a slot in the Ada descriptor pool and let the proved component
        // decide whether this size is one it will accept.  It rejects 0, sizes
        // above its compile-time maximum, and sizes that are not a power of
        // two — the last of which the virtio spec requires and this code never
        // checked.  Doing it here means a device advertising a nonsensical
        // queue size fails at probe time rather than producing a queue whose
        // index arithmetic is subtly wrong.
        let queue_id = acquire_queue_id().ok_or(crate::error::KernelError::ResourceExhausted)?;

        if let Err(status) = ada::initialize(queue_id, queue_size) {
            release_queue_id(queue_id);
            crate::serial_println!(
                "[virtio] descriptor pool rejected queue size {queue_size}: {status:?}"
            );
            return Err(crate::error::KernelError::InvalidArgument);
        }

        // Allocate one 16 KiB frame.  From here on, every early return has to
        // give the pool slot back, so the failure paths are written out rather
        // than using `?`.
        let phys = match frame::alloc_frame() {
            Ok(p) => p,
            Err(e) => {
                ada::reset(queue_id);
                release_queue_id(queue_id);
                return Err(e);
            }
        };
        let phys_addr = phys.addr();
        let virt = phys_addr + hhdm_offset;
        let virt_ptr = virt as *mut u8;

        // Zero the entire frame.
        // SAFETY: We just allocated this frame; the HHDM maps it as
        // writable kernel memory.
        unsafe {
            core::ptr::write_bytes(virt_ptr, 0, frame::FRAME_SIZE);
        }

        // No free-list construction here any more.  The list the driver walks
        // lives in the Ada pool, which `initialize` above has already built;
        // the `next` fields in this table are now written only when a chain is
        // submitted, and are read only by the device.

        // Physical PFN for the legacy transport (4096-byte granularity).
        let pfn = (phys_addr >> 12) as u32;

        let vq = Self {
            phys_frame: phys,
            virt_base: virt_ptr,
            queue_size,
            avail_offset: desc_size,
            used_offset: used_start,
            queue_id,
            avail_idx: 0,
            last_used_idx: 0,
            suppress_interrupts: false,
        };

        Ok((vq, pfn))
    }

    /// Ask the device not to raise an interrupt when it consumes buffers from
    /// this queue.
    ///
    /// Call this on every queue whose driver completes work by polling
    /// [`poll_used`] and never registers an IRQ handler — virtio-gpu and
    /// virtio-sound are both entirely poll-driven.  The avail ring's flags
    /// field is zero after `new` zeroes the frame, and zero means *interrupts
    /// wanted*, so a polling driver that stays silent gets an IRQ line
    /// asserted for completions it will never acknowledge.  On a
    /// level-triggered PCI INTx line, which stays asserted until someone
    /// acknowledges it at the device, that is not merely wasteful.
    ///
    /// This is advisory in both directions: the spec permits a device to
    /// interrupt anyway, and a device that ignores the hint is still handled
    /// correctly because the driver polls regardless.  It removes the
    /// *request*, not the possibility.
    ///
    /// The preference is remembered so [`reset`] can restore it — see
    /// `suppress_interrupts`.
    pub fn set_no_interrupt(&mut self) {
        self.suppress_interrupts = true;
        self.write_avail_flags();
    }

    /// Write the avail ring's flags field from `suppress_interrupts`.
    ///
    /// Shared by [`set_no_interrupt`] and [`reset`] so the two cannot drift.
    fn write_avail_flags(&mut self) {
        let flags = if self.suppress_interrupts {
            VRING_AVAIL_F_NO_INTERRUPT
        } else {
            0
        };
        // SAFETY: `avail_offset` is `queue_size * 16`, the size of the
        // descriptor table, so it lands at the start of the available ring
        // within the exclusively-owned frame.  The ring's first field is its
        // 2-byte `flags` at offset 0, so this writes entirely inside the
        // avail region — it is strictly below the `4 + ring_slot * 2` entry
        // arithmetic elsewhere in this file, which is already established to
        // be in bounds.  The HHDM maps the frame as writable kernel memory,
        // and the write is volatile because the device reads this field.
        unsafe {
            let avail_flags = self.virt_base.add(self.avail_offset).cast::<u16>();
            core::ptr::write_volatile(avail_flags, flags);
        }
        // The device may read the flags field at any time; make the write
        // visible before whatever the caller does next (publishing the queue
        // to the device, or submitting the first buffer).
        fence(Ordering::SeqCst);
    }

    /// Reset the virtqueue to its freshly-initialized state.
    ///
    /// Re-zeroes the descriptor table and both rings, rebuilds the free
    /// descriptor list, and clears the avail/used index tracking — the
    /// same state produced by [`new`].  Reuses the existing backing frame,
    /// so the caller must re-publish the queue to the device (via the
    /// transport's queue-PFN register) after a device reset.
    ///
    /// Used by drivers to recover after a request times out: a timed-out
    /// request leaves descriptors and DMA buffers owned by the device, so
    /// the queue's free list and used-ring accounting are no longer safe
    /// to reuse.  Resetting the device (which drops all outstanding
    /// buffers) and then resetting the queue restores a consistent state.
    // Queue layout arithmetic uses small values that fit in usize.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn reset(&mut self) {
        // Zero the entire frame (descriptor table + avail ring + used ring).
        // SAFETY: virt_base is the start of our exclusively-owned frame,
        // FRAME_SIZE bytes long.
        unsafe {
            core::ptr::write_bytes(self.virt_base, 0, frame::FRAME_SIZE);
        }

        // Rebuild the free list in the Ada pool.  `initialize` is idempotent
        // and is the documented reset path, so this both returns every
        // descriptor the device still nominally owned and re-establishes the
        // invariants the component's proof rests on.
        //
        // If it fails the queue is left with size 0 in the pool, which makes
        // every subsequent allocation and completion reject — the queue goes
        // inert rather than silently reverting to unchecked behaviour.  It can
        // only fail if `queue_size` is unacceptable, which `new` already
        // established it is not.
        if let Err(status) = ada::initialize(self.queue_id, self.queue_size) {
            crate::serial_println!(
                "[virtio] queue {} failed to re-initialize on reset: {status:?}; \
                 queue is now inert",
                self.queue_id
            );
            ada::reset(self.queue_id);
        }

        self.avail_idx = 0;
        self.last_used_idx = 0;

        // The frame zeroing above cleared the avail ring's flags field, so a
        // queue that had asked not to be interrupted has just silently asked
        // to be interrupted again.  Restore the driver's stated preference.
        self.write_avail_flags();
    }

    /// Allocate a descriptor from the free list.
    ///
    /// The index comes from the SPARK component, whose postcondition is that
    /// the result is either "none" or a genuine index into this queue.  That
    /// is what justifies the pointer arithmetic in [`desc_mut`].
    fn alloc_desc(&mut self) -> Option<u16> {
        ada::allocate(self.queue_id)
    }

    /// Free a descriptor as part of rolling back a chain that was never
    /// submitted.
    ///
    /// `allocate` hands back a descriptor already marked as the end of its
    /// chain, so one that has been allocated but not yet linked to a successor
    /// is a well-formed one-element chain, and freeing it is [`free_chain`] on
    /// itself.
    ///
    /// Rollback calls this for every index it allocated, which may include
    /// indices that a *previous* call in the same loop already freed as part
    /// of a longer partial chain — if links 0→1→2 were established before the
    /// failure, freeing index 0 takes 1 and 2 with it. The later calls are
    /// then no-ops: the pool sees a descriptor that is not allocated and
    /// rejects, changing nothing. That is why this goes straight to the pool
    /// instead of through [`free_chain`], which would log each of those
    /// expected rejections as though a device had misbehaved.
    fn free_desc(&mut self, idx: u16) {
        // The count is deliberately discarded. A zero here means "not
        // allocated", which during rollback is the expected outcome for an
        // index a previous iteration already freed as part of a longer partial
        // chain — see above — and never indicates a failure this function
        // could act on.
        let _freed = ada::free_chain(self.queue_id, idx);
    }

    /// Free the chain of descriptors whose head is `head`.
    ///
    /// Returns the number of descriptors returned to the free list; **0 means
    /// the chain was rejected and nothing changed.**
    ///
    /// `head` normally comes from the used ring, which the *device* writes, so
    /// it is attacker-controlled. The validation lives in the SPARK component
    /// rather than here: it rejects an index outside this queue, an index that
    /// is not currently allocated (a double completion), and a chain that
    /// fails to terminate within the queue's own length (a cycle). The walk
    /// follows the pool's private links, so a device that rewrites `next` in
    /// the descriptor table cannot steer it.
    pub fn free_chain(&mut self, head: u16) -> u16 {
        let freed = ada::free_chain(self.queue_id, head);
        if freed == 0 {
            // Reaching here means the device named a chain we do not believe we
            // own. Nothing was mutated, so the queue is still consistent, but
            // it is worth saying out loud: on a correct device it is
            // unreachable, so it means either a device bug or an attempt to
            // walk us off the descriptor table.
            crate::serial_println!(
                "[virtio] queue {}: rejected completion for descriptor {head} \
                 (out of range, not allocated, or a cyclic chain)",
                self.queue_id
            );
        }
        freed
    }

    /// Read the physical address stored in descriptor `idx`.
    ///
    /// Used by drivers to identify which DMA buffer a completed
    /// descriptor chain belongs to — e.g., mapping the `head_idx`
    /// returned by [`poll_used`] back to a buffer slot index.
    ///
    /// Returns `None` if `idx` is not a descriptor of this queue.
    ///
    /// Must be called **before** [`free_chain`], which returns the descriptor
    /// to the free list.
    pub fn desc_phys_addr(&self, idx: u16) -> Option<u64> {
        self.desc(idx).map(|d| d.addr)
    }

    /// Get a reference to descriptor `idx`, or `None` if it is out of range.
    ///
    /// The bound is checked here rather than assumed from the caller. The
    /// previous version asserted in a SAFETY comment that callers only ever
    /// passed indices from `alloc_desc` — which was false, because
    /// `free_chain` reached this function with an index the device chose, and
    /// nothing in the type system or the call graph made the comment true.
    /// A bounds check at the dereference is what actually makes it true.
    fn desc(&self, idx: u16) -> Option<&VirtqDesc> {
        if idx >= self.queue_size {
            return None;
        }
        // SAFETY: idx < queue_size, just checked, and queue_size * 16 is
        // within the frame (established in new(), which refuses any size whose
        // layout does not fit). virt_base points at that exclusively-owned
        // frame, mapped writable through the HHDM, and the descriptor table
        // starts at offset 0 of it.
        Some(unsafe { &*(self.virt_base.add(idx as usize * 16) as *const VirtqDesc) })
    }

    /// Get a mutable reference to descriptor `idx`, or `None` if out of range.
    fn desc_mut(&mut self, idx: u16) -> Option<&mut VirtqDesc> {
        if idx >= self.queue_size {
            return None;
        }
        // SAFETY: as desc(), plus `&mut self` gives us exclusive access.
        Some(unsafe { &mut *(self.virt_base.add(idx as usize * 16) as *mut VirtqDesc) })
    }

    /// Submit a chain of buffers to the available ring.
    ///
    /// `buffers` is a slice of `(physical_addr, length, flags)` tuples.
    /// The descriptors are chained via NEXT flags.
    ///
    /// Returns the head descriptor index (needed to identify the
    /// completion in the used ring).
    // Chain arithmetic uses wrapping ops; descriptor indices are small.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn submit(&mut self, buffers: &[(u64, u32, u16)]) -> KernelResult<u16> {
        if buffers.is_empty() {
            return Err(crate::error::KernelError::InvalidArgument);
        }

        // Allocate descriptors for the chain.
        let mut indices = [0u16; 16]; // Max 16 buffers per request.
        let count = buffers.len().min(16);
        for i in 0..count {
            match self.alloc_desc() {
                Some(idx) => indices[i] = idx,
                None => {
                    // Free already-allocated descriptors.
                    for j in 0..i {
                        self.free_desc(indices[j]);
                    }
                    return Err(crate::error::KernelError::WouldBlock);
                }
            }
        }

        // Record the chain's shape in the Ada pool, which is what `free_chain`
        // will walk on completion.  This is done before touching the
        // descriptor table so that a failure leaves nothing published.
        //
        // These cannot fail: every index came from `allocate` on this queue and
        // so is in range and marked allocated, which is exactly what `link`
        // requires.  Checked anyway — the point of the component is that the
        // caller's belief about an index is never what makes the access safe.
        for i in 0..count.saturating_sub(1) {
            if let Err(status) = ada::link(self.queue_id, indices[i], indices[i + 1]) {
                crate::serial_println!(
                    "[virtio] queue {}: link {} -> {} rejected: {status:?}",
                    self.queue_id,
                    indices[i],
                    indices[i + 1]
                );
                for j in 0..count {
                    self.free_desc(indices[j]);
                }
                return Err(crate::error::KernelError::InternalError);
            }
        }

        // Fill in the descriptors the device reads.  From here the table is a
        // rendering of state the pool already holds authoritatively.
        for i in 0..count {
            let Some(desc) = self.desc_mut(indices[i]) else {
                // Unreachable: `allocate` guarantees the index is below the
                // queue size.  Treated as a failure rather than ignored,
                // because the alternative is submitting a chain with a
                // descriptor we never filled in.
                crate::serial_println!(
                    "[virtio] queue {}: allocator returned out-of-range descriptor {}",
                    self.queue_id,
                    indices[i]
                );
                for j in 0..count {
                    self.free_desc(indices[j]);
                }
                return Err(crate::error::KernelError::InternalError);
            };
            desc.addr = buffers[i].0;
            desc.len = buffers[i].1;
            desc.flags = buffers[i].2;
            if i + 1 < count {
                desc.flags |= VRING_DESC_F_NEXT;
                desc.next = indices[i + 1];
            }
        }

        // Memory fence: ensure descriptor writes are visible before
        // updating the available ring.
        fence(Ordering::SeqCst);

        // Add the head to the available ring.
        // SAFETY for the avail_ring pointer arithmetic below:
        // avail_offset = desc_table_size = queue_size * 16, which is within
        // the allocated frame.  The available ring is: 2-byte flags, 2-byte
        // idx, then queue_size × 2-byte entries.  ring_slot < queue_size,
        // so 4 + ring_slot * 2 stays within the avail region.  The frame is
        // exclusively owned and the HHDM maps it as writable kernel memory.
        let avail_ring_base = unsafe { self.virt_base.add(self.avail_offset) };

        // Ring entry offset: 4 (header) + (avail_idx % queue_size) * 2.
        let ring_slot = (self.avail_idx % self.queue_size) as usize;
        let entry_ptr = unsafe { avail_ring_base.add(4 + ring_slot * 2) as *mut u16 };
        // SAFETY: entry_ptr is within the available ring (see above).
        unsafe {
            core::ptr::write_volatile(entry_ptr, indices[0]);
        }

        // Memory fence before updating avail idx.
        fence(Ordering::SeqCst);

        // Increment the available ring index.
        self.avail_idx = self.avail_idx.wrapping_add(1);
        // SAFETY: avail_ring_base + 2 = the idx field of the available ring
        // header, within the same allocated frame.
        let avail_idx_field = unsafe { avail_ring_base.add(2) as *mut u16 };
        unsafe {
            core::ptr::write_volatile(avail_idx_field, self.avail_idx);
        }

        // Another fence to ensure the index update is visible before
        // the device is notified.
        fence(Ordering::SeqCst);

        Ok(indices[0])
    }

    /// Poll the used ring for completed requests.
    ///
    /// Returns `Some((head_idx, bytes_written))` if a request completed,
    /// `None` if no new completions.
    // Index arithmetic wraps; used ring accesses use small offsets.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn poll_used(&mut self) -> Option<(u16, u32)> {
        // SAFETY for the used_ring pointer arithmetic below:
        // used_offset is page-aligned within the allocated frame (computed
        // in new()).  The used ring is: 2-byte flags, 2-byte idx, then
        // queue_size × 8-byte VirtqUsedElem entries.  ring_slot < queue_size,
        // so 4 + ring_slot * 8 stays within the used region.  The frame is
        // exclusively owned.  Volatile reads are necessary because the
        // device writes the used ring asynchronously.
        let used_ring_base = unsafe { self.virt_base.add(self.used_offset) };
        let device_used_idx =
            unsafe { core::ptr::read_volatile(used_ring_base.add(2) as *const u16) };

        if self.last_used_idx == device_used_idx {
            return None; // No new completions.
        }

        // Read the used ring entry.
        let ring_slot = (self.last_used_idx % self.queue_size) as usize;
        let elem_ptr = unsafe { used_ring_base.add(4 + ring_slot * 8) as *const VirtqUsedElem };
        let elem = unsafe { core::ptr::read_volatile(elem_ptr) };

        self.last_used_idx = self.last_used_idx.wrapping_add(1);

        // `elem.id` is written by the device and is the point at which
        // device-controlled data becomes an index into our own descriptor
        // table.  Validate it here, at the boundary, rather than leaving each
        // driver to remember: every caller of this function immediately uses
        // the value to look something up, and there are ten such call sites
        // across four drivers.  Rejecting once, here, is what makes all of them
        // safe.
        //
        // Note the id is declared u32 in the ring and truncating it would map
        // 0x1_0000 onto descriptor 0 — a valid-looking index for a completion
        // the device never legitimately made — so the width check comes first.
        let Ok(id) = u16::try_from(elem.id) else {
            crate::serial_println!(
                "[virtio] queue {}: device reported used id {} (>= 2^16); ignoring",
                self.queue_id,
                elem.id
            );
            return None;
        };

        if !ada::is_allocated(self.queue_id, id) {
            // Out of range for this queue, or in range but not currently
            // outstanding — a completion for a chain we already freed, or one
            // we never submitted.  Either way there is no buffer to hand back.
            crate::serial_println!(
                "[virtio] queue {}: device completed descriptor {id}, which is \
                 not outstanding; ignoring",
                self.queue_id
            );
            return None;
        }

        Some((id, elem.len))
    }
}

impl Drop for Virtqueue {
    fn drop(&mut self) {
        // Return the descriptor pool slot.  `reset` puts the queue back to
        // size 0, in which every operation on it rejects, so a stale index
        // arriving for a queue that has gone away — a device being unplugged
        // mid-flight is exactly when that happens — is answered as invalid
        // rather than against whatever driver claims the slot next.
        ada::reset(self.queue_id);
        release_queue_id(self.queue_id);

        // Free the backing frame.
        // SAFETY: We own this frame and are being dropped.
        if let Err(e) = unsafe { frame::free_frame(self.phys_frame) } {
            crate::serial_println!("[virtio] WARNING: failed to free virtqueue frame: {:?}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Align `value` up to the next multiple of `align`.
///
/// `align` must be a power of two.
// Small alignment arithmetic.
#[allow(clippy::arithmetic_side_effects)]
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}
