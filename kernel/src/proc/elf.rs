//! ELF64 binary loader.
//!
//! Parses ELF64 executables and extracts the information needed to load
//! them into a process's address space.  Works from raw byte slices
//! (`&[u8]`) — no filesystem dependency.
//!
//! ## Supported Format
//!
//! - ELF64 only (no 32-bit)
//! - Little-endian only (x86_64 is always LE)
//! - `ET_EXEC` (static executables) and `ET_DYN` (PIE / shared objects)
//! - Machine: `EM_X86_64`
//!
//! ## Design
//!
//! The loader follows a two-phase approach:
//!
//! 1. **Parse** — `ElfFile::parse(bytes)` validates the binary and
//!    extracts headers.  No memory allocation, no page table changes.
//!    Returns an `ElfFile` with accessors for program headers, entry
//!    point, and loadable segments.
//!
//! 2. **Load** — `load_into_address_space(elf, pml4)` allocates frames,
//!    maps them at the correct virtual addresses, and copies segment
//!    data.  This is where physical memory is consumed and page tables
//!    are modified.
//!
//! Separating parse from load lets us validate a binary before
//! committing any resources to it.
//!
//! ## BSS Handling
//!
//! ELF segments may have `memsz > filesz`.  The extra bytes beyond
//! `filesz` are BSS (zero-initialized data).  The loader:
//! 1. Copies `filesz` bytes from the ELF file.
//! 2. Zeros the remaining `memsz - filesz` bytes.
//! 3. Both regions share the same mapped frames.
//!
//! ## References
//!
//! - System V ABI AMD64 Supplement
//! - ELF-64 Object File Format (TIS, December 1998)
//! - Linux `fs/binfmt_elf.c` (reference for segment loading)

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr, USER_SPACE_END};
use crate::serial_println;

// ---------------------------------------------------------------------------
// ELF64 constants
// ---------------------------------------------------------------------------

// ELF magic bytes (e_ident[0..4]).
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

// e_ident indices.
const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;
const EI_VERSION: usize = 6;

// EI_CLASS values.
const ELFCLASS64: u8 = 2;

// EI_DATA values (byte order).
const ELFDATA2LSB: u8 = 1; // Little-endian.

// e_type values.
const ET_EXEC: u16 = 2; // Executable file.
const ET_DYN: u16 = 3; // Shared object / PIE.

// e_machine values.
const EM_X86_64: u16 = 62;

// Program header p_type values.
#[allow(dead_code)] const PT_NULL: u32 = 0;
const PT_LOAD: u32 = 1;
#[allow(dead_code)] const PT_DYNAMIC: u32 = 2;
#[allow(dead_code)] const PT_INTERP: u32 = 3;
#[allow(dead_code)] const PT_NOTE: u32 = 4;
#[allow(dead_code)] const PT_PHDR: u32 = 6;
#[allow(dead_code)] const PT_TLS: u32 = 7;
#[allow(dead_code)] const PT_GNU_EH_FRAME: u32 = 0x6474_E550;
#[allow(dead_code)] const PT_GNU_STACK: u32 = 0x6474_E551;
#[allow(dead_code)] const PT_GNU_RELRO: u32 = 0x6474_E552;

// Segment permission flags (p_flags).
const PF_X: u32 = 0x1; // Execute.
const PF_W: u32 = 0x2; // Write.
const PF_R: u32 = 0x4; // Read.

// Minimum sizes.
const ELF64_EHDR_SIZE: usize = 64;
const ELF64_PHDR_SIZE: usize = 56;
const ELF64_SHDR_SIZE: usize = 64;

// Version.
const EV_CURRENT: u8 = 1;

// ---------------------------------------------------------------------------
// ELF64 Header (e_ident is separate, remaining fields below)
// ---------------------------------------------------------------------------

/// Parsed ELF64 file header.
///
/// All values are already in native byte order (little-endian on x86_64).
#[derive(Debug, Clone, Copy)]
pub struct Elf64Ehdr {
    /// Object file type (`ET_EXEC`, `ET_DYN`, etc.).
    pub e_type: u16,
    /// Architecture (`EM_X86_64`).
    pub e_machine: u16,
    /// Object file version (must be `EV_CURRENT`).
    pub e_version: u32,
    /// Virtual address of program entry point.
    pub e_entry: u64,
    /// Byte offset of program header table in the file.
    pub e_phoff: u64,
    /// Byte offset of section header table in the file.
    pub e_shoff: u64,
    /// Processor-specific flags (0 for x86_64).
    pub e_flags: u32,
    /// Size of this header (should be 64 for ELF64).
    pub e_ehsize: u16,
    /// Size of each program header entry.
    pub e_phentsize: u16,
    /// Number of program header entries.
    pub e_phnum: u16,
    /// Size of each section header entry.
    pub e_shentsize: u16,
    /// Number of section header entries.
    pub e_shnum: u16,
    /// Section header string table index.
    pub e_shstrndx: u16,
}

/// Parsed ELF64 program header (one per segment).
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    /// Segment type (`PT_LOAD`, `PT_NOTE`, etc.).
    pub p_type: u32,
    /// Segment permission flags (`PF_R`, `PF_W`, `PF_X`).
    pub p_flags: u32,
    /// Offset of the segment data in the file.
    pub p_offset: u64,
    /// Virtual address where this segment should be loaded.
    pub p_vaddr: u64,
    /// Physical address (ignored on systems with virtual memory).
    pub p_paddr: u64,
    /// Number of bytes of segment data in the file.
    pub p_filesz: u64,
    /// Number of bytes the segment occupies in memory (≥ `p_filesz`).
    /// The difference (`p_memsz - p_filesz`) is BSS (zero-filled).
    pub p_memsz: u64,
    /// Alignment requirement (0 or 1 = no alignment, else power of 2).
    pub p_align: u64,
}

/// A loadable segment extracted from an ELF file.
///
/// Contains everything needed to map the segment into an address space.
#[derive(Debug, Clone)]
pub struct LoadableSegment {
    /// Virtual address where the segment begins (from `p_vaddr`).
    pub vaddr: u64,
    /// Number of bytes to copy from the file.
    pub file_size: u64,
    /// Total size in memory (includes BSS).
    pub mem_size: u64,
    /// Offset into the ELF file where segment data starts.
    pub file_offset: u64,
    /// Read permission.
    pub readable: bool,
    /// Write permission.
    pub writable: bool,
    /// Execute permission.
    pub executable: bool,
}

// ---------------------------------------------------------------------------
// Parsed ELF file
// ---------------------------------------------------------------------------

/// A parsed ELF64 binary.
///
/// Holds a reference to the raw bytes and provides typed access to
/// headers and segments.  Does not allocate — all data comes from
/// the byte slice.
pub struct ElfFile<'a> {
    /// Raw ELF bytes.
    data: &'a [u8],
    /// Parsed file header.
    pub header: Elf64Ehdr,
}

impl<'a> ElfFile<'a> {
    /// Parse an ELF64 binary from a byte slice.
    ///
    /// Validates:
    /// - Magic bytes (0x7F "ELF")
    /// - Class (64-bit)
    /// - Data encoding (little-endian)
    /// - Version (current)
    /// - Machine (x86_64)
    /// - Type (executable or shared object)
    /// - Program header table fits within the file
    ///
    /// Returns `KernelError::InvalidExecutable` if any check fails.
    pub fn parse(data: &'a [u8]) -> KernelResult<Self> {
        // Minimum size: need at least the ELF header.
        if data.len() < ELF64_EHDR_SIZE {
            return Err(KernelError::InvalidExecutable);
        }

        // Check magic bytes.
        if data[0..4] != ELF_MAGIC {
            return Err(KernelError::InvalidExecutable);
        }

        // Check class: must be ELF64.
        if data[EI_CLASS] != ELFCLASS64 {
            return Err(KernelError::InvalidExecutable);
        }

        // Check data encoding: must be little-endian.
        if data[EI_DATA] != ELFDATA2LSB {
            return Err(KernelError::InvalidExecutable);
        }

        // Check version.
        if data[EI_VERSION] != EV_CURRENT {
            return Err(KernelError::InvalidExecutable);
        }

        // Parse the header fields (all little-endian).
        let header = Elf64Ehdr {
            e_type: read_u16(data, 16),
            e_machine: read_u16(data, 18),
            e_version: read_u32(data, 20),
            e_entry: read_u64(data, 24),
            e_phoff: read_u64(data, 32),
            e_shoff: read_u64(data, 40),
            e_flags: read_u32(data, 48),
            e_ehsize: read_u16(data, 52),
            e_phentsize: read_u16(data, 54),
            e_phnum: read_u16(data, 56),
            e_shentsize: read_u16(data, 58),
            e_shnum: read_u16(data, 60),
            e_shstrndx: read_u16(data, 62),
        };

        // Check machine type.
        if header.e_machine != EM_X86_64 {
            return Err(KernelError::InvalidExecutable);
        }

        // Check object type.
        if header.e_type != ET_EXEC && header.e_type != ET_DYN {
            return Err(KernelError::InvalidExecutable);
        }

        // Check that the ELF version in the header is current.
        if header.e_version != u32::from(EV_CURRENT) {
            return Err(KernelError::InvalidExecutable);
        }

        // Check program header entry size.
        if header.e_phentsize != 0
            && (header.e_phentsize as usize) < ELF64_PHDR_SIZE
        {
            return Err(KernelError::InvalidExecutable);
        }

        // Check that the program header table fits within the file.
        if header.e_phnum > 0 {
            let phdr_end = (header.e_phoff as usize)
                .checked_add(
                    (header.e_phnum as usize)
                        .checked_mul(header.e_phentsize as usize)
                        .ok_or(KernelError::InvalidExecutable)?,
                )
                .ok_or(KernelError::InvalidExecutable)?;

            if phdr_end > data.len() {
                return Err(KernelError::InvalidExecutable);
            }
        }

        // Entry point validation: must be non-zero for executables.
        if header.e_type == ET_EXEC && header.e_entry == 0 {
            return Err(KernelError::InvalidExecutable);
        }

        Ok(Self { data, header })
    }

    /// Returns the virtual address of the program entry point.
    #[must_use]
    pub fn entry_point(&self) -> u64 {
        self.header.e_entry
    }

    /// Returns `true` if this is a position-independent executable (PIE).
    #[must_use]
    pub fn is_pie(&self) -> bool {
        self.header.e_type == ET_DYN
    }

    /// Returns the number of program headers.
    #[must_use]
    pub fn program_header_count(&self) -> usize {
        self.header.e_phnum as usize
    }

    /// Parse program header at the given index.
    ///
    /// Returns `None` if the index is out of bounds.
    #[must_use]
    pub fn program_header(&self, index: usize) -> Option<Elf64Phdr> {
        if index >= self.header.e_phnum as usize {
            return None;
        }

        let offset = (self.header.e_phoff as usize)
            + index * (self.header.e_phentsize as usize);

        // Bounds check: the program header table was validated in parse(),
        // but be defensive.
        if offset + ELF64_PHDR_SIZE > self.data.len() {
            return None;
        }

        Some(Elf64Phdr {
            p_type: read_u32(self.data, offset),
            p_flags: read_u32(self.data, offset + 4),
            p_offset: read_u64(self.data, offset + 8),
            p_vaddr: read_u64(self.data, offset + 16),
            p_paddr: read_u64(self.data, offset + 24),
            p_filesz: read_u64(self.data, offset + 32),
            p_memsz: read_u64(self.data, offset + 40),
            p_align: read_u64(self.data, offset + 48),
        })
    }

    /// Iterate over all `PT_LOAD` segments as [`LoadableSegment`]s.
    ///
    /// Validates each segment:
    /// - `memsz >= filesz`
    /// - Segment data fits within the file
    /// - Virtual address is in the user-space range (for `ET_EXEC`)
    ///
    /// Returns an error if any `PT_LOAD` segment is invalid.
    pub fn loadable_segments(&self) -> KernelResult<LoadableSegments<'_>> {
        // Pre-validate all PT_LOAD segments so callers get a clean
        // error up front rather than halfway through loading.
        for i in 0..self.program_header_count() {
            let Some(phdr) = self.program_header(i) else {
                return Err(KernelError::InvalidExecutable);
            };

            if phdr.p_type != PT_LOAD {
                continue;
            }

            // memsz must be >= filesz (BSS can add bytes, never remove).
            if phdr.p_memsz < phdr.p_filesz {
                return Err(KernelError::InvalidExecutable);
            }

            // Segment file data must fit within the ELF file.
            let data_end = (phdr.p_offset as usize)
                .checked_add(phdr.p_filesz as usize)
                .ok_or(KernelError::InvalidExecutable)?;

            if data_end > self.data.len() {
                return Err(KernelError::InvalidExecutable);
            }

            // For static executables, vaddr must be in user space.
            // PIE executables (ET_DYN) may have vaddr=0 (relocated at load
            // time), so we skip this check for them.
            if self.header.e_type == ET_EXEC {
                let seg_end = phdr
                    .p_vaddr
                    .checked_add(phdr.p_memsz)
                    .ok_or(KernelError::InvalidExecutable)?;

                if seg_end > USER_SPACE_END {
                    return Err(KernelError::InvalidExecutable);
                }
            }
        }

        Ok(LoadableSegments {
            elf: self,
            index: 0,
        })
    }

    /// Get the raw bytes for a segment's file content.
    ///
    /// Returns the slice `[p_offset .. p_offset + p_filesz]`.
    /// Returns `None` if the range is out of bounds.
    #[must_use]
    pub fn segment_data(&self, phdr: &Elf64Phdr) -> Option<&'a [u8]> {
        let start = phdr.p_offset as usize;
        let end = start.checked_add(phdr.p_filesz as usize)?;
        self.data.get(start..end)
    }

    /// Get the total size of the raw ELF data.
    #[must_use]
    pub fn file_size(&self) -> usize {
        self.data.len()
    }
}

// ---------------------------------------------------------------------------
// Loadable segment iterator
// ---------------------------------------------------------------------------

/// Iterator over `PT_LOAD` segments in an ELF file.
pub struct LoadableSegments<'a> {
    elf: &'a ElfFile<'a>,
    index: usize,
}

impl<'a> Iterator for LoadableSegments<'a> {
    type Item = LoadableSegment;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.elf.program_header_count() {
            let idx = self.index;
            self.index += 1;

            let phdr = self.elf.program_header(idx)?;
            if phdr.p_type != PT_LOAD {
                continue;
            }

            // Skip zero-size segments (they're valid but useless).
            if phdr.p_memsz == 0 {
                continue;
            }

            return Some(LoadableSegment {
                vaddr: phdr.p_vaddr,
                file_size: phdr.p_filesz,
                mem_size: phdr.p_memsz,
                file_offset: phdr.p_offset,
                readable: (phdr.p_flags & PF_R) != 0,
                writable: (phdr.p_flags & PF_W) != 0,
                executable: (phdr.p_flags & PF_X) != 0,
            });
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Segment loading into address space
// ---------------------------------------------------------------------------

/// Convert ELF segment flags to page table flags.
///
/// The mapping is:
/// - `PF_R` → `PRESENT` (all readable pages are present)
/// - `PF_W` → `WRITABLE`
/// - No `PF_X` → `NO_EXECUTE`
/// - All userspace pages get `USER_ACCESSIBLE`
#[must_use]
pub fn segment_flags_to_page_flags(seg: &LoadableSegment) -> PageFlags {
    let mut flags = PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE;

    if seg.writable {
        flags = flags | PageFlags::WRITABLE;
    }

    if !seg.executable {
        flags = flags | PageFlags::NO_EXECUTE;
    }

    flags
}

/// Load all `PT_LOAD` segments from a parsed ELF file into an address
/// space.
///
/// For each loadable segment:
/// 1. Allocate physical frames covering `[vaddr .. vaddr + memsz)`,
///    rounded up to frame boundaries.
/// 2. Map the frames into the target address space with appropriate
///    permissions.
/// 3. Copy `filesz` bytes from the ELF data.
/// 4. Zero the remaining `memsz - filesz` bytes (BSS).
///
/// The `pml4_phys` is the physical address of the target process's
/// PML4 page table.  The HHDM is used to write segment data into the
/// newly allocated frames.
///
/// # Errors
///
/// Returns `OutOfMemory` if frame allocation fails.
/// Returns `InvalidExecutable` if any segment is invalid.
/// Returns `InvalidAddress` if a segment maps to a bad virtual address.
///
/// On error, frames already allocated for earlier segments are NOT
/// automatically freed — the caller should destroy the address space
/// (which frees all mapped frames).
///
/// # Safety
///
/// `pml4_phys` must be the physical address of a valid PML4 table.
/// The caller must ensure no other CPU is using this address space
/// concurrently.
pub unsafe fn load_segments(
    elf: &ElfFile<'_>,
    pml4_phys: u64,
) -> KernelResult<()> {
    let hhdm = page_table::hhdm()
        .ok_or(KernelError::InternalError)?;

    let segments = elf.loadable_segments()?;

    for seg in segments {
        // SAFETY: Forwarding caller's safety requirements — pml4_phys
        // is valid, no concurrent access.
        unsafe {
            load_one_segment(elf, &seg, pml4_phys, hhdm)?;
        }
    }

    Ok(())
}

/// Load a single segment into the target address space.
///
/// # Safety
///
/// Same requirements as [`load_segments`].
unsafe fn load_one_segment(
    elf: &ElfFile<'_>,
    seg: &LoadableSegment,
    pml4_phys: u64,
    hhdm: u64,
) -> KernelResult<()> {
    // Calculate frame-aligned boundaries.
    // We need to map whole frames, but the segment may not start or
    // end on a frame boundary.
    let frame_size = FRAME_SIZE as u64;
    let seg_start = seg.vaddr;
    let seg_end = seg_start
        .checked_add(seg.mem_size)
        .ok_or(KernelError::InvalidAddress)?;

    let frame_start = seg_start & !(frame_size - 1);

    // Round up to next frame boundary.
    let frame_end = (seg_end
        .checked_add(frame_size - 1)
        .ok_or(KernelError::InvalidAddress)?)
        & !(frame_size - 1);

    let page_flags = segment_flags_to_page_flags(seg);

    // Allocate and map frames, then copy data.
    let mut current_vaddr = frame_start;
    while current_vaddr < frame_end {
        let virt = VirtAddr::new(current_vaddr);

        // Validate user-space address.
        if !virt.is_user() {
            return Err(KernelError::InvalidAddress);
        }

        // Allocate a physical frame.
        let phys_frame = frame::alloc_frame()?;

        // Zero the entire frame first (covers BSS and partial pages).
        let frame_virt = phys_frame.to_virt(hhdm);
        // SAFETY: frame_virt points to a freshly allocated, exclusively
        // owned frame mapped via the HHDM.
        unsafe {
            core::ptr::write_bytes(frame_virt as *mut u8, 0, FRAME_SIZE);
        }

        // Copy file data that falls within this frame.
        copy_segment_data_to_frame(
            elf,
            seg,
            current_vaddr,
            frame_virt,
        );

        // Map the frame into the target address space.
        // SAFETY: pml4_phys is valid (caller invariant), phys_frame is
        // freshly allocated and exclusively ours, virt is in user space.
        unsafe {
            page_table::map_frame(pml4_phys, virt, phys_frame, page_flags)?;
        }

        current_vaddr = current_vaddr
            .checked_add(frame_size)
            .ok_or(KernelError::InvalidAddress)?;
    }

    Ok(())
}

/// Copy the file-backed portion of a segment into a mapped frame.
///
/// The frame covers `[frame_vaddr .. frame_vaddr + FRAME_SIZE)` in
/// virtual address space.  The segment covers `[seg.vaddr ..
/// seg.vaddr + seg.file_size)` for file-backed data.  We compute the
/// overlap and copy only the relevant bytes.
fn copy_segment_data_to_frame(
    elf: &ElfFile<'_>,
    seg: &LoadableSegment,
    frame_vaddr: u64,
    frame_hhdm_virt: u64,
) {
    let frame_size = FRAME_SIZE as u64;
    let frame_end = frame_vaddr + frame_size;

    // The file-backed region of the segment.
    let file_start = seg.vaddr;
    let file_end = seg.vaddr + seg.file_size;

    // Overlap between this frame and the file-backed region.
    let overlap_start = file_start.max(frame_vaddr);
    let overlap_end = file_end.min(frame_end);

    if overlap_start >= overlap_end {
        return; // No file data in this frame (pure BSS or past file data).
    }

    let byte_count = (overlap_end - overlap_start) as usize;

    // Offset into the file.
    let file_offset = seg.file_offset + (overlap_start - seg.vaddr);

    // Offset into the frame.
    let frame_offset = (overlap_start - frame_vaddr) as usize;

    // Get source data from the ELF file.
    let src_start = file_offset as usize;
    let src_end = src_start + byte_count;

    // Bounds check on source data.
    if src_end > elf.data.len() || frame_offset + byte_count > FRAME_SIZE {
        return; // Silently skip — validation already caught bad segments.
    }

    // SAFETY: frame_hhdm_virt is a valid HHDM mapping of an exclusively
    // owned frame.  Source is a valid slice of the ELF data.
    unsafe {
        let dst = (frame_hhdm_virt as *mut u8).add(frame_offset);
        let src = elf.data.as_ptr().add(src_start);
        core::ptr::copy_nonoverlapping(src, dst, byte_count);
    }
}

// ---------------------------------------------------------------------------
// Helper: read little-endian integers from a byte slice
// ---------------------------------------------------------------------------

/// Read a little-endian `u16` from `data` at byte offset `off`.
///
/// # Panics
///
/// Panics if `off + 2 > data.len()` (caller must validate bounds).
#[inline]
fn read_u16(data: &[u8], off: usize) -> u16 {
    let bytes: [u8; 2] = [data[off], data[off + 1]];
    u16::from_le_bytes(bytes)
}

/// Read a little-endian `u32` from `data` at byte offset `off`.
#[inline]
fn read_u32(data: &[u8], off: usize) -> u32 {
    let bytes: [u8; 4] = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    u32::from_le_bytes(bytes)
}

/// Read a little-endian `u64` from `data` at byte offset `off`.
#[inline]
fn read_u64(data: &[u8], off: usize) -> u64 {
    let bytes: [u8; 8] = [
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ];
    u64::from_le_bytes(bytes)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Build a minimal valid ELF64 executable header for testing.
///
/// Creates a complete ELF64 header with one PT_LOAD program header
/// that maps a small code segment at a userspace address.  The "code"
/// is just NOP bytes — we're testing the parser, not execution.
fn build_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    // We'll build:
    // - 64-byte ELF header
    // - 56-byte program header (one PT_LOAD segment)
    // - 16 bytes of "code" (NOPs)
    //
    // Total: 136 bytes

    let phdr_offset: u64 = 64; // Right after the ELF header.
    let code_offset: u64 = 120; // After header + phdr.
    let code_size: u64 = 16;
    let load_vaddr: u64 = 0x0000_0040_0000_0000; // Userspace address.

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---

    // e_ident
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    // e_ident[7..16] = 0 (padding, already zeroed)

    // e_type
    write_u16(&mut buf, 16, ET_EXEC);
    // e_machine
    write_u16(&mut buf, 18, EM_X86_64);
    // e_version
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    // e_entry
    write_u64(&mut buf, 24, load_vaddr);
    // e_phoff
    write_u64(&mut buf, 32, phdr_offset);
    // e_shoff (0 = no section headers)
    write_u64(&mut buf, 40, 0);
    // e_flags
    write_u32(&mut buf, 48, 0);
    // e_ehsize
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    // e_phentsize
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    // e_phnum
    write_u16(&mut buf, 56, 1);
    // e_shentsize
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    // e_shnum
    write_u16(&mut buf, 60, 0);
    // e_shstrndx
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD) ---
    let ph = phdr_offset as usize;
    // p_type
    write_u32(&mut buf, ph, PT_LOAD);
    // p_flags
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    // p_offset
    write_u64(&mut buf, ph + 8, code_offset);
    // p_vaddr
    write_u64(&mut buf, ph + 16, load_vaddr);
    // p_paddr
    write_u64(&mut buf, ph + 24, 0);
    // p_filesz
    write_u64(&mut buf, ph + 32, code_size);
    // p_memsz (same as filesz — no BSS in this segment)
    write_u64(&mut buf, ph + 40, code_size);
    // p_align
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- "Code" segment ---
    //
    // Real x86_64 instructions that call SYS_EXIT(0) via SYSCALL.
    // This allows the test ELF to be loaded and executed in ring 3.
    //
    //   mov eax, 1          ; SYS_EXIT (B8 01 00 00 00)
    //   xor edi, edi        ; exit code = 0 (31 FF)
    //   syscall             ; enter kernel (0F 05)
    //   int3                ; safety net — unreachable (CC)
    //
    // Remaining bytes filled with INT3 for safety.
    let code_start = code_offset as usize;
    let code_end = (code_offset + code_size) as usize;
    for byte in &mut buf[code_start..code_end] {
        *byte = 0xCC; // INT3 — trap if executed unexpectedly.
    }
    // mov eax, 1 (SYS_EXIT)
    buf[code_start] = 0xB8;
    buf[code_start + 1] = 0x01;
    buf[code_start + 2] = 0x00;
    buf[code_start + 3] = 0x00;
    buf[code_start + 4] = 0x00;
    // xor edi, edi (exit code 0)
    buf[code_start + 5] = 0x31;
    buf[code_start + 6] = 0xFF;
    // syscall
    buf[code_start + 7] = 0x0F;
    buf[code_start + 8] = 0x05;

    buf
}

/// Public wrapper for test ELF generation.
///
/// Used by `spawn` module tests that need a valid ELF binary.
pub fn build_test_elf_public() -> alloc::vec::Vec<u8> {
    build_test_elf()
}

/// Build a test ELF that exercises stack growth.
///
/// The code decrements RSP by 128 KiB (well beyond the initial 64 KiB
/// stack allocation) and writes to the new location, triggering page
/// faults that the kernel should resolve via stack growth.  After
/// verifying the write, it calls SYS_EXIT(0).
///
/// Code:
/// ```x86asm
///   sub rsp, 0x20000    ; grow stack by 128 KiB (past initial 64 KiB)
///   mov qword [rsp], 42 ; touch the new stack page → triggers #PF
///   mov eax, 1          ; SYS_EXIT
///   xor edi, edi        ; exit code 0
///   syscall
///   int3                ; unreachable
/// ```
pub fn build_stack_growth_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 32; // Need more bytes for these instructions.
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // ELF header (same boilerplate).
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr);
    write_u64(&mut buf, 32, phdr_offset);
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // Program header (PT_LOAD, R+X).
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // Code: stack growth test.
    let c = code_offset as usize;
    let end = (code_offset + code_size) as usize;
    for byte in &mut buf[c..end] {
        *byte = 0xCC; // INT3 safety net.
    }

    // sub rsp, 0x20000  (48 81 EC 00 00 02 00) — grow by 128 KiB
    buf[c] = 0x48;
    buf[c + 1] = 0x81;
    buf[c + 2] = 0xEC;
    buf[c + 3] = 0x00;
    buf[c + 4] = 0x00;
    buf[c + 5] = 0x02;
    buf[c + 6] = 0x00;
    // mov qword [rsp], 42  (48 C7 04 24 2A 00 00 00) — touch the page
    buf[c + 7] = 0x48;
    buf[c + 8] = 0xC7;
    buf[c + 9] = 0x04;
    buf[c + 10] = 0x24;
    buf[c + 11] = 0x2A;
    buf[c + 12] = 0x00;
    buf[c + 13] = 0x00;
    buf[c + 14] = 0x00;
    // mov eax, 1 (SYS_EXIT)
    buf[c + 15] = 0xB8;
    buf[c + 16] = 0x01;
    buf[c + 17] = 0x00;
    buf[c + 18] = 0x00;
    buf[c + 19] = 0x00;
    // xor edi, edi (exit code 0)
    buf[c + 20] = 0x31;
    buf[c + 21] = 0xFF;
    // syscall
    buf[c + 22] = 0x0F;
    buf[c + 23] = 0x05;

    buf
}

/// Build a test ELF that triggers a page fault (null pointer write).
///
/// Used by spawn tests to verify that ring 3 faults kill the process
/// instead of crashing the kernel.
///
/// Code:
/// ```x86asm
///   xor eax, eax        ; rax = 0
///   mov [rax], eax       ; write to address 0 → #PF
///   int3                 ; unreachable safety net
/// ```
pub fn build_faulting_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 16;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // ELF header (same boilerplate as build_test_elf).
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr);
    write_u64(&mut buf, 32, phdr_offset);
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // Program header (PT_LOAD, R+X).
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // Code: null pointer write → page fault.
    let code_start = code_offset as usize;
    let code_end = (code_offset + code_size) as usize;
    for byte in &mut buf[code_start..code_end] {
        *byte = 0xCC; // INT3 safety net.
    }
    // xor eax, eax  (31 C0) → rax = 0
    buf[code_start] = 0x31;
    buf[code_start + 1] = 0xC0;
    // mov [rax], eax (89 00) → write to address 0 → #PF
    buf[code_start + 2] = 0x89;
    buf[code_start + 3] = 0x00;

    buf
}

/// Build a test ELF whose code calls `SYS_PROCESS_EXEC` (syscall 503).
///
/// The generated code:
/// ```x86asm
///   mov eax, 503           ; SYS_PROCESS_EXEC
///   movabs rdi, <elf_addr> ; pointer to ELF data in user memory
///   mov esi, <elf_len>     ; length of ELF data
///   syscall                ; exec the new binary
///   int3                   ; unreachable — exec doesn't return on success
/// ```
///
/// `elf_addr` and `elf_len` are patched into the code as immediate
/// operands.  The caller must ensure that the target ELF data is
/// mapped at `elf_addr` in the process's address space before the
/// code executes.
pub fn build_exec_test_elf(elf_addr: u64, elf_len: u32) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 32; // Enough for our instructions.
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // ELF header (same boilerplate).
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr);
    write_u64(&mut buf, 32, phdr_offset);
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // Program header (PT_LOAD, R+X).
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // Code: call SYS_PROCESS_EXEC(elf_addr, elf_len)
    let c = code_offset as usize;
    for byte in &mut buf[c..(c + code_size as usize)] {
        *byte = 0xCC; // INT3 safety net.
    }

    // mov eax, 503 (0x1F7)  →  B8 F7 01 00 00
    buf[c] = 0xB8;
    buf[c + 1] = 0xF7;
    buf[c + 2] = 0x01;
    buf[c + 3] = 0x00;
    buf[c + 4] = 0x00;

    // movabs rdi, <elf_addr>  →  48 BF <8 bytes LE>
    buf[c + 5] = 0x48;
    buf[c + 6] = 0xBF;
    let addr_bytes = elf_addr.to_le_bytes();
    buf[c + 7..c + 15].copy_from_slice(&addr_bytes);

    // mov esi, <elf_len>  →  BE <4 bytes LE>
    buf[c + 15] = 0xBE;
    let len_bytes = elf_len.to_le_bytes();
    buf[c + 16..c + 20].copy_from_slice(&len_bytes);

    // syscall  →  0F 05
    buf[c + 20] = 0x0F;
    buf[c + 21] = 0x05;

    // int3 at c+22 (already filled by safety net above)

    buf
}

/// Build a test ELF with BSS (memsz > filesz).
fn build_test_elf_with_bss() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 32; // File-backed bytes.
    let mem_size: u64 = 128; // Total in memory (96 bytes BSS).
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // ELF header (same as build_test_elf).
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr);
    write_u64(&mut buf, 32, phdr_offset);
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // PT_LOAD with BSS.
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_W); // Data segment (rw).
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size); // filesz
    write_u64(&mut buf, ph + 40, mem_size); // memsz > filesz
    write_u64(&mut buf, ph + 48, 0x1000);

    // Fill file-backed portion with recognizable pattern.
    for (i, byte) in buf[code_offset as usize..(code_offset + code_size) as usize]
        .iter_mut()
        .enumerate()
    {
        *byte = (i & 0xFF) as u8;
    }

    buf
}

/// Helper: write a little-endian u16 into a byte buffer.
fn write_u16(buf: &mut [u8], off: usize, val: u16) {
    let bytes = val.to_le_bytes();
    buf[off] = bytes[0];
    buf[off + 1] = bytes[1];
}

/// Helper: write a little-endian u32 into a byte buffer.
fn write_u32(buf: &mut [u8], off: usize, val: u32) {
    let bytes = val.to_le_bytes();
    buf[off] = bytes[0];
    buf[off + 1] = bytes[1];
    buf[off + 2] = bytes[2];
    buf[off + 3] = bytes[3];
}

/// Helper: write a little-endian u64 into a byte buffer.
fn write_u64(buf: &mut [u8], off: usize, val: u64) {
    let bytes = val.to_le_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        buf[off + i] = b;
    }
}

/// Run ELF loader self-tests.
pub fn self_test() -> KernelResult<()> {
    test_parse_valid_elf()?;
    test_parse_invalid_magic()?;
    test_parse_wrong_class()?;
    test_parse_wrong_machine()?;
    test_parse_too_small()?;
    test_loadable_segments()?;
    test_bss_segment()?;
    test_segment_flags()?;
    test_entry_point()?;

    Ok(())
}

/// Test 1: Parse a valid ELF64 executable.
fn test_parse_valid_elf() -> KernelResult<()> {
    let data = build_test_elf();
    let elf = ElfFile::parse(&data)?;

    if elf.header.e_type != ET_EXEC {
        serial_println!("[elf]   FAIL: e_type should be ET_EXEC");
        return Err(KernelError::InternalError);
    }

    if elf.header.e_machine != EM_X86_64 {
        serial_println!("[elf]   FAIL: e_machine should be EM_X86_64");
        return Err(KernelError::InternalError);
    }

    if elf.program_header_count() != 1 {
        serial_println!("[elf]   FAIL: expected 1 program header, got {}", elf.program_header_count());
        return Err(KernelError::InternalError);
    }

    serial_println!("[elf]   Parse valid ELF: OK");
    Ok(())
}

/// Test 2: Reject invalid magic bytes.
fn test_parse_invalid_magic() -> KernelResult<()> {
    let mut data = build_test_elf();
    data[0] = 0x00; // Corrupt magic.

    match ElfFile::parse(&data) {
        Err(KernelError::InvalidExecutable) => {}
        other => {
            serial_println!("[elf]   FAIL: invalid magic should fail: {:?}", other.map(|_| ()));
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[elf]   Reject invalid magic: OK");
    Ok(())
}

/// Test 3: Reject 32-bit ELF.
fn test_parse_wrong_class() -> KernelResult<()> {
    let mut data = build_test_elf();
    data[EI_CLASS] = 1; // ELFCLASS32

    match ElfFile::parse(&data) {
        Err(KernelError::InvalidExecutable) => {}
        other => {
            serial_println!("[elf]   FAIL: wrong class should fail: {:?}", other.map(|_| ()));
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[elf]   Reject 32-bit ELF: OK");
    Ok(())
}

/// Test 4: Reject non-x86_64 ELF.
fn test_parse_wrong_machine() -> KernelResult<()> {
    let mut data = build_test_elf();
    write_u16(&mut data, 18, 3); // EM_386

    match ElfFile::parse(&data) {
        Err(KernelError::InvalidExecutable) => {}
        other => {
            serial_println!("[elf]   FAIL: wrong machine should fail: {:?}", other.map(|_| ()));
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[elf]   Reject non-x86_64: OK");
    Ok(())
}

/// Test 5: Reject truncated data (too small for ELF header).
fn test_parse_too_small() -> KernelResult<()> {
    let data = [0x7F, b'E', b'L', b'F']; // Only magic, rest missing.

    match ElfFile::parse(&data) {
        Err(KernelError::InvalidExecutable) => {}
        other => {
            serial_println!("[elf]   FAIL: truncated data should fail: {:?}", other.map(|_| ()));
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[elf]   Reject truncated data: OK");
    Ok(())
}

/// Test 6: Extract loadable segments from a valid ELF.
fn test_loadable_segments() -> KernelResult<()> {
    let data = build_test_elf();
    let elf = ElfFile::parse(&data)?;

    let segments: alloc::vec::Vec<LoadableSegment> =
        elf.loadable_segments()?.collect();

    if segments.len() != 1 {
        serial_println!("[elf]   FAIL: expected 1 loadable segment, got {}", segments.len());
        return Err(KernelError::InternalError);
    }

    let seg = &segments[0];
    if seg.vaddr != 0x0000_0040_0000_0000 {
        serial_println!("[elf]   FAIL: wrong vaddr: {:#x}", seg.vaddr);
        return Err(KernelError::InternalError);
    }

    if seg.file_size != 16 {
        serial_println!("[elf]   FAIL: wrong file_size: {}", seg.file_size);
        return Err(KernelError::InternalError);
    }

    if seg.mem_size != 16 {
        serial_println!("[elf]   FAIL: wrong mem_size: {}", seg.mem_size);
        return Err(KernelError::InternalError);
    }

    serial_println!("[elf]   Loadable segments: OK");
    Ok(())
}

/// Test 7: BSS segment (memsz > filesz).
fn test_bss_segment() -> KernelResult<()> {
    let data = build_test_elf_with_bss();
    let elf = ElfFile::parse(&data)?;

    let segments: alloc::vec::Vec<LoadableSegment> =
        elf.loadable_segments()?.collect();

    if segments.len() != 1 {
        serial_println!("[elf]   FAIL: expected 1 loadable segment, got {}", segments.len());
        return Err(KernelError::InternalError);
    }

    let seg = &segments[0];
    if seg.file_size != 32 {
        serial_println!("[elf]   FAIL: wrong file_size: {}", seg.file_size);
        return Err(KernelError::InternalError);
    }

    if seg.mem_size != 128 {
        serial_println!("[elf]   FAIL: wrong mem_size: {}", seg.mem_size);
        return Err(KernelError::InternalError);
    }

    // mem_size > file_size → BSS present.
    if seg.mem_size <= seg.file_size {
        serial_println!("[elf]   FAIL: BSS segment should have mem_size > file_size");
        return Err(KernelError::InternalError);
    }

    serial_println!("[elf]   BSS segment: OK");
    Ok(())
}

/// Test 8: Segment permission flag conversion.
fn test_segment_flags() -> KernelResult<()> {
    // Read + execute segment.
    let rx_seg = LoadableSegment {
        vaddr: 0x1000,
        file_size: 16,
        mem_size: 16,
        file_offset: 0,
        readable: true,
        writable: false,
        executable: true,
    };
    let rx_flags = segment_flags_to_page_flags(&rx_seg);

    // Should have PRESENT + USER_ACCESSIBLE, not WRITABLE, not NO_EXECUTE.
    if !rx_flags.contains(PageFlags::PRESENT) {
        serial_println!("[elf]   FAIL: RX segment should be PRESENT");
        return Err(KernelError::InternalError);
    }
    if rx_flags.contains(PageFlags::WRITABLE) {
        serial_println!("[elf]   FAIL: RX segment should not be WRITABLE");
        return Err(KernelError::InternalError);
    }
    if rx_flags.contains(PageFlags::NO_EXECUTE) {
        serial_println!("[elf]   FAIL: RX segment should not be NO_EXECUTE");
        return Err(KernelError::InternalError);
    }

    // Read + write segment (data).
    let rw_seg = LoadableSegment {
        vaddr: 0x2000,
        file_size: 16,
        mem_size: 16,
        file_offset: 0,
        readable: true,
        writable: true,
        executable: false,
    };
    let rw_flags = segment_flags_to_page_flags(&rw_seg);

    if !rw_flags.contains(PageFlags::WRITABLE) {
        serial_println!("[elf]   FAIL: RW segment should be WRITABLE");
        return Err(KernelError::InternalError);
    }
    if !rw_flags.contains(PageFlags::NO_EXECUTE) {
        serial_println!("[elf]   FAIL: RW segment should be NO_EXECUTE");
        return Err(KernelError::InternalError);
    }

    serial_println!("[elf]   Segment flag conversion: OK");
    Ok(())
}

/// Test 9: Entry point extraction.
fn test_entry_point() -> KernelResult<()> {
    let data = build_test_elf();
    let elf = ElfFile::parse(&data)?;

    let expected = 0x0000_0040_0000_0000_u64;
    if elf.entry_point() != expected {
        serial_println!(
            "[elf]   FAIL: entry point {:#x}, expected {:#x}",
            elf.entry_point(),
            expected,
        );
        return Err(KernelError::InternalError);
    }

    if elf.is_pie() {
        serial_println!("[elf]   FAIL: ET_EXEC should not be PIE");
        return Err(KernelError::InternalError);
    }

    serial_println!("[elf]   Entry point: OK");
    Ok(())
}
