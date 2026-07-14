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
const EI_OSABI: usize = 7;
#[allow(dead_code)] const EI_ABIVERSION: usize = 8;

// EI_CLASS values.
const ELFCLASS64: u8 = 2;

// EI_DATA values (byte order).
const ELFDATA2LSB: u8 = 1; // Little-endian.

// EI_OSABI values relevant to Linux-binary detection.
//
// Most Linux toolchains emit `ELFOSABI_SYSV` (0) regardless of target —
// the OSABI byte is a weak signal.  But when it IS set to LINUX/GNU,
// it's an unambiguous indicator.
const ELFOSABI_SYSV: u8 = 0;
const ELFOSABI_GNU: u8 = 3;
// ELFOSABI_LINUX is an alias for ELFOSABI_GNU (same value, 3).  glibc
// historically used the name "GNU"; many references say "LINUX".

// e_type values.
const ET_EXEC: u16 = 2; // Executable file.
const ET_DYN: u16 = 3; // Shared object / PIE.

// e_machine values.
const EM_X86_64: u16 = 62;

// Program header p_type values.
#[allow(dead_code)] const PT_NULL: u32 = 0;
const PT_LOAD: u32 = 1;
#[allow(dead_code)] const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
#[allow(dead_code)] const PT_NOTE: u32 = 4;
#[allow(dead_code)] const PT_PHDR: u32 = 6;
#[allow(dead_code)] const PT_TLS: u32 = 7;
#[allow(dead_code)] const PT_GNU_EH_FRAME: u32 = 0x6474_E550;
#[allow(dead_code)] const PT_GNU_STACK: u32 = 0x6474_E551;
#[allow(dead_code)] const PT_GNU_RELRO: u32 = 0x6474_E552;
/// GNU property note — a strong Linux indicator emitted by binutils/gcc.
/// Defined in the Linux Foundation gABI proposal.  Not used by FreeBSD/
/// OpenBSD/NetBSD as of writing.
const PT_GNU_PROPERTY: u32 = 0x6474_E553;

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
    /// `e_ident[EI_OSABI]` — operating-system / ABI identifier.
    ///
    /// Most toolchains emit `ELFOSABI_SYSV` (0) regardless of target.
    /// A non-zero value such as `ELFOSABI_GNU` (3) is an unambiguous
    /// Linux-binary indicator.  See [`detect_linux_abi`].
    pub e_ident_osabi: u8,
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
    #[allow(dead_code)] // ELF spec field — not yet used by the loader.
    pub e_shoff: u64,
    /// Processor-specific flags (0 for x86_64).
    #[allow(dead_code)] // ELF spec field — always 0 for x86_64.
    pub e_flags: u32,
    /// Size of this header (should be 64 for ELF64).
    #[allow(dead_code)] // ELF spec field — validated implicitly.
    pub e_ehsize: u16,
    /// Size of each program header entry.
    pub e_phentsize: u16,
    /// Number of program header entries.
    pub e_phnum: u16,
    /// Size of each section header entry.
    #[allow(dead_code)] // ELF spec field — section headers not yet parsed.
    pub e_shentsize: u16,
    /// Number of section header entries.
    #[allow(dead_code)] // ELF spec field — section headers not yet parsed.
    pub e_shnum: u16,
    /// Section header string table index.
    #[allow(dead_code)] // ELF spec field — section headers not yet parsed.
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
    #[allow(dead_code)] // ELF spec field — unused on virtual memory systems.
    pub p_paddr: u64,
    /// Number of bytes of segment data in the file.
    pub p_filesz: u64,
    /// Number of bytes the segment occupies in memory (≥ `p_filesz`).
    /// The difference (`p_memsz - p_filesz`) is BSS (zero-filled).
    pub p_memsz: u64,
    /// Alignment requirement (0 or 1 = no alignment, else power of 2).
    #[allow(dead_code)] // ELF spec field — alignment enforced by frame size.
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
    #[allow(dead_code)] // Public API — all mapped pages are readable via PRESENT.
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
            e_ident_osabi: data[EI_OSABI],
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
        // Reject e_phentsize < ELF64_PHDR_SIZE when program headers exist.
        // A zero e_phentsize with e_phnum > 0 would cause all headers to
        // be read from the same offset, producing silently wrong results.
        if header.e_phnum > 0
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
            .checked_add(
                index.checked_mul(self.header.e_phentsize as usize)?
            )?;

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

    /// Read the bytes of a program header's file image as a slice.
    ///
    /// Returns the raw `[p_offset .. p_offset + p_filesz]` slice, or
    /// `None` if the range is out of bounds.  Useful for inspecting
    /// `PT_INTERP` / `PT_NOTE` segment contents during ABI detection.
    #[must_use]
    pub fn raw_segment_bytes(&self, phdr: &Elf64Phdr) -> Option<&'a [u8]> {
        let start = phdr.p_offset as usize;
        let end = start.checked_add(phdr.p_filesz as usize)?;
        self.data.get(start..end)
    }

    /// Detect whether this ELF binary speaks the Linux x86_64 syscall ABI.
    ///
    /// Returns `true` when the binary should run with
    /// [`crate::proc::pcb::AbiMode::Linux`] so its raw `syscall`
    /// instructions are routed through the Linux translation layer in
    /// `kernel::syscall::linux`.
    ///
    /// ## Signals (in order of reliability)
    ///
    /// 1. **`e_ident[EI_OSABI]`** set to `ELFOSABI_GNU` (3, alias for
    ///    `ELFOSABI_LINUX`).  Unambiguous when present.  glibc-linked
    ///    binaries that use `STT_GNU_IFUNC` or other GNU extensions
    ///    almost always set this; static-pie musl binaries may also set
    ///    it.  Most Linux toolchains, however, leave it as `ELFOSABI_SYSV`
    ///    (0), so absence is not a refutation.
    ///
    /// 2. **`PT_INTERP` pointing at a known Linux dynamic loader.**
    ///    Dynamic Linux binaries always have a `PT_INTERP` segment with
    ///    a NUL-terminated path string.  We match the substring
    ///    `ld-linux-x86-64` (glibc) or `ld-musl-x86_64` (musl) — both
    ///    are Linux-specific.  This catches the vast majority of
    ///    dynamically-linked Linux binaries regardless of `EI_OSABI`.
    ///
    /// 3. **`PT_GNU_PROPERTY` segment present.**  This segment carries
    ///    GNU-specific property notes (Intel CET endbr64 markers,
    ///    `GNU_PROPERTY_X86_FEATURE_1_AND`, etc.) emitted by binutils
    ///    and gcc since 2018.  As of this writing it is not used by
    ///    FreeBSD/OpenBSD/NetBSD toolchains, so its presence on an
    ///    x86_64 ELF is a strong Linux indicator.  This catches static
    ///    GNU/Linux binaries built with recent toolchains.
    ///
    /// ## Deliberate non-signals
    ///
    /// - `PT_GNU_STACK` / `PT_GNU_RELRO` alone are NOT used as signals
    ///   even though both originate in GNU/Linux; FreeBSD's clang now
    ///   emits them too and they would generate false positives on
    ///   FreeBSD binaries.
    /// - `NT_GNU_ABI_TAG` notes inside `PT_NOTE` segments would be a
    ///   reliable signal but require walking the note table; punt to
    ///   a follow-up if false-negative rates turn out to matter.
    /// - `e_machine` is already validated as `EM_X86_64` by
    ///   [`ElfFile::parse`] — that check happens unconditionally.
    ///
    /// ## False-positive / false-negative profile
    ///
    /// False positives (returning `true` for a non-Linux binary) are
    /// the dangerous direction: a Native binary mis-detected as Linux
    /// would have its `syscall`s routed through the wrong dispatch
    /// table, almost certainly resulting in `-ENOSYS` or wildly wrong
    /// semantics.  The signals above are chosen so that false positives
    /// require a binary that intentionally mimics Linux markers — not
    /// something the host toolchain produces by accident.
    ///
    /// False negatives (returning `false` for a real Linux binary)
    /// degrade to running the Linux binary under our native ABI, which
    /// will produce wrong syscall results — but this is no worse than
    /// having no Linux ABI support at all, and the binary can be
    /// flagged manually via [`crate::proc::pcb::set_abi_mode`] or a
    /// future explicit-runtime syscall.
    #[must_use]
    pub fn detect_linux_abi(&self) -> bool {
        // Signal 1: EI_OSABI explicit Linux/GNU tag.
        if self.header.e_ident_osabi == ELFOSABI_GNU {
            return true;
        }

        // Signal 2 + 3: walk program headers once, checking for
        // PT_INTERP with a Linux loader path and PT_GNU_PROPERTY.
        for i in 0..self.program_header_count() {
            let Some(phdr) = self.program_header(i) else { continue };

            match phdr.p_type {
                PT_INTERP => {
                    if let Some(bytes) = self.raw_segment_bytes(&phdr)
                        && is_linux_interp(bytes)
                    {
                        return true;
                    }
                }
                PT_GNU_PROPERTY => {
                    return true;
                }
                _ => {}
            }
        }

        false
    }

    /// Return the dynamic loader path from the `PT_INTERP` segment.
    ///
    /// A dynamically-linked ELF (`ET_DYN` executables and any binary that
    /// is not fully static) carries a `PT_INTERP` program header whose
    /// file image is a NUL-terminated path naming the program interpreter
    /// — e.g. `/lib64/ld-linux-x86-64.so.2` (glibc) or
    /// `/lib/ld-musl-x86_64.so.1` (musl).  The kernel must load *that*
    /// interpreter (not the executable's own `e_entry`) and transfer
    /// control to it; the interpreter then maps shared libraries and
    /// jumps to the real entry point (passed via `AT_ENTRY`).
    ///
    /// Returns the path with its trailing NUL (and any bytes after the
    /// first NUL) trimmed, or `None` when:
    /// - the binary is statically linked (no `PT_INTERP` segment), or
    /// - the segment's file image is out of bounds, or
    /// - the path is empty (a malformed `PT_INTERP`).
    ///
    /// The result is raw bytes, never `str`: an interpreter path is an
    /// OS path and may contain any byte except `/` (separator) and NUL
    /// (terminator), so it must not be forced through UTF-8 validation.
    #[must_use]
    pub fn interp_path(&self) -> Option<&'a [u8]> {
        for i in 0..self.program_header_count() {
            let Some(phdr) = self.program_header(i) else {
                continue;
            };
            if phdr.p_type != PT_INTERP {
                continue;
            }
            let bytes = self.raw_segment_bytes(&phdr)?;
            // The image is NUL-terminated in the file; trim at the first
            // NUL (Linux's `load_elf_interp` likewise treats the segment
            // as a C string).  An image with no NUL is tolerated by using
            // its full length, but a leading NUL (empty path) is rejected.
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            let path = bytes.get(..end)?;
            if path.is_empty() {
                return None;
            }
            return Some(path);
        }
        None
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

            // Validate that vaddr + memsz doesn't overflow and fits
            // in user address space.  This applies to both ET_EXEC and
            // ET_DYN — a PIE binary may have vaddr=0, but the segment
            // still must not wrap around or exceed the user space limit.
            let seg_end = phdr
                .p_vaddr
                .checked_add(phdr.p_memsz)
                .ok_or(KernelError::InvalidExecutable)?;

            if seg_end > USER_SPACE_END {
                return Err(KernelError::InvalidExecutable);
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
    #[allow(dead_code)] // Public API — useful for callers inspecting raw segment data.
    #[must_use]
    pub fn segment_data(&self, phdr: &Elf64Phdr) -> Option<&'a [u8]> {
        let start = phdr.p_offset as usize;
        let end = start.checked_add(phdr.p_filesz as usize)?;
        self.data.get(start..end)
    }

    /// Get the total size of the raw ELF data.
    #[allow(dead_code)] // Public API — useful for diagnostics and validation.
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

impl Iterator for LoadableSegments<'_> {
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
        flags |= PageFlags::WRITABLE;
    }

    if !seg.executable {
        flags |= PageFlags::NO_EXECUTE;
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
    // SAFETY: forwarding caller's safety requirements to the bias-aware
    // loader; bias 0 maps every segment at its own `p_vaddr`, exactly the
    // historical behaviour of this function.
    unsafe { load_segments_with_bias(elf, pml4_phys, 0) }
}

/// Load all `PT_LOAD` segments at a runtime load bias.
///
/// Identical to [`load_segments`] except that every segment is mapped at
/// `bias + p_vaddr` instead of `p_vaddr`.  This is how a position-
/// independent program interpreter (`ld.so`, always `ET_DYN` with
/// `p_vaddr` values relative to 0) is placed at a chosen base address:
/// the kernel picks `bias`, maps the loader there, and enters it at
/// `bias + e_entry`.  For the main executable `bias` is 0 (`ET_EXEC`
/// images have absolute `p_vaddr`s; `ET_DYN`/PIE executables are loaded
/// at a fixed bias chosen by the caller — currently 0).
///
/// The biased range `[bias + p_vaddr, bias + p_vaddr + p_memsz)` is
/// re-validated against [`USER_SPACE_END`]; an overflow or out-of-range
/// segment yields [`KernelError::InvalidAddress`].
///
/// # Errors
///
/// Same as [`load_segments`], plus [`KernelError::InvalidAddress`] when
/// applying `bias` overflows or pushes a segment past the user-space
/// limit.
///
/// # Safety
///
/// Same requirements as [`load_segments`]: `pml4_phys` must be a valid
/// PML4 and no other CPU may use the address space concurrently.
pub unsafe fn load_segments_with_bias(
    elf: &ElfFile<'_>,
    pml4_phys: u64,
    bias: u64,
) -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    let frame_size = FRAME_SIZE as u64;
    let hw_size = page_table::HW_PAGE_SIZE as u64;
    // frame_size is a power of two, so frame_size - 1 is the alignment mask.
    let frame_mask = frame_size.wrapping_sub(1);

    // --- Pass 1: page span ----------------------------------------------------
    // Compute the 16 KiB-frame-aligned address range that covers every
    // `PT_LOAD` segment after applying `bias`.  Standard x86-64 Linux binaries
    // align segments only to 4 KiB, so two segments routinely share a 16 KiB
    // frame; loading each segment independently (the old approach) double-mapped
    // those shared frames and failed with `AlreadyExists`.  Instead we walk the
    // whole span once, frame by frame.
    let mut min_page: u64 = u64::MAX;
    let mut max_end: u64 = 0;
    for seg in elf.loadable_segments()? {
        let start = seg.vaddr.checked_add(bias).ok_or(KernelError::InvalidAddress)?;
        let end = start.checked_add(seg.mem_size).ok_or(KernelError::InvalidAddress)?;
        if end > USER_SPACE_END {
            return Err(KernelError::InvalidAddress);
        }
        let page_start = start & !frame_mask;
        let page_end = end
            .checked_add(frame_mask)
            .ok_or(KernelError::InvalidAddress)?
            & !frame_mask;
        if page_start < min_page {
            min_page = page_start;
        }
        if page_end > max_end {
            max_end = page_end;
        }
    }
    if min_page == u64::MAX {
        // No loadable segments (a degenerate ELF) — nothing to map.
        return Ok(());
    }

    // --- Pass 2: map the span frame by frame ---------------------------------
    // For each 16 KiB frame in [min_page, max_end): determine which segments
    // touch it, derive per-4 KiB-subpage permissions from segment coverage,
    // allocate+zero one frame, copy each overlapping segment's file bytes in,
    // and map with `map_frame_subpages`.  A frame that no segment touches (an
    // inter-segment hole ≥ 16 KiB) is left entirely unmapped.
    let mut page = min_page;
    while page < max_end {
        let page_end_addr = page
            .checked_add(frame_size)
            .ok_or(KernelError::InvalidAddress)?;

        // Derive per-subpage permission flags.  Each 4 KiB subpage gets the
        // union of the page flags of every segment whose memory range
        // intersects it.  Because Linux segments are 4 KiB-aligned and never
        // overlap at 4 KiB granularity, each subpage is covered by at most one
        // segment, so this yields each segment's exact R/W/X — preserving W^X.
        let mut subpage_flags = [PageFlags::empty(); page_table::HW_PAGES_PER_FRAME];
        let mut page_used = false;
        for seg in elf.loadable_segments()? {
            let s = seg.vaddr.checked_add(bias).ok_or(KernelError::InvalidAddress)?;
            let e = s.checked_add(seg.mem_size).ok_or(KernelError::InvalidAddress)?;
            // Skip a segment that does not intersect this frame at all.
            if e <= page || s >= page_end_addr {
                continue;
            }
            let seg_flags = segment_flags_to_page_flags(&seg);
            for (i, sf) in subpage_flags.iter_mut().enumerate() {
                let sub_start = page
                    .checked_add((i as u64).wrapping_mul(hw_size))
                    .ok_or(KernelError::InvalidAddress)?;
                let sub_end = sub_start
                    .checked_add(hw_size)
                    .ok_or(KernelError::InvalidAddress)?;
                if s < sub_end && e > sub_start {
                    *sf |= seg_flags;
                    page_used = true;
                }
            }
        }

        if !page_used {
            page = page_end_addr;
            continue;
        }

        // Allocate and zero one frame for this page (covers BSS + any
        // file/page tail past EOF, matching Linux's zero-fill).
        let phys_frame = frame::alloc_frame()?;
        let frame_virt = phys_frame.to_virt(hhdm);
        // SAFETY: freshly allocated, exclusively owned frame mapped via HHDM.
        unsafe {
            core::ptr::write_bytes(frame_virt as *mut u8, 0, FRAME_SIZE);
        }

        // Copy the file-backed bytes of every overlapping segment into the
        // frame.  `copy_segment_data_to_frame` clips to the overlap of the
        // segment's file region with this frame, so a large segment is filled
        // in across successive frames and a small one only touches its bytes.
        for seg in elf.loadable_segments()? {
            let biased_vaddr =
                seg.vaddr.checked_add(bias).ok_or(KernelError::InvalidAddress)?;
            let biased = LoadableSegment {
                vaddr: biased_vaddr,
                ..seg
            };
            copy_segment_data_to_frame(elf, &biased, page, frame_virt);
        }

        // Map the frame with per-subpage permissions.  On failure free the
        // just-allocated frame (it was never mapped, so address-space teardown
        // would not find it).
        let virt = VirtAddr::new(page);
        // SAFETY: pml4_phys is valid (caller invariant), phys_frame is freshly
        // allocated and exclusively ours, virt is a frame-aligned user address.
        if let Err(e) = unsafe {
            page_table::map_frame_subpages(pml4_phys, virt, phys_frame, subpage_flags)
        } {
            // SAFETY: phys_frame was just allocated and never shared.
            let _ = unsafe { frame::free_frame(phys_frame) };
            return Err(e);
        }

        page = page_end_addr;
    }

    Ok(())
}

/// Compute the highest 16 KiB-frame-aligned virtual address occupied by any
/// `PT_LOAD` segment of `elf` when loaded at runtime bias `bias`.
///
/// This is where the Linux `brk`/`sbrk` heap begins: Linux places the
/// program break immediately after the executable's last loadable segment
/// (its data/BSS), rounded up to a page boundary (`mm/mmap.c` /
/// `fs/binfmt_elf.c` `set_brk`).  The returned address is frame-aligned and
/// suitable as the initial `brk_start`.
///
/// Returns `Ok(0)` if the image has no loadable segments (a degenerate ELF;
/// the caller treats a zero result as "no heap").
///
/// # Errors
///
/// [`KernelError::InvalidAddress`] if applying `bias` or the frame round-up
/// overflows `u64`.
pub fn image_end(elf: &ElfFile<'_>, bias: u64) -> KernelResult<u64> {
    let frame_size = FRAME_SIZE as u64;
    // frame_size is a power of two, so frame_size - 1 is the alignment mask.
    let mask = frame_size.wrapping_sub(1);
    let mut highest: u64 = 0;
    for seg in elf.loadable_segments()? {
        let biased = seg
            .vaddr
            .checked_add(bias)
            .ok_or(KernelError::InvalidAddress)?;
        let seg_end = biased
            .checked_add(seg.mem_size)
            .ok_or(KernelError::InvalidAddress)?;
        // Round the segment end up to the next frame boundary.
        let aligned_end = seg_end
            .checked_add(mask)
            .ok_or(KernelError::InvalidAddress)?
            & !mask;
        if aligned_end > highest {
            highest = aligned_end;
        }
    }
    Ok(highest)
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
    let frame_end = frame_vaddr.saturating_add(frame_size);

    // The file-backed region of the segment.
    let file_start = seg.vaddr;
    let file_end = seg.vaddr.saturating_add(seg.file_size);

    // Overlap between this frame and the file-backed region.
    let overlap_start = file_start.max(frame_vaddr);
    let overlap_end = file_end.min(frame_end);

    if overlap_start >= overlap_end {
        return; // No file data in this frame (pure BSS or past file data).
    }

    let byte_count = (overlap_end - overlap_start) as usize;

    // Offset into the file.
    let file_offset = match seg.file_offset.checked_add(overlap_start.saturating_sub(seg.vaddr)) {
        Some(v) => v,
        None => return, // Overflow — skip (validation already caught bad segments).
    };

    // Offset into the frame.
    let frame_offset = (overlap_start - frame_vaddr) as usize;

    // Get source data from the ELF file.
    let src_start = file_offset as usize;
    let src_end = src_start.saturating_add(byte_count);

    // Bounds check on source data.
    if src_end > elf.data.len() || frame_offset.saturating_add(byte_count) > FRAME_SIZE {
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

/// Return `true` if `bytes` (a NUL-terminated `PT_INTERP` path image)
/// names a known Linux dynamic loader.
///
/// The check is substring-based on the path before the NUL terminator,
/// matching both `/lib64/ld-linux-x86-64.so.2` (glibc, the
/// near-universal Linux dynamic linker) and
/// `/lib/ld-musl-x86_64.so.1` (musl).  Both substrings are
/// Linux-specific — no other extant x86_64 OS ships a loader named
/// `ld-linux-x86-64` or `ld-musl-x86_64`.
#[inline]
fn is_linux_interp(bytes: &[u8]) -> bool {
    // Trim trailing NULs / NUL-terminate.  A well-formed PT_INTERP
    // image is a C string with `p_filesz` bytes; the terminator may
    // be at the end of the slice or somewhere in the middle.
    let path = match bytes.iter().position(|&b| b == 0) {
        Some(nul_pos) => bytes.get(..nul_pos).unwrap_or(&[]),
        None => bytes,
    };

    contains_subslice(path, b"ld-linux-x86-64")
        || contains_subslice(path, b"ld-musl-x86_64")
}

/// Returns `true` if `haystack` contains `needle` as a contiguous
/// subsequence.  Naive O(n*m) scan — adequate for the very short
/// strings used here.
#[inline]
fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return needle.is_empty();
    }
    let last = haystack.len() - needle.len();
    let mut i = 0;
    while i <= last {
        if let Some(window) = haystack.get(i..i + needle.len())
            && window == needle
        {
            return true;
        }
        i += 1;
    }
    false
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

/// Build a **Linux-ABI** test ELF that exits with `argc` as its status.
///
/// This validates the System V initial-stack wiring end-to-end: the
/// binary is tagged `ELFOSABI_GNU`, so `detect_linux_abi` reports true
/// and `spawn_process` builds a System V stack (argc/argv/envp/auxv).
/// The code reads `argc` from `[%rsp]` — exactly where the SysV ABI says
/// the kernel must place it — and passes it to `exit(2)`:
///
/// ```text
///   mov rdi, [rsp]      ; rdi = argc                (48 8B 3C 24)
///   mov eax, 60         ; Linux SYS_exit            (B8 3C 00 00 00)
///   syscall             ; exit(argc)                (0F 05)
///   int3                ; unreachable trap          (CC ...)
/// ```
///
/// If the kernel laid out the stack correctly, the resulting zombie's
/// exit code equals the number of argv entries passed to the spawn.
pub fn build_linux_argc_exit_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 16;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    // Tag as Linux/GNU so detect_linux_abi() returns true (signal 1).
    buf[EI_OSABI] = ELFOSABI_GNU;

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code: exit(argc) reading argc from [rsp]. ---
    let code_start = code_offset as usize;
    let code_end = (code_offset + code_size) as usize;
    for byte in &mut buf[code_start..code_end] {
        *byte = 0xCC; // INT3 trap padding.
    }
    // mov rdi, [rsp]  (48 8B 3C 24)
    buf[code_start] = 0x48;
    buf[code_start + 1] = 0x8B;
    buf[code_start + 2] = 0x3C;
    buf[code_start + 3] = 0x24;
    // mov eax, 60  (B8 3C 00 00 00) — Linux SYS_exit
    buf[code_start + 4] = 0xB8;
    buf[code_start + 5] = 0x3C;
    buf[code_start + 6] = 0x00;
    buf[code_start + 7] = 0x00;
    buf[code_start + 8] = 0x00;
    // syscall  (0F 05)
    buf[code_start + 9] = 0x0F;
    buf[code_start + 10] = 0x05;

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that dereferences `argv[0]` and
/// exits with its first byte:
///
/// ```text
///   mov   rsi, [rsp+8]      ; rsi = argv[0]            (48 8B 74 24 08)
///   movzx edi, byte [rsi]   ; edi = argv[0][0]         (0F B6 3E)
///   mov   eax, 60           ; Linux SYS_exit           (B8 3C 00 00 00)
///   syscall                 ; exit(argv[0][0])         (0F 05)
///   int3                    ; unreachable trap         (CC)
/// ```
///
/// Where [`build_linux_argc_exit_test_elf`] reads only the *scalar* `argc`
/// from `[rsp]`, this image **dereferences a pointer the SysV stack builder
/// placed** — `argv[0]` — and reads a byte *through* it.  That covers a
/// distinct failure mode: a stack builder could compute `argc` correctly yet
/// place the wrong absolute argv-string addresses (off-by-one / wrong
/// stack-relative base finalised at spawn time), which the argc-only test
/// cannot catch but which crashes every real program.  Spawn it with an
/// `argv[0]` whose first byte is a known sentinel and assert the zombie's
/// exit code equals that byte.
///
/// Tagged `ELFOSABI_GNU` so `spawn_process` builds a System V stack for it.
#[must_use]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn build_linux_argv0_deref_exit_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 16;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU; // tag Linux/GNU so detect_linux_abi() is true

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code: exit(argv[0][0]) ---
    let cs = code_offset as usize;
    for byte in &mut buf[cs..(cs + code_size as usize)] {
        *byte = 0xCC; // INT3 trap padding.
    }
    // mov rsi, [rsp+8]  (48 8B 74 24 08)
    buf[cs] = 0x48;
    buf[cs + 1] = 0x8B;
    buf[cs + 2] = 0x74;
    buf[cs + 3] = 0x24;
    buf[cs + 4] = 0x08;
    // movzx edi, byte [rsi]  (0F B6 3E)
    buf[cs + 5] = 0x0F;
    buf[cs + 6] = 0xB6;
    buf[cs + 7] = 0x3E;
    // mov eax, 60  (B8 3C 00 00 00) — Linux SYS_exit
    buf[cs + 8] = 0xB8;
    buf[cs + 9] = 0x3C;
    buf[cs + 10] = 0x00;
    buf[cs + 11] = 0x00;
    buf[cs + 12] = 0x00;
    // syscall  (0F 05)
    buf[cs + 13] = 0x0F;
    buf[cs + 14] = 0x05;

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that dereferences `envp[0]` and
/// exits with its first byte:
///
/// ```text
///   mov   rdi, [rsp]            ; rdi = argc               (48 8B 3C 24)
///   mov   rsi, [rsp+rdi*8+16]   ; rsi = envp[0]            (48 8B 74 FC 10)
///   movzx edi, byte [rsi]       ; edi = envp[0][0]         (0F B6 3E)
///   mov   eax, 60               ; Linux SYS_exit           (B8 3C 00 00 00)
///   syscall                     ; exit(envp[0][0])         (0F 05)
///   int3                        ; unreachable trap         (CC)
/// ```
///
/// This is the sibling of [`build_linux_argv0_deref_exit_elf`], but it covers
/// a **distinct addressing path**: `argv[0]` sits at the *fixed* offset
/// `[rsp+8]`, whereas `envp[0]` lives at the *variable* offset
/// `[rsp + 16 + argc*8]` (just past the `argc` argv pointers and their NULL
/// terminator).  A stack builder could place argv correctly yet put the envp
/// array at the wrong slot — invisible to the argv test but fatal to
/// `getenv()` (toolchains depend on `PATH`/`TMPDIR`/`CC`).  The program
/// computes the envp address from the runtime `argc`, so it validates the
/// real arithmetic the C runtime performs.  Spawn it with an `envp[0]` whose
/// first byte is a known sentinel and assert the zombie's exit code equals it.
///
/// Tagged `ELFOSABI_GNU` so `spawn_process` builds a System V stack for it.
#[must_use]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn build_linux_envp0_deref_exit_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 24; // 19 bytes of code + INT3 padding
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU; // tag Linux/GNU so detect_linux_abi() is true

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code: exit(envp[0][0]) ---
    let cs = code_offset as usize;
    for byte in &mut buf[cs..(cs + code_size as usize)] {
        *byte = 0xCC; // INT3 trap padding.
    }
    // mov rdi, [rsp]  (48 8B 3C 24) — rdi = argc
    buf[cs] = 0x48;
    buf[cs + 1] = 0x8B;
    buf[cs + 2] = 0x3C;
    buf[cs + 3] = 0x24;
    // mov rsi, [rsp + rdi*8 + 16]  (48 8B 74 FC 10) — rsi = envp[0]
    // envp = argv_base(rsp+8) + (argc+1)*8 = rsp + 16 + argc*8.
    buf[cs + 4] = 0x48;
    buf[cs + 5] = 0x8B;
    buf[cs + 6] = 0x74;
    buf[cs + 7] = 0xFC;
    buf[cs + 8] = 0x10;
    // movzx edi, byte [rsi]  (0F B6 3E) — edi = envp[0][0]
    buf[cs + 9] = 0x0F;
    buf[cs + 10] = 0xB6;
    buf[cs + 11] = 0x3E;
    // mov eax, 60  (B8 3C 00 00 00) — Linux SYS_exit
    buf[cs + 12] = 0xB8;
    buf[cs + 13] = 0x3C;
    buf[cs + 14] = 0x00;
    buf[cs + 15] = 0x00;
    buf[cs + 16] = 0x00;
    // syscall  (0F 05)
    buf[cs + 17] = 0x0F;
    buf[cs + 18] = 0x05;

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the full
/// `fork(2)` → child `exit(2)` → parent `wait4(2)` reap cycle entirely in
/// ring 3, then exits with the child's `WEXITSTATUS`.
///
/// This is the single most important process-lifecycle primitive for a real
/// toolchain: `make` spawns `gcc`, which spawns `cc1`/`as`/`ld`, each via
/// `fork`+`execve`+`wait4`.  The parent issues a **blocking** `wait4(-1,
/// &status, 0, NULL)` — exactly what `make`/`gcc` do — which exercises the
/// real block-and-wake path: the parent registers as a wait-any waiter and
/// sleeps in `block_current`, leaving the run queue; the child then runs and
/// its exit (`on_thread_exit`) wakes the parent, which re-scans, reaps, and
/// exits with the child's `WEXITSTATUS`.
///
/// **Why this cannot hang the boot:** the launcher itself blocks, but the
/// *harness* that drives it ([`crate::proc::spawn::self_test_linux_fork_wait`])
/// pumps the scheduler with a **bounded** `yield_now` loop and force-destroys
/// the launcher if it never becomes a zombie.  So even if the child-exit
/// wakeup were broken, the worst case is a clean failed assertion, never a
/// boot hang.  (An earlier non-blocking `WNOHANG`+`sched_yield` spin version
/// timed out because the child was starved while the parent stayed runnable;
/// blocking is both simpler and matches real toolchain usage.)
///
/// Pseudo-assembly (offsets are bytes from the segment start):
///
/// ```text
///  0  sub   rsp, 16             ; reserve a 4-byte status slot on the stack
///  4  mov   eax, 57             ; SYS_fork
///  9  syscall                   ; rax = child pid (parent) | 0 (child)
/// 11  test  rax, rax
/// 14  jz    child               ; rax==0 -> child path
/// 16  mov   edi, -1             ; parent: pid = -1 (wait for any child)
/// 21  mov   rsi, rsp            ; &status
/// 24  xor   edx, edx            ; options = 0 (blocking wait)
/// 26  xor   r10d, r10d          ; rusage = NULL
/// 29  mov   eax, 61             ; SYS_wait4
/// 34  syscall                   ; rax = reaped pid (>0) | <0 on error
/// 36  test  rax, rax
/// 39  jle   parent_fail         ; rax<=0 -> unexpected (no child reaped)
/// 41  movzx edi, byte [rsp+1]   ; WEXITSTATUS = byte at &status+1
/// 46  mov   eax, 60             ; SYS_exit(WEXITSTATUS)
/// 51  syscall
/// 53 parent_fail:
/// 53  mov   edi, 0xA1           ; wait4-error sentinel (161)
/// 58  mov   eax, 60             ; SYS_exit
/// 63  syscall
/// 65 child:
/// 65  mov   edi, 0x4B           ; child exit code (sentinel 75)
/// 70  mov   eax, 60             ; SYS_exit
/// 75  syscall
/// 77  int3                      ; unreachable trap
/// ```
///
/// On a healthy system the child exits `0x4B`, the kernel encodes the normal
/// exit as `wstatus = (0x4B << 8)`, so `WEXITSTATUS` (the byte at
/// `&status + 1`) is `0x4B`, and the parent exits `0x4B` (75).  The paired
/// self-test [`crate::proc::spawn::self_test_linux_fork_wait`] asserts the
/// parent zombie's exit code is exactly 75.
///
/// Tagged `ELFOSABI_GNU` so `spawn_process` builds a System V stack and routes
/// the process through the Linux ABI.
#[must_use]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn build_linux_fork_wait_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 88; // 78 bytes of code + INT3 padding
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU; // tag Linux/GNU so detect_linux_abi() is true

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    // INT3-fill the whole segment first; the explicit bytes below overwrite
    // the live instructions and leave the tail as trap padding.
    for byte in &mut buf[cs..(cs + code_size as usize)] {
        *byte = 0xCC;
    }
    // Hand-assembled (encodings verified against the Intel SDM):
    //   jz  rel8 = child(65) - 16 = 0x31
    //   jle rel8 = parent_fail(53) - 41 = 0x0C
    let code: [u8; 78] = [
        0x48, 0x83, 0xEC, 0x10, // sub rsp, 16
        0xB8, 0x39, 0x00, 0x00, 0x00, // mov eax, 57 (SYS_fork)
        0x0F, 0x05, // syscall
        0x48, 0x85, 0xC0, // test rax, rax
        0x74, 0x31, // jz child
        // parent: blocking wait4(-1, &status, 0, NULL)
        0xBF, 0xFF, 0xFF, 0xFF, 0xFF, // mov edi, -1
        0x48, 0x89, 0xE6, // mov rsi, rsp
        0x31, 0xD2, // xor edx, edx (options = 0, blocking)
        0x45, 0x31, 0xD2, // xor r10d, r10d (rusage = NULL)
        0xB8, 0x3D, 0x00, 0x00, 0x00, // mov eax, 61 (SYS_wait4)
        0x0F, 0x05, // syscall
        0x48, 0x85, 0xC0, // test rax, rax
        0x7E, 0x0C, // jle parent_fail
        0x0F, 0xB6, 0x7C, 0x24, 0x01, // movzx edi, byte [rsp+1] (WEXITSTATUS)
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (SYS_exit)
        0x0F, 0x05, // syscall
        // parent_fail:
        0xBF, 0xA1, 0x00, 0x00, 0x00, // mov edi, 0xA1 (wait4-error sentinel 161)
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (SYS_exit)
        0x0F, 0x05, // syscall
        // child:
        0xBF, 0x4B, 0x00, 0x00, 0x00, // mov edi, 0x4B (child sentinel 75)
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (SYS_exit)
        0x0F, 0x05, // syscall
        0xCC, // int3
    ];
    buf[cs..(cs + code.len())].copy_from_slice(&code);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the full
/// `fork(2)` → child `execve(2)` → parent `wait4(2)` reap cycle in ring 3
/// and exits with the **exec target's** `WEXITSTATUS`.
///
/// This is the exact subprocess pattern a real toolchain runs: `make`
/// `fork`s, the child `execve`s `gcc` (replacing its image), and the parent
/// blocks in `wait4` until the tool exits, then reads its status.  The
/// simpler [`build_linux_fork_wait_test_elf`] has the child `exit` directly;
/// here the child instead `execve`s `path_nul`, so a correct parent
/// `WEXITSTATUS` proves the *whole* fork→exec→wait chain end to end:
///
///   * the forked child resumes and reads its `execve` arguments (path,
///     argv, envp) out of its **copy-on-write** post-fork memory (read path);
///   * `execve` tears down the CoW clone and loads a fresh image in place
///     (same PID), which then `exit`s the target sentinel;
///   * the parent, blocked in `wait4`, is woken by the child's exit and
///     writes the status word back through a pointer on its **own** CoW
///     stack (the write path that the `validate_user_write` CoW-break fix
///     unblocked).
///
/// Layout (offsets = bytes from segment start):
/// ```text
///   sub rsp,16 ; fork ; test rax,rax ; jz child
///   parent: wait4(-1,&status,0,NULL) ; jle parent_fail
///           movzx edi,[rsp+1] ; exit(WEXITSTATUS)
///   parent_fail: exit(0xA2)             ; wait4 returned <= 0
///   child:  execve(path, argv=[path,NULL], envp=[NULL])
///           exit(0xE7)                  ; only if execve returned (failed)
/// ```
///
/// The exec target is staged by the harness as
/// [`build_linux_exit_elf`]`(sentinel)`, so the reaped `WEXITSTATUS` equals
/// that sentinel.  Tagged `ELFOSABI_GNU` for the SysV stack + Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_fork_execve_wait_test_elf(path_nul: &[u8]) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // --- Assemble the code linearly, recording label positions and the
    //     rel8/imm64 patch slots; resolve them once all offsets are known. ---
    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // sub rsp, 16  (reserve a 4-byte status slot)
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x10]);
    // mov eax, 57 (SYS_fork); syscall
    code.extend_from_slice(&[0xB8, 0x39, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    // test rax, rax
    code.extend_from_slice(&[0x48, 0x85, 0xC0]);
    // jz child (rel8 patched below)
    code.extend_from_slice(&[0x74, 0x00]);
    let jz_rel = code.len() - 1;

    // parent: blocking wait4(-1, &status, 0, NULL)
    code.extend_from_slice(&[0xBF, 0xFF, 0xFF, 0xFF, 0xFF]); // mov edi, -1
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp (&status)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (options = 0)
    code.extend_from_slice(&[0x45, 0x31, 0xD2]); // xor r10d, r10d (rusage = NULL)
    code.extend_from_slice(&[0xB8, 0x3D, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,61; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    // jle parent_fail (rel8 patched below)
    code.extend_from_slice(&[0x7E, 0x00]);
    let jle_rel = code.len() - 1;
    code.extend_from_slice(&[0x0F, 0xB6, 0x7C, 0x24, 0x01]); // movzx edi, byte [rsp+1]
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // parent_fail: exit(0xA2) — wait4 returned <= 0 (no child reaped / error)
    let parent_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xA2, 0x00, 0x00, 0x00]); // mov edi, 0xA2
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // child: execve(path, argv, envp)
    let child = code.len();
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, &argv
    let argv_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0xBA]); // movabs rdx, &envp
    let envp_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xB8, 0x3B, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,59 (SYS_execve); syscall
    // execve_fail: exit(0xE7) — only reached if execve returned (failed)
    code.extend_from_slice(&[0xBF, 0xE7, 0x00, 0x00, 0x00]); // mov edi, 0xE7
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall
    code.push(0xCC); // int3 — unreachable trap

    // Patch the two forward rel8 jumps.  rel8 is measured from the byte
    // following the displacement (instruction end = rel_off + 1).
    let jz_disp = (child as isize) - (jz_rel as isize + 1);
    let jle_disp = (parent_fail as isize) - (jle_rel as isize + 1);
    code[jz_rel] = jz_disp as u8;
    code[jle_rel] = jle_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    // 8-align argv; argv = [path, NULL], envp = [NULL].
    let argv_off = (path_end + 7) & !7usize;
    let envp_off = argv_off + 2 * 8;
    let file_size = envp_off + 8;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    let argv_vaddr = vaddr_of(argv_off);
    let envp_vaddr = vaddr_of(envp_off);
    code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes());
    code[argv_imm..argv_imm + 8].copy_from_slice(&argv_vaddr.to_le_bytes());
    code[envp_imm..envp_imm + 8].copy_from_slice(&envp_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // PT_LOAD R+W+X: W keeps argv/envp + the parent's status slot on a
    // writable page; X for the code.
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_W | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path_nul);
    write_u64(&mut buf, argv_off, path_vaddr); // argv[0] = path
    write_u64(&mut buf, argv_off + 8, 0); // argv[1] = NULL
    write_u64(&mut buf, envp_off, 0); // envp[0] = NULL

    buf
}

/// Build a minimal **Linux-ABI** `ET_EXEC` test ELF that writes a single
/// `byte` to **stdout (fd 1)** and then `exit`s with that same value:
///
/// ```text
///   sub  rsp, 16
///   mov  byte [rsp], byte   ; C6 04 24 ib  — stash the byte on the stack
///   mov  edi, 1             ; fd = 1 (stdout)
///   mov  rsi, rsp           ; buf = &byte
///   mov  edx, 1             ; count = 1
///   mov  eax, 1             ; SYS_write
///   syscall
///   mov  edi, byte          ; exit(byte)
///   mov  eax, 60            ; SYS_exit
///   syscall
///   int3                    ; unreachable trap
/// ```
///
/// This is the **producer** end of the shell-pipeline integration test
/// ([`crate::proc::spawn::self_test_linux_pipe_fork_dup2_exec`]): a child
/// `dup2`s a pipe's write end onto fd 1 and `execve`s this image, so the
/// `byte` lands in the pipe for the parent to `read` back.  Tagged
/// `ELFOSABI_GNU` so the loader gives it the SysV stack + Linux ABI.
///
/// The PT_LOAD is `R+X` only: the byte is written to the loader-provided
/// (separately mapped, writable) SysV stack, not into this segment.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_write_byte_exit_elf(byte: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    // sub rsp, 16
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x10]);
    // mov byte [rsp], byte
    code.extend_from_slice(&[0xC6, 0x04, 0x24, byte]);
    // write(1, rsp, 1)
    code.extend_from_slice(&[0xBF, 0x01, 0x00, 0x00, 0x00]); // mov edi, 1
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp
    code.extend_from_slice(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,1; syscall
    // exit(byte)
    code.extend_from_slice(&[0xBF, byte, 0x00, 0x00, 0x00]); // mov edi, byte
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall
    code.push(0xCC); // int3

    let code_len = code.len();
    let file_size = code_offset as usize + code_len;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_len as u64);
    write_u64(&mut buf, ph + 40, code_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..file_size].copy_from_slice(&code);
    buf
}

/// Build a **Linux-ABI** `ET_EXEC` launcher ELF that exercises the canonical
/// **shell-pipeline** primitive end to end: `pipe2` + `fork` + `dup2` +
/// `execve` + blocking `read`.
///
/// ```text
///   sub  rsp, 32                 ; [rsp+0]=fds[0] [rsp+4]=fds[1]
///                                ; [rsp+8]=status [rsp+12]=read buf
///   pipe2(&fds, 0) ; test rax,rax ; js pipe_fail
///   fork           ; test rax,rax ; jz child
///   parent: read(fds[0], &buf, 1) ; test rax,rax ; jle parent_fail
///           wait4(-1, &status, 0, NULL)
///           exit(buf[0])         ; movzx edi, byte [rsp+12]
///   parent_fail: exit(0xA3)      ; read returned <= 0 (no byte / error)
///   pipe_fail:   exit(0xA4)      ; pipe2 returned < 0
///   child:  dup2(fds[1], 1)      ; redirect the pipe write end onto stdout
///           execve(path, argv=[path,NULL], envp=[NULL])
///           exit(0xE7)           ; only if execve returned (failed)
/// ```
///
/// The exec target is staged by the harness as
/// [`build_linux_write_byte_exit_elf`]`(sentinel)`: it writes `sentinel` to
/// fd 1 (which `dup2` aliased to the pipe's write end) and exits.  A clean
/// parent `exit(sentinel)` therefore proves the full chain:
///
///   * `pipe2` allocated a read/write fd pair in the launcher's table;
///   * `fork` cloned the **fd table** into the child (the child uses
///     `fds[1]`, inherited across the CoW fork, to feed `dup2`);
///   * `dup2` aliased the inherited write end onto a fixed fd (1) that the
///     `execve`'d target — which knows nothing of the dynamic pipe fds —
///     can write to;
///   * `execve` replaced the child image **without** disturbing the fd table
///     (fd 1 survives the exec);
///   * the byte traversed the pipe IPC path and the parent's blocking `read`
///     woke and returned it.
///
/// Self-diagnosing sentinels: `0xA4` = `pipe2` failed, `0xA3` = parent
/// `read` returned `<= 0`, `0xE7` = child `execve` failed.  `path_nul` must
/// be NUL-terminated.  Tagged `ELFOSABI_GNU` for the SysV stack + Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_pipe_fork_dup2_exec_test_elf(path_nul: &[u8]) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // sub rsp, 32  (fds[0], fds[1], status, read buf)
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x20]);
    // pipe2(&fds, 0): rdi = rsp, rsi = 0
    code.extend_from_slice(&[0x48, 0x89, 0xE7]); // mov rdi, rsp
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi
    code.extend_from_slice(&[0xB8, 0x25, 0x01, 0x00, 0x00, 0x0F, 0x05]); // mov eax,293; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x78, 0x00]); // js pipe_fail (rel8)
    let js_rel = code.len() - 1;

    // fork
    code.extend_from_slice(&[0xB8, 0x39, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,57; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x74, 0x00]); // jz child (rel8)
    let jz_rel = code.len() - 1;

    // parent: read(fds[0], &buf, 1)
    code.extend_from_slice(&[0x8B, 0x3C, 0x24]); // mov edi, [rsp]      (fds[0])
    code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x0C]); // lea rsi, [rsp+12]  (&buf)
    code.extend_from_slice(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
    code.extend_from_slice(&[0xB8, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,0; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x7E, 0x00]); // jle parent_fail (rel8)
    let jle_rel = code.len() - 1;
    // wait4(-1, &status, 0, NULL)
    code.extend_from_slice(&[0xBF, 0xFF, 0xFF, 0xFF, 0xFF]); // mov edi, -1
    code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x08]); // lea rsi, [rsp+8] (&status)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    code.extend_from_slice(&[0x45, 0x31, 0xD2]); // xor r10d, r10d
    code.extend_from_slice(&[0xB8, 0x3D, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,61; syscall
    // exit(buf[0])
    code.extend_from_slice(&[0x0F, 0xB6, 0x7C, 0x24, 0x0C]); // movzx edi, byte [rsp+12]
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // parent_fail: exit(0xA3) — read returned <= 0
    let parent_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xA3, 0x00, 0x00, 0x00]); // mov edi, 0xA3
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // pipe_fail: exit(0xA4) — pipe2 returned < 0
    let pipe_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xA4, 0x00, 0x00, 0x00]); // mov edi, 0xA4
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // child: dup2(fds[1], 1)
    let child = code.len();
    code.extend_from_slice(&[0x8B, 0x7C, 0x24, 0x04]); // mov edi, [rsp+4] (oldfd = fds[1])
    code.extend_from_slice(&[0xBE, 0x01, 0x00, 0x00, 0x00]); // mov esi, 1       (newfd = 1)
    code.extend_from_slice(&[0xB8, 0x21, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,33; syscall
    // execve(path, argv, envp)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, &argv
    let argv_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0xBA]); // movabs rdx, &envp
    let envp_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xB8, 0x3B, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,59; syscall
    // execve_fail: exit(0xE7)
    code.extend_from_slice(&[0xBF, 0xE7, 0x00, 0x00, 0x00]); // mov edi, 0xE7
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall
    code.push(0xCC); // int3 — unreachable trap

    // Patch the three forward rel8 jumps (disp measured from byte after disp).
    let js_disp = (pipe_fail as isize) - (js_rel as isize + 1);
    let jz_disp = (child as isize) - (jz_rel as isize + 1);
    let jle_disp = (parent_fail as isize) - (jle_rel as isize + 1);
    code[js_rel] = js_disp as u8;
    code[jz_rel] = jz_disp as u8;
    code[jle_rel] = jle_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    let argv_off = (path_end + 7) & !7usize;
    let envp_off = argv_off + 2 * 8;
    let file_size = envp_off + 8;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    let argv_vaddr = vaddr_of(argv_off);
    let envp_vaddr = vaddr_of(envp_off);
    code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes());
    code[argv_imm..argv_imm + 8].copy_from_slice(&argv_vaddr.to_le_bytes());
    code[envp_imm..envp_imm + 8].copy_from_slice(&envp_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // PT_LOAD R+W+X: W keeps argv/envp on a writable page; X for the code.
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_W | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path_nul);
    write_u64(&mut buf, argv_off, path_vaddr); // argv[0] = path
    write_u64(&mut buf, argv_off + 8, 0); // argv[1] = NULL
    write_u64(&mut buf, envp_off, 0); // envp[0] = NULL

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the
/// **`symlink(2)` + `readlink(2)`** syscalls end to end from ring 3:
///
/// ```text
///   sub  rsp, 64                     ; [rsp..rsp+64] = readlink output buf
///   symlink("Z", link)              ; create link -> "Z"
///   test rax,rax ; jnz symlink_fail ; success returns 0
///   readlink(link, rsp, 64)         ; read it back
///   cmp  rax, 1  ; jne  len_fail     ; "Z" is one byte, no trailing NUL
///   movzx eax, byte [rsp]
///   cmp  al, 'Z' ; jne content_fail  ; byte must round-trip
///   exit(0)
///   symlink_fail: exit(0xB1)
///   len_fail:     exit(0xB3)
///   content_fail: exit(0xB4)
/// ```
///
/// The `link_nul` argument is the link pathname (NUL-terminated); the harness
/// must remove any pre-existing entry at that path first (so `symlink` does
/// not fail `EEXIST`).  A clean `exit(0)` proves the full chain: the kernel
/// created a real symlink whose stored target (`"Z"`) was read back verbatim
/// with the Linux `readlink` contract (count returned, no trailing NUL).
///
/// Self-diagnosing sentinels: `0xB1` = `symlink` returned non-zero, `0xB3` =
/// `readlink` returned a length other than 1, `0xB4` = the byte read back was
/// not `'Z'`.  Tagged `ELFOSABI_GNU` for the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_symlink_readlink_test_elf(link_nul: &[u8]) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // sub rsp, 64  (readlink output buffer)
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x40]);

    // symlink(&target, &link)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &target
    let target_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, &link
    let link_imm1 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xB8, 0x58, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,88; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz symlink_fail (rel8)
    let jnz_sym_rel = code.len() - 1;

    // readlink(&link, rsp, 64)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &link
    let link_imm2 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp
    code.extend_from_slice(&[0xBA, 0x40, 0x00, 0x00, 0x00]); // mov edx, 64
    code.extend_from_slice(&[0xB8, 0x59, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,89; syscall
    code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x01]); // cmp rax, 1
    code.extend_from_slice(&[0x75, 0x00]); // jne len_fail (rel8)
    let jne_len_rel = code.len() - 1;

    // check buf[0] == 'Z'
    code.extend_from_slice(&[0x0F, 0xB6, 0x04, 0x24]); // movzx eax, byte [rsp]
    code.extend_from_slice(&[0x3C, 0x5A]); // cmp al, 0x5A ('Z')
    code.extend_from_slice(&[0x75, 0x00]); // jne content_fail (rel8)
    let jne_content_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // symlink_fail: exit(0xB1)
    let symlink_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xB1, 0x00, 0x00, 0x00]); // mov edi, 0xB1
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // len_fail: exit(0xB3)
    let len_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xB3, 0x00, 0x00, 0x00]); // mov edi, 0xB3
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // content_fail: exit(0xB4)
    let content_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xB4, 0x00, 0x00, 0x00]); // mov edi, 0xB4
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall
    code.push(0xCC); // int3 — unreachable trap

    // Patch the three forward rel8 jumps (disp from byte after the disp).
    let jnz_sym_disp = (symlink_fail as isize) - (jnz_sym_rel as isize + 1);
    let jne_len_disp = (len_fail as isize) - (jne_len_rel as isize + 1);
    let jne_content_disp = (content_fail as isize) - (jne_content_rel as isize + 1);
    code[jnz_sym_rel] = jnz_sym_disp as u8;
    code[jne_len_rel] = jne_len_disp as u8;
    code[jne_content_rel] = jne_content_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let target: &[u8] = b"Z\0";
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let target_off = data_base;
    let target_end = target_off + target.len();
    let link_off = target_end;
    let link_end = link_off + link_nul.len();
    let file_size = link_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let target_vaddr = vaddr_of(target_off);
    let link_vaddr = vaddr_of(link_off);
    code[target_imm..target_imm + 8].copy_from_slice(&target_vaddr.to_le_bytes());
    code[link_imm1..link_imm1 + 8].copy_from_slice(&link_vaddr.to_le_bytes());
    code[link_imm2..link_imm2 + 8].copy_from_slice(&link_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[target_off..target_end].copy_from_slice(target);
    buf[link_off..link_end].copy_from_slice(link_nul);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the **`link(2)`**
/// (hard-link) syscall from ring 3:
///
/// ```text
///   sub  rsp, 16                     ; [rsp] = read buf
///   link(old, new)                  ; create new as a hard link to old
///   test rax,rax ; jnz link_fail    ; success returns 0
///   open(new, O_RDONLY)             ; open the new name
///   test rax,rax ; js  open_fail    ; fd < 0 on error
///   mov  r8, rax                    ; save fd
///   read(fd, rsp, 1)                ; read one byte through the link
///   cmp  rax, 1  ; jne read_fail
///   movzx eax, byte [rsp]
///   cmp  al, 'L' ; jne content_fail ; byte must match the source's contents
///   exit(0)
///   link_fail:    exit(0xC1)
///   open_fail:    exit(0xC2)
///   read_fail:    exit(0xC3)
///   content_fail: exit(0xC4)
/// ```
///
/// The harness pre-creates `old` with the single byte `'L'`, removes any
/// pre-existing `new`, and passes both NUL-terminated paths.  A clean
/// `exit(0)` proves the kernel created a real hard link whose contents (the
/// shared inode's data) are readable through the new name.
///
/// Self-diagnosing sentinels: `0xC1` = `link` returned non-zero, `0xC2` =
/// `open(new)` failed, `0xC3` = `read` returned a length other than 1, `0xC4`
/// = the byte read back was not `'L'`.  Tagged `ELFOSABI_GNU`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_link_test_elf(old_nul: &[u8], new_nul: &[u8]) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // sub rsp, 16 (read buffer)
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x10]);

    // link(&old, &new)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &old
    let old_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, &new
    let new_imm1 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xB8, 0x56, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,86; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz link_fail
    let jnz_link_rel = code.len() - 1;

    // open(&new, O_RDONLY, 0)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &new
    let new_imm2 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi (flags = O_RDONLY)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (mode = 0)
    code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,2; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x78, 0x00]); // js open_fail (fd < 0)
    let js_open_rel = code.len() - 1;
    code.extend_from_slice(&[0x49, 0x89, 0xC0]); // mov r8, rax (save fd)

    // read(fd, rsp, 1)
    code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp
    code.extend_from_slice(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
    code.extend_from_slice(&[0xB8, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,0; syscall
    code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x01]); // cmp rax, 1
    code.extend_from_slice(&[0x75, 0x00]); // jne read_fail
    let jne_read_rel = code.len() - 1;

    // check buf[0] == 'L'
    code.extend_from_slice(&[0x0F, 0xB6, 0x04, 0x24]); // movzx eax, byte [rsp]
    code.extend_from_slice(&[0x3C, 0x4C]); // cmp al, 0x4C ('L')
    code.extend_from_slice(&[0x75, 0x00]); // jne content_fail
    let jne_content_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // link_fail: exit(0xC1)
    let link_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xC1, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // open_fail: exit(0xC2)
    let open_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xC2, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // read_fail: exit(0xC3)
    let read_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xC3, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // content_fail: exit(0xC4)
    let content_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xC4, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3

    // Patch the four forward rel8 jumps.
    let jnz_link_disp = (link_fail as isize) - (jnz_link_rel as isize + 1);
    let js_open_disp = (open_fail as isize) - (js_open_rel as isize + 1);
    let jne_read_disp = (read_fail as isize) - (jne_read_rel as isize + 1);
    let jne_content_disp = (content_fail as isize) - (jne_content_rel as isize + 1);
    code[jnz_link_rel] = jnz_link_disp as u8;
    code[js_open_rel] = js_open_disp as u8;
    code[jne_read_rel] = jne_read_disp as u8;
    code[jne_content_rel] = jne_content_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let old_off = data_base;
    let old_end = old_off + old_nul.len();
    let new_off = old_end;
    let new_end = new_off + new_nul.len();
    let file_size = new_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let old_vaddr = vaddr_of(old_off);
    let new_vaddr = vaddr_of(new_off);
    code[old_imm..old_imm + 8].copy_from_slice(&old_vaddr.to_le_bytes());
    code[new_imm1..new_imm1 + 8].copy_from_slice(&new_vaddr.to_le_bytes());
    code[new_imm2..new_imm2 + 8].copy_from_slice(&new_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[old_off..old_end].copy_from_slice(old_nul);
    buf[new_off..new_end].copy_from_slice(new_nul);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the
/// **`utimensat(2)`** timestamp-update syscall from ring 3:
///
/// ```text
///   sub  rsp, 64                          ; scratch for struct timespec[2]
///   mov  qword [rsp+0],  atime_sec        ; times[0].tv_sec
///   mov  qword [rsp+8],  0                ; times[0].tv_nsec
///   mov  qword [rsp+16], mtime_sec        ; times[1].tv_sec
///   mov  qword [rsp+24], 0                ; times[1].tv_nsec
///   utimensat(AT_FDCWD, &path, rsp, 0)
///   test rax,rax ; jnz fail               ; success returns 0
///   exit(0)
///   fail: exit(0xD1)
/// ```
///
/// The harness pre-creates `path`, then independently reads the file's
/// metadata back through the VFS and asserts `accessed_ns ==
/// atime_sec * 1e9` and `modified_ns == mtime_sec * 1e9`.  A clean
/// `exit(0)` plus the kernel-side timestamp match proves the kernel applied
/// the requested times.  `0xD1` = `utimensat` returned non-zero.
///
/// `atime_sec` / `mtime_sec` are emitted as sign-extended `imm32`, so callers
/// must keep them in `0..=i32::MAX` (positive epoch seconds).  Tagged
/// `ELFOSABI_GNU`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn build_linux_utimensat_test_elf(
    path_nul: &[u8],
    atime_sec: i32,
    mtime_sec: i32,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // sub rsp, 64 (struct timespec[2] = 32B + slack)
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x40]);

    // Build the timespec[2] array on the stack.  Encoding for
    // `mov qword [rsp+disp8], imm32` (imm32 sign-extended to 64): 48 C7 44 24
    // <disp8> <imm32 LE>.
    let mut store_qword = |disp: u8, imm: i32| {
        code.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, disp]);
        code.extend_from_slice(&imm.to_le_bytes());
    };
    store_qword(0x00, atime_sec); // times[0].tv_sec
    store_qword(0x08, 0); // times[0].tv_nsec
    store_qword(0x10, mtime_sec); // times[1].tv_sec
    store_qword(0x18, 0); // times[1].tv_nsec

    // utimensat(AT_FDCWD, &path, rsp, 0)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, AT_FDCWD (-100)
    code.extend_from_slice(&(-100i64).to_le_bytes());
    code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0x89, 0xE2]); // mov rdx, rsp (times)
    code.extend_from_slice(&[0x4D, 0x31, 0xD2]); // xor r10, r10 (flags = 0)
    code.extend_from_slice(&[0xB8, 0x18, 0x01, 0x00, 0x00, 0x0F, 0x05]); // mov eax,280; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz fail
    let jnz_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // fail: exit(0xD1)
    let fail = code.len();
    code.extend_from_slice(&[0xBF, 0xD1, 0x00, 0x00, 0x00]); // mov edi, 0xD1
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall
    code.push(0xCC); // int3

    // Patch the forward rel8 jump.
    let jnz_disp = (fail as isize) - (jnz_rel as isize + 1);
    code[jnz_rel] = jnz_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    let file_size = path_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path_nul);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the **`chmod(2)`**
/// and **`chown(2)`** metadata-mutation syscalls from ring 3:
///
/// ```text
///   chmod(&path, mode)
///   test rax,rax ; jnz fail1            ; success returns 0
///   chown(&path, uid, gid)
///   test rax,rax ; jnz fail2
///   exit(0)
///   fail1: exit(0xE1)
///   fail2: exit(0xE2)
/// ```
///
/// The harness pre-creates `path`, then independently reads the file's
/// metadata back and asserts `permissions == mode & 0o777`, `uid == uid`,
/// `gid == gid`.  `0xE1` = `chmod` failed, `0xE2` = `chown` failed.
/// `mode`/`uid`/`gid` are emitted as `imm32`.  Tagged `ELFOSABI_GNU`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_chmod_chown_test_elf(
    path_nul: &[u8],
    mode: u32,
    uid: u32,
    gid: u32,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // chmod(&path, mode)  [nr 90]
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm1 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.push(0xBE); // mov esi, mode
    code.extend_from_slice(&mode.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x5A, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,90; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz fail1
    let jnz1_rel = code.len() - 1;

    // chown(&path, uid, gid)  [nr 92]
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm2 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.push(0xBE); // mov esi, uid
    code.extend_from_slice(&uid.to_le_bytes());
    code.push(0xBA); // mov edx, gid
    code.extend_from_slice(&gid.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x5C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,92; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz fail2
    let jnz2_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // fail1: exit(0xE1)
    let fail1 = code.len();
    code.extend_from_slice(&[0xBF, 0xE1, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // fail2: exit(0xE2)
    let fail2 = code.len();
    code.extend_from_slice(&[0xBF, 0xE2, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3

    // Patch the two forward rel8 jumps.
    let jnz1_disp = (fail1 as isize) - (jnz1_rel as isize + 1);
    let jnz2_disp = (fail2 as isize) - (jnz2_rel as isize + 1);
    code[jnz1_rel] = jnz1_disp as u8;
    code[jnz2_rel] = jnz2_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    let file_size = path_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    code[path_imm1..path_imm1 + 8].copy_from_slice(&path_vaddr.to_le_bytes());
    code[path_imm2..path_imm2 + 8].copy_from_slice(&path_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path_nul);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the
/// **`truncate(2)`** (path-based) and **`ftruncate(2)`** (fd-based)
/// file-resize syscalls from ring 3:
///
/// ```text
///   truncate(&path, shrink_size)        ; shrink the pre-staged file
///   test rax,rax ; jnz trunc_fail       ; success returns 0
///   open(&path, O_RDWR, 0)              ; reopen writable for ftruncate
///   test rax,rax ; js  open_fail        ; fd < 0 on error
///   mov  r8, rax                        ; save fd
///   ftruncate(fd, grow_size)            ; grow (zero-extend) via the fd
///   test rax,rax ; jnz ftrunc_fail      ; success returns 0
///   exit(0)
///   trunc_fail:  exit(0xF1)
///   open_fail:   exit(0xF2)
///   ftrunc_fail: exit(0xF3)
/// ```
///
/// The harness pre-creates `path` with a known byte pattern longer than
/// both sizes, then independently reads the file back through the VFS
/// and asserts the final length equals `grow_size` (the last resize) with
/// the leading `shrink_size` bytes preserved and the grown tail zero-
/// filled.  A clean `exit(0)` plus the kernel-side length/content match
/// proves both the path and fd resize paths reach the real `Vfs::truncate`.
///
/// Sentinels: `0xF1` = `truncate` returned non-zero, `0xF2` = `open(O_RDWR)`
/// failed (fd < 0), `0xF3` = `ftruncate` returned non-zero.  `shrink_size`
/// and `grow_size` are emitted as `imm32` (loaded into `esi`, zero-extended
/// to `rsi`), so callers must keep them in `0..=u32::MAX`.  Tagged
/// `ELFOSABI_GNU`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_truncate_test_elf(
    path_nul: &[u8],
    shrink_size: u32,
    grow_size: u32,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // truncate(&path, shrink_size)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm1 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.push(0xBE); // mov esi, imm32 (length)
    code.extend_from_slice(&shrink_size.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x4C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,76; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz trunc_fail
    let jnz_trunc_rel = code.len() - 1;

    // open(&path, O_RDWR, 0)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm2 = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xBE, 0x02, 0x00, 0x00, 0x00]); // mov esi, 2 (O_RDWR)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (mode = 0)
    code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,2; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x78, 0x00]); // js open_fail (fd < 0)
    let js_open_rel = code.len() - 1;
    code.extend_from_slice(&[0x49, 0x89, 0xC0]); // mov r8, rax (save fd)

    // ftruncate(fd, grow_size)
    code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
    code.push(0xBE); // mov esi, imm32 (length)
    code.extend_from_slice(&grow_size.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x4D, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,77; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz ftrunc_fail
    let jnz_ftrunc_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // trunc_fail: exit(0xF1)
    let trunc_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xF1, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // open_fail: exit(0xF2)
    let open_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xF2, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // ftrunc_fail: exit(0xF3)
    let ftrunc_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xF3, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3 — unreachable trap

    // Patch the three forward rel8 jumps.
    let jnz_trunc_disp = (trunc_fail as isize) - (jnz_trunc_rel as isize + 1);
    let js_open_disp = (open_fail as isize) - (js_open_rel as isize + 1);
    let jnz_ftrunc_disp = (ftrunc_fail as isize) - (jnz_ftrunc_rel as isize + 1);
    code[jnz_trunc_rel] = jnz_trunc_disp as u8;
    code[js_open_rel] = js_open_disp as u8;
    code[jnz_ftrunc_rel] = jnz_ftrunc_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    let file_size = path_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    code[path_imm1..path_imm1 + 8].copy_from_slice(&path_vaddr.to_le_bytes());
    code[path_imm2..path_imm2 + 8].copy_from_slice(&path_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path_nul);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises
/// **`fchmodat2(2)` with `AT_EMPTY_PATH`** (Linux syscall #452) from
/// ring 3 — the fd-targeted chmod form whose path resolution goes
/// through `handle_path`:
///
/// ```text
///   open(&path, O_RDWR, 0)                       ; fd for the target file
///   test rax,rax ; js  open_fail                 ; fd < 0 on error
///   mov  r8, rax                                 ; save fd
///   fchmodat2(fd, &empty, mode, AT_EMPTY_PATH)   ; chmod via the fd
///   test rax,rax ; jnz chmod_fail                ; success returns 0
///   exit(0)
///   open_fail:  exit(0xE5)
///   chmod_fail: exit(0xE6)
/// ```
///
/// The harness pre-creates `path`, then independently reads the file's
/// metadata back and asserts `permissions == (mode & 0o7777)`.  A clean
/// `exit(0)` plus the kernel-side mode match proves the
/// `AT_EMPTY_PATH → dirfd → handle_path → Vfs::set_permissions` branch
/// works end-to-end from ring 3.  `mode` is emitted as `imm32` (loaded
/// into `edx`); `AT_EMPTY_PATH` (0x1000) is loaded into `r10d` (the 4th
/// syscall arg).  Sentinels: `0xE5` = `open(O_RDWR)` failed, `0xE6` =
/// `fchmodat2` returned non-zero.  Tagged `ELFOSABI_GNU`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_fchmodat2_emptypath_test_elf(
    path_nul: &[u8],
    mode: u32,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // open(&path, O_RDWR, 0)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xBE, 0x02, 0x00, 0x00, 0x00]); // mov esi, 2 (O_RDWR)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (mode = 0)
    code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,2; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x78, 0x00]); // js open_fail
    let js_open_rel = code.len() - 1;
    code.extend_from_slice(&[0x49, 0x89, 0xC0]); // mov r8, rax (save fd)

    // fchmodat2(fd, &empty, mode, AT_EMPTY_PATH)
    code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
    code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, &empty
    let empty_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.push(0xBA); // mov edx, imm32 (mode)
    code.extend_from_slice(&mode.to_le_bytes());
    code.extend_from_slice(&[0x41, 0xBA, 0x00, 0x10, 0x00, 0x00]); // mov r10d, 0x1000
    code.extend_from_slice(&[0xB8, 0xC4, 0x01, 0x00, 0x00, 0x0F, 0x05]); // mov eax,452; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz chmod_fail
    let jnz_chmod_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // open_fail: exit(0xE5)
    let open_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xE5, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // chmod_fail: exit(0xE6)
    let chmod_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xE6, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3 — unreachable trap

    // Patch the two forward rel8 jumps.
    let js_open_disp = (open_fail as isize) - (js_open_rel as isize + 1);
    let jnz_chmod_disp = (chmod_fail as isize) - (jnz_chmod_rel as isize + 1);
    code[js_open_rel] = js_open_disp as u8;
    code[jnz_chmod_rel] = jnz_chmod_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let empty: &[u8] = b"\0";
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    let empty_off = path_end;
    let empty_end = empty_off + empty.len();
    let file_size = empty_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    let empty_vaddr = vaddr_of(empty_off);
    code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes());
    code[empty_imm..empty_imm + 8].copy_from_slice(&empty_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path_nul);
    buf[empty_off..empty_end].copy_from_slice(empty);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises the **virtio-gpu
/// `DRM_IOCTL_VIRTGPU_GETPARAM`** render ioctl on `/dev/dri/renderD128` from
/// ring 3 — the honest "no-3D" path landed for Q18 (design-decisions §59):
///
/// ```text
///   open("/dev/dri/renderD128", O_RDWR, 0)     ; render node fd
///   test rax,rax ; js open_fail                ; fd < 0 on error
///   mov r8, rax                                ; save fd
///   sub rsp, 64                                ; scratch on the stack
///   mov [rsp]    = 0xFFFF_FFFF_FFFF_FFFF        ; result sentinel
///   mov [rsp+8]  = VIRTGPU_PARAM_3D_FEATURES(1) ; getparam.param
///   mov [rsp+16] = rsp                          ; getparam.value -> result slot
///   ioctl(fd, DRM_IOCTL_VIRTGPU_GETPARAM, rsp+8)
///   test rax,rax ; jnz ioctl_fail              ; GETPARAM must succeed (ret 0)
///   mov rax, [rsp] ; test rax,rax ; jnz value_fail ; kernel must write 0 (no 3D)
///   exit(0)
///   open_fail:  exit(0xE1)
///   ioctl_fail: exit(0xE2)
///   value_fail: exit(0xE3)
/// ```
///
/// A clean `exit(0)` proves the full ring-3 path: `open(renderD128)` →
/// `drm_card_ioctl` → `virtgpu_render_ioctl` → `virtgpu_getparam_ioctl`, with
/// the honest policy value (`3D_FEATURES = 0`) copied back to userspace. The
/// distinct sentinels let the harness tell an open failure from an ioctl
/// failure from a wrong reported value. Tagged `ELFOSABI_GNU` so the loader
/// treats it as Linux-ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_virtgpu_getparam_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // DRM_IOCTL_VIRTGPU_GETPARAM request number (asserted in virtgpu_uapi).
    let getparam_ioctl = crate::drm::virtgpu_uapi::DRM_IOCTL_VIRTGPU_GETPARAM;
    // VIRTGPU_PARAM_3D_FEATURES.
    let param_3d = crate::drm::virtgpu_uapi::VIRTGPU_PARAM_3D_FEATURES;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // open("/dev/dri/renderD128", O_RDWR, 0)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xBE, 0x02, 0x00, 0x00, 0x00]); // mov esi, 2 (O_RDWR)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (mode 0)
    code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,2; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x78, 0x00]); // js open_fail
    let js_open_rel = code.len() - 1;
    code.extend_from_slice(&[0x49, 0x89, 0xC0]); // mov r8, rax (save fd)

    // Stack scratch: result slot at [rsp], getparam struct at [rsp+8].
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x40]); // sub rsp, 64
    // result slot = all-ones sentinel (detect that the kernel writes 0).
    code.extend_from_slice(&[0x48, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF]); // mov rax, -1
    code.extend_from_slice(&[0x48, 0x89, 0x04, 0x24]); // mov [rsp], rax
    // getparam.param = VIRTGPU_PARAM_3D_FEATURES (a small u64, fits imm32).
    code.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x08]); // mov qword [rsp+8], imm32
    code.extend_from_slice(&(param_3d as u32).to_le_bytes());
    // getparam.value = rsp (address of the result slot).
    code.extend_from_slice(&[0x48, 0x89, 0x64, 0x24, 0x10]); // mov [rsp+16], rsp

    // ioctl(fd, DRM_IOCTL_VIRTGPU_GETPARAM, &getparam)
    code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
    code.push(0xBE); // mov esi, imm32 (ioctl request; zero-extended to rsi)
    code.extend_from_slice(&getparam_ioctl.to_le_bytes());
    code.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x08]); // lea rdx, [rsp+8]
    code.extend_from_slice(&[0xB8, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,16; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz ioctl_fail
    let jnz_ioctl_rel = code.len() - 1;

    // Verify the kernel wrote the honest 3D_FEATURES value (0) into the slot.
    code.extend_from_slice(&[0x48, 0x8B, 0x04, 0x24]); // mov rax, [rsp]
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz value_fail
    let jnz_value_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // open_fail: exit(0xE1)
    let open_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xE1, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    // ioctl_fail: exit(0xE2)
    let ioctl_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xE2, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    // value_fail: exit(0xE3)
    let value_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xE3, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3 — unreachable trap

    // Patch the three forward rel8 jumps.
    let js_open_disp = (open_fail as isize) - (js_open_rel as isize + 1);
    let jnz_ioctl_disp = (ioctl_fail as isize) - (jnz_ioctl_rel as isize + 1);
    let jnz_value_disp = (value_fail as isize) - (jnz_value_rel as isize + 1);
    code[js_open_rel] = js_open_disp as u8;
    code[jnz_ioctl_rel] = jnz_ioctl_disp as u8;
    code[jnz_value_rel] = jnz_value_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let path: &[u8] = b"/dev/dri/renderD128\0";
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path.len();
    let file_size = path_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that exercises
/// **`fallocate(2)` mode 0 (posix_fallocate grow)** (Linux syscall #285)
/// from ring 3 — the fd-targeted path whose backing resolution goes
/// through `handle_path` → `Vfs::truncate`:
///
/// ```text
///   open(&path, O_RDWR, 0)                 ; fd for the target file
///   test rax,rax ; js  open_fail           ; fd < 0 on error
///   mov  r8, rax                           ; save fd
///   fallocate(fd, 0, 0, grow_len)          ; mode=0, offset=0, len=grow
///   test rax,rax ; jnz falloc_fail         ; success returns 0
///   exit(0)
///   open_fail:   exit(0xD1)
///   falloc_fail: exit(0xD2)
/// ```
///
/// The harness pre-creates `path` with a *smaller* size, then independently
/// reads the file size back and asserts it grew to exactly `grow_len` (the
/// posix_fallocate guarantee: logical size becomes at least `offset+len`,
/// here `0+grow_len`).  A clean `exit(0)` plus the kernel-side size match
/// proves the `fd → handle_path → Vfs::file_size/Vfs::truncate` grow path
/// works end-to-end from ring 3.  `grow_len` is emitted as `imm32` into
/// `r10d` (the 4th syscall arg); `mode` (`esi`) and `offset` (`edx`) are
/// zeroed.  Sentinels: `0xD1` = `open(O_RDWR)` failed, `0xD2` = `fallocate`
/// returned non-zero.  Tagged `ELFOSABI_GNU`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_fallocate_grow_test_elf(
    path_nul: &[u8],
    grow_len: u32,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // open(&path, O_RDWR, 0)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xBE, 0x02, 0x00, 0x00, 0x00]); // mov esi, 2 (O_RDWR)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (mode = 0)
    code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,2; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x78, 0x00]); // js open_fail
    let js_open_rel = code.len() - 1;
    code.extend_from_slice(&[0x49, 0x89, 0xC0]); // mov r8, rax (save fd)

    // fallocate(fd, 0, 0, grow_len)
    code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi (mode = 0)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (offset = 0)
    code.push(0x41);
    code.push(0xBA); // mov r10d, imm32 (grow_len)
    code.extend_from_slice(&grow_len.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x1D, 0x01, 0x00, 0x00, 0x0F, 0x05]); // mov eax,285; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x75, 0x00]); // jnz falloc_fail
    let jnz_falloc_rel = code.len() - 1;

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall

    // open_fail: exit(0xD1)
    let open_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xD1, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // falloc_fail: exit(0xD2)
    let falloc_fail = code.len();
    code.extend_from_slice(&[0xBF, 0xD2, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3 — unreachable trap

    // Patch the two forward rel8 jumps.
    let js_open_disp = (open_fail as isize) - (js_open_rel as isize + 1);
    let jnz_falloc_disp = (falloc_fail as isize) - (jnz_falloc_rel as isize + 1);
    code[js_open_rel] = js_open_disp as u8;
    code[jnz_falloc_rel] = jnz_falloc_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    let file_size = path_end;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    let path_vaddr = vaddr_of(path_off);
    code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes());

    // --- File image ---
    let seg_len = file_size - code_offset as usize;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_end].copy_from_slice(path_nul);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that verifies the **`%fs`
/// (TLS) base survives context switches**.  The process installs a
/// caller-chosen `sentinel` FS base, then in a loop yields the CPU and
/// re-reads its FS base, asserting it is unchanged on every iteration:
///
/// ```text
///   sub  rsp, 16                       ; [rsp+0] = ARCH_GET_FS out slot
///   arch_prctl(ARCH_SET_FS, sentinel)  ; install our TLS base
///   mov  r15d, 50                      ; loop counter
/// loop_top:
///   sched_yield()                      ; give the other process the CPU
///   arch_prctl(ARCH_GET_FS, &slot)     ; read FS base back (live MSR)
///   cmp  [rsp], sentinel ; jne fail    ; must equal what we set
///   dec  r15d ; jnz loop_top
///   exit(0)                            ; success
/// fail:
///   exit(0xF1)                         ; FS base was clobbered
/// ```
///
/// The harness ([`crate::proc::spawn::self_test_linux_fs_tls_switch`])
/// spawns **two** of these with **distinct** sentinels.  The self-tests
/// run single-CPU before `smp::init()`, so the two processes time-share
/// CPU 0 via the cooperative `sched_yield`, interleaving deterministically.
/// `IA32_FS_BASE` is a global CPU register **not** part of the saved GP
/// `Context`; if the scheduler fails to swap it on switch-in, process A
/// resuming after B's yield would read B's sentinel and `exit(0xF1)`.
/// Both processes exiting 0 proves the per-task FS base is restored.
///
/// `sentinel` must be a canonical user address (`< 1 << 47`) and non-zero,
/// matching the `arch_prctl(ARCH_SET_FS)` validation.  The PT_LOAD is
/// `R+X` only (the out slot lives on the loader-provided writable SysV
/// stack).  Tagged `ELFOSABI_GNU` for the Linux ABI + SysV stack.
#[must_use]
pub fn build_linux_fs_tls_test_elf(sentinel: u64) -> alloc::vec::Vec<u8> {
    // ARCH_SET_FS = 0x1002, ARCH_GET_FS = 0x1003; fail with 0xF1.
    build_linux_seg_base_test_elf(0x1002, 0x1003, sentinel, 0xF1)
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that verifies the **userspace
/// `%gs` base survives context switches** — the `%gs` analogue of
/// [`build_linux_fs_tls_test_elf`].
///
/// Installs `sentinel` via `arch_prctl(ARCH_SET_GS)`, then loops
/// `sched_yield` + `arch_prctl(ARCH_GET_GS)` asserting the value is
/// unchanged; `exit(0)` on success, `exit(0xF2)` if the base was clobbered.
/// Used by [`crate::proc::spawn::self_test_linux_gs_tls_switch`] with two
/// distinct sentinels.  Under Slate's entry-stub convention the userspace
/// `%gs` base is the active `IA32_GS_BASE` (symmetric to `%fs`); this test
/// exercises `arch_prctl(ARCH_SET_GS/GET_GS)` and the scheduler's switch-in
/// restore of that MSR.  `sentinel` must be a canonical user address
/// (`< 1 << 47`, non-zero).
#[must_use]
pub fn build_linux_gs_tls_test_elf(sentinel: u64) -> alloc::vec::Vec<u8> {
    // ARCH_SET_GS = 0x1001, ARCH_GET_GS = 0x1004; fail with 0xF2.
    build_linux_seg_base_test_elf(0x1001, 0x1004, sentinel, 0xF2)
}

/// Shared body for the `%fs`/`%gs` segment-base context-switch tests.
///
/// Emits: install `sentinel` via `arch_prctl(set_code, sentinel)`, then loop
/// 50× `{ sched_yield(); arch_prctl(get_code, &slot); if slot != sentinel
/// goto fail }`; `exit(0)` on success, `exit(fail_code)` on mismatch.  The
/// two arch_prctl `code` immediates and the failure exit code are the only
/// things that differ between the FS and GS variants.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
fn build_linux_seg_base_test_elf(
    set_code: u32,
    get_code: u32,
    sentinel: u64,
    fail_code: u8,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // sub rsp, 16  (arch_prctl GET output slot at [rsp+0])
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x10]);
    // arch_prctl(set_code, sentinel): eax=158, edi=set_code, rsi=sentinel
    code.extend_from_slice(&[0xB8, 0x9E, 0x00, 0x00, 0x00]); // mov eax, 158
    code.push(0xBF); // mov edi, set_code
    code.extend_from_slice(&set_code.to_le_bytes());
    code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, sentinel
    let set_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    // mov r15d, 50  (loop counter)
    code.extend_from_slice(&[0x41, 0xBF, 0x32, 0x00, 0x00, 0x00]);

    // loop_top:
    let loop_top = code.len();
    // sched_yield()
    code.extend_from_slice(&[0xB8, 0x18, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,24; syscall
    // arch_prctl(get_code, &slot): eax=158, edi=get_code, rsi=rsp
    code.extend_from_slice(&[0xB8, 0x9E, 0x00, 0x00, 0x00]); // mov eax, 158
    code.push(0xBF); // mov edi, get_code
    code.extend_from_slice(&get_code.to_le_bytes());
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    // cmp [rsp], sentinel
    code.extend_from_slice(&[0x48, 0x8B, 0x04, 0x24]); // mov rax, [rsp]
    code.extend_from_slice(&[0x48, 0xB9]); // movabs rcx, sentinel
    let cmp_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0x48, 0x39, 0xC8]); // cmp rax, rcx
    code.extend_from_slice(&[0x75, 0x00]); // jne fail (rel8)
    let jne_rel = code.len() - 1;
    // dec r15d ; jnz loop_top
    code.extend_from_slice(&[0x41, 0xFF, 0xCF]); // dec r15d
    code.extend_from_slice(&[0x75, 0x00]); // jnz loop_top (rel8, backward)
    let jnz_rel = code.len() - 1;

    // success: exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall
    code.push(0xCC); // int3

    // fail: exit(fail_code)
    let fail = code.len();
    code.extend_from_slice(&[0xBF, fail_code, 0x00, 0x00, 0x00]); // mov edi, fail_code
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,60; syscall
    code.push(0xCC); // int3

    // Patch jumps (disp measured from the byte after the disp byte).
    let jne_disp = (fail as isize) - (jne_rel as isize + 1);
    let jnz_disp = (loop_top as isize) - (jnz_rel as isize + 1);
    code[jne_rel] = jne_disp as u8;
    code[jnz_rel] = jnz_disp as u8;
    // Patch the two sentinel imm64 slots.
    code[set_imm..set_imm + 8].copy_from_slice(&sentinel.to_le_bytes());
    code[cmp_imm..cmp_imm + 8].copy_from_slice(&sentinel.to_le_bytes());

    let code_len = code.len();
    let file_size = code_offset as usize + code_len;
    let mut buf = vec![0u8; file_size];

    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_len as u64);
    write_u64(&mut buf, ph + 40, code_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..file_size].copy_from_slice(&code);
    buf
}

/// Build a minimal **Linux-ABI** `ET_EXEC` test ELF that simply calls
/// `exit(exit_code)`:
///
/// ```text
///   mov edi, exit_code   ; BF <imm32>
///   mov eax, 60          ; Linux SYS_exit
///   syscall
///   int3                 ; unreachable trap
/// ```
///
/// Tagged `ELFOSABI_GNU` so [`ElfFile::detect_linux_abi`] reports true and
/// `spawn_process`/`exec_process` route it through the Linux ABI.  Handy as
/// the *target* of an `execve`/`execveat` test: the resulting zombie's exit
/// code is exactly `exit_code`, proving control reached this image.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_exit_elf(exit_code: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 16;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU; // tag Linux/GNU so detect_linux_abi() is true

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code: exit(exit_code). ---
    let cs = code_offset as usize;
    for byte in &mut buf[cs..(cs + code_size as usize)] {
        *byte = 0xCC; // INT3 trap padding.
    }
    // mov edi, exit_code  (BF <imm32>)
    buf[cs] = 0xBF;
    buf[cs + 1] = exit_code;
    buf[cs + 2] = 0x00;
    buf[cs + 3] = 0x00;
    buf[cs + 4] = 0x00;
    // mov eax, 60  (B8 3C 00 00 00) — Linux SYS_exit
    buf[cs + 5] = 0xB8;
    buf[cs + 6] = 0x3C;
    buf[cs + 7] = 0x00;
    buf[cs + 8] = 0x00;
    buf[cs + 9] = 0x00;
    // syscall  (0F 05)
    buf[cs + 10] = 0x0F;
    buf[cs + 11] = 0x05;

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` launcher ELF that `execveat(2)`s a target
/// program, used to test the `execveat` exec path end-to-end from ring 3.
///
/// Two forms are produced, selected by `fexecve`:
///
/// - `fexecve == false` (path form):
///   ```text
///     mov   rdi, -100              ; AT_FDCWD
///     movabs rsi, &path            ; pathname
///     movabs rdx, &argv            ; argv = [&path; argc] ++ [NULL]
///     movabs r10, &envp            ; envp = [NULL]
///     mov   r8d, flags_extra       ; flags (0, or e.g. AT_SYMLINK_NOFOLLOW)
///     mov   eax, 322               ; SYS_execveat
///     syscall
///     mov   edi, 0xEE              ; (only reached if exec failed)
///     mov   eax, 60                ; SYS_exit
///     syscall
///   ```
///
/// - `fexecve == true` (`AT_EMPTY_PATH` form, glibc's `fexecve`):
///   ```text
///     movabs rdi, &path            ; open(path, O_RDONLY, 0)
///     xor   esi, esi
///     xor   edx, edx
///     mov   eax, 2                 ; SYS_open
///     syscall                      ; rax = fd
///     mov   rdi, rax               ; dirfd = fd
///     movabs rsi, &empty           ; pathname = "" (AT_EMPTY_PATH)
///     movabs rdx, &argv
///     movabs r10, &envp
///     mov   r8d, 0x1000|flags_extra; flags = AT_EMPTY_PATH (+ extra)
///     mov   eax, 322               ; SYS_execveat
///     syscall
///     mov   edi, 0xEE
///     mov   eax, 60
///     syscall
///   ```
///
/// `flags_extra` is OR'd into the `flags` argument (the fexecve form always
/// adds `AT_EMPTY_PATH` on top). Pass `0` for the plain forms, or e.g.
/// `AT_SYMLINK_NOFOLLOW` (0x100) to test that `execveat` refuses a symlink
/// target (the launcher then exits `0xEE` because execveat returns `ELOOP`).
///
/// `argc` (clamped to ≥1) sets how many entries the passed `argv` holds (all
/// pointing at the path string). Pair with [`build_linux_argc_exit_test_elf`]
/// as the target to verify `execve` rebuilds the new image's initial stack
/// with the *passed* argv: the target then exits with `argc`.
///
/// On success control transfers to the target image (which should `exit`
/// with a sentinel); on failure the launcher exits `0xEE`, so the test can
/// distinguish "execveat worked" from "execveat returned an error".
///
/// `path_nul` must be NUL-terminated.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_execveat_test_elf(
    fexecve: bool,
    flags_extra: u32,
    argc: usize,
    path_nul: &[u8],
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // --- Assemble the code, recording the byte offsets of the 8-byte
    //     movabs immediates so they can be patched with absolute vaddrs
    //     once the data layout (which follows the code) is known. ---
    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    // Patch slots (index into `code` of the imm64 start), filled below.
    let path_imm: usize;
    let empty_imm: usize;
    let argv_imm: usize;
    let envp_imm: usize;

    // Helper: push `movabs <reg-prefix bytes>, imm64` with a zero
    // placeholder and return the imm's start offset.
    if fexecve {
        // movabs rdi, &path        (48 BF <8>)
        code.extend_from_slice(&[0x48, 0xBF]);
        path_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
        // xor esi, esi             (31 F6)  O_RDONLY
        code.extend_from_slice(&[0x31, 0xF6]);
        // xor edx, edx             (31 D2)  mode
        code.extend_from_slice(&[0x31, 0xD2]);
        // mov eax, 2               (B8 02 00 00 00)  SYS_open
        code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00]);
        // syscall                  (0F 05)
        code.extend_from_slice(&[0x0F, 0x05]);
        // mov rdi, rax             (48 89 C7)  dirfd = fd
        code.extend_from_slice(&[0x48, 0x89, 0xC7]);
        // movabs rsi, &empty       (48 BE <8>)
        code.extend_from_slice(&[0x48, 0xBE]);
        empty_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
        // movabs rdx, &argv        (48 BA <8>)
        code.extend_from_slice(&[0x48, 0xBA]);
        argv_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
        // movabs r10, &envp        (49 BA <8>)
        code.extend_from_slice(&[0x49, 0xBA]);
        envp_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
    } else {
        // mov rdi, -100            (48 C7 C7 9C FF FF FF)  AT_FDCWD
        code.extend_from_slice(&[0x48, 0xC7, 0xC7, 0x9C, 0xFF, 0xFF, 0xFF]);
        // movabs rsi, &path        (48 BE <8>)
        code.extend_from_slice(&[0x48, 0xBE]);
        path_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
        empty_imm = usize::MAX; // unused in the path form
        // movabs rdx, &argv        (48 BA <8>)
        code.extend_from_slice(&[0x48, 0xBA]);
        argv_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
        // movabs r10, &envp        (49 BA <8>)
        code.extend_from_slice(&[0x49, 0xBA]);
        envp_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
    }
    // Common tail (both forms):
    // mov r8d, <flags>             (41 B8 <imm32>)  — the fexecve form must
    // carry AT_EMPTY_PATH (0x1000); both forms OR in any caller-requested
    // extra flag bits (e.g. AT_SYMLINK_NOFOLLOW for the reject-symlink test).
    let final_flags = if fexecve { 0x1000u32 | flags_extra } else { flags_extra };
    code.extend_from_slice(&[0x41, 0xB8]);
    code.extend_from_slice(&final_flags.to_le_bytes());
    // mov eax, 322                 (B8 42 01 00 00)  SYS_execveat
    code.extend_from_slice(&[0xB8, 0x42, 0x01, 0x00, 0x00]);
    // syscall                      (0F 05)
    code.extend_from_slice(&[0x0F, 0x05]);
    // -- failure path (only reached if execveat returned) --
    // mov edi, 0xEE                (BF EE 00 00 00)
    code.extend_from_slice(&[0xBF, 0xEE, 0x00, 0x00, 0x00]);
    // mov eax, 60                  (B8 3C 00 00 00)  SYS_exit
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00]);
    // syscall                      (0F 05)
    code.extend_from_slice(&[0x0F, 0x05]);
    // int3                         (CC) — unreachable safety net
    code.push(0xCC);

    // --- Data layout (placed in the same PT_LOAD, after the code) ---
    // file offset f maps to vaddr load_vaddr + (f - code_offset).
    let code_len = code.len();
    let data_base = code_offset as usize + code_len; // file offset of data
    // path string
    let path_off = data_base;
    let path_end = path_off + path_nul.len();
    // empty string (single NUL) — always present; cheap and keeps offsets
    // uniform between the two forms.
    let empty_off = path_end;
    let after_empty = empty_off + 1;
    // 8-align the argv array.  argv holds `argc` pointers (all → the path
    // string, which is a valid NUL-terminated string the SysV stack builder
    // copies) plus a NULL terminator, so the exec'd image sees this argc.
    let argc = argc.max(1); // a real exec always has argv[0]
    let argv_off = (after_empty + 7) & !7usize;
    let envp_off = argv_off + (argc + 1) * 8; // argc ptrs + NULL
    let file_size = envp_off + 8; // envp = [NULL]

    let vaddr_of = |file_off: usize| -> u64 {
        load_vaddr + (file_off as u64 - code_offset)
    };
    let path_vaddr = vaddr_of(path_off);
    let empty_vaddr = vaddr_of(empty_off);
    let argv_vaddr = vaddr_of(argv_off);
    let envp_vaddr = vaddr_of(envp_off);

    // Patch the movabs immediates.
    code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes());
    if empty_imm != usize::MAX {
        code[empty_imm..empty_imm + 8].copy_from_slice(&empty_vaddr.to_le_bytes());
    }
    code[argv_imm..argv_imm + 8].copy_from_slice(&argv_vaddr.to_le_bytes());
    code[envp_imm..envp_imm + 8].copy_from_slice(&envp_vaddr.to_le_bytes());

    // --- Build the file image ---
    let seg_len = file_size - code_offset as usize; // bytes from code_offset
    let mut buf = vec![0u8; file_size];

    // ELF header
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // Program header: PT_LOAD R+W+X covering code + data (the launcher
    // never writes, but R+W keeps argv/envp on a writable page like a
    // real loader's data segment; X is needed for the code).
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_W | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // Code
    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    // path string
    buf[path_off..path_end].copy_from_slice(path_nul);
    // empty string is already a zero byte at empty_off.
    // argv = [path_vaddr; argc] followed by NULL.
    for i in 0..argc {
        write_u64(&mut buf, argv_off + i * 8, path_vaddr);
    }
    write_u64(&mut buf, argv_off + argc * 8, 0);
    // envp = [NULL]
    write_u64(&mut buf, envp_off, 0);

    buf
}

/// Build a **Linux-ABI** test ELF that exercises the file-backed `mmap(2)`
/// path end-to-end from ring 3.
///
/// This is the user-mode counterpart to the kernel-context
/// `self_test_file_mmap` (which drives `linux_file_mmap` directly): it
/// proves the *whole* real syscall path works for a Linux-ABI process —
/// `open(2)` installing a Linux fd, `mmap(2)` routing to `linux_file_mmap`
/// with a valid `caller_pid()`, the mapped pages being readable from ring 3,
/// and the file's bytes being delivered into the **second** 16 KiB frame
/// (so multi-frame file-backed mapping is verified through the real path,
/// not just the kernel-context helper).
///
/// The code performs:
///
/// ```text
///   lea  rdi, [rip + path]   ; rdi = absolute path string  (48 8D 3D ..)
///   xor  esi, esi            ; flags = O_RDONLY (0)         (31 F6)
///   xor  edx, edx            ; mode = 0                     (31 D2)
///   mov  eax, 2              ; Linux SYS_open               (B8 02 ..)
///   syscall                  ; rax = fd
///   mov  r8, rax             ; r8 = fd (mmap arg5)          (49 89 C0)
///   xor  edi, edi            ; addr = NULL                  (31 FF)
///   mov  esi, <len>          ; length                       (BE ..)
///   mov  edx, 1              ; prot = PROT_READ             (BA 01 ..)
///   mov  r10d, 2             ; flags = MAP_PRIVATE          (41 BA 02 ..)
///   mov  r9d, <offset>       ; mmap file offset             (41 B9 ..)
///   mov  eax, 9              ; Linux SYS_mmap               (B8 09 ..)
///   syscall                  ; rax = mapped base
///   movzx edi, byte [rax + <read_off>] ; rdi = mapped byte  (0F B6 B8 ..)
///   mov  eax, 60             ; Linux SYS_exit               (B8 3C ..)
///   syscall                  ; exit(byte)
///   int3                     ; unreachable trap
/// ```
///
/// `read_off` is chosen by the caller to land in the second frame, so the
/// resulting zombie's exit code equals the file byte at that offset — a
/// value the test seeds to a known sentinel.  If `mmap` fails, `rax` holds a
/// negative errno and the `movzx` dereferences a bad address (the process
/// faults rather than exiting with the sentinel), so the test still detects
/// the failure.
///
/// `map_offset` is the file offset passed to `mmap(2)` (must be frame-aligned
/// for the mapping to land where expected); the byte read at `read_off` is
/// relative to the returned mapping base, i.e. it reflects file byte
/// `map_offset + read_off`.
///
/// `path_nul` must be NUL-terminated.  `read_off` must be `<= u32::MAX`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub fn build_linux_mmap_test_elf(
    path_nul: &[u8],
    mmap_len: u32,
    read_off: u32,
    map_offset: u32,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Machine code, assembled below; the path string follows immediately.
    let code: [u8; 67] = [
        // lea rdi, [rip + disp32]  (disp filled in below at [3..7])
        0x48, 0x8D, 0x3D, 0x00, 0x00, 0x00, 0x00, // 0
        0x31, 0xF6, // xor esi, esi              (O_RDONLY)            7
        0x31, 0xD2, // xor edx, edx              (mode 0)             9
        0xB8, 0x02, 0x00, 0x00, 0x00, // mov eax, 2 (SYS_open)       11
        0x0F, 0x05, // syscall                                       16
        0x49, 0x89, 0xC0, // mov r8, rax         (fd -> arg5)        18
        0x31, 0xFF, // xor edi, edi              (addr = NULL)       21
        0xBE, 0x00, 0x00, 0x00, 0x00, // mov esi, imm32 (length)     23 (imm @24)
        0xBA, 0x01, 0x00, 0x00, 0x00, // mov edx, 1 (PROT_READ)      28
        0x41, 0xBA, 0x02, 0x00, 0x00, 0x00, // mov r10d, 2 (PRIVATE) 33
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, // mov r9d, imm32 (offset) 39 (imm @41)
        0xB8, 0x09, 0x00, 0x00, 0x00, // mov eax, 9 (SYS_mmap)       45
        0x0F, 0x05, // syscall                                       50
        0x0F, 0xB6, 0xB8, 0x00, 0x00, 0x00, 0x00, // movzx edi,[rax+disp32] 52 (disp @55)
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (SYS_exit)      59
        0x0F, 0x05, // syscall                                       64
        0xCC, // int3                                                66
    ];
    let code_len = code.len(); // 67

    let path_offset_in_seg = code_len; // path string starts after the code
    let seg_data_len = code_len + path_nul.len();
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU; // tag Linux/GNU so detect_linux_abi() is true

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering code + path) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Copy the code, then patch the two operands ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    // lea rdi, [rip + disp32]: instruction at seg-offset 0, length 7, so the
    // RIP used by the CPU is (load_vaddr + 7).  The path lives at
    // (load_vaddr + path_offset_in_seg), so disp = path_offset_in_seg - 7.
    let lea_disp = (path_offset_in_seg as i64) - 7;
    write_u32(&mut buf, cs + 3, lea_disp as u32);

    // mov esi, imm32 (mmap length) — imm at seg-offset 24.
    write_u32(&mut buf, cs + 24, mmap_len);

    // mov r9d, imm32 (mmap file offset) — imm at seg-offset 41.
    write_u32(&mut buf, cs + 41, map_offset);

    // movzx edi, byte [rax + disp32] (read offset) — disp at seg-offset 55.
    write_u32(&mut buf, cs + 55, read_off);

    // --- Path string immediately after the code ---
    let path_start = cs + path_offset_in_seg;
    buf[path_start..path_start + path_nul.len()].copy_from_slice(path_nul);

    buf
}

/// Build a Linux-ABI ET_EXEC that exercises the `brk(2)` heap end-to-end.
///
/// The program:
/// 1. `brk(0)` to query the initial program break (the heap floor); saves it.
/// 2. `brk(old + 0x8000)` to grow the heap by 32 KiB (two 16 KiB frames).
/// 3. Verifies the kernel returned the requested new break (proves the grow
///    succeeded — on failure Linux/our kernel returns the *unchanged* break).
/// 4. Writes `sentinel` into the **second** frame of the new heap
///    (`old + 0x4000`), proving demand-paging maps frames beyond the first.
/// 5. Reads the byte back and `exit(sentinel)`.
///
/// On any mismatch it exits `0xAA` so the test can distinguish a grow failure
/// from a read-back failure.  Tagged `ELFOSABI_GNU` so the loader sets up the
/// Linux `brk` region (`set_brk_region`); `brk` needs no capabilities.
pub fn build_linux_brk_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Offsets (within the segment) are noted so the
    // `jne` displacement can be computed.  SYS_brk=12, SYS_exit=60.
    let mut code: [u8; 72] = [
        0x31, 0xFF, // xor edi, edi               (brk arg = 0)          @0
        0xB8, 0x0C, 0x00, 0x00, 0x00, // mov eax, 12 (SYS_brk)          @2
        0x0F, 0x05, // syscall                                          @7
        0x48, 0x89, 0xC3, // mov rbx, rax         (save old break)      @9
        0x48, 0x8D, 0xB8, 0x00, 0x80, 0x00, 0x00, // lea rdi,[rax+0x8000] @12
        0x48, 0x89, 0xFD, // mov rbp, rdi         (save desired break)  @19
        0xB8, 0x0C, 0x00, 0x00, 0x00, // mov eax, 12 (SYS_brk)          @22
        0x0F, 0x05, // syscall                                          @27
        0x48, 0x39, 0xE8, // cmp rax, rbp         (granted == desired?) @29
        0x0F, 0x85, 0x15, 0x00, 0x00, 0x00, // jne fail (disp=21)       @32
        0xC6, 0x83, 0x00, 0x40, 0x00, 0x00, 0x00, // mov byte[rbx+0x4000],sentinel @38 (imm @44)
        0x0F, 0xB6, 0xBB, 0x00, 0x40, 0x00, 0x00, // movzx edi,byte[rbx+0x4000]    @45
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (SYS_exit)         @52
        0x0F, 0x05, // syscall                                          @57
        // fail:                                                        @59
        0xBF, 0xAA, 0x00, 0x00, 0x00, // mov edi, 0xAA  (mismatch code) @59
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (SYS_exit)         @64
        0x0F, 0x05, // syscall                                          @69
        0xCC, // int3 (unreachable safety net)                         @71
    ];
    // Patch the sentinel into the `mov byte [rbx+0x4000], imm8` immediate.
    code[44] = sentinel;
    let code_len = code.len(); // 72

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU; // tag Linux/GNU so detect_linux_abi() is true

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that validates the full
/// **SA_RESTART transparent-restart** path end-to-end in ring 3.
///
/// The payload is entirely self-contained (no kernel↔child fd sharing
/// required), proving the slow-object interruptibility fix
/// (`read` on an empty pipe → `ERESTARTSYS` → handler runs → transparent
/// restart returns the handler-written byte):
///
/// 1. `pipe(fds)` creates a fresh pipe.  Since the child's stdio occupies
///    fds 0/1/2, the read end is fd 3 and the write end is fd 4
///    (deterministic), so they're hardcoded.
/// 2. `rt_sigaction(SIGUSR1, &act, NULL, 8)` installs a handler with
///    `SA_RESTART | SA_RESTORER`; `sa_restorer` points at an embedded
///    `rt_sigreturn` trampoline.
/// 3. `read(3, buf, 1)` blocks on the empty pipe.
/// 4. The orchestrator posts `SIGUSR1`.  The blocked read is interrupted,
///    returns `ERESTARTSYS`, the kernel builds the signal frame and runs
///    the handler.
/// 5. The handler does `write(4, &sentinel, 1)` — depositing one byte into
///    the pipe — then `ret`s into the restorer, which issues
///    `rt_sigreturn`.
/// 6. Because `SA_RESTART` was set, the kernel transparently restarts the
///    `read`, which now finds the handler-written byte and returns it.
/// 7. `exit(buf[0])` — so a correct SA_RESTART path yields exit code
///    `sentinel`.  A *broken* path would instead surface `EINTR` from the
///    read (buf untouched), yielding a different exit code, or hang.
///
/// Used by [`crate::proc::spawn::self_test_linux_sa_restart`].
#[must_use]
pub fn build_linux_sa_restart_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Linux ABI syscall numbers: pipe=22,
    // rt_sigaction=13, read=0, exit=60, write=1, rt_sigreturn=15.
    // SIGUSR1=10.  SA_RESTART|SA_RESTORER = 0x14000000.
    //
    // Stack frame (after `sub rsp, 64`):
    //   [rsp+0]  pipe fd array (rfd@+0, wfd@+4)   — fds become 3/4
    //   [rsp+8]  read buffer (1 byte)
    //   [rsp+16] struct kernel_sigaction (32 bytes):
    //            +16 sa_handler, +24 sa_flags, +32 sa_restorer, +40 sa_mask
    let mut code: [u8; 156] = [
        // _start:
        0x48, 0x83, 0xEC, 0x40, //             sub rsp, 64                 @0
        0x48, 0x89, 0xE7, //                   mov rdi, rsp  (fd array)    @4
        0xB8, 0x16, 0x00, 0x00, 0x00, //       mov eax, 22   (SYS_pipe)    @7
        0x0F, 0x05, //                         syscall                     @12
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, // mov rax, handler_addr       @14 (imm@16)
        0x48, 0x89, 0x44, 0x24, 0x10, //       mov [rsp+16], rax           @24
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x14, // mov qword [rsp+24],0x14000000 @29
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, // mov rax, restorer_addr      @38 (imm@40)
        0x48, 0x89, 0x44, 0x24, 0x20, //       mov [rsp+32], rax           @48
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, // mov qword [rsp+40],0 (mask)   @53
        0xBF, 0x0A, 0x00, 0x00, 0x00, //       mov edi, 10   (SIGUSR1)     @62
        0x48, 0x8D, 0x74, 0x24, 0x10, //       lea rsi, [rsp+16] (&act)    @67
        0x31, 0xD2, //                         xor edx, edx  (oact=NULL)   @72
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, // mov r10d, 8   (sigsetsize)  @74
        0xB8, 0x0D, 0x00, 0x00, 0x00, //       mov eax, 13   (rt_sigaction)@80
        0x0F, 0x05, //                         syscall                     @85
        0xBF, 0x03, 0x00, 0x00, 0x00, //       mov edi, 3    (rfd)         @87
        0x48, 0x8D, 0x74, 0x24, 0x08, //       lea rsi, [rsp+8] (buf)      @92
        0xBA, 0x01, 0x00, 0x00, 0x00, //       mov edx, 1    (count)       @97
        0x31, 0xC0, //                         xor eax, eax  (SYS_read)    @102
        0x0F, 0x05, //                         syscall  (blocks)           @104
        0x0F, 0xB6, 0x7C, 0x24, 0x08, //       movzx edi, byte [rsp+8]     @106
        0xB8, 0x3C, 0x00, 0x00, 0x00, //       mov eax, 60   (SYS_exit)    @111
        0x0F, 0x05, //                         syscall                     @116
        0xCC, //                               int3 (unreachable)          @118
        // handler:                                                        @119
        0xBF, 0x04, 0x00, 0x00, 0x00, //       mov edi, 4    (wfd)         @119
        0x48, 0xBE, 0, 0, 0, 0, 0, 0, 0, 0, // mov rsi, sentinel_addr      @124 (imm@126)
        0xBA, 0x01, 0x00, 0x00, 0x00, //       mov edx, 1    (count)       @134
        0xB8, 0x01, 0x00, 0x00, 0x00, //       mov eax, 1    (SYS_write)   @139
        0x0F, 0x05, //                         syscall                     @144
        0xC3, //                               ret -> restorer (pretcode)  @146
        // restorer:                                                       @147
        0xB8, 0x0F, 0x00, 0x00, 0x00, //       mov eax, 15   (rt_sigreturn)@147
        0x0F, 0x05, //                         syscall                     @152
        0xCC, //                               int3 (unreachable)          @154
        // sentinel:                                                       @155
        0x00, //                               <sentinel byte>             @155
    ];

    // Patch absolute addresses (segment is mapped at a fixed vaddr).
    let handler_addr = load_vaddr.wrapping_add(119);
    let restorer_addr = load_vaddr.wrapping_add(147);
    let sentinel_addr = load_vaddr.wrapping_add(155);
    code[16..24].copy_from_slice(&handler_addr.to_le_bytes());
    code[40..48].copy_from_slice(&restorer_addr.to_le_bytes());
    code[126..134].copy_from_slice(&sentinel_addr.to_le_bytes());
    code[155] = sentinel;
    let code_len = code.len(); // 156

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU; // tag Linux/GNU so detect_linux_abi() is true

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that validates a **blocking
/// `signalfd` read is interruptible by a signal that is NOT in the fd's
/// acceptance mask** — the signalfd analogue of the slow-object
/// interruptibility fixes.
///
/// The payload:
/// 1. Installs a `SIGUSR1` handler with `SA_RESTORER` but **without**
///    `SA_RESTART` (so an interrupted slow syscall surfaces `EINTR` rather
///    than transparently restarting).  The handler body is a bare `ret`
///    (its only job is to *exist*, so `SIGUSR1` is deliverable and the read
///    is interrupted rather than the process terminated).
/// 2. Creates a `signalfd` watching only `SIGUSR2` (mask bit 11).
/// 3. Blocks in `read(sfd, buf, 128)` on that signalfd.
/// 4. The orchestrator posts **`SIGUSR1`** — which is *not* in the signalfd
///    mask.  A correct kernel wakes the blocked read, the handler runs, and
///    the read returns `-EINTR`.
/// 5. `exit(sentinel)` if the read returned a negative value (the expected
///    `-EINTR`); `exit(0xEE)` if it unexpectedly returned a record.
///
/// This *distinguishes* the fix from the bug: before the fix the signalfd
/// read registered a waiter only for *watched* signals, so `SIGUSR1` never
/// woke it — the thread parked forever (the handler, which only runs at the
/// syscall-return checkpoint, could never fire), so the child would never
/// become a zombie and the orchestrator's state check would fail.  Used by
/// [`crate::proc::spawn::self_test_linux_signalfd_interrupt`].
#[must_use]
pub fn build_linux_signalfd_interrupt_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Linux ABI numbers: rt_sigaction=13,
    // signalfd4=289, read=0, exit=60, rt_sigreturn=15.  SIGUSR1=10,
    // SIGUSR2=12 (mask bit 1<<11 = 0x800).  sa_flags = SA_RESTORER only
    // (0x04000000) — deliberately NO SA_RESTART so the interrupted read
    // yields EINTR.
    //
    // Stack frame (after `sub rsp, 256`):
    //   [rsp+16] struct kernel_sigaction (32 bytes)
    //   [rsp+48] signalfd sigset_t mask (8 bytes) = 0x800 (SIGUSR2)
    //   [rsp+64] signalfd read buffer (128 bytes)
    let mut code: [u8; 168] = [
        // _start:
        0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00, // sub rsp, 256             @0
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, handler_addr    @7 (imm@9)
        0x48, 0x89, 0x44, 0x24, 0x10, //             mov [rsp+16], rax        @17
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, // mov qword [rsp+40],0 (mask)  @46
        0xBF, 0x0A, 0x00, 0x00, 0x00, //             mov edi, 10  (SIGUSR1)   @55
        0x48, 0x8D, 0x74, 0x24, 0x10, //             lea rsi, [rsp+16] (&act) @60
        0x31, 0xD2, //                               xor edx, edx (oact=NULL) @65
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, //       mov r10d, 8  (sigsetsz)  @67
        0xB8, 0x0D, 0x00, 0x00, 0x00, //             mov eax, 13 (rt_sigaction)@73
        0x0F, 0x05, //                               syscall                  @78
        0x48, 0xC7, 0x44, 0x24, 0x30, 0x00, 0x08, 0x00, 0x00, // mov qword [rsp+48],0x800 (SIGUSR2) @80
        0xBF, 0xFF, 0xFF, 0xFF, 0xFF, //             mov edi, -1  (create)    @89
        0x48, 0x8D, 0x74, 0x24, 0x30, //             lea rsi, [rsp+48] (&mask)@94
        0xBA, 0x08, 0x00, 0x00, 0x00, //             mov edx, 8   (sizemask)  @99
        0x45, 0x31, 0xD2, //                         xor r10d, r10d (flags=0) @104
        0xB8, 0x21, 0x01, 0x00, 0x00, //             mov eax, 289 (signalfd4) @107
        0x0F, 0x05, //                               syscall                  @112
        0x48, 0x89, 0xC3, //                         mov rbx, rax (sfd)       @114
        0x48, 0x89, 0xDF, //                         mov rdi, rbx (fd)        @117
        0x48, 0x8D, 0x74, 0x24, 0x40, //             lea rsi, [rsp+64] (buf)  @120
        0xBA, 0x80, 0x00, 0x00, 0x00, //             mov edx, 128 (count)     @125
        0x31, 0xC0, //                               xor eax, eax (SYS_read)  @130
        0x0F, 0x05, //                               syscall  (blocks)        @132
        0x48, 0x85, 0xC0, //                         test rax, rax            @134
        0x78, 0x07, //                               js ok (+7 -> @146)       @137
        0xBF, 0xEE, 0x00, 0x00, 0x00, //             mov edi, 0xEE (unexpected)@139
        0xEB, 0x05, //                               jmp exit_syscall (+5)    @144
        // ok:                                                                @146
        0xBF, 0x00, 0x00, 0x00, 0x00, //             mov edi, sentinel        @146 (imm@147)
        // exit_syscall:                                                      @151
        0xB8, 0x3C, 0x00, 0x00, 0x00, //             mov eax, 60 (SYS_exit)   @151
        0x0F, 0x05, //                               syscall                  @156
        0xCC, //                                     int3 (unreachable)       @158
        // handler:                                                           @159
        0xC3, //                                     ret -> restorer (pretcode)@159
        // restorer:                                                          @160
        0xB8, 0x0F, 0x00, 0x00, 0x00, //             mov eax, 15 (rt_sigreturn)@160
        0x0F, 0x05, //                               syscall                  @165
        0xCC, //                                     int3 (unreachable)       @167
    ];

    // Patch absolute addresses + the sentinel exit-code immediate.
    let handler_addr = load_vaddr.wrapping_add(159);
    let restorer_addr = load_vaddr.wrapping_add(160);
    code[9..17].copy_from_slice(&handler_addr.to_le_bytes());
    code[33..41].copy_from_slice(&restorer_addr.to_le_bytes());
    code[147] = sentinel; // low byte of `mov edi, sentinel` imm32
    let code_len = code.len(); // 168

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that validates a **blocking
/// `eventfd` read is interruptible by a deliverable signal** — the eventfd
/// analogue of the slow-object interruptibility fixes (pipe / stream socket
/// / signalfd).
///
/// The payload:
/// 1. Installs a `SIGUSR1` handler with `SA_RESTORER` but **without**
///    `SA_RESTART` (so an interrupted slow syscall surfaces `EINTR` rather
///    than transparently restarting).  The handler body is a bare `ret`.
/// 2. Creates an `eventfd2(0, 0)` (initial counter 0) — fd 3.
/// 3. Blocks in `read(efd, buf, 8)` on the zero counter.
/// 4. The orchestrator posts `SIGUSR1`.  A correct kernel wakes the blocked
///    read, the handler runs, and the read returns `-EINTR`.
/// 5. `exit(sentinel)` if the read returned a negative value (the expected
///    `-EINTR`); `exit(0xEE)` if it unexpectedly returned a counter value.
///
/// This *distinguishes* the fix from the bug: before the fix the eventfd
/// read parked with a bare `block_current()` and a single-slot waiter that
/// only writers woke, so `SIGUSR1` never woke it — the thread parked forever
/// (the handler, which only runs at the syscall-return checkpoint, could
/// never fire), so the child would never become a zombie and the
/// orchestrator's state check would fail.  Used by
/// [`crate::proc::spawn::self_test_linux_eventfd_interrupt`].
#[must_use]
pub fn build_linux_eventfd_interrupt_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Linux ABI numbers: rt_sigaction=13,
    // eventfd2=290 (0x122), read=0, exit=60, rt_sigreturn=15.  SIGUSR1=10.
    // sa_flags = SA_RESTORER only (0x04000000) — deliberately NO SA_RESTART
    // so the interrupted read yields EINTR.
    //
    // Stack frame (after `sub rsp, 256`):
    //   [rsp+16] struct kernel_sigaction (32 bytes)
    //   [rsp+64] eventfd read buffer (8 bytes)
    let mut code: [u8; 145] = [
        // _start:
        0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00, // sub rsp, 256             @0
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, handler_addr    @7 (imm@9)
        0x48, 0x89, 0x44, 0x24, 0x10, //             mov [rsp+16], rax        @17
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, // mov qword [rsp+40],0 (mask)  @46
        0xBF, 0x0A, 0x00, 0x00, 0x00, //             mov edi, 10  (SIGUSR1)   @55
        0x48, 0x8D, 0x74, 0x24, 0x10, //             lea rsi, [rsp+16] (&act) @60
        0x31, 0xD2, //                               xor edx, edx (oact=NULL) @65
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, //       mov r10d, 8  (sigsetsz)  @67
        0xB8, 0x0D, 0x00, 0x00, 0x00, //             mov eax, 13 (rt_sigaction)@73
        0x0F, 0x05, //                               syscall                  @78
        // eventfd2(0, 0):
        0x31, 0xFF, //                               xor edi, edi (initval=0) @80
        0x31, 0xF6, //                               xor esi, esi (flags=0)   @82
        0xB8, 0x22, 0x01, 0x00, 0x00, //             mov eax, 290 (eventfd2)  @84
        0x0F, 0x05, //                               syscall                  @89
        0x48, 0x89, 0xC3, //                         mov rbx, rax (efd)       @91
        // read(efd, buf, 8):
        0x48, 0x89, 0xDF, //                         mov rdi, rbx (fd)        @94
        0x48, 0x8D, 0x74, 0x24, 0x40, //             lea rsi, [rsp+64] (buf)  @97
        0xBA, 0x08, 0x00, 0x00, 0x00, //             mov edx, 8   (count)     @102
        0x31, 0xC0, //                               xor eax, eax (SYS_read)  @107
        0x0F, 0x05, //                               syscall  (blocks)        @109
        0x48, 0x85, 0xC0, //                         test rax, rax            @111
        0x78, 0x07, //                               js ok (+7 -> @123)       @114
        0xBF, 0xEE, 0x00, 0x00, 0x00, //             mov edi, 0xEE (unexpected)@116
        0xEB, 0x05, //                               jmp exit_syscall (+5)    @121
        // ok:                                                                @123
        0xBF, 0x00, 0x00, 0x00, 0x00, //             mov edi, sentinel        @123 (imm@124)
        // exit_syscall:                                                      @128
        0xB8, 0x3C, 0x00, 0x00, 0x00, //             mov eax, 60 (SYS_exit)   @128
        0x0F, 0x05, //                               syscall                  @133
        0xCC, //                                     int3 (unreachable)       @135
        // handler:                                                           @136
        0xC3, //                                     ret -> restorer (pretcode)@136
        // restorer:                                                          @137
        0xB8, 0x0F, 0x00, 0x00, 0x00, //             mov eax, 15 (rt_sigreturn)@137
        0x0F, 0x05, //                               syscall                  @142
        0xCC, //                                     int3 (unreachable)       @144
    ];

    // Patch absolute addresses + the sentinel exit-code immediate.
    let handler_addr = load_vaddr.wrapping_add(136);
    let restorer_addr = load_vaddr.wrapping_add(137);
    code[9..17].copy_from_slice(&handler_addr.to_le_bytes());
    code[33..41].copy_from_slice(&restorer_addr.to_le_bytes());
    code[124] = sentinel; // low byte of `mov edi, sentinel` imm32
    let code_len = code.len(); // 145

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that validates a **blocking
/// `timerfd` read is interruptible by a deliverable signal** — the timerfd
/// analogue of the slow-object interruptibility fixes.
///
/// The payload:
/// 1. Installs a `SIGUSR1` handler with `SA_RESTORER` but **without**
///    `SA_RESTART` (so an interrupted slow syscall surfaces `EINTR`).  The
///    handler body is a bare `ret`.
/// 2. Creates a `timerfd_create(CLOCK_MONOTONIC, 0)` — fd 3 — and *never arms
///    it* (no `timerfd_settime`).  A read of a disarmed timerfd blocks
///    indefinitely (until armed or interrupted), which is the cleanest
///    indefinite-block case to interrupt.
/// 3. Blocks in `read(tfd, buf, 8)`.
/// 4. The orchestrator posts `SIGUSR1`.  A correct kernel wakes the blocked
///    read, the handler runs, and the read returns `-EINTR`.
/// 5. `exit(sentinel)` if the read returned a negative value (the expected
///    `-EINTR`); `exit(0xEE)` if it unexpectedly returned a count.
///
/// This *distinguishes* the fix from the bug: before the fix the timerfd read
/// parked with a bare `block_current()` and a single-slot reader waiter that
/// only `settime`/the expiry hrtimer woke, so `SIGUSR1` never woke it — the
/// thread parked forever (the handler runs only at the syscall-return
/// checkpoint, which a parked read never reaches), so the child never becomes
/// a zombie and the orchestrator's state check fails.  Used by
/// [`crate::proc::spawn::self_test_linux_timerfd_interrupt`].
#[must_use]
pub fn build_linux_timerfd_interrupt_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Linux ABI numbers: rt_sigaction=13,
    // timerfd_create=283 (0x11B), read=0, exit=60, rt_sigreturn=15.
    // SIGUSR1=10.  CLOCK_MONOTONIC=1.  sa_flags = SA_RESTORER only
    // (0x04000000) — deliberately NO SA_RESTART so the interrupted read
    // yields EINTR.
    //
    // Stack frame (after `sub rsp, 256`):
    //   [rsp+16] struct kernel_sigaction (32 bytes)
    //   [rsp+64] timerfd read buffer (8 bytes)
    let mut code: [u8; 148] = [
        // _start:
        0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00, // sub rsp, 256             @0
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, handler_addr    @7 (imm@9)
        0x48, 0x89, 0x44, 0x24, 0x10, //             mov [rsp+16], rax        @17
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, // mov qword [rsp+40],0 (mask)  @46
        0xBF, 0x0A, 0x00, 0x00, 0x00, //             mov edi, 10  (SIGUSR1)   @55
        0x48, 0x8D, 0x74, 0x24, 0x10, //             lea rsi, [rsp+16] (&act) @60
        0x31, 0xD2, //                               xor edx, edx (oact=NULL) @65
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, //       mov r10d, 8  (sigsetsz)  @67
        0xB8, 0x0D, 0x00, 0x00, 0x00, //             mov eax, 13 (rt_sigaction)@73
        0x0F, 0x05, //                               syscall                  @78
        // timerfd_create(CLOCK_MONOTONIC, 0):
        0xBF, 0x01, 0x00, 0x00, 0x00, //             mov edi, 1  (CLOCK_MONO) @80
        0x31, 0xF6, //                               xor esi, esi (flags=0)   @85
        0xB8, 0x1B, 0x01, 0x00, 0x00, //             mov eax, 283(timerfd_cr) @87
        0x0F, 0x05, //                               syscall                  @92
        0x48, 0x89, 0xC3, //                         mov rbx, rax (tfd)       @94
        // read(tfd, buf, 8) — disarmed timer ⇒ blocks indefinitely:
        0x48, 0x89, 0xDF, //                         mov rdi, rbx (fd)        @97
        0x48, 0x8D, 0x74, 0x24, 0x40, //             lea rsi, [rsp+64] (buf)  @100
        0xBA, 0x08, 0x00, 0x00, 0x00, //             mov edx, 8   (count)     @105
        0x31, 0xC0, //                               xor eax, eax (SYS_read)  @110
        0x0F, 0x05, //                               syscall  (blocks)        @112
        0x48, 0x85, 0xC0, //                         test rax, rax            @114
        0x78, 0x07, //                               js ok (+7 -> @126)       @117
        0xBF, 0xEE, 0x00, 0x00, 0x00, //             mov edi, 0xEE (unexpected)@119
        0xEB, 0x05, //                               jmp exit_syscall (+5)    @124
        // ok:                                                                @126
        0xBF, 0x00, 0x00, 0x00, 0x00, //             mov edi, sentinel        @126 (imm@127)
        // exit_syscall:                                                      @131
        0xB8, 0x3C, 0x00, 0x00, 0x00, //             mov eax, 60 (SYS_exit)   @131
        0x0F, 0x05, //                               syscall                  @136
        0xCC, //                                     int3 (unreachable)       @138
        // handler:                                                           @139
        0xC3, //                                     ret -> restorer (pretcode)@139
        // restorer:                                                          @140
        0xB8, 0x0F, 0x00, 0x00, 0x00, //             mov eax, 15 (rt_sigreturn)@140
        0x0F, 0x05, //                               syscall                  @145
        0xCC, //                                     int3 (unreachable)       @147
    ];

    // Patch absolute addresses + the sentinel exit-code immediate.
    let handler_addr = load_vaddr.wrapping_add(139);
    let restorer_addr = load_vaddr.wrapping_add(140);
    code[9..17].copy_from_slice(&handler_addr.to_le_bytes());
    code[33..41].copy_from_slice(&restorer_addr.to_le_bytes());
    code[127] = sentinel; // low byte of `mov edi, sentinel` imm32
    let code_len = code.len(); // 148

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that validates a **blocking
/// `inotify` read is interruptible by a deliverable signal** — the inotify
/// analogue of the slow-object interruptibility fixes.
///
/// The payload:
/// 1. Installs a `SIGUSR1` handler with `SA_RESTORER` but **without**
///    `SA_RESTART` (so an interrupted slow syscall surfaces `EINTR`).  The
///    handler body is a bare `ret`.
/// 2. Creates an `inotify_init1(0)` instance — fd 3 — with **no watches**.  A
///    read of an inotify fd with no queued events blocks indefinitely
///    regardless of watches, the cleanest indefinite-block case to interrupt.
/// 3. Blocks in `read(ifd, buf, 16)`.
/// 4. The orchestrator posts `SIGUSR1`.  A correct kernel wakes the blocked
///    read, the handler runs, and the read returns `-EINTR`.
/// 5. `exit(sentinel)` if the read returned a negative value (the expected
///    `-EINTR`); `exit(0xEE)` if it unexpectedly returned data.
///
/// This *distinguishes* the fix from the bug: before the fix the inotify read
/// registered only a notify-waiter and parked with a bare `block_current()`,
/// so `SIGUSR1` never woke it — the thread parked forever (the handler runs
/// only at the syscall-return checkpoint, which a parked read never reaches),
/// so the child never becomes a zombie and the orchestrator's state check
/// fails.  Used by [`crate::proc::spawn::self_test_linux_inotify_interrupt`].
#[must_use]
pub fn build_linux_inotify_interrupt_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Linux ABI numbers: rt_sigaction=13,
    // inotify_init1=294 (0x126), read=0, exit=60, rt_sigreturn=15.
    // SIGUSR1=10.  sa_flags = SA_RESTORER only (0x04000000) — deliberately NO
    // SA_RESTART so the interrupted read yields EINTR.
    //
    // Stack frame (after `sub rsp, 256`):
    //   [rsp+16] struct kernel_sigaction (32 bytes)
    //   [rsp+64] inotify read buffer (16 bytes)
    let mut code: [u8; 143] = [
        // _start:
        0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00, // sub rsp, 256             @0
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, handler_addr    @7 (imm@9)
        0x48, 0x89, 0x44, 0x24, 0x10, //             mov [rsp+16], rax        @17
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, // mov qword [rsp+40],0 (mask)  @46
        0xBF, 0x0A, 0x00, 0x00, 0x00, //             mov edi, 10  (SIGUSR1)   @55
        0x48, 0x8D, 0x74, 0x24, 0x10, //             lea rsi, [rsp+16] (&act) @60
        0x31, 0xD2, //                               xor edx, edx (oact=NULL) @65
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, //       mov r10d, 8  (sigsetsz)  @67
        0xB8, 0x0D, 0x00, 0x00, 0x00, //             mov eax, 13 (rt_sigaction)@73
        0x0F, 0x05, //                               syscall                  @78
        // inotify_init1(0):
        0x31, 0xFF, //                               xor edi, edi (flags=0)   @80
        0xB8, 0x26, 0x01, 0x00, 0x00, //             mov eax, 294(inotify_in1)@82
        0x0F, 0x05, //                               syscall                  @87
        0x48, 0x89, 0xC3, //                         mov rbx, rax (ifd)       @89
        // read(ifd, buf, 16) — no events queued ⇒ blocks indefinitely:
        0x48, 0x89, 0xDF, //                         mov rdi, rbx (fd)        @92
        0x48, 0x8D, 0x74, 0x24, 0x40, //             lea rsi, [rsp+64] (buf)  @95
        0xBA, 0x10, 0x00, 0x00, 0x00, //             mov edx, 16  (count)     @100
        0x31, 0xC0, //                               xor eax, eax (SYS_read)  @105
        0x0F, 0x05, //                               syscall  (blocks)        @107
        0x48, 0x85, 0xC0, //                         test rax, rax            @109
        0x78, 0x07, //                               js ok (+7 -> @121)       @112
        0xBF, 0xEE, 0x00, 0x00, 0x00, //             mov edi, 0xEE (unexpected)@114
        0xEB, 0x05, //                               jmp exit_syscall (+5)    @119
        // ok:                                                                @121
        0xBF, 0x00, 0x00, 0x00, 0x00, //             mov edi, sentinel        @121 (imm@122)
        // exit_syscall:                                                      @126
        0xB8, 0x3C, 0x00, 0x00, 0x00, //             mov eax, 60 (SYS_exit)   @126
        0x0F, 0x05, //                               syscall                  @131
        0xCC, //                                     int3 (unreachable)       @133
        // handler:                                                           @134
        0xC3, //                                     ret -> restorer (pretcode)@134
        // restorer:                                                          @135
        0xB8, 0x0F, 0x00, 0x00, 0x00, //             mov eax, 15 (rt_sigreturn)@135
        0x0F, 0x05, //                               syscall                  @140
        0xCC, //                                     int3 (unreachable)       @142
    ];

    // Patch absolute addresses + the sentinel exit-code immediate.
    let handler_addr = load_vaddr.wrapping_add(134);
    let restorer_addr = load_vaddr.wrapping_add(135);
    code[9..17].copy_from_slice(&handler_addr.to_le_bytes());
    code[33..41].copy_from_slice(&restorer_addr.to_le_bytes());
    code[122] = sentinel; // low byte of `mov edi, sentinel` imm32
    let code_len = code.len(); // 143

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF that validates a **blocking
/// `poll()` is interruptible by a deliverable signal**, returning `-EINTR`.
///
/// Per the Linux SA_RESTART taxonomy, `poll`/`select`/`epoll_wait` are
/// **always** interrupted by `-EINTR` and never restarted (even under
/// `SA_RESTART`).  This test exercises the `poll` path.
///
/// The payload:
/// 1. Installs a `SIGUSR1` handler (`SA_RESTORER`, no `SA_RESTART` — though the
///    flag is irrelevant for poll, which never restarts).  The handler is a
///    bare `ret`.
/// 2. Creates an `eventfd2(0, 0)` (counter 0 ⇒ never `POLLIN`-ready) — fd 3.
/// 3. Blocks in `poll(&pollfd{fd, POLLIN}, 1, -1)` (wait forever).
/// 4. The orchestrator posts `SIGUSR1`.  A correct kernel breaks the re-poll
///    wait, the handler runs, and `poll` returns `-EINTR`.
/// 5. `exit(sentinel)` if `poll` returned negative (`-EINTR`); `exit(0xEE)` if
///    it unexpectedly returned `>= 0`.
///
/// This *distinguishes* the fix from the bug: before the fix `poll_core`
/// busy-polled in 10 ms slices and never checked for a pending signal, so the
/// handler (which runs only at the syscall-return checkpoint) never fired — the
/// child spun forever and never became a zombie.  Used by
/// [`crate::proc::spawn::self_test_linux_poll_interrupt`].
#[must_use]
pub fn build_linux_poll_interrupt_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Linux ABI numbers: rt_sigaction=13,
    // eventfd2=290 (0x122), poll=7, exit=60, rt_sigreturn=15.  SIGUSR1=10.
    // POLLIN=0x0001.  poll(fds, nfds, timeout): rdi/rsi/rdx; timeout=-1.
    //
    // Stack frame (after `sub rsp, 256`):
    //   [rsp+16] struct kernel_sigaction (32 bytes)
    //   [rsp+64] struct pollfd { fd(4), events(2), revents(2) } (8 bytes)
    let mut code: [u8; 165] = [
        // _start:
        0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00, // sub rsp, 256             @0
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, handler_addr    @7 (imm@9)
        0x48, 0x89, 0x44, 0x24, 0x10, //             mov [rsp+16], rax        @17
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, // mov qword [rsp+40],0 (mask)  @46
        0xBF, 0x0A, 0x00, 0x00, 0x00, //             mov edi, 10  (SIGUSR1)   @55
        0x48, 0x8D, 0x74, 0x24, 0x10, //             lea rsi, [rsp+16] (&act) @60
        0x31, 0xD2, //                               xor edx, edx (oact=NULL) @65
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, //       mov r10d, 8  (sigsetsz)  @67
        0xB8, 0x0D, 0x00, 0x00, 0x00, //             mov eax, 13 (rt_sigaction)@73
        0x0F, 0x05, //                               syscall                  @78
        // eventfd2(0, 0):
        0x31, 0xFF, //                               xor edi, edi (initval=0) @80
        0x31, 0xF6, //                               xor esi, esi (flags=0)   @82
        0xB8, 0x22, 0x01, 0x00, 0x00, //             mov eax, 290 (eventfd2)  @84
        0x0F, 0x05, //                               syscall                  @89
        // build struct pollfd at [rsp+64]:
        0x89, 0x44, 0x24, 0x40, //                   mov [rsp+64], eax (fd)   @91
        0x66, 0xC7, 0x44, 0x24, 0x44, 0x01, 0x00, // mov word [rsp+68],1(IN)  @95
        0x66, 0xC7, 0x44, 0x24, 0x46, 0x00, 0x00, // mov word [rsp+70],0(rev) @102
        // poll(&pollfd, 1, -1):
        0x48, 0x8D, 0x7C, 0x24, 0x40, //             lea rdi, [rsp+64] (fds)  @109
        0xBE, 0x01, 0x00, 0x00, 0x00, //             mov esi, 1   (nfds)      @114
        0xBA, 0xFF, 0xFF, 0xFF, 0xFF, //             mov edx, -1  (timeout)   @119
        0xB8, 0x07, 0x00, 0x00, 0x00, //             mov eax, 7   (poll)      @124
        0x0F, 0x05, //                               syscall  (blocks)        @129
        0x48, 0x85, 0xC0, //                         test rax, rax            @131
        0x78, 0x07, //                               js ok (+7 -> @143)       @134
        0xBF, 0xEE, 0x00, 0x00, 0x00, //             mov edi, 0xEE (unexpected)@136
        0xEB, 0x05, //                               jmp exit_syscall (+5)    @141
        // ok:                                                                @143
        0xBF, 0x00, 0x00, 0x00, 0x00, //             mov edi, sentinel        @143 (imm@144)
        // exit_syscall:                                                      @148
        0xB8, 0x3C, 0x00, 0x00, 0x00, //             mov eax, 60 (SYS_exit)   @148
        0x0F, 0x05, //                               syscall                  @153
        0xCC, //                                     int3 (unreachable)       @155
        // handler:                                                           @156
        0xC3, //                                     ret -> restorer (pretcode)@156
        // restorer:                                                          @157
        0xB8, 0x0F, 0x00, 0x00, 0x00, //             mov eax, 15 (rt_sigreturn)@157
        0x0F, 0x05, //                               syscall                  @162
        0xCC, //                                     int3 (unreachable)       @164
    ];

    // Patch absolute addresses + the sentinel exit-code immediate.
    let handler_addr = load_vaddr.wrapping_add(156);
    let restorer_addr = load_vaddr.wrapping_add(157);
    code[9..17].copy_from_slice(&handler_addr.to_le_bytes());
    code[33..41].copy_from_slice(&restorer_addr.to_le_bytes());
    code[144] = sentinel; // low byte of `mov edi, sentinel` imm32
    let code_len = code.len(); // 165

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a ring-3 ELF that exercises the **`poll(NULL, 0, -1)` empty-set,
/// infinite-timeout** path: it must *block* until a signal, then return
/// `-EINTR` — not return `0` immediately.
///
/// The child installs a SIGUSR1 handler, then calls `poll(NULL, 0, -1)` (no
/// fds, wait forever).  With no fds to watch, only a delivered signal can end
/// the wait.  A correct kernel blocks, is interrupted by SIGUSR1, and `poll`
/// returns `-EINTR`; the child exits with `sentinel`.  The pre-fix bug
/// returned `ok(0)` immediately (the `nfds == 0` quick path only slept for a
/// positive timeout), so `poll` returned `0` *before* the signal was posted
/// and the child exited `0xEE` (or never blocked at all).  Used by
/// [`crate::proc::spawn::self_test_linux_poll_empty_infinite`].
#[must_use]
pub fn build_linux_poll_empty_infinite_test_elf(sentinel: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Hand-assembled x86_64.  Linux ABI: rt_sigaction=13, poll=7, exit=60,
    // rt_sigreturn=15.  SIGUSR1=10.  poll(fds, nfds, timeout): rdi/rsi/rdx.
    //
    // Stack frame (after `sub rsp, 256`):
    //   [rsp+16] struct kernel_sigaction (32 bytes)
    let mut code: [u8; 130] = [
        // _start:
        0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00, // sub rsp, 256             @0
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, handler_addr    @7 (imm@9)
        0x48, 0x89, 0x44, 0x24, 0x10, //             mov [rsp+16], rax        @17
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, // mov qword [rsp+40],0 (mask)  @46
        0xBF, 0x0A, 0x00, 0x00, 0x00, //             mov edi, 10  (SIGUSR1)   @55
        0x48, 0x8D, 0x74, 0x24, 0x10, //             lea rsi, [rsp+16] (&act) @60
        0x31, 0xD2, //                               xor edx, edx (oact=NULL) @65
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, //       mov r10d, 8  (sigsetsz)  @67
        0xB8, 0x0D, 0x00, 0x00, 0x00, //             mov eax, 13 (rt_sigaction)@73
        0x0F, 0x05, //                               syscall                  @78
        // poll(NULL, 0, -1):
        0x31, 0xFF, //                               xor edi, edi (fds=NULL)  @80
        0x31, 0xF6, //                               xor esi, esi (nfds=0)    @82
        0xBA, 0xFF, 0xFF, 0xFF, 0xFF, //             mov edx, -1  (timeout)   @84
        0xB8, 0x07, 0x00, 0x00, 0x00, //             mov eax, 7   (poll)      @89
        0x0F, 0x05, //                               syscall  (blocks)        @94
        0x48, 0x85, 0xC0, //                         test rax, rax            @96
        0x78, 0x07, //                               js ok (+7 -> @108)       @99
        0xBF, 0xEE, 0x00, 0x00, 0x00, //             mov edi, 0xEE (returned 0)@101
        0xEB, 0x05, //                               jmp exit_syscall (+5)    @106
        // ok:                                                                @108
        0xBF, 0x00, 0x00, 0x00, 0x00, //             mov edi, sentinel        @108 (imm@109)
        // exit_syscall:                                                      @113
        0xB8, 0x3C, 0x00, 0x00, 0x00, //             mov eax, 60 (SYS_exit)   @113
        0x0F, 0x05, //                               syscall                  @118
        0xCC, //                                     int3 (unreachable)       @120
        // handler:                                                           @121
        0xC3, //                                     ret -> restorer          @121
        // restorer:                                                          @122
        0xB8, 0x0F, 0x00, 0x00, 0x00, //             mov eax, 15 (rt_sigreturn)@122
        0x0F, 0x05, //                               syscall                  @127
        0xCC, //                                     int3 (unreachable)       @129
    ];

    // Patch absolute addresses + the sentinel exit-code immediate.
    let handler_addr = load_vaddr.wrapping_add(121);
    let restorer_addr = load_vaddr.wrapping_add(122);
    code[9..17].copy_from_slice(&handler_addr.to_le_bytes());
    code[33..41].copy_from_slice(&restorer_addr.to_le_bytes());
    code[109] = sentinel; // low byte of `mov edi, sentinel` imm32
    let code_len = code.len(); // 130

    let seg_data_len = code_len;
    let file_size = code_offset as usize + seg_data_len;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;

    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD: R+X covering the code) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_data_len as u64);
    write_u64(&mut buf, ph + 40, seg_data_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    // --- Code ---
    let cs = code_offset as usize;
    buf[cs..cs + code_len].copy_from_slice(&code);

    buf
}

/// Build a "Hello from userspace!" ELF that calls SYS_CONSOLE_WRITE
/// then SYS_EXIT(0).
///
/// The ELF contains:
/// - x86_64 code that uses LEA to compute the address of the embedded
///   string, then issues `syscall` with rax=100 (SYS_CONSOLE_WRITE).
/// - A second `syscall` with rax=1 (SYS_EXIT), rdi=0.
///
/// This proves the full userspace → kernel syscall → console output path.
pub fn build_hello_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    // Layout:
    // - 64-byte ELF header
    // - 56-byte program header (one PT_LOAD)
    // - Code + string data
    //
    // The string is embedded after the code instructions, in the same
    // PT_LOAD segment so it's mapped alongside the code.

    let msg = b"Hello from userspace!\n";
    let msg_len = msg.len(); // 22 bytes

    // We'll assemble x86_64 machine code manually:
    //
    //   ; rax = SYS_CONSOLE_WRITE (100)
    //   mov eax, 100              ; B8 64 00 00 00
    //   ; rdi = pointer to string (computed via RIP-relative LEA)
    //   lea rdi, [rip + offset]   ; 48 8D 3D xx xx xx xx
    //   ; rsi = string length
    //   mov esi, <msg_len>        ; BE xx 00 00 00
    //   syscall                   ; 0F 05
    //   ; rax = SYS_EXIT (1)
    //   mov eax, 1                ; B8 01 00 00 00
    //   ; rdi = exit code 0
    //   xor edi, edi              ; 31 FF
    //   syscall                   ; 0F 05
    //   int3                      ; CC (safety)
    //   ; <string data follows here>
    //
    // Encoding sizes:
    //   mov eax, 100:    5 bytes (offset 0)
    //   lea rdi, [rip+]: 7 bytes (offset 5)
    //   mov esi, len:    5 bytes (offset 12)
    //   syscall:         2 bytes (offset 17)
    //   mov eax, 1:      5 bytes (offset 19)
    //   xor edi, edi:    2 bytes (offset 24)
    //   syscall:         2 bytes (offset 26)
    //   int3:            1 byte  (offset 28)
    //   string:          starts at offset 29

    let code_instructions_len: usize = 29;
    let total_code_data = code_instructions_len + msg_len;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 + 56
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let file_size = code_offset as usize + total_code_data;
    let mut buf = vec![0u8; file_size];

    // --- ELF header ---
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
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0); // e_shnum
    write_u16(&mut buf, 62, 0); // e_shstrndx

    // --- Program header ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, total_code_data as u64); // p_filesz
    write_u64(&mut buf, ph + 40, total_code_data as u64); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    // --- Code ---
    let cs = code_offset as usize;

    // mov eax, 100 (SYS_CONSOLE_WRITE)
    buf[cs] = 0xB8;
    buf[cs + 1] = 100;
    buf[cs + 2] = 0x00;
    buf[cs + 3] = 0x00;
    buf[cs + 4] = 0x00;

    // lea rdi, [rip + offset_to_string]
    // At this instruction, RIP points to the NEXT instruction (cs+12).
    // The string starts at cs+29.  So offset = 29 - 12 = 17.
    let rip_after_lea = 12; // offset within code segment after LEA
    let string_offset_in_code = code_instructions_len;
    #[allow(clippy::arithmetic_side_effects)]
    let rip_rel = (string_offset_in_code - rip_after_lea) as i32;
    buf[cs + 5] = 0x48; // REX.W
    buf[cs + 6] = 0x8D; // LEA
    buf[cs + 7] = 0x3D; // ModRM: rdi, [rip+disp32]
    let rel_bytes = rip_rel.to_le_bytes();
    buf[cs + 8] = rel_bytes[0];
    buf[cs + 9] = rel_bytes[1];
    buf[cs + 10] = rel_bytes[2];
    buf[cs + 11] = rel_bytes[3];

    // mov esi, msg_len
    buf[cs + 12] = 0xBE;
    #[allow(clippy::cast_possible_truncation)]
    let len_bytes = (msg_len as u32).to_le_bytes();
    buf[cs + 13] = len_bytes[0];
    buf[cs + 14] = len_bytes[1];
    buf[cs + 15] = len_bytes[2];
    buf[cs + 16] = len_bytes[3];

    // syscall
    buf[cs + 17] = 0x0F;
    buf[cs + 18] = 0x05;

    // mov eax, 1 (SYS_EXIT)
    buf[cs + 19] = 0xB8;
    buf[cs + 20] = 0x01;
    buf[cs + 21] = 0x00;
    buf[cs + 22] = 0x00;
    buf[cs + 23] = 0x00;

    // xor edi, edi (exit code 0)
    buf[cs + 24] = 0x31;
    buf[cs + 25] = 0xFF;

    // syscall
    buf[cs + 26] = 0x0F;
    buf[cs + 27] = 0x05;

    // int3 (safety net)
    buf[cs + 28] = 0xCC;

    // --- String data ---
    buf[cs + code_instructions_len..cs + code_instructions_len + msg_len]
        .copy_from_slice(msg);

    buf
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

/// Build a test ELF for SEH: exception handler catches fault and exits.
///
/// The ELF contains two code regions:
///
/// **Main code** (entry point, offset 0):
/// ```x86asm
///   mov eax, 504                ; SYS_SET_EXCEPTION_HANDLER
///   movabs rdi, handler_addr    ; handler at +64 bytes into code
///   syscall                     ; register the handler
///   xor eax, eax                ; rax = 0
///   mov [rax], eax              ; write to address 0 → #PF
///   int3                        ; unreachable (handler runs instead)
/// ```
///
/// **Exception handler** (offset 64):
/// ```x86asm
///   mov eax, 1                  ; SYS_EXIT
///   xor edi, edi                ; exit code 0
///   syscall                     ; exit cleanly
///   int3
/// ```
///
/// If SEH dispatch works, the process exits cleanly via the handler
/// instead of being killed by the kernel.
pub fn build_seh_exit_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 128; // Room for main code + handler.
    let load_vaddr: u64 = 0x0000_0040_0000_0000;
    let handler_offset: u64 = 64; // Handler at +64 bytes within code.
    #[allow(clippy::arithmetic_side_effects)]
    let handler_vaddr: u64 = load_vaddr + handler_offset;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
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
    write_u64(&mut buf, 24, load_vaddr); // Entry point = main code.
    write_u64(&mut buf, 32, phdr_offset);
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD, R+X) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // Fill with INT3 safety net.
    let c = code_offset as usize;
    for byte in &mut buf[c..(c + code_size as usize)] {
        *byte = 0xCC;
    }

    // --- Main code at offset 0 ---

    // mov eax, 504 (0x1F8)  →  B8 F8 01 00 00
    buf[c] = 0xB8;
    buf[c + 1] = 0xF8;
    buf[c + 2] = 0x01;
    buf[c + 3] = 0x00;
    buf[c + 4] = 0x00;

    // movabs rdi, handler_vaddr  →  48 BF <8 bytes LE>
    buf[c + 5] = 0x48;
    buf[c + 6] = 0xBF;
    buf[c + 7..c + 15].copy_from_slice(&handler_vaddr.to_le_bytes());

    // syscall  →  0F 05
    buf[c + 15] = 0x0F;
    buf[c + 16] = 0x05;

    // xor eax, eax  →  31 C0
    buf[c + 17] = 0x31;
    buf[c + 18] = 0xC0;

    // mov [rax], eax  →  89 00  (write to address 0 → #PF)
    buf[c + 19] = 0x89;
    buf[c + 20] = 0x00;

    // int3 at c+21 (already filled by safety net).

    // --- Exception handler at offset 64 ---
    let h = c + handler_offset as usize;

    // mov eax, 1 (SYS_EXIT)  →  B8 01 00 00 00
    buf[h] = 0xB8;
    buf[h + 1] = 0x01;
    buf[h + 2] = 0x00;
    buf[h + 3] = 0x00;
    buf[h + 4] = 0x00;

    // xor edi, edi  →  31 FF
    buf[h + 5] = 0x31;
    buf[h + 6] = 0xFF;

    // syscall  →  0F 05
    buf[h + 7] = 0x0F;
    buf[h + 8] = 0x05;

    // int3 at h+9 (already filled).

    buf
}

/// Build a test ELF for full SEH round-trip: handler resumes execution.
///
/// The ELF tests the full exception → handler → resume path:
///
/// **Main code** (entry point, offset 0):
/// ```x86asm
///   mov eax, 504                ; SYS_SET_EXCEPTION_HANDLER
///   movabs rdi, handler_addr    ; handler at +64 bytes into code
///   syscall                     ; register the handler
///   ud2                         ; triggers #UD (2 bytes)
///   mov eax, 1                  ; SYS_EXIT ← resume point
///   xor edi, edi                ; exit code 0
///   syscall                     ; exit cleanly
///   int3                        ; unreachable
/// ```
///
/// **Exception handler** (offset 64):
/// ```x86asm
///   add qword [rdi+16], 2      ; ctx->rip += 2 (skip ud2)
///   mov eax, 505                ; SYS_EXCEPTION_RETURN
///   syscall                     ; resume at modified RIP
///   int3                        ; unreachable
/// ```
///
/// The handler receives a pointer to [`ExceptionContext`] in RDI.
/// `ExceptionContext.rip` is at byte offset 16 (after `code: u64` and
/// `aux: u64`).  The handler adds 2 to skip past the 2-byte `ud2`,
/// then calls `SYS_EXCEPTION_RETURN` which restores the CPU state
/// and resumes execution at the `mov eax, 1` instruction.
pub fn build_seh_resume_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size: u64 = 128;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;
    let handler_offset: u64 = 64;
    #[allow(clippy::arithmetic_side_effects)]
    let handler_vaddr: u64 = load_vaddr + handler_offset;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
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

    // --- Program header (PT_LOAD, R+X) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, code_size);
    write_u64(&mut buf, ph + 40, code_size);
    write_u64(&mut buf, ph + 48, 0x1000);

    // Fill with INT3 safety net.
    let c = code_offset as usize;
    for byte in &mut buf[c..(c + code_size as usize)] {
        *byte = 0xCC;
    }

    // --- Main code at offset 0 ---

    // mov eax, 504 (0x1F8)  →  B8 F8 01 00 00
    buf[c] = 0xB8;
    buf[c + 1] = 0xF8;
    buf[c + 2] = 0x01;
    buf[c + 3] = 0x00;
    buf[c + 4] = 0x00;

    // movabs rdi, handler_vaddr  →  48 BF <8 bytes LE>
    buf[c + 5] = 0x48;
    buf[c + 6] = 0xBF;
    buf[c + 7..c + 15].copy_from_slice(&handler_vaddr.to_le_bytes());

    // syscall  →  0F 05
    buf[c + 15] = 0x0F;
    buf[c + 16] = 0x05;

    // ud2  →  0F 0B  (triggers #UD; handler will skip these 2 bytes)
    buf[c + 17] = 0x0F;
    buf[c + 18] = 0x0B;

    // --- Resume point after handler (entry + 19) ---

    // mov eax, 1 (SYS_EXIT)  →  B8 01 00 00 00
    buf[c + 19] = 0xB8;
    buf[c + 20] = 0x01;
    buf[c + 21] = 0x00;
    buf[c + 22] = 0x00;
    buf[c + 23] = 0x00;

    // xor edi, edi  →  31 FF
    buf[c + 24] = 0x31;
    buf[c + 25] = 0xFF;

    // syscall  →  0F 05
    buf[c + 26] = 0x0F;
    buf[c + 27] = 0x05;

    // int3 at c+28 (already filled).

    // --- Exception handler at offset 64 ---
    let h = c + handler_offset as usize;

    // add qword [rdi+16], 2  →  48 83 47 10 02
    // (ExceptionContext.rip is at offset 16: code(u64) + aux(u64) = 16 bytes)
    buf[h] = 0x48;
    buf[h + 1] = 0x83;
    buf[h + 2] = 0x47;
    buf[h + 3] = 0x10;
    buf[h + 4] = 0x02;

    // mov eax, 505 (0x1F9) (SYS_EXCEPTION_RETURN)  →  B8 F9 01 00 00
    buf[h + 5] = 0xB8;
    buf[h + 6] = 0xF9;
    buf[h + 7] = 0x01;
    buf[h + 8] = 0x00;
    buf[h + 9] = 0x00;

    // syscall  →  0F 05
    buf[h + 10] = 0x0F;
    buf[h + 11] = 0x05;

    // int3 at h+12 (already filled).

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
    test_zero_phentsize()?;
    test_detect_linux_abi_sysv_is_native()?;
    test_detect_linux_abi_osabi_gnu()?;
    test_detect_linux_abi_interp_glibc()?;
    test_detect_linux_abi_interp_musl()?;
    test_detect_linux_abi_interp_unrelated()?;
    test_detect_linux_abi_gnu_property()?;
    test_is_linux_interp_helper()?;
    test_interp_path_dynamic()?;
    test_interp_path_static()?;
    test_interp_path_empty()?;

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

/// Test 10: Reject e_phentsize == 0 when program headers exist.
///
/// A zero e_phentsize with e_phnum > 0 would cause all program headers
/// to be read from the same offset, producing silently wrong results.
fn test_zero_phentsize() -> KernelResult<()> {
    let mut data = build_test_elf();
    // Set e_phentsize to 0 (offset 54 in ELF header).
    write_u16(&mut data, 54, 0);

    match ElfFile::parse(&data) {
        Err(KernelError::InvalidExecutable) => {}
        other => {
            serial_println!(
                "[elf]   FAIL: zero e_phentsize should fail: {:?}",
                other.map(|_| ()),
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[elf]   Reject zero e_phentsize: OK");
    Ok(())
}

// ---------------------------------------------------------------------------
// Linux ABI detection self-tests
// ---------------------------------------------------------------------------

/// Build an ELF whose only program header is `PT_INTERP` containing
/// `interp_path` (NUL-terminated).  Used by the detection tests.
///
/// The header is structured so that `ElfFile::parse` accepts it:
/// - Valid ELF64 magic / class / data / version / machine / type.
/// - One program header (e_phnum = 1).
/// - The PT_INTERP segment points at a region of the buffer that
///   contains the NUL-terminated path.
///
/// `osabi` is written into `e_ident[EI_OSABI]`.  Pass `ELFOSABI_SYSV`
/// for "no OSABI hint" or `ELFOSABI_GNU` to explicitly tag as Linux.
/// Build a dynamically-linked Linux test ELF whose `PT_INTERP` segment
/// names `interp_path` (which must be NUL-terminated).  Used by the
/// spawn interpreter-loading self-tests to exercise the ld.so path.
pub fn build_dynamic_interp_test_elf(interp_path: &[u8]) -> alloc::vec::Vec<u8> {
    // ELFOSABI_SYSV (0): interp_path() keys off PT_INTERP, not the OSABI.
    build_interp_elf(0, interp_path)
}

fn build_interp_elf(osabi: u8, interp_path: &[u8]) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let interp_offset: u64 = 64 + ELF64_PHDR_SIZE as u64; // 120
    // The PT_INTERP image is just the NUL-terminated path.
    let interp_size: u64 = interp_path.len() as u64;
    let total: u64 = interp_offset + interp_size;

    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; total as usize];

    // ELF header.
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = osabi;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry — must be non-zero for ET_EXEC.
    write_u64(&mut buf, 32, phdr_offset);
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0); // e_shnum
    write_u16(&mut buf, 62, 0); // e_shstrndx

    // Program header: PT_INTERP.
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_INTERP);
    write_u32(&mut buf, ph + 4, PF_R);
    write_u64(&mut buf, ph + 8, interp_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr — arbitrary, not loaded.
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, interp_size); // p_filesz
    write_u64(&mut buf, ph + 40, interp_size); // p_memsz
    write_u64(&mut buf, ph + 48, 1); // p_align

    // INTERP image data — the path bytes (caller supplies NUL terminator).
    let interp_start = interp_offset as usize;
    buf[interp_start..interp_start + interp_path.len()].copy_from_slice(interp_path);

    buf
}

/// Machine code for `exit(exit_code)` via the Linux x86_64 `syscall`
/// convention: `mov edi, exit_code; mov eax, 60 (SYS_exit); syscall`.
///
/// Returned as a fixed 12-byte array so callers can `copy_from_slice` it
/// into a segment without per-byte indexing.
#[must_use]
fn linux_exit_machine_code(exit_code: u8) -> [u8; 12] {
    [
        0xBF, exit_code, 0x00, 0x00, 0x00, // mov edi, exit_code
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (Linux SYS_exit)
        0x0F, 0x05, // syscall
    ]
}

/// Build a minimal Linux-ABI program **interpreter** ("ld.so" stand-in)
/// that simply calls `exit(exit_code)`.
///
/// This is an `ET_DYN` (PIE) image with a single `PT_LOAD` at `p_vaddr = 0`
/// and `e_entry = 0`, so the kernel's `load_interpreter` maps it at
/// `LINUX_INTERP_BASE` and enters it at `base + 0` — exactly the path a real
/// `ld.so` takes.  Tagged `ELFOSABI_GNU`.
///
/// Used by the dynamic-launch end-to-end self-test: if the kernel correctly
/// loads and enters the interpreter (rather than the executable's own
/// entry), the process exits with `exit_code`, proving the whole
/// dynamically-linked launch path executes.
// Test fixture: the buffer is sized to fit exactly and every offset is a
// compile-time constant, so the indexing is provably in-bounds and the
// offset arithmetic cannot overflow.  Matches the surrounding `build_*` ELF
// fixture builders.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
#[must_use]
pub fn build_linux_interp_exit_elf(exit_code: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = phdr_offset + ELF64_PHDR_SIZE as u64; // 120
    let code = linux_exit_machine_code(exit_code);
    let code_size = code.len() as u64;
    // ET_DYN / PIE: p_vaddr = 0, e_entry = 0 (entry at segment start).
    let load_vaddr: u64 = 0;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_DYN);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry = 0 (allowed for ET_DYN)
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header (PT_LOAD, R+X) ---
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr = 0
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, code_size); // p_filesz
    write_u64(&mut buf, ph + 40, code_size); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    // --- Code ---
    let cs = code_offset as usize;
    if let Some(dst) = buf.get_mut(cs..cs + code.len()) {
        dst.copy_from_slice(&code);
    }

    buf
}

/// Build a dynamically-linked Linux-ABI **executable** that names
/// `interp_path` (which must be NUL-terminated) in a `PT_INTERP` segment.
///
/// The executable also carries its own `PT_LOAD` code that would
/// `exit(exit_code)` — but that code must **not** run: when an interpreter
/// is present the kernel enters the interpreter's entry instead.  The
/// self-test gives the executable and interpreter distinct exit codes so
/// the observed exit code proves which one actually executed.
///
/// `ET_EXEC`, tagged `ELFOSABI_GNU`.
// Test fixture: the buffer is sized to fit exactly (header + 2 phdrs +
// interp path + code) and every offset is derived from compile-time
// constants plus the caller-supplied path length, so the indexing is
// provably in-bounds and the offset arithmetic cannot overflow for any
// realistic interpreter path.  Matches the surrounding `build_*` builders.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
#[must_use]
pub fn build_linux_dynamic_exe_elf(interp_path: &[u8], exit_code: u8) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    // Two program headers: PT_INTERP then PT_LOAD.
    let interp_offset: u64 = phdr_offset + 2 * ELF64_PHDR_SIZE as u64; // 176
    let interp_size: u64 = interp_path.len() as u64;
    let code_offset: u64 = interp_offset + interp_size;
    let code = linux_exit_machine_code(exit_code);
    let code_size = code.len() as u64;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; (code_offset + code_size) as usize];

    // --- ELF header ---
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_GNU;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry = code segment start
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 2); // e_phnum (PT_INTERP + PT_LOAD)
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // --- Program header 0: PT_INTERP ---
    let ph0 = phdr_offset as usize;
    write_u32(&mut buf, ph0, PT_INTERP);
    write_u32(&mut buf, ph0 + 4, PF_R);
    write_u64(&mut buf, ph0 + 8, interp_offset); // p_offset
    write_u64(&mut buf, ph0 + 16, load_vaddr); // p_vaddr (arbitrary, not loaded)
    write_u64(&mut buf, ph0 + 24, 0);
    write_u64(&mut buf, ph0 + 32, interp_size); // p_filesz
    write_u64(&mut buf, ph0 + 40, interp_size); // p_memsz
    write_u64(&mut buf, ph0 + 48, 1); // p_align

    // --- Program header 1: PT_LOAD (R+X) for the executable's own code ---
    let ph1 = phdr_offset as usize + ELF64_PHDR_SIZE;
    write_u32(&mut buf, ph1, PT_LOAD);
    write_u32(&mut buf, ph1 + 4, PF_R | PF_X);
    write_u64(&mut buf, ph1 + 8, code_offset); // p_offset
    write_u64(&mut buf, ph1 + 16, load_vaddr); // p_vaddr
    write_u64(&mut buf, ph1 + 24, 0);
    write_u64(&mut buf, ph1 + 32, code_size); // p_filesz
    write_u64(&mut buf, ph1 + 40, code_size); // p_memsz
    write_u64(&mut buf, ph1 + 48, 0x1000); // p_align

    // --- PT_INTERP path bytes (caller supplies the NUL terminator) ---
    let is = interp_offset as usize;
    if let Some(dst) = buf.get_mut(is..is + interp_path.len()) {
        dst.copy_from_slice(interp_path);
    }

    // --- Executable's own code (should be shadowed by the interpreter) ---
    let cs = code_offset as usize;
    if let Some(dst) = buf.get_mut(cs..cs + code.len()) {
        dst.copy_from_slice(&code);
    }

    buf
}

/// Build a tiny ELF with a single `PT_GNU_PROPERTY` program header and
/// `EI_OSABI = ELFOSABI_SYSV` (no other Linux markers).  Used to verify
/// that the property-segment signal alone trips detection.
fn build_gnu_property_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let prop_offset: u64 = 64 + ELF64_PHDR_SIZE as u64;
    // A minimal but non-zero PT_GNU_PROPERTY body — the detector only
    // inspects p_type, so any byte content suffices.
    let prop_size: u64 = 16;
    let total: u64 = prop_offset + prop_size;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut buf = vec![0u8; total as usize];

    // ELF header.
    buf[0] = 0x7F;
    buf[1] = b'E';
    buf[2] = b'L';
    buf[3] = b'F';
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[EI_VERSION] = EV_CURRENT;
    buf[EI_OSABI] = ELFOSABI_SYSV;
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset);
    write_u64(&mut buf, 40, 0);
    write_u32(&mut buf, 48, 0);
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, 1);
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0);
    write_u16(&mut buf, 62, 0);

    // Program header: PT_GNU_PROPERTY.
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_GNU_PROPERTY);
    write_u32(&mut buf, ph + 4, PF_R);
    write_u64(&mut buf, ph + 8, prop_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, prop_size);
    write_u64(&mut buf, ph + 40, prop_size);
    write_u64(&mut buf, ph + 48, 8);

    buf
}

/// Test 11: Default `build_test_elf` (`ELFOSABI_SYSV`, no PT_INTERP,
/// no PT_GNU_PROPERTY) is NOT detected as Linux — must be Native.
fn test_detect_linux_abi_sysv_is_native() -> KernelResult<()> {
    let data = build_test_elf();
    let elf = ElfFile::parse(&data)?;
    // Sanity: the default test ELF should have e_ident_osabi == SYSV (0).
    if elf.header.e_ident_osabi != ELFOSABI_SYSV {
        serial_println!(
            "[elf]   FAIL: default test ELF should have OSABI=SYSV, got {}",
            elf.header.e_ident_osabi,
        );
        return Err(KernelError::InternalError);
    }
    if elf.detect_linux_abi() {
        serial_println!(
            "[elf]   FAIL: default SYSV/PT_LOAD-only ELF should NOT be detected as Linux",
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   Detect Linux ABI: default SYSV is Native: OK");
    Ok(())
}

/// Test 12: `EI_OSABI = ELFOSABI_GNU` alone makes the ELF Linux.
fn test_detect_linux_abi_osabi_gnu() -> KernelResult<()> {
    // Take the default test ELF and just flip the OSABI byte.
    let mut data = build_test_elf();
    data[EI_OSABI] = ELFOSABI_GNU;
    let elf = ElfFile::parse(&data)?;
    if !elf.detect_linux_abi() {
        serial_println!(
            "[elf]   FAIL: ELFOSABI_GNU should be detected as Linux",
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   Detect Linux ABI: ELFOSABI_GNU: OK");
    Ok(())
}

/// Test 13: PT_INTERP pointing at `/lib64/ld-linux-x86-64.so.2` (glibc)
/// trips detection even with `EI_OSABI = ELFOSABI_SYSV`.
fn test_detect_linux_abi_interp_glibc() -> KernelResult<()> {
    let data = build_interp_elf(ELFOSABI_SYSV, b"/lib64/ld-linux-x86-64.so.2\0");
    let elf = ElfFile::parse(&data)?;
    if !elf.detect_linux_abi() {
        serial_println!(
            "[elf]   FAIL: PT_INTERP=/lib64/ld-linux-x86-64.so.2 should detect as Linux",
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   Detect Linux ABI: glibc PT_INTERP: OK");
    Ok(())
}

/// Test 14: PT_INTERP pointing at a musl loader trips detection.
fn test_detect_linux_abi_interp_musl() -> KernelResult<()> {
    let data = build_interp_elf(ELFOSABI_SYSV, b"/lib/ld-musl-x86_64.so.1\0");
    let elf = ElfFile::parse(&data)?;
    if !elf.detect_linux_abi() {
        serial_println!(
            "[elf]   FAIL: PT_INTERP=/lib/ld-musl-x86_64.so.1 should detect as Linux",
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   Detect Linux ABI: musl PT_INTERP: OK");
    Ok(())
}

/// Test 15: PT_INTERP pointing at an unrelated path (e.g. a custom
/// loader) does NOT trip detection.  Guards against
/// "any-PT_INTERP-means-Linux" false positives.
fn test_detect_linux_abi_interp_unrelated() -> KernelResult<()> {
    let data = build_interp_elf(ELFOSABI_SYSV, b"/system/loader\0");
    let elf = ElfFile::parse(&data)?;
    if elf.detect_linux_abi() {
        serial_println!(
            "[elf]   FAIL: PT_INTERP=/system/loader should NOT detect as Linux",
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   Detect Linux ABI: unrelated PT_INTERP stays Native: OK");
    Ok(())
}

/// Test 16: PT_GNU_PROPERTY presence alone trips detection.
fn test_detect_linux_abi_gnu_property() -> KernelResult<()> {
    let data = build_gnu_property_elf();
    let elf = ElfFile::parse(&data)?;
    if !elf.detect_linux_abi() {
        serial_println!(
            "[elf]   FAIL: PT_GNU_PROPERTY should detect as Linux",
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   Detect Linux ABI: PT_GNU_PROPERTY: OK");
    Ok(())
}

/// Test 17: `is_linux_interp` helper — direct unit test of the
/// substring matcher.  Catches regressions in NUL handling and the
/// glibc/musl substring choices.
fn test_is_linux_interp_helper() -> KernelResult<()> {
    // Positive cases.
    let positives: &[&[u8]] = &[
        b"/lib64/ld-linux-x86-64.so.2\0",
        b"/lib/ld-linux-x86-64.so.2\0",
        b"/lib/ld-musl-x86_64.so.1\0",
        b"/usr/lib/ld-linux-x86-64.so.2\0",
        // Even without trailing NUL.
        b"/lib64/ld-linux-x86-64.so.2",
    ];
    for case in positives {
        if !is_linux_interp(case) {
            serial_println!(
                "[elf]   FAIL: is_linux_interp should accept {:?}",
                core::str::from_utf8(case).unwrap_or("<non-utf8>"),
            );
            return Err(KernelError::InternalError);
        }
    }
    // Negative cases.
    let negatives: &[&[u8]] = &[
        b"\0",
        b"",
        b"/system/loader\0",
        b"/lib/ld-elf.so.1\0",     // FreeBSD's loader.
        b"/libexec/ld.so\0",       // OpenBSD's loader.
        // Substring after the NUL terminator must NOT count.
        b"/system/loader\0/lib64/ld-linux-x86-64.so.2",
    ];
    for case in negatives {
        if is_linux_interp(case) {
            serial_println!(
                "[elf]   FAIL: is_linux_interp should reject {:?}",
                core::str::from_utf8(case).unwrap_or("<non-utf8>"),
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[elf]   is_linux_interp helper: OK");
    Ok(())
}

/// Test: `interp_path` extracts and NUL-trims the `PT_INTERP` path of a
/// dynamically-linked binary.
fn test_interp_path_dynamic() -> KernelResult<()> {
    // build_interp_elf writes the path image verbatim (including the
    // trailing NUL the caller supplies); interp_path must trim at it.
    let data = build_interp_elf(ELFOSABI_SYSV, b"/lib64/ld-linux-x86-64.so.2\0");
    let elf = ElfFile::parse(&data)?;
    match elf.interp_path() {
        Some(path) if path == b"/lib64/ld-linux-x86-64.so.2" => {}
        other => {
            serial_println!(
                "[elf]   FAIL: interp_path dynamic mismatch: {:?}",
                other.map(core::str::from_utf8),
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[elf]   interp_path dynamic: OK");
    Ok(())
}

/// Test: `interp_path` returns `None` for a static binary (no
/// `PT_INTERP` — `build_test_elf` is a single `PT_LOAD`).
fn test_interp_path_static() -> KernelResult<()> {
    let data = build_test_elf();
    let elf = ElfFile::parse(&data)?;
    if elf.interp_path().is_some() {
        serial_println!("[elf]   FAIL: interp_path should be None for static ELF");
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   interp_path static: OK");
    Ok(())
}

/// Test: `interp_path` rejects an empty (leading-NUL) `PT_INTERP` image.
fn test_interp_path_empty() -> KernelResult<()> {
    let data = build_interp_elf(ELFOSABI_SYSV, b"\0");
    let elf = ElfFile::parse(&data)?;
    if elf.interp_path().is_some() {
        serial_println!("[elf]   FAIL: interp_path should be None for empty PT_INTERP");
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   interp_path empty: OK");
    Ok(())
}
