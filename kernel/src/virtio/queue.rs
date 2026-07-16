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

use core::sync::atomic::{fence, Ordering};

use crate::error::KernelResult;
use crate::mm::frame::{self, PhysFrame};

// ---------------------------------------------------------------------------
// Virtqueue descriptor
// ---------------------------------------------------------------------------

/// Descriptor flags.
pub const VRING_DESC_F_NEXT: u16 = 1;      // Descriptor chains to next.
pub const VRING_DESC_F_WRITE: u16 = 2;     // Device writes (vs. reads).

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
    /// Next free descriptor index (head of free list).
    free_head: u16,
    /// Number of free descriptors.
    free_count: u16,
    /// Driver's copy of the available ring index (what we've submitted).
    avail_idx: u16,
    /// Last used ring index we've seen.
    last_used_idx: u16,
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
        let desc_size = qs * 16;                         // 16 bytes per descriptor
        let avail_size = 4 + (qs * 2) + 2;               // header + ring + used_event
        let avail_end = desc_size + avail_size;
        let used_start = align_up(avail_end, 4096);       // Used ring is page-aligned
        let used_size = 4 + (qs * 8);                     // header + used_elem array
        let total = used_start + used_size;

        // Verify it fits in a single 16 KiB frame.
        if total > frame::FRAME_SIZE {
            return Err(crate::error::KernelError::InvalidArgument);
        }

        // Allocate one 16 KiB frame.
        let phys = frame::alloc_frame()?;
        let phys_addr = phys.addr();
        let virt = phys_addr + hhdm_offset;
        let virt_ptr = virt as *mut u8;

        // Zero the entire frame.
        // SAFETY: We just allocated this frame; the HHDM maps it as
        // writable kernel memory.
        unsafe {
            core::ptr::write_bytes(virt_ptr, 0, frame::FRAME_SIZE);
        }

        // Initialize the free descriptor list (each descriptor's `next`
        // points to the following one).
        for i in 0..queue_size {
            // SAFETY: virt_ptr is the start of a zeroed, owned frame.
            // i < queue_size, and queue_size * 16 < FRAME_SIZE (checked
            // indirectly via total < FRAME_SIZE above), so the pointer
            // arithmetic stays within the allocated frame.
            let desc = unsafe { &mut *(virt_ptr.add(i as usize * 16) as *mut VirtqDesc) };
            desc.next = if i + 1 < queue_size { i + 1 } else { 0xFFFF };
        }

        // Physical PFN for the legacy transport (4096-byte granularity).
        let pfn = (phys_addr >> 12) as u32;

        let vq = Self {
            phys_frame: phys,
            virt_base: virt_ptr,
            queue_size,
            avail_offset: desc_size,
            used_offset: used_start,
            free_head: 0,
            free_count: queue_size,
            avail_idx: 0,
            last_used_idx: 0,
        };

        Ok((vq, pfn))
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

        // Rebuild the free descriptor list (each `next` points to the
        // following descriptor; the last terminates the list).
        for i in 0..self.queue_size {
            // SAFETY: i < queue_size, and queue_size * 16 < FRAME_SIZE
            // (checked in new()), so the pointer stays within the frame.
            let desc = unsafe { &mut *(self.virt_base.add(i as usize * 16) as *mut VirtqDesc) };
            desc.next = if i + 1 < self.queue_size { i + 1 } else { 0xFFFF };
        }

        self.free_head = 0;
        self.free_count = self.queue_size;
        self.avail_idx = 0;
        self.last_used_idx = 0;
    }

    /// Allocate a descriptor from the free list.
    fn alloc_desc(&mut self) -> Option<u16> {
        if self.free_count == 0 {
            return None;
        }
        let idx = self.free_head;
        let desc = self.desc(idx);
        self.free_head = desc.next;
        self.free_count = self.free_count.wrapping_sub(1);
        Some(idx)
    }

    /// Free a descriptor (return it to the free list).
    fn free_desc(&mut self, idx: u16) {
        let old_head = self.free_head;
        let desc = self.desc_mut(idx);
        desc.next = old_head;
        desc.flags = 0;
        self.free_head = idx;
        self.free_count = self.free_count.wrapping_add(1);
    }

    /// Free a chain of descriptors starting from `head`.
    pub fn free_chain(&mut self, head: u16) {
        let mut idx = head;
        loop {
            let desc = self.desc(idx);
            let next = desc.next;
            let has_next = desc.flags & VRING_DESC_F_NEXT != 0;
            self.free_desc(idx);
            if has_next {
                idx = next;
            } else {
                break;
            }
        }
    }

    /// Read the physical address stored in descriptor `idx`.
    ///
    /// Used by drivers to identify which DMA buffer a completed
    /// descriptor chain belongs to — e.g., mapping the `head_idx`
    /// returned by [`poll_used`] back to a buffer slot index.
    ///
    /// Must be called **before** [`free_chain`], which overwrites
    /// descriptor metadata.
    pub fn desc_phys_addr(&self, idx: u16) -> u64 {
        self.desc(idx).addr
    }

    /// Get a reference to descriptor `idx`.
    fn desc(&self, idx: u16) -> &VirtqDesc {
        // SAFETY: idx is within 0..queue_size (ensured by alloc_desc).
        unsafe { &*(self.virt_base.add(idx as usize * 16) as *const VirtqDesc) }
    }

    /// Get a mutable reference to descriptor `idx`.
    fn desc_mut(&mut self, idx: u16) -> &mut VirtqDesc {
        // SAFETY: same as desc() — idx is within 0..queue_size
        // (ensured by alloc_desc), and we have exclusive access (&mut self).
        unsafe { &mut *(self.virt_base.add(idx as usize * 16) as *mut VirtqDesc) }
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

        // Fill in the descriptors.
        for i in 0..count {
            let desc = self.desc_mut(indices[i]);
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
        let entry_ptr = unsafe {
            avail_ring_base.add(4 + ring_slot * 2) as *mut u16
        };
        // SAFETY: entry_ptr is within the available ring (see above).
        unsafe { core::ptr::write_volatile(entry_ptr, indices[0]); }

        // Memory fence before updating avail idx.
        fence(Ordering::SeqCst);

        // Increment the available ring index.
        self.avail_idx = self.avail_idx.wrapping_add(1);
        // SAFETY: avail_ring_base + 2 = the idx field of the available ring
        // header, within the same allocated frame.
        let avail_idx_field = unsafe {
            avail_ring_base.add(2) as *mut u16
        };
        unsafe { core::ptr::write_volatile(avail_idx_field, self.avail_idx); }

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
        let device_used_idx = unsafe {
            core::ptr::read_volatile(used_ring_base.add(2) as *const u16)
        };

        if self.last_used_idx == device_used_idx {
            return None; // No new completions.
        }

        // Read the used ring entry.
        let ring_slot = (self.last_used_idx % self.queue_size) as usize;
        let elem_ptr = unsafe {
            used_ring_base.add(4 + ring_slot * 8) as *const VirtqUsedElem
        };
        let elem = unsafe { core::ptr::read_volatile(elem_ptr) };

        self.last_used_idx = self.last_used_idx.wrapping_add(1);

        Some((elem.id as u16, elem.len))
    }
}

impl Drop for Virtqueue {
    fn drop(&mut self) {
        // Free the backing frame.
        // SAFETY: We own this frame and are being dropped.
        if let Err(e) = unsafe { frame::free_frame(self.phys_frame) } {
            crate::serial_println!(
                "[virtio] WARNING: failed to free virtqueue frame: {:?}",
                e
            );
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
