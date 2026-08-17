//! The BAR0 linear framebuffer aperture: mapping the card's video memory so
//! the CPU can write pixels into it.
//!
//! ## What this is for
//!
//! [`super::mmio`] maps BAR2, which is registers — a few hundred bytes of
//! control surface. This maps BAR0, which is the video memory itself, and it is
//! the other half of what a display driver needs: the CRTC is told *where* to
//! scan out from through a register, but something has to have put pixels
//! there.
//!
//! Keeping the two apart is not tidiness. The register aperture is one frame
//! and every access to it is a device action with side effects; this is
//! megabytes of ordinary storage whose only special property is where it lives.
//! Giving them one type would mean one bounds check covering two regions with
//! nothing in common, and would make a stray write into the register file
//! indistinguishable from a stray write into a framebuffer.
//!
//! ## Caching
//!
//! The aperture is mapped **write-combining**: the CPU gathers consecutive
//! stores in a fill buffer and pushes them out as whole cache-line bursts,
//! which is what the long sequential runs of a framebuffer paint produce. It
//! is both correct — nothing lingers in a cache line the CRTC cannot see, and
//! [`VramAperture::flush`] fences before handing the buffer to the display
//! engine — and several times faster than the uncacheable mapping this used to
//! take.
//!
//! Write-combining is only a memory type at all because [`crate::mm::pat`]
//! programs the `IA32_PAT` MSR at boot; the power-on table has no `WC` entry.
//! If that did not run — a CPU without PAT — the same PTE bits keep their
//! power-on meaning of write-through, which is still *correct* for a
//! framebuffer and merely no faster than uncached. So this mapping degrades in
//! speed and never in correctness, and [`VramAperture::memory_type`] reports
//! which of the two was obtained rather than leaving it to be inferred.
//!
//! Writeback is the one type that would be wrong, and it is the one type this
//! never asks for.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{FRAME_SIZE, PhysFrame};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::serial_println;

use super::mmio::check_offset;

/// Largest BAR0 window this driver will map, in bytes.
///
/// A cap rather than "whatever the card reports", because the mapping is not
/// free: every 16 KiB frame costs four page-table entries, so 16 MiB is 4096
/// PTEs and a card advertising 256 MiB would be 65536 of them — half a megabyte
/// of page tables built at boot for memory this driver has no use for. 16 MiB
/// holds a 1920x1080 32-bit scanout buffer three times over, which is more than
/// anything here allocates.
///
/// The cap is not merely an optimisation: [`super::vram::VramAllocator`] is
/// sized from the *mapped* length rather than the reported VRAM size precisely
/// so that it can never hand out an offset outside the mapping. An allocator
/// that knew about memory the CPU cannot reach would produce a framebuffer the
/// CRTC displays as garbage and the driver cannot write to, with both halves
/// looking correct in isolation.
pub const MAX_APERTURE: u64 = 16 * 1024 * 1024;

/// Cache line size assumed by [`VramAperture::flush`], in bytes.
///
/// 64 on every x86-64 part this kernel targets. Being wrong low would flush
/// more lines than necessary (slow, still correct); being wrong high would skip
/// lines (fast, silently wrong), which is why the constant is small rather than
/// large.
const CACHE_LINE: u32 = 64;

/// A mapped window onto a card's video memory.
///
/// Offsets are from the base of VRAM, which is the same origin the CRTC's
/// `CRTC_OFFSET` register and [`super::vram::VramAllocator`] use. That is the
/// whole reason this type exists in the form it does: an allocator offset can
/// be handed to the CRTC and to the CPU without conversion, so there is no
/// coordinate to get wrong between them.
pub struct VramAperture {
    /// Kernel-virtual base of the mapping.
    base: *mut u8,
    /// Physical base (BAR0).
    phys: u64,
    /// Bytes actually mapped.
    len: u64,
    /// Whether the mapping ended up cacheable despite the request.
    ///
    /// Tracked rather than assumed. `map_frame` can report a frame as already
    /// mapped — the bootloader's direct map may already cover this address on a
    /// machine with enough RAM — in which case the flags in force are whoever
    /// mapped it first's, not ours. If those permit caching, pixel writes can
    /// sit in a cache line that the CRTC, which reads through the memory
    /// controller, will never see. [`VramAperture::flush`] is what closes that
    /// gap, and it needs to know whether there is a gap to close.
    cached: bool,
    /// The memory type the mapping actually ended up with.
    ///
    /// Kept alongside `cached` rather than replacing it because they answer
    /// different questions: this one is "what did we get", which belongs in a
    /// log, and `cached` is "must `flush` do work", which is a hot-path
    /// predicate. Deriving the second from the first at every call would put a
    /// match in the flush path to re-answer a question settled at map time.
    mem_type: crate::mm::pat::MemoryType,
}

// SAFETY: `VramAperture` is a handle to a PCI BAR. The pointer addresses device
// memory, not any Rust allocation, so moving it between threads aliases
// nothing. Only `Send` is claimed: concurrent use is the caller's to serialise.
unsafe impl Send for VramAperture {}

impl VramAperture {
    /// Map up to [`MAX_APERTURE`] bytes of the aperture at `phys`.
    ///
    /// `size` is the card's reported VRAM; the mapping is the smaller of that
    /// and [`MAX_APERTURE`], rounded *down* to a whole frame. Down, not up: an
    /// aperture rounded up would include a frame that is partly outside the
    /// card's memory, and the memory controller's answer to a read there is not
    /// specified.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if `phys` is zero or not frame-aligned, or if `size`
    /// is smaller than one frame — an unassigned BAR reads as zero, and a card
    /// reporting less than 16 KiB of video memory has not been configured.
    ///
    /// # Safety
    ///
    /// `phys` must be the physical base of a PCI memory BAR that is not system
    /// RAM. The caller must not create a second `VramAperture` over the same
    /// region.
    pub unsafe fn map(phys: u64, size: u64) -> KernelResult<Self> {
        if phys == 0 || !phys.is_multiple_of(FRAME_SIZE as u64) {
            return Err(KernelError::InvalidArgument);
        }
        let frame_size = FRAME_SIZE as u64;
        // Whole frames only, rounded down. `frame_size` is a non-zero constant,
        // so the division cannot trap; it is written with `checked_*` anyway
        // because a silent wrap here would produce a length that disagrees with
        // the frames actually mapped, and every bounds check downstream trusts
        // this number.
        let frames = size
            .min(MAX_APERTURE)
            .checked_div(frame_size)
            .ok_or(KernelError::InternalError)?;
        let len = frames
            .checked_mul(frame_size)
            .ok_or(KernelError::InternalError)?;
        if len == 0 {
            return Err(KernelError::InvalidArgument);
        }

        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let virt = phys.wrapping_add(hhdm);
        let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
        // Write-combining, not uncacheable: this is a framebuffer, and the
        // whole difference between a usable one and a 640x480 one is whether
        // sequential stores cross the bus in cache-line bursts. Correctness is
        // unaffected either way -- neither type lets a store linger in a cache
        // line the CRTC cannot see -- so this is a pure speed choice with a
        // safe fallback: without `mm::pat` these same bits mean write-through.
        let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::WRITE_COMBINING;

        let frames = usize::try_from(frames).map_err(|_| KernelError::InvalidArgument)?;
        let mut preexisting = 0u32;
        for i in 0..frames {
            let off = (i as u64).wrapping_mul(frame_size);
            let Some(frame) = PhysFrame::from_addr(phys.wrapping_add(off)) else {
                return Err(KernelError::InvalidArgument);
            };
            // SAFETY: `phys + off` lies inside a PCI memory BAR, so it is device
            // memory rather than RAM, and `virt + off` is its HHDM image, which
            // no allocator hands out. A failure here means the address is
            // already mapped, which is recorded and checked below rather than
            // treated as fatal.
            if unsafe {
                page_table::map_frame(pml4, VirtAddr::new(virt.wrapping_add(off)), frame, flags)
            }
            .is_err()
            {
                preexisting = preexisting.saturating_add(1);
            }
            for p in 0..page_table::HW_PAGES_PER_FRAME {
                let addr = virt
                    .wrapping_add(off)
                    .wrapping_add((p.saturating_mul(page_table::HW_PAGE_SIZE)) as u64);
                // SAFETY: `invlpg` on a just-mapped address, to drop any stale
                // translation. It is a no-op, not a fault, on an address with
                // no cached translation.
                unsafe {
                    core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
                }
            }
        }

        // Ask the page tables what the mapping actually is, rather than assuming
        // the request was honoured. This is the check that turns "some frames
        // were already mapped" from an unknown into a known: if the flags in
        // force permit caching, `flush` has work to do, and if they do not, it
        // does not.
        let mem_type = match page_table::translate_flags(pml4, VirtAddr::new(virt)) {
            Some(f) => crate::mm::pat::memory_type_of(f),
            None => {
                // Nothing is mapped at the base at all, so the aperture is
                // unusable — reads would fault rather than return pixels.
                return Err(KernelError::NotSupported);
            }
        };
        let cached = mem_type.writes_may_linger();
        if preexisting > 0 {
            serial_println!(
                "[ati]   note: {} of {} VRAM frames were already mapped (type={}, writes may linger={})",
                preexisting,
                frames,
                mem_type.name(),
                cached
            );
        }

        Ok(Self {
            base: virt as *mut u8,
            phys,
            len,
            cached,
            mem_type,
        })
    }

    /// Physical base of the aperture.
    #[must_use]
    pub const fn phys(&self) -> u64 {
        self.phys
    }

    /// Bytes mapped. This, not the card's reported VRAM size, is what bounds
    /// every access and what the allocator is sized from.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether writes through this mapping can sit in a cache the display
    /// engine cannot see — i.e. whether [`Self::flush`] has real work to do.
    ///
    /// False for every memory type this driver asks for. It is not
    /// unconditionally false because the mapping may have been made by someone
    /// else first; see the `cached` field.
    #[must_use]
    pub const fn is_cached(&self) -> bool {
        self.cached
    }

    /// The memory type the mapping actually obtained.
    ///
    /// Worth reporting rather than assuming: the driver asks for
    /// write-combining, but gets write-through on a CPU where
    /// [`crate::mm::pat`] could not program the table, and could in principle
    /// get something else entirely if the range was already mapped by another
    /// subsystem. All of those are correct; only one of them is fast.
    #[must_use]
    pub const fn memory_type(&self) -> crate::mm::pat::MemoryType {
        self.mem_type
    }

    /// A raw pointer to `len` bytes at `offset`, for callers that must write
    /// pixels in bulk rather than through [`Self::write32`].
    ///
    /// This is the one place the aperture hands out unchecked access, and it is
    /// bounds-checked *here* precisely because nothing downstream can be: once
    /// a caller has the pointer, this type has no further say. The range is
    /// verified whole, so a pointer that comes back covers `len` mapped bytes.
    ///
    /// The memory is write-combining, so stores through the pointer may be
    /// gathered and reordered in a fill buffer rather than reaching the card
    /// one at a time — which is the point, and is why callers must go through
    /// [`Self::flush`] before pointing the CRTC at the result,
    /// since [`Self::is_cached`] may say otherwise on a machine where the
    /// mapping was already established with different attributes.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if `len` is zero or the range is not wholly inside the
    /// mapping.
    pub fn ptr_at(&self, offset: u32, len: u32) -> KernelResult<*mut u8> {
        if len == 0 {
            return Err(KernelError::InvalidArgument);
        }
        let end = u64::from(offset)
            .checked_add(u64::from(len))
            .ok_or(KernelError::InvalidArgument)?;
        if end > self.len {
            return Err(KernelError::InvalidArgument);
        }
        // SAFETY: `offset` is inside the mapping — `offset + len <= self.len`
        // and `len >= 1` — so the result is an address this handle has mapped.
        Ok(unsafe { self.base.add(offset as usize) })
    }

    /// Write one 32-bit pixel at a byte offset.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if the offset is not 4-byte aligned or the four bytes
    /// there are not wholly inside the mapping.
    // The `*mut u8` → `*mut u32` cast is alignment-checked by `check_offset`,
    // which rejects any offset that is not a multiple of four; the base is
    // frame-aligned. See that function's documentation.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn write32(&self, offset: u32, value: u32) -> KernelResult<()> {
        let off = check_offset(offset, self.len)?;
        // SAFETY: `off` is bounds- and alignment-checked against this handle's
        // own mapping, which is present and writable.
        unsafe {
            self.base
                .add(off as usize)
                .cast::<u32>()
                .write_volatile(value);
        }
        Ok(())
    }

    /// Read one 32-bit pixel back.
    ///
    /// Exists for verification rather than for drawing: reading video memory
    /// over the bus is slow, and the reason to do it is to prove that a write
    /// landed. A framebuffer that is mapped but not actually backed by the
    /// card's memory accepts every write and returns zero, which is otherwise
    /// indistinguishable from working.
    ///
    /// # Errors
    ///
    /// As [`Self::write32`].
    #[allow(clippy::cast_ptr_alignment)]
    pub fn read32(&self, offset: u32) -> KernelResult<u32> {
        let off = check_offset(offset, self.len)?;
        // SAFETY: as `write32` — bounds- and alignment-checked, mapping present.
        Ok(unsafe { self.base.add(off as usize).cast::<u32>().read_volatile() })
    }

    /// Fill `count` consecutive 32-bit words starting at `offset`.
    ///
    /// The whole range is bounds-checked once, before anything is written,
    /// rather than per word. Checking per word would be no safer — the first
    /// failing word would already have been preceded by hundreds of successful
    /// ones — and a partially-filled framebuffer that reports an error is
    /// harder to reason about than one that was never touched.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if the offset is not 4-byte aligned, or if the range
    /// runs past the end of the mapping.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn fill32(&self, offset: u32, count: u32, value: u32) -> KernelResult<()> {
        if count == 0 {
            return Ok(());
        }
        // Validate the first and last words. The first establishes alignment,
        // and since each step is exactly four bytes, every word between two
        // aligned in-range endpoints is itself aligned and in range.
        check_offset(offset, self.len)?;
        let span = count
            .checked_sub(1)
            .and_then(|n| n.checked_mul(4))
            .ok_or(KernelError::InvalidArgument)?;
        let last = offset
            .checked_add(span)
            .ok_or(KernelError::InvalidArgument)?;
        check_offset(last, self.len)?;

        for i in 0..count {
            let off = (offset as usize).saturating_add((i as usize).saturating_mul(4));
            // SAFETY: `off` runs from a checked-in-range first word to a
            // checked-in-range last word in four-byte steps, so every address
            // is 4-byte aligned and inside the mapping.
            unsafe {
                self.base.add(off).cast::<u32>().write_volatile(value);
            }
        }
        Ok(())
    }

    /// Make previously written bytes visible to the card's scanout engine.
    ///
    /// Two separate hazards, handled by the two halves of this method.
    ///
    /// **The fence, which is unconditional and now load-bearing.** The mapping
    /// is write-combining, so stores sit in a fill buffer and reach the card in
    /// bursts, at a time of the CPU's choosing and not necessarily in program
    /// order. `sfence` drains them. Pointing the CRTC at a buffer whose last
    /// rows are still in a fill buffer displays a torn frame, and the
    /// `CRTC_OFFSET` write that does the pointing is itself a store that must
    /// not be allowed to overtake the pixels. This was cheap insurance when the
    /// aperture was uncacheable — an uncached store is in memory by the time
    /// the next instruction runs — but under write-combining it is what makes
    /// the mapping safe to use at all.
    ///
    /// **The write-back loop, which is conditional and normally skipped.** Only
    /// a mapping that turned out cacheable — see [`Self::is_cached`], which
    /// reports what the page tables actually say rather than what was requested
    /// — can hold pixels in a dirty cache line. The CRTC reads through the
    /// memory controller and never sees one, so those pixels simply do not
    /// appear. `clflush` is how they are made to.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if the range is not wholly within the mapping.
    pub fn flush(&self, offset: u32, len: u32) -> KernelResult<()> {
        let end = u64::from(offset)
            .checked_add(u64::from(len))
            .ok_or(KernelError::InvalidArgument)?;
        if end > self.len {
            return Err(KernelError::InvalidArgument);
        }

        if self.cached {
            // Walk cache lines, not bytes. Starting from the line containing
            // `offset` so a range that begins mid-line still has that line
            // written back.
            let first = offset & !(CACHE_LINE.wrapping_sub(1));
            let mut addr = first;
            while u64::from(addr) < end {
                // SAFETY: `addr` is inside the mapping (it starts at or below
                // `offset` and stops before `end`, both checked above).
                // `clflush` on a mapped address is always well defined.
                unsafe {
                    let p = self.base.add(addr as usize);
                    core::arch::asm!("clflush [{}]", in(reg) p, options(nostack, preserves_flags));
                }
                addr = addr.saturating_add(CACHE_LINE);
            }
        }

        // SAFETY: `sfence` has no operands and no memory effects beyond
        // ordering; it is always safe to execute.
        unsafe {
            core::arch::asm!("sfence", options(nostack, preserves_flags));
        }
        Ok(())
    }
}
