//! Restart without the firmware: load a kernel image and start it exactly as
//! Limine would.
//!
//! This is the load-and-jump half of the live kernel replacement (`power.reload`,
//! and the mechanism design-decisions §1536 and roadmap §1126 describe). The
//! running kernel starts a new kernel image *itself*, without returning to the
//! firmware: it reproduces the handoff Limine performs — answering the same
//! requests `boot.rs` makes, building Limine-shaped page tables at the same HHDM
//! offset, quiescing the machine, and jumping through a trampoline — so the new
//! kernel boots through its one ordinary boot path and never knows it was not
//! started by the firmware.
//!
//! # Why recreate Limine's handoff rather than add a SlateOS entry point
//!
//! The alternative — a second, SlateOS-specific entry that takes a boot-facts
//! record — would mean a second boot path in every kernel, exercised only by
//! restarts, and every consumer of boot information reading it through an
//! abstraction. Recreating Limine's handoff means the new kernel exercises the
//! path every real boot does, needs no knowledge of being restarted, and any
//! Limine-protocol image (an older SlateOS included) can be started this way.
//! See design-decisions §1536 for the full tradeoff.
//!
//! # What this module does, in stages
//!
//! 1. **Parse** the new image's ELF `PT_LOAD` segments ([`parse_kernel_elf`]).
//! 2. **Plan** a physically contiguous destination for the loaded image, chosen
//!    from the firmware memory map's USABLE regions and avoiding the frames the
//!    handoff itself occupies ([`plan_destination`]). The image is larger than
//!    the buddy allocator's biggest block, and its natural destination overlaps
//!    the kind of range the running kernel occupies, so — as Linux's kexec does
//!    — the segment bytes are staged in scattered frames and copied to the
//!    contiguous destination *last*, by the trampoline.
//! 3. **Answer** the Limine requests found in the loaded image (later stage).
//! 4. **Build** the handoff page tables (later stage).
//! 5. **Quiesce** the machine and **jump** (later stage, capability-gated).
//!
//! Stages 3–5 land in follow-up changes; this module is inert until
//! [`crate::syscall`] gains the `power.reload` entry point that drives it. The
//! parsing and planning here are pure and covered by [`self_test`].

use crate::error::{KernelError, KernelResult};
use crate::limine::{MemmapEntry, memmap_type};
use crate::mm::frame::FRAME_SIZE;

// ---------------------------------------------------------------------------
// ELF constants
// ---------------------------------------------------------------------------

/// `PT_LOAD`: a segment the loader copies into memory.
const PT_LOAD: u32 = 1;
/// `PF_X`: the segment is executable.
const PF_X: u32 = 1;
/// `PF_W`: the segment is writable.
const PF_W: u32 = 2;
/// `EM_X86_64`: the only machine this kernel runs on.
const EM_X86_64: u16 = 62;
/// The first four bytes of every ELF file.
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
/// `ELFCLASS64`: 64-bit objects.
const ELFCLASS64: u8 = 2;
/// `ELFDATA2LSB`: little-endian, the only byte order x86-64 uses.
const ELFDATA2LSB: u8 = 1;

/// The most `PT_LOAD` segments a kernel image may have. The SlateOS kernel has
/// four; the cap guards the fixed-size [`ParsedKernel::segments`] array against
/// a malformed or hostile image claiming a huge program-header count.
const MAX_SEGMENTS: usize = 16;

/// [`FRAME_SIZE`] (16 KiB) as a `u64`. The kernel targets only x86-64, where a
/// `usize` → `u64` cast cannot truncate.
#[allow(clippy::cast_possible_truncation)]
const FRAME_U64: u64 = FRAME_SIZE as u64;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a kexec could not be prepared.
///
/// A narrow enum rather than reusing [`KernelError`] directly so each failure
/// names the stage it came from; [`KexecError::as_kernel_error`] maps it back to
/// the kernel's error type for the syscall boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KexecError {
    /// The image is not a valid x86-64 ELF64 kernel, or a header is out of
    /// bounds, or no loadable segment was found.
    BadImage,
    /// The image declares more loadable segments than [`MAX_SEGMENTS`].
    TooManySegments,
    /// A size or offset computed from the image overflowed a `u64`/`usize`.
    Overflow,
    /// No contiguous run of usable physical memory is large enough to hold the
    /// loaded image clear of the handoff's own frames.
    NoDestination,
}

impl KexecError {
    /// The [`KernelError`] a syscall should surface for this failure.
    #[must_use]
    pub fn as_kernel_error(self) -> KernelError {
        match self {
            Self::BadImage | Self::TooManySegments | Self::Overflow => {
                KernelError::InvalidArgument
            }
            Self::NoDestination => KernelError::OutOfMemory,
        }
    }
}

// ---------------------------------------------------------------------------
// Parsed ELF
// ---------------------------------------------------------------------------

/// One `PT_LOAD` segment of the image to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedSegment {
    /// Virtual address the segment is linked at.
    pub vaddr: u64,
    /// Bytes the segment occupies in memory (`p_memsz`). The tail beyond
    /// `filesz` is BSS and must be zeroed.
    pub memsz: u64,
    /// Bytes present in the file (`p_filesz`); `<= memsz`.
    pub filesz: u64,
    /// Offset of the segment's bytes within the ELF image (`p_offset`).
    pub file_offset: u64,
    /// The segment is writable (`PF_W`).
    pub writable: bool,
    /// The segment is executable (`PF_X`).
    pub executable: bool,
}

impl ParsedSegment {
    /// A zero segment, for initializing the fixed-size array.
    const EMPTY: Self = Self {
        vaddr: 0,
        memsz: 0,
        filesz: 0,
        file_offset: 0,
        writable: false,
        executable: false,
    };
}

/// The loadable content of a kernel ELF image.
#[derive(Debug, Clone, Copy)]
pub struct ParsedKernel {
    /// The entry point virtual address (`e_entry`).
    pub entry: u64,
    /// The lowest `p_vaddr` across all loadable segments.
    pub min_vaddr: u64,
    /// The highest `p_vaddr + p_memsz` across all loadable segments.
    pub max_vaddr: u64,
    /// The loadable segments, `segment_count` of them valid.
    pub segments: [ParsedSegment; MAX_SEGMENTS],
    /// How many entries of [`Self::segments`] are valid.
    pub segment_count: usize,
}

impl ParsedKernel {
    /// The size of the loaded image: the span from the lowest to the highest
    /// linked address, rounded to nothing (the caller frame-aligns it).
    ///
    /// This is the amount of physically contiguous memory the new kernel
    /// occupies; the destination planner sizes the run to hold it.
    #[must_use]
    pub fn image_span(&self) -> Option<u64> {
        self.max_vaddr.checked_sub(self.min_vaddr)
    }

    /// The valid segments as a slice.
    #[must_use]
    pub fn segments(&self) -> &[ParsedSegment] {
        // `segment_count` never exceeds the array length (enforced on insert).
        self.segments.get(..self.segment_count).unwrap_or(&[])
    }
}

// ---------------------------------------------------------------------------
// Little-endian readers (bounds-checked, alignment-independent)
// ---------------------------------------------------------------------------

/// Read a little-endian `u16` at `off`, or `None` if it would read past the end.
fn le_u16(buf: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let bytes: [u8; 2] = buf.get(off..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

/// Read a little-endian `u32` at `off`, or `None` if it would read past the end.
fn le_u32(buf: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let bytes: [u8; 4] = buf.get(off..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Read a little-endian `u64` at `off`, or `None` if it would read past the end.
fn le_u64(buf: &[u8], off: usize) -> Option<u64> {
    let end = off.checked_add(8)?;
    let bytes: [u8; 8] = buf.get(off..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

// ELF64 header field offsets (see the spec, or `ksyms::Elf64Header`).
const E_MACHINE_OFF: usize = 18;
const E_ENTRY_OFF: usize = 24;
const E_PHOFF_OFF: usize = 32;
const E_PHENTSIZE_OFF: usize = 54;
const E_PHNUM_OFF: usize = 56;
// Program-header field offsets, from the start of each entry.
const P_TYPE_OFF: usize = 0;
const P_FLAGS_OFF: usize = 4;
const P_OFFSET_OFF: usize = 8;
const P_VADDR_OFF: usize = 16;
const P_FILESZ_OFF: usize = 32;
const P_MEMSZ_OFF: usize = 40;
/// The smallest a valid ELF64 program-header entry can be.
const PHENT_MIN: usize = 56;

/// Parse the `PT_LOAD` segments of an x86-64 ELF64 kernel image.
///
/// Validates the ELF identity, machine and class, then walks the program
/// headers, collecting every `PT_LOAD` segment with a non-zero memory size.
/// Every offset and length is bounds-checked against `image`, so a truncated or
/// malformed image is rejected rather than read out of bounds.
///
/// # Errors
///
/// - [`KexecError::BadImage`] — not an x86-64 ELF64, a header lies outside the
///   image, a segment's file range lies outside the image, `filesz > memsz`, or
///   there are no loadable segments.
/// - [`KexecError::TooManySegments`] — more than [`MAX_SEGMENTS`] loadable
///   segments.
/// - [`KexecError::Overflow`] — a header offset or segment extent overflowed.
pub fn parse_kernel_elf(image: &[u8]) -> Result<ParsedKernel, KexecError> {
    use KexecError::{BadImage, Overflow, TooManySegments};

    if image.get(0..4) != Some(&ELF_MAGIC[..]) {
        return Err(BadImage);
    }
    if image.get(4).copied() != Some(ELFCLASS64) || image.get(5).copied() != Some(ELFDATA2LSB) {
        return Err(BadImage);
    }
    if le_u16(image, E_MACHINE_OFF).ok_or(BadImage)? != EM_X86_64 {
        return Err(BadImage);
    }

    let entry = le_u64(image, E_ENTRY_OFF).ok_or(BadImage)?;
    let phoff = usize::try_from(le_u64(image, E_PHOFF_OFF).ok_or(BadImage)?).map_err(|_| Overflow)?;
    let phentsize = usize::from(le_u16(image, E_PHENTSIZE_OFF).ok_or(BadImage)?);
    let phnum = usize::from(le_u16(image, E_PHNUM_OFF).ok_or(BadImage)?);
    if phentsize < PHENT_MIN {
        return Err(BadImage);
    }

    let mut segments = [ParsedSegment::EMPTY; MAX_SEGMENTS];
    let mut segment_count = 0usize;
    let mut min_vaddr = u64::MAX;
    let mut max_vaddr = 0u64;

    for i in 0..phnum {
        let ph = phoff
            .checked_add(i.checked_mul(phentsize).ok_or(Overflow)?)
            .ok_or(Overflow)?;

        if le_u32(image, ph.checked_add(P_TYPE_OFF).ok_or(Overflow)?).ok_or(BadImage)? != PT_LOAD {
            continue;
        }
        let flags = le_u32(image, ph.checked_add(P_FLAGS_OFF).ok_or(Overflow)?).ok_or(BadImage)?;
        let file_offset =
            le_u64(image, ph.checked_add(P_OFFSET_OFF).ok_or(Overflow)?).ok_or(BadImage)?;
        let vaddr = le_u64(image, ph.checked_add(P_VADDR_OFF).ok_or(Overflow)?).ok_or(BadImage)?;
        let filesz =
            le_u64(image, ph.checked_add(P_FILESZ_OFF).ok_or(Overflow)?).ok_or(BadImage)?;
        let memsz = le_u64(image, ph.checked_add(P_MEMSZ_OFF).ok_or(Overflow)?).ok_or(BadImage)?;

        if memsz == 0 {
            continue;
        }
        if filesz > memsz {
            return Err(BadImage);
        }
        // The file bytes the segment copies must lie within the image.
        let foff = usize::try_from(file_offset).map_err(|_| Overflow)?;
        let fsz = usize::try_from(filesz).map_err(|_| Overflow)?;
        let fend = foff.checked_add(fsz).ok_or(Overflow)?;
        if fend > image.len() {
            return Err(BadImage);
        }
        let seg_end = vaddr.checked_add(memsz).ok_or(Overflow)?;

        let slot = segments.get_mut(segment_count).ok_or(TooManySegments)?;
        *slot = ParsedSegment {
            vaddr,
            memsz,
            filesz,
            file_offset,
            writable: flags & PF_W != 0,
            executable: flags & PF_X != 0,
        };
        segment_count = segment_count.checked_add(1).ok_or(Overflow)?;

        if vaddr < min_vaddr {
            min_vaddr = vaddr;
        }
        if seg_end > max_vaddr {
            max_vaddr = seg_end;
        }
    }

    if segment_count == 0 {
        return Err(BadImage);
    }

    Ok(ParsedKernel {
        entry,
        min_vaddr,
        max_vaddr,
        segments,
        segment_count,
    })
}

// ---------------------------------------------------------------------------
// Destination planning
// ---------------------------------------------------------------------------

/// A half-open physical address range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysRange {
    /// First byte of the range.
    pub start: u64,
    /// One past the last byte of the range.
    pub end: u64,
}

impl PhysRange {
    /// Whether this range and `other` share any byte.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// Round `x` up to a multiple of `align` (a power of two), or `None` on overflow.
fn align_up(x: u64, align: u64) -> Option<u64> {
    let mask = align.wrapping_sub(1);
    Some(x.checked_add(mask)? & !mask)
}

/// Round `x` down to a multiple of `align` (a power of two).
fn align_down(x: u64, align: u64) -> u64 {
    x & !align.wrapping_sub(1)
}

/// Choose a physically contiguous destination for the loaded image.
///
/// Scans the firmware memory map for a `USABLE` region with a frame-aligned
/// sub-run of at least `needed` bytes that does not overlap any range in
/// `excluded` — the frames the handoff itself occupies (the staged segment
/// copies, the handoff page tables, the responses, and the trampoline control
/// page), which must survive until the trampoline's final copy and so cannot be
/// overwritten by the new kernel landing on them.
///
/// Returns the frame-aligned base of the chosen run. `excluded` need not be
/// sorted. The search is first-fit by ascending region then ascending address,
/// which keeps it deterministic and easy to reason about; kexec is not a hot
/// path, so first-fit's simplicity is worth more than best-fit's packing.
///
/// # Errors
///
/// [`KexecError::NoDestination`] if no region has a large enough clear sub-run,
/// or [`KexecError::Overflow`] if an address computation overflows.
pub fn plan_destination(
    memory_map: &[&MemmapEntry],
    needed: u64,
    excluded: &[PhysRange],
) -> Result<u64, KexecError> {
    let frame = FRAME_U64;
    let needed = align_up(needed, frame).ok_or(KexecError::Overflow)?;
    if needed == 0 {
        return Err(KexecError::NoDestination);
    }

    for entry in memory_map {
        if entry.type_ != memmap_type::USABLE {
            continue;
        }
        let region_start = match align_up(entry.base, frame) {
            Some(s) => s,
            None => continue,
        };
        let region_top = match entry.base.checked_add(entry.length) {
            Some(t) => align_down(t, frame),
            None => continue,
        };
        if region_top <= region_start {
            continue;
        }

        let mut cand = region_start;
        while let Some(cand_end) = cand.checked_add(needed) {
            if cand_end > region_top {
                break;
            }
            let window = PhysRange {
                start: cand,
                end: cand_end,
            };
            match excluded.iter().find(|e| e.overlaps(window)) {
                None => return Ok(cand),
                Some(blocker) => {
                    // Jump past the blocker. It overlaps `window`, so its end is
                    // strictly above `cand`; the jump always makes progress and
                    // the loop is bounded by `region_top`.
                    cand = align_up(blocker.end, frame).ok_or(KexecError::Overflow)?;
                }
            }
        }
    }

    Err(KexecError::NoDestination)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise ELF parsing and destination planning on fabricated inputs.
///
/// Diagnostic: these are pure functions, so a failure means the loader would
/// mis-read an image or place the new kernel on memory it still needs — a bug
/// to surface, but it cannot affect a kernel that never invokes `power.reload`.
pub fn self_test() -> KernelResult<()> {
    use crate::selftest;

    // ---- ELF parsing ----
    let image = build_test_elf();
    let parsed = match parse_kernel_elf(&image) {
        Ok(p) => p,
        Err(e) => {
            crate::serial_println!("  FAIL: parse_kernel_elf rejected a valid image: {:?}", e);
            return Err(KernelError::InternalError);
        }
    };
    selftest::check_eq!(parsed.entry, TEST_ENTRY, "entry point");
    selftest::check_eq!(parsed.segment_count, 2, "two loadable segments");
    selftest::check_eq!(parsed.min_vaddr, TEST_VADDR_A, "min vaddr");
    selftest::check_eq!(
        parsed.max_vaddr,
        TEST_VADDR_B.wrapping_add(TEST_MEMSZ_B),
        "max vaddr"
    );
    selftest::check_eq!(
        parsed.image_span(),
        Some(TEST_VADDR_B.wrapping_add(TEST_MEMSZ_B).wrapping_sub(TEST_VADDR_A)),
        "image span"
    );
    let seg_a = parsed.segments().first().ok_or(KernelError::InternalError)?;
    selftest::check!(seg_a.executable && !seg_a.writable, "segment A is R-X");
    let seg_b = parsed.segments().get(1).ok_or(KernelError::InternalError)?;
    selftest::check!(seg_b.writable && !seg_b.executable, "segment B is RW-");

    // A segment whose file range runs past the image end is rejected.
    let mut truncated = image.clone();
    truncated.truncate(image.len().wrapping_sub(1));
    selftest::check!(
        parse_kernel_elf(&truncated).is_err(),
        "a truncated image is rejected"
    );
    // Garbage is rejected, not read out of bounds.
    selftest::check!(
        parse_kernel_elf(&[0u8; 8]).is_err(),
        "a non-ELF buffer is rejected"
    );

    // ---- destination planning ----
    let frame = FRAME_U64;
    // One small reserved region then a large usable one.
    let e0 = MemmapEntry {
        base: 0,
        length: frame,
        type_: memmap_type::RESERVED,
    };
    let e1 = MemmapEntry {
        base: frame,
        length: frame.wrapping_mul(100),
        type_: memmap_type::USABLE,
    };
    let map: [&MemmapEntry; 2] = [&e0, &e1];

    // With nothing excluded, first-fit returns the start of the usable region.
    let dst = plan_destination(&map, frame.wrapping_mul(3), &[]).map_err(|e| {
        crate::serial_println!("  FAIL: plan_destination found nothing: {:?}", e);
        KernelError::InternalError
    })?;
    selftest::check_eq!(dst, frame, "first-fit base is the usable region start");

    // An excluded range at the front pushes the choice past it.
    let blocker = PhysRange {
        start: frame,
        end: frame.wrapping_mul(5),
    };
    let dst2 = plan_destination(&map, frame.wrapping_mul(3), &[blocker]).map_err(|e| {
        crate::serial_println!("  FAIL: plan_destination (excluded) found nothing: {:?}", e);
        KernelError::InternalError
    })?;
    selftest::check_eq!(dst2, frame.wrapping_mul(5), "choice jumps past the blocker");
    selftest::check!(
        !PhysRange {
            start: dst2,
            end: dst2.wrapping_add(frame.wrapping_mul(3))
        }
        .overlaps(blocker),
        "the chosen run clears the blocker"
    );

    // A request too large for any usable region fails cleanly.
    selftest::check!(
        plan_destination(&map, frame.wrapping_mul(1000), &[]).is_err(),
        "an over-large request has no destination"
    );

    // ---- error mapping ----
    selftest::check_eq!(
        KexecError::BadImage.as_kernel_error(),
        KernelError::InvalidArgument,
        "a bad image is an invalid argument"
    );
    selftest::check_eq!(
        KexecError::NoDestination.as_kernel_error(),
        KernelError::OutOfMemory,
        "no destination is out of memory"
    );

    Ok(())
}

// Fabricated-ELF parameters, shared between the builder and the assertions.
const TEST_ENTRY: u64 = 0xffff_ffff_8000_1234;
const TEST_VADDR_A: u64 = 0xffff_ffff_8000_0000;
const TEST_MEMSZ_A: u64 = 0x1000;
const TEST_VADDR_B: u64 = 0xffff_ffff_8000_2000;
const TEST_MEMSZ_B: u64 = 0x2000;

/// Build a minimal but valid x86-64 ELF64 image with two `PT_LOAD` segments
/// (one R-X, one RW-), for [`self_test`].
// Scaffolding that writes a buffer of statically-known size: direct indexing and
// small `usize` → `u16`/`u64` casts of constants cannot go wrong here, and a
// panic would only fail the self-test it serves.
#[allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects
)]
fn build_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const EHSIZE: usize = 64;
    const PHENTSIZE: usize = 56;
    let phoff = EHSIZE;
    let data_off = EHSIZE + 2 * PHENTSIZE;

    // Header + two program headers + one byte of segment file content.
    let mut buf = vec![0u8; data_off + 1];

    // e_ident
    buf[0..4].copy_from_slice(&ELF_MAGIC);
    buf[4] = ELFCLASS64;
    buf[5] = ELFDATA2LSB;
    buf[6] = 1; // EV_CURRENT
    // e_type = ET_EXEC (2)
    buf[16..18].copy_from_slice(&2u16.to_le_bytes());
    buf[E_MACHINE_OFF..E_MACHINE_OFF + 2].copy_from_slice(&EM_X86_64.to_le_bytes());
    buf[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    buf[E_ENTRY_OFF..E_ENTRY_OFF + 8].copy_from_slice(&TEST_ENTRY.to_le_bytes());
    buf[E_PHOFF_OFF..E_PHOFF_OFF + 8].copy_from_slice(&(phoff as u64).to_le_bytes());
    buf[52..54].copy_from_slice(&(EHSIZE as u16).to_le_bytes()); // e_ehsize
    buf[E_PHENTSIZE_OFF..E_PHENTSIZE_OFF + 2].copy_from_slice(&(PHENTSIZE as u16).to_le_bytes());
    buf[E_PHNUM_OFF..E_PHNUM_OFF + 2].copy_from_slice(&2u16.to_le_bytes());

    let write_ph = |buf: &mut [u8], at: usize, flags: u32, vaddr: u64, memsz: u64, filesz: u64| {
        buf[at + P_TYPE_OFF..at + P_TYPE_OFF + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        buf[at + P_FLAGS_OFF..at + P_FLAGS_OFF + 4].copy_from_slice(&flags.to_le_bytes());
        // p_offset: point both segments at the single content byte, which is in
        // bounds; filesz is 1 so the bounds check passes.
        buf[at + P_OFFSET_OFF..at + P_OFFSET_OFF + 8]
            .copy_from_slice(&(data_off as u64).to_le_bytes());
        buf[at + P_VADDR_OFF..at + P_VADDR_OFF + 8].copy_from_slice(&vaddr.to_le_bytes());
        // p_paddr (offset 24) left zero.
        buf[at + P_FILESZ_OFF..at + P_FILESZ_OFF + 8].copy_from_slice(&filesz.to_le_bytes());
        buf[at + P_MEMSZ_OFF..at + P_MEMSZ_OFF + 8].copy_from_slice(&memsz.to_le_bytes());
        // p_align (offset 48) left zero.
    };
    // Segment A: R-X (PF_R | PF_X = 5).
    write_ph(&mut buf, phoff, PF_X | 4, TEST_VADDR_A, TEST_MEMSZ_A, 1);
    // Segment B: RW- (PF_R | PF_W = 6).
    write_ph(
        &mut buf,
        phoff + PHENTSIZE,
        PF_W | 4,
        TEST_VADDR_B,
        TEST_MEMSZ_B,
        1,
    );

    buf
}
