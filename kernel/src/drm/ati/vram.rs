//! Suballocation of the card's video memory.
//!
//! ## Why this exists rather than reusing the system allocator
//!
//! A GEM buffer on this card wants to live in VRAM, not in system RAM. That is
//! the whole point of having a real display driver: a scanout buffer that is
//! already in video memory is displayed by writing its address to
//! `CRTC_OFFSET`, whereas one in system RAM has to be copied there every frame.
//! The first is a page flip; the second is what
//! [`crate::drm::driver::LimineBackend`] does because it has no other option.
//!
//! VRAM cannot be handed out by the buddy allocator, which owns system frames
//! and would be told to manage memory that is not RAM and cannot hold kernel
//! data structures. So this module manages the card's memory separately, in
//! byte offsets from the base of VRAM.
//!
//! That separation has a sharp edge worth naming, because it is the bug this
//! module exists to make impossible: a VRAM-resident [`crate::drm::gem::GemObject`]
//! carries `PhysFrame`s pointing into the BAR0 aperture, and `free_backing`
//! returns `phys_frames` to the buddy allocator. Freeing such an object through
//! the ordinary path would hand card memory to the system allocator, which then
//! satisfies a kernel allocation out of it — silent corruption rather than a
//! leak. VRAM-backed objects must be released through [`VramAllocator::free`].
//!
//! ## Why a free list rather than a bump allocator
//!
//! A display driver allocates few, large, long-lived buffers, and the lazy
//! reading of that is that a bump allocator is enough — nothing is ever freed
//! in the steady state. It is enough right up to the first mode change, which
//! frees a 1920x1080 scanout buffer and allocates a differently-sized one; a
//! bump allocator answers that by leaking the first, and 16 MiB of VRAM
//! tolerates about seven such changes before it is exhausted. The failure is
//! then an out-of-memory error on a resolution the card can obviously display,
//! arriving an unpredictable number of mode-sets after the code that caused it.
//!
//! First-fit over a coalesced free list is a few dozen lines more and does not
//! have that failure. It is also entirely decidable without hardware, so
//! [`super::tests`] checks it on every boot.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

/// A contiguous run of video memory: a byte offset from the base of VRAM, and a
/// length.
///
/// Offsets rather than addresses, because that is what the CRTC's
/// `CRTC_OFFSET` register takes and what a caller needs in order to compute one
/// — and because a type that cannot name an address outside VRAM cannot be used
/// to point the scanout engine at system RAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VramRegion {
    /// Byte offset from the base of video memory.
    pub start: u32,
    /// Length in bytes. Never zero in the free list — a zero-length region
    /// carries no information and would defeat the coalescing invariant, since
    /// it is adjacent to everything.
    pub len: u32,
}

impl VramRegion {
    /// One past the last byte of the region.
    ///
    /// # Errors
    ///
    /// `InternalError` if the region's end overflows, which cannot happen for a
    /// region this allocator produced — every one is bounded by `total`.
    fn end(self) -> KernelResult<u32> {
        self.start
            .checked_add(self.len)
            .ok_or(KernelError::InternalError)
    }
}

/// Round `value` up to a multiple of `align`.
///
/// `align` must be a power of two. Returns `None` on overflow rather than
/// wrapping to a small value, which would produce an allocation that appears to
/// fit and points at the wrong end of memory.
const fn align_up(value: u32, align: u32) -> Option<u32> {
    let mask = align.wrapping_sub(1);
    match value.checked_add(mask) {
        Some(v) => Some(v & !mask),
        None => None,
    }
}

/// A first-fit allocator over a card's video memory.
///
/// ## Invariants
///
/// The free list is the canonical description of what is free, which requires
/// all three of:
///
/// 1. sorted by `start`, strictly ascending;
/// 2. no two entries overlap;
/// 3. no two entries are *adjacent* — a free that touches a neighbour merges
///    with it.
///
/// (3) is what makes the representation canonical rather than merely correct:
/// without it the same free set has many representations, `largest_free` under-
/// reports, and a request that would fit is refused because the space is split
/// across two list entries that are in fact one run. It is also what makes a
/// double free detectable, since a correct free never *overlaps* an existing
/// entry.
pub struct VramAllocator {
    /// Total size of the managed region in bytes.
    total: u32,
    /// Free runs, maintained under the invariants above.
    free: Vec<VramRegion>,
}

impl VramAllocator {
    /// Create an allocator over `total` bytes of video memory, all free.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if `total` is zero. A card reporting no video memory
    /// has not been configured, and an allocator over nothing would refuse
    /// every request with an error that says "out of memory" when the truth is
    /// "this device was never initialised".
    pub fn new(total: u32) -> KernelResult<Self> {
        if total == 0 {
            return Err(KernelError::InvalidArgument);
        }
        let free = alloc::vec![VramRegion {
            start: 0,
            len: total,
        }];
        Ok(Self { total, free })
    }

    /// Total size of the managed region, in bytes.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// Bytes currently free, summed over the free list.
    #[must_use]
    pub fn free_bytes(&self) -> u32 {
        self.free
            .iter()
            .fold(0u32, |acc, r| acc.saturating_add(r.len))
    }

    /// The largest single allocation that could succeed at alignment 1.
    ///
    /// Reported separately from [`Self::free_bytes`] because the difference
    /// between the two *is* the fragmentation, and a driver that fails to
    /// allocate 8 MiB with 12 MiB free needs to be able to say so.
    #[must_use]
    pub fn largest_free(&self) -> u32 {
        self.free.iter().map(|r| r.len).max().unwrap_or(0)
    }

    /// Number of runs in the free list. Diagnostic; see [`Self::largest_free`].
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.free.len()
    }

    /// Allocate `len` bytes aligned to `align`, returning the byte offset.
    ///
    /// First fit rather than best fit. Best fit would leave smaller remainders,
    /// but the workload here is a handful of large buffers whose sizes are set
    /// by the display mode, not a stream of small ones, so the packing quality
    /// of the two is indistinguishable and first fit is the one whose behaviour
    /// is obvious from reading it.
    ///
    /// # Errors
    ///
    /// - `InvalidArgument` if `len` is zero, or `align` is zero or not a power
    ///   of two.
    /// - `OutOfMemory` if no free run can hold `len` bytes at that alignment.
    pub fn alloc(&mut self, len: u32, align: u32) -> KernelResult<u32> {
        if len == 0 || align == 0 || !align.is_power_of_two() {
            return Err(KernelError::InvalidArgument);
        }

        // Find the first run that can hold the request once aligned. The
        // alignment padding is counted against the run, not ignored: a run that
        // holds `len` bytes but not `len` bytes *at the required alignment* is
        // not a fit, and treating it as one is how an allocator returns an
        // address that overlaps its neighbour.
        let mut found: Option<(usize, u32)> = None;
        for (i, region) in self.free.iter().enumerate() {
            let Some(aligned) = align_up(region.start, align) else {
                continue;
            };
            // `aligned >= region.start` by construction, so this cannot wrap.
            let head = aligned.wrapping_sub(region.start);
            let Some(need) = head.checked_add(len) else {
                continue;
            };
            if need <= region.len {
                found = Some((i, aligned));
                break;
            }
        }
        let Some((idx, offset)) = found else {
            return Err(KernelError::OutOfMemory);
        };

        let region = *self.free.get(idx).ok_or(KernelError::InternalError)?;
        let region_end = region.end()?;
        let alloc_end = offset.checked_add(len).ok_or(KernelError::InternalError)?;

        // What is left of the run after carving out [offset, alloc_end): a head
        // before it (the alignment padding) and a tail after it, either of
        // which may be empty. Zero-length remainders are dropped rather than
        // stored, per invariant (3) on the free list.
        let head = VramRegion {
            start: region.start,
            len: offset.wrapping_sub(region.start),
        };
        let tail = VramRegion {
            start: alloc_end,
            len: region_end.wrapping_sub(alloc_end),
        };

        match (head.len > 0, tail.len > 0) {
            (true, true) => {
                *self.free.get_mut(idx).ok_or(KernelError::InternalError)? = head;
                // `idx + 1` is at most `len()`, which `insert` accepts.
                self.free.insert(idx.saturating_add(1), tail);
            }
            (true, false) => {
                *self.free.get_mut(idx).ok_or(KernelError::InternalError)? = head;
            }
            (false, true) => {
                *self.free.get_mut(idx).ok_or(KernelError::InternalError)? = tail;
            }
            (false, false) => {
                self.free.remove(idx);
            }
        }

        Ok(offset)
    }

    /// Return a previously allocated run to the free list.
    ///
    /// The range is validated against the free list rather than trusted. A
    /// double free, or a free of a range that was never allocated, is rejected
    /// with an error instead of corrupting the list — and corrupting it is not
    /// a contained fault: the next allocation would hand out memory that is
    /// still in use as somebody's scanout buffer, and the symptom is one image
    /// appearing inside another with nothing in the code to point at.
    ///
    /// # Errors
    ///
    /// - `InvalidArgument` if `len` is zero, or the range is not wholly within
    ///   the managed region.
    /// - `AlreadyExists` if any part of the range is already free — which means
    ///   a double free, or a free of an interior slice of a live allocation.
    pub fn free(&mut self, start: u32, len: u32) -> KernelResult<()> {
        if len == 0 {
            return Err(KernelError::InvalidArgument);
        }
        let end = start.checked_add(len).ok_or(KernelError::InvalidArgument)?;
        if end > self.total {
            return Err(KernelError::InvalidArgument);
        }

        // Where the run belongs in the sorted list.
        let idx = self.free.partition_point(|r| r.start < start);

        // Overlap with the run before it, if any.
        if let Some(prev_idx) = idx.checked_sub(1) {
            let prev = *self.free.get(prev_idx).ok_or(KernelError::InternalError)?;
            if prev.end()? > start {
                return Err(KernelError::AlreadyExists);
            }
        }
        // Overlap with the run after it, if any. `next.start >= start` by the
        // definition of the partition point, so overlap means it starts before
        // this run ends.
        if let Some(next) = self.free.get(idx) {
            if next.start < end {
                return Err(KernelError::AlreadyExists);
            }
        }

        self.free.insert(idx, VramRegion { start, len });

        // Coalesce forwards first, then backwards. Forwards first because
        // merging with the previous run would move the new run's index, and
        // doing it in this order means neither step has to account for the
        // other.
        let next_idx = idx.saturating_add(1);
        if let Some(next) = self.free.get(next_idx).copied() {
            if end == next.start {
                let merged = VramRegion {
                    start,
                    len: len
                        .checked_add(next.len)
                        .ok_or(KernelError::InternalError)?,
                };
                *self.free.get_mut(idx).ok_or(KernelError::InternalError)? = merged;
                self.free.remove(next_idx);
            }
        }
        if let Some(prev_idx) = idx.checked_sub(1) {
            let prev = *self.free.get(prev_idx).ok_or(KernelError::InternalError)?;
            let cur = *self.free.get(idx).ok_or(KernelError::InternalError)?;
            if prev.end()? == cur.start {
                let merged = VramRegion {
                    start: prev.start,
                    len: prev
                        .len
                        .checked_add(cur.len)
                        .ok_or(KernelError::InternalError)?,
                };
                *self
                    .free
                    .get_mut(prev_idx)
                    .ok_or(KernelError::InternalError)? = merged;
                self.free.remove(idx);
            }
        }

        Ok(())
    }

    /// Check the free list's invariants, for the self-test.
    ///
    /// Exposed rather than kept private because the invariants are the thing
    /// worth testing: a test that only checks return values would pass against
    /// an allocator whose list had silently stopped being sorted, right up
    /// until the first allocation that overlapped a live one.
    ///
    /// # Errors
    ///
    /// `CorruptedData` if the list is unsorted, overlapping, adjacent (not
    /// coalesced), contains a zero-length run, or extends past `total`.
    pub fn check_invariants(&self) -> KernelResult<()> {
        let mut prev_end: Option<u32> = None;
        for region in &self.free {
            if region.len == 0 {
                return Err(KernelError::CorruptedData);
            }
            let end = region.end()?;
            if end > self.total {
                return Err(KernelError::CorruptedData);
            }
            if let Some(pe) = prev_end {
                // `>=` rather than `>`: equality means two runs are adjacent
                // and should have been merged, which is invariant (3).
                if region.start <= pe {
                    return Err(KernelError::CorruptedData);
                }
            }
            prev_end = Some(end);
        }
        Ok(())
    }
}
