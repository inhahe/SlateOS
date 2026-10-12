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
//! 3. **Answer** the Limine requests found in the loaded image
//!    ([`find_request_sites`], the `build_*_response` functions,
//!    [`patch_requests`]).
//! 4. **Build** the handoff page tables ([`build_handoff_tables`]) and stage
//!    the image ([`stage_image`]); [`prepare_handoff`] does stages 1–4.
//! 5. **Quiesce** the machine and **jump** ([`PreparedHandoff::execute`],
//!    [`quiesce`], the trampoline).
//!
//! [`reload`] runs the whole of it, on the bootstrap CPU. Two things call it:
//! `SYS_POWER_RELOAD`, gated by `Rights::RELOAD_KERNEL`, which moves its
//! caller to the bootstrap CPU first; and the boot path under
//! `kexec.selftest=1` ([`reload_self`]), which reloads the running kernel into
//! itself to validate the jump -- the one part no self-test can exercise.
//! Everything before the jump is covered by [`self_test`].

use crate::error::{KernelError, KernelResult};
use crate::limine::{MemmapEntry, memmap_type};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::mm::page_table::{PageFlags, PageTableEntry, VirtAddr};
use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

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
            Self::BadImage | Self::TooManySegments | Self::Overflow => KernelError::InvalidArgument,
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
    let phoff =
        usize::try_from(le_u64(image, E_PHOFF_OFF).ok_or(BadImage)?).map_err(|_| Overflow)?;
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

/// Choose a contiguous destination placed as *high* in physical memory as a
/// USABLE region allows, for at least `needed` bytes.
///
/// The handoff's own frames (page tables, staging, responses, the control page)
/// come from the running kernel's buddy allocator, which hands out low memory
/// first; putting the destination at the top of the highest usable region keeps
/// it clear of them in practice, so the trampoline's copy into the destination
/// cannot clobber a structure the handoff still needs. The caller still verifies
/// no overlap and fails cleanly if the rare collision happens.
///
/// Returns the frame-aligned base of the chosen run (its *top* minus `needed`,
/// aligned down).
///
/// # Errors
///
/// [`KexecError::NoDestination`] if no usable region is large enough, or
/// [`KexecError::Overflow`] on an address computation overflow.
pub fn plan_destination_high(memory_map: &[&MemmapEntry], needed: u64) -> Result<u64, KexecError> {
    let frame = FRAME_U64;
    let needed = align_up(needed, frame).ok_or(KexecError::Overflow)?;
    if needed == 0 {
        return Err(KexecError::NoDestination);
    }

    let mut best: Option<u64> = None;
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
        // The highest frame-aligned base at which [base, base+needed) fits.
        let Some(room) = region_top.checked_sub(needed) else {
            continue;
        };
        let candidate = align_down(room, frame);
        if candidate < region_start {
            continue;
        }
        if best.is_none_or(|b| candidate > b) {
            best = Some(candidate);
        }
    }

    best.ok_or(KexecError::NoDestination)
}

// ---------------------------------------------------------------------------
// Limine request discovery
// ---------------------------------------------------------------------------

/// The two leading id words common to every Limine request.
const COMMON_MAGIC: [u64; 2] = [0xc7b1_dd30_df4c_8b88, 0x0a82_e883_a194_f07b];
/// The four-word marker the kernel places before its request section.
const REQUESTS_START_MARKER: [u64; 4] = [
    0xf6b8_f4b3_9de7_d1ae,
    0xfab9_1a69_40fc_b9cf,
    0x785c_6ed0_15d3_e316,
    0x181e_920a_7852_b9d9,
];
/// The two-word marker the kernel places after its request section.
const REQUESTS_END_MARKER: [u64; 2] = [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62];
/// The base-revision tag's two magic words; a third word holds the revision,
/// which the bootloader sets to 0 to signal the requested revision is supported.
const BASE_REVISION_MAGIC: [u64; 2] = [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc];

/// Offset, within a request block, of the `revision` field (after `id[4]`).
pub const REQUEST_REVISION_OFFSET: usize = 32;
/// Offset, within a request block, of the `response` pointer (after `revision`).
pub const REQUEST_RESPONSE_OFFSET: usize = 40;
/// Offset, within the base-revision tag, of the revision word.
pub const BASE_REVISION_WORD_OFFSET: usize = 16;

/// Feature ids — the third and fourth id words — of the requests the kernel
/// makes (`kernel/src/boot.rs`). Mirrors `crate::limine`; kept here so the
/// loader is self-contained, and asserted equal where the two can be compared.
mod feature_id {
    pub const MEMMAP: [u64; 2] = [0x67cf_3d9d_378a_806f, 0xe304_acdf_c50c_3c62];
    pub const HHDM: [u64; 2] = [0x48dc_f1cb_8ad2_b852, 0x6398_4e95_9a98_244b];
    pub const FRAMEBUFFER: [u64; 2] = [0x9d58_27dc_d881_dd75, 0xa314_8604_f6fa_b11b];
    pub const RSDP: [u64; 2] = [0xc5e7_7b6b_397e_7b43, 0x2763_7845_accd_cf3c];
    pub const EXECUTABLE_ADDRESS: [u64; 2] = [0x71ba_7686_3cc5_5f63, 0xb264_4a48_c516_a487];
    pub const KERNEL_FILE: [u64; 2] = [0xad97_e90e_83f1_ed67, 0x31eb_5d1c_5ff2_3b69];
}

/// Byte offsets, within a loaded image, of the Limine structures the handoff
/// must fill. Each is the offset of the block's first id word (the base
/// revision tag's first magic word); the response pointer lives at
/// `offset + REQUEST_RESPONSE_OFFSET`, the base-revision word at
/// `offset + BASE_REVISION_WORD_OFFSET`. `None` when the image carries no such
/// request — the handoff then simply does not answer it, exactly as Limine
/// leaves an unmade request's response pointer untouched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestSites {
    /// The base-revision tag.
    pub base_revision: Option<usize>,
    /// The memory-map request.
    pub memmap: Option<usize>,
    /// The HHDM request.
    pub hhdm: Option<usize>,
    /// The framebuffer request.
    pub framebuffer: Option<usize>,
    /// The RSDP request.
    pub rsdp: Option<usize>,
    /// The executable-address request.
    pub executable_address: Option<usize>,
    /// The kernel-file request.
    pub kernel_file: Option<usize>,
}

/// Find `needle` (consecutive little-endian `u64` words) at an 8-byte-aligned
/// offset within `image[from..to]` for which `allowed(offset, length)` holds,
/// returning the offset of the first such match.
fn find_first_aligned(
    image: &[u8],
    needle: &[u64],
    from: usize,
    to: usize,
    allowed: impl Fn(usize, usize) -> bool,
) -> Option<usize> {
    let span = needle.len().checked_mul(8)?;
    let limit = to.min(image.len());
    let mut off = from.checked_next_multiple_of(8)?;
    while let Some(end) = off.checked_add(span) {
        if end > limit {
            break;
        }
        if words_match(image, off, needle) && allowed(off, span) {
            return Some(off);
        }
        off = off.checked_add(8)?;
    }
    None
}

/// The file ranges, `[start, end)`, of `image`'s writable loadable segments
/// -- where a bootloader can write a request's response, so where requests
/// are -- or none if `image` is not an ELF this module parses (a hand-built
/// test image), which leaves the whole image eligible.
fn writable_file_ranges(image: &[u8]) -> alloc::vec::Vec<(usize, usize)> {
    let Ok(parsed) = parse_kernel_elf(image) else {
        return alloc::vec::Vec::new();
    };
    parsed
        .segments()
        .iter()
        .filter(|s| s.writable)
        .filter_map(|s| {
            let start = usize::try_from(s.file_offset).ok()?;
            let end = start.checked_add(usize::try_from(s.filesz).ok()?)?;
            Some((start, end))
        })
        .collect()
}

/// Whether `image` at `off` holds `needle`'s words, little-endian.
fn words_match(image: &[u8], off: usize, needle: &[u64]) -> bool {
    for (i, want) in needle.iter().enumerate() {
        let at = match off.checked_add(i.saturating_mul(8)) {
            Some(a) => a,
            None => return false,
        };
        if le_u64(image, at) != Some(*want) {
            return false;
        }
    }
    true
}

/// Locate the base-revision tag and the kernel's Limine requests within a
/// loaded image.
///
/// The search is bounded to the window between the requests-start marker and
/// the first requests-end marker after it, as the protocol specifies
/// (base-revision tags included). An image with no markers is scanned whole, so
/// a hand-built or older image is still handled. Each request and the tag are
/// identified by their magic at an 8-byte-aligned offset.
///
/// Only a writable loadable segment holds requests -- the bootloader writes
/// their responses there -- so markers and requests anywhere else are not the
/// image's own: a loader carries the same words as constants in `.rodata`, and
/// this one does (`REQUESTS_START_MARKER` and its siblings below). Until
/// 2026-10-08 this took the *last* start marker and the *last* end marker in
/// the whole file, which in a SlateOS kernel are those constants -- `.rodata`
/// comes after `.requests` -- so the window held no request and nothing was
/// patched: the first self-reload to reach the new kernel stopped at "Limine
/// did not answer the HHDM request".
#[must_use]
pub fn find_request_sites(image: &[u8]) -> RequestSites {
    let writable = writable_file_ranges(image);
    let in_writable = |off: usize, len: usize| {
        writable.is_empty()
            || off.checked_add(len).is_some_and(|end| {
                writable
                    .iter()
                    .any(|&(start, stop)| off >= start && end <= stop)
            })
    };
    // Bound the scan to the marker window: the first start marker in a
    // writable segment, and the first end marker after it.
    let start = find_first_aligned(image, &REQUESTS_START_MARKER, 0, image.len(), in_writable);
    let scan_from = start.map_or(0, |s| {
        s.saturating_add(REQUESTS_START_MARKER.len().saturating_mul(8))
    });
    let scan_to = find_first_aligned(
        image,
        &REQUESTS_END_MARKER,
        scan_from,
        image.len(),
        in_writable,
    )
    .unwrap_or(image.len());

    let mut sites = RequestSites::default();
    let mut off = scan_from & !7usize;
    while let Some(end) = off.checked_add(16) {
        if end > scan_to {
            break;
        }
        if !in_writable(off, 16) {
            // Not where a request can be (an unmarked image scanned whole).
        } else if words_match(image, off, &BASE_REVISION_MAGIC) {
            sites.base_revision = Some(off);
        } else if words_match(image, off, &COMMON_MAGIC) {
            // A request: its feature id is the next two words.
            if let Some(id_at) = off.checked_add(16) {
                let fid = [
                    le_u64(image, id_at).unwrap_or(0),
                    le_u64(image, id_at.saturating_add(8)).unwrap_or(0),
                ];
                match fid {
                    feature_id::MEMMAP => sites.memmap = Some(off),
                    feature_id::HHDM => sites.hhdm = Some(off),
                    feature_id::FRAMEBUFFER => sites.framebuffer = Some(off),
                    feature_id::RSDP => sites.rsdp = Some(off),
                    feature_id::EXECUTABLE_ADDRESS => sites.executable_address = Some(off),
                    feature_id::KERNEL_FILE => sites.kernel_file = Some(off),
                    _ => {}
                }
            }
        }
        off = match off.checked_add(8) {
            Some(o) => o,
            None => break,
        };
    }
    sites
}

// ---------------------------------------------------------------------------
// Limine response building
// ---------------------------------------------------------------------------

/// A bump allocator over the handoff's staging memory.
///
/// The responses the new kernel reads must live at known physical addresses so
/// their cross-references (the memory map's entry-pointer array, say) can be
/// written as the HHDM virtual addresses the new kernel will dereference — the
/// same HHDM offset the running kernel uses. The arena owns a byte buffer that
/// *is* a physical frame run (its first byte at `phys_base`), hands out aligned
/// sub-ranges, and reports each one's HHDM pointer so a later field can point at
/// it.
///
/// It is written to be testable off a real frame: a self-test backs it with an
/// ordinary `Vec` and a fabricated `phys_base`, and checks the bytes and
/// pointers without performing a handoff.
pub struct HandoffArena<'a> {
    /// The staging bytes — in a real handoff, the HHDM view of a frame run.
    buf: &'a mut [u8],
    /// The physical address of `buf[0]`.
    phys_base: u64,
    /// The HHDM offset the new kernel will use (equal to the running kernel's).
    hhdm: u64,
    /// Bytes handed out so far.
    used: usize,
}

impl<'a> HandoffArena<'a> {
    /// Create an arena over `buf`, whose first byte is at physical `phys_base`,
    /// for a kernel that maps physical memory at `hhdm`.
    #[must_use]
    pub fn new(buf: &'a mut [u8], phys_base: u64, hhdm: u64) -> Self {
        Self {
            buf,
            phys_base,
            hhdm,
            used: 0,
        }
    }

    /// Bytes used so far — how much of the staging run the handoff needs.
    #[must_use]
    pub fn used(&self) -> usize {
        self.used
    }

    /// Reserve `size` bytes aligned to `align` (a power of two), zeroed.
    ///
    /// Returns the sub-range's offset within the buffer and its HHDM pointer, or
    /// `None` if the arena is full or an address computation overflows.
    fn reserve(&mut self, size: usize, align: usize) -> Option<(usize, u64)> {
        let start = self.used.checked_next_multiple_of(align)?;
        let end = start.checked_add(size)?;
        let slot = self.buf.get_mut(start..end)?;
        slot.fill(0);
        self.used = end;
        let ptr = self
            .phys_base
            .checked_add(u64::try_from(start).ok()?)?
            .checked_add(self.hhdm)?;
        Some((start, ptr))
    }

    /// Write `bytes` at `off`, or `None` if it would run past the buffer.
    fn put(&mut self, off: usize, bytes: &[u8]) -> Option<()> {
        let end = off.checked_add(bytes.len())?;
        self.buf.get_mut(off..end)?.copy_from_slice(bytes);
        Some(())
    }
}

/// Build a two-`u64` response (`revision = 0`, then `value`) — the shape of the
/// HHDM and RSDP responses. Returns the response's HHDM pointer.
fn build_pair_response(arena: &mut HandoffArena<'_>, value: u64) -> Option<u64> {
    let (off, ptr) = arena.reserve(16, 8)?;
    arena.put(off, &0u64.to_le_bytes())?;
    arena.put(off.checked_add(8)?, &value.to_le_bytes())?;
    Some(ptr)
}

/// Build the HHDM response (`revision`, `offset`). Returns its HHDM pointer.
pub fn build_hhdm_response(arena: &mut HandoffArena<'_>, hhdm_offset: u64) -> Option<u64> {
    build_pair_response(arena, hhdm_offset)
}

/// Build the RSDP response (`revision`, `address`). Returns its HHDM pointer.
///
/// `address` is physical under base revision 3, which is what the running kernel
/// holds and passes straight through.
pub fn build_rsdp_response(arena: &mut HandoffArena<'_>, rsdp_address: u64) -> Option<u64> {
    build_pair_response(arena, rsdp_address)
}

/// Build the executable-address response (`revision`, `physical_base`,
/// `virtual_base`). Returns its HHDM pointer.
///
/// `physical_base` is where the handoff places the new image (the contiguous
/// destination), `virtual_base` its lowest linked address, so the new kernel's
/// own virtual-to-physical (`v - virtual_base + physical_base`) is correct.
pub fn build_executable_address_response(
    arena: &mut HandoffArena<'_>,
    physical_base: u64,
    virtual_base: u64,
) -> Option<u64> {
    let (off, ptr) = arena.reserve(24, 8)?;
    arena.put(off, &0u64.to_le_bytes())?;
    arena.put(off.checked_add(8)?, &physical_base.to_le_bytes())?;
    arena.put(off.checked_add(16)?, &virtual_base.to_le_bytes())?;
    Some(ptr)
}

/// Build the memory-map response: the entries, an array of pointers to them, and
/// the response pointing at that array — the three-level shape the kernel reads
/// (`MemmapResponse` → `entries_ptr` → each `MemmapEntry`). Returns the
/// response's HHDM pointer.
///
/// `entries` is the adjusted map the new kernel should see: the firmware's, with
/// the new image marked executable-and-modules and the handoff regions
/// bootloader-reclaimable. Each tuple is `(base, length, type)`.
pub fn build_memmap_response(
    arena: &mut HandoffArena<'_>,
    entries: &[(u64, u64, u64)],
) -> Option<u64> {
    // The entries themselves, remembering each one's HHDM pointer.
    let mut entry_ptrs: alloc::vec::Vec<u64> = alloc::vec::Vec::with_capacity(entries.len());
    for &(base, length, type_) in entries {
        let (off, ptr) = arena.reserve(24, 8)?;
        arena.put(off, &base.to_le_bytes())?;
        arena.put(off.checked_add(8)?, &length.to_le_bytes())?;
        arena.put(off.checked_add(16)?, &type_.to_le_bytes())?;
        entry_ptrs.push(ptr);
    }

    // The array of pointers to the entries.
    let ptr_array_bytes = entry_ptrs.len().checked_mul(8)?;
    let (array_off, array_ptr) = arena.reserve(ptr_array_bytes, 8)?;
    for (i, ep) in entry_ptrs.iter().enumerate() {
        let at = array_off.checked_add(i.checked_mul(8)?)?;
        arena.put(at, &ep.to_le_bytes())?;
    }

    // The response: revision, entry_count, entries_ptr.
    let (off, ptr) = arena.reserve(24, 8)?;
    arena.put(off, &0u64.to_le_bytes())?;
    arena.put(
        off.checked_add(8)?,
        &u64::try_from(entries.len()).ok()?.to_le_bytes(),
    )?;
    arena.put(off.checked_add(16)?, &array_ptr.to_le_bytes())?;
    Some(ptr)
}

/// A framebuffer as the handoff passes it on: the fields of Limine's
/// `limine_framebuffer` at revision 0, which a restarted kernel reads as it
/// read the bootloader's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferDesc {
    /// Its address in the direct map -- the same offset in both kernels.
    pub address: u64,
    /// Width in pixels.
    pub width: u64,
    /// Height in pixels.
    pub height: u64,
    /// Bytes from one row to the next.
    pub pitch: u64,
    /// Bits per pixel.
    pub bpp: u16,
    /// Limine's memory model (1: RGB).
    pub memory_model: u8,
    /// Red, green and blue: each mask's size then its shift.
    pub masks: [u8; 6],
}

impl FramebufferDesc {
    /// The descriptor of a framebuffer the bootloader gave this kernel.
    #[must_use]
    pub fn of(fb: &crate::limine::Framebuffer) -> Self {
        Self {
            address: fb.address as u64,
            width: fb.width,
            height: fb.height,
            pitch: fb.pitch,
            bpp: fb.bpp,
            memory_model: fb.memory_model,
            masks: [
                fb.red_mask_size,
                fb.red_mask_shift,
                fb.green_mask_size,
                fb.green_mask_shift,
                fb.blue_mask_size,
                fb.blue_mask_shift,
            ],
        }
    }

    /// The physical bytes it occupies: from its address less `hhdm`,
    /// `pitch * height` of them. `None` if that does not compute.
    fn phys_range(&self, hhdm: u64) -> Option<PhysRange> {
        let start = self.address.checked_sub(hhdm)?;
        let end = start.checked_add(self.pitch.checked_mul(self.height)?)?;
        Some(PhysRange { start, end })
    }
}

/// The framebuffers of `fbs` a restarted kernel can be given: those wholly
/// inside one `FRAMEBUFFER` entry of `memory_map` -- the memory the handoff
/// maps, write-combining, for them ([`framebuffer_ranges`]). A framebuffer the
/// map does not describe would be handed over unmapped, and the new kernel's
/// first pixel would fault with no handler to say so.
#[must_use]
pub fn passable_framebuffers(
    fbs: &[FramebufferDesc],
    memory_map: &[&MemmapEntry],
    hhdm: u64,
) -> alloc::vec::Vec<FramebufferDesc> {
    let ranges = framebuffer_ranges(memory_map);
    fbs.iter()
        .filter(|fb| {
            fb.phys_range(hhdm).is_some_and(|r| {
                ranges
                    .iter()
                    .any(|m| m.start <= r.start && r.end <= m.end && r.start < r.end)
            })
        })
        .copied()
        .collect()
}

/// The size of Limine's `limine_framebuffer` at revision 0: through `edid`.
const LIMINE_FRAMEBUFFER_SIZE: usize = 64;

/// Build the framebuffer response for `fbs`: each descriptor in Limine's
/// layout (`address`, `width`, `height`, `pitch`, `bpp`, `memory_model`, the
/// six mask bytes, seven unused, then `edid_size` and `edid`, both zero: no
/// EDID is passed on), an array of pointers to them, and the response
/// (`revision` 0, the count, the array). Returns the response's HHDM pointer.
pub fn build_framebuffer_response(
    arena: &mut HandoffArena<'_>,
    fbs: &[FramebufferDesc],
) -> Option<u64> {
    let mut ptrs: alloc::vec::Vec<u64> = alloc::vec::Vec::with_capacity(fbs.len());
    for fb in fbs {
        let (off, ptr) = arena.reserve(LIMINE_FRAMEBUFFER_SIZE, 8)?;
        arena.put(off, &fb.address.to_le_bytes())?;
        arena.put(off.checked_add(8)?, &fb.width.to_le_bytes())?;
        arena.put(off.checked_add(16)?, &fb.height.to_le_bytes())?;
        arena.put(off.checked_add(24)?, &fb.pitch.to_le_bytes())?;
        arena.put(off.checked_add(32)?, &fb.bpp.to_le_bytes())?;
        arena.put(off.checked_add(34)?, &[fb.memory_model])?;
        arena.put(off.checked_add(35)?, &fb.masks)?;
        ptrs.push(ptr);
    }
    let (array_off, array_ptr) = arena.reserve(ptrs.len().checked_mul(8)?, 8)?;
    for (i, p) in ptrs.iter().enumerate() {
        arena.put(array_off.checked_add(i.checked_mul(8)?)?, &p.to_le_bytes())?;
    }
    let (off, ptr) = arena.reserve(24, 8)?;
    arena.put(
        off.checked_add(8)?,
        &u64::try_from(ptrs.len()).ok()?.to_le_bytes(),
    )?;
    arena.put(off.checked_add(16)?, &array_ptr.to_le_bytes())?;
    Some(ptr)
}

/// Build the kernel-file response carrying the command line `cmdline`, and no
/// file: the `LimineFile` (`crate::limine::LimineFile`) it points to has the
/// line, NUL-terminated, an empty path, and a null address and zero size.
/// Returns the response's HHDM pointer.
///
/// The command line is what the new kernel reads from this response
/// (`boot::kernel_cmdline`): the options it was started with -- its
/// self-test switches, its boot deadline, whatever a restart passes on.
/// Without the response it boots with none. The file itself is not carried:
/// it would need a contiguous run the size of the image kept alive across
/// the jump, and a restarted kernel uses it only to name the functions in a
/// backtrace and to restart itself again from its own image
/// (`kexec::reload_self`) -- a caller of `power.reload` hands its image in.
/// So `boot::kernel_file_address` answers `None` in a restarted kernel.
///
/// The line stops at its first NUL, if it has one, as Limine's does.
pub fn build_kernel_file_response(arena: &mut HandoffArena<'_>, cmdline: &[u8]) -> Option<u64> {
    use crate::limine::{KernelFileResponse, LimineFile};
    let line = cmdline.split(|&b| b == 0).next().unwrap_or_default();

    // The line and the (empty) path, each NUL-terminated.
    let (line_off, line_ptr) = arena.reserve(line.len().checked_add(1)?, 1)?;
    arena.put(line_off, line)?;
    let (_, path_ptr) = arena.reserve(1, 1)?;

    // The file: everything zero (revision 0, no address, no size, no media)
    // but the two strings.
    let (file_off, file_ptr) = arena.reserve(
        core::mem::size_of::<LimineFile>(),
        core::mem::align_of::<LimineFile>(),
    )?;
    arena.put(
        file_off.checked_add(core::mem::offset_of!(LimineFile, path))?,
        &path_ptr.to_le_bytes(),
    )?;
    arena.put(
        file_off.checked_add(core::mem::offset_of!(LimineFile, cmdline))?,
        &line_ptr.to_le_bytes(),
    )?;

    // The response: revision 0, then the file.
    let (resp_off, resp_ptr) = arena.reserve(
        core::mem::size_of::<KernelFileResponse>(),
        core::mem::align_of::<KernelFileResponse>(),
    )?;
    arena.put(
        resp_off.checked_add(core::mem::offset_of!(KernelFileResponse, kernel_file))?,
        &file_ptr.to_le_bytes(),
    )?;
    Some(resp_ptr)
}

// ---------------------------------------------------------------------------
// Handoff page tables
// ---------------------------------------------------------------------------

/// A 4 KiB hardware page.
const SIZE_4K: u64 = 0x1000;
/// A 2 MiB page (one PD entry).
const SIZE_2M: u64 = 0x20_0000;
/// A 1 GiB page (one PDPT entry).
const SIZE_1G: u64 = 0x4000_0000;

/// The page tables the new kernel starts on, and the frames they occupy.
///
/// These reproduce what Limine hands a kernel: the higher-half direct map at the
/// running kernel's own HHDM offset (so the trampoline, running through the HHDM,
/// stays valid across the `CR3` switch), and the new image mapped at its linked
/// addresses, each segment with its own permissions, pointing at the contiguous
/// destination. The direct-map leaves are left executable (NX clear) exactly as
/// Limine leaves them; the new kernel hardens its own direct map at boot.
pub struct HandoffTables {
    /// Physical address of the PML4 — what the trampoline loads into `CR3`.
    pub pml4_phys: u64,
    /// Every frame these tables occupy, so the handoff can exclude them from the
    /// image destination and free them if it is abandoned before the jump.
    pub frames: alloc::vec::Vec<PhysFrame>,
}

impl HandoffTables {
    /// Free every table frame — for abandoning a prepared handoff before the
    /// jump. After a successful jump the new kernel owns this memory instead.
    ///
    /// # Safety
    ///
    /// No CPU may be using these tables (`CR3` must not point into them).
    pub unsafe fn free(self) {
        for f in self.frames {
            // SAFETY: each frame came from `alloc_frame_zeroed` in this module,
            // and the caller guarantees it is not the active `CR3`. A free error
            // on this abandon path is not actionable, so it is dropped.
            let _ = unsafe { frame::free_frame(f) };
        }
    }
}

/// A zeroed frame for the handoff, wholly below physical `below` -- the image
/// destination's base.
///
/// Every frame the handoff builds in -- page tables, responses, the staged
/// image, the trampoline's control region -- comes from here, so none can lie
/// in the destination the trampoline copies the image into: a collision is
/// impossible by construction rather than refused after the fact. Until
/// 2026-10-08 they came from wherever the allocator liked, and the first
/// self-reload boot (`kexec.selftest=1`) was refused with `InvalidArgument`
/// by the after-the-fact check: the allocator hands out high memory as well
/// as low, and the destination is at the top of the highest region.
fn handoff_frame_zeroed(below: u64) -> KernelResult<PhysFrame> {
    let f = frame::alloc_order_constrained(0, below)?;
    // SAFETY: `f` was just allocated and is ours alone; nothing maps it yet.
    if let Err(e) = unsafe { frame::zero_frame(f) } {
        // SAFETY: as above; it was never handed out.
        let _ = unsafe { frame::free_order(f, 0) };
        return Err(e);
    }
    Ok(f)
}

/// The frames a set of handoff page tables is built in: each allocated below
/// `below` ([`handoff_frame_zeroed`]) and recorded in `list`, so an abandoned
/// handoff can free them.
struct TableFrames<'a> {
    list: &'a mut alloc::vec::Vec<PhysFrame>,
    below: u64,
}

/// Allocate a zeroed frame for a page table, recording it for cleanup.
fn alloc_table(frames: &mut TableFrames<'_>) -> KernelResult<u64> {
    let f = handoff_frame_zeroed(frames.below)?;
    frames.list.push(f);
    Ok(f.addr())
}

/// Write a page-table entry at `table_phys[index]`, reached through the HHDM.
///
/// # Safety
///
/// `table_phys` must be a table frame this module allocated, `hhdm` must map it,
/// and `index` must be `< 512`.
// `index < 512`, so `index * 8 < 4096` and the HHDM add stays in-range; the
// cast of a < 512 index to u64 cannot truncate.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
unsafe fn write_entry(table_phys: u64, index: usize, entry: PageTableEntry, hhdm: u64) {
    let addr = table_phys + hhdm + (index as u64) * 8;
    // SAFETY: the address is within a live, HHDM-mapped table frame (caller).
    unsafe { core::ptr::write_volatile(addr as *mut u64, entry.raw()) };
}

/// Read the page-table entry at `table_phys[index]`, reached through the HHDM.
///
/// # Safety
///
/// As [`write_entry`].
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
unsafe fn read_entry(table_phys: u64, index: usize, hhdm: u64) -> PageTableEntry {
    let addr = table_phys + hhdm + (index as u64) * 8;
    // SAFETY: the address is within a live, HHDM-mapped table frame (caller).
    PageTableEntry::from_raw(unsafe { core::ptr::read_volatile(addr as *const u64) })
}

/// Return the present child table under `table_phys[index]`, creating it if
/// absent. Intermediate entries permit user and write; the leaf's flags decide
/// the actual access.
///
/// # Safety
///
/// As [`write_entry`]; `frames` collects any newly allocated table.
unsafe fn next_table(
    table_phys: u64,
    index: usize,
    hhdm: u64,
    frames: &mut TableFrames<'_>,
) -> KernelResult<u64> {
    // SAFETY: caller's contract on table_phys/hhdm/index.
    let existing = unsafe { read_entry(table_phys, index, hhdm) };
    if existing.is_present() {
        if existing.is_huge() {
            // A huge mapping already covers this range; we cannot descend into it.
            return Err(KernelError::InvalidArgument);
        }
        return Ok(existing.phys_addr());
    }
    let child = alloc_table(frames)?;
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER_ACCESSIBLE;
    // SAFETY: as above; `child` is a fresh 4 KiB-aligned table frame.
    unsafe { write_entry(table_phys, index, PageTableEntry::new(child, flags), hhdm) };
    Ok(child)
}

/// Map a 4 KiB page `virt -> phys` with `flags` (PRESENT is added).
///
/// # Safety
///
/// As [`write_entry`]; `pml4` must be a table frame this module allocated.
unsafe fn map_4k(
    pml4: u64,
    virt: u64,
    phys: u64,
    flags: PageFlags,
    hhdm: u64,
    frames: &mut TableFrames<'_>,
) -> KernelResult<()> {
    let va = VirtAddr::new(virt);
    // SAFETY: caller's contract; each level is created or descended in turn.
    unsafe {
        let pdpt = next_table(pml4, va.pml4_index(), hhdm, frames)?;
        let pd = next_table(pdpt, va.pdpt_index(), hhdm, frames)?;
        let pt = next_table(pd, va.pd_index(), hhdm, frames)?;
        write_entry(
            pt,
            va.pt_index(),
            PageTableEntry::new(phys, flags | PageFlags::PRESENT),
            hhdm,
        );
    }
    Ok(())
}

/// Map one huge page `virt -> phys` at the PDPT level (1 GiB) or PD level (2 MiB).
///
/// # Safety
///
/// As [`map_4k`].
unsafe fn map_huge(
    pml4: u64,
    virt: u64,
    phys: u64,
    flags: PageFlags,
    one_gib: bool,
    hhdm: u64,
    frames: &mut TableFrames<'_>,
) -> KernelResult<()> {
    let va = VirtAddr::new(virt);
    let leaf_flags = flags | PageFlags::PRESENT | PageFlags::HUGE_PAGE;
    // SAFETY: caller's contract; huge leaf sits at the PDPT (1 GiB) or PD (2 MiB).
    unsafe {
        let pdpt = next_table(pml4, va.pml4_index(), hhdm, frames)?;
        if one_gib {
            write_entry(
                pdpt,
                va.pdpt_index(),
                PageTableEntry::new(phys, leaf_flags),
                hhdm,
            );
        } else {
            let pd = next_table(pdpt, va.pdpt_index(), hhdm, frames)?;
            write_entry(
                pd,
                va.pd_index(),
                PageTableEntry::new(phys, leaf_flags),
                hhdm,
            );
        }
    }
    Ok(())
}

/// The physical ranges the new kernel's direct map (HHDM) covers: what Limine
/// maps under base revision 3 -- the memory-map entries of type usable,
/// bootloader-reclaimable and executable-and-modules -- each widened to whole
/// 4 KiB pages, sorted, adjacent ones merged.
///
/// Nothing else: not the MMIO holes, not reserved, ACPI or bad memory. The
/// kernel maps device registers itself, uncached, at their HHDM addresses
/// (`apic`, `ioapic`: "no existing mapping conflicts because Limine didn't
/// map this region"), and reaches ACPI tables with `map_4k_if_absent`; a
/// direct map that already covered those addresses with write-back huge pages
/// refused the first and served the device registers cached. That is what the
/// first self-reload to boot through did (2026-10-08: `[apic] WARNING: Failed
/// to map APIC MMIO`, `[ioapic] WARNING: MMIO map failed`), when the handoff
/// mapped everything from 0 to the top of RAM.
///
/// The framebuffer's entries are the other part of Limine's direct map, mapped
/// write-combining rather than write-back: [`framebuffer_ranges`].
#[must_use]
pub fn direct_map_ranges(memory_map: &[&MemmapEntry]) -> alloc::vec::Vec<PhysRange> {
    ranges_of_types(
        memory_map,
        &[
            memmap_type::USABLE,
            memmap_type::BOOTLOADER_RECLAIMABLE,
            memmap_type::EXECUTABLE_AND_MODULES,
        ],
    )
}

/// The physical ranges of `memory_map`'s `FRAMEBUFFER` entries, which the new
/// kernel's direct map covers write-combining, as Limine maps them, so the
/// framebuffers the handoff passes on ([`build_framebuffer_response`]) can be
/// drawn on at the addresses they are given -- widened to whole 4 KiB pages,
/// sorted, adjacent ones merged.
#[must_use]
pub fn framebuffer_ranges(memory_map: &[&MemmapEntry]) -> alloc::vec::Vec<PhysRange> {
    ranges_of_types(memory_map, &[memmap_type::FRAMEBUFFER])
}

/// The entries of `memory_map` whose type is one of `types`, each widened to
/// whole 4 KiB pages, sorted, overlapping or adjacent ones merged.
fn ranges_of_types(memory_map: &[&MemmapEntry], types: &[u64]) -> alloc::vec::Vec<PhysRange> {
    let mut ranges: alloc::vec::Vec<PhysRange> = memory_map
        .iter()
        .filter(|e| types.contains(&e.type_))
        .filter_map(|e| {
            let start = align_down(e.base, SIZE_4K);
            let end = align_up(e.base.checked_add(e.length)?, SIZE_4K)?;
            (start < end).then_some(PhysRange { start, end })
        })
        .collect();
    ranges.sort_unstable_by_key(|r| r.start);
    let mut merged: alloc::vec::Vec<PhysRange> = alloc::vec::Vec::with_capacity(ranges.len());
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

/// Direct-map `range` at `hhdm` with `flags`: 1 GiB pages (where the CPU has
/// them) and 2 MiB pages over the aligned blocks it covers whole, 4 KiB pages
/// at its edges -- so the map covers the range and nothing beside it.
/// `PageFlags::WRITE_COMBINING` is the PWT bit, the same in a huge entry as in
/// a 4 KiB one, so the memory type holds at every size.
///
/// # Safety
///
/// As [`map_4k`]; the ranges mapped into one table must not overlap.
unsafe fn map_direct(
    pml4: u64,
    range: PhysRange,
    flags: PageFlags,
    one_gib: bool,
    hhdm: u64,
    frames: &mut TableFrames<'_>,
) -> KernelResult<()> {
    let mut p = range.start;
    while p < range.end {
        let virt = hhdm.checked_add(p).ok_or(KernelError::InvalidArgument)?;
        let fits = |size: u64| {
            p.is_multiple_of(size) && p.checked_add(size).is_some_and(|e| e <= range.end)
        };
        let step = if one_gib && fits(SIZE_1G) {
            // SAFETY: the caller's contract; a whole aligned 1 GiB of the range.
            unsafe { map_huge(pml4, virt, p, flags, true, hhdm, frames)? };
            SIZE_1G
        } else if fits(SIZE_2M) {
            // SAFETY: as above, 2 MiB.
            unsafe { map_huge(pml4, virt, p, flags, false, hhdm, frames)? };
            SIZE_2M
        } else {
            // SAFETY: as above, one 4 KiB page.
            unsafe { map_4k(pml4, virt, p, flags, hhdm, frames)? };
            SIZE_4K
        };
        p = p.checked_add(step).ok_or(KernelError::InvalidArgument)?;
    }
    Ok(())
}

/// Populate a fresh PML4 with the handoff mappings, returning its physical
/// address. On any error the caller frees `frames`.
fn try_build_tables(
    frames: &mut TableFrames<'_>,
    parsed: &ParsedKernel,
    dest_base: u64,
    hhdm: u64,
    direct_map: &[PhysRange],
    framebuffer_map: &[PhysRange],
) -> KernelResult<u64> {
    let pml4 = alloc_table(frames)?;

    // The higher-half direct map: the ranges Limine would map
    // (`direct_map_ranges`), with the largest pages they allow, writable and
    // executable as Limine leaves them.
    let one_gib = crate::cpu::features().is_some_and(|f| f.page_1g);
    let ram = PageFlags::PRESENT | PageFlags::WRITABLE;
    for &range in direct_map {
        // SAFETY: pml4 and its descendants are freshly allocated tables this
        // module owns, all reachable through `hhdm`; the ranges are disjoint.
        unsafe { map_direct(pml4, range, ram, one_gib, hhdm, frames)? };
    }
    // The framebuffers, write-combining, as Limine maps them; never RAM, so
    // disjoint from the ranges above.
    let fb = ram | PageFlags::WRITE_COMBINING | PageFlags::NO_EXECUTE;
    for &range in framebuffer_map {
        // SAFETY: as above.
        unsafe { map_direct(pml4, range, fb, one_gib, hhdm, frames)? };
    }

    // The new image: 4 KiB pages at its linked addresses, each segment with its
    // own permissions, pointing at the contiguous destination.
    for seg in parsed.segments() {
        let seg_start = seg.vaddr & !(SIZE_4K.wrapping_sub(1));
        let seg_top = seg
            .vaddr
            .checked_add(seg.memsz)
            .and_then(|t| t.checked_next_multiple_of(SIZE_4K))
            .ok_or(KernelError::InvalidArgument)?;
        let mut v = seg_start;
        while v < seg_top {
            let image_off = v
                .checked_sub(parsed.min_vaddr)
                .ok_or(KernelError::InvalidArgument)?;
            let phys = dest_base
                .checked_add(image_off)
                .ok_or(KernelError::InvalidArgument)?;
            let mut flags = PageFlags::empty();
            if seg.writable {
                flags |= PageFlags::WRITABLE;
            }
            if !seg.executable {
                flags |= PageFlags::NO_EXECUTE;
            }
            // SAFETY: as above.
            unsafe { map_4k(pml4, v, phys, flags, hhdm, frames)? };
            v = v.checked_add(SIZE_4K).ok_or(KernelError::InvalidArgument)?;
        }
    }

    Ok(pml4)
}

/// Build the page tables the new kernel will start on.
///
/// `dest_base` is where the handoff places the image (physically contiguous),
/// `hhdm` the direct-map offset to reproduce (the running kernel's own, so one
/// trampoline address is valid in both tables), `direct_map` the disjoint
/// physical ranges of RAM to direct-map ([`direct_map_ranges`]), and
/// `framebuffer_map` those of the framebuffers ([`framebuffer_ranges`]),
/// mapped write-combining. On failure every frame allocated so far is freed.
///
/// # Errors
///
/// [`KernelError::OutOfMemory`] if a table frame cannot be allocated, or
/// [`KernelError::InvalidArgument`] if an address computation overflows.
pub fn build_handoff_tables(
    parsed: &ParsedKernel,
    dest_base: u64,
    hhdm: u64,
    direct_map: &[PhysRange],
    framebuffer_map: &[PhysRange],
) -> KernelResult<HandoffTables> {
    let mut frames: alloc::vec::Vec<PhysFrame> = alloc::vec::Vec::new();
    let built = try_build_tables(
        &mut TableFrames {
            list: &mut frames,
            below: dest_base,
        },
        parsed,
        dest_base,
        hhdm,
        direct_map,
        framebuffer_map,
    );
    match built {
        Ok(pml4_phys) => Ok(HandoffTables { pml4_phys, frames }),
        Err(e) => {
            for f in frames.drain(..) {
                // SAFETY: these tables are installed nowhere (no CPU uses them);
                // a free error while unwinding an allocation failure is not
                // actionable, so it is dropped.
                let _ = unsafe { frame::free_frame(f) };
            }
            Err(e)
        }
    }
}

/// Follow the handoff tables for `virt`, returning the mapped physical address
/// and leaf flags, or `None` if unmapped. Used by the self-test to confirm a
/// built mapping resolves; the page size is folded into the returned physical
/// address (the leaf's frame base plus the in-page offset).
///
/// # Safety
///
/// `pml4` must be a table built by [`build_handoff_tables`], reachable via `hhdm`.
unsafe fn translate(pml4: u64, virt: u64, hhdm: u64) -> Option<(u64, PageFlags)> {
    let va = VirtAddr::new(virt);
    // SAFETY: caller's contract; each level is present-checked before descent.
    unsafe {
        let pml4e = read_entry(pml4, va.pml4_index(), hhdm);
        if !pml4e.is_present() {
            return None;
        }
        let pdpte = read_entry(pml4e.phys_addr(), va.pdpt_index(), hhdm);
        if !pdpte.is_present() {
            return None;
        }
        if pdpte.is_huge() {
            return Some((pdpte.phys_addr(), pdpte.flags()));
        }
        let pde = read_entry(pdpte.phys_addr(), va.pd_index(), hhdm);
        if !pde.is_present() {
            return None;
        }
        if pde.is_huge() {
            return Some((pde.phys_addr(), pde.flags()));
        }
        let pte = read_entry(pde.phys_addr(), va.pt_index(), hhdm);
        if !pte.is_present() {
            return None;
        }
        Some((pte.phys_addr(), pte.flags()))
    }
}

// ---------------------------------------------------------------------------
// Staging the image
// ---------------------------------------------------------------------------

/// One copy the trampoline performs just before the jump: `len` bytes from
/// `src_phys` (a staged source frame) to `dst_phys` (the contiguous
/// destination). Kept small and `Copy` so the whole page list is a plain `Vec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyOp {
    /// Physical address of the staged bytes.
    pub src_phys: u64,
    /// Physical address they are copied to at handoff.
    pub dst_phys: u64,
    /// Number of bytes to copy.
    pub len: u64,
}

/// The image staged for handoff: the source frames holding its segment bytes,
/// and the page list the trampoline replays to place them at the contiguous
/// destination.
///
/// The bytes are staged in ordinary (possibly scattered) frames and copied to
/// the destination *last*, by the trampoline — because the destination may
/// overlap the running kernel, which is dead only once the jump is made.
pub struct StagedImage {
    /// Frames holding the staged segment bytes (filesz copied, the rest zero).
    pub source_frames: alloc::vec::Vec<PhysFrame>,
    /// Source-to-destination copies, in ascending destination order.
    pub copy_ops: alloc::vec::Vec<CopyOp>,
}

impl StagedImage {
    /// Free every staged source frame — for abandoning a prepared handoff
    /// before the jump.
    ///
    /// # Safety
    ///
    /// No handoff may be in progress reading these frames.
    pub unsafe fn free(self) {
        for f in self.source_frames {
            // SAFETY: each frame came from `alloc_frame_zeroed` here and is not
            // in use per the contract above; a free error is not actionable.
            let _ = unsafe { frame::free_frame(f) };
        }
    }
}

/// Stage a kernel image's segments into frames and build the copy list that
/// places them at `dest_base` (plus each segment's offset from `min_vaddr`).
///
/// Each segment is split into [`FRAME_SIZE`] chunks; one zeroed frame is
/// allocated per chunk, the segment's file bytes for that chunk copied in (the
/// rest left zero, which is the BSS), and one [`CopyOp`] recorded. The frames
/// need not be contiguous: the trampoline copies each to its destination.
///
/// On any failure every frame allocated so far is freed.
///
/// # Errors
///
/// [`KernelError::OutOfMemory`] if a frame cannot be allocated, or
/// [`KernelError::InvalidArgument`] if a size or address computation overflows,
/// or a segment's file bytes lie outside `image`.
pub fn stage_image(
    image: &[u8],
    parsed: &ParsedKernel,
    dest_base: u64,
    hhdm: u64,
) -> KernelResult<StagedImage> {
    let mut source_frames: alloc::vec::Vec<PhysFrame> = alloc::vec::Vec::new();
    let mut copy_ops: alloc::vec::Vec<CopyOp> = alloc::vec::Vec::new();
    match stage_segments(
        image,
        parsed,
        dest_base,
        hhdm,
        &mut source_frames,
        &mut copy_ops,
    ) {
        Ok(()) => Ok(StagedImage {
            source_frames,
            copy_ops,
        }),
        Err(e) => {
            for f in source_frames.drain(..) {
                // SAFETY: these frames are used by nothing; freeing on the error
                // path cannot race, and a free error is not actionable.
                let _ = unsafe { frame::free_frame(f) };
            }
            Err(e)
        }
    }
}

/// The body of [`stage_image`]; the caller frees `source_frames` on error.
fn stage_segments(
    image: &[u8],
    parsed: &ParsedKernel,
    dest_base: u64,
    hhdm: u64,
    source_frames: &mut alloc::vec::Vec<PhysFrame>,
    copy_ops: &mut alloc::vec::Vec<CopyOp>,
) -> KernelResult<()> {
    for seg in parsed.segments() {
        let dst_start = dest_base
            .checked_add(
                seg.vaddr
                    .checked_sub(parsed.min_vaddr)
                    .ok_or(KernelError::InvalidArgument)?,
            )
            .ok_or(KernelError::InvalidArgument)?;
        let file_off =
            usize::try_from(seg.file_offset).map_err(|_| KernelError::InvalidArgument)?;
        let filesz = usize::try_from(seg.filesz).map_err(|_| KernelError::InvalidArgument)?;

        let mut done: u64 = 0;
        while done < seg.memsz {
            let chunk = FRAME_U64.min(seg.memsz.saturating_sub(done));
            let f = handoff_frame_zeroed(dest_base)?;
            source_frames.push(f);

            // Copy the file-backed portion of this chunk; the rest stays zero.
            let done_usize = usize::try_from(done).map_err(|_| KernelError::InvalidArgument)?;
            let file_present = filesz.saturating_sub(done_usize); // file bytes left
            let chunk_usize = usize::try_from(chunk).map_err(|_| KernelError::InvalidArgument)?;
            let to_copy = file_present.min(chunk_usize);
            if to_copy > 0 {
                let src_start = file_off
                    .checked_add(done_usize)
                    .ok_or(KernelError::InvalidArgument)?;
                let src_end = src_start
                    .checked_add(to_copy)
                    .ok_or(KernelError::InvalidArgument)?;
                let src = image
                    .get(src_start..src_end)
                    .ok_or(KernelError::InvalidArgument)?;
                let dst_virt = f
                    .addr()
                    .checked_add(hhdm)
                    .ok_or(KernelError::InvalidArgument)?;
                // SAFETY: `dst_virt` is the HHDM view of a frame just allocated
                // here (so mapped and owned), and `to_copy <= FRAME_SIZE`, so the
                // write stays within that frame.
                unsafe {
                    core::ptr::copy_nonoverlapping(src.as_ptr(), dst_virt as *mut u8, to_copy);
                }
            }

            copy_ops.push(CopyOp {
                src_phys: f.addr(),
                dst_phys: dst_start
                    .checked_add(done)
                    .ok_or(KernelError::InvalidArgument)?,
                len: chunk,
            });
            done = done
                .checked_add(chunk)
                .ok_or(KernelError::InvalidArgument)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The memory map the new kernel sees
// ---------------------------------------------------------------------------

/// The firmware memory-map type that is dead once the jump is made becomes
/// `USABLE`: the old kernel image and modules, and the old bootloader-reclaimable
/// memory (its Limine structures), are all free to the new kernel. Every other
/// type is kept.
fn converted_base_type(type_: u64) -> u64 {
    match type_ {
        memmap_type::EXECUTABLE_AND_MODULES | memmap_type::BOOTLOADER_RECLAIMABLE => {
            memmap_type::USABLE
        }
        other => other,
    }
}

/// Build the memory map the new kernel should see, from the firmware's.
///
/// The firmware map is still readable at handoff time — SlateOS never reclaims
/// bootloader-reclaimable memory, so the Limine responses persist — and is the
/// base. Dead old types become `USABLE` ([`converted_base_type`]); then
/// `overlays` impose the handoff's own types on their ranges: the destination as
/// `EXECUTABLE_AND_MODULES` (so the new kernel does not allocate over itself) and
/// the regions it must read during early boot and keep — the handoff page tables
/// (which *become* its kernel tables) and the Limine responses — as
/// `BOOTLOADER_RECLAIMABLE` (which the new kernel's allocator does not touch).
/// The staged source frames and the trampoline control page need no overlay:
/// once the copy and jump are done they are free, so leaving them `USABLE` is
/// correct. ACPI, reserved, framebuffer and bad-memory regions are kept as-is.
///
/// `overlays` is a list of `(range, type)`; the ranges must not overlap one
/// another. Adjacent same-type ranges in the result are merged. A sub-range
/// covered by neither a firmware entry nor an overlay is a gap and is omitted,
/// exactly as the firmware map omits the gaps between its entries.
#[must_use]
pub fn adjust_memory_map(
    firmware: &[&MemmapEntry],
    overlays: &[(PhysRange, u64)],
) -> alloc::vec::Vec<(u64, u64, u64)> {
    // Every edge at which the type can change: firmware entry and overlay bounds.
    let mut bounds: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    for e in firmware {
        bounds.push(e.base);
        bounds.push(e.base.saturating_add(e.length));
    }
    for (r, _) in overlays {
        bounds.push(r.start);
        bounds.push(r.end);
    }
    bounds.sort_unstable();
    bounds.dedup();

    // Classify each gap between consecutive edges: an overlay wins, else the
    // firmware entry that covers it (type converted), else it is a hole.
    let mut raw: alloc::vec::Vec<(u64, u64, u64)> = alloc::vec::Vec::new();
    for pair in bounds.windows(2) {
        let &[lo, hi] = pair else { continue };
        if lo >= hi {
            continue;
        }
        let type_ = overlays
            .iter()
            .find(|(r, _)| r.start <= lo && lo < r.end)
            .map(|(_, t)| *t)
            .or_else(|| {
                firmware
                    .iter()
                    .find(|e| e.base <= lo && lo < e.base.saturating_add(e.length))
                    .map(|e| converted_base_type(e.type_))
            });
        if let Some(t) = type_ {
            raw.push((lo, hi, t));
        }
    }

    // Merge adjacent ranges of the same type into single entries.
    let mut merged: alloc::vec::Vec<(u64, u64, u64)> = alloc::vec::Vec::new();
    for (lo, hi, t) in raw {
        if let Some(last) = merged.last_mut() {
            if last.1 == lo && last.2 == t {
                last.1 = hi;
                continue;
            }
        }
        merged.push((lo, hi, t));
    }

    // Return as (base, length, type).
    merged
        .into_iter()
        .map(|(lo, hi, t)| (lo, hi.saturating_sub(lo), t))
        .collect()
}

// ---------------------------------------------------------------------------
// Patching the requests
// ---------------------------------------------------------------------------

/// The HHDM pointers of the responses the handoff built, to patch into the
/// loaded image's requests. `None` where no response was built -- because the
/// image carries no such request, or the handoff does not answer it -- in which
/// case that request's response pointer is left untouched, exactly as Limine
/// leaves an unmade request's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResponseAddrs {
    /// Pointer to the memory-map response.
    pub memmap: Option<u64>,
    /// Pointer to the HHDM response.
    pub hhdm: Option<u64>,
    /// Pointer to the framebuffer response.
    pub framebuffer: Option<u64>,
    /// Pointer to the RSDP response.
    pub rsdp: Option<u64>,
    /// Pointer to the executable-address response.
    pub executable_address: Option<u64>,
    /// Pointer to the kernel-file response.
    pub kernel_file: Option<u64>,
}

/// Write a little-endian `u64` at `off` in `image`, or fail if out of bounds.
fn write_u64_at(image: &mut [u8], off: usize, value: u64) -> KernelResult<()> {
    let end = off.checked_add(8).ok_or(KernelError::InvalidArgument)?;
    let slot = image
        .get_mut(off..end)
        .ok_or(KernelError::InvalidArgument)?;
    slot.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Write one response pointer into its request's `response` field, when both the
/// request site and the response exist.
fn patch_one(image: &mut [u8], site: Option<usize>, response: Option<u64>) -> KernelResult<()> {
    if let (Some(off), Some(ptr)) = (site, response) {
        let at = off
            .checked_add(REQUEST_RESPONSE_OFFSET)
            .ok_or(KernelError::InvalidArgument)?;
        write_u64_at(image, at, ptr)?;
    }
    Ok(())
}

/// Fill in the loaded image's Limine requests: write each built response's HHDM
/// pointer into its request's `response` field, and set the base-revision tag's
/// revision word to 0 to mark the requested revision supported -- the two writes
/// Limine performs. Requests with no response are left untouched. Operates on the
/// kernel's own mutable copy of the image (never the caller's buffer), before it
/// is staged.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if a site plus its field offset lies outside
/// `image` (a truncated or malformed image).
pub fn patch_requests(
    image: &mut [u8],
    sites: &RequestSites,
    responses: &ResponseAddrs,
) -> KernelResult<()> {
    if let Some(off) = sites.base_revision {
        let at = off
            .checked_add(BASE_REVISION_WORD_OFFSET)
            .ok_or(KernelError::InvalidArgument)?;
        write_u64_at(image, at, 0)?;
    }
    patch_one(image, sites.memmap, responses.memmap)?;
    patch_one(image, sites.hhdm, responses.hhdm)?;
    patch_one(image, sites.framebuffer, responses.framebuffer)?;
    patch_one(image, sites.rsdp, responses.rsdp)?;
    patch_one(
        image,
        sites.executable_address,
        responses.executable_address,
    )?;
    patch_one(image, sites.kernel_file, responses.kernel_file)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Orchestration: preparing the whole handoff
// ---------------------------------------------------------------------------

/// The physical range a single frame occupies.
fn frame_range(f: PhysFrame) -> PhysRange {
    PhysRange {
        start: f.addr(),
        end: f.addr().saturating_add(FRAME_U64),
    }
}

/// A handoff prepared but not yet executed: the page tables, the staged image,
/// and the Limine responses. Dropping it leaks its frames; call
/// [`PreparedHandoff::free`] to abandon one (a later change executes it).
pub struct PreparedHandoff {
    /// The new kernel's entry point (virtual).
    pub entry: u64,
    /// Physical base the image is placed at (its reported `physical_base`).
    pub dest_base: u64,
    /// The handoff page tables (the PML4 for `CR3`, and the table frames).
    tables: HandoffTables,
    /// The staged image: source frames and the trampoline's copy list.
    staged: StagedImage,
    /// The response arena frame(s).
    arena_frames: alloc::vec::Vec<PhysFrame>,
}

impl PreparedHandoff {
    /// Physical address of the PML4 the trampoline loads into `CR3`.
    #[must_use]
    pub fn pml4_phys(&self) -> u64 {
        self.tables.pml4_phys
    }

    /// The source-to-destination copies the trampoline replays.
    #[must_use]
    pub fn copy_ops(&self) -> &[CopyOp] {
        &self.staged.copy_ops
    }

    /// Free every frame this handoff owns, abandoning it before any jump.
    ///
    /// # Safety
    ///
    /// No CPU may be using these tables, and nothing may be reading these frames.
    pub unsafe fn free(self) {
        // SAFETY: caller's contract that nothing uses these.
        unsafe {
            self.tables.free();
            self.staged.free();
        }
        for f in self.arena_frames {
            // SAFETY: arena frames came from this module; a free error on the
            // abandon path is not actionable.
            let _ = unsafe { frame::free_frame(f) };
        }
    }
}

/// What a restarted kernel is told about the machine and its start: the facts
/// Limine's responses carry on a firmware boot, gathered by the caller of
/// [`prepare_handoff`].
#[derive(Clone, Copy)]
pub struct HandoffFacts<'a> {
    /// The firmware's memory map; the handoff hands on an adjusted copy.
    pub memory_map: &'a [&'a MemmapEntry],
    /// The direct-map offset -- the running kernel's own, kept.
    pub hhdm: u64,
    /// The RSDP's physical address, if the machine has ACPI.
    pub rsdp_address: Option<u64>,
    /// The command line, if one is passed on.
    pub cmdline: Option<&'a [u8]>,
    /// The framebuffers to pass on.
    pub framebuffers: &'a [FramebufferDesc],
}

/// Build the Limine responses into the arena frame and return their pointers.
///
/// The memory-map response carries the adjusted map: the destination as
/// `EXECUTABLE_AND_MODULES`, and the page-table and arena frames as
/// `BOOTLOADER_RECLAIMABLE` (the regions the new kernel keeps). A kernel-file
/// response is built when there is a command line to pass on, carrying only
/// it ([`build_kernel_file_response`]), and a framebuffer response when there
/// are framebuffers ([`build_framebuffer_response`]).
fn build_all_responses(
    arena_phys: u64,
    facts: &HandoffFacts<'_>,
    dest_base: u64,
    parsed: &ParsedKernel,
    tables: &HandoffTables,
    arena_all_frames: &[PhysFrame],
) -> KernelResult<ResponseAddrs> {
    let hhdm = facts.hhdm;
    let span = parsed.image_span().ok_or(KernelError::InvalidArgument)?;
    let dest_end = dest_base
        .checked_add(align_up(span, FRAME_U64).ok_or(KernelError::InvalidArgument)?)
        .ok_or(KernelError::InvalidArgument)?;

    // Overlays: the destination is the new image; the page tables and the arena
    // must survive into the new kernel, so they are bootloader-reclaimable.
    let mut overlays: alloc::vec::Vec<(PhysRange, u64)> = alloc::vec::Vec::new();
    overlays.push((
        PhysRange {
            start: dest_base,
            end: dest_end,
        },
        memmap_type::EXECUTABLE_AND_MODULES,
    ));
    for f in &tables.frames {
        overlays.push((frame_range(*f), memmap_type::BOOTLOADER_RECLAIMABLE));
    }
    for f in arena_all_frames {
        overlays.push((frame_range(*f), memmap_type::BOOTLOADER_RECLAIMABLE));
    }
    let adjusted = adjust_memory_map(facts.memory_map, &overlays);

    // The arena is the HHDM view of the arena frame.
    let arena_virt = arena_phys
        .checked_add(hhdm)
        .ok_or(KernelError::InvalidArgument)?;
    // SAFETY: `arena_virt` is the HHDM view of a frame this module just allocated
    // and owns exclusively; it is mapped and `FRAME_SIZE` bytes long, and the
    // borrow lasts only for this function.
    let buf = unsafe { core::slice::from_raw_parts_mut(arena_virt as *mut u8, FRAME_SIZE) };
    let mut arena = HandoffArena::new(buf, arena_phys, hhdm);

    let hhdm_ptr = build_hhdm_response(&mut arena, hhdm).ok_or(KernelError::OutOfMemory)?;
    let mm_ptr = build_memmap_response(&mut arena, &adjusted).ok_or(KernelError::OutOfMemory)?;
    let ea_ptr = build_executable_address_response(&mut arena, dest_base, parsed.min_vaddr)
        .ok_or(KernelError::OutOfMemory)?;
    let rsdp_ptr = match facts.rsdp_address {
        Some(addr) => Some(build_rsdp_response(&mut arena, addr).ok_or(KernelError::OutOfMemory)?),
        None => None,
    };
    let kernel_file_ptr = match facts.cmdline {
        Some(line) => {
            Some(build_kernel_file_response(&mut arena, line).ok_or(KernelError::OutOfMemory)?)
        }
        None => None,
    };
    let framebuffer_ptr = if facts.framebuffers.is_empty() {
        None
    } else {
        Some(
            build_framebuffer_response(&mut arena, facts.framebuffers)
                .ok_or(KernelError::OutOfMemory)?,
        )
    };

    Ok(ResponseAddrs {
        memmap: Some(mm_ptr),
        hhdm: Some(hhdm_ptr),
        framebuffer: framebuffer_ptr,
        rsdp: rsdp_ptr,
        executable_address: Some(ea_ptr),
        kernel_file: kernel_file_ptr,
    })
}

/// Prepare a complete handoff for `image`: parse it, choose a high destination,
/// build the page tables, build the Limine responses, patch them into a copy of
/// the image, and stage that copy. The result holds everything the trampoline
/// needs; nothing is quiesced and no jump is made.
///
/// `facts` is what the new kernel is told ([`HandoffFacts`]): the memory map
/// and HHDM offset; the RSDP (physical under base revision 3), so it finds
/// ACPI; the command line, carried by a kernel-file response with no file
/// ([`build_kernel_file_response`]) -- without one it boots with none; and the
/// framebuffers, so it has a screen. A framebuffer the memory map does not
/// describe is not passed on ([`passable_framebuffers`]).
///
/// On any failure every frame allocated so far is freed.
///
/// # Errors
///
/// Propagates parse/plan/build/stage failures; [`KernelError::OutOfMemory`] if
/// the arena cannot be allocated or overflows; [`KernelError::InvalidArgument`]
/// if a handoff frame collides with the destination (the rare case
/// [`plan_destination_high`] is designed to avoid).
pub fn prepare_handoff(image: &[u8], facts: &HandoffFacts<'_>) -> KernelResult<PreparedHandoff> {
    let memory_map = facts.memory_map;
    let hhdm = facts.hhdm;
    let parsed = parse_kernel_elf(image).map_err(|e| {
        crate::serial_println!("[kexec] the image is not a kernel this can load: {:?}", e);
        e.as_kernel_error()
    })?;
    let span = parsed
        .image_span()
        .ok_or_else(|| refused("the image's span", KernelError::InvalidArgument))?;
    let dest_base = plan_destination_high(memory_map, span).map_err(|e| {
        crate::serial_println!(
            "[kexec] no destination for {:#x} bytes of image: {:?}",
            span,
            e
        );
        e.as_kernel_error()
    })?;
    crate::serial_println!(
        "[kexec] image {:#x} bytes, entry {:#x}, to physical {:#x}",
        span,
        parsed.entry,
        dest_base
    );

    let tables = build_handoff_tables(
        &parsed,
        dest_base,
        hhdm,
        &direct_map_ranges(memory_map),
        &framebuffer_ranges(memory_map),
    )
    .map_err(|e| refused("the page tables", e))?;

    // Only the framebuffers the map describes, which the tables just mapped.
    let framebuffers = passable_framebuffers(facts.framebuffers, memory_map, hhdm);
    if framebuffers.len() < facts.framebuffers.len() {
        crate::serial_println!(
            "[kexec] {} of {} framebuffer(s) lie outside the memory map's framebuffer entries and are not passed on",
            facts.framebuffers.len().saturating_sub(framebuffers.len()),
            facts.framebuffers.len()
        );
    }
    let facts = HandoffFacts {
        framebuffers: &framebuffers,
        ..*facts
    };

    match prepare_after_tables(image, &parsed, &facts, dest_base, span, &tables) {
        Ok((staged, arena_frames)) => Ok(PreparedHandoff {
            entry: parsed.entry,
            dest_base,
            tables,
            staged,
            arena_frames,
        }),
        Err(e) => {
            // SAFETY: the tables are installed nowhere (no CPU uses them).
            unsafe { tables.free() };
            Err(e)
        }
    }
}

/// The part of [`prepare_handoff`] after the page tables are built: the arena,
/// responses, request patching, staging, and the destination-collision check.
/// Frees the arena and staging on its own errors; the caller frees the tables.
fn prepare_after_tables(
    image: &[u8],
    parsed: &ParsedKernel,
    facts: &HandoffFacts<'_>,
    dest_base: u64,
    span: u64,
    tables: &HandoffTables,
) -> KernelResult<(StagedImage, alloc::vec::Vec<PhysFrame>)> {
    let hhdm = facts.hhdm;
    let arena_frame =
        handoff_frame_zeroed(dest_base).map_err(|e| refused("the response arena", e))?;
    let arena_frames = alloc::vec![arena_frame];

    // Build the responses, then patch them into a copy of the image and stage it.
    // On any error, free the arena frame (and staging if it was built).
    let responses = match build_all_responses(
        arena_frame.addr(),
        facts,
        dest_base,
        parsed,
        tables,
        &arena_frames,
    ) {
        Ok(r) => r,
        Err(e) => {
            free_frames(&arena_frames);
            return Err(refused("the Limine responses", e));
        }
    };

    let mut patched = image.to_vec();
    let sites = find_request_sites(&patched);
    if let Err(e) = patch_requests(&mut patched, &sites, &responses) {
        free_frames(&arena_frames);
        return Err(refused("patching the requests", e));
    }

    let staged = match stage_image(&patched, parsed, dest_base, hhdm) {
        Ok(s) => s,
        Err(e) => {
            free_frames(&arena_frames);
            return Err(refused("staging the image", e));
        }
    };

    // The destination must not overlap any handoff frame, or the trampoline's
    // copy into it would clobber a structure the new kernel still needs. Every
    // one was allocated below `dest_base` (`handoff_frame_zeroed`), so this
    // cannot fire; it stays as the check of that.
    let dest = PhysRange {
        start: dest_base,
        end: dest_base.saturating_add(span),
    };
    let collision = tables
        .frames
        .iter()
        .chain(staged.source_frames.iter())
        .chain(arena_frames.iter())
        .find(|f| frame_range(**f).overlaps(dest))
        .copied();
    if let Some(f) = collision {
        crate::serial_println!(
            "[kexec] handoff frame {:#x} lies in the destination {:#x}..{:#x}",
            f.addr(),
            dest.start,
            dest.end
        );
        // SAFETY: nothing uses the staged frames; freeing on this error is safe.
        unsafe { staged.free() };
        free_frames(&arena_frames);
        return Err(KernelError::InvalidArgument);
    }

    Ok((staged, arena_frames))
}

/// Say on the serial line which step of a handoff failed, and pass the error
/// on: a refused restart is otherwise only a code, and the steps share them.
fn refused(step: &str, e: KernelError) -> KernelError {
    crate::serial_println!("[kexec] {} failed: {:?}", step, e);
    e
}

/// Free a set of frames, ignoring errors (used only on handoff error paths).
fn free_frames(frames: &[PhysFrame]) {
    for &f in frames {
        // SAFETY: these frames came from this module and are used by nothing on
        // the error path; a free error is not actionable.
        let _ = unsafe { frame::free_frame(f) };
    }
}

// ---------------------------------------------------------------------------
// Quiesce
// ---------------------------------------------------------------------------

/// Bring the machine to the state Limine hands a fresh kernel, just before the
/// jump: interrupts off, no device DMA in flight, no other CPU running, no timer
/// armed. The new kernel re-initialises every one of these from scratch, so what
/// matters is only that none of them can touch memory or deliver an interrupt
/// during the handoff.
///
/// This is the irreversible point: after it the running kernel can no longer
/// service interrupts or schedule, so it must be immediately followed by the jump
/// into the new image. It is never called from a self-test -- every step would
/// break the running kernel -- only from the handoff execution
/// ([`PreparedHandoff::execute`]).
///
/// # Safety
///
/// The caller must jump into the new kernel immediately after this returns: the
/// machine is left unable to run the old kernel. Only the bootstrap CPU may call
/// it, and a prepared handoff must be ready.
pub unsafe fn quiesce() {
    // 1. No more interrupts on this CPU.
    // SAFETY: we are about to hand off; the old kernel will not run again.
    unsafe { crate::cpu::cli() };

    // 1b. Stop every other CPU where it is, and wait until each has: from here
    //     nothing but this CPU runs the old kernel (`stop_other_cpus`).
    stop_other_cpus();

    // 2. Mask every IOAPIC line, so no device interrupt is delivered to the new
    //    kernel before it has installed its own IDT and re-programmed the IOAPIC.
    crate::ioapic::mask_all();

    // 3. Stop the LAPIC timer (the one interrupt source the kernel itself arms).
    // SAFETY: as above; the new kernel re-arms its own timer.
    unsafe { crate::apic::stop_timer() };

    // 4. Stop all device DMA at the source by clearing bus mastering on every PCI
    //    function. A device left mastering could DMA through the old kernel's
    //    mappings into memory the new kernel is using. (Bus 0 only for now: QEMU's
    //    devices live there; recursing bridges is noted in todo.txt.)
    for dev in crate::pci::scan_bus0() {
        crate::pci::disable_bus_master(dev.address);
    }

    // 5. The IOMMU: SlateOS never enables DMA-remapping translation (the iommu
    //    module is informational only), so there is no translation to turn off.
    //    If that changes, clear the translation-enable bit on every unit here, so
    //    the new kernel's first DMA is not translated through the old tables.

    // 6. Send INIT to every other CPU, leaving each waiting for a SIPI --
    //    exactly the state the firmware leaves them in for the MP request. Each
    //    is parked by step 1b already; one that never answered it is reset all
    //    the same, since an INIT is taken whatever a CPU is doing.
    let self_id = crate::apic::read_id();
    let count = crate::smp::cpu_count();
    for i in 0..count {
        if let Some(apic_id) = crate::smp::cpu_apic_id(i) {
            if apic_id != self_id {
                // SAFETY: handing off; the AP will be re-started by the new kernel.
                unsafe { crate::apic::send_init_ipi(apic_id) };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stopping the other CPUs
// ---------------------------------------------------------------------------

/// Set from the moment a restart starts stopping the other CPUs: an NMI that
/// finds it set parks the CPU it arrives on ([`park_if_restarting`]). Never
/// cleared -- the kernel that set it is about to cease to exist.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// The CPU stopping the others, which its own NMI handler must never park
/// (an NMI from elsewhere -- the hard-lockup watchdog -- can reach it too).
static STOPPER: AtomicUsize = AtomicUsize::new(usize::MAX);

/// The CPUs (bit N = CPU N) parked so far.
static PARKED: AtomicU64 = AtomicU64::new(0);

/// How long [`stop_other_cpus`] waits for every other CPU to park. An NMI is
/// taken within microseconds; the margin is for an emulator's CPU threads
/// under a loaded host.
const STOP_WAIT_NS: u64 = 500_000_000;

/// Bit `cpu` of a CPU mask (0 for a CPU past the mask).
fn kexec_cpu_bit(cpu: usize) -> u64 {
    u32::try_from(cpu)
        .ok()
        .and_then(|c| 1u64.checked_shl(c))
        .unwrap_or(0)
}

/// The NMI handler's first step: if a restart is stopping the other CPUs,
/// park this one where it is -- interrupts off, `hlt` for good, until the
/// INIT that ends it ([`quiesce`]). Returns at once otherwise.
///
/// Parked in the NMI handler, the CPU takes no further NMI (they stay blocked
/// until an `iretq` that never comes) and no interrupt, and runs no more of
/// the old kernel: a lock it held stays held, which nothing will ever ask for
/// again.
pub fn park_if_restarting() {
    if !STOPPING.load(Ordering::Acquire) {
        return;
    }
    let cpu = crate::smp::fast_cpu_index();
    if cpu == STOPPER.load(Ordering::Acquire) {
        return;
    }
    PARKED.fetch_or(kexec_cpu_bit(cpu), Ordering::AcqRel);
    loop {
        // SAFETY: clearing the interrupt flag and halting have no memory
        // effects; with it clear, only an NMI (blocked: this is the NMI
        // handler), an SMI or the coming INIT ends the halt.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

/// Stop every CPU but this one before the jump: send each an NMI, which
/// parks it ([`park_if_restarting`]), and wait until each says it has.
///
/// The INIT [`quiesce`] sends afterwards would stop them too, but not at a
/// moment this CPU can see: an INIT is taken when the target next can, and a
/// CPU still running the old kernel while the trampoline copies the new one
/// over memory could write into it. An NMI rather than an ordinary interrupt
/// because a CPU spinning with interrupts off -- on a lock this one holds, say
/// -- takes it all the same. Linux stops the other CPUs the same way before a
/// kexec (`native_stop_other_cpus`, an NMI for the ones that do not answer).
///
/// A CPU that has not parked within [`STOP_WAIT_NS`] is reported, and reset
/// by the INIT regardless.
fn stop_other_cpus() {
    let me = crate::smp::fast_cpu_index();
    let mut others = 0u64;
    for cpu in 0..crate::smp::cpu_count() {
        if cpu != me && crate::smp::cpu_apic_id(cpu).is_some() {
            others |= kexec_cpu_bit(cpu);
        }
    }
    if others == 0 {
        return;
    }
    STOPPER.store(me, Ordering::Release);
    PARKED.store(0, Ordering::Release);
    STOPPING.store(true, Ordering::SeqCst);
    // SAFETY: the APIC is up (the kernel is running), and every CPU's NMI
    // handler parks on this NMI (`park_if_restarting`).
    unsafe { crate::apic::send_nmi_all_excluding_self() };
    let start = crate::hrtimer::now_ns();
    while PARKED.load(Ordering::Acquire) & others != others {
        if crate::hrtimer::now_ns().saturating_sub(start) > STOP_WAIT_NS {
            // Lock-free: a parked CPU may hold the serial lock.
            crate::emergency_println!(
                "[kexec] CPUs {:#x} did not stop; the INIT resets them regardless",
                others & !PARKED.load(Ordering::Acquire)
            );
            return;
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Trampoline and the jump
// ---------------------------------------------------------------------------

// The handoff trampoline: a position-independent, pure-64-bit blob run from a
// control frame's HHDM alias (valid in both the old and new page tables, which
// share the HHDM offset). It switches to the new page tables, flushes every TLB
// entry, takes a fresh stack, copies the staged image to its contiguous
// destination, loads a fresh GDT and an empty IDT, zeroes the general registers
// and jumps to the new kernel's entry -- the state Limine hands a kernel. `rdi`
// points at a parameter block (the offsets below).
//
// The order is the point:
//
// - **The new tables come first.** The copy writes the destination, which is
//   checked against every handoff frame (the new tables, the staged source, the
//   responses, this control region) but not against the *old* kernel's page
//   tables -- frames of its own the old kernel allocated wherever it liked. A
//   copy run under the old tables could overwrite the very entries translating
//   it. Under the new ones it cannot: they are handoff frames, and they map the
//   HHDM at the same offset, so this blob, its parameters and the copy list stay
//   where they are.
// - **Then CR4: PGE, PCIDE and CET off.** The running kernel maps its own image
//   GLOBAL (`mm::protect`), and a CR3 write keeps global translations: without
//   this the new kernel, linked at the same addresses, would fetch and store
//   through the old kernel's frames until each stale entry happened to be
//   evicted. Clearing PGE (and PCIDE, if set) invalidates every TLB entry of every
//   PCID, globals included, as Linux's `relocate_kernel` does by setting CR4 to a
//   known state. CET off too, so no supervisor IBT or shadow-stack check meets an
//   entry point that was not built for it; the new kernel turns on what it uses.
//   CR0, EFER and PAT stay as they are -- valid long-mode state the new kernel
//   re-initialises as from a Limine boot, and EFER.NXE kept is what makes the
//   handoff tables' NX bits legal.
// - **Then the stack**, before anything pushes: the old one is mapped by the old
//   tables only. The far return below and Limine's 0 return address use this one.
//
// See `todo.txt` (kexec 5a) and design-decisions §1536.
global_asm!(
    ".global kexec_trampoline_start",
    "kexec_trampoline_start:",
    "cld",
    "mov r15, rdi",          // r15 = params
    "mov r14, [r15 + 0x10]", // r14 = hhdm
    // The new page tables (see above).
    "mov rax, [r15 + 0x18]", // new_cr3
    "mov cr3, rax",
    // CR4 to a known state: PGE (bit 7), PCIDE (17) and CET (23) off, which
    // flushes every TLB entry, global ones included.
    "mov rax, cr4",
    "mov rcx, 0xFFFFFFFFFF7DFF7F",
    "and rax, rcx",
    "mov cr4, rax",
    // The fresh stack, from here on.
    "mov rsp, [r15 + 0x28]",
    // Copy the staged image: for each CopyOp{src_phys, dst_phys, len}.
    "mov r13, [r15 + 0x00]", // r13 = copy_count
    "mov r12, [r15 + 0x08]", // r12 = copy_list (HHDM)
    "2:",
    "test r13, r13",
    "jz 3f",
    "mov rsi, [r12 + 0x00]", // src_phys
    "add rsi, r14",          // + hhdm
    "mov rdi, [r12 + 0x08]", // dst_phys
    "add rdi, r14",          // + hhdm
    "mov rcx, [r12 + 0x10]", // len
    "rep movsb",
    "add r12, 24",
    "dec r13",
    "jmp 2b",
    "3:",
    // Load the handoff GDT and an empty IDT.
    "mov rax, [r15 + 0x30]", // gdtr_ptr
    "lgdt [rax]",
    "mov rax, [r15 + 0x38]", // idtr_ptr
    "lidt [rax]",
    // Reload the data segments (selector 0x30), then CS (0x28) via a far return.
    "mov ax, 0x30",
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "mov gs, ax",
    "mov ss, ax",
    "lea rax, [rip + 4f]",
    "push 0x28",
    "push rax",
    "retfq",
    "4:",
    // Limine's 0 return address on the fresh stack, whose top the far return
    // left as it found it.
    "mov rsp, [r15 + 0x28]",
    "sub rsp, 8",
    "mov qword ptr [rsp], 0",
    // Entry point in rax, zero every other general register, and jump.
    "mov rax, [r15 + 0x20]",
    "xor rbx, rbx",
    "xor rcx, rcx",
    "xor rdx, rdx",
    "xor rsi, rsi",
    "xor rdi, rdi",
    "xor rbp, rbp",
    "xor r8, r8",
    "xor r9, r9",
    "xor r10, r10",
    "xor r11, r11",
    "xor r12, r12",
    "xor r13, r13",
    "xor r14, r14",
    "xor r15, r15",
    "jmp rax",
    ".global kexec_trampoline_end",
    "kexec_trampoline_end:",
);

unsafe extern "C" {
    /// First byte of the trampoline blob (a linker symbol, not readable data).
    static kexec_trampoline_start: u8;
    /// One past the last byte of the trampoline blob.
    static kexec_trampoline_end: u8;
}

/// The handoff GDT the trampoline loads: Limine's seven descriptors, so a new
/// kernel that assumes Limine's selectors (CS `0x28`, data `0x30`) is satisfied.
/// The 16- and 32-bit entries are never used in long mode but keep the layout
/// identical to what Limine presents.
const HANDOFF_GDT: [u64; 7] = [
    0,
    0x0000_9B00_0000_FFFF, // 0x08 16-bit code
    0x0000_9300_0000_FFFF, // 0x10 16-bit data
    0x00CF_9B00_0000_FFFF, // 0x18 32-bit code
    0x00CF_9300_0000_FFFF, // 0x20 32-bit data
    0x0020_9B00_0000_0000, // 0x28 64-bit code
    0x0000_9300_0000_0000, // 0x30 64-bit data
];

/// The trampoline blob as bytes, to copy into the control frame.
fn trampoline_bytes() -> &'static [u8] {
    let start = (&raw const kexec_trampoline_start).addr();
    let end = (&raw const kexec_trampoline_end).addr();
    let len = end.saturating_sub(start);
    // SAFETY: the two linker symbols bound a contiguous run of our own `.text`
    // (the `global_asm!` above); the bytes are live for the kernel's lifetime.
    unsafe { core::slice::from_raw_parts(start as *const u8, len) }
}

/// The smallest buddy order whose block holds `bytes`, capped at the allocator's
/// maximum (`BUDDY_MAX_ORDER`).
// `order` stays below `BUDDY_MAX_ORDER` (10), so `FRAME_SIZE << order` is at most
// 16 MiB and cannot overflow a `usize`.
#[allow(clippy::arithmetic_side_effects)]
fn order_for(bytes: usize) -> usize {
    let mut order = 0usize;
    while order < crate::mm::frame::BUDDY_MAX_ORDER && (FRAME_SIZE << order) < bytes {
        order = order.saturating_add(1);
    }
    order
}

/// Round `n` up to a multiple of 16.
fn align16(n: usize) -> usize {
    n.saturating_add(15) & !15usize
}

/// Write a `u64` at `base_virt + off` (possibly unaligned).
///
/// # Safety
///
/// `base_virt + off .. + 8` must be within a mapped, writable region this code
/// owns.
unsafe fn put_u64(base_virt: u64, off: usize, val: u64) {
    let at = base_virt.wrapping_add(off as u64) as *mut u64;
    // SAFETY: caller's contract; unaligned because some fields follow a u16.
    unsafe { core::ptr::write_unaligned(at, val) };
}

/// Write a `u16` at `base_virt + off`.
///
/// # Safety
///
/// As [`put_u64`], for two bytes.
unsafe fn put_u16(base_virt: u64, off: usize, val: u16) {
    let at = base_virt.wrapping_add(off as u64) as *mut u16;
    // SAFETY: caller's contract.
    unsafe { core::ptr::write_unaligned(at, val) };
}

impl PreparedHandoff {
    /// Perform the handoff: build the trampoline control region, quiesce the
    /// machine, and jump into the new kernel. **Does not return on success.**
    ///
    /// Returns a [`KernelError`] only if preparation fails *before* the machine
    /// is quiesced (so the system is unharmed and the handoff has been freed);
    /// once [`quiesce`] runs there is no way back and the function diverges.
    ///
    /// # Safety
    ///
    /// Only the bootstrap CPU may call this, and the caller is committing to the
    /// restart: on success the old kernel ceases to exist. The prepared handoff's
    /// page tables and responses become the new kernel's.
    // The arithmetic is over small offsets within one freshly-allocated region
    // and the casts are usize<->u64 on a 64-bit target; both are checked by the
    // layout and the final `region_size >= needed` guard.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub unsafe fn execute(self) -> KernelError {
        let Some(hhdm) = crate::mm::page_table::hhdm() else {
            // SAFETY: nothing has been quiesced; free the prepared handoff.
            unsafe { self.free() };
            return KernelError::NotSupported;
        };

        let blob = trampoline_bytes();
        let blob_len = blob.len();
        let count = self.staged.copy_ops.len();

        // Lay the control region out: blob, params, GDT, GDTR, IDTR, the copy
        // list, then a stack at the top.
        let params_off = align16(blob_len);
        let gdt_off = params_off + 64;
        let gdtr_off = gdt_off + 56;
        let idtr_off = gdtr_off + 16;
        let copy_list_off = align16(idtr_off + 16);
        let copy_list_bytes = count.saturating_mul(24);
        let stack_bottom = align16(copy_list_off + copy_list_bytes);
        const STACK_SIZE: usize = 0x1_0000; // 64 KiB, as Limine gives
        let needed = stack_bottom.saturating_add(STACK_SIZE);

        let order = order_for(needed);
        // Below the destination, as every handoff frame is
        // (`handoff_frame_zeroed`): the copy into it must not reach the
        // trampoline running it.
        let region = match frame::alloc_order_constrained(order, self.dest_base) {
            Ok(f) => f,
            Err(e) => {
                // SAFETY: nothing quiesced.
                unsafe { self.free() };
                return refused("the trampoline's control region", e);
            }
        };
        let region_phys = region.addr();
        let region_virt = region_phys.wrapping_add(hhdm);
        let region_size = FRAME_U64 << order;

        // Guard the layout actually fits (it will for any real kernel).
        if (region_size as usize) < needed {
            // SAFETY: the region and handoff are unused; free both.
            unsafe {
                let _ = frame::free_order(region, order);
                self.free();
            }
            return KernelError::OutOfMemory;
        }

        // The destination the trampoline copies into must not overlap the control
        // region, or the copy would clobber the trampoline or its data.
        let dest_end = self
            .staged
            .copy_ops
            .iter()
            .map(|o| o.dst_phys.saturating_add(o.len))
            .max()
            .unwrap_or(self.dest_base);
        let dest = PhysRange {
            start: self.dest_base,
            end: dest_end,
        };
        let region_range = PhysRange {
            start: region_phys,
            end: region_phys.wrapping_add(region_size),
        };
        if region_range.overlaps(dest) {
            // SAFETY: nothing quiesced; free the region and the handoff.
            unsafe {
                let _ = frame::free_order(region, order);
                self.free();
            }
            return refused(
                "placing the control region clear of the destination",
                KernelError::InvalidArgument,
            );
        }

        // Fill the control region.
        // SAFETY: `region_virt` is the HHDM view of a fresh contiguous allocation
        // we own exclusively, `region_size >= needed` bytes long; every offset
        // below is within `needed`.
        unsafe {
            core::ptr::copy_nonoverlapping(blob.as_ptr(), region_virt as *mut u8, blob_len);
            for (i, &entry) in HANDOFF_GDT.iter().enumerate() {
                put_u64(region_virt, gdt_off + i * 8, entry);
            }
            let gdt_hhdm = region_phys.wrapping_add(gdt_off as u64).wrapping_add(hhdm);
            put_u16(region_virt, gdtr_off, 0x37); // 7*8 - 1
            put_u64(region_virt, gdtr_off + 2, gdt_hhdm);
            put_u16(region_virt, idtr_off, 0); // empty IDT
            put_u64(region_virt, idtr_off + 2, 0);
            for (i, op) in self.staged.copy_ops.iter().enumerate() {
                let base = copy_list_off + i * 24;
                put_u64(region_virt, base, op.src_phys);
                put_u64(region_virt, base + 8, op.dst_phys);
                put_u64(region_virt, base + 16, op.len);
            }
            // Parameter block (offsets match the trampoline's reads).
            put_u64(region_virt, params_off, count as u64);
            put_u64(
                region_virt,
                params_off + 0x08,
                region_phys
                    .wrapping_add(copy_list_off as u64)
                    .wrapping_add(hhdm),
            );
            put_u64(region_virt, params_off + 0x10, hhdm);
            put_u64(region_virt, params_off + 0x18, self.tables.pml4_phys);
            put_u64(region_virt, params_off + 0x20, self.entry);
            put_u64(
                region_virt,
                params_off + 0x28,
                region_virt.wrapping_add(region_size),
            );
            put_u64(
                region_virt,
                params_off + 0x30,
                region_phys.wrapping_add(gdtr_off as u64).wrapping_add(hhdm),
            );
            put_u64(
                region_virt,
                params_off + 0x38,
                region_phys.wrapping_add(idtr_off as u64).wrapping_add(hhdm),
            );
        }

        let params_hhdm = region_virt.wrapping_add(params_off as u64);
        let blob_hhdm = region_virt;

        // The running kernel maps the direct map no-execute; the trampoline
        // runs from it until -- and after -- the switch to the new tables,
        // which map it executable. Every page the blob covers is let execute
        // here, while a failure can still back out.
        let mut page = blob_hhdm & !(SIZE_4K - 1);
        while page < blob_hhdm.wrapping_add(blob_len as u64) {
            // SAFETY: the direct map's entries for the control region, which
            // this function owns; only this CPU will execute it.
            if let Err(e) = unsafe { allow_execution_at(page) } {
                // SAFETY: nothing quiesced; free the region and the handoff.
                unsafe {
                    let _ = frame::free_order(region, order);
                    self.free();
                }
                return refused("letting the trampoline execute", e);
            }
            page = page.wrapping_add(SIZE_4K);
        }

        // Point of no return: stop the machine, then jump. The handoff's frames
        // (tables, staged image, responses) and this control region become the
        // new kernel's; nothing here is freed.
        // SAFETY: only the BSP reaches here, the control region and new tables are
        // built, and the trampoline never returns to this address space.
        unsafe {
            quiesce();
            asm!(
                "mov rdi, {params}",
                "jmp {blob}",
                params = in(reg) params_hhdm,
                blob = in(reg) blob_hhdm,
                options(noreturn),
            );
        }
    }
}

/// Clear NX for the 4 KiB page holding `virt` in the running page tables, at
/// every level from the PML4 entry down to the leaf -- a 1 GiB, 2 MiB or 4 KiB
/// entry -- and flush that translation from this CPU's TLB.
///
/// The trampoline runs from the control region's direct-map (HHDM) alias,
/// which the new tables map executable, as Limine leaves the direct map, but
/// which the running kernel maps no-execute: the first self-reload boot to
/// reach the jump (2026-10-08) faulted on the trampoline's first instruction
/// (`#PF` at the control region, error 0x11 -- an instruction fetch from a
/// present page). Letting the dying kernel's tables execute the pages the jump
/// needs is the smaller of the two ways round that; the other is a second
/// executable mapping at an address valid in both tables. When the leaf is a
/// huge page the whole 2 MiB or 1 GiB of the direct map it covers becomes
/// executable, for the moments the old kernel has left.
///
/// # Safety
///
/// Only on the way out, from [`PreparedHandoff::execute`]: the page is one the
/// handoff owns, about to be jumped to on this CPU, and the old kernel is
/// committed to ending.
unsafe fn allow_execution_at(virt: u64) -> KernelResult<()> {
    use crate::mm::page_table;
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let v = VirtAddr::new(virt);
    let nx = PageFlags::NO_EXECUTE.bits();
    let mut table = page_table::cr3_to_pml4(page_table::read_cr3());
    let indices = [v.pml4_index(), v.pdpt_index(), v.pd_index(), v.pt_index()];
    for (level, &index) in indices.iter().enumerate() {
        // SAFETY: `table` is the active PML4, or the table a present,
        // non-huge entry above it names; `index` comes from a `VirtAddr`, so
        // it is below 512, and the HHDM maps every table.
        let entry = unsafe { page_table::read_entry(table, index, hhdm) };
        if !entry.is_present() {
            return Err(KernelError::InvalidAddress);
        }
        if entry.raw() & nx != 0 {
            // SAFETY: as above; only the NX bit changes, so the entry still
            // maps what it mapped (the caller's contract covers the rest).
            unsafe {
                page_table::write_entry(
                    table,
                    index,
                    PageTableEntry::from_raw(entry.raw() & !nx),
                    hhdm,
                );
            }
        }
        // The PTE, or a huge PDPT or PD entry, is the leaf.
        if level == 3 || (level > 0 && entry.is_huge()) {
            break;
        }
        table = entry.phys_addr();
    }
    // SAFETY: `invlpg` is always valid in ring 0.
    unsafe { page_table::invlpg(virt) };
    Ok(())
}

/// Reload the running kernel into itself: the boot-test kexec mode's trigger.
///
/// The running kernel's own ELF (the file Limine loaded), restarted into
/// through [`crate::power::reload`] -- the body of `SYS_POWER_RELOAD`, so the
/// kexec boot proves the call's flush and its move to the bootstrap CPU as
/// well as the jump. On success it does not return (the second kernel boots);
/// on any pre-jump failure it returns the [`KernelError`]. The second kernel
/// is given this one's command line without the `kexec.selftest=1` that asked
/// for the reload ([`restart_cmdline`]), so it boots as this one was told to
/// -- skipping the self-tests, say, so the harness need not wait for them
/// twice -- and does not reload again: the self-reload happens exactly once.
///
/// Called only from the boot path under the `kexec.selftest=1` command line, to
/// validate the trampoline (the one part no self-test can exercise). Not reached
/// on an ordinary boot. The caller commits to the restart: on success the old
/// kernel ceases to exist.
pub fn reload_self() -> KernelError {
    let Some(image) = running_image() else {
        return KernelError::NotSupported;
    };
    let cmdline = restart_cmdline(crate::boot::kernel_cmdline_bytes().unwrap_or_default());
    crate::power::reload(image, &cmdline)
}

/// The running kernel's own image: the ELF file Limine loaded, which it keeps
/// mapped for the kernel's lifetime. `None` when Limine did not say where.
#[must_use]
pub fn running_image() -> Option<&'static [u8]> {
    let (addr, size) = crate::boot::kernel_file_address()?;
    // SAFETY: Limine keeps the kernel file mapped, live and unwritten, for the
    // kernel's lifetime (the kernel-file response's contract).
    Some(unsafe { core::slice::from_raw_parts(addr as *const u8, size) })
}

/// Restart into `image`, handing it `cmdline`: gather the running kernel's boot
/// facts (memory map, direct-map offset, RSDP, framebuffers), prepare the
/// handoff, and execute it. Shared by the boot-path self-reload
/// ([`reload_self`]) and `SYS_POWER_RELOAD`.
///
/// On success it does not return: the new kernel boots. On any failure before
/// the machine is quiesced it returns the [`KernelError`], with nothing changed.
///
/// # Safety
///
/// Only the bootstrap CPU may call this ([`quiesce`] INITs every other CPU),
/// and the caller is committing to the restart: on success the old kernel
/// ceases to exist.
pub unsafe fn reload(image: &[u8], cmdline: &[u8]) -> KernelError {
    let Some(hhdm) = crate::mm::page_table::hhdm() else {
        return KernelError::NotSupported;
    };
    let framebuffers: alloc::vec::Vec<FramebufferDesc> = crate::boot::framebuffers()
        .iter()
        .map(|fb| FramebufferDesc::of(fb))
        .collect();
    let facts = HandoffFacts {
        memory_map: crate::boot::memory_map(),
        hhdm,
        rsdp_address: crate::boot::rsdp_address(),
        cmdline: Some(cmdline),
        framebuffers: &framebuffers,
    };
    let prepared = match prepare_handoff(image, &facts) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // SAFETY: the bootstrap CPU is committing to the restart (caller's contract).
    unsafe { prepared.execute() }
}

/// The command line a self-reload passes on: `line`'s words in order, single
/// spaces between them, without `kexec.selftest=1` -- the word that asked for
/// the reload, which would ask again.
#[must_use]
pub fn restart_cmdline(line: &[u8]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(line.len());
    for word in line
        .split(|b| b.is_ascii_whitespace())
        .filter(|w| !w.is_empty() && *w != b"kexec.selftest=1")
    {
        if !out.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(word);
    }
    out
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise the kexec loader on fabricated inputs: ELF parsing, destination
/// planning, Limine request discovery and response building, and -- when the
/// frame allocator is up (so, at boot) -- building real handoff page tables and
/// staging the image, each verified then freed.
///
/// Diagnostic: a failure means the loader would mis-read an image, place the new
/// kernel on memory it still needs, or build a bad handoff -- a bug to surface,
/// but it cannot affect a kernel that never invokes `power.reload`.
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
        Some(
            TEST_VADDR_B
                .wrapping_add(TEST_MEMSZ_B)
                .wrapping_sub(TEST_VADDR_A)
        ),
        "image span"
    );
    let seg_a = parsed
        .segments()
        .first()
        .ok_or(KernelError::InternalError)?;
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

    // ---- the direct map's ranges ----
    // Limine's types only, widened to 4 KiB, sorted, adjacent ones merged; the
    // hole, ACPI NVS and the framebuffer left out.
    {
        let entry = |base: u64, length: u64, type_: u64| MemmapEntry {
            base,
            length,
            type_,
        };
        let fw = [
            entry(0x40_0000, 0x10_0000, memmap_type::EXECUTABLE_AND_MODULES),
            entry(0, 0xa_0000, memmap_type::USABLE),
            entry(0xa_0000, 0x6_0000, memmap_type::RESERVED),
            entry(0x10_0000, 0x10_0800, memmap_type::USABLE),
            entry(0x20_0800, 0xf_f800, memmap_type::BOOTLOADER_RECLAIMABLE),
            entry(0x30_0000, 0x1_0000, memmap_type::ACPI_NVS),
            entry(0x8000_0000, 0x40_0000, memmap_type::FRAMEBUFFER),
        ];
        let refs: alloc::vec::Vec<&MemmapEntry> = fw.iter().collect();
        let got: alloc::vec::Vec<(u64, u64)> = direct_map_ranges(&refs)
            .iter()
            .map(|r| (r.start, r.end))
            .collect();
        selftest::check_eq!(
            got,
            alloc::vec![
                (0, 0xa_0000),
                (0x10_0000, 0x30_0000),
                (0x40_0000, 0x50_0000)
            ],
            "the direct map covers usable, bootloader-reclaimable and executable memory only"
        );
    }

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

    // High placement puts the run at the top of the usable region: the region is
    // [frame, frame*101); a 3-frame run sits at frame*98.
    let dst_high = plan_destination_high(&map, frame.wrapping_mul(3)).map_err(|e| {
        crate::serial_println!("  FAIL: plan_destination_high found nothing: {:?}", e);
        KernelError::InternalError
    })?;
    selftest::check_eq!(
        dst_high,
        frame.wrapping_mul(98),
        "high placement sits at the top"
    );
    selftest::check!(
        plan_destination_high(&map, frame.wrapping_mul(1000)).is_err(),
        "an over-large high request has no destination"
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

    // ---- Limine request discovery ----
    let (reqimg, want) = build_test_request_image();
    let sites = find_request_sites(&reqimg);
    selftest::check_eq!(sites.base_revision, want.base_revision, "base-revision tag");
    selftest::check_eq!(sites.hhdm, want.hhdm, "HHDM request");
    selftest::check_eq!(sites.rsdp, want.rsdp, "RSDP request");
    selftest::check_eq!(sites.memmap, want.memmap, "memmap request");
    selftest::check_eq!(
        sites.executable_address,
        want.executable_address,
        "executable-address request"
    );
    // Requests the image does not carry are not invented.
    selftest::check_eq!(sites.framebuffer, None, "no framebuffer request");
    selftest::check_eq!(sites.kernel_file, None, "no kernel-file request");
    // The response pointer the handoff would patch is reachable in the image,
    // and the layout offsets point where the fabricated blocks put their fields:
    // a request's revision word (0) sits at REQUEST_REVISION_OFFSET, its response
    // pointer (0, unmade) just after, and the base-revision tag's revision (3)
    // at BASE_REVISION_WORD_OFFSET.
    if let Some(off) = sites.hhdm {
        selftest::check!(
            off.saturating_add(REQUEST_RESPONSE_OFFSET)
                .saturating_add(8)
                <= reqimg.len(),
            "the HHDM request's response field is within the image"
        );
        selftest::check_eq!(
            le_u64(&reqimg, off.saturating_add(REQUEST_REVISION_OFFSET)),
            Some(0),
            "the request revision word is where REQUEST_REVISION_OFFSET says"
        );
        selftest::check_eq!(
            le_u64(&reqimg, off.saturating_add(REQUEST_RESPONSE_OFFSET)),
            Some(0),
            "the request response pointer is unmade"
        );
    }
    if let Some(off) = sites.base_revision {
        selftest::check_eq!(
            le_u64(&reqimg, off.saturating_add(BASE_REVISION_WORD_OFFSET)),
            Some(3),
            "the base-revision word is where BASE_REVISION_WORD_OFFSET says"
        );
    }
    // Content outside the marker window is ignored: a stray request magic placed
    // after the end marker must not be picked up.
    selftest::check!(
        find_request_sites(&build_request_image_with_stray_after_end())
            .memmap
            .is_none(),
        "a request past the end marker is out of the window"
    );
    // A second marker window after the first -- a loader's constants in
    // `.rodata`, which this module's are in a SlateOS kernel -- is not the
    // image's: the first one is.
    let (decoyed, hhdm_at) = build_request_image_with_decoy_after_end();
    let found = find_request_sites(&decoyed);
    selftest::check_eq!(
        found.hhdm,
        Some(hhdm_at),
        "the first marker window is the image's"
    );
    selftest::check!(
        found.memmap.is_none() && found.base_revision.is_none(),
        "a second window after it is not"
    );
    // In an ELF, only a writable segment holds requests: a whole window in a
    // read-only segment, though it comes first, is a loader's constants.
    let (elf, memmap_at) = build_test_elf_with_requests();
    let found = find_request_sites(&elf);
    selftest::check_eq!(
        found.memmap,
        Some(memmap_at),
        "the requests in the writable segment are the image's"
    );
    selftest::check!(
        found.hhdm.is_none() && found.base_revision.is_none(),
        "a marker window in a read-only segment is passed over"
    );

    // ---- Limine response building ----
    const FAKE_PHYS: u64 = 0x0020_0000;
    const FAKE_HHDM: u64 = 0xffff_8000_0000_0000;
    // Round-trip an HHDM pointer the builders return back to its buffer offset.
    let to_off = |ptr: u64| -> usize {
        usize::try_from(ptr.wrapping_sub(FAKE_HHDM).wrapping_sub(FAKE_PHYS)).unwrap_or(usize::MAX)
    };
    let entries = [(0u64, 0x1000u64, 0u64), (0x1000u64, 0x2000u64, 5u64)];

    let mut staging = alloc::vec![0u8; 1024];
    // Build every response into the arena, then drop it so the buffer can be
    // read back: the arena holds `staging` mutably for as long as it lives.
    let (hhdm_ptr, mm_ptr, rsdp_ptr, ea_ptr, used) = {
        let mut arena = HandoffArena::new(&mut staging, FAKE_PHYS, FAKE_HHDM);
        let hhdm = build_hhdm_response(&mut arena, FAKE_HHDM).ok_or(KernelError::InternalError)?;
        let mm = build_memmap_response(&mut arena, &entries).ok_or(KernelError::InternalError)?;
        let rsdp =
            build_rsdp_response(&mut arena, 0x000f_e000).ok_or(KernelError::InternalError)?;
        let ea = build_executable_address_response(&mut arena, 0x0100_0000, 0xffff_ffff_8000_0000)
            .ok_or(KernelError::InternalError)?;
        (hhdm, mm, rsdp, ea, arena.used())
    };
    selftest::check!(used <= 1024, "staging stays within the arena");

    // HHDM response: revision 0 then the offset.
    let hhdm_off = to_off(hhdm_ptr);
    selftest::check_eq!(
        le_u64(&staging, hhdm_off),
        Some(0),
        "HHDM response revision"
    );
    selftest::check_eq!(
        le_u64(&staging, hhdm_off.saturating_add(8)),
        Some(FAKE_HHDM),
        "HHDM response offset"
    );

    // Memmap response: two entries reachable through the pointer array.
    let mm_off = to_off(mm_ptr);
    selftest::check_eq!(
        le_u64(&staging, mm_off),
        Some(0),
        "memmap response revision"
    );
    selftest::check_eq!(
        le_u64(&staging, mm_off.saturating_add(8)),
        Some(2),
        "memmap response entry_count"
    );
    // entries_ptr -> the pointer array; pointer[1] -> the second entry; whose
    // (base, length, type) round-trips.
    let array_ptr =
        le_u64(&staging, mm_off.saturating_add(16)).ok_or(KernelError::InternalError)?;
    let entry1_ptr =
        le_u64(&staging, to_off(array_ptr).saturating_add(8)).ok_or(KernelError::InternalError)?;
    let entry1_off = to_off(entry1_ptr);
    selftest::check_eq!(le_u64(&staging, entry1_off), Some(0x1000), "entry 1 base");
    selftest::check_eq!(
        le_u64(&staging, entry1_off.saturating_add(8)),
        Some(0x2000),
        "entry 1 length"
    );
    selftest::check_eq!(
        le_u64(&staging, entry1_off.saturating_add(16)),
        Some(5),
        "entry 1 type"
    );

    // RSDP (revision, address) and executable-address (revision, phys, virt).
    selftest::check_eq!(
        le_u64(&staging, to_off(rsdp_ptr).saturating_add(8)),
        Some(0x000f_e000),
        "RSDP response address"
    );
    let ea_off = to_off(ea_ptr);
    selftest::check_eq!(
        le_u64(&staging, ea_off.saturating_add(8)),
        Some(0x0100_0000),
        "executable-address physical_base"
    );
    selftest::check_eq!(
        le_u64(&staging, ea_off.saturating_add(16)),
        Some(0xffff_ffff_8000_0000),
        "executable-address virtual_base"
    );

    // ---- handoff page tables ----
    // Build real tables (needs the frame allocator, so this runs at boot) for the
    // fabricated two-segment image, confirm a few translations, then free them.
    if let Some(real_hhdm) = crate::mm::page_table::hhdm() {
        // A frame-aligned stand-in destination. At 4 GiB, so the frames the
        // tables and staging are built in -- all from below it
        // (`handoff_frame_zeroed`) -- are plentiful at boot.
        const DEST_BASE: u64 = 0x1_0000_0000;
        // Direct-map one 4 KiB page at 0 and 4 MiB at 4 MiB: the edge pages,
        // the 2 MiB pages, and what lies beside them, with a tiny table count.
        let direct = [
            PhysRange {
                start: 0,
                end: SIZE_4K,
            },
            PhysRange {
                start: 0x40_0000,
                end: 0x80_0000,
            },
        ];
        // And one page of framebuffer, at 2 GiB.
        let framebuffer = [PhysRange {
            start: 0x8000_0000,
            end: 0x8000_1000,
        }];
        let tables = build_handoff_tables(&parsed, DEST_BASE, real_hhdm, &direct, &framebuffer)
            .map_err(|e| {
                crate::serial_println!("  FAIL: build_handoff_tables: {:?}", e);
                KernelError::InternalError
            })?;
        let pml4 = tables.pml4_phys;

        // The framebuffer: write-combining, not executable.
        // SAFETY: `pml4` is the table just built, reachable via `real_hhdm`.
        let fb_map = unsafe { translate(pml4, real_hhdm.wrapping_add(0x8000_0000), real_hhdm) };
        selftest::check!(
            fb_map.is_some_and(|(p, f)| p == 0x8000_0000
                && f.contains(PageFlags::WRITE_COMBINING)
                && f.contains(PageFlags::NO_EXECUTE)),
            "the framebuffer is direct-mapped write-combining and not executable"
        );

        // The direct map: hhdm+0 -> physical 0, writable, executable (NX clear).
        // SAFETY: `pml4` is the table just built, reachable via `real_hhdm`.
        let dm = unsafe { translate(pml4, real_hhdm, real_hhdm) };
        match dm {
            Some((phys, flags)) => {
                selftest::check_eq!(phys, 0, "direct map hhdm+0 -> phys 0");
                selftest::check!(
                    flags.contains(PageFlags::WRITABLE) && !flags.contains(PageFlags::NO_EXECUTE),
                    "direct map is writable and executable"
                );
            }
            None => {
                crate::serial_println!("  FAIL: direct map hhdm+0 is unmapped");
                // SAFETY: nothing uses these tables.
                unsafe { tables.free() };
                return Err(KernelError::InternalError);
            }
        }

        // The R-X segment maps to the destination base, executable, read-only.
        // SAFETY: as above.
        let seg_a_map = unsafe { translate(pml4, TEST_VADDR_A, real_hhdm) };
        selftest::check_eq!(
            seg_a_map.map(|(p, _)| p),
            Some(DEST_BASE),
            "R-X segment maps to the destination base"
        );
        if let Some((_, flags)) = seg_a_map {
            selftest::check!(
                !flags.contains(PageFlags::WRITABLE) && !flags.contains(PageFlags::NO_EXECUTE),
                "R-X segment is read-only and executable"
            );
        }

        // The RW- segment maps to dest + its image offset, writable, non-exec.
        // SAFETY: as above.
        let seg_b_map = unsafe { translate(pml4, TEST_VADDR_B, real_hhdm) };
        let seg_b_off = TEST_VADDR_B.wrapping_sub(TEST_VADDR_A);
        selftest::check_eq!(
            seg_b_map.map(|(p, _)| p),
            Some(DEST_BASE.wrapping_add(seg_b_off)),
            "RW- segment maps to dest + offset"
        );
        if let Some((_, flags)) = seg_b_map {
            selftest::check!(
                flags.contains(PageFlags::WRITABLE) && flags.contains(PageFlags::NO_EXECUTE),
                "RW- segment is writable and non-executable"
            );
        }

        // An address in neither the direct map nor the image is unmapped.
        // SAFETY: as above.
        let gap = unsafe { translate(pml4, 0xffff_ffff_9000_0000, real_hhdm) };
        selftest::check!(gap.is_none(), "an unmapped address translates to None");

        // The direct map covers its ranges and nothing beside them: the page
        // after the one at 0 is not mapped (so an MMIO hole there could still
        // be mapped uncached by the new kernel), the 4 MiB range is two 2 MiB
        // pages, and the address after it is not mapped either.
        // SAFETY: as above, for each.
        let beside = unsafe { translate(pml4, real_hhdm.wrapping_add(SIZE_4K), real_hhdm) };
        selftest::check!(beside.is_none(), "the direct map stops at its range's edge");
        let low_huge = unsafe { translate(pml4, real_hhdm.wrapping_add(0x40_0000), real_hhdm) };
        let high_huge = unsafe { translate(pml4, real_hhdm.wrapping_add(0x7f_f000), real_hhdm) };
        selftest::check_eq!(
            (low_huge.map(|(p, _)| p), high_huge.map(|(p, _)| p)),
            (Some(0x40_0000), Some(0x60_0000)),
            "an aligned 4 MiB range is two 2 MiB pages"
        );
        let after = unsafe { translate(pml4, real_hhdm.wrapping_add(0x80_0000), real_hhdm) };
        selftest::check!(after.is_none(), "nothing past the 4 MiB range is mapped");

        // SAFETY: these tables were never installed in CR3, so freeing is safe.
        unsafe { tables.free() };

        // ---- staging the image ----
        // Each fabricated segment fits in one 16 KiB frame, so staging yields two
        // source frames and two copy ops placing them at the destination.
        let staged = stage_image(&image, &parsed, DEST_BASE, real_hhdm).map_err(|e| {
            crate::serial_println!("  FAIL: stage_image: {:?}", e);
            KernelError::InternalError
        })?;
        selftest::check_eq!(staged.source_frames.len(), 2, "two staged source frames");
        selftest::check_eq!(staged.copy_ops.len(), 2, "two copy ops");
        if let Some(op0) = staged.copy_ops.first() {
            selftest::check_eq!(op0.dst_phys, DEST_BASE, "R-X segment copies to dest base");
            selftest::check_eq!(op0.len, TEST_MEMSZ_A, "R-X copy length is its memsz");
        }
        if let Some(op1) = staged.copy_ops.get(1) {
            selftest::check_eq!(
                op1.dst_phys,
                DEST_BASE.wrapping_add(TEST_VADDR_B.wrapping_sub(TEST_VADDR_A)),
                "RW- segment copies to dest + offset"
            );
            selftest::check_eq!(op1.len, TEST_MEMSZ_B, "RW- copy length is its memsz");
        }
        // The first source frame carries the file byte at offset 0, BSS zero after.
        if let Some(frame0) = staged.source_frames.first() {
            let base = frame0.addr().wrapping_add(real_hhdm);
            // SAFETY: the frame was just allocated and staged by this module, and
            // `base` is its HHDM view; reading the first two bytes stays within it.
            let (b0, b1) = unsafe {
                (
                    core::ptr::read_volatile(base as *const u8),
                    core::ptr::read_volatile((base.wrapping_add(1)) as *const u8),
                )
            };
            selftest::check_eq!(b0, TEST_CONTENT_BYTE, "staged file byte copied");
            selftest::check_eq!(b1, 0, "staged BSS is zero");
        }
        // SAFETY: nothing is reading these frames.
        unsafe { staged.free() };
    }

    // ---- the adjusted memory map (pure) ----
    let fw0 = MemmapEntry {
        base: 0,
        length: 0x10000,
        type_: memmap_type::USABLE,
    };
    // The old kernel image, and old bootloader-reclaimable, both become usable.
    let fw1 = MemmapEntry {
        base: 0x10000,
        length: 0x10000,
        type_: memmap_type::EXECUTABLE_AND_MODULES,
    };
    let fw2 = MemmapEntry {
        base: 0x20000,
        length: 0x1000,
        type_: memmap_type::BOOTLOADER_RECLAIMABLE,
    };
    let fw3 = MemmapEntry {
        base: 0x21000,
        length: 0x1000,
        type_: memmap_type::ACPI_RECLAIMABLE,
    };
    let fw4 = MemmapEntry {
        base: 0x22000,
        length: 0x1000,
        type_: memmap_type::RESERVED,
    };
    let fw: [&MemmapEntry; 5] = [&fw0, &fw1, &fw2, &fw3, &fw4];
    let overlays = [
        (
            PhysRange {
                start: 0x1000,
                end: 0x5000,
            },
            memmap_type::EXECUTABLE_AND_MODULES,
        ),
        (
            PhysRange {
                start: 0x5000,
                end: 0x6000,
            },
            memmap_type::BOOTLOADER_RECLAIMABLE,
        ),
    ];
    let adjusted = adjust_memory_map(&fw, &overlays);
    let expected: [(u64, u64, u64); 6] = [
        (0x0, 0x1000, memmap_type::USABLE),
        (0x1000, 0x4000, memmap_type::EXECUTABLE_AND_MODULES),
        (0x5000, 0x1000, memmap_type::BOOTLOADER_RECLAIMABLE),
        // 0x6000..0x21000: tail of the first usable, the old kernel, and old
        // bootloader-reclaimable all merge into one usable run.
        (0x6000, 0x1b000, memmap_type::USABLE),
        (0x21000, 0x1000, memmap_type::ACPI_RECLAIMABLE),
        (0x22000, 0x1000, memmap_type::RESERVED),
    ];
    selftest::check_eq!(adjusted.len(), expected.len(), "adjusted map entry count");
    for (i, want) in expected.iter().enumerate() {
        selftest::check_eq!(adjusted.get(i), Some(want), "adjusted map entry");
    }

    // ---- the kernel-file response: a command line and no file (pure) ----
    {
        use crate::limine::{KernelFileResponse, LimineFile};
        let mut kf_staging = alloc::vec![0u8; 512];
        let kf_ptr = {
            let mut arena = HandoffArena::new(&mut kf_staging, FAKE_PHYS, FAKE_HHDM);
            build_kernel_file_response(&mut arena, b"selftest.skip=1 a=b\0ignored")
                .ok_or(KernelError::InternalError)?
        };
        let kf_off = to_off(kf_ptr);
        selftest::check_eq!(
            le_u64(&kf_staging, kf_off),
            Some(0),
            "kernel-file response revision"
        );
        let file_ptr = le_u64(
            &kf_staging,
            kf_off.saturating_add(core::mem::offset_of!(KernelFileResponse, kernel_file)),
        )
        .ok_or(KernelError::InternalError)?;
        let file_off = to_off(file_ptr);
        let field = |off: usize| le_u64(&kf_staging, file_off.saturating_add(off));
        selftest::check_eq!(
            field(core::mem::offset_of!(LimineFile, address)),
            Some(0),
            "the restart's kernel file has no address"
        );
        selftest::check_eq!(
            field(core::mem::offset_of!(LimineFile, size)),
            Some(0),
            "the restart's kernel file has no size"
        );
        let line_ptr =
            field(core::mem::offset_of!(LimineFile, cmdline)).ok_or(KernelError::InternalError)?;
        let line_off = to_off(line_ptr);
        let line = kf_staging
            .get(line_off..)
            .and_then(|rest| rest.split(|&b| b == 0).next())
            .unwrap_or_default();
        selftest::check_eq!(
            line,
            b"selftest.skip=1 a=b".as_slice(),
            "the command line, cut at its NUL, NUL-terminated"
        );
        let path_ptr =
            field(core::mem::offset_of!(LimineFile, path)).ok_or(KernelError::InternalError)?;
        selftest::check_eq!(
            kf_staging.get(to_off(path_ptr)).copied(),
            Some(0),
            "the restart's kernel file has an empty path, not a null one"
        );
    }
    // ---- the framebuffer response, and which framebuffers pass (pure) ----
    {
        let screen = FramebufferDesc {
            address: FAKE_HHDM.wrapping_add(0x8000_0000),
            width: 1024,
            height: 768,
            pitch: 4096,
            bpp: 32,
            memory_model: 1,
            masks: [8, 16, 8, 8, 8, 0],
        };
        let stray = FramebufferDesc {
            address: FAKE_HHDM.wrapping_add(0x9000_0000),
            ..screen
        };
        let mut fb_staging = alloc::vec![0u8; 512];
        let fb_ptr = {
            let mut arena = HandoffArena::new(&mut fb_staging, FAKE_PHYS, FAKE_HHDM);
            build_framebuffer_response(&mut arena, &[stray, screen])
                .ok_or(KernelError::InternalError)?
        };
        let resp = to_off(fb_ptr);
        selftest::check_eq!(
            (
                le_u64(&fb_staging, resp),
                le_u64(&fb_staging, resp.saturating_add(8))
            ),
            (Some(0), Some(2)),
            "framebuffer response: revision 0, two framebuffers"
        );
        let array =
            le_u64(&fb_staging, resp.saturating_add(16)).ok_or(KernelError::InternalError)?;
        let second = le_u64(&fb_staging, to_off(array).saturating_add(8))
            .ok_or(KernelError::InternalError)?;
        let at = to_off(second);
        let bytes = fb_staging
            .get(at..at.saturating_add(LIMINE_FRAMEBUFFER_SIZE))
            .unwrap_or_default();
        selftest::check_eq!(
            (
                le_u64(bytes, 0),
                le_u64(bytes, 8),
                le_u64(bytes, 16),
                le_u64(bytes, 24),
                le_u16(bytes, 32),
            ),
            (
                Some(screen.address),
                Some(1024),
                Some(768),
                Some(4096),
                Some(32)
            ),
            "the second descriptor's address, size, pitch and depth, in Limine's layout"
        );
        selftest::check_eq!(
            bytes.get(34..41),
            Some([1u8, 8, 16, 8, 8, 8, 0].as_slice()),
            "its memory model and mask bytes"
        );
        selftest::check_eq!(
            (le_u64(bytes, 48), le_u64(bytes, 56)),
            (Some(0), Some(0)),
            "no EDID is passed on"
        );

        let map_entries = [
            MemmapEntry {
                base: 0x10_0000,
                length: 0x100_0000,
                type_: memmap_type::USABLE,
            },
            MemmapEntry {
                base: 0x8000_0000,
                length: 0x40_0000,
                type_: memmap_type::FRAMEBUFFER,
            },
        ];
        let map_refs: alloc::vec::Vec<&MemmapEntry> = map_entries.iter().collect();
        selftest::check_eq!(
            framebuffer_ranges(&map_refs)
                .iter()
                .map(|r| (r.start, r.end))
                .collect::<alloc::vec::Vec<_>>(),
            alloc::vec![(0x8000_0000, 0x8040_0000)],
            "the framebuffer ranges are the FRAMEBUFFER entries"
        );
        selftest::check_eq!(
            passable_framebuffers(&[stray, screen], &map_refs, FAKE_HHDM),
            alloc::vec![screen],
            "a framebuffer outside the map's framebuffer entries is not passed on"
        );
    }

    selftest::check_eq!(
        restart_cmdline(b"  sched.boot_deadline_ms=9 kexec.selftest=1\tselftest.skip=1 "),
        b"sched.boot_deadline_ms=9 selftest.skip=1".to_vec(),
        "a self-reload passes the command line on without kexec.selftest=1"
    );
    selftest::check_eq!(
        restart_cmdline(b"kexec.selftest=10"),
        b"kexec.selftest=10".to_vec(),
        "only the exact word is dropped"
    );

    // ---- patching the requests (pure) ----
    let (reqbytes, reqsites) = build_test_request_image();
    let mut img = reqbytes;
    // RSDP's response is deliberately left None: its request must stay untouched.
    let resp = ResponseAddrs {
        memmap: Some(0x2222_0000_0000_0000),
        hhdm: Some(0x1111_0000_0000_0000),
        framebuffer: None,
        rsdp: None,
        executable_address: Some(0x4444_0000_0000_0000),
        kernel_file: None,
    };
    patch_requests(&mut img, &reqsites, &resp).map_err(|e| {
        crate::serial_println!("  FAIL: patch_requests: {:?}", e);
        KernelError::InternalError
    })?;
    if let Some(off) = reqsites.base_revision {
        selftest::check_eq!(
            le_u64(&img, off.saturating_add(BASE_REVISION_WORD_OFFSET)),
            Some(0),
            "base revision marked supported"
        );
    }
    if let Some(off) = reqsites.hhdm {
        selftest::check_eq!(
            le_u64(&img, off.saturating_add(REQUEST_RESPONSE_OFFSET)),
            Some(0x1111_0000_0000_0000),
            "HHDM request response patched"
        );
    }
    if let Some(off) = reqsites.executable_address {
        selftest::check_eq!(
            le_u64(&img, off.saturating_add(REQUEST_RESPONSE_OFFSET)),
            Some(0x4444_0000_0000_0000),
            "executable-address request response patched"
        );
    }
    if let Some(off) = reqsites.rsdp {
        selftest::check_eq!(
            le_u64(&img, off.saturating_add(REQUEST_RESPONSE_OFFSET)),
            Some(0),
            "RSDP request left untouched (no response built)"
        );
    }

    // ---- the whole handoff, prepared end to end (integration) ----
    // Against the real memory map and HHDM, prepare a complete handoff for the
    // fabricated image -- parse, plan, page tables, responses, patch, stage,
    // collision check -- then free it. Validates the orchestration without a jump.
    if let Some(real_hhdm) = crate::mm::page_table::hhdm() {
        let fw = crate::boot::memory_map();
        if !fw.is_empty() {
            let img = build_test_elf();
            let facts = HandoffFacts {
                memory_map: fw,
                hhdm: real_hhdm,
                rsdp_address: Some(0x000f_e000),
                cmdline: Some(b"selftest.skip=1"),
                framebuffers: &[],
            };
            match prepare_handoff(&img, &facts) {
                Ok(prep) => {
                    selftest::check!(!prep.copy_ops().is_empty(), "prepared handoff has copy ops");
                    selftest::check_eq!(prep.entry, TEST_ENTRY, "prepared handoff entry point");
                    selftest::check!(prep.pml4_phys() != 0, "prepared handoff has a PML4");
                    selftest::check!(prep.dest_base != 0, "prepared handoff has a destination");
                    // SAFETY: this handoff was never installed in CR3 or jumped to.
                    unsafe { prep.free() };
                }
                Err(e) => {
                    crate::serial_println!("  FAIL: prepare_handoff: {:?}", e);
                    return Err(KernelError::InternalError);
                }
            }
        }
    }

    // ---- trampoline helpers (the jump itself is harness-validated) ----
    // The blob assembled and is non-empty; the layout helpers compute as expected;
    // the GDT has Limine's 64-bit code/data selectors.
    selftest::check!(!trampoline_bytes().is_empty(), "trampoline blob assembled");
    selftest::check_eq!(order_for(1), 0, "order_for(1) is 0");
    selftest::check_eq!(order_for(FRAME_SIZE), 0, "order_for(one frame) is 0");
    selftest::check_eq!(
        order_for(FRAME_SIZE.saturating_add(1)),
        1,
        "order_for(frame+1) is 1"
    );
    selftest::check_eq!(align16(1), 16, "align16(1) is 16");
    selftest::check_eq!(align16(16), 16, "align16(16) is 16");
    selftest::check_eq!(align16(17), 32, "align16(17) is 32");
    selftest::check_eq!(
        HANDOFF_GDT.get(5),
        Some(&0x0020_9B00_0000_0000),
        "GDT 64-bit code descriptor (CS 0x28)"
    );
    selftest::check_eq!(
        HANDOFF_GDT.get(6),
        Some(&0x0000_9300_0000_0000),
        "GDT 64-bit data descriptor (0x30)"
    );
    // put_u16/put_u64 round-trip through a local buffer (no real frame needed).
    let mut scratch = [0u8; 32];
    let scratch_virt = scratch.as_mut_ptr() as u64;
    // SAFETY: writes land within the 32-byte stack buffer (offsets 0 and 2..10).
    unsafe {
        put_u16(scratch_virt, 0, 0x1234);
        put_u64(scratch_virt, 2, 0xDEAD_BEEF_CAFE_F00D);
    }
    selftest::check_eq!(le_u16_bytes(&scratch, 0), 0x1234, "put_u16 round-trip");
    selftest::check_eq!(
        le_u64(&scratch, 2),
        Some(0xDEAD_BEEF_CAFE_F00D),
        "put_u64 round-trip (unaligned)"
    );

    Ok(())
}

/// Read a little-endian `u16` at `off` for the self-test's `put_u16` check.
fn le_u16_bytes(buf: &[u8], off: usize) -> u16 {
    le_u16(buf, off).unwrap_or(0)
}

// Fabricated-ELF parameters, shared between the builder and the assertions.
const TEST_ENTRY: u64 = 0xffff_ffff_8000_1234;
const TEST_VADDR_A: u64 = 0xffff_ffff_8000_0000;
const TEST_MEMSZ_A: u64 = 0x1000;
const TEST_VADDR_B: u64 = 0xffff_ffff_8000_2000;
const TEST_MEMSZ_B: u64 = 0x2000;
/// The one file-content byte both fabricated segments carry, distinct from the
/// zero fill so the staging self-test can tell a copied byte from the BSS.
const TEST_CONTENT_BYTE: u8 = 0xAB;

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
    buf[data_off] = TEST_CONTENT_BYTE;

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

/// Append one Limine request block (common magic + feature id + zeroed revision
/// and response) to a word buffer, for the request-discovery self-test.
fn push_test_request(words: &mut alloc::vec::Vec<u64>, feature: [u64; 2]) {
    words.push(COMMON_MAGIC[0]);
    words.push(COMMON_MAGIC[1]);
    words.push(feature[0]);
    words.push(feature[1]);
    words.push(0); // revision
    words.push(0); // response pointer (what the handoff patches)
}

/// Flatten a word buffer to little-endian bytes.
fn words_to_bytes(words: &[u64]) -> alloc::vec::Vec<u8> {
    let mut bytes = alloc::vec::Vec::with_capacity(words.len().saturating_mul(8));
    for w in words {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    bytes
}

/// Build a fabricated loaded image — start marker, base-revision tag, four
/// requests, end marker — and the [`RequestSites`] [`find_request_sites`] should
/// return for it. Every block is 8-byte aligned because each word is 8 bytes.
// Builds a buffer of known contents; `len * 8` cannot overflow here.
#[allow(clippy::arithmetic_side_effects)]
fn build_test_request_image() -> (alloc::vec::Vec<u8>, RequestSites) {
    let mut words: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    words.extend_from_slice(&REQUESTS_START_MARKER);
    let base_off = words.len() * 8;
    words.push(BASE_REVISION_MAGIC[0]);
    words.push(BASE_REVISION_MAGIC[1]);
    words.push(3); // requested base revision
    let hhdm_off = words.len() * 8;
    push_test_request(&mut words, feature_id::HHDM);
    let rsdp_off = words.len() * 8;
    push_test_request(&mut words, feature_id::RSDP);
    let memmap_off = words.len() * 8;
    push_test_request(&mut words, feature_id::MEMMAP);
    let exec_off = words.len() * 8;
    push_test_request(&mut words, feature_id::EXECUTABLE_ADDRESS);
    words.extend_from_slice(&REQUESTS_END_MARKER);

    let want = RequestSites {
        base_revision: Some(base_off),
        memmap: Some(memmap_off),
        hhdm: Some(hhdm_off),
        framebuffer: None,
        rsdp: Some(rsdp_off),
        executable_address: Some(exec_off),
        kernel_file: None,
    };
    (words_to_bytes(&words), want)
}

/// Build an image whose marker window is followed by a second one -- start
/// marker, base-revision tag, a memory-map request, end marker -- as a
/// loader's own `.rodata` constants follow a SlateOS kernel's `.requests`;
/// and the offset of the first window's HHDM request, the one
/// [`find_request_sites`] must find.
// Builds a buffer of known contents; `len * 8` cannot overflow here.
#[allow(clippy::arithmetic_side_effects)]
fn build_request_image_with_decoy_after_end() -> (alloc::vec::Vec<u8>, usize) {
    let mut words: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    words.extend_from_slice(&REQUESTS_START_MARKER);
    let hhdm_off = words.len() * 8;
    push_test_request(&mut words, feature_id::HHDM);
    words.extend_from_slice(&REQUESTS_END_MARKER);
    // The decoy window.
    words.extend_from_slice(&REQUESTS_START_MARKER);
    words.push(BASE_REVISION_MAGIC[0]);
    words.push(BASE_REVISION_MAGIC[1]);
    words.push(3);
    push_test_request(&mut words, feature_id::MEMMAP);
    words.extend_from_slice(&REQUESTS_END_MARKER);
    (words_to_bytes(&words), hhdm_off)
}

/// Build an ELF whose read-only, executable first segment holds a whole marker
/// window -- start marker, base-revision tag, an HHDM request, end marker --
/// before its writable second segment holds the real one, with a memory-map
/// request; and the offset of that request. [`find_request_sites`] must take
/// the writable segment's: a bootloader writes responses, so requests are
/// writable, and the read-only window is a loader's constants.
// Builds a buffer of known contents at offsets fixed by the ELF layout.
#[allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects
)]
fn build_test_elf_with_requests() -> (alloc::vec::Vec<u8>, usize) {
    const EHSIZE: usize = 64;
    const PHENTSIZE: usize = 56;
    let phoff = EHSIZE;
    let data_off = EHSIZE + 2 * PHENTSIZE;

    let mut decoy: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    decoy.extend_from_slice(&REQUESTS_START_MARKER);
    decoy.push(BASE_REVISION_MAGIC[0]);
    decoy.push(BASE_REVISION_MAGIC[1]);
    decoy.push(3);
    push_test_request(&mut decoy, feature_id::HHDM);
    decoy.extend_from_slice(&REQUESTS_END_MARKER);
    let decoy = words_to_bytes(&decoy);

    let mut real: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    real.extend_from_slice(&REQUESTS_START_MARKER);
    let memmap_in_real = real.len() * 8;
    push_test_request(&mut real, feature_id::MEMMAP);
    real.extend_from_slice(&REQUESTS_END_MARKER);
    let real = words_to_bytes(&real);

    let real_off = data_off + decoy.len();
    let mut buf = alloc::vec![0u8; real_off + real.len()];
    buf[data_off..real_off].copy_from_slice(&decoy);
    buf[real_off..].copy_from_slice(&real);

    buf[0..4].copy_from_slice(&ELF_MAGIC);
    buf[4] = ELFCLASS64;
    buf[5] = ELFDATA2LSB;
    buf[6] = 1; // EV_CURRENT
    buf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    buf[E_MACHINE_OFF..E_MACHINE_OFF + 2].copy_from_slice(&EM_X86_64.to_le_bytes());
    buf[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    buf[E_ENTRY_OFF..E_ENTRY_OFF + 8].copy_from_slice(&TEST_ENTRY.to_le_bytes());
    buf[E_PHOFF_OFF..E_PHOFF_OFF + 8].copy_from_slice(&(phoff as u64).to_le_bytes());
    buf[52..54].copy_from_slice(&(EHSIZE as u16).to_le_bytes()); // e_ehsize
    buf[E_PHENTSIZE_OFF..E_PHENTSIZE_OFF + 2].copy_from_slice(&(PHENTSIZE as u16).to_le_bytes());
    buf[E_PHNUM_OFF..E_PHNUM_OFF + 2].copy_from_slice(&2u16.to_le_bytes());

    let mut write_ph =
        |at: usize, flags: u32, offset: usize, vaddr: u64, memsz: u64, filesz: usize| {
            buf[at + P_TYPE_OFF..at + P_TYPE_OFF + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
            buf[at + P_FLAGS_OFF..at + P_FLAGS_OFF + 4].copy_from_slice(&flags.to_le_bytes());
            buf[at + P_OFFSET_OFF..at + P_OFFSET_OFF + 8]
                .copy_from_slice(&(offset as u64).to_le_bytes());
            buf[at + P_VADDR_OFF..at + P_VADDR_OFF + 8].copy_from_slice(&vaddr.to_le_bytes());
            buf[at + P_FILESZ_OFF..at + P_FILESZ_OFF + 8]
                .copy_from_slice(&(filesz as u64).to_le_bytes());
            buf[at + P_MEMSZ_OFF..at + P_MEMSZ_OFF + 8].copy_from_slice(&memsz.to_le_bytes());
        };
    // The decoy in R-X (PF_R | PF_X), the real window in RW- (PF_R | PF_W).
    write_ph(
        phoff,
        PF_X | 4,
        data_off,
        TEST_VADDR_A,
        TEST_MEMSZ_A,
        decoy.len(),
    );
    write_ph(
        phoff + PHENTSIZE,
        PF_W | 4,
        real_off,
        TEST_VADDR_B,
        TEST_MEMSZ_B,
        real.len(),
    );

    (buf, real_off + memmap_in_real)
}

/// Build an image with a stray request magic *after* the end marker, to prove
/// [`find_request_sites`] honours the marker window.
fn build_request_image_with_stray_after_end() -> alloc::vec::Vec<u8> {
    let mut words: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    words.extend_from_slice(&REQUESTS_START_MARKER);
    push_test_request(&mut words, feature_id::HHDM);
    words.extend_from_slice(&REQUESTS_END_MARKER);
    // Past the end marker: must be ignored.
    push_test_request(&mut words, feature_id::MEMMAP);
    words_to_bytes(&words)
}
