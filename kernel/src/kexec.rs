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
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::mm::page_table::{PageFlags, PageTableEntry, VirtAddr};

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
pub fn plan_destination_high(
    memory_map: &[&MemmapEntry],
    needed: u64,
) -> Result<u64, KexecError> {
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
/// offset within `image[from..to]`, returning the offset of the last match
/// (Limine honours the *last* start marker, if an image carries several).
fn find_last_aligned(image: &[u8], needle: &[u64], from: usize, to: usize) -> Option<usize> {
    let span = needle.len().checked_mul(8)?;
    let limit = to.min(image.len());
    let mut found = None;
    let mut off = from.checked_next_multiple_of(8)?;
    while let Some(end) = off.checked_add(span) {
        if end > limit {
            break;
        }
        if words_match(image, off, needle) {
            found = Some(off);
        }
        off = off.checked_add(8)?;
    }
    found
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
/// The search is bounded to the window between the last requests-start marker
/// and the first requests-end marker after it, as the protocol specifies
/// (base-revision tags included). An image with no markers is scanned whole, so
/// a hand-built or older image is still handled. Each request and the tag are
/// identified by their magic at an 8-byte-aligned offset.
#[must_use]
pub fn find_request_sites(image: &[u8]) -> RequestSites {
    // Bound the scan to the marker window. `find_last_aligned` gives the last
    // start marker; the end marker is the first one after it.
    let start = find_last_aligned(image, &REQUESTS_START_MARKER, 0, image.len());
    let scan_from =
        start.map_or(0, |s| s.saturating_add(REQUESTS_START_MARKER.len().saturating_mul(8)));
    let scan_to = find_last_aligned(image, &REQUESTS_END_MARKER, scan_from, image.len())
        .unwrap_or(image.len());

    let mut sites = RequestSites::default();
    let mut off = scan_from & !7usize;
    while let Some(end) = off.checked_add(16) {
        if end > scan_to {
            break;
        }
        if words_match(image, off, &BASE_REVISION_MAGIC) {
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
    arena.put(off.checked_add(8)?, &u64::try_from(entries.len()).ok()?.to_le_bytes())?;
    arena.put(off.checked_add(16)?, &array_ptr.to_le_bytes())?;
    Some(ptr)
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

/// Allocate a zeroed frame for a page table, recording it for cleanup.
fn alloc_table(frames: &mut alloc::vec::Vec<PhysFrame>) -> KernelResult<u64> {
    let f = frame::alloc_frame_zeroed()?;
    frames.push(f);
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
    frames: &mut alloc::vec::Vec<PhysFrame>,
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
    frames: &mut alloc::vec::Vec<PhysFrame>,
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
    frames: &mut alloc::vec::Vec<PhysFrame>,
) -> KernelResult<()> {
    let va = VirtAddr::new(virt);
    let leaf_flags = flags | PageFlags::PRESENT | PageFlags::HUGE_PAGE;
    // SAFETY: caller's contract; huge leaf sits at the PDPT (1 GiB) or PD (2 MiB).
    unsafe {
        let pdpt = next_table(pml4, va.pml4_index(), hhdm, frames)?;
        if one_gib {
            write_entry(pdpt, va.pdpt_index(), PageTableEntry::new(phys, leaf_flags), hhdm);
        } else {
            let pd = next_table(pdpt, va.pdpt_index(), hhdm, frames)?;
            write_entry(pd, va.pd_index(), PageTableEntry::new(phys, leaf_flags), hhdm);
        }
    }
    Ok(())
}

/// Populate a fresh PML4 with the handoff mappings, returning its physical
/// address. On any error the caller frees `frames`.
fn try_build_tables(
    frames: &mut alloc::vec::Vec<PhysFrame>,
    parsed: &ParsedKernel,
    dest_base: u64,
    hhdm: u64,
    max_phys: u64,
) -> KernelResult<u64> {
    let pml4 = alloc_table(frames)?;

    // The higher-half direct map: all physical RAM up to `max_phys`, with the
    // largest page the CPU supports, writable and executable as Limine leaves it.
    let one_gib = crate::cpu::features().is_some_and(|f| f.page_1g);
    let step = if one_gib { SIZE_1G } else { SIZE_2M };
    let limit = max_phys
        .checked_next_multiple_of(step)
        .ok_or(KernelError::InvalidArgument)?;
    let hhdm_flags = PageFlags::PRESENT | PageFlags::WRITABLE;
    let mut p = 0u64;
    while p < limit {
        let virt = hhdm.checked_add(p).ok_or(KernelError::InvalidArgument)?;
        // SAFETY: pml4 and its descendants are freshly allocated tables this
        // module owns, all reachable through `hhdm`.
        unsafe { map_huge(pml4, virt, p, hhdm_flags, one_gib, hhdm, frames)? };
        p = p.checked_add(step).ok_or(KernelError::InvalidArgument)?;
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
/// trampoline address is valid in both tables), and `max_phys` the top of
/// physical RAM to direct-map. On failure every frame allocated so far is freed.
///
/// # Errors
///
/// [`KernelError::OutOfMemory`] if a table frame cannot be allocated, or
/// [`KernelError::InvalidArgument`] if an address computation overflows.
pub fn build_handoff_tables(
    parsed: &ParsedKernel,
    dest_base: u64,
    hhdm: u64,
    max_phys: u64,
) -> KernelResult<HandoffTables> {
    let mut frames: alloc::vec::Vec<PhysFrame> = alloc::vec::Vec::new();
    match try_build_tables(&mut frames, parsed, dest_base, hhdm, max_phys) {
        Ok(pml4_phys) => Ok(HandoffTables {
            pml4_phys,
            frames,
        }),
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
            .checked_add(seg.vaddr.checked_sub(parsed.min_vaddr).ok_or(KernelError::InvalidArgument)?)
            .ok_or(KernelError::InvalidArgument)?;
        let file_off = usize::try_from(seg.file_offset).map_err(|_| KernelError::InvalidArgument)?;
        let filesz = usize::try_from(seg.filesz).map_err(|_| KernelError::InvalidArgument)?;

        let mut done: u64 = 0;
        while done < seg.memsz {
            let chunk = FRAME_U64.min(seg.memsz.saturating_sub(done));
            let f = frame::alloc_frame_zeroed()?;
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
                    core::ptr::copy_nonoverlapping(
                        src.as_ptr(),
                        dst_virt as *mut u8,
                        to_copy,
                    );
                }
            }

            copy_ops.push(CopyOp {
                src_phys: f.addr(),
                dst_phys: dst_start
                    .checked_add(done)
                    .ok_or(KernelError::InvalidArgument)?,
                len: chunk,
            });
            done = done.checked_add(chunk).ok_or(KernelError::InvalidArgument)?;
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
    patch_one(image, sites.executable_address, responses.executable_address)?;
    patch_one(image, sites.kernel_file, responses.kernel_file)?;
    Ok(())
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

    // High placement puts the run at the top of the usable region: the region is
    // [frame, frame*101); a 3-frame run sits at frame*98.
    let dst_high = plan_destination_high(&map, frame.wrapping_mul(3)).map_err(|e| {
        crate::serial_println!("  FAIL: plan_destination_high found nothing: {:?}", e);
        KernelError::InternalError
    })?;
    selftest::check_eq!(dst_high, frame.wrapping_mul(98), "high placement sits at the top");
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
            off.saturating_add(REQUEST_RESPONSE_OFFSET).saturating_add(8) <= reqimg.len(),
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
        find_request_sites(&build_request_image_with_stray_after_end()).memmap.is_none(),
        "a request past the end marker is out of the window"
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
        let rsdp = build_rsdp_response(&mut arena, 0x000f_e000).ok_or(KernelError::InternalError)?;
        let ea = build_executable_address_response(&mut arena, 0x0100_0000, 0xffff_ffff_8000_0000)
            .ok_or(KernelError::InternalError)?;
        (hhdm, mm, rsdp, ea, arena.used())
    };
    selftest::check!(used <= 1024, "staging stays within the arena");

    // HHDM response: revision 0 then the offset.
    let hhdm_off = to_off(hhdm_ptr);
    selftest::check_eq!(le_u64(&staging, hhdm_off), Some(0), "HHDM response revision");
    selftest::check_eq!(
        le_u64(&staging, hhdm_off.saturating_add(8)),
        Some(FAKE_HHDM),
        "HHDM response offset"
    );

    // Memmap response: two entries reachable through the pointer array.
    let mm_off = to_off(mm_ptr);
    selftest::check_eq!(le_u64(&staging, mm_off), Some(0), "memmap response revision");
    selftest::check_eq!(
        le_u64(&staging, mm_off.saturating_add(8)),
        Some(2),
        "memmap response entry_count"
    );
    // entries_ptr -> the pointer array; pointer[1] -> the second entry; whose
    // (base, length, type) round-trips.
    let array_ptr = le_u64(&staging, mm_off.saturating_add(16)).ok_or(KernelError::InternalError)?;
    let entry1_ptr = le_u64(&staging, to_off(array_ptr).saturating_add(8))
        .ok_or(KernelError::InternalError)?;
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
        const DEST_BASE: u64 = 0x0100_0000; // a frame-aligned stand-in destination
        // Direct-map a single page's worth of physical RAM: rounds up to one
        // huge page, keeping the test's table count tiny.
        let tables = build_handoff_tables(&parsed, DEST_BASE, real_hhdm, SIZE_4K)
            .map_err(|e| {
                crate::serial_println!("  FAIL: build_handoff_tables: {:?}", e);
                KernelError::InternalError
            })?;
        let pml4 = tables.pml4_phys;

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
    let fw0 = MemmapEntry { base: 0, length: 0x10000, type_: memmap_type::USABLE };
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
        (PhysRange { start: 0x1000, end: 0x5000 }, memmap_type::EXECUTABLE_AND_MODULES),
        (PhysRange { start: 0x5000, end: 0x6000 }, memmap_type::BOOTLOADER_RECLAIMABLE),
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

    Ok(())
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
