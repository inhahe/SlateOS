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
use crate::mm::page_table::{self, PageFlags, USER_SPACE_END, VirtAddr};
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
#[allow(dead_code)]
const EI_ABIVERSION: usize = 8;

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

/// SlateOS's own `EI_OSABI` value: 255, the top of the range the gABI leaves
/// to the architecture/OS (64–255). One of the two forms of the explicit
/// native marker (design-decisions.md §33); `userspace/readelf` already names
/// it `ELFOSABI_SLATEOS`.
const ELFOSABI_SLATEOS: u8 = 255;

/// Owner name of the SlateOS ABI note, NUL-terminated as ELF note names are.
/// The other form of the native marker: a note in a `PT_NOTE` segment.
const SLATEOS_NOTE_NAME: &[u8] = b"SlateOS\0";

/// `n_type` of the note that declares "this binary speaks the SlateOS native
/// system-call ABI". Its 4-byte descriptor is the ABI revision, currently 1;
/// the kernel accepts any revision for now, and the field exists so a future
/// incompatible ABI can be told apart without a new note type.
const NT_SLATEOS_ABI: u32 = 1;

// e_type values.
const ET_EXEC: u16 = 2; // Executable file.
const ET_DYN: u16 = 3; // Shared object / PIE.

// e_machine values.
const EM_X86_64: u16 = 62;

// Program header p_type values.
#[allow(dead_code)]
const PT_NULL: u32 = 0;
const PT_LOAD: u32 = 1;
#[allow(dead_code)]
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
#[allow(dead_code)]
const PT_PHDR: u32 = 6;
#[allow(dead_code)]
const PT_TLS: u32 = 7;
#[allow(dead_code)]
const PT_GNU_EH_FRAME: u32 = 0x6474_E550;
#[allow(dead_code)]
const PT_GNU_STACK: u32 = 0x6474_E551;
#[allow(dead_code)]
const PT_GNU_RELRO: u32 = 0x6474_E552;
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
    /// The program-header table as the file stores it: `e_phnum` entries of
    /// `e_phentsize` bytes at `e_phoff`. `None` if it runs past the file.
    /// What a copy of the table is made from when no loaded segment holds it
    /// (`spawn::place_phdr_table`).
    #[must_use]
    pub fn phdr_table_bytes(&self) -> Option<&'a [u8]> {
        let start = usize::try_from(self.header.e_phoff).ok()?;
        let len =
            usize::from(self.header.e_phentsize).checked_mul(usize::from(self.header.e_phnum))?;
        self.data.get(start..start.checked_add(len)?)
    }

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
        if header.e_phnum > 0 && (header.e_phentsize as usize) < ELF64_PHDR_SIZE {
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
            .checked_add(index.checked_mul(self.header.e_phentsize as usize)?)?;

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
    ///
    /// ## The SlateOS native marker outranks every signal above
    ///
    /// A binary carrying [`Self::has_slateos_marker`] is native, whatever else
    /// it carries. That precedence is not a tie-break, it is a correction:
    /// signal 1 is *not* unambiguous for our own toolchain. The slateos target
    /// is an LLVM `x86_64-unknown-linux-musl` target, and an object is tagged
    /// `ELFOSABI_GNU` as soon as it uses a GNU extension — so a native SlateOS
    /// program can reach the kernel as OSABI 3. `coreutils`' binaries do, and
    /// were being run with the *Linux* syscall table: their first native
    /// syscall (`SYS_SET_FS_BASE`, 528) was refused as an unknown Linux
    /// number. Found 2026-09-24, when `ctest-coreutils-runs` exec'd
    /// `/mnt/bin/true` for the first time. The marker is how design-decisions
    /// §33 (operator's decision) says native binaries identify themselves; its
    /// producers are the toolchain's (`requests/a-bd-coreutils-cannot-start-
    /// two-link-faults.md`).
    #[must_use]
    pub fn detect_linux_abi(&self) -> bool {
        if self.has_slateos_marker() {
            return false;
        }

        // Signal 1: EI_OSABI explicit Linux/GNU tag.
        if self.header.e_ident_osabi == ELFOSABI_GNU {
            return true;
        }

        // Signal 2 + 3: walk program headers once, checking for
        // PT_INTERP with a Linux loader path and PT_GNU_PROPERTY.
        for i in 0..self.program_header_count() {
            let Some(phdr) = self.program_header(i) else {
                continue;
            };

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

    /// Whether this binary declares itself SlateOS-native — the explicit
    /// marker of design-decisions.md §33.
    ///
    /// Either form is sufficient:
    ///
    /// * `e_ident[EI_OSABI] == ELFOSABI_SLATEOS` (255); or
    /// * a note in a `PT_NOTE` segment whose owner is `"SlateOS"` and whose
    ///   type is `NT_SLATEOS_ABI` (1), descriptor = ABI revision (4 bytes).
    ///
    /// Two forms because they suit different producers: a linker script can
    /// keep a note that the C runtime emits, so every binary linked against
    /// our libc carries it with no extra step; a byte in the header can be
    /// stamped after the fact by whatever stages a binary, for toolchains that
    /// cannot be taught to keep a note.
    #[must_use]
    pub fn has_slateos_marker(&self) -> bool {
        if self.header.e_ident_osabi == ELFOSABI_SLATEOS {
            return true;
        }
        (0..self.program_header_count()).any(|i| {
            let Some(phdr) = self.program_header(i) else {
                return false;
            };
            if phdr.p_type != PT_NOTE {
                return false;
            }
            self.raw_segment_bytes(&phdr).is_some_and(|notes| {
                notes_contain(notes, phdr.p_align, SLATEOS_NOTE_NAME, NT_SLATEOS_ABI)
            })
        })
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
pub unsafe fn load_segments(elf: &ElfFile<'_>, pml4_phys: u64) -> KernelResult<()> {
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
        let start = seg
            .vaddr
            .checked_add(bias)
            .ok_or(KernelError::InvalidAddress)?;
        let end = start
            .checked_add(seg.mem_size)
            .ok_or(KernelError::InvalidAddress)?;
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
            let s = seg
                .vaddr
                .checked_add(bias)
                .ok_or(KernelError::InvalidAddress)?;
            let e = s
                .checked_add(seg.mem_size)
                .ok_or(KernelError::InvalidAddress)?;
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
            let biased_vaddr = seg
                .vaddr
                .checked_add(bias)
                .ok_or(KernelError::InvalidAddress)?;
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
        if let Err(e) =
            unsafe { page_table::map_frame_subpages(pml4_phys, virt, phys_frame, subpage_flags) }
        {
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
    let file_offset = match seg
        .file_offset
        .checked_add(overlap_start.saturating_sub(seg.vaddr))
    {
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

/// Whether the note segment image `notes` holds a note owned by `name` (the
/// exact bytes, NUL included) with type `n_type`.
///
/// Each note is `Elf64_Nhdr { n_namesz, n_descsz, n_type }` (three `u32`s),
/// then the name, then the descriptor, each padded to the note alignment.
/// That alignment is 4 for ordinary notes and 8 for `.note.gnu.property`, and
/// a segment's `p_align` says which — anything other than 8 is taken as 4,
/// which is what every toolchain emits for a 4-aligned note segment.
///
/// Every size in a note is attacker-controlled (it is file content), so every
/// offset is computed with checked arithmetic and every read goes through
/// `get`: a malformed note ends the walk with `false`, it never panics and
/// never reads outside the segment.
fn notes_contain(notes: &[u8], p_align: u64, name: &[u8], n_type: u32) -> bool {
    let align: usize = if p_align == 8 { 8 } else { 4 };
    let pad = |x: usize| -> Option<usize> {
        x.checked_add(align.wrapping_sub(1))
            .map(|v| v & !align.wrapping_sub(1))
    };
    let word = |at: usize| -> Option<u32> {
        let b: [u8; 4] = notes.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(u32::from_le_bytes(b))
    };
    let mut off = 0usize;
    // One loop turn per note; `next > off` below guarantees progress, so
    // this terminates within `notes.len() / 12` turns.
    while off < notes.len() {
        let Some(namesz) = word(off) else {
            return false;
        };
        let Some(descsz) = word(off.saturating_add(4)) else {
            return false;
        };
        let Some(ty) = word(off.saturating_add(8)) else {
            return false;
        };
        let Some(name_start) = off.checked_add(12) else {
            return false;
        };
        let Some(name_end) = name_start.checked_add(namesz as usize) else {
            return false;
        };
        let Some(desc_start) = pad(name_end) else {
            return false;
        };
        let Some(desc_end) = desc_start.checked_add(descsz as usize) else {
            return false;
        };
        if desc_end > notes.len() {
            return false;
        }
        if ty == n_type && notes.get(name_start..name_end) == Some(name) {
            return true;
        }
        let Some(next) = pad(desc_end) else {
            return false;
        };
        if next <= off {
            return false;
        }
        off = next;
    }
    false
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

    contains_subslice(path, b"ld-linux-x86-64") || contains_subslice(path, b"ld-musl-x86_64")
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

/// [`build_test_elf_public`] with its one segment widened down to file offset
/// 0, so that it maps the ELF header and the program headers too, as a
/// linker's first segment does: the program-header self-test's "found in a
/// segment" case (`spawn::self_test_main_phdr`). The code stays at the same
/// address. (`build_test_elf`'s segment starts after the headers, and the
/// self-test assumed otherwise until its first boot, rq42.)
#[must_use]
#[allow(clippy::arithmetic_side_effects)] // offsets and sizes of a 200-byte image
pub fn build_test_elf_mapping_headers() -> alloc::vec::Vec<u8> {
    let mut buf = build_test_elf();
    // The program header is at 64: p_offset +8, p_vaddr +16, p_filesz +32,
    // p_memsz +40.
    let read = |b: &[u8], at: usize| {
        b.get(at..at + 8)
            .and_then(|s| <[u8; 8]>::try_from(s).ok())
            .map_or(0, u64::from_le_bytes)
    };
    let offset = read(&buf, 64 + 8);
    let vaddr = read(&buf, 64 + 16);
    let filesz = read(&buf, 64 + 32);
    let memsz = read(&buf, 64 + 40);
    write_u64(&mut buf, 64 + 8, 0);
    write_u64(&mut buf, 64 + 16, vaddr - offset);
    write_u64(&mut buf, 64 + 32, filesz + offset);
    write_u64(&mut buf, 64 + 40, memsz + offset);
    buf
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
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
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
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
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
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
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

/// Build a **Linux-ABI** `ET_EXEC` test ELF for `madvise`'s promises -- the
/// advice values programs build on rather than hints -- end to end.
///
/// Before forking it checks, in its own address space:
/// - `MADV_DONTNEED` of the second page of a two-page mapping: that page
///   reads zero, the first keeps its bytes (4 KiB exact, inside one 16 KiB
///   frame -- the partly-present fault path);
/// - `MADV_REMOVE` on shared memory: it reads zero; on private memory:
///   `EINVAL`, and nothing changes;
/// - `MADV_WIPEONFORK` on shared memory: `EINVAL`.
///
/// Then the fork advice: three private pages each hold a secret; A is marked
/// wipe-on-fork, B is left alone (the control), C is marked don't-fork. The
/// child checks what it got: A reads zero, B the secret, and C is not mapped
/// at all (`mprotect` on it is `ENOMEM` -- a check that does not fault). The
/// child then writes A. The parent reaps the child and checks that its own A
/// and C still hold the secret.
///
/// The parent's A matters as much as the child's. A is one 4 KiB page of a
/// 16 KiB frame whose other sub-pages *are* copied, so the child's first touch
/// of A takes the sub-page fault path beside a frame it shares with the parent
/// -- the path that, before 2026-10-07, zeroed the slice of that shared frame
/// and so wiped the *parent's* secret instead of giving the child a page.
///
/// Exit codes: `0x2A` pass. The parent's: `0x30` mmap failed, `0x31` / `0x32`
/// the wipe / don't-fork advice failed, `0x36` wipe of shared memory was not
/// `EINVAL`, `0x3A` `MADV_REMOVE` on shared memory failed, `0x3B` the shared
/// page was not zero after it, `0x3C` `MADV_REMOVE` on private memory was not
/// `EINVAL`, `0x37` `MADV_DONTNEED` failed, `0x38` the dropped page was not
/// zero, `0x39` its neighbour lost its bytes, `0x33` fork failed, `0x34` wait4
/// failed, `0x35` the child did not exit normally, `0x51` / `0x52` the
/// parent's A / C lost its secret. The child's, passed through as the
/// parent's: `0x41` A was not zero, `0x42` B was not the secret, `0x43` C was
/// mapped, `0x44` A was not writable.
///
/// The bytes are GNU `as` output for this source (no relocations; every
/// branch is relative), assembled with `as --64` and taken with
/// `objcopy -O binary -j .text`:
///
/// ```text
/// .intel_syntax noprefix
/// .globl _start
/// _start:
///     call map_page               # page A: wiped across fork
///     mov r12, rax
///     call map_page               # page B: copied (the control)
///     mov r13, rax
///     call map_page               # page C: not copied at all
///     mov r14, rax
///     movabs rax, 0x5EC2E75EC2E7
///     mov [r12], rax
///     mov [r13], rax
///     mov [r14], rax
///     mov rdi, r12                # madvise(A, 4096, MADV_WIPEONFORK)
///     mov edx, 18
///     call advise
///     mov edi, 0x31
///     test rax, rax
///     jnz exit
///     mov rdi, r14                # madvise(C, 4096, MADV_DONTFORK)
///     mov edx, 10
///     call advise
///     mov edi, 0x32
///     test rax, rax
///     jnz exit
///     mov eax, 9                  # D = mmap(.., MAP_SHARED|MAP_ANONYMOUS)
///     xor edi, edi
///     mov esi, 4096
///     mov edx, 3
///     mov r10d, 0x21
///     mov r8, -1
///     xor r9d, r9d
///     syscall
///     mov edi, 0x30
///     cmp rax, -4096
///     ja exit
///     mov rbx, rax                # D
///     movabs rax, 0x5EC2E75EC2E7
///     mov [rbx], rax
///     mov rdi, rbx                # wipe of shared memory: EINVAL
///     mov edx, 18
///     call advise
///     mov edi, 0x36
///     cmp rax, -22
///     jne exit
///     mov rdi, rbx                # MADV_REMOVE on shared memory: zeros
///     mov edx, 9
///     call advise
///     mov edi, 0x3A
///     test rax, rax
///     jnz exit
///     mov edi, 0x3B
///     cmp qword ptr [rbx], 0
///     jne exit
///     mov rdi, r13                # MADV_REMOVE on private memory: EINVAL
///     mov edx, 9
///     call advise
///     mov edi, 0x3C
///     cmp rax, -22
///     jne exit
///     mov eax, 9                  # E = mmap(NULL, 8192, RW, PRIVATE|ANON)
///     xor edi, edi
///     mov esi, 8192
///     mov edx, 3
///     mov r10d, 0x22
///     mov r8, -1
///     xor r9d, r9d
///     syscall
///     mov edi, 0x30
///     cmp rax, -4096
///     ja exit
///     mov r15, rax
///     movabs rax, 0x5EC2E75EC2E7
///     mov [r15], rax
///     mov [r15 + 4096], rax
///     lea rdi, [r15 + 4096]       # MADV_DONTNEED of E's second page alone
///     mov edx, 4
///     call advise
///     mov edi, 0x37
///     test rax, rax
///     jnz exit
///     mov edi, 0x38               # the dropped page reads zero
///     cmp qword ptr [r15 + 4096], 0
///     jne exit
///     movabs rax, 0x5EC2E75EC2E7
///     mov edi, 0x39               # its neighbour keeps its bytes
///     cmp [r15], rax
///     jne exit
///     mov eax, 57                 # fork
///     syscall
///     mov edi, 0x33
///     test rax, rax
///     js exit
///     jz child
///     sub rsp, 16                 # parent: wait4(-1, &status, 0, NULL)
///     mov edi, -1
///     mov rsi, rsp
///     xor edx, edx
///     xor r10d, r10d
///     mov eax, 61
///     syscall
///     mov edi, 0x34
///     test rax, rax
///     jle exit
///     mov eax, [rsp]
///     mov edi, 0x35
///     test eax, 0x7f              # WIFEXITED
///     jnz exit
///     movzx edi, ah               # WEXITSTATUS
///     cmp edi, 0x5A
///     jne exit                    # the child's own failure code
///     movabs rax, 0x5EC2E75EC2E7  # the parent's pages are still its own
///     mov edi, 0x51
///     cmp [r12], rax
///     jne exit
///     mov edi, 0x52
///     cmp [r14], rax
///     jne exit
///     mov edi, 0x2A               # pass
///     jmp exit
/// child:
///     mov edi, 0x41               # wiped: the child sees zeros
///     cmp qword ptr [r12], 0
///     jne exit
///     movabs rax, 0x5EC2E75EC2E7
///     mov edi, 0x42               # copied: the child sees the secret
///     cmp [r13], rax
///     jne exit
///     mov rdi, r14                # not copied: no mapping, so mprotect is ENOMEM
///     mov esi, 4096
///     mov edx, 1
///     mov eax, 10
///     syscall
///     mov edi, 0x43
///     cmp rax, -12
///     jne exit
///     mov qword ptr [r12], 7      # the wiped page is the child's own, writable
///     mov edi, 0x44
///     cmp qword ptr [r12], 7
///     jne exit
///     mov edi, 0x5A               # child: all checks passed
/// exit:
///     mov eax, 231                # exit_group(edi)
///     syscall
///     int3
/// map_page:                       # rax = mmap(NULL, 4096, RW, PRIVATE|ANON, -1, 0)
///     mov eax, 9
///     xor edi, edi
///     mov esi, 4096
///     mov edx, 3
///     mov r10d, 0x22
///     mov r8, -1
///     xor r9d, r9d
///     syscall
///     cmp rax, -4096
///     ja map_fail
///     ret
/// map_fail:
///     mov edi, 0x30
///     jmp exit
/// advise:                         # rax = madvise(rdi, 4096, edx)
///     mov esi, 4096
///     mov eax, 28
///     syscall
///     ret
/// ```
///
/// Paired with [`crate::proc::spawn::self_test_linux_madvise`]. Tagged
/// `ELFOSABI_GNU` so `spawn_process` routes it through the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_madvise_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const CODE: [u8; 701] = [
        0xE8, 0x78, 0x02, 0x00, 0x00, 0x49, 0x89, 0xC4, 0xE8, 0x70, 0x02, 0x00, 0x00, 0x49, 0x89,
        0xC5, 0xE8, 0x68, 0x02, 0x00, 0x00, 0x49, 0x89, 0xC6, 0x48, 0xB8, 0xE7, 0xC2, 0x5E, 0xE7,
        0xC2, 0x5E, 0x00, 0x00, 0x49, 0x89, 0x04, 0x24, 0x49, 0x89, 0x45, 0x00, 0x49, 0x89, 0x06,
        0x4C, 0x89, 0xE7, 0xBA, 0x12, 0x00, 0x00, 0x00, 0xE8, 0x76, 0x02, 0x00, 0x00, 0xBF, 0x31,
        0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x0F, 0x85, 0x2D, 0x02, 0x00, 0x00, 0x4C, 0x89, 0xF7,
        0xBA, 0x0A, 0x00, 0x00, 0x00, 0xE8, 0x5B, 0x02, 0x00, 0x00, 0xBF, 0x32, 0x00, 0x00, 0x00,
        0x48, 0x85, 0xC0, 0x0F, 0x85, 0x12, 0x02, 0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00, 0x31,
        0xFF, 0xBE, 0x00, 0x10, 0x00, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x21, 0x00,
        0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x45, 0x31, 0xC9, 0x0F, 0x05, 0xBF,
        0x30, 0x00, 0x00, 0x00, 0x48, 0x3D, 0x00, 0xF0, 0xFF, 0xFF, 0x0F, 0x87, 0xDE, 0x01, 0x00,
        0x00, 0x48, 0x89, 0xC3, 0x48, 0xB8, 0xE7, 0xC2, 0x5E, 0xE7, 0xC2, 0x5E, 0x00, 0x00, 0x48,
        0x89, 0x03, 0x48, 0x89, 0xDF, 0xBA, 0x12, 0x00, 0x00, 0x00, 0xE8, 0xFC, 0x01, 0x00, 0x00,
        0xBF, 0x36, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0xEA, 0x0F, 0x85, 0xB2, 0x01, 0x00, 0x00,
        0x48, 0x89, 0xDF, 0xBA, 0x09, 0x00, 0x00, 0x00, 0xE8, 0xE0, 0x01, 0x00, 0x00, 0xBF, 0x3A,
        0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x0F, 0x85, 0x97, 0x01, 0x00, 0x00, 0xBF, 0x3B, 0x00,
        0x00, 0x00, 0x48, 0x83, 0x3B, 0x00, 0x0F, 0x85, 0x88, 0x01, 0x00, 0x00, 0x4C, 0x89, 0xEF,
        0xBA, 0x09, 0x00, 0x00, 0x00, 0xE8, 0xB6, 0x01, 0x00, 0x00, 0xBF, 0x3C, 0x00, 0x00, 0x00,
        0x48, 0x83, 0xF8, 0xEA, 0x0F, 0x85, 0x6C, 0x01, 0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00,
        0x31, 0xFF, 0xBE, 0x00, 0x20, 0x00, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x22,
        0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x45, 0x31, 0xC9, 0x0F, 0x05,
        0xBF, 0x30, 0x00, 0x00, 0x00, 0x48, 0x3D, 0x00, 0xF0, 0xFF, 0xFF, 0x0F, 0x87, 0x38, 0x01,
        0x00, 0x00, 0x49, 0x89, 0xC7, 0x48, 0xB8, 0xE7, 0xC2, 0x5E, 0xE7, 0xC2, 0x5E, 0x00, 0x00,
        0x49, 0x89, 0x07, 0x49, 0x89, 0x87, 0x00, 0x10, 0x00, 0x00, 0x49, 0x8D, 0xBF, 0x00, 0x10,
        0x00, 0x00, 0xBA, 0x04, 0x00, 0x00, 0x00, 0xE8, 0x4B, 0x01, 0x00, 0x00, 0xBF, 0x37, 0x00,
        0x00, 0x00, 0x48, 0x85, 0xC0, 0x0F, 0x85, 0x02, 0x01, 0x00, 0x00, 0xBF, 0x38, 0x00, 0x00,
        0x00, 0x49, 0x83, 0xBF, 0x00, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x85, 0xEF, 0x00, 0x00, 0x00,
        0x48, 0xB8, 0xE7, 0xC2, 0x5E, 0xE7, 0xC2, 0x5E, 0x00, 0x00, 0xBF, 0x39, 0x00, 0x00, 0x00,
        0x49, 0x39, 0x07, 0x0F, 0x85, 0xD7, 0x00, 0x00, 0x00, 0xB8, 0x39, 0x00, 0x00, 0x00, 0x0F,
        0x05, 0xBF, 0x33, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x0F, 0x88, 0xC2, 0x00, 0x00, 0x00,
        0x74, 0x67, 0x48, 0x83, 0xEC, 0x10, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0x89, 0xE6, 0x31,
        0xD2, 0x45, 0x31, 0xD2, 0xB8, 0x3D, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x34, 0x00, 0x00,
        0x00, 0x48, 0x85, 0xC0, 0x0F, 0x8E, 0x9A, 0x00, 0x00, 0x00, 0x8B, 0x04, 0x24, 0xBF, 0x35,
        0x00, 0x00, 0x00, 0xA9, 0x7F, 0x00, 0x00, 0x00, 0x0F, 0x85, 0x87, 0x00, 0x00, 0x00, 0x0F,
        0xB6, 0xFC, 0x83, 0xFF, 0x5A, 0x75, 0x7F, 0x48, 0xB8, 0xE7, 0xC2, 0x5E, 0xE7, 0xC2, 0x5E,
        0x00, 0x00, 0xBF, 0x51, 0x00, 0x00, 0x00, 0x49, 0x39, 0x04, 0x24, 0x75, 0x6A, 0xBF, 0x52,
        0x00, 0x00, 0x00, 0x49, 0x39, 0x06, 0x75, 0x60, 0xBF, 0x2A, 0x00, 0x00, 0x00, 0xEB, 0x59,
        0xBF, 0x41, 0x00, 0x00, 0x00, 0x49, 0x83, 0x3C, 0x24, 0x00, 0x75, 0x4D, 0x48, 0xB8, 0xE7,
        0xC2, 0x5E, 0xE7, 0xC2, 0x5E, 0x00, 0x00, 0xBF, 0x42, 0x00, 0x00, 0x00, 0x49, 0x39, 0x45,
        0x00, 0x75, 0x38, 0x4C, 0x89, 0xF7, 0xBE, 0x00, 0x10, 0x00, 0x00, 0xBA, 0x01, 0x00, 0x00,
        0x00, 0xB8, 0x0A, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x43, 0x00, 0x00, 0x00, 0x48, 0x83,
        0xF8, 0xF4, 0x75, 0x19, 0x49, 0xC7, 0x04, 0x24, 0x07, 0x00, 0x00, 0x00, 0xBF, 0x44, 0x00,
        0x00, 0x00, 0x49, 0x83, 0x3C, 0x24, 0x07, 0x75, 0x05, 0xBF, 0x5A, 0x00, 0x00, 0x00, 0xB8,
        0xE7, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xCC, 0xB8, 0x09, 0x00, 0x00, 0x00, 0x31, 0xFF, 0xBE,
        0x00, 0x10, 0x00, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x22, 0x00, 0x00, 0x00,
        0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x45, 0x31, 0xC9, 0x0F, 0x05, 0x48, 0x3D, 0x00,
        0xF0, 0xFF, 0xFF, 0x77, 0x01, 0xC3, 0xBF, 0x30, 0x00, 0x00, 0x00, 0xEB, 0xC5, 0xBE, 0x00,
        0x10, 0x00, 0x00, 0xB8, 0x1C, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xC3,
    ];
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size = CODE.len() as u64;
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
    buf[cs..(cs + CODE.len())].copy_from_slice(&CODE);

    buf
}

/// Build a **Linux-ABI** `ET_EXEC` test ELF for the names that stand for a
/// process's own descriptors -- `/dev/stdin` and `/dev/fd/N` -- end to end
/// (`syscall::linux::reopen_own_fd`, `own_fd_stat_target`).
///
/// It makes a file standard input with its offset at 6, opens `/dev/stdin`
/// and reads "hello" from it -- a re-open, a new description from offset 0 --
/// then reads "world" from descriptor 0, whose offset the re-open left alone.
/// `stat("/dev/stdin")` is the file's (11 bytes). `/dev/fd/9` with 9 closed is
/// `ENOENT`. With a pipe's read end as 9, `open("/dev/fd/9", O_WRONLY)` is the
/// pipe's *write* end (Linux opens a pipe through `/proc` as a FIFO): a byte
/// written there is read from the read end. With the pipe as standard input,
/// `stat("/dev/stdin")` is a FIFO.
///
/// Exit codes: `0x2A` pass. `0x30`-`0x33` setting up the file failed; `0x34`
/// opening `/dev/stdin` failed; `0x35`/`0x36` it did not read "hello" from 0;
/// `0x37`/`0x38` descriptor 0 lost its offset; `0x39`/`0x3A` `stat` of
/// `/dev/stdin` failed or was not the file; `0x3B` a closed `/dev/fd/9` was not
/// `ENOENT`; `0x3C`/`0x3D` the pipe setup failed; `0x3E` opening the other end
/// failed; `0x3F`-`0x41` the byte written there did not arrive; `0x42`/`0x43`
/// `stat` of `/dev/stdin` on a pipe failed or was not a FIFO.
///
/// The bytes are GNU `as` output for this source (no relocations; every
/// branch and string reference is RIP-relative), assembled with `as --64` and
/// taken with `objcopy -O binary -j .text`:
///
/// ```text
/// .intel_syntax noprefix
/// .globl _start
/// _start:
///     sub rsp, 256                # [rsp, +144) stat buffer; [rsp+160] read buffer; [rsp+200] pipe fds
///     lea rdi, [rip + path_file]  # f = open(path_file, O_RDWR|O_CREAT|O_TRUNC, 0644)
///     mov esi, 0x242
///     mov edx, 0x1a4
///     mov eax, 2
///     syscall
///     mov edi, 0x30
///     test rax, rax
///     js exit
///     mov r12, rax
///     mov rdi, r12                # write(f, "hello world", 11)
///     lea rsi, [rip + hello]
///     mov edx, 11
///     mov eax, 1
///     syscall
///     mov edi, 0x31
///     cmp rax, 11
///     jne exit
///     mov rdi, r12                # lseek(f, 6, SEEK_SET)
///     mov esi, 6
///     xor edx, edx
///     mov eax, 8
///     syscall
///     mov edi, 0x32
///     cmp rax, 6
///     jne exit
///     mov rdi, r12                # dup2(f, 0): stdin is the file, at offset 6
///     xor esi, esi
///     mov eax, 33
///     syscall
///     mov edi, 0x33
///     test rax, rax
///     jne exit
///     lea rdi, [rip + path_stdin] # g = open("/dev/stdin", O_RDONLY)
///     xor esi, esi
///     xor edx, edx
///     mov eax, 2
///     syscall
///     mov edi, 0x34
///     test rax, rax
///     js exit
///     mov r13, rax
///     mov rdi, r13                # read(g, buf, 5): a new description, from 0
///     lea rsi, [rsp + 160]
///     mov edx, 5
///     xor eax, eax
///     syscall
///     mov edi, 0x35
///     cmp rax, 5
///     jne exit
///     mov edi, 0x36
///     mov eax, [rsp + 160]
///     cmp eax, 0x6c6c6568         # "hell"
///     jne exit
///     xor edi, edi                # read(0, buf, 5): descriptor 0 kept its offset
///     lea rsi, [rsp + 160]
///     mov edx, 5
///     xor eax, eax
///     syscall
///     mov edi, 0x37
///     cmp rax, 5
///     jne exit
///     mov edi, 0x38
///     mov eax, [rsp + 160]
///     cmp eax, 0x6c726f77         # "worl"
///     jne exit
///     lea rdi, [rip + path_stdin] # stat("/dev/stdin"): the file, 11 bytes
///     mov rsi, rsp
///     mov eax, 4
///     syscall
///     mov edi, 0x39
///     test rax, rax
///     jne exit
///     mov edi, 0x3A
///     cmp qword ptr [rsp + 48], 11
///     jne exit
///     lea rdi, [rip + path_fd9]   # open("/dev/fd/9") with 9 closed: ENOENT
///     xor esi, esi
///     xor edx, edx
///     mov eax, 2
///     syscall
///     mov edi, 0x3B
///     cmp rax, -2
///     jne exit
///     lea rdi, [rsp + 200]        # pipe(p)
///     mov eax, 22
///     syscall
///     mov edi, 0x3C
///     test rax, rax
///     jne exit
///     mov edi, [rsp + 200]        # dup2(p[0], 9)
///     mov esi, 9
///     mov eax, 33
///     syscall
///     mov edi, 0x3D
///     cmp rax, 9
///     jne exit
///     lea rdi, [rip + path_fd9]   # w = open("/dev/fd/9", O_WRONLY): the other end
///     mov esi, 1
///     xor edx, edx
///     mov eax, 2
///     syscall
///     mov edi, 0x3E
///     test rax, rax
///     js exit
///     mov rdi, rax                # write(w, "Q", 1)
///     lea rsi, [rip + q]
///     mov edx, 1
///     mov eax, 1
///     syscall
///     mov edi, 0x3F
///     cmp rax, 1
///     jne exit
///     mov edi, [rsp + 200]        # read(p[0], buf, 1) == 'Q'
///     lea rsi, [rsp + 160]
///     mov edx, 1
///     xor eax, eax
///     syscall
///     mov edi, 0x40
///     cmp rax, 1
///     jne exit
///     mov edi, 0x41
///     cmp byte ptr [rsp + 160], 0x51
///     jne exit
///     mov edi, [rsp + 200]        # dup2(p[0], 0): stdin is the pipe
///     xor esi, esi
///     mov eax, 33
///     syscall
///     lea rdi, [rip + path_stdin] # stat("/dev/stdin"): a FIFO
///     mov rsi, rsp
///     mov eax, 4
///     syscall
///     mov edi, 0x42
///     test rax, rax
///     jne exit
///     mov eax, [rsp + 24]
///     and eax, 0xf000
///     mov edi, 0x43
///     cmp eax, 0x1000
///     jne exit
///     lea rdi, [rip + path_file]  # unlink(path_file)
///     mov eax, 87
///     syscall
///     mov edi, 0x2A               # pass
/// exit:
///     mov eax, 231                # exit_group(edi)
///     syscall
///     int3
/// path_file:
///     .asciz "/tmp/dev-stdin-test"
/// path_stdin:
///     .asciz "/dev/stdin"
/// path_fd9:
///     .asciz "/dev/fd/9"
/// hello:
///     .ascii "hello world"
/// q:
///     .ascii "Q"
/// ```
///
/// Paired with [`crate::proc::spawn::self_test_linux_dev_stdin`]. Tagged
/// `ELFOSABI_GNU` so `spawn_process` routes it through the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_dev_stdin_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const CODE: [u8; 703] = [
        0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00, 0x48, 0x8D, 0x3D, 0x7C, 0x02, 0x00, 0x00, 0xBE,
        0x42, 0x02, 0x00, 0x00, 0xBA, 0xA4, 0x01, 0x00, 0x00, 0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F,
        0x05, 0xBF, 0x30, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x0F, 0x88, 0x55, 0x02, 0x00, 0x00,
        0x49, 0x89, 0xC4, 0x4C, 0x89, 0xE7, 0x48, 0x8D, 0x35, 0x79, 0x02, 0x00, 0x00, 0xBA, 0x0B,
        0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x31, 0x00, 0x00, 0x00,
        0x48, 0x83, 0xF8, 0x0B, 0x0F, 0x85, 0x2D, 0x02, 0x00, 0x00, 0x4C, 0x89, 0xE7, 0xBE, 0x06,
        0x00, 0x00, 0x00, 0x31, 0xD2, 0xB8, 0x08, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x32, 0x00,
        0x00, 0x00, 0x48, 0x83, 0xF8, 0x06, 0x0F, 0x85, 0x0D, 0x02, 0x00, 0x00, 0x4C, 0x89, 0xE7,
        0x31, 0xF6, 0xB8, 0x21, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x33, 0x00, 0x00, 0x00, 0x48,
        0x85, 0xC0, 0x0F, 0x85, 0xF3, 0x01, 0x00, 0x00, 0x48, 0x8D, 0x3D, 0x08, 0x02, 0x00, 0x00,
        0x31, 0xF6, 0x31, 0xD2, 0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x34, 0x00, 0x00,
        0x00, 0x48, 0x85, 0xC0, 0x0F, 0x88, 0xD3, 0x01, 0x00, 0x00, 0x49, 0x89, 0xC5, 0x4C, 0x89,
        0xEF, 0x48, 0x8D, 0xB4, 0x24, 0xA0, 0x00, 0x00, 0x00, 0xBA, 0x05, 0x00, 0x00, 0x00, 0x31,
        0xC0, 0x0F, 0x05, 0xBF, 0x35, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x05, 0x0F, 0x85, 0xAD,
        0x01, 0x00, 0x00, 0xBF, 0x36, 0x00, 0x00, 0x00, 0x8B, 0x84, 0x24, 0xA0, 0x00, 0x00, 0x00,
        0x3D, 0x68, 0x65, 0x6C, 0x6C, 0x0F, 0x85, 0x96, 0x01, 0x00, 0x00, 0x31, 0xFF, 0x48, 0x8D,
        0xB4, 0x24, 0xA0, 0x00, 0x00, 0x00, 0xBA, 0x05, 0x00, 0x00, 0x00, 0x31, 0xC0, 0x0F, 0x05,
        0xBF, 0x37, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x05, 0x0F, 0x85, 0x74, 0x01, 0x00, 0x00,
        0xBF, 0x38, 0x00, 0x00, 0x00, 0x8B, 0x84, 0x24, 0xA0, 0x00, 0x00, 0x00, 0x3D, 0x77, 0x6F,
        0x72, 0x6C, 0x0F, 0x85, 0x5D, 0x01, 0x00, 0x00, 0x48, 0x8D, 0x3D, 0x72, 0x01, 0x00, 0x00,
        0x48, 0x89, 0xE6, 0xB8, 0x04, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x39, 0x00, 0x00, 0x00,
        0x48, 0x85, 0xC0, 0x0F, 0x85, 0x3E, 0x01, 0x00, 0x00, 0xBF, 0x3A, 0x00, 0x00, 0x00, 0x48,
        0x83, 0x7C, 0x24, 0x30, 0x0B, 0x0F, 0x85, 0x2D, 0x01, 0x00, 0x00, 0x48, 0x8D, 0x3D, 0x4D,
        0x01, 0x00, 0x00, 0x31, 0xF6, 0x31, 0xD2, 0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF,
        0x3B, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0xFE, 0x0F, 0x85, 0x0C, 0x01, 0x00, 0x00, 0x48,
        0x8D, 0xBC, 0x24, 0xC8, 0x00, 0x00, 0x00, 0xB8, 0x16, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF,
        0x3C, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x0F, 0x85, 0xEF, 0x00, 0x00, 0x00, 0x8B, 0xBC,
        0x24, 0xC8, 0x00, 0x00, 0x00, 0xBE, 0x09, 0x00, 0x00, 0x00, 0xB8, 0x21, 0x00, 0x00, 0x00,
        0x0F, 0x05, 0xBF, 0x3D, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x09, 0x0F, 0x85, 0xCD, 0x00,
        0x00, 0x00, 0x48, 0x8D, 0x3D, 0xED, 0x00, 0x00, 0x00, 0xBE, 0x01, 0x00, 0x00, 0x00, 0x31,
        0xD2, 0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x3E, 0x00, 0x00, 0x00, 0x48, 0x85,
        0xC0, 0x0F, 0x88, 0xAA, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0x48, 0x8D, 0x35, 0xDC, 0x00,
        0x00, 0x00, 0xBA, 0x01, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF,
        0x3F, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01, 0x0F, 0x85, 0x85, 0x00, 0x00, 0x00, 0x8B,
        0xBC, 0x24, 0xC8, 0x00, 0x00, 0x00, 0x48, 0x8D, 0xB4, 0x24, 0xA0, 0x00, 0x00, 0x00, 0xBA,
        0x01, 0x00, 0x00, 0x00, 0x31, 0xC0, 0x0F, 0x05, 0xBF, 0x40, 0x00, 0x00, 0x00, 0x48, 0x83,
        0xF8, 0x01, 0x75, 0x62, 0xBF, 0x41, 0x00, 0x00, 0x00, 0x80, 0xBC, 0x24, 0xA0, 0x00, 0x00,
        0x00, 0x51, 0x75, 0x53, 0x8B, 0xBC, 0x24, 0xC8, 0x00, 0x00, 0x00, 0x31, 0xF6, 0xB8, 0x21,
        0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x8D, 0x3D, 0x58, 0x00, 0x00, 0x00, 0x48, 0x89, 0xE6,
        0xB8, 0x04, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x42, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0,
        0x75, 0x28, 0x8B, 0x44, 0x24, 0x18, 0x25, 0x00, 0xF0, 0x00, 0x00, 0xBF, 0x43, 0x00, 0x00,
        0x00, 0x3D, 0x00, 0x10, 0x00, 0x00, 0x75, 0x13, 0x48, 0x8D, 0x3D, 0x14, 0x00, 0x00, 0x00,
        0xB8, 0x57, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x2A, 0x00, 0x00, 0x00, 0xB8, 0xE7, 0x00,
        0x00, 0x00, 0x0F, 0x05, 0xCC, 0x2F, 0x74, 0x6D, 0x70, 0x2F, 0x64, 0x65, 0x76, 0x2D, 0x73,
        0x74, 0x64, 0x69, 0x6E, 0x2D, 0x74, 0x65, 0x73, 0x74, 0x00, 0x2F, 0x64, 0x65, 0x76, 0x2F,
        0x73, 0x74, 0x64, 0x69, 0x6E, 0x00, 0x2F, 0x64, 0x65, 0x76, 0x2F, 0x66, 0x64, 0x2F, 0x39,
        0x00, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x77, 0x6F, 0x72, 0x6C, 0x64, 0x51,
    ];
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size = CODE.len() as u64;
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
    buf[cs..(cs + CODE.len())].copy_from_slice(&CODE);

    buf
}

/// Build a Linux-ABI ELF that exercises POSIX timers end to end:
/// `timer_create` (ids, clocks, sigevents, `SIGEV_THREAD_ID`), a one-shot's
/// `SIGALRM` carrying its id, a periodic timer whose skipped expiries come
/// back as `si_overrun` and `timer_getoverrun`, two timers on one signal each
/// delivering, `SIGEV_NONE`, an absolute `CLOCK_REALTIME` arming in the past,
/// a `signalfd` read of a timer's record, an `SA_SIGINFO` handler reached
/// through `rt_sigsuspend`, and a forked child that has no timers. Exits
/// 0x2A on success, else the failed check's code (0x30-0x66; the child's
/// 0x70-0x71 come back through its exit status, 0x64).
///
/// Unlike this file's other test programs, which are assembled by hand, it
/// is freestanding C -- too long to keep right in assembly -- compiled to a
/// flat, relocation-free blob linked at the load address below. The same
/// source built with `-DLINUX_ORACLE` (which expects Linux's answer for the
/// CPU-time clocks, the one place this kernel differs) exits 0x2A on Linux
/// 6.6, which is how its expectations were checked.
///
/// Built with gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, in WSL:
///
/// ```text
/// gcc -O1 -std=gnu11 -ffreestanding -fno-builtin -fno-stack-protec
///     tor -fno-asynchronous-unwind-tables -fno-unwind-tables -fcf-protec
///     tion=none -fno-jump-tables -mgeneral-regs-only -fPIE -fvisibility=
///     hidden -nostdlib -static -Wall -Wextra -Wl,-T,posix_timers.ld -Wl,
///     --build-id=none -Wl,-z,norelro -Wl,-z,noexecstack
///     -o posix_timers.elf posix_timers.c
/// objcopy -O binary -j .text posix_timers.elf posix_timers.bin
/// ```
///
/// `posix_timers.ld`:
///
/// ```text
/// ENTRY(_start)
/// SECTIONS
/// {
///   . = 0x4000000000;
///   .text : {
///     *(.text.start)
///     *(.text .text.*)
///     *(.rodata .rodata.*)
///   }
///   .data : { *(.data .data.*) }
///   .bss : { *(.bss .bss.*) *(COMMON) }
///   /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
/// }
/// ```
///
/// `posix_timers.c`:
///
/// ```c
/// /* Ring-3 test of POSIX timers through the Linux ABI
///  * (spawn::self_test_linux_posix_timers). Freestanding: raw syscalls, no libc.
///  * Exits 0x2A when every check passes, otherwise with the first failed
///  * check's code. Built by the generator in the builder's doc comment
///  * (kernel/src/proc/elf.rs, build_linux_posix_timers_test_elf). */
///
/// typedef unsigned long u64;
/// typedef long i64;
/// typedef int i32;
/// typedef unsigned int u32;
///
/// /* A page mapped at a fixed address: the signal handler's mailbox. */
/// #define SHARED 0x5000000000UL
///
/// static inline long sc6(long n, long a, long b, long c, long d, long e, long f)
/// {
///     register long r10 __asm__("r10") = d;
///     register long r8 __asm__("r8") = e;
///     register long r9 __asm__("r9") = f;
///     long ret;
///     __asm__ volatile("syscall"
///                      : "=a"(ret)
///                      : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
///                      : "rcx", "r11", "memory");
///     return ret;
/// }
/// #define sc(n, a, b, c, d) sc6((n), (long)(a), (long)(b), (long)(c), (long)(d), 0, 0)
///
/// static void __attribute__((noreturn)) die(int code)
/// {
///     for (;;)
///         sc(231, code, 0, 0, 0); /* exit_group */
/// }
/// #define CHECK(cond, code)  \
///     do {                   \
///         if (!(cond))       \
///             die(code);     \
///     } while (0)
///
/// struct ts { i64 sec, nsec; };
/// struct its { struct ts interval, value; };
/// struct sigev { u64 value; i32 signo; i32 notify; i32 tid; i32 pad[11]; };
/// struct si { i32 signo, err, code, pad; u32 a, b; u64 value; u64 rest[12]; };
/// struct sfd {
///     u32 signo; i32 err, code; u32 pid, uid; i32 fd; u32 tid, band, overrun, trapno;
///     i32 status, sint; u64 ptr, utime, stime, addr; u64 pad[6];
/// };
/// struct kact { u64 handler, flags, restorer, mask; };
///
/// /* gcc may call these for struct initialisation and copies. */
/// void *memset(void *d, int c, u64 n)
/// {
///     volatile unsigned char *p = d;
///     while (n--)
///         *p++ = (unsigned char)c;
///     return d;
/// }
/// void *memcpy(void *d, const void *s, u64 n)
/// {
///     volatile unsigned char *p = d;
///     const unsigned char *q = s;
///     while (n--)
///         *p++ = *q++;
///     return d;
/// }
///
/// static long tcreate(long clk, struct sigev *ev, i32 *id) { return sc(222, clk, ev, id, 0); }
/// static long tset(i32 id, long flags, struct its *nv, struct its *ov) { return sc(223, id, flags, nv, ov); }
/// static long tget(i32 id, struct its *cv) { return sc(224, id, cv, 0, 0); }
/// static long tover(i32 id) { return sc(225, id, 0, 0, 0); }
/// static long tdel(i32 id) { return sc(226, id, 0, 0, 0); }
///
/// static void its_set(struct its *t, i64 vs, i64 vns, i64 is, i64 ins)
/// {
///     t->value.sec = vs;
///     t->value.nsec = vns;
///     t->interval.sec = is;
///     t->interval.nsec = ins;
/// }
///
/// static void msleep(long ms)
/// {
///     struct ts t = { ms / 1000, (ms % 1000) * 1000000 };
///     sc(35, &t, 0, 0, 0); /* nanosleep */
/// }
///
/// /* rt_sigtimedwait for the signals in mask, waiting at most timeout_ms. */
/// static long sigwait_ms(u64 mask, struct si *info, long timeout_ms)
/// {
///     struct ts t = { timeout_ms / 1000, (timeout_ms % 1000) * 1000000 };
///     memset(info, 0, sizeof *info);
///     return sc(128, &mask, info, &t, 8);
/// }
///
/// static void handler(int sig, struct si *info, void *uc)
/// {
///     volatile u64 *box = (volatile u64 *)SHARED;
///     (void)uc;
///     box[0] = (u64)sig;
///     box[1] = (u64)(i64)info->code;
///     box[2] = info->value;
///     box[3] = info->a;
///     box[4] += 1;
/// }
///
/// __attribute__((naked)) static void restorer(void)
/// {
///     __asm__ volatile("mov $15, %eax\n\tsyscall\n\tud2"); /* rt_sigreturn */
/// }
///
/// #define BIT(s) (1UL << ((s) - 1))
/// #define EPERM 1
/// #define EINTR 4
/// #define EAGAIN 11
/// #define EINVAL 22
/// #define EOPNOTSUPP 95
///
/// __attribute__((used, noreturn)) void main_(void)
/// {
///     struct its t, o, z;
///     struct si info;
///     struct sigev ev;
///     i32 id0 = -1, id1 = -1, x = -1, p = -1, a = -1, b = -1, n = -1, r = -1, h = -1;
///     /* Taken synchronously, so blocked: SIGUSR1, SIGUSR2, SIGALRM,
///      * SIGRTMIN+2; and SIGRTMIN+3 until the handler test unblocks it. */
///     u64 blk = BIT(10) | BIT(12) | BIT(14) | BIT(36) | BIT(37);
///     CHECK(sc(14, 0, &blk, 0, 8) == 0, 0x30);
///
///     /* Ids: 0 and 1; a deleted timer is gone. */
///     CHECK(tcreate(1, 0, &id0) == 0 && id0 == 0, 0x31);
///     CHECK(tcreate(1, 0, &id1) == 0 && id1 == 1, 0x32);
///     CHECK(tdel(id1) == 0 && tdel(id1) == -EINVAL, 0x33);
///     its_set(&t, 1, 0, 0, 0);
///     CHECK(tover(99) == -EINVAL && tget(99, &t) == -EINVAL && tset(99, 0, &t, 0) == -EINVAL, 0x34);
///
///     /* Clocks: none, a reader-only one, an alarm one, the CPU-time ones. */
///     CHECK(tcreate(10, 0, &x) == -EINVAL, 0x35);
///     CHECK(tcreate(4, 0, &x) == -EOPNOTSUPP, 0x36);
///     CHECK(tcreate(8, 0, &x) == -EPERM, 0x37);
/// #ifdef LINUX_ORACLE
///     /* Linux times the CPU-time clocks; this kernel answers EOPNOTSUPP. */
///     CHECK(tcreate(2, 0, &x) == 0 && tdel(x) == 0 && tcreate(-6, 0, &x) == 0 && tdel(x) == 0, 0x38);
/// #else
///     CHECK(tcreate(2, 0, &x) == -EOPNOTSUPP && tcreate(-6, 0, &x) == -EOPNOTSUPP, 0x38);
/// #endif
///
///     /* Sigevents: a bad notify; SIGEV_THREAD_ID to no thread of ours, then to
///      * our own. */
///     memset(&ev, 0, sizeof ev);
///     ev.notify = 3;
///     CHECK(tcreate(1, &ev, &x) == -EINVAL, 0x39);
///     ev.notify = 4;
///     ev.signo = 10;
///     ev.tid = 0x7fffffff;
///     CHECK(tcreate(1, &ev, &x) == -EINVAL, 0x3A);
///     ev.tid = (i32)sc(186, 0, 0, 0, 0); /* gettid */
///     CHECK(tcreate(1, &ev, &x) == 0 && tdel(x) == 0, 0x3B);
///
///     /* No sigevent: SIGALRM, carrying the timer id. A fired one-shot reads 0. */
///     its_set(&t, 0, 1000000, 0, 0);
///     CHECK(tset(id0, 0, &t, 0) == 0, 0x3C);
///     msleep(20);
///     CHECK(tget(id0, &t) == 0 && t.value.sec == 0 && t.value.nsec == 0, 0x3D);
///     CHECK(sigwait_ms(BIT(14), &info, 0) == 14, 0x3E);
///     CHECK(info.code == -2 && info.a == (u32)id0 && info.value == (u64)id0, 0x3F);
///
///     /* Periodic, blocked 105 ms: one signal, the other expiries its overrun. */
///     memset(&ev, 0, sizeof ev);
///     ev.signo = 10;
///     ev.value = 0x1122334455667788UL;
///     CHECK(tcreate(1, &ev, &p) == 0, 0x40);
///     its_set(&t, 0, 10000000, 0, 10000000);
///     CHECK(tset(p, 0, &t, 0) == 0, 0x41);
///     msleep(105);
///     CHECK(tget(p, &t) == 0 && t.interval.nsec == 10000000 && t.value.sec == 0
///               && t.value.nsec > 0 && t.value.nsec <= 10000000,
///           0x42);
///     CHECK(tover(p) == 0, 0x43);
///     CHECK(sigwait_ms(BIT(10), &info, 0) == 10, 0x44);
///     CHECK(info.code == -2 && info.a == (u32)p && info.value == 0x1122334455667788UL, 0x45);
///     CHECK(info.b >= 9 && info.b < 1000000, 0x46);
///     CHECK(tover(p) == (long)info.b, 0x47);
///     /* Disarm: the old setting comes back, the new one reads all zero. */
///     memset(&z, 0, sizeof z);
///     CHECK(tset(p, 0, &z, &o) == 0 && o.interval.sec == 0 && o.interval.nsec == 10000000, 0x48);
///     CHECK(tget(p, &t) == 0 && t.value.sec == 0 && t.value.nsec == 0 && t.interval.nsec == 0, 0x49);
///     CHECK(tover(p) == 0, 0x4A);
///
///     /* Two timers on SIGUSR2 both deliver, in order; a kill between them is
///      * merged into what is queued. */
///     ev.signo = 12;
///     ev.value = 111;
///     CHECK(tcreate(1, &ev, &a) == 0, 0x4B);
///     ev.value = 222;
///     CHECK(tcreate(1, &ev, &b) == 0, 0x4B);
///     its_set(&t, 0, 1000000, 0, 0);
///     CHECK(tset(a, 0, &t, 0) == 0 && tset(b, 0, &t, 0) == 0, 0x4C);
///     msleep(20);
///     CHECK(sc(62, sc(39, 0, 0, 0, 0), 12, 0, 0) == 0, 0x4D); /* kill(getpid(), SIGUSR2) */
///     CHECK(sigwait_ms(BIT(12), &info, 0) == 12 && info.code == -2 && info.value == 111, 0x4E);
///     CHECK(sigwait_ms(BIT(12), &info, 0) == 12 && info.code == -2 && info.value == 222, 0x4F);
///     CHECK(sigwait_ms(BIT(12), &info, 0) == -EAGAIN, 0x50);
///
///     /* SIGEV_NONE keeps time and delivers nothing. */
///     memset(&ev, 0, sizeof ev);
///     ev.notify = 1;
///     CHECK(tcreate(1, &ev, &n) == 0, 0x51);
///     its_set(&t, 0, 10000000, 0, 0);
///     CHECK(tset(n, 0, &t, 0) == 0, 0x52);
///     CHECK(tget(n, &t) == 0 && t.value.nsec > 0, 0x53);
///     msleep(20);
///     CHECK(tget(n, &t) == 0 && t.value.sec == 0 && t.value.nsec == 0, 0x54);
///
///     /* TIMER_ABSTIME on CLOCK_REALTIME, a time long past: fires at once. */
///     memset(&ev, 0, sizeof ev);
///     ev.signo = 36;
///     ev.value = 7;
///     CHECK(tcreate(0, &ev, &r) == 0, 0x55);
///     its_set(&t, 1, 0, 0, 0);
///     CHECK(tset(r, 1, &t, 0) == 0, 0x56);
///     CHECK(sigwait_ms(BIT(36), &info, 1000) == 36 && info.code == -2 && info.value == 7
///               && info.a == (u32)r,
///           0x57);
///
///     /* A signalfd read reports the timer's id, overrun and value. */
///     {
///         u64 m = BIT(12);
///         struct sfd rec;
///         long fd = sc(289, -1, &m, 8, 0); /* signalfd4 */
///         CHECK(fd >= 0, 0x58);
///         its_set(&t, 0, 1000000, 0, 0);
///         CHECK(tset(a, 0, &t, 0) == 0, 0x59);
///         msleep(20);
///         memset(&rec, 0, sizeof rec);
///         CHECK(sc(0, fd, &rec, sizeof rec, 0) == 128, 0x5A);
///         CHECK(rec.signo == 12 && rec.code == -2 && rec.tid == (u32)a && rec.sint == 111
///                   && rec.ptr == 111 && rec.overrun == 0,
///               0x5B);
///         sc(3, fd, 0, 0, 0);
///     }
///
///     /* A handler: unblocked by rt_sigsuspend, it sees the timer's record, and
///      * rt_sigsuspend answers EINTR. */
///     {
///         volatile u64 *box = (volatile u64 *)SHARED;
///         struct kact act;
///         u64 susp = blk & ~BIT(37);
///         CHECK(sc6(9, SHARED, 4096, 3, 0x32, -1, 0) == (long)SHARED, 0x5C); /* mmap fixed */
///         memset(&act, 0, sizeof act);
///         act.handler = (u64)handler;
///         act.flags = 4 | 0x04000000; /* SA_SIGINFO | SA_RESTORER */
///         act.restorer = (u64)restorer;
///         CHECK(sc(13, 37, &act, 0, 8) == 0, 0x5D);
///         memset(&ev, 0, sizeof ev);
///         ev.signo = 37;
///         ev.value = 0xABCD;
///         CHECK(tcreate(1, &ev, &h) == 0, 0x5E);
///         its_set(&t, 0, 1000000, 0, 0);
///         CHECK(tset(h, 0, &t, 0) == 0, 0x5F);
///         CHECK(sc(130, &susp, 8, 0, 0) == -EINTR, 0x60); /* rt_sigsuspend */
///         CHECK(box[4] == 1 && box[0] == 37 && box[1] == (u64)-2 && box[2] == 0xABCD
///                   && box[3] == (u64)h,
///               0x61);
///     }
///
///     /* fork: the child has no timers, and its ids start again at 0. */
///     {
///         long pid = sc(57, 0, 0, 0, 0);
///         int status = 0;
///         if (pid == 0) {
///             struct its ct;
///             i32 cid = -1;
///             if (tget(id0, &ct) != -EINVAL)
///                 die(0x70);
///             if (tcreate(1, 0, &cid) != 0 || cid != 0)
///                 die(0x71);
///             die(0x5A);
///         }
///         CHECK(pid > 0, 0x62);
///         CHECK(sc(61, pid, &status, 0, 0) == pid, 0x63); /* wait4 */
///         CHECK(((status >> 8) & 0xff) == 0x5A && (status & 0x7f) == 0, 0x64);
///     }
///
///     /* The parent's are untouched; delete them all. */
///     CHECK(tget(id0, &t) == 0, 0x65);
///     CHECK(tdel(id0) == 0 && tdel(p) == 0 && tdel(a) == 0 && tdel(b) == 0 && tdel(n) == 0
///               && tdel(r) == 0 && tdel(h) == 0,
///           0x66);
///     die(0x2A);
/// }
///
/// __attribute__((naked, section(".text.start"))) void _start(void)
/// {
///     __asm__ volatile("and $-16, %rsp\n\tcall main_\n\tud2");
/// }
/// ```
///
/// Paired with [`crate::proc::spawn::self_test_linux_posix_timers`]. Tagged
/// `ELFOSABI_GNU` so `spawn_process` routes it through the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_posix_timers_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const CODE: [u8; 5038] = [
        0x48, 0x83, 0xE4, 0xF0, 0xE8, 0xD2, 0x01, 0x00, 0x00, 0x0F, 0x0B, 0x0F, 0x0B, 0x53, 0x48,
        0x63, 0xFF, 0xBB, 0xE7, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xD8, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xEB, 0xDF, 0x48, 0x63, 0xFF, 0x41, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE2, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F,
        0x05, 0xC3, 0x48, 0xBA, 0xCF, 0xF7, 0x53, 0xE3, 0xA5, 0x9B, 0xC4, 0x20, 0x48, 0x89, 0xF8,
        0x48, 0xF7, 0xEA, 0x48, 0xC1, 0xFA, 0x07, 0x48, 0x89, 0xF8, 0x48, 0xC1, 0xF8, 0x3F, 0x48,
        0x29, 0xC2, 0x48, 0x89, 0x54, 0x24, 0xF0, 0x48, 0x69, 0xD2, 0xE8, 0x03, 0x00, 0x00, 0x48,
        0x29, 0xD7, 0x48, 0x69, 0xFF, 0x40, 0x42, 0x0F, 0x00, 0x48, 0x89, 0x7C, 0x24, 0xF8, 0x48,
        0x8D, 0x7C, 0x24, 0xF0, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x23, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xC3, 0x48, 0x63, 0xC7, 0x48, 0xA3, 0x00, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x63, 0x46, 0x08, 0x48, 0xA3, 0x08, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x46, 0x18, 0x48, 0xA3, 0x10, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x8B, 0x46, 0x10, 0x48, 0xA3, 0x18, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0xBA, 0x20, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x02,
        0x48, 0x83, 0xC0, 0x01, 0x48, 0x89, 0x02, 0xC3, 0xB8, 0x0F, 0x00, 0x00, 0x00, 0x0F, 0x05,
        0x0F, 0x0B, 0x0F, 0x0B, 0x48, 0x89, 0xF8, 0x48, 0x85, 0xD2, 0x74, 0x15, 0x48, 0x01, 0xFA,
        0x48, 0x89, 0xF9, 0x49, 0x89, 0xC8, 0x48, 0x83, 0xC1, 0x01, 0x41, 0x88, 0x30, 0x48, 0x39,
        0xCA, 0x75, 0xF1, 0xC3, 0x53, 0x48, 0x83, 0xEC, 0x18, 0x48, 0x89, 0x3C, 0x24, 0x48, 0x89,
        0xF3, 0x48, 0x89, 0xD1, 0x48, 0xBA, 0xCF, 0xF7, 0x53, 0xE3, 0xA5, 0x9B, 0xC4, 0x20, 0x48,
        0x89, 0xC8, 0x48, 0xF7, 0xEA, 0x48, 0xC1, 0xFA, 0x07, 0x48, 0x89, 0xC8, 0x48, 0xC1, 0xF8,
        0x3F, 0x48, 0x29, 0xC2, 0x48, 0x89, 0x54, 0x24, 0x08, 0x48, 0x69, 0xD2, 0xE8, 0x03, 0x00,
        0x00, 0x48, 0x29, 0xD1, 0x48, 0x69, 0xC9, 0x40, 0x42, 0x0F, 0x00, 0x48, 0x89, 0x4C, 0x24,
        0x10, 0xBA, 0x80, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDF, 0xE8,
        0x88, 0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0x54, 0x24, 0x08, 0x48, 0x89, 0xE7, 0x41, 0xBA, 0x08,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xB8, 0x80, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDE, 0x0F, 0x05, 0x48, 0x83, 0xC4, 0x18, 0x5B,
        0xC3, 0x48, 0x89, 0xF8, 0x48, 0x85, 0xD2, 0x74, 0x1E, 0x48, 0x01, 0xFA, 0x48, 0x89, 0xF9,
        0x48, 0x83, 0xC6, 0x01, 0x49, 0x89, 0xC8, 0x48, 0x83, 0xC1, 0x01, 0x44, 0x0F, 0xB6, 0x4E,
        0xFF, 0x45, 0x88, 0x08, 0x48, 0x39, 0xCA, 0x75, 0xE8, 0xC3, 0x41, 0x55, 0x41, 0x54, 0x55,
        0x53, 0x48, 0x81, 0xEC, 0xE8, 0x01, 0x00, 0x00, 0xC7, 0x84, 0x24, 0xBC, 0x00, 0x00, 0x00,
        0xFF, 0xFF, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xB8, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
        0xC7, 0x84, 0x24, 0xB4, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xB0,
        0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xAC, 0x00, 0x00, 0x00, 0xFF,
        0xFF, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xA8, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xC7,
        0x84, 0x24, 0xA4, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xA0, 0x00,
        0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0x9C, 0x00, 0x00, 0x00, 0xFF, 0xFF,
        0xFF, 0xFF, 0x48, 0xB8, 0x00, 0x2A, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x48, 0x89, 0x84,
        0x24, 0x90, 0x00, 0x00, 0x00, 0x48, 0x8D, 0xB4, 0x24, 0x90, 0x00, 0x00, 0x00, 0x41, 0xBA,
        0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0E, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x0F,
        0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x30, 0x00, 0x00, 0x00, 0xE8, 0x78, 0xFD, 0xFF,
        0xFF, 0x48, 0x8D, 0x94, 0x24, 0xBC, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00,
        0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48,
        0x85, 0xC0, 0x75, 0x0A, 0x83, 0xBC, 0x24, 0xBC, 0x00, 0x00, 0x00, 0x00, 0x74, 0x0A, 0xBF,
        0x31, 0x00, 0x00, 0x00, 0xE8, 0x34, 0xFD, 0xFF, 0xFF, 0x48, 0x8D, 0x94, 0x24, 0xB8, 0x00,
        0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0x00,
        0xBE, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x0A, 0x83, 0xBC, 0x24,
        0xB8, 0x00, 0x00, 0x00, 0x01, 0x74, 0x0A, 0xBF, 0x32, 0x00, 0x00, 0x00, 0xE8, 0xF0, 0xFC,
        0xFF, 0xFF, 0xBF, 0x01, 0x00, 0x00, 0x00, 0xE8, 0x10, 0xFD, 0xFF, 0xFF, 0x48, 0x85, 0xC0,
        0x75, 0x12, 0x8B, 0xBC, 0x24, 0xB8, 0x00, 0x00, 0x00, 0xE8, 0xFF, 0xFC, 0xFF, 0xFF, 0x48,
        0x83, 0xF8, 0xEA, 0x74, 0x0A, 0xBF, 0x33, 0x00, 0x00, 0x00, 0xE8, 0xC5, 0xFC, 0xFF, 0xFF,
        0x48, 0xC7, 0x84, 0x24, 0xD0, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84,
        0x24, 0xD8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC0, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC8, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE1, 0x00, 0x00,
        0x00, 0xBF, 0x63, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xEA,
        0x74, 0x0A, 0xBF, 0x34, 0x00, 0x00, 0x00, 0xE8, 0x5F, 0xFC, 0xFF, 0xFF, 0x48, 0x8D, 0x9C,
        0x24, 0xC0, 0x01, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDE, 0x0F, 0x05,
        0x48, 0x83, 0xF8, 0xEA, 0x75, 0xDE, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00,
        0x00, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xEA, 0x75, 0xC9, 0x4C, 0x8D, 0xA4,
        0x24, 0xB4, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x0A,
        0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE2, 0x0F, 0x05, 0x48, 0x83,
        0xF8, 0xEA, 0x74, 0x0A, 0xBF, 0x35, 0x00, 0x00, 0x00, 0xE8, 0xF4, 0xFB, 0xFF, 0xFF, 0x41,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x04, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00,
        0x00, 0x00, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xA1, 0x74, 0x0A, 0xBF, 0x36, 0x00, 0x00, 0x00,
        0xE8, 0xC1, 0xFB, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x08,
        0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xFF, 0x74,
        0x0A, 0xBF, 0x37, 0x00, 0x00, 0x00, 0xE8, 0x8E, 0xFB, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8,
        0xDE, 0x00, 0x00, 0x00, 0xBF, 0x02, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x0F,
        0x05, 0x48, 0x83, 0xF8, 0xA1, 0x74, 0x0A, 0xBF, 0x38, 0x00, 0x00, 0x00, 0xE8, 0x5B, 0xFB,
        0xFF, 0xFF, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0x48, 0xC7, 0xC7, 0xFA, 0xFF, 0xFF, 0xFF, 0x0F,
        0x05, 0x48, 0x83, 0xF8, 0xA1, 0x75, 0xE2, 0x48, 0x8D, 0xAC, 0x24, 0xC0, 0x00, 0x00, 0x00,
        0xBA, 0x40, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0xE8, 0x32,
        0xFC, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xCC, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x41,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEE,
        0x4C, 0x89, 0xE2, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xEA, 0x74, 0x0A, 0xBF, 0x39, 0x00, 0x00,
        0x00, 0xE8, 0xEE, 0xFA, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xCC, 0x00, 0x00, 0x00, 0x04, 0x00,
        0x00, 0x00, 0xC7, 0x84, 0x24, 0xC8, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0xC7, 0x84,
        0x24, 0xD0, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x7F, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00,
        0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xEA, 0x74, 0x0A,
        0xBF, 0x3A, 0x00, 0x00, 0x00, 0xE8, 0x9F, 0xFA, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0xBA, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6, 0x0F,
        0x05, 0x89, 0x84, 0x24, 0xD0, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x01,
        0x00, 0x00, 0x00, 0x48, 0x89, 0xEE, 0x4C, 0x89, 0xE2, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75,
        0x11, 0x8B, 0xBC, 0x24, 0xB4, 0x00, 0x00, 0x00, 0xE8, 0x7B, 0xFA, 0xFF, 0xFF, 0x48, 0x85,
        0xC0, 0x74, 0x0A, 0xBF, 0x3B, 0x00, 0x00, 0x00, 0xE8, 0x42, 0xFA, 0xFF, 0xFF, 0x48, 0xC7,
        0x84, 0x24, 0xD0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xD8,
        0x01, 0x00, 0x00, 0x40, 0x42, 0x0F, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC0, 0x01, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x48, 0x63, 0xBC, 0x24, 0xBC, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00,
        0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0x85, 0xC0,
        0x74, 0x0A, 0xBF, 0x3C, 0x00, 0x00, 0x00, 0xE8, 0xDA, 0xF9, 0xFF, 0xFF, 0xBF, 0x14, 0x00,
        0x00, 0x00, 0xE8, 0x1F, 0xFA, 0xFF, 0xFF, 0x48, 0x63, 0xBC, 0x24, 0xBC, 0x00, 0x00, 0x00,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xDE, 0x0F, 0x05, 0x48, 0x8B, 0x94, 0x24, 0xD0, 0x01, 0x00, 0x00, 0x48, 0x0B, 0x94, 0x24,
        0xD8, 0x01, 0x00, 0x00, 0x48, 0x09, 0xC2, 0x74, 0x0A, 0xBF, 0x3D, 0x00, 0x00, 0x00, 0xE8,
        0x88, 0xF9, 0xFF, 0xFF, 0x48, 0x8D, 0xB4, 0x24, 0x00, 0x01, 0x00, 0x00, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0xBF, 0x00, 0x20, 0x00, 0x00, 0xE8, 0x94, 0xFA, 0xFF, 0xFF, 0x48, 0x83, 0xF8,
        0x0E, 0x74, 0x0A, 0xBF, 0x3E, 0x00, 0x00, 0x00, 0xE8, 0x61, 0xF9, 0xFF, 0xFF, 0x83, 0xBC,
        0x24, 0x08, 0x01, 0x00, 0x00, 0xFE, 0x75, 0x1C, 0x8B, 0x84, 0x24, 0xBC, 0x00, 0x00, 0x00,
        0x39, 0x84, 0x24, 0x10, 0x01, 0x00, 0x00, 0x75, 0x0C, 0x48, 0x98, 0x48, 0x39, 0x84, 0x24,
        0x18, 0x01, 0x00, 0x00, 0x74, 0x0A, 0xBF, 0x3F, 0x00, 0x00, 0x00, 0xE8, 0x31, 0xF9, 0xFF,
        0xFF, 0x48, 0x8D, 0xBC, 0x24, 0xC0, 0x00, 0x00, 0x00, 0xBA, 0x40, 0x00, 0x00, 0x00, 0xBE,
        0x00, 0x00, 0x00, 0x00, 0xE8, 0x1F, 0xFA, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xC8, 0x00, 0x00,
        0x00, 0x0A, 0x00, 0x00, 0x00, 0x48, 0xB8, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11,
        0x48, 0x89, 0x84, 0x24, 0xC0, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x94, 0x24, 0xB0, 0x00, 0x00,
        0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x48,
        0x89, 0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x40, 0x00, 0x00, 0x00, 0xE8,
        0xC5, 0xF8, 0xFF, 0xFF, 0x48, 0xC7, 0x84, 0x24, 0xD0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x84, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x80, 0x96, 0x98, 0x00, 0x48, 0xC7,
        0x84, 0x24, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC8,
        0x01, 0x00, 0x00, 0x80, 0x96, 0x98, 0x00, 0x48, 0x63, 0xBC, 0x24, 0xB0, 0x00, 0x00, 0x00,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xDA, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x41, 0x00, 0x00, 0x00, 0xE8, 0x5D,
        0xF8, 0xFF, 0xFF, 0xBF, 0x69, 0x00, 0x00, 0x00, 0xE8, 0xA2, 0xF8, 0xFF, 0xFF, 0x48, 0x63,
        0xBC, 0x24, 0xB0, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDE, 0x0F, 0x05, 0x48, 0x81, 0xBC, 0x24, 0xC8, 0x01,
        0x00, 0x00, 0x80, 0x96, 0x98, 0x00, 0x75, 0x1E, 0x48, 0x0B, 0x84, 0x24, 0xD0, 0x01, 0x00,
        0x00, 0x75, 0x14, 0x48, 0x8B, 0x84, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x48, 0x83, 0xE8, 0x01,
        0x48, 0x3D, 0x7F, 0x96, 0x98, 0x00, 0x76, 0x0A, 0xBF, 0x42, 0x00, 0x00, 0x00, 0xE8, 0xF4,
        0xF7, 0xFF, 0xFF, 0x48, 0x63, 0xBC, 0x24, 0xB0, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0xE1, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48,
        0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x43, 0x00, 0x00, 0x00, 0xE8, 0xBC, 0xF7, 0xFF, 0xFF, 0x48,
        0x8D, 0xB4, 0x24, 0x00, 0x01, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x02,
        0x00, 0x00, 0xE8, 0xC8, 0xF8, 0xFF, 0xFF, 0x48, 0x83, 0xF8, 0x0A, 0x74, 0x0A, 0xBF, 0x44,
        0x00, 0x00, 0x00, 0xE8, 0x95, 0xF7, 0xFF, 0xFF, 0x83, 0xBC, 0x24, 0x08, 0x01, 0x00, 0x00,
        0xFE, 0x75, 0x24, 0x8B, 0x84, 0x24, 0xB0, 0x00, 0x00, 0x00, 0x39, 0x84, 0x24, 0x10, 0x01,
        0x00, 0x00, 0x75, 0x14, 0x48, 0xBA, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x48,
        0x39, 0x94, 0x24, 0x18, 0x01, 0x00, 0x00, 0x74, 0x0A, 0xBF, 0x45, 0x00, 0x00, 0x00, 0xE8,
        0x5D, 0xF7, 0xFF, 0xFF, 0x8B, 0x8C, 0x24, 0x14, 0x01, 0x00, 0x00, 0x8D, 0x51, 0xF7, 0x81,
        0xFA, 0x36, 0x42, 0x0F, 0x00, 0x76, 0x0A, 0xBF, 0x46, 0x00, 0x00, 0x00, 0xE8, 0x41, 0xF7,
        0xFF, 0xFF, 0x48, 0x63, 0xF8, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE1,
        0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x8B, 0x94, 0x24, 0x14, 0x01, 0x00, 0x00,
        0x48, 0x39, 0xC2, 0x74, 0x0A, 0xBF, 0x47, 0x00, 0x00, 0x00, 0xE8, 0x07, 0xF7, 0xFF, 0xFF,
        0x4C, 0x8D, 0xA4, 0x24, 0x80, 0x01, 0x00, 0x00, 0xBA, 0x20, 0x00, 0x00, 0x00, 0xBE, 0x00,
        0x00, 0x00, 0x00, 0x4C, 0x89, 0xE7, 0xE8, 0xF2, 0xF7, 0xFF, 0xFF, 0x48, 0x63, 0xBC, 0x24,
        0xB0, 0x00, 0x00, 0x00, 0x4C, 0x8D, 0x94, 0x24, 0xA0, 0x01, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE,
        0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE2, 0x0F, 0x05, 0x48, 0x0B, 0x84, 0x24, 0xA0, 0x01,
        0x00, 0x00, 0x75, 0x0E, 0x48, 0x81, 0xBC, 0x24, 0xA8, 0x01, 0x00, 0x00, 0x80, 0x96, 0x98,
        0x00, 0x74, 0x0A, 0xBF, 0x48, 0x00, 0x00, 0x00, 0xE8, 0xA0, 0xF6, 0xFF, 0xFF, 0x48, 0x63,
        0xBC, 0x24, 0xB0, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDE, 0x0F, 0x05, 0x48, 0x8B, 0x94, 0x24, 0xD0, 0x01,
        0x00, 0x00, 0x48, 0x0B, 0x94, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x48, 0x0B, 0x94, 0x24, 0xC8,
        0x01, 0x00, 0x00, 0x48, 0x09, 0xC2, 0x74, 0x0A, 0xBF, 0x49, 0x00, 0x00, 0x00, 0xE8, 0x50,
        0xF6, 0xFF, 0xFF, 0x48, 0x63, 0xBC, 0x24, 0xB0, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0xE1, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48,
        0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x4A, 0x00, 0x00, 0x00, 0xE8, 0x18, 0xF6, 0xFF, 0xFF, 0xC7,
        0x84, 0x24, 0xC8, 0x00, 0x00, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC0,
        0x00, 0x00, 0x00, 0x6F, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x94, 0x24, 0xAC, 0x00, 0x00, 0x00,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x4B, 0x00, 0x00, 0x00, 0xE8, 0xC9,
        0xF5, 0xFF, 0xFF, 0x48, 0xC7, 0x84, 0x24, 0xC0, 0x00, 0x00, 0x00, 0xDE, 0x00, 0x00, 0x00,
        0x48, 0x8D, 0x94, 0x24, 0xA8, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00,
        0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x4B,
        0x00, 0x00, 0x00, 0xE8, 0x88, 0xF5, 0xFF, 0xFF, 0x48, 0xC7, 0x84, 0x24, 0xD0, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x40, 0x42,
        0x0F, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48,
        0xC7, 0x84, 0x24, 0xC8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x63, 0xBC, 0x24,
        0xAC, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x14, 0x48, 0x63, 0xBC,
        0x24, 0xA8, 0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0,
        0x74, 0x0A, 0xBF, 0x4C, 0x00, 0x00, 0x00, 0xE8, 0x0C, 0xF5, 0xFF, 0xFF, 0xBF, 0x14, 0x00,
        0x00, 0x00, 0xE8, 0x51, 0xF5, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0xB8, 0x27, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x89,
        0xC7, 0xB8, 0x3E, 0x00, 0x00, 0x00, 0xBE, 0x0C, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85,
        0xC0, 0x74, 0x0A, 0xBF, 0x4D, 0x00, 0x00, 0x00, 0xE8, 0xC0, 0xF4, 0xFF, 0xFF, 0x48, 0x8D,
        0xB4, 0x24, 0x00, 0x01, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x08, 0x00,
        0x00, 0xE8, 0xCC, 0xF5, 0xFF, 0xFF, 0x48, 0x83, 0xF8, 0x0C, 0x75, 0x15, 0x83, 0xBC, 0x24,
        0x08, 0x01, 0x00, 0x00, 0xFE, 0x75, 0x0B, 0x48, 0x83, 0xBC, 0x24, 0x18, 0x01, 0x00, 0x00,
        0x6F, 0x74, 0x0A, 0xBF, 0x4E, 0x00, 0x00, 0x00, 0xE8, 0x84, 0xF4, 0xFF, 0xFF, 0x48, 0x8D,
        0xB4, 0x24, 0x00, 0x01, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x08, 0x00,
        0x00, 0xE8, 0x90, 0xF5, 0xFF, 0xFF, 0x48, 0x83, 0xF8, 0x0C, 0x75, 0x18, 0x83, 0xBC, 0x24,
        0x08, 0x01, 0x00, 0x00, 0xFE, 0x75, 0x0E, 0x48, 0x81, 0xBC, 0x24, 0x18, 0x01, 0x00, 0x00,
        0xDE, 0x00, 0x00, 0x00, 0x74, 0x0A, 0xBF, 0x4F, 0x00, 0x00, 0x00, 0xE8, 0x45, 0xF4, 0xFF,
        0xFF, 0x48, 0x8D, 0xB4, 0x24, 0x00, 0x01, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xBF,
        0x00, 0x08, 0x00, 0x00, 0xE8, 0x51, 0xF5, 0xFF, 0xFF, 0x48, 0x83, 0xF8, 0xF5, 0x74, 0x0A,
        0xBF, 0x50, 0x00, 0x00, 0x00, 0xE8, 0x1E, 0xF4, 0xFF, 0xFF, 0x48, 0x8D, 0xBC, 0x24, 0xC0,
        0x00, 0x00, 0x00, 0xBA, 0x40, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0xE8, 0x0C,
        0xF5, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xCC, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x48,
        0x8D, 0x94, 0x24, 0xA4, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00,
        0xBF, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A,
        0xBF, 0x51, 0x00, 0x00, 0x00, 0xE8, 0xC4, 0xF3, 0xFF, 0xFF, 0x48, 0xC7, 0x84, 0x24, 0xD0,
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xD8, 0x01, 0x00, 0x00,
        0x80, 0x96, 0x98, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x84, 0x24, 0xC8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x63,
        0xBC, 0x24, 0xA4, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE,
        0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF,
        0x52, 0x00, 0x00, 0x00, 0xE8, 0x5C, 0xF3, 0xFF, 0xFF, 0x48, 0x63, 0xBC, 0x24, 0xA4, 0x00,
        0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xDE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x0B, 0x48, 0x83, 0xBC, 0x24, 0xD8,
        0x01, 0x00, 0x00, 0x00, 0x7F, 0x0A, 0xBF, 0x53, 0x00, 0x00, 0x00, 0xE8, 0x19, 0xF3, 0xFF,
        0xFF, 0xBF, 0x14, 0x00, 0x00, 0x00, 0xE8, 0x5E, 0xF3, 0xFF, 0xFF, 0x48, 0x63, 0xBC, 0x24,
        0xA4, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xDE, 0x0F, 0x05, 0x48, 0x8B, 0x94, 0x24, 0xD0, 0x01, 0x00, 0x00,
        0x48, 0x0B, 0x94, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x48, 0x09, 0xC2, 0x74, 0x0A, 0xBF, 0x54,
        0x00, 0x00, 0x00, 0xE8, 0xC7, 0xF2, 0xFF, 0xFF, 0x48, 0x8D, 0xBC, 0x24, 0xC0, 0x00, 0x00,
        0x00, 0xBA, 0x40, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0xE8, 0xB5, 0xF3, 0xFF,
        0xFF, 0xC7, 0x84, 0x24, 0xC8, 0x00, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84,
        0x24, 0xC0, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x94, 0x24, 0xA0, 0x00,
        0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x55, 0x00, 0x00, 0x00,
        0xE8, 0x61, 0xF2, 0xFF, 0xFF, 0x48, 0xC7, 0x84, 0x24, 0xD0, 0x01, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48,
        0xC7, 0x84, 0x24, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24,
        0xC8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x63, 0xBC, 0x24, 0xA0, 0x00, 0x00,
        0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE, 0x01, 0x00, 0x00, 0x00, 0x48,
        0x89, 0xDA, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x56, 0x00, 0x00, 0x00, 0xE8,
        0xF9, 0xF1, 0xFF, 0xFF, 0x48, 0x8D, 0xB4, 0x24, 0x00, 0x01, 0x00, 0x00, 0xBA, 0xE8, 0x03,
        0x00, 0x00, 0x48, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0xE8, 0x00, 0xF3,
        0xFF, 0xFF, 0x48, 0x83, 0xF8, 0x24, 0x75, 0x25, 0x83, 0xBC, 0x24, 0x08, 0x01, 0x00, 0x00,
        0xFE, 0x75, 0x1B, 0x48, 0x83, 0xBC, 0x24, 0x18, 0x01, 0x00, 0x00, 0x07, 0x75, 0x10, 0x8B,
        0x84, 0x24, 0xA0, 0x00, 0x00, 0x00, 0x39, 0x84, 0x24, 0x10, 0x01, 0x00, 0x00, 0x74, 0x0A,
        0xBF, 0x57, 0x00, 0x00, 0x00, 0xE8, 0xA8, 0xF1, 0xFF, 0xFF, 0x48, 0xC7, 0x44, 0x24, 0x08,
        0x00, 0x08, 0x00, 0x00, 0x48, 0x8D, 0x74, 0x24, 0x08, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x21, 0x01,
        0x00, 0x00, 0x48, 0xC7, 0xC7, 0xFF, 0xFF, 0xFF, 0xFF, 0xBA, 0x08, 0x00, 0x00, 0x00, 0x0F,
        0x05, 0x49, 0x89, 0xC4, 0x48, 0x85, 0xC0, 0x79, 0x0A, 0xBF, 0x58, 0x00, 0x00, 0x00, 0xE8,
        0x63, 0xF1, 0xFF, 0xFF, 0x48, 0xC7, 0x84, 0x24, 0xD0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x84, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x40, 0x42, 0x0F, 0x00, 0x48, 0xC7,
        0x84, 0x24, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC8,
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x63, 0xBC, 0x24, 0xAC, 0x00, 0x00, 0x00,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xDA, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x59, 0x00, 0x00, 0x00, 0xE8, 0xFB,
        0xF0, 0xFF, 0xFF, 0xBF, 0x14, 0x00, 0x00, 0x00, 0xE8, 0x40, 0xF1, 0xFF, 0xFF, 0x4C, 0x8D,
        0x6C, 0x24, 0x10, 0xBA, 0x80, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89,
        0xEF, 0xE8, 0xDF, 0xF1, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x00, 0x00, 0x00, 0x00, 0xBA,
        0x80, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE7, 0x4C, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x3D, 0x80,
        0x00, 0x00, 0x00, 0x74, 0x0A, 0xBF, 0x5A, 0x00, 0x00, 0x00, 0xE8, 0xA4, 0xF0, 0xFF, 0xFF,
        0x83, 0x7C, 0x24, 0x10, 0x0C, 0x75, 0x2A, 0x83, 0x7C, 0x24, 0x18, 0xFE, 0x75, 0x23, 0x8B,
        0x84, 0x24, 0xAC, 0x00, 0x00, 0x00, 0x39, 0x44, 0x24, 0x28, 0x75, 0x16, 0x83, 0x7C, 0x24,
        0x3C, 0x6F, 0x75, 0x0F, 0x48, 0x83, 0x7C, 0x24, 0x40, 0x6F, 0x75, 0x07, 0x83, 0x7C, 0x24,
        0x30, 0x00, 0x74, 0x0A, 0xBF, 0x5B, 0x00, 0x00, 0x00, 0xE8, 0x69, 0xF0, 0xFF, 0xFF, 0x41,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xBD, 0x03, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89,
        0xE8, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0xEF, 0xFF, 0xFF,
        0xFF, 0x48, 0x23, 0x84, 0x24, 0x90, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x08, 0x41,
        0xBA, 0x32, 0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0xBF, 0x00,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x10,
        0x00, 0x00, 0x4C, 0x89, 0xEA, 0x0F, 0x05, 0x48, 0x39, 0xF8, 0x74, 0x0A, 0xBF, 0x5C, 0x00,
        0x00, 0x00, 0xE8, 0xF8, 0xEF, 0xFF, 0xFF, 0x4C, 0x8D, 0x64, 0x24, 0x10, 0xBA, 0x20, 0x00,
        0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE7, 0xE8, 0xE6, 0xF0, 0xFF, 0xFF,
        0x48, 0x8D, 0x05, 0x89, 0xF0, 0xFF, 0xFF, 0x48, 0x89, 0x44, 0x24, 0x10, 0x48, 0xC7, 0x44,
        0x24, 0x18, 0x04, 0x00, 0x00, 0x04, 0x48, 0x8D, 0x05, 0xBF, 0xF0, 0xFF, 0xFF, 0x48, 0x89,
        0x44, 0x24, 0x20, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0D, 0x00, 0x00, 0x00, 0xBF, 0x25, 0x00, 0x00,
        0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74,
        0x0A, 0xBF, 0x5D, 0x00, 0x00, 0x00, 0xE8, 0x8B, 0xEF, 0xFF, 0xFF, 0x48, 0x8D, 0xBC, 0x24,
        0xC0, 0x00, 0x00, 0x00, 0xBA, 0x40, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0xE8,
        0x79, 0xF0, 0xFF, 0xFF, 0xC7, 0x84, 0x24, 0xC8, 0x00, 0x00, 0x00, 0x25, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0x84, 0x24, 0xC0, 0x00, 0x00, 0x00, 0xCD, 0xAB, 0x00, 0x00, 0x48, 0x8D, 0x94,
        0x24, 0x9C, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00, 0x00, 0xBF, 0x01,
        0x00, 0x00, 0x00, 0x48, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x5E,
        0x00, 0x00, 0x00, 0xE8, 0x25, 0xEF, 0xFF, 0xFF, 0x48, 0xC7, 0x84, 0x24, 0xD0, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xD8, 0x01, 0x00, 0x00, 0x40, 0x42,
        0x0F, 0x00, 0x48, 0xC7, 0x84, 0x24, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48,
        0xC7, 0x84, 0x24, 0xC8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x63, 0xBC, 0x24,
        0x9C, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDF, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x5F, 0x00,
        0x00, 0x00, 0xE8, 0xBD, 0xEE, 0xFF, 0xFF, 0x48, 0x8D, 0x7C, 0x24, 0x08, 0x41, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xB8, 0x82, 0x00, 0x00, 0x00, 0xBE, 0x08, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x0F, 0x05, 0x48, 0x83, 0xF8, 0xFC, 0x74, 0x0A, 0xBF, 0x60, 0x00, 0x00, 0x00, 0xE8, 0x85,
        0xEE, 0xFF, 0xFF, 0x48, 0xA1, 0x20, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83,
        0xF8, 0x01, 0x75, 0x49, 0x48, 0xA1, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0x83, 0xF8, 0x25, 0x75, 0x39, 0x48, 0xA1, 0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x48, 0x83, 0xF8, 0xFE, 0x75, 0x29, 0x48, 0xA1, 0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x3D, 0xCD, 0xAB, 0x00, 0x00, 0x75, 0x17, 0x48, 0xA1, 0x18, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x63, 0x94, 0x24, 0x9C, 0x00, 0x00, 0x00, 0x48, 0x39, 0xD0,
        0x74, 0x0A, 0xBF, 0x61, 0x00, 0x00, 0x00, 0xE8, 0x22, 0xEE, 0xFF, 0xFF, 0x41, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x39, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89,
        0xD6, 0x0F, 0x05, 0x48, 0x89, 0xC7, 0xC7, 0x44, 0x24, 0x04, 0x00, 0x00, 0x00, 0x00, 0x48,
        0x85, 0xC0, 0x75, 0x74, 0xC7, 0x44, 0x24, 0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0x74,
        0x24, 0x10, 0x48, 0x63, 0xBC, 0x24, 0xBC, 0x00, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00,
        0x0F, 0x05, 0x48, 0x83, 0xF8, 0xEA, 0x74, 0x0A, 0xBF, 0x70, 0x00, 0x00, 0x00, 0xE8, 0xC2,
        0xED, 0xFF, 0xFF, 0x48, 0x8D, 0x54, 0x24, 0x08, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xDE, 0x00, 0x00,
        0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85,
        0xC0, 0x75, 0x07, 0x83, 0x7C, 0x24, 0x08, 0x00, 0x74, 0x0A, 0xBF, 0x71, 0x00, 0x00, 0x00,
        0xE8, 0x84, 0xED, 0xFF, 0xFF, 0xBF, 0x5A, 0x00, 0x00, 0x00, 0xE8, 0x7A, 0xED, 0xFF, 0xFF,
        0x7F, 0x0A, 0xBF, 0x62, 0x00, 0x00, 0x00, 0xE8, 0x6E, 0xED, 0xFF, 0xFF, 0x48, 0x8D, 0x74,
        0x24, 0x04, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x3D, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x0F, 0x05, 0x48, 0x39, 0xC7, 0x74, 0x0A, 0xBF, 0x63, 0x00, 0x00, 0x00, 0xE8, 0x3C, 0xED,
        0xFF, 0xFF, 0x8B, 0x44, 0x24, 0x04, 0x25, 0x7F, 0xFF, 0x00, 0x00, 0x3D, 0x00, 0x5A, 0x00,
        0x00, 0x74, 0x0A, 0xBF, 0x64, 0x00, 0x00, 0x00, 0xE8, 0x22, 0xED, 0xFF, 0xFF, 0x48, 0x63,
        0xBC, 0x24, 0xBC, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xE0, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF,
        0x65, 0x00, 0x00, 0x00, 0xE8, 0xEA, 0xEC, 0xFF, 0xFF, 0x8B, 0xBC, 0x24, 0xBC, 0x00, 0x00,
        0x00, 0xE8, 0x08, 0xED, 0xFF, 0xFF, 0x48, 0x85, 0xC0, 0x75, 0x66, 0x8B, 0xBC, 0x24, 0xB0,
        0x00, 0x00, 0x00, 0xE8, 0xF7, 0xEC, 0xFF, 0xFF, 0x48, 0x85, 0xC0, 0x75, 0x55, 0x8B, 0xBC,
        0x24, 0xAC, 0x00, 0x00, 0x00, 0xE8, 0xE6, 0xEC, 0xFF, 0xFF, 0x48, 0x85, 0xC0, 0x75, 0x44,
        0x8B, 0xBC, 0x24, 0xA8, 0x00, 0x00, 0x00, 0xE8, 0xD5, 0xEC, 0xFF, 0xFF, 0x48, 0x85, 0xC0,
        0x75, 0x33, 0x8B, 0xBC, 0x24, 0xA4, 0x00, 0x00, 0x00, 0xE8, 0xC4, 0xEC, 0xFF, 0xFF, 0x48,
        0x85, 0xC0, 0x75, 0x22, 0x8B, 0xBC, 0x24, 0xA0, 0x00, 0x00, 0x00, 0xE8, 0xB3, 0xEC, 0xFF,
        0xFF, 0x48, 0x85, 0xC0, 0x75, 0x11, 0x8B, 0xBC, 0x24, 0x9C, 0x00, 0x00, 0x00, 0xE8, 0xA2,
        0xEC, 0xFF, 0xFF, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x66, 0x00, 0x00, 0x00, 0xE8, 0x69,
        0xEC, 0xFF, 0xFF, 0xBF, 0x2A, 0x00, 0x00, 0x00, 0xE8, 0x5F, 0xEC, 0xFF, 0xFF,
    ];
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size = CODE.len() as u64;
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
    buf[cs..(cs + CODE.len())].copy_from_slice(&CODE);

    buf
}

/// Build a Linux-ABI ELF that checks a signal reaches a thread making no system
/// calls, and that the code it interrupted gets every register back.
///
/// A loop holds known values in all general registers (`rcx` and `r11`
/// included), the sixteen XMM registers, MXCSR (round toward zero) and its
/// 128-byte red zone, and spins until a `SIGALRM` handler (from `setitimer`,
/// 50 ms out) has run -- so the signal can only arrive from the timer
/// interrupt. The handler records the MXCSR it starts with (the initial 0x1F80,
/// as on Linux) and the interrupted `rip` from its `ucontext`, then clobbers
/// every XMM register, MXCSR, `rcx`, `r11` and the argument registers. After
/// `rt_sigreturn` the loop must find all of its values intact. Exits 0x2A on
/// success, else the failed check's code (0x30-0x3C). The same program exits
/// 0x2A on Linux 6.6.
///
/// Built with gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, in WSL, linked by `posix_timers.ld`
/// (see [`build_linux_posix_timers_test_elf`]):
///
/// ```text
/// gcc -O1 -std=gnu11 -ffreestanding -fno-builtin -fno-stack-protec
///     tor -fno-asynchronous-unwind-tables -fno-unwind-tables -fcf-protec
///     tion=none -fno-jump-tables -fPIE -fvisibility=hidden -nostdlib -st
///     atic -Wall -Wextra -Wl,-T,posix_timers.ld -Wl,--build-id=none -Wl,
///     -z,norelro -Wl,-z,noexecstack
///     -o sigdeliver.elf sigdeliver.c
/// objcopy -O binary -j .text sigdeliver.elf sigdeliver.bin
/// ```
///
/// `sigdeliver.c`:
///
/// ```c
/// /* Ring-3 test of signal delivery from an interrupt, through the Linux ABI
///  * (spawn::self_test_linux_signal_from_interrupt). Freestanding: raw syscalls.
///  *
///  * A loop that makes no system calls holds known values in every general
///  * register, rcx and r11 included, in all sixteen XMM registers, in MXCSR
///  * (round toward zero) and in its red zone, and spins until a SIGALRM handler
///  * has run. The signal can only reach it from the timer interrupt. The handler
///  * records the MXCSR it starts with (Linux: the initial 0x1F80) and clobbers
///  * everything it can; after rt_sigreturn the loop must find all of its values
///  * intact. Exits 0x2A when every check passes, otherwise the failed check's
///  * code. */
///
/// typedef unsigned long u64;
/// typedef long i64;
/// typedef int i32;
/// typedef unsigned int u32;
///
/// #define SHARED 0x5000000000UL
///
/// static inline long sc6(long n, long a, long b, long c, long d, long e, long f)
/// {
///     register long r10 __asm__("r10") = d;
///     register long r8 __asm__("r8") = e;
///     register long r9 __asm__("r9") = f;
///     long ret;
///     __asm__ volatile("syscall"
///                      : "=a"(ret)
///                      : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
///                      : "rcx", "r11", "memory");
///     return ret;
/// }
/// #define sc(n, a, b, c, d) sc6((n), (long)(a), (long)(b), (long)(c), (long)(d), 0, 0)
///
/// static void __attribute__((noreturn)) die(int code)
/// {
///     for (;;)
///         sc(231, code, 0, 0, 0);
/// }
/// #define CHECK(cond, code)  \
///     do {                   \
///         if (!(cond))       \
///             die(code);     \
///     } while (0)
///
/// void *memset(void *d, int c, u64 n)
/// {
///     volatile unsigned char *p = d;
///     while (n--)
///         *p++ = (unsigned char)c;
///     return d;
/// }
///
/// struct kact { u64 handler, flags, restorer, mask; };
///
/// /* The mailbox (at SHARED, 8-byte words):
///  *   [0]  handler runs           [1]  MXCSR the handler started with
///  *   [2]  loop iterations        [3]  the rip the handler's context says
///  *   [8..21]  the loop's GPRs after the handler: rcx r11 rdx rsi rdi r8 r9 r10
///  *            r12 r13 r14 r15 rbx rbp
///  *   [24..39] the loop's red zone, 16 words below its rsp
///  *   [40]     the loop's MXCSR after
///  *   [64..95] xmm0..xmm15, 16 bytes each
///  *   [96], [97] the loop's first and last instruction addresses */
/// #define BOX ((volatile u64 *)SHARED)
///
/// struct ucontext_part { u64 flags, link, ss_sp; i32 ss_flags, pad; u64 ss_size; u64 gregs[23]; };
///
/// static void handler(int sig, void *info, void *ucv)
/// {
///     struct ucontext_part *uc = ucv;
///     u32 mxcsr;
///     (void)sig;
///     (void)info;
///     __asm__ volatile("stmxcsr %0" : "=m"(mxcsr));
///     BOX[1] = mxcsr;
///     /* uc_mcontext.rip: gregs index 16 (r8..r15, rdi, rsi, rbp, rbx, rdx, rax, rcx, rsp, rip). */
///     BOX[3] = uc->gregs[16];
///     /* Clobber what the loop holds: every XMM register, MXCSR, rcx, r11 and
///      * the argument registers. */
///     {
///         u32 weird = 0x3F80; /* round down, all masked */
///         __asm__ volatile("pcmpeqd %%xmm0, %%xmm0\n\t"
///                          "pcmpeqd %%xmm1, %%xmm1\n\t"
///                          "pcmpeqd %%xmm2, %%xmm2\n\t"
///                          "pcmpeqd %%xmm3, %%xmm3\n\t"
///                          "pcmpeqd %%xmm4, %%xmm4\n\t"
///                          "pcmpeqd %%xmm5, %%xmm5\n\t"
///                          "pcmpeqd %%xmm6, %%xmm6\n\t"
///                          "pcmpeqd %%xmm7, %%xmm7\n\t"
///                          "pcmpeqd %%xmm8, %%xmm8\n\t"
///                          "pcmpeqd %%xmm9, %%xmm9\n\t"
///                          "pcmpeqd %%xmm10, %%xmm10\n\t"
///                          "pcmpeqd %%xmm11, %%xmm11\n\t"
///                          "pcmpeqd %%xmm12, %%xmm12\n\t"
///                          "pcmpeqd %%xmm13, %%xmm13\n\t"
///                          "pcmpeqd %%xmm14, %%xmm14\n\t"
///                          "pcmpeqd %%xmm15, %%xmm15\n\t"
///                          "ldmxcsr %0\n\t"
///                          "mov $-1, %%rcx\n\t"
///                          "mov $-1, %%r11\n\t"
///                          "mov $-1, %%rdx\n\t"
///                          "mov $-1, %%rsi\n\t"
///                          "mov $-1, %%r8\n\t"
///                          "mov $-1, %%r9\n\t"
///                          "mov $-1, %%r10\n\t"
///                          :
///                          : "m"(weird)
///                          : "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7",
///                            "xmm8", "xmm9", "xmm10", "xmm11", "xmm12", "xmm13", "xmm14",
///                            "xmm15", "rcx", "r11", "rdx", "rsi", "r8", "r9", "r10", "memory");
///     }
///     BOX[0] += 1;
/// }
///
/// __attribute__((naked)) static void restorer(void)
/// {
///     __asm__ volatile("mov $15, %eax\n\tsyscall\n\tud2");
/// }
///
/// /* Spin, holding the patterns, until the handler has run (or about a billion
///  * iterations). Every pattern is stored back to the mailbox afterwards. */
/// __attribute__((noinline)) static void spin(void)
/// {
///     __asm__ volatile(
///         "push %%rbx\n\t"
///         "push %%rbp\n\t"
///         "mov $0x5000000000, %%rax\n\t"
///         "lea 1f(%%rip), %%rbx\n\t"
///         "mov %%rbx, 768(%%rax)\n\t"
///         "lea 3f(%%rip), %%rbx\n\t"
///         "mov %%rbx, 776(%%rax)\n\t"
///         /* The red zone: 16 words below rsp. */
///         "movabs $0x7272727272727200, %%rbx\n\t"
///         "mov $16, %%ecx\n\t"
///         "0:\n\t"
///         "mov %%rcx, %%rdx\n\t"
///         "neg %%rdx\n\t"
///         "lea (%%rbx,%%rcx), %%rbp\n\t"
///         "mov %%rbp, (%%rsp,%%rdx,8)\n\t"
///         "dec %%ecx\n\t"
///         "jnz 0b\n\t"
///         /* MXCSR: round toward zero, all masked. */
///         "movl $0x7F80, 800(%%rax)\n\t"
///         "ldmxcsr 800(%%rax)\n\t"
///         /* XMM i = (0x1000 + i) in both halves, from the GPR patterns below. */
///         "mov $0x1000, %%rdx\n\t"
///         "movq %%rdx, %%xmm0\n\t punpcklqdq %%xmm0, %%xmm0\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm1\n\t punpcklqdq %%xmm1, %%xmm1\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm2\n\t punpcklqdq %%xmm2, %%xmm2\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm3\n\t punpcklqdq %%xmm3, %%xmm3\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm4\n\t punpcklqdq %%xmm4, %%xmm4\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm5\n\t punpcklqdq %%xmm5, %%xmm5\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm6\n\t punpcklqdq %%xmm6, %%xmm6\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm7\n\t punpcklqdq %%xmm7, %%xmm7\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm8\n\t punpcklqdq %%xmm8, %%xmm8\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm9\n\t punpcklqdq %%xmm9, %%xmm9\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm10\n\t punpcklqdq %%xmm10, %%xmm10\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm11\n\t punpcklqdq %%xmm11, %%xmm11\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm12\n\t punpcklqdq %%xmm12, %%xmm12\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm13\n\t punpcklqdq %%xmm13, %%xmm13\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm14\n\t punpcklqdq %%xmm14, %%xmm14\n\t inc %%rdx\n\t"
///         "movq %%rdx, %%xmm15\n\t punpcklqdq %%xmm15, %%xmm15\n\t"
///         /* GPR patterns. */
///         "movabs $0x1111111111111111, %%rcx\n\t"
///         "movabs $0x2222222222222222, %%r11\n\t"
///         "movabs $0x3333333333333333, %%rdx\n\t"
///         "movabs $0x4444444444444444, %%rsi\n\t"
///         "movabs $0x5555555555555555, %%rdi\n\t"
///         "movabs $0x6666666666666666, %%r8\n\t"
///         "movabs $0x7777777777777777, %%r9\n\t"
///         "movabs $0x8888888888888888, %%r10\n\t"
///         "movabs $0x9999999999999999, %%r12\n\t"
///         "movabs $0xAAAAAAAAAAAAAAAA, %%r13\n\t"
///         "movabs $0xBBBBBBBBBBBBBBBB, %%r14\n\t"
///         "movabs $0xCCCCCCCCCCCCCCCC, %%r15\n\t"
///         "movabs $0xDDDDDDDDDDDDDDDD, %%rbx\n\t"
///         "movabs $0xEEEEEEEEEEEEEEEE, %%rbp\n\t"
///         /* The loop: count in memory, stop when the handler has run or after
///          * 2^30 iterations. No system call, no fault -- only an interrupt can
///          * bring the signal in. */
///         "1:\n\t"
///         "incq 16(%%rax)\n\t"
///         "cmpq $0, (%%rax)\n\t"
///         "jne 2f\n\t"
///         "cmpq $0x40000000, 16(%%rax)\n\t"
///         "jb 1b\n\t"
///         "2:\n\t"
///         "3:\n\t"
///         /* Store everything back. */
///         "mov %%rcx, 64(%%rax)\n\t"
///         "mov %%r11, 72(%%rax)\n\t"
///         "mov %%rdx, 80(%%rax)\n\t"
///         "mov %%rsi, 88(%%rax)\n\t"
///         "mov %%rdi, 96(%%rax)\n\t"
///         "mov %%r8, 104(%%rax)\n\t"
///         "mov %%r9, 112(%%rax)\n\t"
///         "mov %%r10, 120(%%rax)\n\t"
///         "mov %%r12, 128(%%rax)\n\t"
///         "mov %%r13, 136(%%rax)\n\t"
///         "mov %%r14, 144(%%rax)\n\t"
///         "mov %%r15, 152(%%rax)\n\t"
///         "mov %%rbx, 160(%%rax)\n\t"
///         "mov %%rbp, 168(%%rax)\n\t"
///         "stmxcsr 320(%%rax)\n\t"
///         "movdqu %%xmm0, 512(%%rax)\n\t"
///         "movdqu %%xmm1, 528(%%rax)\n\t"
///         "movdqu %%xmm2, 544(%%rax)\n\t"
///         "movdqu %%xmm3, 560(%%rax)\n\t"
///         "movdqu %%xmm4, 576(%%rax)\n\t"
///         "movdqu %%xmm5, 592(%%rax)\n\t"
///         "movdqu %%xmm6, 608(%%rax)\n\t"
///         "movdqu %%xmm7, 624(%%rax)\n\t"
///         "movdqu %%xmm8, 640(%%rax)\n\t"
///         "movdqu %%xmm9, 656(%%rax)\n\t"
///         "movdqu %%xmm10, 672(%%rax)\n\t"
///         "movdqu %%xmm11, 688(%%rax)\n\t"
///         "movdqu %%xmm12, 704(%%rax)\n\t"
///         "movdqu %%xmm13, 720(%%rax)\n\t"
///         "movdqu %%xmm14, 736(%%rax)\n\t"
///         "movdqu %%xmm15, 752(%%rax)\n\t"
///         /* The red zone back to the mailbox. */
///         "mov $16, %%ecx\n\t"
///         "4:\n\t"
///         "mov %%rcx, %%rdx\n\t"
///         "neg %%rdx\n\t"
///         "mov (%%rsp,%%rdx,8), %%rbx\n\t"
///         "mov %%rbx, 184(%%rax,%%rcx,8)\n\t"
///         "dec %%ecx\n\t"
///         "jnz 4b\n\t"
///         /* Back to the default MXCSR for the C code. */
///         "movl $0x1F80, 800(%%rax)\n\t"
///         "ldmxcsr 800(%%rax)\n\t"
///         "pop %%rbp\n\t"
///         "pop %%rbx\n\t"
///         :
///         :
///         : "rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14",
///           "r15", "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7", "xmm8",
///           "xmm9", "xmm10", "xmm11", "xmm12", "xmm13", "xmm14", "xmm15", "memory", "cc");
/// }
///
/// __attribute__((used, noreturn)) void main_(void)
/// {
///     struct kact act;
///     int attempt;
///     /* The mailbox. */
///     CHECK(sc6(9, SHARED, 4096, 3, 0x32, -1, 0) == (long)SHARED, 0x30);
///     memset(&act, 0, sizeof act);
///     act.handler = (u64)handler;
///     act.flags = 4 | 0x04000000; /* SA_SIGINFO | SA_RESTORER */
///     act.restorer = (u64)restorer;
///     CHECK(sc(13, 14, &act, 0, 8) == 0, 0x31); /* rt_sigaction(SIGALRM) */
///
///     /* Up to three tries: a try in which the signal arrives before the loop
///      * starts (a host stall inside setitimer) tests nothing, so it is redone. */
///     for (attempt = 0; attempt < 3; attempt++) {
///         u64 itv[4] = { 0, 0, 0, 50000 }; /* it_interval 0, it_value 50 ms */
///         int i;
///         for (i = 0; i < 128; i++)
///             BOX[i] = 0;
///         CHECK(sc(38, 0, itv, 0, 0) == 0, 0x32); /* setitimer(ITIMER_REAL) */
///         spin();
///         if (BOX[2] > 1)
///             break;
///     }
///     CHECK(BOX[0] == 1, 0x33);            /* the handler ran once */
///     CHECK(BOX[2] > 1, 0x34);             /* ...while the loop was spinning */
///     CHECK(BOX[3] >= BOX[96] && BOX[3] <= BOX[97], 0x35); /* interrupted inside the loop */
///     CHECK(BOX[1] == 0x1F80, 0x36);       /* the handler started from the initial MXCSR */
///     CHECK(BOX[8] == 0x1111111111111111UL && BOX[9] == 0x2222222222222222UL, 0x37); /* rcx, r11 */
///     CHECK(BOX[10] == 0x3333333333333333UL && BOX[11] == 0x4444444444444444UL
///               && BOX[12] == 0x5555555555555555UL && BOX[13] == 0x6666666666666666UL
///               && BOX[14] == 0x7777777777777777UL && BOX[15] == 0x8888888888888888UL,
///           0x38);
///     CHECK(BOX[16] == 0x9999999999999999UL && BOX[17] == 0xAAAAAAAAAAAAAAAAUL
///               && BOX[18] == 0xBBBBBBBBBBBBBBBBUL && BOX[19] == 0xCCCCCCCCCCCCCCCCUL
///               && BOX[20] == 0xDDDDDDDDDDDDDDDDUL && BOX[21] == 0xEEEEEEEEEEEEEEEEUL,
///           0x39);
///     {
///         int i;
///         for (i = 1; i <= 16; i++)
///             CHECK(BOX[23 + i] == 0x7272727272727200UL + (u64)i, 0x3A); /* red zone */
///     }
///     CHECK((BOX[40] & 0xFFFF) == 0x7F80, 0x3B); /* MXCSR */
///     {
///         int i;
///         for (i = 0; i < 16; i++)
///             CHECK(BOX[64 + 2 * i] == 0x1000UL + (u64)i && BOX[65 + 2 * i] == 0x1000UL + (u64)i,
///                   0x3C);
///     }
///     die(0x2A);
/// }
///
/// __attribute__((naked, section(".text.start"))) void _start(void)
/// {
///     __asm__ volatile("and $-16, %rsp\n\tcall main_\n\tud2");
/// }
/// ```
///
/// Paired with [`crate::proc::spawn::self_test_linux_signal_from_interrupt`]. Tagged
/// `ELFOSABI_GNU` so `spawn_process` routes it through the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_signal_from_interrupt_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const CODE: [u8; 2042] = [
        0x48, 0x83, 0xE4, 0xF0, 0xE8, 0xF4, 0x03, 0x00, 0x00, 0x0F, 0x0B, 0x0F, 0x0B, 0x53, 0x48,
        0x63, 0xFF, 0xBB, 0xE7, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xD8, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xEB, 0xDF, 0x0F, 0xAE, 0x5C, 0x24, 0xFC,
        0x8B, 0x44, 0x24, 0xFC, 0x48, 0xA3, 0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0x8B, 0x82, 0xA8, 0x00, 0x00, 0x00, 0x48, 0xA3, 0x18, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0xC7, 0x44, 0x24, 0xF8, 0x80, 0x3F, 0x00, 0x00, 0x66, 0x0F, 0x76, 0xC0, 0x66, 0x0F,
        0x76, 0xC9, 0x66, 0x0F, 0x76, 0xD2, 0x66, 0x0F, 0x76, 0xDB, 0x66, 0x0F, 0x76, 0xE4, 0x66,
        0x0F, 0x76, 0xED, 0x66, 0x0F, 0x76, 0xF6, 0x66, 0x0F, 0x76, 0xFF, 0x66, 0x45, 0x0F, 0x76,
        0xC0, 0x66, 0x45, 0x0F, 0x76, 0xC9, 0x66, 0x45, 0x0F, 0x76, 0xD2, 0x66, 0x45, 0x0F, 0x76,
        0xDB, 0x66, 0x45, 0x0F, 0x76, 0xE4, 0x66, 0x45, 0x0F, 0x76, 0xED, 0x66, 0x45, 0x0F, 0x76,
        0xF6, 0x66, 0x45, 0x0F, 0x76, 0xFF, 0x0F, 0xAE, 0x54, 0x24, 0xF8, 0x48, 0xC7, 0xC1, 0xFF,
        0xFF, 0xFF, 0xFF, 0x49, 0xC7, 0xC3, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0xC7, 0xC2, 0xFF, 0xFF,
        0xFF, 0xFF, 0x48, 0xC7, 0xC6, 0xFF, 0xFF, 0xFF, 0xFF, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF,
        0xFF, 0x49, 0xC7, 0xC1, 0xFF, 0xFF, 0xFF, 0xFF, 0x49, 0xC7, 0xC2, 0xFF, 0xFF, 0xFF, 0xFF,
        0x48, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x02, 0x48, 0x83,
        0xC0, 0x01, 0x48, 0x89, 0x02, 0xC3, 0xB8, 0x0F, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x0F, 0x0B,
        0x0F, 0x0B, 0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x53, 0x55, 0x48, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x1D, 0x9F, 0x01, 0x00, 0x00, 0x48,
        0x89, 0x98, 0x00, 0x03, 0x00, 0x00, 0x48, 0x8D, 0x1D, 0xA5, 0x01, 0x00, 0x00, 0x48, 0x89,
        0x98, 0x08, 0x03, 0x00, 0x00, 0x48, 0xBB, 0x00, 0x72, 0x72, 0x72, 0x72, 0x72, 0x72, 0x72,
        0xB9, 0x10, 0x00, 0x00, 0x00, 0x48, 0x89, 0xCA, 0x48, 0xF7, 0xDA, 0x48, 0x8D, 0x2C, 0x0B,
        0x48, 0x89, 0x2C, 0xD4, 0xFF, 0xC9, 0x75, 0xEE, 0xC7, 0x80, 0x20, 0x03, 0x00, 0x00, 0x80,
        0x7F, 0x00, 0x00, 0x0F, 0xAE, 0x90, 0x20, 0x03, 0x00, 0x00, 0x48, 0xC7, 0xC2, 0x00, 0x10,
        0x00, 0x00, 0x66, 0x48, 0x0F, 0x6E, 0xC2, 0x66, 0x0F, 0x6C, 0xC0, 0x48, 0xFF, 0xC2, 0x66,
        0x48, 0x0F, 0x6E, 0xCA, 0x66, 0x0F, 0x6C, 0xC9, 0x48, 0xFF, 0xC2, 0x66, 0x48, 0x0F, 0x6E,
        0xD2, 0x66, 0x0F, 0x6C, 0xD2, 0x48, 0xFF, 0xC2, 0x66, 0x48, 0x0F, 0x6E, 0xDA, 0x66, 0x0F,
        0x6C, 0xDB, 0x48, 0xFF, 0xC2, 0x66, 0x48, 0x0F, 0x6E, 0xE2, 0x66, 0x0F, 0x6C, 0xE4, 0x48,
        0xFF, 0xC2, 0x66, 0x48, 0x0F, 0x6E, 0xEA, 0x66, 0x0F, 0x6C, 0xED, 0x48, 0xFF, 0xC2, 0x66,
        0x48, 0x0F, 0x6E, 0xF2, 0x66, 0x0F, 0x6C, 0xF6, 0x48, 0xFF, 0xC2, 0x66, 0x48, 0x0F, 0x6E,
        0xFA, 0x66, 0x0F, 0x6C, 0xFF, 0x48, 0xFF, 0xC2, 0x66, 0x4C, 0x0F, 0x6E, 0xC2, 0x66, 0x45,
        0x0F, 0x6C, 0xC0, 0x48, 0xFF, 0xC2, 0x66, 0x4C, 0x0F, 0x6E, 0xCA, 0x66, 0x45, 0x0F, 0x6C,
        0xC9, 0x48, 0xFF, 0xC2, 0x66, 0x4C, 0x0F, 0x6E, 0xD2, 0x66, 0x45, 0x0F, 0x6C, 0xD2, 0x48,
        0xFF, 0xC2, 0x66, 0x4C, 0x0F, 0x6E, 0xDA, 0x66, 0x45, 0x0F, 0x6C, 0xDB, 0x48, 0xFF, 0xC2,
        0x66, 0x4C, 0x0F, 0x6E, 0xE2, 0x66, 0x45, 0x0F, 0x6C, 0xE4, 0x48, 0xFF, 0xC2, 0x66, 0x4C,
        0x0F, 0x6E, 0xEA, 0x66, 0x45, 0x0F, 0x6C, 0xED, 0x48, 0xFF, 0xC2, 0x66, 0x4C, 0x0F, 0x6E,
        0xF2, 0x66, 0x45, 0x0F, 0x6C, 0xF6, 0x48, 0xFF, 0xC2, 0x66, 0x4C, 0x0F, 0x6E, 0xFA, 0x66,
        0x45, 0x0F, 0x6C, 0xFF, 0x48, 0xB9, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x49,
        0xBB, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x48, 0xBA, 0x33, 0x33, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x33, 0x48, 0xBE, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x48,
        0xBF, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x49, 0xB8, 0x66, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x66, 0x66, 0x49, 0xB9, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x49,
        0xBA, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x49, 0xBC, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x49, 0xBD, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x49,
        0xBE, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0x49, 0xBF, 0xCC, 0xCC, 0xCC, 0xCC,
        0xCC, 0xCC, 0xCC, 0xCC, 0x48, 0xBB, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0x48,
        0xBD, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0x48, 0xFF, 0x40, 0x10, 0x48, 0x83,
        0x38, 0x00, 0x75, 0x0A, 0x48, 0x81, 0x78, 0x10, 0x00, 0x00, 0x00, 0x40, 0x72, 0xEC, 0x48,
        0x89, 0x48, 0x40, 0x4C, 0x89, 0x58, 0x48, 0x48, 0x89, 0x50, 0x50, 0x48, 0x89, 0x70, 0x58,
        0x48, 0x89, 0x78, 0x60, 0x4C, 0x89, 0x40, 0x68, 0x4C, 0x89, 0x48, 0x70, 0x4C, 0x89, 0x50,
        0x78, 0x4C, 0x89, 0xA0, 0x80, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xA8, 0x88, 0x00, 0x00, 0x00,
        0x4C, 0x89, 0xB0, 0x90, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xB8, 0x98, 0x00, 0x00, 0x00, 0x48,
        0x89, 0x98, 0xA0, 0x00, 0x00, 0x00, 0x48, 0x89, 0xA8, 0xA8, 0x00, 0x00, 0x00, 0x0F, 0xAE,
        0x98, 0x40, 0x01, 0x00, 0x00, 0xF3, 0x0F, 0x7F, 0x80, 0x00, 0x02, 0x00, 0x00, 0xF3, 0x0F,
        0x7F, 0x88, 0x10, 0x02, 0x00, 0x00, 0xF3, 0x0F, 0x7F, 0x90, 0x20, 0x02, 0x00, 0x00, 0xF3,
        0x0F, 0x7F, 0x98, 0x30, 0x02, 0x00, 0x00, 0xF3, 0x0F, 0x7F, 0xA0, 0x40, 0x02, 0x00, 0x00,
        0xF3, 0x0F, 0x7F, 0xA8, 0x50, 0x02, 0x00, 0x00, 0xF3, 0x0F, 0x7F, 0xB0, 0x60, 0x02, 0x00,
        0x00, 0xF3, 0x0F, 0x7F, 0xB8, 0x70, 0x02, 0x00, 0x00, 0xF3, 0x44, 0x0F, 0x7F, 0x80, 0x80,
        0x02, 0x00, 0x00, 0xF3, 0x44, 0x0F, 0x7F, 0x88, 0x90, 0x02, 0x00, 0x00, 0xF3, 0x44, 0x0F,
        0x7F, 0x90, 0xA0, 0x02, 0x00, 0x00, 0xF3, 0x44, 0x0F, 0x7F, 0x98, 0xB0, 0x02, 0x00, 0x00,
        0xF3, 0x44, 0x0F, 0x7F, 0xA0, 0xC0, 0x02, 0x00, 0x00, 0xF3, 0x44, 0x0F, 0x7F, 0xA8, 0xD0,
        0x02, 0x00, 0x00, 0xF3, 0x44, 0x0F, 0x7F, 0xB0, 0xE0, 0x02, 0x00, 0x00, 0xF3, 0x44, 0x0F,
        0x7F, 0xB8, 0xF0, 0x02, 0x00, 0x00, 0xB9, 0x10, 0x00, 0x00, 0x00, 0x48, 0x89, 0xCA, 0x48,
        0xF7, 0xDA, 0x48, 0x8B, 0x1C, 0xD4, 0x48, 0x89, 0x9C, 0xC8, 0xB8, 0x00, 0x00, 0x00, 0xFF,
        0xC9, 0x75, 0xEA, 0xC7, 0x80, 0x20, 0x03, 0x00, 0x00, 0x80, 0x1F, 0x00, 0x00, 0x0F, 0xAE,
        0x90, 0x20, 0x03, 0x00, 0x00, 0x5D, 0x5B, 0x41, 0x5C, 0x41, 0x5D, 0x41, 0x5E, 0x41, 0x5F,
        0xC3, 0x48, 0x89, 0xF8, 0x48, 0x85, 0xD2, 0x74, 0x15, 0x48, 0x01, 0xFA, 0x48, 0x89, 0xF9,
        0x49, 0x89, 0xC8, 0x48, 0x83, 0xC1, 0x01, 0x41, 0x88, 0x30, 0x48, 0x39, 0xCA, 0x75, 0xF1,
        0xC3, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x55, 0x53, 0x48, 0x83, 0xEC, 0x40, 0x41, 0xBA,
        0x32, 0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0x48, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xB8, 0x09, 0x00,
        0x00, 0x00, 0xBE, 0x00, 0x10, 0x00, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48,
        0x39, 0xF8, 0x74, 0x0A, 0xBF, 0x30, 0x00, 0x00, 0x00, 0xE8, 0xC7, 0xFB, 0xFF, 0xFF, 0x48,
        0x8D, 0x5C, 0x24, 0x20, 0xBA, 0x20, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48,
        0x89, 0xDF, 0xE8, 0x82, 0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0x05, 0xD3, 0xFB, 0xFF, 0xFF, 0x48,
        0x89, 0x44, 0x24, 0x20, 0x48, 0xC7, 0x44, 0x24, 0x28, 0x04, 0x00, 0x00, 0x04, 0x48, 0x8D,
        0x05, 0x7D, 0xFC, 0xFF, 0xFF, 0x48, 0x89, 0x44, 0x24, 0x30, 0x41, 0xBA, 0x08, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0D,
        0x00, 0x00, 0x00, 0xBF, 0x0E, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xDE, 0x0F, 0x05, 0xBD, 0x03, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x0F, 0x85, 0xEC, 0x00,
        0x00, 0x00, 0x48, 0xBB, 0x00, 0x04, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x49, 0x89, 0xE5,
        0x41, 0xBC, 0x26, 0x00, 0x00, 0x00, 0x49, 0xBE, 0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x04, 0x24, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24, 0x08, 0x00,
        0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24, 0x10, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44,
        0x24, 0x18, 0x50, 0xC3, 0x00, 0x00, 0x48, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83, 0xC0, 0x08, 0x48, 0x39, 0xD8,
        0x75, 0xF0, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE0, 0x48, 0x89,
        0xD7, 0x4C, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x75, 0xE8, 0xC9, 0xFB, 0xFF,
        0xFF, 0x49, 0x8B, 0x06, 0x48, 0x83, 0xF8, 0x01, 0x77, 0x05, 0x83, 0xED, 0x01, 0x75, 0x89,
        0x48, 0xA1, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01, 0x75,
        0x5C, 0x48, 0xA1, 0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01,
        0x76, 0x56, 0x48, 0xB9, 0x18, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x11,
        0x48, 0xA1, 0x00, 0x03, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xC2, 0x72, 0x12,
        0x48, 0x8B, 0x11, 0x48, 0xA1, 0x08, 0x03, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39,
        0xD0, 0x73, 0x32, 0xBF, 0x35, 0x00, 0x00, 0x00, 0xE8, 0x6F, 0xFA, 0xFF, 0xFF, 0xBF, 0x31,
        0x00, 0x00, 0x00, 0xE8, 0x65, 0xFA, 0xFF, 0xFF, 0xBF, 0x32, 0x00, 0x00, 0x00, 0xE8, 0x5B,
        0xFA, 0xFF, 0xFF, 0xBF, 0x33, 0x00, 0x00, 0x00, 0xE8, 0x51, 0xFA, 0xFF, 0xFF, 0xBF, 0x34,
        0x00, 0x00, 0x00, 0xE8, 0x47, 0xFA, 0xFF, 0xFF, 0x48, 0xA1, 0x08, 0x00, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x48, 0x3D, 0x80, 0x1F, 0x00, 0x00, 0x75, 0x3C, 0x48, 0xA1, 0x40, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x48, 0x39, 0xD0, 0x75, 0x19, 0x48, 0xA1, 0x48, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0xBA, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x48, 0x39, 0xD0, 0x74,
        0x14, 0xBF, 0x37, 0x00, 0x00, 0x00, 0xE8, 0xF9, 0xF9, 0xFF, 0xFF, 0xBF, 0x36, 0x00, 0x00,
        0x00, 0xE8, 0xEF, 0xF9, 0xFF, 0xFF, 0x48, 0xA1, 0x50, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0xBA, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x48, 0x39, 0xD0, 0x75,
        0x19, 0x48, 0xA1, 0x58, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x44, 0x44,
        0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x48, 0x39, 0xD0, 0x74, 0x0A, 0xBF, 0x38, 0x00, 0x00,
        0x00, 0xE8, 0xB3, 0xF9, 0xFF, 0xFF, 0x48, 0xA1, 0x60, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0xBA, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x48, 0x39, 0xD0, 0x75,
        0xDD, 0x48, 0xA1, 0x68, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x66, 0x66,
        0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x48, 0x39, 0xD0, 0x75, 0xC4, 0x48, 0xA1, 0x70, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
        0x77, 0x48, 0x39, 0xD0, 0x75, 0xAB, 0x48, 0xA1, 0x78, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0xBA, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x48, 0x39, 0xD0, 0x75,
        0x92, 0x48, 0xA1, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x48, 0x39, 0xD0, 0x75, 0x19, 0x48, 0xA1, 0x88, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
        0xAA, 0x48, 0x39, 0xD0, 0x74, 0x0A, 0xBF, 0x39, 0x00, 0x00, 0x00, 0xE8, 0x13, 0xF9, 0xFF,
        0xFF, 0x48, 0xA1, 0x90, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0xBB, 0xBB,
        0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0x48, 0x39, 0xD0, 0x75, 0xDD, 0x48, 0xA1, 0x98, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC,
        0xCC, 0x48, 0x39, 0xD0, 0x75, 0xC4, 0x48, 0xA1, 0xA0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0xBA, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0x48, 0x39, 0xD0, 0x75,
        0xAB, 0x48, 0xA1, 0xA8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0xEE, 0xEE,
        0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0x48, 0x39, 0xD0, 0x75, 0x92, 0x48, 0xB8, 0x01, 0x72,
        0x72, 0x72, 0x72, 0x72, 0x72, 0x72, 0x48, 0xB9, 0xB8, 0x70, 0x6C, 0x6C, 0xBC, 0x6C, 0x6C,
        0x6C, 0x48, 0xBA, 0x11, 0x72, 0x72, 0x72, 0x72, 0x72, 0x72, 0x72, 0x48, 0x8D, 0x34, 0xC1,
        0x48, 0x8B, 0x36, 0x48, 0x39, 0xC6, 0x75, 0x32, 0x48, 0x83, 0xC0, 0x01, 0x48, 0x39, 0xD0,
        0x75, 0xEB, 0x48, 0xA1, 0x40, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x10,
        0x00, 0x00, 0x48, 0xB9, 0x00, 0x02, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x66, 0x3D, 0x80,
        0x7F, 0x74, 0x25, 0xBF, 0x3B, 0x00, 0x00, 0x00, 0xE8, 0x53, 0xF8, 0xFF, 0xFF, 0xBF, 0x3A,
        0x00, 0x00, 0x00, 0xE8, 0x49, 0xF8, 0xFF, 0xFF, 0x48, 0x83, 0xC1, 0x10, 0x48, 0x83, 0xC2,
        0x01, 0x48, 0x81, 0xFA, 0x10, 0x10, 0x00, 0x00, 0x74, 0x1B, 0x48, 0x8B, 0x01, 0x48, 0x39,
        0xD0, 0x75, 0x09, 0x48, 0x8B, 0x41, 0x08, 0x48, 0x39, 0xD0, 0x74, 0xDE, 0xBF, 0x3C, 0x00,
        0x00, 0x00, 0xE8, 0x1D, 0xF8, 0xFF, 0xFF, 0xBF, 0x2A, 0x00, 0x00, 0x00, 0xE8, 0x13, 0xF8,
        0xFF, 0xFF,
    ];
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size = CODE.len() as u64;
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
    buf[cs..(cs + CODE.len())].copy_from_slice(&CODE);

    buf
}

/// Build a Linux-ABI ELF that checks signals are per thread: masks, queues and
/// thread-directed sends, with a real second thread (`clone(CLONE_THREAD)`).
///
/// The main thread blocks `SIGUSR1` and `SIGUSR2` and starts a worker, which
/// must start with that mask and then unblocks both for itself alone -- the
/// main thread's mask must not change. A `kill` of the process must run its
/// handler on the worker, the one thread not blocking it; a `tgkill` to the
/// main thread must wait on the main thread's own queue (the worker, though it
/// does not block `SIGUSR2`, must not take it) until the main thread takes it
/// with `rt_sigtimedwait` (`si_code` `SI_TKILL`); a `tgkill` to the worker runs
/// the handler there. The worker is joined through its `CLONE_CHILD_CLEARTID`
/// word. Exits 0x2A on success, else the failed check's code (0x30-0x4A). The
/// same program exits 0x2A on Linux 6.6.
///
/// Built with gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, in WSL, linked by `posix_timers.ld`
/// (see [`build_linux_posix_timers_test_elf`]):
///
/// ```text
/// gcc -O1 -std=gnu11 -ffreestanding -fno-builtin -fno-stack-protec
///     tor -fno-asynchronous-unwind-tables -fno-unwind-tables -fcf-protec
///     tion=none -fno-jump-tables -mgeneral-regs-only -fPIE -fvisibility=
///     hidden -nostdlib -static -Wall -Wextra -Wl,-T,posix_timers.ld -Wl,
///     --build-id=none -Wl,-z,norelro -Wl,-z,noexecstack
///     -o threadsig.elf threadsig.c
/// objcopy -O binary -j .text threadsig.elf threadsig.bin
/// ```
///
/// `threadsig.c`:
///
/// ```c
/// /* Ring-3 test of per-thread signal state through the Linux ABI
///  * (spawn::self_test_linux_thread_signals). Freestanding: raw syscalls and a
///  * raw clone(CLONE_THREAD).
///  *
///  * The main thread blocks SIGUSR1 and SIGUSR2 and starts a worker, which must
///  * inherit that mask, then unblocks both for itself alone. A kill() to the
///  * process must be taken by the worker (the one thread not blocking it); a
///  * tgkill() to the main thread must wait on the main thread's own queue -- the
///  * worker must not take it -- until the main thread takes it with
///  * rt_sigtimedwait; a tgkill() to the worker runs the handler on the worker.
///  * Exits 0x2A when every check passes, otherwise the failed check's code. */
///
/// typedef unsigned long u64;
/// typedef long i64;
/// typedef int i32;
/// typedef unsigned int u32;
///
/// #define SHARED 0x5000000000UL
/// #define BOX ((volatile u64 *)SHARED)
///
/// static inline long sc6(long n, long a, long b, long c, long d, long e, long f)
/// {
///     register long r10 __asm__("r10") = d;
///     register long r8 __asm__("r8") = e;
///     register long r9 __asm__("r9") = f;
///     long ret;
///     __asm__ volatile("syscall"
///                      : "=a"(ret)
///                      : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
///                      : "rcx", "r11", "memory");
///     return ret;
/// }
/// #define sc(n, a, b, c, d) sc6((n), (long)(a), (long)(b), (long)(c), (long)(d), 0, 0)
///
/// static void __attribute__((noreturn)) die(int code)
/// {
///     for (;;)
///         sc(231, code, 0, 0, 0); /* exit_group */
/// }
/// #define CHECK(cond, code)  \
///     do {                   \
///         if (!(cond))       \
///             die(code);     \
///     } while (0)
///
/// void *memset(void *d, int c, u64 n)
/// {
///     volatile unsigned char *p = d;
///     while (n--)
///         *p++ = (unsigned char)c;
///     return d;
/// }
///
/// struct ts { i64 sec, nsec; };
/// struct kact { u64 handler, flags, restorer, mask; };
/// struct si { i32 signo, err, code, pad; u32 a, b; u64 value; u64 rest[12]; };
///
/// #define BIT(s) (1UL << ((s) - 1))
/// #define SIGUSR1 10
/// #define SIGUSR2 12
///
/// /* Mailbox (8-byte words at SHARED):
///  *   [10] the worker's tid   [11] the mask the worker started with
///  *   [12] worker ready       [13] main says the worker may finish
///  *   [19] handler runs       [20+i] the tid each ran on   [30+i] its signal
///  *   [40] the worker's clear-tid word (as an int) */
///
/// static void msleep(long ms)
/// {
///     struct ts t = { ms / 1000, (ms % 1000) * 1000000 };
///     sc(35, &t, 0, 0, 0);
/// }
///
/// static void handler(int sig, void *info, void *uc)
/// {
///     u64 n = BOX[19];
///     (void)info;
///     (void)uc;
///     if (n < 8) {
///         BOX[20 + n] = (u64)sc(186, 0, 0, 0, 0); /* gettid */
///         BOX[30 + n] = (u64)sig;
///     }
///     BOX[19] = n + 1;
/// }
///
/// __attribute__((naked)) static void restorer(void)
/// {
///     __asm__ volatile("mov $15, %eax\n\tsyscall\n\tud2");
/// }
///
/// static void worker(void)
/// {
///     u64 old = 0;
///     u64 unblock = BIT(SIGUSR1) | BIT(SIGUSR2);
///     BOX[10] = (u64)sc(186, 0, 0, 0, 0);
///     sc(14, 0, 0, &old, 8);   /* rt_sigprocmask(SIG_BLOCK, NULL, &old): read */
///     BOX[11] = old;
///     sc(14, 1, &unblock, 0, 8); /* SIG_UNBLOCK, for this thread alone */
///     BOX[12] = 1;
///     while (BOX[13] == 0)
///         msleep(1);
/// }
///
/// /* clone(CLONE_VM|FS|FILES|SIGHAND|THREAD|SYSVSEM|PARENT_SETTID|CHILD_CLEARTID)
///  * on `stack_top`; the child calls `fn` (passed in r12, which it inherits) and
///  * exits. */
/// static long spawn_thread(void (*fn)(void), u64 stack_top, i32 *tid_word)
/// {
///     long ret;
///     register long r10 __asm__("r10") = (long)tid_word; /* child_tid */
///     register long r8 __asm__("r8") = 0;                /* tls */
///     register long r12 __asm__("r12") = (long)fn;
///     __asm__ volatile("syscall\n\t"
///                      "test %%rax, %%rax\n\t"
///                      "jnz 1f\n\t"
///                      "xor %%ebp, %%ebp\n\t"
///                      "call *%%r12\n\t"
///                      "mov $60, %%eax\n\t" /* exit: this thread only */
///                      "xor %%edi, %%edi\n\t"
///                      "syscall\n\t"
///                      "ud2\n\t"
///                      "1:\n\t"
///                      : "=a"(ret)
///                      : "a"(56), "D"(0x3D0F00L), "S"(stack_top), "d"(tid_word), "r"(r10),
///                        "r"(r8), "r"(r12)
///                      : "rcx", "r11", "memory");
///     return ret;
/// }
///
/// __attribute__((used, noreturn)) void main_(void)
/// {
///     struct kact act;
///     struct si info;
///     struct ts zero = { 0, 0 };
///     u64 block = BIT(SIGUSR1) | BIT(SIGUSR2);
///     u64 mine = 0;
///     long worker_tid, me, pid, i;
///     u64 stack;
///     volatile i32 *tid_word = (volatile i32 *)(SHARED + 40 * 8);
///
///     CHECK(sc6(9, SHARED, 4096, 3, 0x32, -1, 0) == (long)SHARED, 0x30); /* mailbox */
///     for (i = 0; i < 64; i++)
///         BOX[i] = 0;
///     memset(&act, 0, sizeof act);
///     act.handler = (u64)handler;
///     act.flags = 4 | 0x04000000; /* SA_SIGINFO | SA_RESTORER */
///     act.restorer = (u64)restorer;
///     CHECK(sc(13, SIGUSR1, &act, 0, 8) == 0 && sc(13, SIGUSR2, &act, 0, 8) == 0, 0x31);
///     CHECK(sc(14, 0, &block, 0, 8) == 0, 0x32); /* main blocks both */
///
///     stack = (u64)sc6(9, 0, 65536, 3, 0x22, -1, 0);
///     CHECK(stack < (u64)-4096, 0x33);
///     worker_tid = spawn_thread(worker, stack + 65536, (i32 *)tid_word);
///     CHECK(worker_tid > 0, 0x34);
///     for (i = 0; i < 5000 && BOX[12] == 0; i++)
///         msleep(1);
///     CHECK(BOX[12] == 1 && (long)BOX[10] == worker_tid, 0x35);
///
///     /* The worker started with the main thread's mask... */
///     CHECK((BOX[11] & block) == block, 0x40);
///     /* ...and unblocking for itself left the main thread's alone. */
///     CHECK(sc(14, 0, 0, &mine, 8) == 0 && (mine & block) == block, 0x41);
///
///     /* kill(): only the worker can take it. */
///     pid = sc(39, 0, 0, 0, 0);
///     me = sc(186, 0, 0, 0, 0);
///     CHECK(sc(62, pid, SIGUSR1, 0, 0) == 0, 0x42);
///     for (i = 0; i < 5000 && BOX[19] < 1; i++)
///         msleep(1);
///     CHECK(BOX[19] == 1 && (long)BOX[20] == worker_tid && BOX[30] == SIGUSR1, 0x43);
///
///     /* tgkill() to the main thread, which blocks it: it waits for the main
///      * thread alone, though the worker does not block SIGUSR2. */
///     CHECK(sc(234, pid, me, SIGUSR2, 0) == 0, 0x44);
///     msleep(30);
///     CHECK(BOX[19] == 1, 0x45); /* the worker did not take it */
///     mine = 0;
///     CHECK(sc(127, &mine, 8, 0, 0) == 0 && (mine & BIT(SIGUSR2)), 0x46); /* rt_sigpending */
///     memset(&info, 0, sizeof info);
///     CHECK(sc(128, &(u64){ BIT(SIGUSR2) }, &info, &zero, 8) == SIGUSR2 && info.code == -6, 0x47);
///
///     /* tgkill() to the worker runs the handler there. */
///     CHECK(sc(234, pid, worker_tid, SIGUSR2, 0) == 0, 0x48);
///     for (i = 0; i < 5000 && BOX[19] < 2; i++)
///         msleep(1);
///     CHECK(BOX[19] == 2 && (long)BOX[21] == worker_tid && BOX[31] == SIGUSR2, 0x49);
///
///     /* Let the worker finish, and join it through its clear-tid word. */
///     BOX[13] = 1;
///     for (i = 0; i < 5000 && *tid_word != 0; i++) {
///         i32 v = *tid_word;
///         if (v != 0) {
///             struct ts t = { 0, 1000000 };
///             sc6(202, (long)tid_word, 0, v, (long)&t, 0, 0); /* futex wait, 1 ms */
///         }
///     }
///     CHECK(*tid_word == 0, 0x4A);
///     die(0x2A);
/// }
///
/// __attribute__((naked, section(".text.start"))) void _start(void)
/// {
///     __asm__ volatile("and $-16, %rsp\n\tcall main_\n\tud2");
/// }
/// ```
///
/// Paired with [`crate::proc::spawn::self_test_linux_thread_signals`]. Tagged
/// `ELFOSABI_GNU` so `spawn_process` routes it through the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_thread_signals_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const CODE: [u8; 1881] = [
        0x48, 0x83, 0xE4, 0xF0, 0xE8, 0xE6, 0x01, 0x00, 0x00, 0x0F, 0x0B, 0x0F, 0x0B, 0x53, 0x48,
        0x63, 0xFF, 0xBB, 0xE7, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xD8, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xEB, 0xDF, 0x48, 0xBA, 0xCF, 0xF7, 0x53,
        0xE3, 0xA5, 0x9B, 0xC4, 0x20, 0x48, 0x89, 0xF8, 0x48, 0xF7, 0xEA, 0x48, 0xC1, 0xFA, 0x07,
        0x48, 0x89, 0xF8, 0x48, 0xC1, 0xF8, 0x3F, 0x48, 0x29, 0xC2, 0x48, 0x89, 0x54, 0x24, 0xF0,
        0x48, 0x69, 0xD2, 0xE8, 0x03, 0x00, 0x00, 0x48, 0x29, 0xD7, 0x48, 0x69, 0xFF, 0x40, 0x42,
        0x0F, 0x00, 0x48, 0x89, 0x7C, 0x24, 0xF8, 0x48, 0x8D, 0x7C, 0x24, 0xF0, 0x41, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x23, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05,
        0xC3, 0x55, 0x53, 0x48, 0xB8, 0x98, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B,
        0x18, 0x48, 0x83, 0xFB, 0x07, 0x77, 0x4A, 0x89, 0xFD, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0xBA, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6, 0x0F, 0x05,
        0x48, 0x8D, 0x0C, 0xDD, 0xA0, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x48, 0x8D, 0x34, 0x11, 0x48, 0x89, 0x06, 0x48, 0x8D, 0x54, 0x11, 0x50,
        0x48, 0x63, 0xC5, 0x48, 0x89, 0x02, 0x48, 0x8D, 0x43, 0x01, 0x48, 0xA3, 0x98, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x5B, 0x5D, 0xC3, 0xB8, 0x0F, 0x00, 0x00, 0x00, 0x0F, 0x05,
        0x0F, 0x0B, 0x0F, 0x0B, 0x55, 0x53, 0x48, 0x83, 0xEC, 0x10, 0x48, 0xC7, 0x44, 0x24, 0x08,
        0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x04, 0x24, 0x00, 0x0A, 0x00, 0x00, 0x41, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xBB, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xBA, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDF, 0x48, 0x89,
        0xDE, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0xA3, 0x50, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x8D, 0x54, 0x24, 0x08, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, 0xBD, 0x0E, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xE8, 0x0F, 0x05, 0x48, 0x8B, 0x44, 0x24, 0x08, 0x48, 0xA3, 0x58,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0xE6, 0xBF, 0x01, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xE8, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0xB8, 0x60, 0x00, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x48, 0xC7, 0x00, 0x01, 0x00, 0x00, 0x00, 0x48, 0xA1, 0x68, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x75, 0x1C, 0x48, 0xBB, 0x68, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0x00, 0xE8, 0x75, 0xFE, 0xFF, 0xFF,
        0x48, 0x8B, 0x03, 0x48, 0x85, 0xC0, 0x74, 0xEE, 0x48, 0x83, 0xC4, 0x10, 0x5B, 0x5D, 0xC3,
        0x48, 0x89, 0xF8, 0x48, 0x85, 0xD2, 0x74, 0x15, 0x48, 0x01, 0xFA, 0x48, 0x89, 0xF9, 0x49,
        0x89, 0xC8, 0x48, 0x83, 0xC1, 0x01, 0x41, 0x88, 0x30, 0x48, 0x39, 0xCA, 0x75, 0xF1, 0xC3,
        0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x55, 0x53, 0x48, 0x81, 0xEC, 0xE0, 0x00,
        0x00, 0x00, 0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24,
        0x30, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24, 0x20, 0x00, 0x0A, 0x00, 0x00, 0x48,
        0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x32, 0x00, 0x00, 0x00, 0x49,
        0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0x48, 0xBF, 0x00,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x10,
        0x00, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x89, 0xFA, 0x48, 0xB9, 0x00,
        0x02, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xF8, 0x0F, 0x85, 0x9A, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x02, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83, 0xC2, 0x08, 0x48, 0x39, 0xCA,
        0x75, 0xF0, 0x48, 0x8D, 0x9C, 0x24, 0xB8, 0x00, 0x00, 0x00, 0xBA, 0x20, 0x00, 0x00, 0x00,
        0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDF, 0xE8, 0x3F, 0xFF, 0xFF, 0xFF, 0x48, 0x8D,
        0x05, 0xFE, 0xFD, 0xFF, 0xFF, 0x48, 0x89, 0x84, 0x24, 0xB8, 0x00, 0x00, 0x00, 0x48, 0xC7,
        0x84, 0x24, 0xC0, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x04, 0x48, 0x8D, 0x05, 0x53, 0xFE,
        0xFF, 0xFF, 0x48, 0x89, 0x84, 0x24, 0xC8, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDE, 0x41, 0xBA,
        0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x0D, 0x00, 0x00, 0x00, 0xBF, 0x0A, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x11, 0xB8, 0x0D, 0x00, 0x00, 0x00, 0xBF, 0x0C,
        0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x14, 0xBF, 0x31, 0x00, 0x00, 0x00,
        0xE8, 0x0B, 0xFD, 0xFF, 0xFF, 0xBF, 0x30, 0x00, 0x00, 0x00, 0xE8, 0x01, 0xFD, 0xFF, 0xFF,
        0x48, 0x8D, 0x74, 0x24, 0x20, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0E,
        0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x3B, 0x41, 0xBA,
        0x22, 0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00,
        0x01, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x3D, 0xFF, 0xEF, 0xFF, 0xFF,
        0x76, 0x14, 0xBF, 0x33, 0x00, 0x00, 0x00, 0xE8, 0x9B, 0xFC, 0xFF, 0xFF, 0xBF, 0x32, 0x00,
        0x00, 0x00, 0xE8, 0x91, 0xFC, 0xFF, 0xFF, 0x48, 0x8D, 0xB0, 0x00, 0x00, 0x01, 0x00, 0x49,
        0xBA, 0x40, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x4C, 0x8D, 0x25, 0x78, 0xFD, 0xFF, 0xFF, 0xB8, 0x38, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x0F,
        0x3D, 0x00, 0x4C, 0x89, 0xD2, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x10, 0x31, 0xED, 0x41,
        0xFF, 0xD4, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x31, 0xFF, 0x0F, 0x05, 0x0F, 0x0B, 0x49, 0x89,
        0xC4, 0xBB, 0x88, 0x13, 0x00, 0x00, 0x48, 0xBD, 0x60, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x85, 0xC0, 0x7E, 0x42, 0x48, 0x8B, 0x45, 0x00, 0x48, 0x85, 0xC0, 0x75, 0x10,
        0xBF, 0x01, 0x00, 0x00, 0x00, 0xE8, 0x4F, 0xFC, 0xFF, 0xFF, 0x48, 0x83, 0xEB, 0x01, 0x75,
        0xE7, 0x48, 0xA1, 0x60, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01,
        0x75, 0x0F, 0x48, 0xA1, 0x50, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x4C, 0x39, 0xE0,
        0x74, 0x14, 0xBF, 0x35, 0x00, 0x00, 0x00, 0xE8, 0xF6, 0xFB, 0xFF, 0xFF, 0xBF, 0x34, 0x00,
        0x00, 0x00, 0xE8, 0xEC, 0xFB, 0xFF, 0xFF, 0x48, 0xA1, 0x58, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0xF7, 0xD0, 0x48, 0x85, 0x44, 0x24, 0x20, 0x75, 0x4A, 0x48, 0x8D, 0x6C,
        0x24, 0x18, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0E, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xF7, 0x48, 0x89, 0xEA, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x12, 0x48, 0x8B,
        0x44, 0x24, 0x20, 0x48, 0x89, 0xC2, 0x48, 0x23, 0x54, 0x24, 0x18, 0x48, 0x39, 0xD0, 0x74,
        0x14, 0xBF, 0x41, 0x00, 0x00, 0x00, 0xE8, 0x8E, 0xFB, 0xFF, 0xFF, 0xBF, 0x40, 0x00, 0x00,
        0x00, 0xE8, 0x84, 0xFB, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8,
        0x27, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x89, 0xC3,
        0xB8, 0xBA, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x49, 0x89, 0xC7, 0xB8, 0x3E, 0x00, 0x00, 0x00,
        0xBE, 0x0A, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDF, 0x0F, 0x05, 0x41, 0xBD, 0x88, 0x13, 0x00,
        0x00, 0x49, 0xBE, 0x98, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x75,
        0x41, 0x49, 0x8B, 0x06, 0x48, 0x85, 0xC0, 0x75, 0x10, 0xBF, 0x01, 0x00, 0x00, 0x00, 0xE8,
        0x47, 0xFB, 0xFF, 0xFF, 0x49, 0x83, 0xED, 0x01, 0x75, 0xE8, 0x48, 0xA1, 0x98, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01, 0x75, 0x0F, 0x48, 0xA1, 0xA0, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x4C, 0x39, 0xE0, 0x74, 0x14, 0xBF, 0x43, 0x00, 0x00,
        0x00, 0xE8, 0xEE, 0xFA, 0xFF, 0xFF, 0xBF, 0x42, 0x00, 0x00, 0x00, 0xE8, 0xE4, 0xFA, 0xFF,
        0xFF, 0x48, 0xA1, 0xF0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x0A,
        0x75, 0xDC, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xEA, 0x00, 0x00, 0x00, 0xBA, 0x0C, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xDF, 0x4C, 0x89, 0xFE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x44,
        0x00, 0x00, 0x00, 0xE8, 0xA1, 0xFA, 0xFF, 0xFF, 0xBF, 0x1E, 0x00, 0x00, 0x00, 0xE8, 0xC1,
        0xFA, 0xFF, 0xFF, 0x48, 0xA1, 0x98, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83,
        0xF8, 0x01, 0x75, 0x45, 0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00, 0x00, 0x41, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x7F, 0x00, 0x00, 0x00, 0xBE, 0x08, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x48, 0x89, 0xEF, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x07, 0xF6, 0x44, 0x24, 0x19,
        0x08, 0x75, 0x14, 0xBF, 0x46, 0x00, 0x00, 0x00, 0xE8, 0x42, 0xFA, 0xFF, 0xFF, 0xBF, 0x45,
        0x00, 0x00, 0x00, 0xE8, 0x38, 0xFA, 0xFF, 0xFF, 0x48, 0x8D, 0x6C, 0x24, 0x38, 0xBA, 0x80,
        0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0xE8, 0xE5, 0xFB, 0xFF,
        0xFF, 0x48, 0x8D, 0x54, 0x24, 0x28, 0x48, 0xC7, 0x84, 0x24, 0xD8, 0x00, 0x00, 0x00, 0x00,
        0x08, 0x00, 0x00, 0x48, 0x8D, 0xBC, 0x24, 0xD8, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x08, 0x00,
        0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8,
        0x80, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0x0C, 0x75, 0x07,
        0x83, 0x7C, 0x24, 0x40, 0xFA, 0x74, 0x0A, 0xBF, 0x47, 0x00, 0x00, 0x00, 0xE8, 0xD5, 0xF9,
        0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xEA, 0x00, 0x00, 0x00, 0xBA, 0x0C, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xDF, 0x4C, 0x89, 0xE6, 0x0F, 0x05, 0xBB, 0x88, 0x13, 0x00, 0x00, 0x48, 0xBD,
        0x98, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x75, 0x43, 0x48, 0x8B,
        0x45, 0x00, 0x48, 0x83, 0xF8, 0x01, 0x77, 0x10, 0xBF, 0x01, 0x00, 0x00, 0x00, 0xE8, 0xB3,
        0xF9, 0xFF, 0xFF, 0x48, 0x83, 0xEB, 0x01, 0x75, 0xE6, 0x48, 0xA1, 0x98, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x02, 0x75, 0x0F, 0x48, 0xA1, 0xA8, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x4C, 0x39, 0xE0, 0x74, 0x14, 0xBF, 0x49, 0x00, 0x00, 0x00,
        0xE8, 0x5A, 0xF9, 0xFF, 0xFF, 0xBF, 0x48, 0x00, 0x00, 0x00, 0xE8, 0x50, 0xF9, 0xFF, 0xFF,
        0x48, 0xA1, 0xF8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x0C, 0x75,
        0xDC, 0x48, 0xB8, 0x68, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x00, 0x01,
        0x00, 0x00, 0x00, 0xBB, 0x88, 0x13, 0x00, 0x00, 0x48, 0xBF, 0x40, 0x01, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x4C, 0x8D, 0x54, 0x24, 0x08, 0xBD, 0xCA, 0x00, 0x00, 0x00, 0xEB, 0x06,
        0x48, 0x83, 0xEB, 0x01, 0x74, 0x39, 0x8B, 0x07, 0x85, 0xC0, 0x74, 0x33, 0x8B, 0x07, 0x85,
        0xC0, 0x74, 0xEE, 0x48, 0xC7, 0x44, 0x24, 0x08, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44,
        0x24, 0x10, 0x40, 0x42, 0x0F, 0x00, 0x48, 0x63, 0xD0, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xE8, 0x0F,
        0x05, 0xEB, 0xC1, 0xA1, 0x40, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x85, 0xC0, 0x74,
        0x0A, 0xBF, 0x4A, 0x00, 0x00, 0x00, 0xE8, 0xBE, 0xF8, 0xFF, 0xFF, 0xBF, 0x2A, 0x00, 0x00,
        0x00, 0xE8, 0xB4, 0xF8, 0xFF, 0xFF,
    ];
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size = CODE.len() as u64;
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
    buf[cs..(cs + CODE.len())].copy_from_slice(&CODE);

    buf
}

/// Build a Linux-ABI ring-3 test of the alternate signal stack, a freestanding C
/// program: `sigaltstack`'s answers (`ENOMEM`, `EINVAL`, the old stack reported
/// only on success, `SS_ONSTACK` and `SS_DISABLE|SS_AUTODISARM` as requests); an
/// `SA_ONSTACK` handler on the stack, where a query says `SS_ONSTACK` and a
/// change is `EPERM`, and one without it off the stack; the frame's `uc_stack`
/// and what `rt_sigreturn` restores from it; `SS_AUTODISARM`; a signal nested on
/// the stack; `AT_MINSIGSTKSZ` covering the frame; a stack overflow's `SIGSEGV`
/// handled on the stack and recovered from; the main stack growing past 80 KiB;
/// a new thread with no stack and a forked child keeping it; and `SIGSEGV`
/// ending a child whose frame does not fit the stack, or that faults with
/// `SIGSEGV` blocked. Exits `0x2A` when every check passes (it does on Linux
/// 6.6, in WSL), otherwise the failed check's code.
///
/// Built with gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, in WSL, linked by `posix_timers.ld`
/// (see [`build_linux_posix_timers_test_elf`]):
///
/// ```text
/// gcc -O1 -std=gnu11 -ffreestanding -fno-builtin -fno-stack-protec
///     tor -fno-asynchronous-unwind-tables -fno-unwind-tables -fcf-protec
///     tion=none -fno-jump-tables -mgeneral-regs-only -fPIE -fvisibility=
///     hidden -nostdlib -static -Wall -Wextra -Wl,-T,posix_timers.ld -Wl,
///     --build-id=none -Wl,-z,norelro -Wl,-z,noexecstack
///     -o sigaltstk.elf sigaltstk.c
/// objcopy -O binary -j .text sigaltstk.elf sigaltstk.bin
/// ```
///
/// `sigaltstk.c`:
///
/// ```c
/// /* Ring-3 test of the alternate signal stack through the Linux ABI
///  * (spawn::self_test_linux_sigaltstack). Freestanding: raw syscalls.
///  *
///  * sigaltstack's answers (ENOMEM, EINVAL, the old stack only on success,
///  * SS_ONSTACK and SS_DISABLE|SS_AUTODISARM as requests); an SA_ONSTACK handler
///  * runs on the stack, where a query says SS_ONSTACK and a change is EPERM, and
///  * one without SA_ONSTACK does not; the frame's uc_stack and what rt_sigreturn
///  * restores from it; SS_AUTODISARM disarms while a handler runs; a signal
///  * nested on the stack stays on it; the frame fits in AT_MINSIGSTKSZ; a stack
///  * overflow's SIGSEGV is handled on the stack and the program recovers; the
///  * main stack grows past 80 KiB; a new thread starts with no stack and a forked
///  * child keeps it; a frame that does not fit the stack, and a fault while
///  * SIGSEGV is blocked, end the process by SIGSEGV. Exits 0x2A when every check
///  * passes, otherwise the failed check's code. */
///
/// typedef unsigned long u64;
/// typedef long i64;
/// typedef int i32;
/// typedef unsigned int u32;
///
/// #define SHARED 0x5000000000UL /* mailbox, one page */
/// #define ALT 0x5000100000UL    /* the alternate stack */
/// #define ALT_SIZE 0x10000UL
/// #define LOW 0x5000200000UL    /* a small stack, nothing mapped below it */
/// #define LOW_SIZE 0x20000UL
/// #define BOX ((volatile u64 *)SHARED)
///
/// #define SIGUSR1 10
/// #define SIGSEGV 11
/// #define SIGUSR2 12
/// #define SIGRT 34
/// #define SS_ONSTACK 1U
/// #define SS_DISABLE 2U
/// #define SS_AUTODISARM (1U << 31)
/// #define SA_SIGINFO 4UL
/// #define SA_RESTORER 0x04000000UL
/// #define SA_ONSTACK 0x08000000UL
/// #define SA_NODEFER 0x40000000UL
/// #define BIT(s) (1UL << ((s) - 1))
///
/// static inline long sc6(long n, long a, long b, long c, long d, long e, long f)
/// {
///     register long r10 __asm__("r10") = d;
///     register long r8 __asm__("r8") = e;
///     register long r9 __asm__("r9") = f;
///     long ret;
///     __asm__ volatile("syscall"
///                      : "=a"(ret)
///                      : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
///                      : "rcx", "r11", "memory");
///     return ret;
/// }
/// #define sc(n, a, b, c, d) sc6((n), (long)(a), (long)(b), (long)(c), (long)(d), 0, 0)
///
/// static void __attribute__((noreturn)) die(int code)
/// {
///     for (;;)
///         sc(231, code, 0, 0, 0); /* exit_group */
/// }
/// #define CHECK(cond, code)  \
///     do {                   \
///         if (!(cond))       \
///             die(code);     \
///     } while (0)
///
/// void *memset(void *d, int c, u64 n)
/// {
///     volatile unsigned char *p = d;
///     while (n--)
///         *p++ = (unsigned char)c;
///     return d;
/// }
///
/// void *memcpy(void *d, const void *s, u64 n)
/// {
///     volatile unsigned char *p = d;
///     const volatile unsigned char *q = s;
///     while (n--)
///         *p++ = *q++;
///     return d;
/// }
///
/// struct stk { u64 sp; i32 flags, pad; u64 size; };
/// struct kact { u64 handler, flags, restorer, mask; };
/// struct si { i32 signo, err, code, pad; u64 addr; u64 rest[13]; };
/// /* ucontext_t through uc_mcontext.fpregs; gregs: r8..r15 rdi rsi rbp rbx rdx
///  * rax rcx rsp rip eflags csgsfs err trapno oldmask cr2. */
/// struct uc { u64 flags, link; struct stk stack; u64 gregs[23]; u64 fpstate; };
/// #define REG_RSP 15
/// #define REG_RIP 16
///
/// /* Mailbox (8-byte words at SHARED):
///  *   [0] handler runs        [1] &local in the last handler
///  *   [2..4] its frame's uc_stack: sp, flags, size
///  *   [5..7] sigaltstack(NULL, &q) in the handler: sp, flags, size
///  *   [8] sigaltstack(&elsewhere, NULL) in the handler
///  *   [9] the frame's address (uc - 8)   [10] uc_mcontext.fpstate
///  *   [11] the FPU image's size          [12] resume rsp  [13] resume rip
///  *   [14] si_code  [15] si_addr  [16] si_signo
///  *   [20] nesting depth   [21..23] &local at depth 0..2
///  *   [24] pid  [25] tid
///  *   [26] nonzero: the handler rewrites uc_stack to { [27], 0, [28] }
///  *   [30..32] a new thread's query: sp, flags, size  [33] it is done
///  *   [34] the pipe a child writes to   [40] a thread's clear-tid word */
///
/// static void report_handler(int sig, struct si *info, struct uc *uc)
/// {
///     volatile u64 local = 0;
///     struct stk q, elsewhere;
///     (void)sig;
///     (void)info;
///     elsewhere.sp = ALT + 0x4000;
///     elsewhere.flags = 0;
///     elsewhere.pad = 0;
///     elsewhere.size = 0x4000;
///     BOX[1] = (u64)&local;
///     BOX[2] = uc->stack.sp;
///     BOX[3] = (u32)uc->stack.flags;
///     BOX[4] = uc->stack.size;
///     memset(&q, 0xAA, sizeof q);
///     sc(131, 0, &q, 0, 0);
///     BOX[5] = q.sp;
///     BOX[6] = (u32)q.flags;
///     BOX[7] = q.size;
///     BOX[8] = (u64)sc(131, &elsewhere, 0, 0, 0);
///     BOX[9] = (u64)uc - 8;
///     BOX[10] = uc->fpstate;
///     BOX[11] = 0;
///     if (uc->fpstate) {
///         u32 magic1 = *(volatile u32 *)(uc->fpstate + 464);
///         u32 extended = *(volatile u32 *)(uc->fpstate + 468);
///         BOX[11] = magic1 == 0x46505853U ? extended : 512;
///     }
///     if (BOX[26]) {
///         uc->stack.sp = BOX[27];
///         uc->stack.flags = 0;
///         uc->stack.size = BOX[28];
///     }
///     BOX[0] += 1;
/// }
///
/// static void nest_handler(int sig, struct si *info, struct uc *uc)
/// {
///     volatile u64 local = 0;
///     u64 depth = BOX[20];
///     (void)info;
///     (void)uc;
///     if (depth < 3)
///         BOX[21 + depth] = (u64)&local;
///     BOX[20] = depth + 1;
///     if (depth < 2)
///         sc(234, BOX[24], BOX[25], sig, 0); /* tgkill: again, nested */
/// }
///
/// static void segv_handler(int sig, struct si *info, struct uc *uc)
/// {
///     volatile u64 local = 0;
///     BOX[1] = (u64)&local;
///     BOX[14] = (u64)(u32)info->code;
///     BOX[15] = info->addr;
///     BOX[16] = (u64)sig;
///     BOX[0] += 1;
///     uc->gregs[REG_RIP] = BOX[13];
///     uc->gregs[REG_RSP] = BOX[12];
/// }
///
/// /* In a child: one byte down the pipe per level, then the signal again. */
/// static void flood_handler(int sig, struct si *info, struct uc *uc)
/// {
///     char b = 1;
///     (void)info;
///     (void)uc;
///     sc(1, BOX[34], &b, 1, 0);
///     sc(62, sc(39, 0, 0, 0, 0), sig, 0, 0);
/// }
///
/// __attribute__((naked)) static void restorer(void)
/// {
///     __asm__ volatile("mov $15, %eax\n\tsyscall\n\tud2");
/// }
///
/// static void install(int sig, void *fn, u64 flags)
/// {
///     struct kact a;
///     memset(&a, 0, sizeof a);
///     a.handler = (u64)fn;
///     if (fn) {
///         a.flags = flags | SA_SIGINFO | SA_RESTORER;
///         a.restorer = (u64)restorer;
///     }
///     CHECK(sc(13, sig, &a, 0, 8) == 0, 0x3F);
/// }
///
/// static long sas(const struct stk *n, struct stk *o)
/// {
///     return sc(131, n, o, 0, 0);
/// }
///
/// static void stk_set(struct stk *s, u64 sp, u32 flags, u64 size)
/// {
///     s->sp = sp;
///     s->flags = (i32)flags;
///     s->pad = 0;
///     s->size = size;
/// }
///
/// static int query_is(u64 sp, u32 flags, u64 size)
/// {
///     struct stk q;
///     memset(&q, 0xAA, sizeof q);
///     return sas(0, &q) == 0 && q.sp == sp && (u32)q.flags == flags && q.size == size;
/// }
///
/// static int on_alt(u64 a)
/// {
///     return a >= ALT && a < ALT + ALT_SIZE;
/// }
///
/// /* Run off the bottom of the small stack at LOW: the SIGSEGV handler sends
///  * this back to label 2 on the stack it was called on. */
/// __attribute__((noinline)) static void overflow(void)
/// {
///     __asm__ volatile("push %%rbx\n\t"
///                      "push %%rbp\n\t"
///                      "push %%r12\n\t"
///                      "push %%r13\n\t"
///                      "push %%r14\n\t"
///                      "push %%r15\n\t"
///                      "mov $0x5000000000, %%rax\n\t"
///                      "mov %%rsp, 96(%%rax)\n\t" /* BOX[12] */
///                      "lea 2f(%%rip), %%rbx\n\t"
///                      "mov %%rbx, 104(%%rax)\n\t" /* BOX[13] */
///                      "movabs $0x5000220000, %%rsp\n\t" /* LOW + LOW_SIZE */
///                      "1:\n\t"
///                      "sub $512, %%rsp\n\t"
///                      "movq $1, (%%rsp)\n\t"
///                      "jmp 1b\n\t"
///                      "2:\n\t"
///                      "pop %%r15\n\t"
///                      "pop %%r14\n\t"
///                      "pop %%r13\n\t"
///                      "pop %%r12\n\t"
///                      "pop %%rbp\n\t"
///                      "pop %%rbx\n\t"
///                      :
///                      :
///                      : "rax", "memory", "cc");
/// }
///
/// /* Touch every 4 KiB of the 512 KiB below the stack pointer: the main stack
///  * must grow that far. */
/// __attribute__((noinline)) static void deep_stack(void)
/// {
///     __asm__ volatile("mov %%rsp, %%rax\n\t"
///                      "lea -0x80000(%%rsp), %%rcx\n\t"
///                      "1:\n\t"
///                      "sub $4096, %%rax\n\t"
///                      "movq $0, (%%rax)\n\t"
///                      "cmp %%rcx, %%rax\n\t"
///                      "ja 1b\n\t"
///                      :
///                      :
///                      : "rax", "rcx", "memory", "cc");
/// }
///
/// static void escaped(void)
/// {
///     die(0x55);
/// }
///
/// static void thread_fn(void)
/// {
///     struct stk q;
///     memset(&q, 0xAA, sizeof q);
///     sc(131, 0, &q, 0, 0);
///     BOX[30] = q.sp;
///     BOX[31] = (u32)q.flags;
///     BOX[32] = q.size;
///     BOX[33] = 1;
/// }
///
/// /* clone(CLONE_VM|FS|FILES|SIGHAND|THREAD|SYSVSEM|PARENT_SETTID|CHILD_CLEARTID)
///  * on `stack_top`; the child calls `fn` (passed in r12) and exits. */
/// static long spawn_thread(void (*fn)(void), u64 stack_top, i32 *tid_word)
/// {
///     long ret;
///     register long r10 __asm__("r10") = (long)tid_word; /* child_tid */
///     register long r8 __asm__("r8") = 0;                /* tls */
///     register long r12 __asm__("r12") = (long)fn;
///     __asm__ volatile("syscall\n\t"
///                      "test %%rax, %%rax\n\t"
///                      "jnz 1f\n\t"
///                      "xor %%ebp, %%ebp\n\t"
///                      "call *%%r12\n\t"
///                      "mov $60, %%eax\n\t" /* exit: this thread only */
///                      "xor %%edi, %%edi\n\t"
///                      "syscall\n\t"
///                      "ud2\n\t"
///                      "1:\n\t"
///                      : "=a"(ret)
///                      : "a"(56), "D"(0x3D0F00L), "S"(stack_top), "d"(tid_word), "r"(r10),
///                        "r"(r8), "r"(r12)
///                      : "rcx", "r11", "memory");
///     return ret;
/// }
///
/// static u64 auxv_get(u64 *sp0, u64 type)
/// {
///     u64 *p = sp0 + 1 + sp0[0] + 1; /* past argc, argv[] and its NULL: envp */
///     while (*p)
///         p++;
///     for (p++; p[0] != 0; p += 2)
///         if (p[0] == type)
///             return p[1];
///     return 0;
/// }
///
/// static int wait_child(long pid)
/// {
///     int status = -1;
///     CHECK(sc(61, pid, &status, 0, 0) == pid, 0xAF);
///     return status;
/// }
///
/// __attribute__((used, noreturn)) void main_(u64 *sp0)
/// {
///     u64 minsig = auxv_get(sp0, 51); /* AT_MINSIGSTKSZ */
///     struct stk n, o;
///     long pid, tid, child, i;
///     int status, pipefd[2];
///     u64 stack;
///     volatile i32 *tid_word = (volatile i32 *)(SHARED + 40 * 8);
///
///     CHECK(sc6(9, SHARED, 4096, 3, 0x32, -1, 0) == (long)SHARED, 0x30); /* mailbox */
///     CHECK(sc6(9, ALT, ALT_SIZE, 3, 0x32, -1, 0) == (long)ALT, 0x30);
///     for (i = 0; i < 64; i++)
///         BOX[i] = 0;
///     pid = sc(39, 0, 0, 0, 0);
///     tid = sc(186, 0, 0, 0, 0);
///     BOX[24] = (u64)pid;
///     BOX[25] = (u64)tid;
///
///     /* The main stack grows well past its first 64 KiB. */
///     deep_stack();
///
///     /* ---- the call's answers ---- */
///     CHECK(query_is(0, SS_DISABLE, 0), 0x31); /* none at first */
///     stk_set(&n, ALT, 0, 1024);
///     memset(&o, 0xAA, sizeof o);
///     CHECK(sas(&n, &o) == -12, 0x32); /* ENOMEM */
///     CHECK(o.sp == 0xAAAAAAAAAAAAAAAAUL && o.size == 0xAAAAAAAAAAAAAAAAUL, 0x33); /* nothing reported */
///     stk_set(&n, ALT, 5, ALT_SIZE);
///     CHECK(sas(&n, 0) == -22, 0x34); /* EINVAL */
///     stk_set(&n, ALT, SS_DISABLE | SS_AUTODISARM, 1234);
///     CHECK(sas(&n, 0) == 0, 0x35);
///     CHECK(query_is(0, SS_DISABLE | SS_AUTODISARM, 0), 0x36);
///     stk_set(&n, ALT, SS_ONSTACK, ALT_SIZE);
///     memset(&o, 0xAA, sizeof o);
///     CHECK(sas(&n, &o) == 0 && o.sp == 0 && (u32)o.flags == (SS_DISABLE | SS_AUTODISARM)
///               && o.size == 0,
///           0x37);
///     CHECK(query_is(ALT, 0, ALT_SIZE), 0x38); /* off the stack: 0 */
///
///     /* ---- an SA_ONSTACK handler runs on it ---- */
///     install(SIGUSR1, report_handler, SA_ONSTACK);
///     install(SIGUSR2, report_handler, 0);
///     CHECK(sc(62, pid, SIGUSR1, 0, 0) == 0, 0x40);
///     CHECK(BOX[0] == 1, 0x41);
///     CHECK(on_alt(BOX[1]), 0x42);
///     CHECK(BOX[2] == ALT && BOX[3] == SS_ONSTACK && BOX[4] == ALT_SIZE, 0x43); /* flags as set */
///     CHECK(BOX[5] == ALT && BOX[6] == SS_ONSTACK && BOX[7] == ALT_SIZE, 0x44);
///     CHECK(BOX[8] == (u64)-1, 0x45); /* EPERM: it is running on it */
///     /* The frame and its FPU image are inside, within AT_MINSIGSTKSZ of the top. */
///     CHECK(minsig > 440 && minsig < 65536 && minsig % 16 == 0, 0x46);
///     CHECK(on_alt(BOX[9]) && ALT + ALT_SIZE - BOX[9] <= minsig, 0x47);
///     CHECK(BOX[10] > BOX[9] && BOX[11] >= 512 && BOX[10] + BOX[11] <= ALT + ALT_SIZE, 0x48);
///     CHECK(query_is(ALT, 0, ALT_SIZE), 0x49);
///
///     /* ---- one without SA_ONSTACK does not; rt_sigreturn restores uc_stack ---- */
///     BOX[26] = 1;
///     BOX[27] = ALT + 0x8000;
///     BOX[28] = 0x8000;
///     CHECK(sc(62, pid, SIGUSR2, 0, 0) == 0, 0x50);
///     BOX[26] = 0;
///     CHECK(BOX[0] == 2, 0x51);
///     CHECK(!on_alt(BOX[1]), 0x52);
///     CHECK(BOX[2] == ALT && BOX[3] == SS_ONSTACK && BOX[4] == ALT_SIZE, 0x53);
///     CHECK(BOX[6] == 0, 0x54); /* not on it */
///     CHECK(BOX[8] == 0, 0x55); /* so a change is allowed */
///     CHECK(query_is(ALT + 0x8000, 0, 0x8000), 0x56); /* the frame's, rewritten */
///
///     /* ---- SS_AUTODISARM ---- */
///     stk_set(&n, ALT, SS_AUTODISARM, ALT_SIZE);
///     CHECK(sas(&n, 0) == 0, 0x60);
///     CHECK(query_is(ALT, SS_AUTODISARM, ALT_SIZE), 0x61);
///     CHECK(sc(62, pid, SIGUSR1, 0, 0) == 0, 0x62);
///     CHECK(BOX[0] == 3 && on_alt(BOX[1]), 0x63);
///     CHECK(BOX[2] == ALT && BOX[3] == SS_AUTODISARM && BOX[4] == ALT_SIZE, 0x64);
///     CHECK(BOX[5] == 0 && BOX[6] == SS_DISABLE && BOX[7] == 0, 0x65); /* disarmed meanwhile */
///     CHECK(BOX[8] == 0, 0x66);
///     CHECK(query_is(ALT, SS_AUTODISARM, ALT_SIZE), 0x67); /* re-armed by rt_sigreturn */
///
///     /* ---- a signal nested on the stack stays on it ---- */
///     stk_set(&n, ALT, 0, ALT_SIZE);
///     CHECK(sas(&n, 0) == 0, 0x70);
///     install(SIGRT, nest_handler, SA_ONSTACK | SA_NODEFER);
///     CHECK(sc(234, pid, tid, SIGRT, 0) == 0, 0x71);
///     CHECK(BOX[20] == 3, 0x72);
///     CHECK(on_alt(BOX[21]) && on_alt(BOX[22]) && on_alt(BOX[23]) && BOX[22] < BOX[21]
///               && BOX[23] < BOX[22],
///           0x73);
///
///     /* ---- a stack overflow's SIGSEGV runs on it, and the program goes on ---- */
///     install(SIGSEGV, segv_handler, SA_ONSTACK);
///     CHECK(sc6(9, LOW, LOW_SIZE, 3, 0x32, -1, 0) == (long)LOW, 0x80);
///     sc(11, LOW - 0x10000, 0x10000, 0, 0); /* nothing below it */
///     BOX[0] = 0;
///     overflow();
///     CHECK(BOX[0] == 1, 0x81);
///     CHECK(BOX[16] == SIGSEGV && BOX[14] == 1, 0x82); /* SEGV_MAPERR */
///     CHECK(BOX[15] == LOW - 512, 0x83);
///     CHECK(on_alt(BOX[1]), 0x84);
///
///     /* ---- a new thread starts with none ---- */
///     stack = (u64)sc6(9, 0, 65536, 3, 0x22, -1, 0);
///     CHECK(stack < (u64)-4096, 0x90);
///     CHECK(spawn_thread(thread_fn, stack + 65536, (i32 *)tid_word) > 0, 0x91);
///     for (i = 0; i < 5000 && *tid_word != 0; i++) {
///         i32 v = *tid_word;
///         if (v != 0) {
///             i64 t[2] = { 0, 1000000 };
///             sc6(202, (long)tid_word, 0, v, (long)t, 0, 0); /* futex wait, 1 ms */
///         }
///     }
///     CHECK(*tid_word == 0 && BOX[33] == 1, 0x92);
///     CHECK(BOX[30] == 0 && BOX[31] == SS_DISABLE && BOX[32] == 0, 0x93);
///
///     /* ---- a forked child keeps it ---- */
///     child = sc(57, 0, 0, 0, 0);
///     if (child == 0)
///         sc(60, query_is(ALT, 0, ALT_SIZE) ? 0x2B : 0x2C, 0, 0, 0);
///     CHECK(child > 0, 0xA0);
///     status = wait_child(child);
///     CHECK(status == 0x2B00, 0xA1);
///
///     /* ---- a frame that does not fit the stack: SIGSEGV ---- */
///     CHECK(sc(293, pipefd, 0, 0, 0) == 0, 0xA2);
///     child = sc(57, 0, 0, 0, 0);
///     if (child == 0) {
///         sc(3, pipefd[0], 0, 0, 0);
///         BOX[34] = (u64)pipefd[1];
///         install(SIGSEGV, 0, 0);
///         install(SIGUSR2, flood_handler, SA_ONSTACK | SA_NODEFER);
///         sc(62, sc(39, 0, 0, 0, 0), SIGUSR2, 0, 0);
///         die(0x2D);
///     }
///     CHECK(child > 0, 0xA3);
///     sc(3, pipefd[1], 0, 0, 0);
///     status = wait_child(child);
///     CHECK((status & 0x7F) == SIGSEGV, 0xA4);
///     /* Several frames fitted before one did not: a byte each, read into the
///      * mailbox's upper half. */
///     CHECK(sc(0, pipefd[0], SHARED + 2048, 2048, 0) >= 3, 0xA5);
///     sc(3, pipefd[0], 0, 0, 0);
///
///     /* ---- a fault while SIGSEGV is blocked ends the process ---- */
///     child = sc(57, 0, 0, 0, 0);
///     if (child == 0) {
///         u64 segv = BIT(SIGSEGV);
///         BOX[12] = LOW + LOW_SIZE - 8;
///         BOX[13] = (u64)escaped;
///         sc(14, 0, &segv, 0, 8); /* SIG_BLOCK */
///         *(volatile u64 *)(LOW - 512) = 1;
///         die(0x2E);
///     }
///     CHECK(child > 0, 0xA6);
///     status = wait_child(child);
///     CHECK((status & 0x7F) == SIGSEGV, 0xA7);
///
///     die(0x2A);
/// }
///
/// __attribute__((naked, section(".text.start"))) void _start(void)
/// {
///     __asm__ volatile("mov %rsp, %rdi\n\tand $-16, %rsp\n\tcall main_\n\tud2");
/// }
/// ```
///
/// Paired with [`crate::proc::spawn::self_test_linux_sigaltstack`]. Tagged
/// `ELFOSABI_GNU` so `spawn_process` routes it through the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_sigaltstack_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const CODE: [u8; 5302] = [
        0x48, 0x89, 0xE7, 0x48, 0x83, 0xE4, 0xF0, 0xE8, 0xB4, 0x05, 0x00, 0x00, 0x0F, 0x0B, 0x0F,
        0x0B, 0x53, 0x48, 0x63, 0xFF, 0xBB, 0xE7, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x48, 0x89, 0xD8, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xEB, 0xDF, 0x48, 0xC7,
        0x44, 0x24, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x48, 0xB8, 0xA0, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0x8B, 0x08, 0x48, 0x83, 0xF9, 0x02, 0x77, 0x5C, 0x48, 0x8D, 0x80, 0x60,
        0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0x84, 0xC8, 0xA8, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x54, 0x24,
        0xF8, 0x48, 0x89, 0x10, 0x48, 0x8D, 0x41, 0x01, 0x48, 0xA3, 0xA0, 0x00, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x48, 0x83, 0xF9, 0x01, 0x77, 0x3F, 0x48, 0x63, 0xD7, 0x48, 0xB8, 0xC8,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x30, 0x48, 0x8D, 0x40, 0xF8, 0x48,
        0x8B, 0x38, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xEA, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xC3, 0x48, 0x8D,
        0x41, 0x01, 0x48, 0xA3, 0xA0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xC3, 0x48, 0xC7,
        0x44, 0x24, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x44, 0x24, 0xF8, 0x48, 0xA3, 0x08,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x8B, 0x46, 0x08, 0x48, 0xA3, 0x70, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x46, 0x10, 0x48, 0xA3, 0x78, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x63, 0xC7, 0x48, 0xA3, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0xB9, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x01,
        0x48, 0x83, 0xC0, 0x01, 0x48, 0x89, 0x01, 0x48, 0xA1, 0x68, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0x89, 0x82, 0xA8, 0x00, 0x00, 0x00, 0x48, 0xA1, 0x60, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x82, 0xA0, 0x00, 0x00, 0x00, 0xC3, 0x53, 0x89, 0xFB,
        0xC6, 0x44, 0x24, 0xFF, 0x01, 0x48, 0x8D, 0x74, 0x24, 0xFF, 0x48, 0xB8, 0x10, 0x01, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x38, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00,
        0x00, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48, 0x63, 0xDB, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8,
        0x27, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x89, 0xC7,
        0xB8, 0x3E, 0x00, 0x00, 0x00, 0x48, 0x89, 0xDE, 0x0F, 0x05, 0x5B, 0xC3, 0xB8, 0x0F, 0x00,
        0x00, 0x00, 0x0F, 0x05, 0x0F, 0x0B, 0x0F, 0x0B, 0x53, 0x55, 0x41, 0x54, 0x41, 0x55, 0x41,
        0x56, 0x41, 0x57, 0x48, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89,
        0x60, 0x60, 0x48, 0x8D, 0x1D, 0x1F, 0x00, 0x00, 0x00, 0x48, 0x89, 0x58, 0x68, 0x48, 0xBC,
        0x00, 0x00, 0x22, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x81, 0xEC, 0x00, 0x02, 0x00, 0x00,
        0x48, 0xC7, 0x04, 0x24, 0x01, 0x00, 0x00, 0x00, 0xEB, 0xEF, 0x41, 0x5F, 0x41, 0x5E, 0x41,
        0x5D, 0x41, 0x5C, 0x5D, 0x5B, 0xC3, 0x48, 0x89, 0xE0, 0x48, 0x8D, 0x8C, 0x24, 0x00, 0x00,
        0xF8, 0xFF, 0x48, 0x2D, 0x00, 0x10, 0x00, 0x00, 0x48, 0xC7, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x39, 0xC8, 0x77, 0xEE, 0xC3, 0xBF, 0x55, 0x00, 0x00, 0x00, 0xE8, 0x02, 0xFE, 0xFF,
        0xFF, 0x48, 0x83, 0xEC, 0x10, 0xC7, 0x44, 0x24, 0x0C, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0x8D,
        0x74, 0x24, 0x0C, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x3D, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x0F, 0x05, 0x48, 0x39, 0xC7, 0x75, 0x09, 0x8B, 0x44, 0x24, 0x0C, 0x48, 0x83, 0xC4,
        0x10, 0xC3, 0xBF, 0xAF, 0x00, 0x00, 0x00, 0xE8, 0xBB, 0xFD, 0xFF, 0xFF, 0x48, 0x89, 0xF8,
        0x48, 0x85, 0xD2, 0x74, 0x15, 0x48, 0x01, 0xFA, 0x48, 0x89, 0xF9, 0x49, 0x89, 0xC8, 0x48,
        0x83, 0xC1, 0x01, 0x41, 0x88, 0x30, 0x48, 0x39, 0xCA, 0x75, 0xF1, 0xC3, 0x53, 0x48, 0x83,
        0xEC, 0x20, 0x48, 0x8D, 0x5C, 0x24, 0x08, 0xBA, 0x18, 0x00, 0x00, 0x00, 0xBE, 0xAA, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xDF, 0xE8, 0xC6, 0xFF, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0x83, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xDE, 0x0F,
        0x05, 0x48, 0x8B, 0x44, 0x24, 0x08, 0x48, 0xA3, 0xF0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x8B, 0x44, 0x24, 0x10, 0x48, 0xA3, 0xF8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x48, 0x8B, 0x44, 0x24, 0x18, 0x48, 0xA3, 0x00, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x48, 0xB8, 0x08, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x48, 0x83, 0xC4, 0x20, 0x5B, 0xC3, 0x41, 0x54, 0x55, 0x53, 0x48, 0x83, 0xEC,
        0x40, 0x48, 0x89, 0xD3, 0x48, 0xC7, 0x44, 0x24, 0x38, 0x00, 0x00, 0x00, 0x00, 0x48, 0xB8,
        0x00, 0x40, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x08, 0xC7, 0x44,
        0x24, 0x10, 0x00, 0x00, 0x00, 0x00, 0xC7, 0x44, 0x24, 0x14, 0x00, 0x00, 0x00, 0x00, 0x48,
        0xC7, 0x44, 0x24, 0x18, 0x00, 0x40, 0x00, 0x00, 0x48, 0x8D, 0x44, 0x24, 0x38, 0x48, 0xA3,
        0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x42, 0x10, 0x48, 0xA3, 0x10,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x8B, 0x42, 0x18, 0x48, 0xA3, 0x18, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x42, 0x20, 0x48, 0xA3, 0x20, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x4C, 0x8D, 0x64, 0x24, 0x20, 0xBA, 0x18, 0x00, 0x00, 0x00, 0xBE,
        0xAA, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE7, 0xE8, 0xD4, 0xFE, 0xFF, 0xFF, 0x41, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xBD, 0x83, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xE8, 0x48, 0x89,
        0xD7, 0x4C, 0x89, 0xE6, 0x0F, 0x05, 0x48, 0x8B, 0x44, 0x24, 0x20, 0x48, 0xA3, 0x28, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x8B, 0x44, 0x24, 0x28, 0x48, 0xA3, 0x30, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x44, 0x24, 0x30, 0x48, 0xA3, 0x38, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x7C, 0x24, 0x08, 0x48, 0x89, 0xE8, 0x48, 0x89,
        0xD6, 0x0F, 0x05, 0x48, 0xA3, 0x40, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8D,
        0x43, 0xF8, 0x48, 0xA3, 0x48, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x83,
        0xE0, 0x00, 0x00, 0x00, 0x48, 0xA3, 0x50, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0xB8, 0x58, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x48, 0x8B, 0x83, 0xE0, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x74, 0x26, 0x8B, 0x90,
        0xD0, 0x01, 0x00, 0x00, 0x8B, 0x80, 0xD4, 0x01, 0x00, 0x00, 0x81, 0xFA, 0x53, 0x58, 0x50,
        0x46, 0xBA, 0x00, 0x02, 0x00, 0x00, 0x0F, 0x45, 0xC2, 0x89, 0xC0, 0x48, 0xA3, 0x58, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xA1, 0xD0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x85, 0xC0, 0x74, 0x23, 0x48, 0xA1, 0xD8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x89, 0x43, 0x10, 0xC7, 0x43, 0x18, 0x00, 0x00, 0x00, 0x00, 0x48, 0xA1, 0xE0,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x43, 0x20, 0x48, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x02, 0x48, 0x83, 0xC0, 0x01, 0x48, 0x89,
        0x02, 0x48, 0x83, 0xC4, 0x40, 0x5B, 0x5D, 0x41, 0x5C, 0xC3, 0x41, 0x55, 0x41, 0x54, 0x55,
        0x53, 0x48, 0x83, 0xEC, 0x20, 0x48, 0x89, 0xFB, 0x89, 0xF5, 0x49, 0x89, 0xD4, 0x4C, 0x8D,
        0x6C, 0x24, 0x08, 0xBA, 0x18, 0x00, 0x00, 0x00, 0xBE, 0xAA, 0x00, 0x00, 0x00, 0x4C, 0x89,
        0xEF, 0xE8, 0x90, 0xFD, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8,
        0x83, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x4C, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0,
        0x75, 0x07, 0x48, 0x39, 0x5C, 0x24, 0x08, 0x74, 0x0D, 0x89, 0xD0, 0x48, 0x83, 0xC4, 0x20,
        0x5B, 0x5D, 0x41, 0x5C, 0x41, 0x5D, 0xC3, 0x39, 0x6C, 0x24, 0x10, 0x75, 0xED, 0x4C, 0x39,
        0x64, 0x24, 0x18, 0x0F, 0x94, 0xC2, 0x0F, 0xB6, 0xD2, 0xEB, 0xE0, 0x41, 0x54, 0x55, 0x53,
        0x48, 0x83, 0xEC, 0x20, 0x89, 0xFB, 0x49, 0x89, 0xF4, 0x48, 0x89, 0xD5, 0x48, 0x89, 0xE7,
        0xBA, 0x20, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0xE8, 0x1E, 0xFD, 0xFF, 0xFF,
        0x4C, 0x89, 0x24, 0x24, 0x4D, 0x85, 0xE4, 0x74, 0x1B, 0x48, 0x89, 0xEA, 0x48, 0x81, 0xCA,
        0x04, 0x00, 0x00, 0x04, 0x48, 0x89, 0x54, 0x24, 0x08, 0x48, 0x8D, 0x05, 0x3C, 0xFC, 0xFF,
        0xFF, 0x48, 0x89, 0x44, 0x24, 0x10, 0x48, 0x89, 0xE6, 0x48, 0x63, 0xFB, 0x41, 0xBA, 0x08,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xB8, 0x0D, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0,
        0x75, 0x09, 0x48, 0x83, 0xC4, 0x20, 0x5B, 0x5D, 0x41, 0x5C, 0xC3, 0xBF, 0x3F, 0x00, 0x00,
        0x00, 0xE8, 0x79, 0xFA, 0xFF, 0xFF, 0x48, 0x89, 0xF8, 0x48, 0x85, 0xD2, 0x74, 0x20, 0x48,
        0x01, 0xFA, 0x48, 0x89, 0xF9, 0x49, 0x89, 0xF1, 0x48, 0x83, 0xC6, 0x01, 0x49, 0x89, 0xC8,
        0x48, 0x83, 0xC1, 0x01, 0x45, 0x0F, 0xB6, 0x09, 0x45, 0x88, 0x08, 0x48, 0x39, 0xCA, 0x75,
        0xE6, 0xC3, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x55, 0x53, 0x48, 0x83, 0xEC, 0x50, 0x48,
        0x8B, 0x07, 0x48, 0x8D, 0x44, 0xC7, 0x10, 0x48, 0x83, 0x38, 0x00, 0x74, 0x0A, 0x48, 0x83,
        0xC0, 0x08, 0x48, 0x83, 0x38, 0x00, 0x75, 0xF6, 0x48, 0x8D, 0x50, 0x08, 0x48, 0x8B, 0x58,
        0x08, 0x48, 0x85, 0xDB, 0x74, 0x18, 0x48, 0x83, 0xFB, 0x33, 0x74, 0x0E, 0x48, 0x83, 0xC2,
        0x10, 0x48, 0x8B, 0x1A, 0x48, 0x85, 0xDB, 0x75, 0xEE, 0xEB, 0x04, 0x48, 0x8B, 0x5A, 0x08,
        0x41, 0xBA, 0x32, 0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0x48, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xB8,
        0x09, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x10, 0x00, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x0F,
        0x05, 0x48, 0x39, 0xF8, 0x0F, 0x85, 0xCD, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x32, 0x00, 0x00,
        0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0x48,
        0xBF, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00, 0xBE,
        0x00, 0x00, 0x01, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xB9, 0x00, 0x02, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x39, 0xF8, 0x0F, 0x85, 0x8C, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x02, 0x00, 0x00,
        0x00, 0x00, 0x48, 0x83, 0xC2, 0x08, 0x48, 0x39, 0xCA, 0x75, 0xF0, 0x41, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0x27, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6,
        0x0F, 0x05, 0x48, 0x89, 0xC5, 0xB8, 0xBA, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x49, 0x89, 0xC4,
        0x48, 0x89, 0xE8, 0x48, 0xA3, 0xC0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x4C, 0x89,
        0xE0, 0x48, 0xA3, 0xC8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xE8, 0xFB, 0xFA, 0xFF,
        0xFF, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xBE, 0x02, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x00,
        0x00, 0xE8, 0x9D, 0xFD, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x1E, 0xBF, 0x31, 0x00, 0x00, 0x00,
        0xE8, 0x03, 0xF9, 0xFF, 0xFF, 0xBF, 0x30, 0x00, 0x00, 0x00, 0xE8, 0xF9, 0xF8, 0xFF, 0xFF,
        0xBF, 0x30, 0x00, 0x00, 0x00, 0xE8, 0xEF, 0xF8, 0xFF, 0xFF, 0x48, 0xB8, 0x00, 0x00, 0x10,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x38, 0xC7, 0x44, 0x24, 0x40, 0x00,
        0x00, 0x00, 0x00, 0xC7, 0x44, 0x24, 0x44, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24,
        0x48, 0x00, 0x04, 0x00, 0x00, 0x4C, 0x8D, 0x6C, 0x24, 0x20, 0xBA, 0x18, 0x00, 0x00, 0x00,
        0xBE, 0xAA, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xEF, 0xE8, 0xF5, 0xFA, 0xFF, 0xFF, 0x4C, 0x8D,
        0x74, 0x24, 0x38, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x83, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x4C, 0x89, 0xF7, 0x4C, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xF4, 0x75, 0x22,
        0x48, 0xB8, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x48, 0x39, 0x44, 0x24, 0x20,
        0x75, 0x07, 0x48, 0x39, 0x44, 0x24, 0x30, 0x74, 0x14, 0xBF, 0x33, 0x00, 0x00, 0x00, 0xE8,
        0x5F, 0xF8, 0xFF, 0xFF, 0xBF, 0x32, 0x00, 0x00, 0x00, 0xE8, 0x55, 0xF8, 0xFF, 0xFF, 0x48,
        0xB8, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x38, 0xC7,
        0x44, 0x24, 0x40, 0x05, 0x00, 0x00, 0x00, 0xC7, 0x44, 0x24, 0x44, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0x44, 0x24, 0x48, 0x00, 0x00, 0x01, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0x83, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x83, 0xF8,
        0xEA, 0x74, 0x0A, 0xBF, 0x34, 0x00, 0x00, 0x00, 0xE8, 0xFC, 0xF7, 0xFF, 0xFF, 0x48, 0xB8,
        0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x38, 0xC7, 0x44,
        0x24, 0x40, 0x02, 0x00, 0x00, 0x80, 0xC7, 0x44, 0x24, 0x44, 0x00, 0x00, 0x00, 0x00, 0x48,
        0xC7, 0x44, 0x24, 0x48, 0xD2, 0x04, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x83, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74,
        0x0A, 0xBF, 0x35, 0x00, 0x00, 0x00, 0xE8, 0xA4, 0xF7, 0xFF, 0xFF, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0xBE, 0x02, 0x00, 0x00, 0x80, 0xBF, 0x00, 0x00, 0x00, 0x00, 0xE8, 0x1C, 0xFC, 0xFF,
        0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x36, 0x00, 0x00, 0x00, 0xE8, 0x82, 0xF7, 0xFF, 0xFF,
        0x48, 0xB8, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x38,
        0xC7, 0x44, 0x24, 0x40, 0x01, 0x00, 0x00, 0x00, 0xC7, 0x44, 0x24, 0x44, 0x00, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x44, 0x24, 0x48, 0x00, 0x00, 0x01, 0x00, 0x48, 0x8D, 0x7C, 0x24, 0x20,
        0xBA, 0x18, 0x00, 0x00, 0x00, 0xBE, 0xAA, 0x00, 0x00, 0x00, 0xE8, 0x8B, 0xF9, 0xFF, 0xFF,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0x83, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89,
        0xF7, 0x4C, 0x89, 0xEE, 0x0F, 0x05, 0x81, 0x7C, 0x24, 0x28, 0x02, 0x00, 0x00, 0x80, 0x74,
        0x0A, 0xBF, 0x37, 0x00, 0x00, 0x00, 0xE8, 0x0E, 0xF7, 0xFF, 0xFF, 0x48, 0x8B, 0x54, 0x24,
        0x20, 0x48, 0x0B, 0x54, 0x24, 0x30, 0x48, 0x09, 0xC2, 0x75, 0xE7, 0xBA, 0x00, 0x00, 0x01,
        0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0xBF, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00,
        0x00, 0xE8, 0x72, 0xFB, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x38, 0x00, 0x00, 0x00,
        0xE8, 0xD8, 0xF6, 0xFF, 0xFF, 0xBA, 0x00, 0x00, 0x00, 0x08, 0x4C, 0x8D, 0x2D, 0xB2, 0xF9,
        0xFF, 0xFF, 0x4C, 0x89, 0xEE, 0xBF, 0x0A, 0x00, 0x00, 0x00, 0xE8, 0xC4, 0xFB, 0xFF, 0xFF,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xEE, 0xBF, 0x0C, 0x00, 0x00, 0x00, 0xE8, 0xB2,
        0xFB, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x3E, 0x00, 0x00, 0x00, 0xBE, 0x0A, 0x00, 0x00,
        0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75,
        0x62, 0x48, 0xA1, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01,
        0x75, 0x5C, 0x48, 0xA1, 0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00,
        0x00, 0xF0, 0xFF, 0xAF, 0xFF, 0xFF, 0xFF, 0x48, 0x01, 0xD0, 0x48, 0x3D, 0xFF, 0xFF, 0x00,
        0x00, 0x77, 0x47, 0x48, 0xA1, 0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA,
        0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xD0, 0x75, 0x10, 0x48, 0xA1,
        0x18, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01, 0x74, 0x28, 0xBF,
        0x43, 0x00, 0x00, 0x00, 0xE8, 0x20, 0xF6, 0xFF, 0xFF, 0xBF, 0x40, 0x00, 0x00, 0x00, 0xE8,
        0x16, 0xF6, 0xFF, 0xFF, 0xBF, 0x41, 0x00, 0x00, 0x00, 0xE8, 0x0C, 0xF6, 0xFF, 0xFF, 0xBF,
        0x42, 0x00, 0x00, 0x00, 0xE8, 0x02, 0xF6, 0xFF, 0xFF, 0x48, 0xA1, 0x20, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x3D, 0x00, 0x00, 0x01, 0x00, 0x75, 0xC6, 0x48, 0xA1, 0x28,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0x39, 0xD0, 0x75, 0x10, 0x48, 0xA1, 0x30, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0x83, 0xF8, 0x01, 0x74, 0x0A, 0xBF, 0x44, 0x00, 0x00, 0x00, 0xE8, 0xBD,
        0xF5, 0xFF, 0xFF, 0x48, 0xA1, 0x38, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x3D,
        0x00, 0x00, 0x01, 0x00, 0x75, 0xE4, 0x48, 0xA1, 0x40, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x83, 0xF8, 0xFF, 0x75, 0x1E, 0x48, 0x8D, 0x83, 0x47, 0xFE, 0xFF, 0xFF, 0x48,
        0x3D, 0x46, 0xFE, 0x00, 0x00, 0x77, 0x05, 0xF6, 0xC3, 0x0F, 0x74, 0x14, 0xBF, 0x46, 0x00,
        0x00, 0x00, 0xE8, 0x7D, 0xF5, 0xFF, 0xFF, 0xBF, 0x45, 0x00, 0x00, 0x00, 0xE8, 0x73, 0xF5,
        0xFF, 0xFF, 0x48, 0xA1, 0x48, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00,
        0x00, 0xF0, 0xFF, 0xAF, 0xFF, 0xFF, 0xFF, 0x48, 0x01, 0xD0, 0x48, 0x3D, 0xFF, 0xFF, 0x00,
        0x00, 0x77, 0x1C, 0x48, 0xA1, 0x48, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA,
        0x00, 0x00, 0x11, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x29, 0xC2, 0x48, 0x39, 0xD3, 0x73,
        0x0A, 0xBF, 0x47, 0x00, 0x00, 0x00, 0xE8, 0x2E, 0xF5, 0xFF, 0xFF, 0x48, 0xBE, 0x50, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x16, 0x48, 0xA1, 0x48, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xD0, 0x73, 0x31, 0x48, 0xA1, 0x58, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x3D, 0xFF, 0x01, 0x00, 0x00, 0x76, 0x1F, 0x48, 0x8B, 0x16,
        0x48, 0xA1, 0x58, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x01, 0xC2, 0x48, 0xB8,
        0x00, 0x00, 0x11, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xD0, 0x73, 0x0A, 0xBF, 0x48,
        0x00, 0x00, 0x00, 0xE8, 0xD7, 0xF4, 0xFF, 0xFF, 0xBA, 0x00, 0x00, 0x01, 0x00, 0xBE, 0x00,
        0x00, 0x00, 0x00, 0x48, 0xBF, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0xE8, 0x4A,
        0xF9, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x49, 0x00, 0x00, 0x00, 0xE8, 0xB0, 0xF4,
        0xFF, 0xFF, 0x48, 0xB8, 0xD0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x00,
        0x01, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x80, 0x30, 0x7F, 0x10, 0x00, 0x48, 0xA3, 0xD8, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x80, 0xE0, 0x80, 0xEF, 0xFF, 0x48, 0xC7,
        0x00, 0x00, 0x80, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x3E, 0x00, 0x00, 0x00, 0xBE, 0x0C,
        0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x0F, 0x05, 0x48, 0x85,
        0xC0, 0x74, 0x0A, 0xBF, 0x50, 0x00, 0x00, 0x00, 0xE8, 0x4B, 0xF4, 0xFF, 0xFF, 0x48, 0xB8,
        0xD0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xA1, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x02, 0x74,
        0x0A, 0xBF, 0x51, 0x00, 0x00, 0x00, 0xE8, 0x20, 0xF4, 0xFF, 0xFF, 0x48, 0xA1, 0x08, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00, 0x00, 0xF0, 0xFF, 0xAF, 0xFF, 0xFF,
        0xFF, 0x48, 0x01, 0xD0, 0x48, 0x3D, 0xFF, 0xFF, 0x00, 0x00, 0x77, 0x0A, 0xBF, 0x52, 0x00,
        0x00, 0x00, 0xE8, 0xF7, 0xF3, 0xFF, 0xFF, 0x48, 0xA1, 0x10, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0xBA, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xD0,
        0x75, 0x22, 0x48, 0xA1, 0x18, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8,
        0x01, 0x75, 0x12, 0x48, 0xA1, 0x20, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x3D,
        0x00, 0x00, 0x01, 0x00, 0x74, 0x0A, 0xBF, 0x53, 0x00, 0x00, 0x00, 0xE8, 0xB2, 0xF3, 0xFF,
        0xFF, 0x48, 0xA1, 0x30, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x74,
        0x0A, 0xBF, 0x54, 0x00, 0x00, 0x00, 0xE8, 0x99, 0xF3, 0xFF, 0xFF, 0x48, 0xA1, 0x40, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x55, 0x00, 0x00,
        0x00, 0xE8, 0x80, 0xF3, 0xFF, 0xFF, 0xBA, 0x00, 0x80, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00,
        0x00, 0x48, 0xBF, 0x00, 0x80, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0xE8, 0xF3, 0xF7, 0xFF,
        0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x56, 0x00, 0x00, 0x00, 0xE8, 0x59, 0xF3, 0xFF, 0xFF,
        0x48, 0xB8, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x38,
        0xC7, 0x44, 0x24, 0x40, 0x00, 0x00, 0x00, 0x80, 0xC7, 0x44, 0x24, 0x44, 0x00, 0x00, 0x00,
        0x00, 0x48, 0xC7, 0x44, 0x24, 0x48, 0x00, 0x00, 0x01, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0x83, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xF7, 0x48, 0x89, 0xD6, 0x0F,
        0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x60, 0x00, 0x00, 0x00, 0xE8, 0xFE, 0xF2, 0xFF,
        0xFF, 0xBA, 0x00, 0x00, 0x01, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x80, 0x48, 0xBF, 0x00, 0x00,
        0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0xE8, 0x71, 0xF7, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x0A,
        0xBF, 0x61, 0x00, 0x00, 0x00, 0xE8, 0xD7, 0xF2, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x3E,
        0x00, 0x00, 0x00, 0xBE, 0x0A, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xEF, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x62, 0x00, 0x00, 0x00, 0xE8, 0xA2,
        0xF2, 0xFF, 0xFF, 0x48, 0xA1, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83,
        0xF8, 0x03, 0x75, 0x1F, 0x48, 0xA1, 0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0xBA, 0x00, 0x00, 0xF0, 0xFF, 0xAF, 0xFF, 0xFF, 0xFF, 0x48, 0x01, 0xD0, 0x48, 0x3D, 0xFF,
        0xFF, 0x00, 0x00, 0x76, 0x0A, 0xBF, 0x63, 0x00, 0x00, 0x00, 0xE8, 0x69, 0xF2, 0xFF, 0xFF,
        0x48, 0xA1, 0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00, 0x00, 0x10,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xD0, 0x75, 0x26, 0x48, 0xA1, 0x18, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x80, 0x48, 0x39, 0xD0, 0x75, 0x12,
        0x48, 0xA1, 0x20, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x3D, 0x00, 0x00, 0x01,
        0x00, 0x74, 0x0A, 0xBF, 0x64, 0x00, 0x00, 0x00, 0xE8, 0x20, 0xF2, 0xFF, 0xFF, 0x48, 0xA1,
        0x28, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x75, 0x1F, 0x48, 0xA1,
        0x30, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x02, 0x75, 0x0F, 0x48,
        0xA1, 0x38, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF,
        0x65, 0x00, 0x00, 0x00, 0xE8, 0xE8, 0xF1, 0xFF, 0xFF, 0x48, 0xA1, 0x40, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x66, 0x00, 0x00, 0x00, 0xE8,
        0xCF, 0xF1, 0xFF, 0xFF, 0xBA, 0x00, 0x00, 0x01, 0x00, 0xBE, 0x00, 0x00, 0x00, 0x80, 0x48,
        0xBF, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0xE8, 0x42, 0xF6, 0xFF, 0xFF, 0x85,
        0xC0, 0x75, 0x0A, 0xBF, 0x67, 0x00, 0x00, 0x00, 0xE8, 0xA8, 0xF1, 0xFF, 0xFF, 0x48, 0xB8,
        0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x38, 0xC7, 0x44,
        0x24, 0x40, 0x00, 0x00, 0x00, 0x00, 0xC7, 0x44, 0x24, 0x44, 0x00, 0x00, 0x00, 0x00, 0x48,
        0xC7, 0x44, 0x24, 0x48, 0x00, 0x00, 0x01, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x83, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xF7, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48,
        0x85, 0xC0, 0x74, 0x0A, 0xBF, 0x70, 0x00, 0x00, 0x00, 0xE8, 0x4D, 0xF1, 0xFF, 0xFF, 0xBA,
        0x00, 0x00, 0x00, 0x48, 0x48, 0x8D, 0x35, 0x6B, 0xF1, 0xFF, 0xFF, 0xBF, 0x22, 0x00, 0x00,
        0x00, 0xE8, 0x3C, 0xF6, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0xEA, 0x00, 0x00, 0x00, 0xBA,
        0x22, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x4C, 0x89, 0xE6, 0x0F, 0x05, 0x48, 0x85, 0xC0,
        0x74, 0x0A, 0xBF, 0x71, 0x00, 0x00, 0x00, 0xE8, 0x04, 0xF1, 0xFF, 0xFF, 0x48, 0xA1, 0xA0,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x03, 0x74, 0x0A, 0xBF, 0x72,
        0x00, 0x00, 0x00, 0xE8, 0xEA, 0xF0, 0xFF, 0xFF, 0x48, 0xA1, 0xA8, 0x00, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x48, 0xBA, 0x00, 0x00, 0xF0, 0xFF, 0xAF, 0xFF, 0xFF, 0xFF, 0x48, 0x01,
        0xD0, 0x48, 0x3D, 0xFF, 0xFF, 0x00, 0x00, 0x77, 0x62, 0x48, 0xA1, 0xB0, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x01, 0xD0, 0x48, 0x3D, 0xFF, 0xFF, 0x00, 0x00, 0x77, 0x4D,
        0x48, 0xA1, 0xB8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x01, 0xD0, 0x48, 0x3D,
        0xFF, 0xFF, 0x00, 0x00, 0x77, 0x38, 0x48, 0xB8, 0xB0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0x48, 0x8B, 0x10, 0x48, 0xA1, 0xA8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0x39, 0xC2, 0x73, 0x1C, 0x48, 0xB8, 0xB8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0x8B, 0x10, 0x48, 0xA1, 0xB0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xC2,
        0x72, 0x0A, 0xBF, 0x73, 0x00, 0x00, 0x00, 0xE8, 0x5F, 0xF0, 0xFF, 0xFF, 0xBA, 0x00, 0x00,
        0x00, 0x08, 0x48, 0x8D, 0x35, 0x04, 0xF1, 0xFF, 0xFF, 0xBF, 0x0B, 0x00, 0x00, 0x00, 0xE8,
        0x4E, 0xF5, 0xFF, 0xFF, 0x41, 0xBA, 0x32, 0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF,
        0xFF, 0xFF, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0x48, 0xBF, 0x00, 0x00, 0x20, 0x00, 0x50,
        0x00, 0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x02, 0x00, 0xBA, 0x03,
        0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x39, 0xF8, 0x74, 0x0A, 0xBF, 0x80, 0x00, 0x00, 0x00,
        0xE8, 0x0C, 0xF0, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0B, 0x00, 0x00, 0x00, 0x48, 0xBF,
        0x00, 0x00, 0x1F, 0x00, 0x50, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x01, 0x00, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0xBB, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0x03, 0x00, 0x00, 0x00, 0x00, 0xE8, 0x56, 0xF1, 0xFF, 0xFF, 0x48, 0x8B, 0x03,
        0x48, 0x83, 0xF8, 0x01, 0x74, 0x0A, 0xBF, 0x81, 0x00, 0x00, 0x00, 0xE8, 0xB6, 0xEF, 0xFF,
        0xFF, 0x48, 0xA1, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x0B,
        0x75, 0x10, 0x48, 0xA1, 0x70, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8,
        0x01, 0x74, 0x0A, 0xBF, 0x82, 0x00, 0x00, 0x00, 0xE8, 0x8C, 0xEF, 0xFF, 0xFF, 0x48, 0xA1,
        0x78, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00, 0xFE, 0x1F, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x48, 0x39, 0xD0, 0x74, 0x0A, 0xBF, 0x83, 0x00, 0x00, 0x00, 0xE8, 0x69,
        0xEF, 0xFF, 0xFF, 0x48, 0xA1, 0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xBA,
        0x00, 0x00, 0xF0, 0xFF, 0xAF, 0xFF, 0xFF, 0xFF, 0x48, 0x01, 0xD0, 0x48, 0x3D, 0xFF, 0xFF,
        0x00, 0x00, 0x76, 0x0A, 0xBF, 0x84, 0x00, 0x00, 0x00, 0xE8, 0x40, 0xEF, 0xFF, 0xFF, 0x41,
        0xBA, 0x22, 0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0x09, 0x00, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x00, 0x00, 0xBE, 0x00,
        0x00, 0x01, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x3D, 0xFF, 0xEF, 0xFF,
        0xFF, 0x76, 0x0A, 0xBF, 0x90, 0x00, 0x00, 0x00, 0xE8, 0x05, 0xEF, 0xFF, 0xFF, 0x48, 0x8D,
        0xB0, 0x00, 0x00, 0x01, 0x00, 0x49, 0xBA, 0x40, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x8D, 0x25, 0x4A, 0xF1, 0xFF, 0xFF, 0xB8, 0x38,
        0x00, 0x00, 0x00, 0xBF, 0x00, 0x0F, 0x3D, 0x00, 0x4C, 0x89, 0xD2, 0x0F, 0x05, 0x48, 0x85,
        0xC0, 0x75, 0x10, 0x31, 0xED, 0x41, 0xFF, 0xD4, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x31, 0xFF,
        0x0F, 0x05, 0x0F, 0x0B, 0xBB, 0x88, 0x13, 0x00, 0x00, 0x48, 0x85, 0xC0, 0x7E, 0x71, 0x4C,
        0x89, 0xD7, 0x4C, 0x8D, 0x54, 0x24, 0x08, 0xBD, 0xCA, 0x00, 0x00, 0x00, 0x8B, 0x07, 0x85,
        0xC0, 0x74, 0x37, 0x8B, 0x07, 0x85, 0xC0, 0x74, 0x2B, 0x48, 0xC7, 0x44, 0x24, 0x08, 0x00,
        0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24, 0x10, 0x40, 0x42, 0x0F, 0x00, 0x48, 0x63, 0xD0,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xE8, 0x0F, 0x05, 0x48, 0x83, 0xEB, 0x01, 0x75, 0xC3, 0xA1, 0x40,
        0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x85, 0xC0, 0x75, 0x10, 0x48, 0xA1, 0x08, 0x01,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8, 0x01, 0x74, 0x14, 0xBF, 0x92, 0x00,
        0x00, 0x00, 0xE8, 0x48, 0xEE, 0xFF, 0xFF, 0xBF, 0x91, 0x00, 0x00, 0x00, 0xE8, 0x3E, 0xEE,
        0xFF, 0xFF, 0x48, 0xA1, 0xF0, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85, 0xC0,
        0x75, 0x1F, 0x48, 0xA1, 0xF8, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF8,
        0x02, 0x75, 0x0F, 0x48, 0xA1, 0x00, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x85,
        0xC0, 0x74, 0x0A, 0xBF, 0x93, 0x00, 0x00, 0x00, 0xE8, 0x06, 0xEE, 0xFF, 0xFF, 0x41, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x39, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48,
        0x89, 0xD6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x48, 0xBA, 0x00, 0x00, 0x01, 0x00, 0x48,
        0xBF, 0x00, 0x00, 0x10, 0x00, 0x50, 0x00, 0x00, 0x00, 0xE8, 0x55, 0xF2, 0xFF, 0xFF, 0xF7,
        0xD8, 0x48, 0x19, 0xFF, 0x48, 0x83, 0xC7, 0x2C, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xBF, 0xA0, 0x00, 0x00,
        0x00, 0xE8, 0x95, 0xED, 0xFF, 0xFF, 0x7E, 0xF4, 0x48, 0x89, 0xC7, 0xE8, 0x89, 0xEF, 0xFF,
        0xFF, 0x3D, 0x00, 0x2B, 0x00, 0x00, 0x74, 0x0A, 0xBF, 0xA1, 0x00, 0x00, 0x00, 0xE8, 0x7A,
        0xED, 0xFF, 0xFF, 0x48, 0x8D, 0x7C, 0x24, 0x18, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x25, 0x01, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74,
        0x0A, 0xBF, 0xA2, 0x00, 0x00, 0x00, 0xE8, 0x45, 0xED, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0x39, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6,
        0x0F, 0x05, 0x48, 0x89, 0xC3, 0x48, 0x85, 0xC0, 0x75, 0x76, 0x48, 0x63, 0x7C, 0x24, 0x18,
        0xB8, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x63, 0x44, 0x24, 0x1C, 0x48, 0xA3, 0x10,
        0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xBF, 0x0B, 0x00, 0x00, 0x00, 0xE8, 0xF9, 0xF1,
        0xFF, 0xFF, 0xBA, 0x00, 0x00, 0x00, 0x48, 0x48, 0x8D, 0x35, 0x10, 0xEE, 0xFF, 0xFF, 0xBF,
        0x0C, 0x00, 0x00, 0x00, 0xE8, 0xE3, 0xF1, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x27, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xDF, 0x48, 0x89, 0xDE, 0x48, 0x89, 0xDA, 0x0F, 0x05, 0x48, 0x89,
        0xC7, 0xB8, 0x3E, 0x00, 0x00, 0x00, 0xBE, 0x0C, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xBF, 0x2D,
        0x00, 0x00, 0x00, 0xE8, 0xA3, 0xEC, 0xFF, 0xFF, 0x7F, 0x0A, 0xBF, 0xA3, 0x00, 0x00, 0x00,
        0xE8, 0x97, 0xEC, 0xFF, 0xFF, 0x48, 0x63, 0x7C, 0x24, 0x1C, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x89,
        0xDF, 0xE8, 0x67, 0xEE, 0xFF, 0xFF, 0x83, 0xE0, 0x7F, 0x83, 0xF8, 0x0B, 0x74, 0x0A, 0xBF,
        0xA4, 0x00, 0x00, 0x00, 0xE8, 0x57, 0xEC, 0xFF, 0xFF, 0x48, 0x63, 0x7C, 0x24, 0x18, 0x41,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x00, 0x08, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0xBA, 0x00, 0x08, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0x02, 0x7F, 0x0A,
        0xBF, 0xA5, 0x00, 0x00, 0x00, 0xE8, 0x1A, 0xEC, 0xFF, 0xFF, 0x48, 0x63, 0x7C, 0x24, 0x18,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xD6, 0x0F, 0x05, 0xB8, 0x39, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x0F, 0x05, 0x48, 0x85,
        0xC0, 0x75, 0x5B, 0x48, 0xC7, 0x44, 0x24, 0x08, 0x00, 0x04, 0x00, 0x00, 0x48, 0xB8, 0xF8,
        0xFF, 0x21, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0xA3, 0x60, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0x8D, 0x05, 0xB5, 0xED, 0xFF, 0xFF, 0x48, 0xA3, 0x68, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x74, 0x24, 0x08, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00,
        0xB8, 0x0E, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0xB8, 0x00, 0xFE, 0x1F, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0xC7, 0x00, 0x01, 0x00, 0x00, 0x00, 0xBF, 0x2E, 0x00, 0x00, 0x00, 0xE8,
        0x8A, 0xEB, 0xFF, 0xFF, 0x7F, 0x0A, 0xBF, 0xA6, 0x00, 0x00, 0x00, 0xE8, 0x7E, 0xEB, 0xFF,
        0xFF, 0x48, 0x89, 0xC7, 0xE8, 0x74, 0xED, 0xFF, 0xFF, 0x83, 0xE0, 0x7F, 0x83, 0xF8, 0x0B,
        0x74, 0x0A, 0xBF, 0xA7, 0x00, 0x00, 0x00, 0xE8, 0x64, 0xEB, 0xFF, 0xFF, 0xBF, 0x2A, 0x00,
        0x00, 0x00, 0xE8, 0x5A, 0xEB, 0xFF, 0xFF,
    ];
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size = CODE.len() as u64;
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
    buf[cs..(cs + CODE.len())].copy_from_slice(&CODE);

    buf
}

/// Build a Linux-ABI ring-3 test of `SIGPIPE`, a freestanding C program: a
/// write into a pipe nobody reads raises `SIGPIPE` before `EPIPE` is seen -- its
/// default action ends a child, ignored the call is `EPIPE` alone, a handler
/// (`SI_USER`, from the process itself) has run by then, blocked it waits for
/// `sigtimedwait` -- through `write`, `writev`, `vmsplice`, `tee`, `splice` and
/// `sendfile`; on a unix stream socket whose peer is gone or that was shut down
/// for writing, and on a TCP socket never connected (when there is a network);
/// and not with `MSG_NOSIGNAL`, nor on a datagram socket. Exits `0x2A` when
/// every check passes (it does on Linux 6.6, in WSL), otherwise the failed
/// check's code.
///
/// Built with gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, in WSL, linked by `posix_timers.ld`
/// (see [`build_linux_posix_timers_test_elf`]):
///
/// ```text
/// gcc -O1 -std=gnu11 -ffreestanding -fno-builtin -fno-stack-protec
///     tor -fno-asynchronous-unwind-tables -fno-unwind-tables -fcf-protec
///     tion=none -fno-jump-tables -mgeneral-regs-only -fPIE -fvisibility=
///     hidden -nostdlib -static -Wall -Wextra -Wl,-T,posix_timers.ld -Wl,
///     --build-id=none -Wl,-z,norelro -Wl,-z,noexecstack
///     -o sigpipe.elf sigpipe.c
/// objcopy -O binary -j .text sigpipe.elf sigpipe.bin
/// ```
///
/// `sigpipe.c`:
///
/// ```c
/// /* Ring-3 test of SIGPIPE through the Linux ABI (spawn::self_test_linux_sigpipe).
///  * Freestanding: raw syscalls.
///  *
///  * A write into a pipe nobody reads any more raises SIGPIPE before the call
///  * returns EPIPE: its default action ends a child; ignored, the call is EPIPE
///  * alone; handled, the handler has run -- SI_USER, from this process -- by the
///  * time EPIPE is seen; blocked, it waits, pending, for sigtimedwait. The same
///  * for writev, tee, splice, vmsplice and sendfile into such a pipe, for a unix
///  * stream socket whose peer is gone or after shutdown(SHUT_WR), and for a TCP
///  * socket never connected; MSG_NOSIGNAL gives EPIPE alone, and a datagram
///  * socket's EPIPE raises nothing. Exits 0x2A when every check passes,
///  * otherwise the failed check's code. */
///
/// typedef unsigned long u64;
/// typedef long i64;
/// typedef int i32;
/// typedef unsigned int u32;
///
/// #define SHARED 0x5000000000UL /* mailbox, one page */
/// #define BOX ((volatile u64 *)SHARED)
///
/// #define SIGPIPE 13
/// #define EPIPE 32
/// #define SA_SIGINFO 4UL
/// #define SA_RESTORER 0x04000000UL
/// #define MSG_NOSIGNAL 0x4000
/// #define BIT(s) (1UL << ((s) - 1))
///
/// static inline long sc6(long n, long a, long b, long c, long d, long e, long f)
/// {
///     register long r10 __asm__("r10") = d;
///     register long r8 __asm__("r8") = e;
///     register long r9 __asm__("r9") = f;
///     long ret;
///     __asm__ volatile("syscall"
///                      : "=a"(ret)
///                      : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
///                      : "rcx", "r11", "memory");
///     return ret;
/// }
/// #define sc(n, a, b, c, d) sc6((n), (long)(a), (long)(b), (long)(c), (long)(d), 0, 0)
///
/// static void __attribute__((noreturn)) die(int code)
/// {
///     for (;;)
///         sc(231, code, 0, 0, 0); /* exit_group */
/// }
/// #define CHECK(cond, code)  \
///     do {                   \
///         if (!(cond))       \
///             die(code);     \
///     } while (0)
///
/// void *memset(void *d, int c, u64 n)
/// {
///     volatile unsigned char *p = d;
///     while (n--)
///         *p++ = (unsigned char)c;
///     return d;
/// }
///
/// void *memcpy(void *d, const void *s, u64 n)
/// {
///     volatile unsigned char *p = d;
///     const volatile unsigned char *q = s;
///     while (n--)
///         *p++ = *q++;
///     return d;
/// }
///
/// struct kact { u64 handler, flags, restorer, mask; };
/// struct si { i32 signo, err, code, pad; i32 pid; u32 uid; u64 rest[13]; };
///
/// /* Mailbox (8-byte words at SHARED):
///  *   [0] handler runs   [1] its signal   [2] its si_code   [3] its si_pid
///  *   [8] scratch for iovecs; [16..] a byte to write */
///
/// static void handler(int sig, struct si *info, void *uc)
/// {
///     (void)uc;
///     BOX[1] = (u64)sig;
///     BOX[2] = (u64)(u32)info->code;
///     BOX[3] = (u64)(u32)info->pid;
///     BOX[0] += 1;
/// }
///
/// __attribute__((naked)) static void restorer(void)
/// {
///     __asm__ volatile("mov $15, %eax\n\tsyscall\n\tud2");
/// }
///
/// /* SIGPIPE's action: 0 for SIG_DFL, 1 for SIG_IGN, else the handler. */
/// static void action(u64 what)
/// {
///     struct kact a;
///     memset(&a, 0, sizeof a);
///     a.handler = what;
///     if (what > 1) {
///         a.handler = (u64)handler;
///         a.flags = SA_SIGINFO | SA_RESTORER;
///         a.restorer = (u64)restorer;
///     }
///     CHECK(sc(13, SIGPIPE, &a, 0, 8) == 0, 0x3F);
/// }
///
/// /* A pipe whose read end is closed: the write end. */
/// static int broken_pipe(void)
/// {
///     int fds[2];
///     CHECK(sc(293, fds, 0, 0, 0) == 0, 0x30); /* pipe2 */
///     CHECK(sc(3, fds[0], 0, 0, 0) == 0, 0x30);
///     return fds[1];
/// }
///
/// /* A unix socket pair of `type`, the peer of the returned end closed. */
/// static int lonely_socket(int type)
/// {
///     int sv[2];
///     CHECK(sc(53, 1, type, 0, sv) == 0, 0x31); /* socketpair(AF_UNIX) */
///     CHECK(sc(3, sv[1], 0, 0, 0) == 0, 0x31);
///     return sv[0];
/// }
///
/// /* Did `ret` come back EPIPE with exactly `runs` handler runs so far, each a
///  * SIGPIPE from this process (SI_USER)? */
/// static int epipe_after(long ret, u64 runs, long pid)
/// {
///     if (ret != -EPIPE || BOX[0] != runs)
///         return 0;
///     return runs == 0 || (BOX[1] == SIGPIPE && BOX[2] == 0 && (long)BOX[3] == pid);
/// }
///
/// __attribute__((used, noreturn)) void main_(void)
/// {
///     long pid, child, got;
///     int fd, fd2, status, fds[2];
///     u64 iov[2];
///     u64 runs = 0;
///
///     CHECK(sc6(9, SHARED, 4096, 3, 0x32, -1, 0) == (long)SHARED, 0x30); /* mailbox */
///     for (got = 0; got < 64; got++)
///         BOX[got] = 0;
///     BOX[16] = 'x';
///     pid = sc(39, 0, 0, 0, 0);
///
///     /* ---- the default action ends the writer ---- */
///     child = sc(57, 0, 0, 0, 0); /* fork */
///     if (child == 0) {
///         sc(1, broken_pipe(), SHARED + 128, 1, 0);
///         die(0x2C); /* not reached */
///     }
///     CHECK(child > 0, 0x32);
///     status = -1;
///     CHECK(sc(61, child, &status, 0, 0) == child, 0x33);
///     CHECK((status & 0x7F) == SIGPIPE, 0x34);
///
///     /* ---- ignored: EPIPE alone ---- */
///     action(1);
///     fd = broken_pipe();
///     CHECK(sc(1, fd, SHARED + 128, 1, 0) == -EPIPE, 0x40);
///     sc(3, fd, 0, 0, 0);
///
///     /* ---- handled: the handler has run when EPIPE is seen ---- */
///     action(2);
///     fd = broken_pipe();
///     CHECK(epipe_after(sc(1, fd, SHARED + 128, 1, 0), ++runs, pid), 0x41); /* write */
///     iov[0] = SHARED + 128;
///     iov[1] = 1;
///     CHECK(epipe_after(sc(20, fd, iov, 1, 0), ++runs, pid), 0x42); /* writev */
///     CHECK(epipe_after(sc(278, fd, iov, 1, 0), ++runs, pid), 0x43); /* vmsplice */
///     {
///         int src[2];
///         CHECK(sc(293, src, 0, 0, 0) == 0, 0x44);
///         CHECK(sc(1, src[1], SHARED + 128, 1, 0) == 1, 0x44);
///         CHECK(epipe_after(sc(276, src[0], fd, 1, 0), ++runs, pid), 0x45); /* tee */
///         CHECK(epipe_after(sc6(275, src[0], 0, fd, 0, 1, 0), ++runs, pid), 0x46); /* splice */
///         sc(3, src[0], 0, 0, 0);
///         sc(3, src[1], 0, 0, 0);
///     }
///     {
///         /* sendfile from a memfd holding one byte. */
///         long mfd = sc(319, SHARED + 256, 0, 0, 0); /* memfd_create("") */
///         CHECK(mfd >= 0, 0x47);
///         CHECK(sc(1, mfd, SHARED + 128, 1, 0) == 1, 0x47);
///         CHECK(sc(8, mfd, 0, 0, 0) == 0, 0x47); /* lseek to 0 */
///         CHECK(epipe_after(sc(40, fd, mfd, 0, 1), ++runs, pid), 0x48);
///         sc(3, mfd, 0, 0, 0);
///     }
///     sc(3, fd, 0, 0, 0);
///
///     /* ---- a unix stream socket ---- */
///     fd = lonely_socket(1); /* SOCK_STREAM, peer gone */
///     CHECK(epipe_after(sc(1, fd, SHARED + 128, 1, 0), ++runs, pid), 0x50);             /* write */
///     CHECK(epipe_after(sc6(44, fd, SHARED + 128, 1, 0, 0, 0), ++runs, pid), 0x51);     /* send */
///     CHECK(epipe_after(sc6(44, fd, SHARED + 128, 1, MSG_NOSIGNAL, 0, 0), runs, pid), 0x52);
///     sc(3, fd, 0, 0, 0);
///     CHECK(sc(53, 1, 1, 0, fds) == 0, 0x53);
///     CHECK(sc(48, fds[0], 1, 0, 0) == 0, 0x53); /* shutdown(SHUT_WR) */
///     CHECK(epipe_after(sc(1, fds[0], SHARED + 128, 1, 0), ++runs, pid), 0x54);
///     sc(3, fds[0], 0, 0, 0);
///     sc(3, fds[1], 0, 0, 0);
///
///     /* ---- a datagram socket's EPIPE raises nothing ---- */
///     CHECK(sc(53, 1, 2, 0, fds) == 0, 0x55); /* SOCK_DGRAM pair */
///     CHECK(sc(48, fds[0], 1, 0, 0) == 0, 0x55); /* shutdown(SHUT_WR) */
///     got = sc6(44, fds[0], SHARED + 128, 1, 0, 0, 0);
///     CHECK(got == -EPIPE && BOX[0] == runs, 0x56);
///     sc(3, fds[0], 0, 0, 0);
///     sc(3, fds[1], 0, 0, 0);
///
///     /* ---- a TCP socket never connected (when there is a network) ---- */
///     fd2 = (int)sc(41, 2, 1, 0, 0); /* socket(AF_INET, SOCK_STREAM) */
///     if (fd2 >= 0) {
///         CHECK(epipe_after(sc(1, fd2, SHARED + 128, 1, 0), ++runs, pid), 0x58);
///         CHECK(epipe_after(sc6(44, fd2, SHARED + 128, 1, MSG_NOSIGNAL, 0, 0), runs, pid), 0x59);
///         sc(3, fd2, 0, 0, 0);
///     }
///
///     /* ---- blocked: pending, for sigtimedwait ---- */
///     {
///         u64 set = BIT(SIGPIPE), pending = 0;
///         i64 zero[2] = { 0, 0 };
///         struct si info;
///         CHECK(sc(14, 0, &set, 0, 8) == 0, 0x60); /* SIG_BLOCK */
///         fd = broken_pipe();
///         CHECK(sc(1, fd, SHARED + 128, 1, 0) == -EPIPE && BOX[0] == runs, 0x61);
///         CHECK(sc(127, &pending, 8, 0, 0) == 0 && (pending & set), 0x62); /* rt_sigpending */
///         memset(&info, 0, sizeof info);
///         CHECK(sc(128, &set, &info, zero, 8) == SIGPIPE, 0x63); /* rt_sigtimedwait */
///         CHECK(info.code == 0 && info.pid == pid, 0x64);
///         sc(3, fd, 0, 0, 0);
///     }
///     die(0x2A);
/// }
///
/// __attribute__((naked, section(".text.start"))) void _start(void)
/// {
///     __asm__ volatile("and $-16, %rsp\n\tcall main_\n\tud2");
/// }
/// ```
///
/// Paired with [`crate::proc::spawn::self_test_linux_sigpipe`]. Tagged
/// `ELFOSABI_GNU` so `spawn_process` routes it through the Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_linux_sigpipe_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const CODE: [u8; 3214] = [
        0x48, 0x83, 0xE4, 0xF0, 0xE8, 0x22, 0x02, 0x00, 0x00, 0x0F, 0x0B, 0x0F, 0x0B, 0x53, 0x48,
        0x63, 0xFF, 0xBB, 0xE7, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xD8, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xEB, 0xDF, 0x48, 0x63, 0xC7, 0x48, 0xA3,
        0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x8B, 0x46, 0x08, 0x48, 0xA3, 0x10, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x8B, 0x46, 0x10, 0x48, 0xA3, 0x18, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0x8B, 0x02, 0x48, 0x83, 0xC0, 0x01, 0x48, 0x89, 0x02, 0xC3, 0xB8, 0x0F, 0x00, 0x00, 0x00,
        0x0F, 0x05, 0x0F, 0x0B, 0x0F, 0x0B, 0x48, 0x83, 0xEC, 0x10, 0x48, 0x8D, 0x7C, 0x24, 0x08,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x25, 0x01, 0x00, 0x00, 0x48, 0x89,
        0xD6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x34, 0x48, 0x63, 0x7C, 0x24, 0x08, 0x41, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F,
        0x05, 0x48, 0x85, 0xC0, 0x75, 0x13, 0x8B, 0x44, 0x24, 0x0C, 0x48, 0x83, 0xC4, 0x10, 0xC3,
        0xBF, 0x30, 0x00, 0x00, 0x00, 0xE8, 0x22, 0xFF, 0xFF, 0xFF, 0xBF, 0x30, 0x00, 0x00, 0x00,
        0xE8, 0x18, 0xFF, 0xFF, 0xFF, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83, 0xFF, 0xE0, 0x75,
        0x39, 0x48, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x08, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x48, 0x39, 0xF1, 0x75, 0x22, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48,
        0x85, 0xF6, 0x74, 0x18, 0x48, 0xB8, 0x08, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48,
        0x8B, 0x08, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83, 0xF9, 0x0D, 0x74, 0x01, 0xC3, 0x48,
        0xB8, 0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x08, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x48, 0x85, 0xC9, 0x75, 0xE8, 0x48, 0xA1, 0x18, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0x39, 0xD0, 0x0F, 0x94, 0xC0, 0x0F, 0xB6, 0xC0, 0xC3, 0x48, 0x89, 0xF8,
        0x48, 0x85, 0xD2, 0x74, 0x15, 0x48, 0x01, 0xFA, 0x48, 0x89, 0xF9, 0x49, 0x89, 0xC8, 0x48,
        0x83, 0xC1, 0x01, 0x41, 0x88, 0x30, 0x48, 0x39, 0xCA, 0x75, 0xF1, 0xC3, 0x53, 0x48, 0x83,
        0xEC, 0x20, 0x48, 0x89, 0xFB, 0x48, 0x89, 0xE7, 0xBA, 0x20, 0x00, 0x00, 0x00, 0xBE, 0x00,
        0x00, 0x00, 0x00, 0xE8, 0xC8, 0xFF, 0xFF, 0xFF, 0x48, 0x83, 0xFB, 0x01, 0x76, 0x4F, 0x48,
        0x8D, 0x05, 0x8D, 0xFE, 0xFF, 0xFF, 0x48, 0x89, 0x04, 0x24, 0x48, 0xC7, 0x44, 0x24, 0x08,
        0x04, 0x00, 0x00, 0x04, 0x48, 0x8D, 0x05, 0xB5, 0xFE, 0xFF, 0xFF, 0x48, 0x89, 0x44, 0x24,
        0x10, 0x48, 0x89, 0xE6, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0D, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xC7, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x0C, 0x48, 0x83, 0xC4,
        0x20, 0x5B, 0xC3, 0x48, 0x89, 0x1C, 0x24, 0xEB, 0xCB, 0xBF, 0x3F, 0x00, 0x00, 0x00, 0xE8,
        0x0B, 0xFE, 0xFF, 0xFF, 0x48, 0x89, 0xF8, 0x48, 0x85, 0xD2, 0x74, 0x20, 0x48, 0x01, 0xFA,
        0x48, 0x89, 0xF9, 0x49, 0x89, 0xF1, 0x48, 0x83, 0xC6, 0x01, 0x49, 0x89, 0xC8, 0x48, 0x83,
        0xC1, 0x01, 0x45, 0x0F, 0xB6, 0x09, 0x45, 0x88, 0x08, 0x48, 0x39, 0xCA, 0x75, 0xE6, 0xC3,
        0x41, 0x55, 0x41, 0x54, 0x55, 0x53, 0x48, 0x81, 0xEC, 0xC0, 0x00, 0x00, 0x00, 0x41, 0xBA,
        0x32, 0x00, 0x00, 0x00, 0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0x48, 0xBF, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0xB8, 0x09, 0x00,
        0x00, 0x00, 0xBE, 0x00, 0x10, 0x00, 0x00, 0xBA, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48,
        0x89, 0xFA, 0x48, 0xB9, 0x00, 0x02, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x39, 0xF8,
        0x0F, 0x85, 0xAC, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x02, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83,
        0xC2, 0x08, 0x48, 0x39, 0xCA, 0x75, 0xF0, 0x48, 0xB8, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0xC7, 0x00, 0x78, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0x27, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD7, 0x48, 0x89, 0xD6, 0x0F, 0x05,
        0x48, 0x89, 0xC3, 0xB8, 0x39, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x89, 0xC7, 0x48, 0x85,
        0xC0, 0x74, 0x5F, 0x0F, 0x8E, 0x91, 0x00, 0x00, 0x00, 0xC7, 0x84, 0x24, 0xBC, 0x00, 0x00,
        0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0xB4, 0x24, 0xBC, 0x00, 0x00, 0x00, 0x41, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x3D, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x39,
        0xC7, 0x75, 0x65, 0x8B, 0x84, 0x24, 0xBC, 0x00, 0x00, 0x00, 0x83, 0xE0, 0x7F, 0x83, 0xF8,
        0x0D, 0x74, 0x60, 0xBF, 0x34, 0x00, 0x00, 0x00, 0xE8, 0xE5, 0xFC, 0xFF, 0xFF, 0xBF, 0x30,
        0x00, 0x00, 0x00, 0xE8, 0xDB, 0xFC, 0xFF, 0xFF, 0xE8, 0x47, 0xFD, 0xFF, 0xFF, 0x48, 0x63,
        0xF8, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00, 0x00,
        0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0xBF, 0x2C, 0x00, 0x00, 0x00, 0xE8,
        0xA3, 0xFC, 0xFF, 0xFF, 0xBF, 0x32, 0x00, 0x00, 0x00, 0xE8, 0x99, 0xFC, 0xFF, 0xFF, 0xBF,
        0x33, 0x00, 0x00, 0x00, 0xE8, 0x8F, 0xFC, 0xFF, 0xFF, 0xBF, 0x01, 0x00, 0x00, 0x00, 0xE8,
        0xFB, 0xFD, 0xFF, 0xFF, 0xE8, 0xF1, 0xFC, 0xFF, 0xFF, 0x48, 0x63, 0xF8, 0x41, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xB8, 0x01, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xE0, 0x74, 0x0A, 0xBF, 0x40, 0x00, 0x00,
        0x00, 0xE8, 0x47, 0xFC, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8,
        0x03, 0x00, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xBF, 0x02, 0x00, 0x00, 0x00, 0xE8,
        0x92, 0xFD, 0xFF, 0xFF, 0xE8, 0x88, 0xFC, 0xFF, 0xFF, 0x48, 0x63, 0xE8, 0x41, 0xBA, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00,
        0xB8, 0x01, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xEF, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48, 0x89, 0xDA, 0xBE, 0x01, 0x00, 0x00,
        0x00, 0x48, 0x89, 0xC7, 0xE8, 0xC3, 0xFC, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x41,
        0x00, 0x00, 0x00, 0xE8, 0xCD, 0xFB, 0xFF, 0xFF, 0x48, 0xB8, 0x80, 0x00, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0x48, 0x89, 0x84, 0x24, 0xA0, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x84, 0x24,
        0xA8, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x4C, 0x8D, 0xA4, 0x24, 0xA0, 0x00, 0x00,
        0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0x14, 0x00, 0x00, 0x00, 0xBA, 0x01, 0x00, 0x00, 0x00, 0x48,
        0x89, 0xEF, 0x4C, 0x89, 0xE6, 0x0F, 0x05, 0x48, 0x89, 0xDA, 0xBE, 0x02, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xC7, 0xE8, 0x5B, 0xFC, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x42, 0x00,
        0x00, 0x00, 0xE8, 0x65, 0xFB, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x16, 0x01, 0x00, 0x00,
        0xBA, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x4C, 0x89, 0xE6, 0x0F, 0x05, 0x48, 0x89,
        0xDA, 0xBE, 0x03, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0x19, 0xFC, 0xFF, 0xFF, 0x85,
        0xC0, 0x74, 0x66, 0x48, 0x8D, 0x7C, 0x24, 0x20, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x25, 0x01, 0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75,
        0x45, 0x48, 0x63, 0x7C, 0x24, 0x24, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48,
        0xBE, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48,
        0x83, 0xF8, 0x01, 0x74, 0x1E, 0xBF, 0x44, 0x00, 0x00, 0x00, 0xE8, 0xC7, 0xFA, 0xFF, 0xFF,
        0xBF, 0x43, 0x00, 0x00, 0x00, 0xE8, 0xBD, 0xFA, 0xFF, 0xFF, 0xBF, 0x44, 0x00, 0x00, 0x00,
        0xE8, 0xB3, 0xFA, 0xFF, 0xFF, 0x48, 0x63, 0x7C, 0x24, 0x20, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x14,
        0x01, 0x00, 0x00, 0xBA, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x89,
        0xDA, 0xBE, 0x04, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0x65, 0xFB, 0xFF, 0xFF, 0x85,
        0xC0, 0x75, 0x0A, 0xBF, 0x45, 0x00, 0x00, 0x00, 0xE8, 0x6F, 0xFA, 0xFF, 0xFF, 0x48, 0x63,
        0x7C, 0x24, 0x20, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x01, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x13, 0x01, 0x00, 0x00, 0xBE, 0x00, 0x00, 0x00,
        0x00, 0x48, 0x89, 0xEA, 0x0F, 0x05, 0x48, 0x89, 0xDA, 0xBE, 0x05, 0x00, 0x00, 0x00, 0x48,
        0x89, 0xC7, 0xE8, 0x21, 0xFB, 0xFF, 0xFF, 0x85, 0xC0, 0x0F, 0x84, 0xAC, 0x00, 0x00, 0x00,
        0x48, 0x63, 0x7C, 0x24, 0x20, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0x41, 0xBC, 0x03, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE0, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x63, 0x7C,
        0x24, 0x24, 0x4C, 0x89, 0xE0, 0x0F, 0x05, 0xB8, 0x3F, 0x01, 0x00, 0x00, 0x48, 0xBF, 0x00,
        0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x49, 0x89, 0xC4, 0x48, 0x85, 0xC0,
        0x78, 0x69, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE7, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48, 0x83,
        0xF8, 0x01, 0x75, 0x44, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x08, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x28, 0xBF, 0x47, 0x00,
        0x00, 0x00, 0xE8, 0x85, 0xF9, 0xFF, 0xFF, 0xBF, 0x46, 0x00, 0x00, 0x00, 0xE8, 0x7B, 0xF9,
        0xFF, 0xFF, 0xBF, 0x47, 0x00, 0x00, 0x00, 0xE8, 0x71, 0xF9, 0xFF, 0xFF, 0xBF, 0x47, 0x00,
        0x00, 0x00, 0xE8, 0x67, 0xF9, 0xFF, 0xFF, 0x41, 0xBA, 0x01, 0x00, 0x00, 0x00, 0x41, 0xB8,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x28, 0x00, 0x00, 0x00,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x4C, 0x89, 0xE6, 0x0F, 0x05, 0x48, 0x89,
        0xDA, 0xBE, 0x06, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0x1B, 0xFA, 0xFF, 0xFF, 0x85,
        0xC0, 0x75, 0x0A, 0xBF, 0x48, 0x00, 0x00, 0x00, 0xE8, 0x25, 0xF9, 0xFF, 0xFF, 0x41, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xBD, 0x03, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE8,
        0x4C, 0x89, 0xE7, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x4C, 0x89, 0xE8, 0x48, 0x89, 0xEF, 0x0F,
        0x05, 0x4C, 0x8D, 0x54, 0x24, 0x20, 0xBE, 0x01, 0x00, 0x00, 0x00, 0xB8, 0x35, 0x00, 0x00,
        0x00, 0x48, 0x89, 0xF7, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x77, 0x48, 0x63, 0x7C, 0x24,
        0x24, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x48,
        0x89, 0xD6, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x56, 0x48, 0x63, 0x6C, 0x24, 0x20, 0x41,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xEF, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48, 0x89, 0xDA, 0xBE, 0x07,
        0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0x5B, 0xF9, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x1E,
        0xBF, 0x50, 0x00, 0x00, 0x00, 0xE8, 0x65, 0xF8, 0xFF, 0xFF, 0xBF, 0x31, 0x00, 0x00, 0x00,
        0xE8, 0x5B, 0xF8, 0xFF, 0xFF, 0xBF, 0x31, 0x00, 0x00, 0x00, 0xE8, 0x51, 0xF8, 0xFF, 0xFF,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xB8, 0x2C, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00, 0x00, 0x50,
        0x00, 0x00, 0x00, 0xBA, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x0F, 0x05, 0x48, 0x89,
        0xDA, 0xBE, 0x08, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0xFE, 0xF8, 0xFF, 0xFF, 0x85,
        0xC0, 0x75, 0x0A, 0xBF, 0x51, 0x00, 0x00, 0x00, 0xE8, 0x08, 0xF8, 0xFF, 0xFF, 0x41, 0xBA,
        0x00, 0x40, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x2C, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0xBA, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x0F, 0x05, 0x48, 0x89, 0xDA, 0xBE,
        0x08, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0xB5, 0xF8, 0xFF, 0xFF, 0x85, 0xC0, 0x74,
        0x7D, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x48,
        0x89, 0xEF, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x4C, 0x8D, 0xA4, 0x24, 0xB4, 0x00, 0x00, 0x00,
        0x4D, 0x89, 0xE2, 0xBE, 0x01, 0x00, 0x00, 0x00, 0xB8, 0x35, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xF7, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x44, 0x48, 0x63, 0xBC, 0x24, 0xB4, 0x00, 0x00,
        0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9,
        0x00, 0x00, 0x00, 0x00, 0xB8, 0x30, 0x00, 0x00, 0x00, 0xBE, 0x01, 0x00, 0x00, 0x00, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x1E, 0xBF, 0x53, 0x00, 0x00,
        0x00, 0xE8, 0x4C, 0xF7, 0xFF, 0xFF, 0xBF, 0x52, 0x00, 0x00, 0x00, 0xE8, 0x42, 0xF7, 0xFF,
        0xFF, 0xBF, 0x53, 0x00, 0x00, 0x00, 0xE8, 0x38, 0xF7, 0xFF, 0xFF, 0x48, 0x63, 0xBC, 0x24,
        0xB4, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48, 0x89, 0xDA,
        0xBE, 0x09, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0xE2, 0xF7, 0xFF, 0xFF, 0x85, 0xC0,
        0x0F, 0x84, 0xD9, 0x00, 0x00, 0x00, 0x48, 0x63, 0xBC, 0x24, 0xB4, 0x00, 0x00, 0x00, 0x41,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00,
        0x00, 0x00, 0xBD, 0x03, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xE8,
        0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x63, 0xBC, 0x24, 0xB8, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xE8, 0x0F, 0x05, 0x4D, 0x89, 0xE2, 0xB8, 0x35, 0x00, 0x00, 0x00, 0xBF, 0x01, 0x00, 0x00,
        0x00, 0xBE, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x0F, 0x85, 0x8D, 0x00,
        0x00, 0x00, 0x48, 0x63, 0xBC, 0x24, 0xB4, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x30,
        0x00, 0x00, 0x00, 0xBE, 0x01, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05,
        0x48, 0x85, 0xC0, 0x75, 0x67, 0x48, 0x63, 0xBC, 0x24, 0xB4, 0x00, 0x00, 0x00, 0x41, 0xBA,
        0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00,
        0x00, 0xB8, 0x2C, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00,
        0x00, 0xBA, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0xE0, 0x75, 0x13, 0x48,
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x28, 0x48, 0x83, 0xFD,
        0x09, 0x74, 0x28, 0xBF, 0x56, 0x00, 0x00, 0x00, 0xE8, 0x19, 0xF6, 0xFF, 0xFF, 0xBF, 0x54,
        0x00, 0x00, 0x00, 0xE8, 0x0F, 0xF6, 0xFF, 0xFF, 0xBF, 0x55, 0x00, 0x00, 0x00, 0xE8, 0x05,
        0xF6, 0xFF, 0xFF, 0xBF, 0x55, 0x00, 0x00, 0x00, 0xE8, 0xFB, 0xF5, 0xFF, 0xFF, 0x48, 0x63,
        0xBC, 0x24, 0xB4, 0x00, 0x00, 0x00, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00,
        0x00, 0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0x41, 0xBC, 0x03, 0x00, 0x00, 0x00,
        0xBA, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE0, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0x48, 0x63,
        0xBC, 0x24, 0xB8, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE0, 0x0F, 0x05, 0xB8, 0x29, 0x00, 0x00,
        0x00, 0xBF, 0x02, 0x00, 0x00, 0x00, 0xBE, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x85, 0xC0,
        0x0F, 0x88, 0xAA, 0x00, 0x00, 0x00, 0x48, 0x63, 0xE8, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48,
        0xBE, 0x80, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x48, 0x89, 0xC2,
        0x0F, 0x05, 0x48, 0x89, 0xDA, 0xBE, 0x0A, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0x66,
        0xF6, 0xFF, 0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x58, 0x00, 0x00, 0x00, 0xE8, 0x70, 0xF5,
        0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x40, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x2C, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00, 0x00,
        0x00, 0x50, 0x00, 0x00, 0x00, 0xBA, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0x0F, 0x05,
        0x48, 0x89, 0xDA, 0xBE, 0x0A, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC7, 0xE8, 0x1D, 0xF6, 0xFF,
        0xFF, 0x85, 0xC0, 0x75, 0x0A, 0xBF, 0x59, 0x00, 0x00, 0x00, 0xE8, 0x27, 0xF5, 0xFF, 0xFF,
        0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB9, 0x00,
        0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x48, 0x89,
        0xEF, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xBD, 0x0A, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x04, 0x24,
        0x00, 0x10, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24, 0x08, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7,
        0x44, 0x24, 0x10, 0x00, 0x00, 0x00, 0x00, 0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x89, 0xE5, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x0E, 0x00,
        0x00, 0x00, 0x48, 0x89, 0xD7, 0x4C, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x74, 0x0A,
        0xBF, 0x60, 0x00, 0x00, 0x00, 0xE8, 0xA5, 0xF4, 0xFF, 0xFF, 0xE8, 0x11, 0xF5, 0xFF, 0xFF,
        0x4C, 0x63, 0xE0, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x48, 0xBE, 0x80, 0x00,
        0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xE7, 0x48, 0x89, 0xC2, 0x0F, 0x05, 0x48,
        0x83, 0xF8, 0xE0, 0x75, 0x0F, 0x48, 0xA1, 0x00, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00,
        0x48, 0x39, 0xE8, 0x74, 0x0A, 0xBF, 0x61, 0x00, 0x00, 0x00, 0xE8, 0x55, 0xF4, 0xFF, 0xFF,
        0x48, 0x8D, 0x7C, 0x24, 0x08, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00,
        0x00, 0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x7F, 0x00, 0x00, 0x00, 0xBE, 0x08,
        0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x05, 0x48, 0x85, 0xC0, 0x75, 0x0B,
        0x48, 0x8B, 0x44, 0x24, 0x08, 0x48, 0x23, 0x04, 0x24, 0x75, 0x0A, 0xBF, 0x62, 0x00, 0x00,
        0x00, 0xE8, 0x13, 0xF4, 0xFF, 0xFF, 0x48, 0x8D, 0x6C, 0x24, 0x20, 0xBA, 0x80, 0x00, 0x00,
        0x00, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xEF, 0xE8, 0x54, 0xF5, 0xFF, 0xFF, 0x48,
        0x8D, 0x54, 0x24, 0x10, 0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x80, 0x00, 0x00, 0x00, 0x4C, 0x89, 0xEF,
        0x48, 0x89, 0xEE, 0x0F, 0x05, 0x48, 0x83, 0xF8, 0x0D, 0x74, 0x0A, 0xBF, 0x63, 0x00, 0x00,
        0x00, 0xE8, 0xC8, 0xF3, 0xFF, 0xFF, 0x83, 0x7C, 0x24, 0x28, 0x00, 0x75, 0x0A, 0x48, 0x63,
        0x44, 0x24, 0x30, 0x48, 0x39, 0xD8, 0x74, 0x0A, 0xBF, 0x64, 0x00, 0x00, 0x00, 0xE8, 0xAD,
        0xF3, 0xFF, 0xFF, 0x41, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x41, 0xB8, 0x00, 0x00, 0x00, 0x00,
        0x41, 0xB9, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x00, 0x00, 0x00, 0x00, 0xB8, 0x03, 0x00, 0x00,
        0x00, 0x4C, 0x89, 0xE7, 0x48, 0x89, 0xD6, 0x0F, 0x05, 0xBF, 0x2A, 0x00, 0x00, 0x00, 0xE8,
        0x7F, 0xF3, 0xFF, 0xFF,
    ];
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let code_size = CODE.len() as u64;
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
    buf[cs..(cs + CODE.len())].copy_from_slice(&CODE);

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

/// Build a **Linux-ABI** `ET_EXEC` program that drives the SlateOS channel
/// descriptors (`slate_channel_create`, number 1000) from ring 3:
///
/// ```text
///   slate_channel_create(&fds, 0)      ; 0, else exit 0xC1
///   write(fds[0], "hi!", 3)            ; 3, else exit 0xC2
///   read(fds[1], buf, 16)              ; 3 (one message), else 0xC3
///                                      ; the bytes "hi!", else 0xC4
///   fork()                             ; < 0: exit 0xC5
///   child:  write(fds[0], "c", 1); exit(0)
///   parent: wait4(-1, NULL, 0, NULL)
///           read(fds[1], buf, 16)      ; 1 and 'c', else exit 0xC6
///           close(fds[0])
///           read(fds[1], buf, 16)      ; 0 (end of file), else exit 0xC7
///           exit(0x5C)
/// ```
///
/// A clean `exit(0x5C)` proves, from a real Linux-ABI process:
/// - the call made two descriptors and wrote their numbers back;
/// - one `write` was one message, read back whole, by number of bytes;
/// - `fork` shared the channel ends: the child's write through its
///   inherited `fds[0]` reached the parent's `fds[1]`;
/// - the end closed only with its last holder: the child's exit dropped one
///   holder of each end, the parent's `close` the other, and only then did
///   the read see end of file.
///
/// Tagged `ELFOSABI_GNU` for the SysV stack + Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
pub fn build_linux_slate_channel_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;
    /// `jnz rel32`'s second opcode byte.
    const JNZ: u8 = 0x85;
    /// `js rel32`'s second opcode byte.
    const JS: u8 = 0x88;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    // (offset of a rel32 to patch, the failure sentinel it jumps to)
    let mut fail_jumps: alloc::vec::Vec<(usize, u8)> = alloc::vec::Vec::new();
    // `jcc rel32` (0F op) to the failure exit for `sentinel`.
    let jcc_fail = |code: &mut alloc::vec::Vec<u8>,
                    fails: &mut alloc::vec::Vec<(usize, u8)>,
                    op: u8,
                    sentinel: u8| {
        code.extend_from_slice(&[0x0F, op, 0, 0, 0, 0]);
        fails.push((code.len() - 4, sentinel));
    };

    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x40]); // sub rsp, 64
    // slate_channel_create(&fds, 0): fds at [rsp], [rsp+4]
    code.extend_from_slice(&[0x48, 0x89, 0xE7]); // mov rdi, rsp
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi
    code.extend_from_slice(&[0xB8, 0xE8, 0x03, 0x00, 0x00, 0x0F, 0x05]); // mov eax,1000; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC1);
    // "hi!" at [rsp+16]
    code.extend_from_slice(&[0xC7, 0x44, 0x24, 0x10, 0x68, 0x69, 0x21, 0x00]);
    // write(fds[0], rsp+16, 3)
    code.extend_from_slice(&[0x8B, 0x3C, 0x24]); // mov edi, [rsp]
    code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x10]); // lea rsi, [rsp+16]
    code.extend_from_slice(&[0xBA, 0x03, 0x00, 0x00, 0x00]); // mov edx, 3
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,1; syscall
    code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x03]); // cmp rax, 3
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC2);
    // read(fds[1], rsp+32, 16)
    code.extend_from_slice(&[0x8B, 0x7C, 0x24, 0x04]); // mov edi, [rsp+4]
    code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x20]); // lea rsi, [rsp+32]
    code.extend_from_slice(&[0xBA, 0x10, 0x00, 0x00, 0x00]); // mov edx, 16
    code.extend_from_slice(&[0x31, 0xC0, 0x0F, 0x05]); // xor eax, eax; syscall
    code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x03]); // cmp rax, 3
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC3);
    code.extend_from_slice(&[0x8B, 0x44, 0x24, 0x20]); // mov eax, [rsp+32]
    code.extend_from_slice(&[0x25, 0xFF, 0xFF, 0xFF, 0x00]); // and eax, 0x00FFFFFF
    code.extend_from_slice(&[0x3D, 0x68, 0x69, 0x21, 0x00]); // cmp eax, "hi!"
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC4);
    // fork
    code.extend_from_slice(&[0xB8, 0x39, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,57; syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    jcc_fail(&mut code, &mut fail_jumps, JS, 0xC5);
    code.extend_from_slice(&[0x0F, 0x84, 0, 0, 0, 0]); // jz child
    let jz_child = code.len() - 4;
    // parent: wait4(-1, NULL, 0, NULL)
    code.extend_from_slice(&[0x48, 0xC7, 0xC7, 0xFF, 0xFF, 0xFF, 0xFF]); // mov rdi, -1
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    code.extend_from_slice(&[0x45, 0x31, 0xD2]); // xor r10d, r10d
    code.extend_from_slice(&[0xB8, 0x3D, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,61; syscall
    // the child's message: read(fds[1], rsp+32, 16) == 1, 'c'
    code.extend_from_slice(&[0x8B, 0x7C, 0x24, 0x04]);
    code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x20]);
    code.extend_from_slice(&[0xBA, 0x10, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0x31, 0xC0, 0x0F, 0x05]);
    code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x01]); // cmp rax, 1
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC6);
    code.extend_from_slice(&[0x80, 0x7C, 0x24, 0x20, 0x63]); // cmp byte [rsp+32], 'c'
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC6);
    // close(fds[0])
    code.extend_from_slice(&[0x8B, 0x3C, 0x24]); // mov edi, [rsp]
    code.extend_from_slice(&[0xB8, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,3; syscall
    // read(fds[1], ...) == 0: every holder of the write end is gone
    code.extend_from_slice(&[0x8B, 0x7C, 0x24, 0x04]);
    code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x20]);
    code.extend_from_slice(&[0xBA, 0x10, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0x31, 0xC0, 0x0F, 0x05]);
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC7);
    // exit(0x5C)
    code.extend_from_slice(&[0xBF, 0x5C, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // child: write(fds[0], "c", 1); exit(0)
    let child = code.len();
    code.extend_from_slice(&[0xC6, 0x44, 0x24, 0x10, 0x63]); // mov byte [rsp+16], 'c'
    code.extend_from_slice(&[0x8B, 0x3C, 0x24]); // mov edi, [rsp]
    code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x10]); // lea rsi, [rsp+16]
    code.extend_from_slice(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00, 0x0F, 0x05]); // write
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // exit(0)

    // One failure exit per sentinel.
    let mut fail_at: alloc::vec::Vec<(u8, usize)> = alloc::vec::Vec::new();
    for sentinel in [0xC1u8, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7] {
        fail_at.push((sentinel, code.len()));
        code.extend_from_slice(&[0xBF, sentinel, 0x00, 0x00, 0x00]); // mov edi, sentinel
        code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // exit
    }
    code.push(0xCC); // int3

    // Patch the rel32s: displacement from the byte after the field.
    let patch = |code: &mut alloc::vec::Vec<u8>, at: usize, target: usize| {
        let disp = (target as i64 - (at as i64 + 4)) as i32;
        code[at..at + 4].copy_from_slice(&disp.to_le_bytes());
    };
    patch(&mut code, jz_child, child);
    for (at, sentinel) in fail_jumps {
        if let Some(&(_, target)) = fail_at.iter().find(|(s, _)| *s == sentinel) {
            patch(&mut code, at, target);
        }
    }

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

/// Build a **Linux-ABI** `ET_EXEC` that drives Unix-domain sockets by name
/// (`ipc::unix_socket`) from ring 3, through the ordinary Linux calls:
///
/// ```text
///   ; datagrams by abstract name "@slt-dg"
///   a = socket(AF_UNIX, SOCK_DGRAM, 0)        ; >= 0, else exit 0xD1
///   bind(a, "@slt-dg", 9)                     ; 0, else 0xD2
///   b = socket(AF_UNIX, SOCK_DGRAM, 0)        ; else 0xD3
///   sendto(b, "ping", 4, 0, "@slt-dg", 9)     ; 4, else 0xD4
///   recvfrom(a, buf, 64, 0, NULL, NULL)       ; 4, else 0xD5; "ping", else 0xD6
///   ; a stream by abstract name "@slt-st"
///   l = socket(AF_UNIX, SOCK_STREAM, 0)       ; else 0xD7
///   bind(l, "@slt-st", 9)                     ; else 0xD8
///   listen(l, 4)                              ; else 0xD9
///   c = socket(AF_UNIX, SOCK_STREAM, 0)       ; else 0xDA
///   connect(c, "@slt-st", 9)                  ; 0, else 0xDB
///   s = accept(l, NULL, NULL)                 ; >= 0, else 0xDC
///   write(c, "ping", 4)                       ; 4, else 0xDD
///   read(s, buf, 64)                          ; 4, else 0xDE
///   getsockopt(s, SOL_SOCKET, SO_PEERCRED, &ucred, &12)  ; 0, else 0xDF
///   ucred.pid == getpid()                     ; else 0xE0: the kernel's record
///   close(c); read(s, buf, 64)                ; 0 (end of file), else 0xE1
///   ; datagrams by path "/tmp/slt.sock"
///   d1 = socket(..DGRAM..); bind(d1, path, 16)  ; 0, else 0xE2
///   d2 = socket(..DGRAM..); bind(d2, path, 16)  ; -EADDRINUSE, else 0xE3
///   sendto(d2, "ping", 4, 0, path, 16)        ; 4, else 0xE4
///   recvfrom(d1, buf, 64, 0, &from, &110)     ; 4, else 0xE5
///   fromlen == 0 (an unbound sender: none)    ; else 0xE6
///   ; the sender's credentials, as syslog asks for them
///   setsockopt(d1, SOL_SOCKET, SO_PASSCRED, &1, 4)  ; 0, else 0xE9
///   sendto(d2, "ping", 4, 0, path, 16)        ; 4, else 0xEA
///   recvmsg(d1, {iov: buf/64, control: 32 bytes}, 0)  ; 4, "ping", else 0xEB
///   control = one whole message (cmsg_len 28) at SOL_SOCKET /
///     SCM_CREDENTIALS, msg_controllen 32, msg_flags 0  ; else 0xEC
///   its pid == getpid()                       ; else 0xED
///   ; credentials the sender states, as logger --id does
///   sendmsg(d2, {path, "ping", SCM_CREDENTIALS{getpid(), 4242, 4343}})
///                                             ; 4 (root may claim any id), else 0xEE
///   recvmsg(d1, ...)                          ; 4, and the stated pid, uid
///                                             ; and gid, else 0xEF
///   the same with a pid naming no process     ; -ESRCH, else 0xF0
///   the same with cmsg_len 24                 ; -EINVAL, else 0xF1
///   ; several messages a call
///   sendmmsg(d2, ["ping", "pong"] to path, 2) ; 2, else 0xF2; msg_len 4, 4, else 0xF3
///   recvmmsg(d1, [64-byte, 16-byte], 2)       ; 2, else 0xF4; 4, 4, "ping",
///                                             ; "pong", else 0xF5
///   recvmmsg(..., MSG_DONTWAIT) on nothing    ; -EAGAIN, else 0xF6
///   one sent; recvmmsg(..., 2, MSG_WAITFORONE); 1, else 0xF7
///   sendmmsg(d2, NULL, 2)                     ; -EFAULT, else 0xF8
///   sendmmsg with the 2nd entry's bytes at 0x10 ; 1 (the first sent), else 0xF9
///   d = open("/", O_DIRECTORY)               ; a descriptor, else 0xFE
///   sendmmsg(d, ...), getpeername(d, ...)     ; -ENOTSOCK both, else 0xFA
///   setsockopt(d1, SO_RCVTIMEO, {0, 50000})   ; 0, else 0xFB
///   recvfrom(d1, ...) on nothing, blocking    ; -EAGAIN after 50 ms, else 0xFC
///   setsockopt(d1, SO_RCVTIMEO, {0, 1000000}) ; -EDOM, else 0xFD
///   unlink("/tmp/slt.sock")                   ; 0, else 0xE7
///   sendto(d2, "ping", 4, 0, path, 16)        ; -ENOENT, else 0xE8
///   exit(0x5D)
/// ```
///
/// A clean `exit(0x5D)` proves: abstract names; a node made by `bind` that a
/// second `bind` finds in use and `unlink` takes away, after which the name
/// leads nowhere; datagrams kept whole, with an unnamed sender reported as
/// such; a listener's backlog, connect and accept; bytes both ways over the
/// accepted stream; the kernel's record of the peer, both as `SO_PEERCRED`
/// and as an `SCM_CREDENTIALS` control message; credentials a sender states,
/// checked; batches of messages each way; `ENOTSOCK` for a descriptor that is
/// not a socket; end of file when the client closes. Tagged `ELFOSABI_GNU`
/// for the SysV stack + Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::too_many_lines
)]
pub fn build_linux_unix_socket_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;
    /// `jnz rel32`'s second opcode byte.
    const JNZ: u8 = 0x85;
    /// `js rel32`'s second opcode byte.
    const JS: u8 = 0x88;
    // Linux x86-64 syscall numbers.
    const READ: u32 = 0;
    const WRITE: u32 = 1;
    const CLOSE: u32 = 3;
    const GETPID: u32 = 39;
    const SOCKET: u32 = 41;
    const CONNECT: u32 = 42;
    const ACCEPT: u32 = 43;
    const SENDTO: u32 = 44;
    const RECVFROM: u32 = 45;
    const BIND: u32 = 49;
    const LISTEN: u32 = 50;
    const GETSOCKOPT: u32 = 55;
    const SETSOCKOPT: u32 = 54;
    const SENDMSG: u32 = 46;
    const RECVMSG: u32 = 47;
    const OPEN: u32 = 2;
    const GETPEERNAME: u32 = 52;
    const RECVMMSG: u32 = 299;
    const SENDMMSG: u32 = 307;
    const EXIT: u32 = 60;
    const UNLINK: u32 = 87;
    // Stack layout (all [rsp + offset]).
    const FD_A: u32 = 0x00;
    const FD_B: u32 = 0x04;
    const FD_L: u32 = 0x08;
    const FD_C: u32 = 0x0C;
    const FD_S: u32 = 0x10;
    const FD_D1: u32 = 0x14;
    const FD_D2: u32 = 0x18;
    const FD_DIR: u32 = 0x1C;
    const DG_ADDR: u32 = 0x20; // "@slt-dg", 9 bytes
    const ST_ADDR: u32 = 0x40; // "@slt-st", 9 bytes
    const PING: u32 = 0x60;
    const BUF: u32 = 0x80; // 64 bytes
    const LEN: u32 = 0xC0; // a socklen_t in/out
    const FROM: u32 = 0xD0; // 110 bytes
    const UCRED: u32 = 0x150; // 12 bytes
    const PATH_ADDR: u32 = 0x170; // "/tmp/slt.sock", 16 bytes
    const IOV: u32 = 0x1D0; // struct iovec: base, len
    const SEND_IOV: u32 = 0x1E0; // the iovec sendmsg sends from
    const CONTROL: u32 = 0x200; // 32 bytes of control messages
    const MSGHDR: u32 = 0x240; // struct msghdr, 56 bytes
    const ONE: u32 = 0x280; // the int 1
    const SEND_MSGHDR: u32 = 0x2C0; // the msghdr sendmsg sends, 56 bytes
    const SEND_CONTROL: u32 = 0x300; // its control messages, 32 bytes
    const MMSG: u32 = 0x340; // two struct mmsghdr, 64 bytes each
    const MIOV: u32 = 0x3C0; // their two iovecs
    const RBUF2: u32 = 0x3E0; // the second receive buffer, 16 bytes
    const SLASH: u32 = 0x3F0; // "/", NUL-terminated
    const PONG: u32 = 0x3F8; // "pong"
    const FRAME: u32 = 0x400;
    /// "ping", little-endian.
    const PING_WORD: u32 = 0x676E_6970;
    /// "pong", little-endian.
    const PONG_WORD: u32 = 0x676E_6F70;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut fail_jumps: alloc::vec::Vec<(usize, u8)> = alloc::vec::Vec::new();
    let le = |v: u32| v.to_le_bytes();

    // Instruction emitters, every stack operand as [rsp + disp32].
    let mov_edi_mem = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x8B, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_rdi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_rsi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0xB4, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_r8 = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x4C, 0x8D, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_r9 = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x4C, 0x8D, 0x8C, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_r10 = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x4C, 0x8D, 0x94, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let mov_edi_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBF);
        c.extend_from_slice(&le(v));
    };
    let mov_esi_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBE);
        c.extend_from_slice(&le(v));
    };
    let mov_edx_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBA);
        c.extend_from_slice(&le(v));
    };
    let mov_r9d_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.extend_from_slice(&[0x41, 0xB9]);
        c.extend_from_slice(&le(v));
    };
    let xor_r10d = |c: &mut alloc::vec::Vec<u8>| c.extend_from_slice(&[0x45, 0x31, 0xD2]);
    let xor_r8d = |c: &mut alloc::vec::Vec<u8>| c.extend_from_slice(&[0x45, 0x31, 0xC0]);
    let xor_r9d = |c: &mut alloc::vec::Vec<u8>| c.extend_from_slice(&[0x45, 0x31, 0xC9]);
    let syscall = |c: &mut alloc::vec::Vec<u8>, nr: u32| {
        c.push(0xB8);
        c.extend_from_slice(&le(nr));
        c.extend_from_slice(&[0x0F, 0x05]);
    };
    let store_eax = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x89, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let store_imm = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0xC7, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    let cmp_eax_mem = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x3B, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let cmp_mem_imm = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0x81, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    // cmp rax, imm8 (sign-extended)
    let cmp_rax_i8 = |c: &mut alloc::vec::Vec<u8>, v: i8| {
        c.extend_from_slice(&[0x48, 0x83, 0xF8, v as u8]);
    };
    let test_rax = |c: &mut alloc::vec::Vec<u8>| c.extend_from_slice(&[0x48, 0x85, 0xC0]);
    let jcc_fail = |c: &mut alloc::vec::Vec<u8>,
                    fails: &mut alloc::vec::Vec<(usize, u8)>,
                    op: u8,
                    sentinel: u8| {
        c.extend_from_slice(&[0x0F, op, 0, 0, 0, 0]);
        fails.push((c.len() - 4, sentinel));
    };
    // socket(AF_UNIX, kind, 0) into [rsp + slot], or exit `sentinel`.
    let unix_socket = |c: &mut alloc::vec::Vec<u8>,
                       fails: &mut alloc::vec::Vec<(usize, u8)>,
                       kind: u32,
                       slot: u32,
                       sentinel: u8| {
        mov_edi_imm(c, 1);
        mov_esi_imm(c, kind);
        mov_edx_imm(c, 0);
        syscall(c, SOCKET);
        test_rax(c);
        jcc_fail(c, fails, JS, sentinel);
        store_eax(c, slot);
    };

    // sub rsp, FRAME
    code.extend_from_slice(&[0x48, 0x81, 0xEC]);
    code.extend_from_slice(&le(FRAME));
    // The addresses: family 1, a NUL, then the name ("slt-dg" / "slt-st").
    store_imm(&mut code, DG_ADDR, 0x7300_0001);
    store_imm(&mut code, DG_ADDR + 4, 0x642D_746C);
    store_imm(&mut code, DG_ADDR + 8, 0x0000_0067);
    store_imm(&mut code, ST_ADDR, 0x7300_0001);
    store_imm(&mut code, ST_ADDR + 4, 0x732D_746C);
    store_imm(&mut code, ST_ADDR + 8, 0x0000_0074);
    // "/tmp/slt.sock": family 1, the path, its NUL.
    store_imm(&mut code, PATH_ADDR, 0x742F_0001);
    store_imm(&mut code, PATH_ADDR + 4, 0x732F_706D);
    store_imm(&mut code, PATH_ADDR + 8, 0x732E_746C);
    store_imm(&mut code, PATH_ADDR + 12, 0x006B_636F);
    store_imm(&mut code, PING, PING_WORD);

    // --- datagrams by abstract name ---
    unix_socket(&mut code, &mut fail_jumps, 2, FD_A, 0xD1);
    mov_edi_mem(&mut code, FD_A);
    lea_rsi(&mut code, DG_ADDR);
    mov_edx_imm(&mut code, 9);
    syscall(&mut code, BIND);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD2);
    unix_socket(&mut code, &mut fail_jumps, 2, FD_B, 0xD3);
    mov_edi_mem(&mut code, FD_B);
    lea_rsi(&mut code, PING);
    mov_edx_imm(&mut code, 4);
    xor_r10d(&mut code);
    lea_r8(&mut code, DG_ADDR);
    mov_r9d_imm(&mut code, 9);
    syscall(&mut code, SENDTO);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD4);
    mov_edi_mem(&mut code, FD_A);
    lea_rsi(&mut code, BUF);
    mov_edx_imm(&mut code, 64);
    xor_r10d(&mut code);
    xor_r8d(&mut code);
    xor_r9d(&mut code);
    syscall(&mut code, RECVFROM);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD5);
    cmp_mem_imm(&mut code, BUF, PING_WORD);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD6);

    // --- a stream by abstract name ---
    unix_socket(&mut code, &mut fail_jumps, 1, FD_L, 0xD7);
    mov_edi_mem(&mut code, FD_L);
    lea_rsi(&mut code, ST_ADDR);
    mov_edx_imm(&mut code, 9);
    syscall(&mut code, BIND);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD8);
    mov_edi_mem(&mut code, FD_L);
    mov_esi_imm(&mut code, 4);
    syscall(&mut code, LISTEN);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD9);
    unix_socket(&mut code, &mut fail_jumps, 1, FD_C, 0xDA);
    mov_edi_mem(&mut code, FD_C);
    lea_rsi(&mut code, ST_ADDR);
    mov_edx_imm(&mut code, 9);
    syscall(&mut code, CONNECT);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xDB);
    mov_edi_mem(&mut code, FD_L);
    mov_esi_imm(&mut code, 0);
    mov_edx_imm(&mut code, 0);
    syscall(&mut code, ACCEPT);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JS, 0xDC);
    store_eax(&mut code, FD_S);
    mov_edi_mem(&mut code, FD_C);
    lea_rsi(&mut code, PING);
    mov_edx_imm(&mut code, 4);
    syscall(&mut code, WRITE);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xDD);
    mov_edi_mem(&mut code, FD_S);
    lea_rsi(&mut code, BUF);
    mov_edx_imm(&mut code, 64);
    syscall(&mut code, READ);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xDE);
    // getsockopt(s, SOL_SOCKET, SO_PEERCRED, &ucred, &len) with len = 12
    store_imm(&mut code, LEN, 12);
    mov_edi_mem(&mut code, FD_S);
    mov_esi_imm(&mut code, 1);
    mov_edx_imm(&mut code, 17);
    lea_r10(&mut code, UCRED);
    lea_r8(&mut code, LEN);
    syscall(&mut code, GETSOCKOPT);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xDF);
    syscall(&mut code, GETPID);
    cmp_eax_mem(&mut code, UCRED);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE0);
    // close(c), then end of file on s
    mov_edi_mem(&mut code, FD_C);
    syscall(&mut code, CLOSE);
    mov_edi_mem(&mut code, FD_S);
    lea_rsi(&mut code, BUF);
    mov_edx_imm(&mut code, 64);
    syscall(&mut code, READ);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE1);

    // --- datagrams by path ---
    unix_socket(&mut code, &mut fail_jumps, 2, FD_D1, 0xE2);
    mov_edi_mem(&mut code, FD_D1);
    lea_rsi(&mut code, PATH_ADDR);
    mov_edx_imm(&mut code, 16);
    syscall(&mut code, BIND);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE2);
    unix_socket(&mut code, &mut fail_jumps, 2, FD_D2, 0xE3);
    mov_edi_mem(&mut code, FD_D2);
    lea_rsi(&mut code, PATH_ADDR);
    mov_edx_imm(&mut code, 16);
    syscall(&mut code, BIND);
    cmp_rax_i8(&mut code, -98); // EADDRINUSE
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE3);
    mov_edi_mem(&mut code, FD_D2);
    lea_rsi(&mut code, PING);
    mov_edx_imm(&mut code, 4);
    xor_r10d(&mut code);
    lea_r8(&mut code, PATH_ADDR);
    mov_r9d_imm(&mut code, 16);
    syscall(&mut code, SENDTO);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE4);
    store_imm(&mut code, LEN, 110);
    mov_edi_mem(&mut code, FD_D1);
    lea_rsi(&mut code, BUF);
    mov_edx_imm(&mut code, 64);
    xor_r10d(&mut code);
    lea_r8(&mut code, FROM);
    lea_r9(&mut code, LEN);
    syscall(&mut code, RECVFROM);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE5);
    cmp_mem_imm(&mut code, LEN, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE6);

    // --- the sender's credentials, as syslog asks for them ---
    // lea rax, [rsp + d32]
    let lea_rax = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // mov [rsp + d32], rax
    let store_rax = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // mov qword [rsp + d32], imm32 (sign-extended)
    let store_qword = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0x48, 0xC7, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    let mov_r8d_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.extend_from_slice(&[0x41, 0xB8]);
        c.extend_from_slice(&le(v));
    };
    // setsockopt(d1, SOL_SOCKET, SO_PASSCRED, &1, 4)
    store_imm(&mut code, ONE, 1);
    mov_edi_mem(&mut code, FD_D1);
    mov_esi_imm(&mut code, 1);
    mov_edx_imm(&mut code, 16);
    lea_r10(&mut code, ONE);
    mov_r8d_imm(&mut code, 4);
    syscall(&mut code, SETSOCKOPT);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE9);
    mov_edi_mem(&mut code, FD_D2);
    lea_rsi(&mut code, PING);
    mov_edx_imm(&mut code, 4);
    xor_r10d(&mut code);
    lea_r8(&mut code, PATH_ADDR);
    mov_r9d_imm(&mut code, 16);
    syscall(&mut code, SENDTO);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEA);
    // recvmsg(d1, MSGHDR, 0) into rax. The iovec is the 64-byte buffer,
    // emptied so the datagram has to land in it; the msghdr has no name, one
    // iovec, and 32 bytes of control, emptied so nothing left over can pass
    // for the kernel's answer.
    let recvmsg_d1 = |c: &mut alloc::vec::Vec<u8>| {
        store_imm(c, BUF, 0);
        lea_rax(c, BUF);
        store_rax(c, IOV);
        store_qword(c, IOV + 8, 64);
        for off in [0u32, 8, 16, 24] {
            store_qword(c, CONTROL + off, 0);
        }
        store_qword(c, MSGHDR, 0); // msg_name
        store_qword(c, MSGHDR + 8, 0); // msg_namelen
        lea_rax(c, IOV);
        store_rax(c, MSGHDR + 16); // msg_iov
        store_qword(c, MSGHDR + 24, 1); // msg_iovlen
        lea_rax(c, CONTROL);
        store_rax(c, MSGHDR + 32); // msg_control
        store_qword(c, MSGHDR + 40, 32); // msg_controllen
        store_qword(c, MSGHDR + 48, 0); // msg_flags
        mov_edi_mem(c, FD_D1);
        lea_rsi(c, MSGHDR);
        mov_edx_imm(c, 0);
        syscall(c, RECVMSG);
    };
    recvmsg_d1(&mut code);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEB);
    cmp_mem_imm(&mut code, BUF, PING_WORD);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEB);
    // One whole SCM_CREDENTIALS message: cmsg_len 28, SOL_SOCKET,
    // SCM_CREDENTIALS, and msg_controllen 32 (its space) on the way back.
    cmp_mem_imm(&mut code, CONTROL, 28);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEC);
    cmp_mem_imm(&mut code, CONTROL + 8, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEC);
    cmp_mem_imm(&mut code, CONTROL + 12, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEC);
    cmp_mem_imm(&mut code, MSGHDR + 40, 32);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEC);
    cmp_mem_imm(&mut code, MSGHDR + 48, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEC);
    syscall(&mut code, GETPID);
    cmp_eax_mem(&mut code, CONTROL + 16);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xED);

    // --- credentials the sender states (SCM_CREDENTIALS on sendmsg) ---
    // The msghdr: to the path, one iovec of "ping", 32 bytes of control
    // holding one SCM_CREDENTIALS message -- this pid, and the uid and gid
    // 4242 and 4343, which only root may claim (the test runs as root).
    lea_rax(&mut code, PATH_ADDR);
    store_rax(&mut code, SEND_MSGHDR); // msg_name
    store_qword(&mut code, SEND_MSGHDR + 8, 16); // msg_namelen
    lea_rax(&mut code, PING);
    store_rax(&mut code, SEND_IOV);
    store_qword(&mut code, SEND_IOV + 8, 4);
    lea_rax(&mut code, SEND_IOV);
    store_rax(&mut code, SEND_MSGHDR + 16); // msg_iov
    store_qword(&mut code, SEND_MSGHDR + 24, 1); // msg_iovlen
    lea_rax(&mut code, SEND_CONTROL);
    store_rax(&mut code, SEND_MSGHDR + 32); // msg_control
    store_qword(&mut code, SEND_MSGHDR + 40, 32); // msg_controllen
    store_qword(&mut code, SEND_MSGHDR + 48, 0); // msg_flags
    store_qword(&mut code, SEND_CONTROL, 28); // cmsg_len
    store_imm(&mut code, SEND_CONTROL + 8, 1); // SOL_SOCKET
    store_imm(&mut code, SEND_CONTROL + 12, 2); // SCM_CREDENTIALS
    syscall(&mut code, GETPID);
    store_eax(&mut code, SEND_CONTROL + 16);
    store_imm(&mut code, SEND_CONTROL + 20, 4242);
    store_imm(&mut code, SEND_CONTROL + 24, 4343);
    store_imm(&mut code, SEND_CONTROL + 28, 0);
    // sendmsg(d2, SEND_MSGHDR, 0) into rax.
    let sendmsg_d2 = |c: &mut alloc::vec::Vec<u8>| {
        mov_edi_mem(c, FD_D2);
        lea_rsi(c, SEND_MSGHDR);
        mov_edx_imm(c, 0);
        syscall(c, SENDMSG);
    };
    sendmsg_d2(&mut code);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEE);
    recvmsg_d1(&mut code);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEF);
    syscall(&mut code, GETPID);
    cmp_eax_mem(&mut code, CONTROL + 16);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEF);
    cmp_mem_imm(&mut code, CONTROL + 20, 4242);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEF);
    cmp_mem_imm(&mut code, CONTROL + 24, 4343);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xEF);
    // A pid that names no process: ESRCH, and nothing sent.
    store_imm(&mut code, SEND_CONTROL + 16, 0x7FFF_FFF0);
    sendmsg_d2(&mut code);
    cmp_rax_i8(&mut code, -3); // ESRCH
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF0);
    // A credentials message of the wrong length: EINVAL, and nothing sent.
    store_qword(&mut code, SEND_CONTROL, 24);
    sendmsg_d2(&mut code);
    cmp_rax_i8(&mut code, -22); // EINVAL
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF1);

    // --- sendmmsg and recvmmsg: several messages a call ---
    let mov_r10d_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.extend_from_slice(&[0x41, 0xBA]);
        c.extend_from_slice(&le(v));
    };
    // lea rdx, [rsp + d32]
    let lea_rdx = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0x94, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // Two mmsghdrs for sending: each to the path, one iovec each ("ping",
    // then "pong"), no control, msg_len 0 until the kernel fills it in.
    let mmsg_send_layout = |c: &mut alloc::vec::Vec<u8>| {
        lea_rax(c, PING);
        store_rax(c, MIOV);
        store_qword(c, MIOV + 8, 4);
        lea_rax(c, PONG);
        store_rax(c, MIOV + 16);
        store_qword(c, MIOV + 24, 4);
        for (entry, iov) in [(0u32, MIOV), (64, MIOV + 16)] {
            lea_rax(c, PATH_ADDR);
            store_rax(c, MMSG + entry); // msg_name
            store_qword(c, MMSG + entry + 8, 16); // msg_namelen
            lea_rax(c, iov);
            store_rax(c, MMSG + entry + 16); // msg_iov
            store_qword(c, MMSG + entry + 24, 1); // msg_iovlen
            for off in [32u32, 40, 48, 56] {
                store_qword(c, MMSG + entry + off, 0); // control, its length, flags, msg_len
            }
        }
    };
    // The same two for receiving: no name, one emptied buffer each (64 and
    // 16 bytes), no control.
    let mmsg_recv_layout = |c: &mut alloc::vec::Vec<u8>| {
        store_imm(c, BUF, 0);
        store_imm(c, RBUF2, 0);
        lea_rax(c, BUF);
        store_rax(c, MIOV);
        store_qword(c, MIOV + 8, 64);
        lea_rax(c, RBUF2);
        store_rax(c, MIOV + 16);
        store_qword(c, MIOV + 24, 16);
        for (entry, iov) in [(0u32, MIOV), (64, MIOV + 16)] {
            store_qword(c, MMSG + entry, 0);
            store_qword(c, MMSG + entry + 8, 0);
            lea_rax(c, iov);
            store_rax(c, MMSG + entry + 16);
            store_qword(c, MMSG + entry + 24, 1);
            for off in [32u32, 40, 48, 56] {
                store_qword(c, MMSG + entry + off, 0);
            }
        }
    };
    store_imm(&mut code, PONG, PONG_WORD);
    // sendmmsg(d2, MMSG, 2, 0): both sent, each msg_len 4.
    mmsg_send_layout(&mut code);
    mov_edi_mem(&mut code, FD_D2);
    lea_rsi(&mut code, MMSG);
    mov_edx_imm(&mut code, 2);
    xor_r10d(&mut code);
    syscall(&mut code, SENDMMSG);
    cmp_rax_i8(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF2);
    cmp_mem_imm(&mut code, MMSG + 56, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF3);
    cmp_mem_imm(&mut code, MMSG + 64 + 56, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF3);
    // recvmmsg(d1, MMSG, 2, 0, NULL): both, in order, each whole.
    mmsg_recv_layout(&mut code);
    mov_edi_mem(&mut code, FD_D1);
    lea_rsi(&mut code, MMSG);
    mov_edx_imm(&mut code, 2);
    xor_r10d(&mut code);
    xor_r8d(&mut code);
    syscall(&mut code, RECVMMSG);
    cmp_rax_i8(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF4);
    cmp_mem_imm(&mut code, MMSG + 56, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF5);
    cmp_mem_imm(&mut code, MMSG + 64 + 56, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF5);
    cmp_mem_imm(&mut code, BUF, PING_WORD);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF5);
    cmp_mem_imm(&mut code, RBUF2, PONG_WORD);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF5);
    // Nothing left, and MSG_DONTWAIT: EAGAIN.
    mov_edi_mem(&mut code, FD_D1);
    lea_rsi(&mut code, MMSG);
    mov_edx_imm(&mut code, 2);
    mov_r10d_imm(&mut code, 0x40); // MSG_DONTWAIT
    xor_r8d(&mut code);
    syscall(&mut code, RECVMMSG);
    cmp_rax_i8(&mut code, -11); // EAGAIN
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF6);
    // MSG_WAITFORONE: one waiting, two asked for -- the one, without
    // waiting for a second.
    mov_edi_mem(&mut code, FD_D2);
    lea_rsi(&mut code, PING);
    mov_edx_imm(&mut code, 4);
    xor_r10d(&mut code);
    lea_r8(&mut code, PATH_ADDR);
    mov_r9d_imm(&mut code, 16);
    syscall(&mut code, SENDTO);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF7);
    mov_edi_mem(&mut code, FD_D1);
    lea_rsi(&mut code, MMSG);
    mov_edx_imm(&mut code, 2);
    mov_r10d_imm(&mut code, 0x1_0000); // MSG_WAITFORONE
    xor_r8d(&mut code);
    syscall(&mut code, RECVMMSG);
    cmp_rax_i8(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF7);
    // A vector that cannot be read, nothing sent: EFAULT.
    mov_edi_mem(&mut code, FD_D2);
    mov_esi_imm(&mut code, 0);
    mov_edx_imm(&mut code, 2);
    xor_r10d(&mut code);
    syscall(&mut code, SENDMMSG);
    cmp_rax_i8(&mut code, -14); // EFAULT
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF8);
    // The second entry's bytes are nowhere: the first goes, and the answer
    // is 1 rather than the second's EFAULT.
    mmsg_send_layout(&mut code);
    store_qword(&mut code, MIOV + 16, 0x10);
    mov_edi_mem(&mut code, FD_D2);
    lea_rsi(&mut code, MMSG);
    mov_edx_imm(&mut code, 2);
    xor_r10d(&mut code);
    syscall(&mut code, SENDMMSG);
    cmp_rax_i8(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF9);
    mov_edi_mem(&mut code, FD_D1);
    lea_rsi(&mut code, BUF);
    mov_edx_imm(&mut code, 64);
    mov_r10d_imm(&mut code, 0x40); // MSG_DONTWAIT
    xor_r8d(&mut code);
    xor_r9d(&mut code);
    syscall(&mut code, RECVFROM);
    cmp_rax_i8(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xF9);
    // A descriptor that is not a socket: ENOTSOCK, from sendmmsg and from
    // getpeername (the probe an inetd-started program makes of stdin).
    store_imm(&mut code, SLASH, 0x2F);
    lea_rdi(&mut code, SLASH);
    mov_esi_imm(&mut code, 0x1_0000); // O_RDONLY | O_DIRECTORY
    mov_edx_imm(&mut code, 0);
    syscall(&mut code, OPEN);
    test_rax(&mut code);
    // Its own code: a refused open is the harness's fault, not the probes'.
    jcc_fail(&mut code, &mut fail_jumps, JS, 0xFE);
    store_eax(&mut code, FD_DIR);
    mov_edi_mem(&mut code, FD_DIR);
    lea_rsi(&mut code, MMSG);
    mov_edx_imm(&mut code, 1);
    xor_r10d(&mut code);
    syscall(&mut code, SENDMMSG);
    cmp_rax_i8(&mut code, -88); // ENOTSOCK
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xFA);
    store_imm(&mut code, LEN, 110);
    mov_edi_mem(&mut code, FD_DIR);
    lea_rsi(&mut code, FROM);
    lea_rdx(&mut code, LEN);
    syscall(&mut code, GETPEERNAME);
    cmp_rax_i8(&mut code, -88); // ENOTSOCK
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xFA);

    // --- SO_RCVTIMEO: a blocking receive gives up with EAGAIN ---
    // setsockopt(d1, SOL_SOCKET, SO_RCVTIMEO, &{0 s, 50000 us}, 16)
    let setsockopt_timeo = |c: &mut alloc::vec::Vec<u8>| {
        mov_edi_mem(c, FD_D1);
        mov_esi_imm(c, 1);
        mov_edx_imm(c, 20);
        lea_r10(c, RBUF2);
        mov_r8d_imm(c, 16);
        syscall(c, SETSOCKOPT);
    };
    store_qword(&mut code, RBUF2, 0);
    store_qword(&mut code, RBUF2 + 8, 50_000);
    setsockopt_timeo(&mut code);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xFB);
    // recvfrom(d1, BUF, 64, 0, NULL, NULL) on an empty queue, blocking.
    mov_edi_mem(&mut code, FD_D1);
    lea_rsi(&mut code, BUF);
    mov_edx_imm(&mut code, 64);
    xor_r10d(&mut code);
    xor_r8d(&mut code);
    xor_r9d(&mut code);
    syscall(&mut code, RECVFROM);
    cmp_rax_i8(&mut code, -11); // EAGAIN
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xFC);
    // A microsecond count of a whole second is EDOM.
    store_qword(&mut code, RBUF2 + 8, 1_000_000);
    setsockopt_timeo(&mut code);
    cmp_rax_i8(&mut code, -33); // EDOM
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xFD);

    // unlink(path): the sun_path inside the address, NUL-terminated
    lea_rdi(&mut code, PATH_ADDR + 2);
    syscall(&mut code, UNLINK);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE7);
    mov_edi_mem(&mut code, FD_D2);
    lea_rsi(&mut code, PING);
    mov_edx_imm(&mut code, 4);
    xor_r10d(&mut code);
    lea_r8(&mut code, PATH_ADDR);
    mov_r9d_imm(&mut code, 16);
    syscall(&mut code, SENDTO);
    cmp_rax_i8(&mut code, -2); // ENOENT
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xE8);

    // exit(0x5D)
    mov_edi_imm(&mut code, 0x5D);
    syscall(&mut code, EXIT);

    // One failure exit per sentinel.
    let mut fail_at: alloc::vec::Vec<(u8, usize)> = alloc::vec::Vec::new();
    for sentinel in 0xD1u8..=0xFD {
        fail_at.push((sentinel, code.len()));
        mov_edi_imm(&mut code, u32::from(sentinel));
        syscall(&mut code, EXIT);
    }
    code.push(0xCC); // int3

    for (at, sentinel) in fail_jumps {
        if let Some(&(_, target)) = fail_at.iter().find(|(s, _)| *s == sentinel) {
            let disp = (target as i64 - (at as i64 + 4)) as i32;
            code[at..at + 4].copy_from_slice(&disp.to_le_bytes());
        }
    }

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

/// Build a **Linux-ABI** `ET_EXEC` that passes descriptors over Unix-domain
/// sockets (`SCM_RIGHTS`) from ring 3, each a pipe's write end, so that
/// whether every reference was given back shows as the pipe's end of file
/// (all its pipes non-blocking: a reference left over reads `EAGAIN`, never
/// a hang):
///
/// ```text
///   ; a datagram carries a pipe's write end W; ours closed, the passed one
///   ; writes, and its close is the pipe's end
///   socketpair(AF_UNIX, SOCK_DGRAM, 0, [d0, d1])  ; 0, else exit 0x01
///   pipe2([r, w], O_NONBLOCK)                     ; 0, else 0x02
///   sendmsg(d0, {"x", SCM_RIGHTS [w]})            ; 1, else 0x03
///   close(w)                                      ; 0, else 0x04
///   recvmsg(d1, {16 bytes, 32 of control}, MSG_CMSG_CLOEXEC)  ; 1, else 0x05
///   one SCM_RIGHTS message (cmsg_len 20), msg_controllen 24, msg_flags 0
///                                                 ; else 0x06
///   w2 = its descriptor; fcntl(w2, F_GETFD)       ; FD_CLOEXEC, else 0x07
///   write(w2, "xy", 2)                            ; 2, else 0x08
///   read(r, buf, 16)                              ; 2, else 0x09
///   close(w2)                                     ; 0, else 0x0A
///   read(r, buf, 16)                              ; 0 (end of file), else 0x0B
///   ; a stream: the descriptors ride on their send's bytes
///   socketpair(AF_UNIX, SOCK_STREAM, 0, [s0, s1]) ; 0, else 0x0C
///   pipe2([r2, w3], O_NONBLOCK)                   ; 0, else 0x0D
///   write(s0, "ab"); sendmsg(s0, {"cd", SCM_RIGHTS [w3]}); write(s0, "ef")
///                                                 ; 2 each, else 0x0E / 0x0F / 0x10
///   recvmsg(s1, {16 bytes, 32 of control})        ; 4, "abcd" -- read on into
///                                                 ; the marked bytes -- else 0x11
///   one descriptor, w4 (w3 still open: the process
///   held the pipe already, one reference kept)    ; else 0x12
///   read(s1, buf, 16)                             ; 2, "ef", else 0x13
///   close(w3); read(r2, ...)                      ; -EAGAIN (w4 writes still), else 0x14
///   close(w4); read(r2, ...)                      ; 0 (end of file), else 0x15
///   ; room for one of two: MSG_CTRUNC, the other released
///   pipe2([r3, w5], O_NONBLOCK)                   ; 0, else 0x16
///   sendmsg(d0, {"y", SCM_RIGHTS [w5, w5]})       ; 1, else 0x17
///   close(w5); recvmsg(d1, {16 bytes, 20 of control})  ; 1, else 0x18
///   MSG_CTRUNC, one descriptor (cmsg_len 20), msg_controllen 20  ; else 0x19
///   close(w6); read(r3, ...)                      ; 0 (end of file), else 0x1A
///   ; read(2) cannot hand descriptors on: released
///   pipe2([r4, w7], O_NONBLOCK)                   ; 0, else 0x1B
///   sendmsg(d0, {"x", SCM_RIGHTS [w7]})           ; 1, else 0x1C
///   close(w7); read(d1, buf, 16)                  ; 1, else 0x1D
///   read(r4, ...)                                 ; 0 (end of file), else 0x1E
///   sendmsg(d0, {"x", SCM_RIGHTS [999]})          ; -EBADF, else 0x1F
///   ; sequenced packets: each send whole, the peer's close the end
///   socketpair(AF_UNIX, SOCK_SEQPACKET, 0, [q0, q1])  ; 0, else 0x20
///   write(q0, "abcd", 4); write(q0, "ef", 2)      ; 4, 2, else 0x21
///   recvmsg(q1, {2 bytes})                        ; 2, MSG_TRUNC, else 0x22
///   read(q1, buf, 16)                             ; 2 -- the next send, not the
///                                                 ; rest of the first -- else 0x23
///   close(q0); read(q1, buf, 16)                  ; 0 (end of file), else 0x24
///   write(q1, "x", 1)                             ; -EPIPE, else 0x25
///   exit(0x5F)
/// ```
///
/// A clean `exit(0x5F)` proves a descriptor passed on a datagram and on a
/// stream, close-on-exec on request, the stream's bytes stopping at the end of
/// the stretch the descriptors rode on, a received object the process held
/// already kept as one reference, `MSG_CTRUNC` with the surplus released, a
/// plain `read` releasing what it cannot hand on, `EBADF` for a descriptor
/// not open, and `SOCK_SEQPACKET`'s whole messages and end of file -- and, by
/// every pipe's end of file arriving exactly
/// when its last writer closes, that no reference leaked or was released
/// twice. Tagged `ELFOSABI_GNU` for the SysV stack + Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::too_many_lines
)]
pub fn build_linux_scm_rights_test_elf() -> alloc::vec::Vec<u8> {
    /// `jnz rel32`'s second opcode byte.
    const JNZ: u8 = 0x85;
    /// `jz rel32`'s second opcode byte.
    const JZ: u8 = 0x84;
    // Linux x86-64 syscall numbers.
    const READ: u32 = 0;
    const WRITE: u32 = 1;
    const CLOSE: u32 = 3;
    const SENDMSG: u32 = 46;
    const RECVMSG: u32 = 47;
    const SOCKETPAIR: u32 = 53;
    const EXIT: u32 = 60;
    const FCNTL: u32 = 72;
    const PIPE2: u32 = 293;
    const O_NONBLOCK: u32 = 0o4000;
    const MSG_CMSG_CLOEXEC: u32 = 0x4000_0000;
    const MSG_CTRUNC: u32 = 0x8;
    const MSG_TRUNC: u32 = 0x20;
    // Stack layout (all [rsp + offset]).
    const D0: u32 = 0x00; // socketpair(DGRAM): d0, d1
    const D1: u32 = 0x04;
    const S0: u32 = 0x08; // socketpair(STREAM): s0, s1
    const S1: u32 = 0x0C;
    const P1: u32 = 0x10; // pipes: read end, write end
    const P2: u32 = 0x18;
    const P3: u32 = 0x20;
    const P4: u32 = 0x28;
    const BYTES: u32 = 0x30; // "abcdefxy"
    const BUF: u32 = 0x40; // 16 bytes
    const Q0: u32 = 0x50; // socketpair(SEQPACKET): q0, q1
    const Q1: u32 = 0x54;
    const IOV: u32 = 0x60; // the send's struct iovec
    const RIOV: u32 = 0x70; // the receive's
    const CTL_OUT: u32 = 0x80; // the send's control, 24 bytes
    const CTL_IN: u32 = 0xA0; // the receive's, 32 bytes
    const MSG: u32 = 0xC0; // the send's struct msghdr, 56 bytes
    const RMSG: u32 = 0x100; // the receive's
    const FRAME: u32 = 0x140;
    /// "abcd", little-endian.
    const ABCD: u32 = 0x6463_6261;
    /// "ef", little-endian, as the low half of a dword.
    const EF: u32 = 0x6665;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut fail_jumps: alloc::vec::Vec<(usize, u8)> = alloc::vec::Vec::new();
    let le = |v: u32| v.to_le_bytes();
    // Emitters, every stack operand as [rsp + disp32].
    let mov_edi_mem = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x8B, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let mov_edi_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBF);
        c.extend_from_slice(&le(v));
    };
    let mov_esi_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBE);
        c.extend_from_slice(&le(v));
    };
    let mov_edx_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBA);
        c.extend_from_slice(&le(v));
    };
    let lea_rdi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_rsi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0xB4, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_r10 = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x4C, 0x8D, 0x94, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // lea rax, [rsp + d]; mov [rsp + at], rax -- a pointer into the frame.
    let store_ptr = |c: &mut alloc::vec::Vec<u8>, at: u32, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]);
        c.extend_from_slice(&le(at));
    };
    // mov dword [rsp + d], imm32
    let store_imm = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0xC7, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    // mov qword [rsp + d], imm32 (zero-extended: every value here is small)
    let store_imm64 = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0x48, 0xC7, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    // mov eax, [rsp + from]; mov [rsp + to], eax -- copy a descriptor number.
    let copy_dword = |c: &mut alloc::vec::Vec<u8>, to: u32, from: u32| {
        c.extend_from_slice(&[0x8B, 0x84, 0x24]);
        c.extend_from_slice(&le(from));
        c.extend_from_slice(&[0x89, 0x84, 0x24]);
        c.extend_from_slice(&le(to));
    };
    let syscall = |c: &mut alloc::vec::Vec<u8>, nr: u32| {
        c.push(0xB8);
        c.extend_from_slice(&le(nr));
        c.extend_from_slice(&[0x0F, 0x05]);
    };
    let cmp_rax = |c: &mut alloc::vec::Vec<u8>, v: i32| {
        c.extend_from_slice(&[0x48, 0x3D]);
        c.extend_from_slice(&v.to_le_bytes());
    };
    // cmp dword [rsp + d], imm32
    let cmp_mem = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0x81, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    // test dword [rsp + d], imm32
    let test_mem = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0xF7, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    let jcc_fail = |c: &mut alloc::vec::Vec<u8>,
                    fails: &mut alloc::vec::Vec<(usize, u8)>,
                    op: u8,
                    sentinel: u8| {
        c.extend_from_slice(&[0x0F, op, 0, 0, 0, 0]);
        fails.push((c.len() - 4, sentinel));
    };
    // The send's msghdr: no name, one iovec (`len` bytes at BYTES + `from`),
    // and an SCM_RIGHTS message naming the descriptors stored at `fds`.
    let send_msg = |c: &mut alloc::vec::Vec<u8>, from: u32, len: u32, fds: &[u32]| {
        store_ptr(c, IOV, BYTES + from);
        store_imm64(c, IOV + 8, len);
        let cmsg_len = 16 + 4 * fds.len() as u32;
        store_imm64(c, CTL_OUT, cmsg_len);
        store_imm(c, CTL_OUT + 8, 1); // SOL_SOCKET
        store_imm(c, CTL_OUT + 12, 1); // SCM_RIGHTS
        for (i, &fd) in fds.iter().enumerate() {
            copy_dword(c, CTL_OUT + 16 + 4 * i as u32, fd);
        }
        store_imm64(c, MSG, 0);
        store_imm64(c, MSG + 8, 0);
        store_ptr(c, MSG + 16, IOV);
        store_imm64(c, MSG + 24, 1);
        store_ptr(c, MSG + 32, CTL_OUT);
        store_imm64(c, MSG + 40, (cmsg_len + 7) & !7);
        store_imm64(c, MSG + 48, 0);
    };
    // The receive's msghdr: 16 bytes into BUF, `room` bytes of control.
    let recv_msg = |c: &mut alloc::vec::Vec<u8>, room: u32| {
        store_ptr(c, RIOV, BUF);
        store_imm64(c, RIOV + 8, 16);
        for off in (0..32).step_by(8) {
            store_imm64(c, CTL_IN + off, 0);
        }
        store_imm64(c, RMSG, 0);
        store_imm64(c, RMSG + 8, 0);
        store_ptr(c, RMSG + 16, RIOV);
        store_imm64(c, RMSG + 24, 1);
        store_ptr(c, RMSG + 32, CTL_IN);
        store_imm64(c, RMSG + 40, room);
        store_imm64(c, RMSG + 48, 0);
    };
    let close_fd = |c: &mut alloc::vec::Vec<u8>, fd: u32| {
        mov_edi_mem(c, fd);
        syscall(c, CLOSE);
    };
    // read(fd, BUF, 16)
    let read_fd = |c: &mut alloc::vec::Vec<u8>, fd: u32| {
        mov_edi_mem(c, fd);
        lea_rsi(c, BUF);
        mov_edx_imm(c, 16);
        syscall(c, READ);
    };
    // write(fd, BYTES + from, len)
    let write_fd = |c: &mut alloc::vec::Vec<u8>, fd: u32, from: u32, len: u32| {
        mov_edi_mem(c, fd);
        lea_rsi(c, BYTES + from);
        mov_edx_imm(c, len);
        syscall(c, WRITE);
    };
    // sendmsg(fd, MSG, 0) / recvmsg(fd, RMSG, flags)
    let sendmsg = |c: &mut alloc::vec::Vec<u8>, fd: u32| {
        mov_edi_mem(c, fd);
        lea_rsi(c, MSG);
        mov_edx_imm(c, 0);
        syscall(c, SENDMSG);
    };
    let recvmsg = |c: &mut alloc::vec::Vec<u8>, fd: u32, flags: u32| {
        mov_edi_mem(c, fd);
        lea_rsi(c, RMSG);
        mov_edx_imm(c, flags);
        syscall(c, RECVMSG);
    };
    // pipe2(at, O_NONBLOCK)
    let pipe = |c: &mut alloc::vec::Vec<u8>, at: u32| {
        lea_rdi(c, at);
        mov_esi_imm(c, O_NONBLOCK);
        syscall(c, PIPE2);
    };

    // sub rsp, FRAME
    code.extend_from_slice(&[0x48, 0x81, 0xEC]);
    code.extend_from_slice(&le(FRAME));
    store_imm(&mut code, BYTES, ABCD);
    store_imm(&mut code, BYTES + 4, 0x7978_6665); // "efxy"

    // --- a datagram carries a pipe's write end ---
    mov_edi_imm(&mut code, 1); // AF_UNIX
    mov_esi_imm(&mut code, 2); // SOCK_DGRAM
    mov_edx_imm(&mut code, 0);
    lea_r10(&mut code, D0);
    syscall(&mut code, SOCKETPAIR);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x01);
    pipe(&mut code, P1);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x02);
    send_msg(&mut code, 6, 1, &[P1 + 4]);
    sendmsg(&mut code, D0);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x03);
    close_fd(&mut code, P1 + 4);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x04);
    recv_msg(&mut code, 32);
    recvmsg(&mut code, D1, MSG_CMSG_CLOEXEC);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x05);
    for (at, want) in [
        (CTL_IN, 20u32),
        (CTL_IN + 4, 0),
        (CTL_IN + 8, 1),
        (CTL_IN + 12, 1),
        (RMSG + 40, 24),
        (RMSG + 48, 0),
    ] {
        cmp_mem(&mut code, at, want);
        jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x06);
    }
    // fcntl(w2, F_GETFD)
    mov_edi_mem(&mut code, CTL_IN + 16);
    mov_esi_imm(&mut code, 1);
    syscall(&mut code, FCNTL);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x07);
    write_fd(&mut code, CTL_IN + 16, 6, 2);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x08);
    read_fd(&mut code, P1);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x09);
    close_fd(&mut code, CTL_IN + 16);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0A);
    read_fd(&mut code, P1);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0B);

    // --- a stream: the descriptors ride on their send's bytes ---
    mov_edi_imm(&mut code, 1);
    mov_esi_imm(&mut code, 1); // SOCK_STREAM
    mov_edx_imm(&mut code, 0);
    lea_r10(&mut code, S0);
    syscall(&mut code, SOCKETPAIR);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0C);
    pipe(&mut code, P2);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0D);
    write_fd(&mut code, S0, 0, 2);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0E);
    send_msg(&mut code, 2, 2, &[P2 + 4]);
    sendmsg(&mut code, S0);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0F);
    write_fd(&mut code, S0, 4, 2);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x10);
    recv_msg(&mut code, 32);
    recvmsg(&mut code, S1, 0);
    cmp_rax(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x11);
    cmp_mem(&mut code, BUF, ABCD);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x11);
    cmp_mem(&mut code, CTL_IN, 20);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x12);
    cmp_mem(&mut code, CTL_IN + 12, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x12);
    copy_dword(&mut code, P3 + 8, CTL_IN + 16); // w4, kept past the next receive
    read_fd(&mut code, S1);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x13);
    store_imm(&mut code, BUF + 2, 0);
    cmp_mem(&mut code, BUF, EF);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x13);
    close_fd(&mut code, P2 + 4);
    read_fd(&mut code, P2);
    cmp_rax(&mut code, -11); // EAGAIN: w4 still writes
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x14);
    close_fd(&mut code, P3 + 8);
    read_fd(&mut code, P2);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x15);

    // --- room for one of two: MSG_CTRUNC, the other released ---
    pipe(&mut code, P3);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x16);
    send_msg(&mut code, 7, 1, &[P3 + 4, P3 + 4]);
    sendmsg(&mut code, D0);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x17);
    close_fd(&mut code, P3 + 4);
    recv_msg(&mut code, 20);
    recvmsg(&mut code, D1, 0);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x18);
    test_mem(&mut code, RMSG + 48, MSG_CTRUNC);
    jcc_fail(&mut code, &mut fail_jumps, JZ, 0x19);
    cmp_mem(&mut code, CTL_IN, 20);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x19);
    cmp_mem(&mut code, RMSG + 40, 20);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x19);
    close_fd(&mut code, CTL_IN + 16);
    read_fd(&mut code, P3);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x1A);

    // --- read(2) cannot hand descriptors on: released ---
    pipe(&mut code, P4);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x1B);
    send_msg(&mut code, 6, 1, &[P4 + 4]);
    sendmsg(&mut code, D0);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x1C);
    close_fd(&mut code, P4 + 4);
    read_fd(&mut code, D1);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x1D);
    read_fd(&mut code, P4);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x1E);

    // --- a descriptor that is not open ---
    store_imm(&mut code, P4 + 4, 999);
    send_msg(&mut code, 6, 1, &[P4 + 4]);
    sendmsg(&mut code, D0);
    cmp_rax(&mut code, -9); // EBADF
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x1F);

    // --- sequenced packets: each send whole, the peer's close the end ---
    mov_edi_imm(&mut code, 1);
    mov_esi_imm(&mut code, 5); // SOCK_SEQPACKET
    mov_edx_imm(&mut code, 0);
    lea_r10(&mut code, Q0);
    syscall(&mut code, SOCKETPAIR);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x20);
    write_fd(&mut code, Q0, 0, 4);
    cmp_rax(&mut code, 4);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x21);
    write_fd(&mut code, Q0, 4, 2);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x21);
    recv_msg(&mut code, 0);
    store_imm64(&mut code, RIOV + 8, 2);
    recvmsg(&mut code, Q1, 0);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x22);
    test_mem(&mut code, RMSG + 48, MSG_TRUNC);
    jcc_fail(&mut code, &mut fail_jumps, JZ, 0x22);
    read_fd(&mut code, Q1);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x23);
    store_imm(&mut code, BUF + 2, 0);
    cmp_mem(&mut code, BUF, EF);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x23);
    close_fd(&mut code, Q0);
    read_fd(&mut code, Q1);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x24);
    write_fd(&mut code, Q1, 6, 1);
    cmp_rax(&mut code, -32); // EPIPE
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x25);

    // exit(0x5F)
    mov_edi_imm(&mut code, 0x5F);
    syscall(&mut code, EXIT);

    // One failure exit per sentinel.
    let mut fail_at: alloc::vec::Vec<(u8, usize)> = alloc::vec::Vec::new();
    for sentinel in 0x01u8..=0x25 {
        fail_at.push((sentinel, code.len()));
        mov_edi_imm(&mut code, u32::from(sentinel));
        syscall(&mut code, EXIT);
    }
    for (at, sentinel) in fail_jumps {
        if let Some(&(_, target)) = fail_at.iter().find(|(s, _)| *s == sentinel) {
            let disp = (target as i64 - (at as i64 + 4)) as i32;
            code[at..at + 4].copy_from_slice(&disp.to_le_bytes());
        }
    }
    let mut elf = single_segment_test_elf(&code);
    elf[EI_OSABI] = ELFOSABI_GNU;
    elf
}

/// Build a **Linux-ABI** ring-3 program that sets and reads a file's inode
/// flags as `chattr` and `lsattr` do (`FS_IOC_SETFLAGS`, `FS_IOC_GETFLAGS`),
/// and finds them enforced, exiting `0x60` when every step answers as
/// Linux's would and a sentinel naming the first that did not:
///
/// ```text
///   unlink("/tmp/_fflags")                        ; a leftover, if any
///   fd = open("/tmp/_fflags", O_RDWR|O_CREAT, 0644)  ; >= 0, else 0x01
///   ioctl(fd, FS_IOC_GETFLAGS, &f)                ; 0, else 0x02
///   f == 0                                        ; else 0x03
///   if getuid() != 0 goto not_root
///   ioctl(fd, FS_IOC_SETFLAGS, &FS_IMMUTABLE_FL)  ; 0, else 0x04
///   ioctl(fd, FS_IOC_GETFLAGS, &f), f == 0x10     ; else 0x05
///   write(fd, "x", 1)                             ; -EPERM, else 0x06
///   open("/tmp/_fflags", O_WRONLY)                ; -EPERM, else 0x07
///   unlink("/tmp/_fflags")                        ; -EPERM, else 0x08
///   access("/tmp/_fflags", W_OK)                  ; -EPERM, else 0x09
///   ioctl(fd, FS_IOC_SETFLAGS, &0x50)             ; -EOPNOTSUPP (FS_NODUMP_FL
///                                                 ; is not kept), else 0x0A
///   ioctl(fd, FS_IOC_SETFLAGS, &0)                ; 0, else 0x0B
///   write(fd, "x", 1)                             ; 1, else 0x0C
///   fcntl(fd, F_SETFL, O_APPEND); lseek(fd, 0)    ; 0, 0, else 0x13
///   write(fd, "x", 1)                             ; 1, else 0x14
///   lseek(fd, 0); read(fd, buf, 16)               ; 2 -- the write appended,
///                                                 ; else 0x15
///   ioctl(fd, FS_IOC_SETFLAGS, &FS_APPEND_FL)     ; 0, then
///   fcntl(fd, F_SETFL, 0)                         ; -EPERM, then
///   ioctl(fd, FS_IOC_SETFLAGS, &0)                ; 0, else 0x16
///   close(fd); unlink("/tmp/_fflags")             ; 0, else 0x0D
///   pipe2(p, 0); ioctl(p[0], FS_IOC_GETFLAGS, &f) ; -ENOTTY, else 0x0E
///   exit(0x60)
/// not_root:                                       ; the file's owner
///   ioctl(fd, FS_IOC_SETFLAGS, &FS_IMMUTABLE_FL)  ; -EPERM, else 0x10
///   ioctl(fd, FS_IOC_SETFLAGS, &0)                ; 0 (no change), else 0x11
///   close(fd); unlink("/tmp/_fflags")             ; 0, else 0x12
///   exit(0x60)
/// ```
///
/// Run twice by [`super::spawn::self_test_linux_file_flags`], as root and as
/// uid 1000: the immutable flag is root's to set (`CAP_LINUX_IMMUTABLE`),
/// and set, it refuses a write through a descriptor opened before it, a new
/// writable open, an unlink and `access(W_OK)` alike -- and is lifted by the
/// same ioctl, after which the same descriptor writes again, so the
/// refusals were the flag's. `O_APPEND` set by `fcntl(F_SETFL)` moves the next
/// write to the end, and on an append-only file may not be cleared. Tagged
/// `ELFOSABI_GNU` for the SysV stack + Linux ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::too_many_lines
)]
pub fn build_linux_file_flags_test_elf() -> alloc::vec::Vec<u8> {
    /// `jnz rel32`'s second opcode byte.
    const JNZ: u8 = 0x85;
    /// `jl rel32`'s second opcode byte.
    const JL: u8 = 0x8C;
    // Linux x86-64 syscall numbers.
    const READ: u32 = 0;
    const WRITE: u32 = 1;
    const OPEN: u32 = 2;
    const CLOSE: u32 = 3;
    const LSEEK: u32 = 8;
    const IOCTL: u32 = 16;
    const FCNTL: u32 = 72;
    const ACCESS: u32 = 21;
    const EXIT: u32 = 60;
    const UNLINK: u32 = 87;
    const GETUID: u32 = 102;
    const PIPE2: u32 = 293;
    const O_WRONLY: u32 = 0o1;
    const O_RDWR_CREAT: u32 = 0o102;
    const W_OK: u32 = 2;
    const FS_IOC_GETFLAGS: u32 = 0x8008_6601;
    const FS_IOC_SETFLAGS: u32 = 0x4008_6602;
    const FS_IMMUTABLE_FL: u32 = 0x10;
    const FS_APPEND_FL: u32 = 0x20;
    const F_SETFL: u32 = 4;
    const O_APPEND: u32 = 0o2000;
    /// `FS_IMMUTABLE_FL | FS_NODUMP_FL`: one kept, one not.
    const WITH_NODUMP: u32 = 0x50;
    const EPERM: i32 = -1;
    const ENOTTY: i32 = -25;
    const EOPNOTSUPP: i32 = -95;
    // Stack layout (all [rsp + offset]).
    const PATH: u32 = 0x00; // "/tmp/_fflags\0", 16 bytes
    const FD: u32 = 0x10;
    const FLAGS: u32 = 0x18; // the ioctl's int, 8 bytes reserved
    const P: u32 = 0x20; // pipe2's two descriptors
    const BYTE: u32 = 0x28; // "x"
    const BUF: u32 = 0x30; // 16 bytes
    const FRAME: u32 = 0x40;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut fail_jumps: alloc::vec::Vec<(usize, u8)> = alloc::vec::Vec::new();
    let le = |v: u32| v.to_le_bytes();
    // Emitters, every stack operand as [rsp + disp32].
    let mov_edi_mem = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x8B, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let mov_edi_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBF);
        c.extend_from_slice(&le(v));
    };
    let mov_esi_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBE);
        c.extend_from_slice(&le(v));
    };
    let mov_edx_imm = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBA);
        c.extend_from_slice(&le(v));
    };
    let lea_rdi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_rsi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0xB4, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_rdx = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0x94, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // mov dword [rsp + d], imm32
    let store_imm = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0xC7, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    // mov [rsp + d], eax
    let store_eax = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x89, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let syscall = |c: &mut alloc::vec::Vec<u8>, nr: u32| {
        c.push(0xB8);
        c.extend_from_slice(&le(nr));
        c.extend_from_slice(&[0x0F, 0x05]);
    };
    let cmp_rax = |c: &mut alloc::vec::Vec<u8>, v: i32| {
        c.extend_from_slice(&[0x48, 0x3D]);
        c.extend_from_slice(&v.to_le_bytes());
    };
    // cmp dword [rsp + d], imm32
    let cmp_mem = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0x81, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    let jcc_fail = |c: &mut alloc::vec::Vec<u8>,
                    fails: &mut alloc::vec::Vec<(usize, u8)>,
                    op: u8,
                    sentinel: u8| {
        c.extend_from_slice(&[0x0F, op, 0, 0, 0, 0]);
        fails.push((c.len() - 4, sentinel));
    };
    // ioctl(fd at `fd`, request, &FLAGS) with FLAGS set to `flags` first.
    let ioctl_flags = |c: &mut alloc::vec::Vec<u8>, fd: u32, request: u32, flags: u32| {
        store_imm(c, FLAGS, flags);
        mov_edi_mem(c, fd);
        mov_esi_imm(c, request);
        lea_rdx(c, FLAGS);
        syscall(c, IOCTL);
    };
    let path_call = |c: &mut alloc::vec::Vec<u8>, nr: u32, esi: u32, edx: u32| {
        lea_rdi(c, PATH);
        mov_esi_imm(c, esi);
        mov_edx_imm(c, edx);
        syscall(c, nr);
    };
    let write_x = |c: &mut alloc::vec::Vec<u8>| {
        mov_edi_mem(c, FD);
        lea_rsi(c, BYTE);
        mov_edx_imm(c, 1);
        syscall(c, WRITE);
    };
    // fcntl(fd, F_SETFL, flags)
    let setfl = |c: &mut alloc::vec::Vec<u8>, flags: u32| {
        mov_edi_mem(c, FD);
        mov_esi_imm(c, F_SETFL);
        mov_edx_imm(c, flags);
        syscall(c, FCNTL);
    };
    // lseek(fd, 0, SEEK_SET)
    let rewind = |c: &mut alloc::vec::Vec<u8>| {
        mov_edi_mem(c, FD);
        mov_esi_imm(c, 0);
        mov_edx_imm(c, 0);
        syscall(c, LSEEK);
    };

    // sub rsp, FRAME
    code.extend_from_slice(&[0x48, 0x81, 0xEC]);
    code.extend_from_slice(&le(FRAME));
    store_imm(&mut code, PATH, 0x706D_742F); // "/tmp"
    store_imm(&mut code, PATH + 4, 0x6666_5F2F); // "/_ff"
    store_imm(&mut code, PATH + 8, 0x7367_616C); // "lags"
    store_imm(&mut code, PATH + 12, 0);
    store_imm(&mut code, BYTE, u32::from(b'x'));

    // A leftover from an earlier run, if any; its answer does not matter.
    path_call(&mut code, UNLINK, 0, 0);
    path_call(&mut code, OPEN, O_RDWR_CREAT, 0o644);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JL, 0x01);
    store_eax(&mut code, FD);
    // Not yet set: GETFLAGS writes 0 over the all-ones it is given.
    ioctl_flags(&mut code, FD, FS_IOC_GETFLAGS, u32::MAX);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x02);
    cmp_mem(&mut code, FLAGS, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x03);

    syscall(&mut code, GETUID);
    cmp_rax(&mut code, 0);
    code.extend_from_slice(&[0x0F, JNZ, 0, 0, 0, 0]);
    let to_not_root = code.len() - 4;

    // --- root: set, enforced, refused for a flag not kept, cleared ---
    ioctl_flags(&mut code, FD, FS_IOC_SETFLAGS, FS_IMMUTABLE_FL);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x04);
    ioctl_flags(&mut code, FD, FS_IOC_GETFLAGS, 0);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x05);
    cmp_mem(&mut code, FLAGS, FS_IMMUTABLE_FL);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x05);
    write_x(&mut code);
    cmp_rax(&mut code, EPERM);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x06);
    path_call(&mut code, OPEN, O_WRONLY, 0);
    cmp_rax(&mut code, EPERM);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x07);
    path_call(&mut code, UNLINK, 0, 0);
    cmp_rax(&mut code, EPERM);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x08);
    path_call(&mut code, ACCESS, W_OK, 0);
    cmp_rax(&mut code, EPERM);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x09);
    ioctl_flags(&mut code, FD, FS_IOC_SETFLAGS, WITH_NODUMP);
    cmp_rax(&mut code, EOPNOTSUPP);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0A);
    ioctl_flags(&mut code, FD, FS_IOC_SETFLAGS, 0);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0B);
    write_x(&mut code);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0C);
    // O_APPEND by fcntl reaches the open file: written after a rewind, the
    // byte lands at the end, and the file holds two.
    setfl(&mut code, O_APPEND);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x13);
    rewind(&mut code);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x13);
    write_x(&mut code);
    cmp_rax(&mut code, 1);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x14);
    rewind(&mut code);
    mov_edi_mem(&mut code, FD);
    lea_rsi(&mut code, BUF);
    mov_edx_imm(&mut code, 16);
    syscall(&mut code, READ);
    cmp_rax(&mut code, 2);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x15);
    // Append-only, the descriptor keeps O_APPEND.
    ioctl_flags(&mut code, FD, FS_IOC_SETFLAGS, FS_APPEND_FL);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x16);
    setfl(&mut code, 0);
    cmp_rax(&mut code, EPERM);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x16);
    ioctl_flags(&mut code, FD, FS_IOC_SETFLAGS, 0);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x16);
    mov_edi_mem(&mut code, FD);
    syscall(&mut code, CLOSE);
    path_call(&mut code, UNLINK, 0, 0);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0D);
    // Not a regular file: a pipe has no inode flags.
    lea_rdi(&mut code, P);
    mov_esi_imm(&mut code, 0);
    syscall(&mut code, PIPE2);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0E);
    ioctl_flags(&mut code, P, FS_IOC_GETFLAGS, 0);
    cmp_rax(&mut code, ENOTTY);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x0E);
    mov_edi_imm(&mut code, 0x60);
    syscall(&mut code, EXIT);

    // --- not root: the owner, who may not set it ---
    let not_root = code.len();
    let disp = (not_root as i64 - (to_not_root as i64 + 4)) as i32;
    code[to_not_root..to_not_root + 4].copy_from_slice(&disp.to_le_bytes());
    ioctl_flags(&mut code, FD, FS_IOC_SETFLAGS, FS_IMMUTABLE_FL);
    cmp_rax(&mut code, EPERM);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x10);
    ioctl_flags(&mut code, FD, FS_IOC_SETFLAGS, 0);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x11);
    mov_edi_mem(&mut code, FD);
    syscall(&mut code, CLOSE);
    path_call(&mut code, UNLINK, 0, 0);
    cmp_rax(&mut code, 0);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0x12);
    mov_edi_imm(&mut code, 0x60);
    syscall(&mut code, EXIT);

    // One failure exit per sentinel.
    let mut fail_at: alloc::vec::Vec<(u8, usize)> = alloc::vec::Vec::new();
    for sentinel in 0x01u8..=0x16 {
        fail_at.push((sentinel, code.len()));
        mov_edi_imm(&mut code, u32::from(sentinel));
        syscall(&mut code, EXIT);
    }
    for (at, sentinel) in fail_jumps {
        if let Some(&(_, target)) = fail_at.iter().find(|(s, _)| *s == sentinel) {
            let disp = (target as i64 - (at as i64 + 4)) as i32;
            code[at..at + 4].copy_from_slice(&disp.to_le_bytes());
        }
    }
    let mut elf = single_segment_test_elf(&code);
    elf[EI_OSABI] = ELFOSABI_GNU;
    elf
}

/// Build a **native-ABI** ring-3 program that drives the sound card through
/// the device door (`SYS_DEVICE_*`, 1119-1123) as the C library will, its
/// answers Linux errnos:
///
/// ```text
///   open("/dev/snd/nope")                         ; -ENOENT, else exit 0xC1
///   h = open("/dev/snd/pcmC0D0p")                 ; kind 1 in rdx, else 0xC2
///   ioctl(PCM, h, HW_PARAMS, &zeroed hw_params)   ; 0, else 0xC3
///   ioctl(PCM, h, PREPARE)                        ; 0, else 0xC4
///   write(PCM, h, 32 KiB, blocking)               ; all 32 KiB -- two rings,
///                                                 ; so it waits on the pump -- else 0xC5
///   ioctl(PCM, h, DRAIN)                          ; 0 once played out, else 0xC6
///   ioctl(PCM, h, STATUS, &status)                ; 0 and state SETUP, else 0xC7
///   ioctl(PCM, h, 0x41FF)                         ; -ENOTTY, else 0xC8
///   ioctl(PCM, h, PREPARE), START, PAUSE 1        ; 0 each, else 0xC9 / 0xCA / 0xCB
///                                                 ; (paused: nothing drains the ring)
///   write(PCM, h, 32 KiB, DEVICE_NONBLOCK)        ; one ring, 16 KiB, else 0xCC
///   write(PCM, h, 4 KiB, DEVICE_NONBLOCK)         ; -EAGAIN (full, held), else 0xCD
///   ioctl(PCM, h, DROP)                           ; 0, else 0xCE
///   close(PCM, h)                                 ; 0, else 0xCF
///   close(PCM, h)                                 ; -EBADF (not held now), else 0xD0
///   c = open("/dev/snd/controlC0")                ; handle 1, kind 2, else 0xD1
///   ioctl(CONTROL, c, CARD_INFO, &info)           ; 0, else 0xD2
///   read(CONTROL, c, buf, 16)                     ; -EINVAL (ioctl-only), else 0xD3
///   exit(0x5E)
/// ```
///
/// A clean `exit(0x5E)` proves the door end to end: the kernel's Linux
/// handlers reached natively, a blocking write that outlasts the ring (the
/// output pump drained it), a blocking drain, a non-blocking write that fills
/// one ring and then refuses, a pause that holds the queue, the handle owned
/// by its opener and gone after close, and the control device.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::too_many_lines
)]
pub fn build_native_device_door_test_elf() -> alloc::vec::Vec<u8> {
    use crate::audio_alsa as alsa;
    use crate::syscall::number as nr;
    /// `jnz rel32`'s second opcode byte.
    const JNZ: u8 = 0x85;
    /// `js rel32`'s second opcode byte.
    const JS: u8 = 0x88;
    const SYS_EXIT: u32 = 1;
    // Stack layout (all [rsp + offset]).
    const DATA: u32 = 0x0000; // 32 KiB of silence
    const HWP: u32 = 0x8000; // struct snd_pcm_hw_params, 608 bytes
    const STATUS: u32 = 0x8280; // struct snd_pcm_status, 152 bytes
    const CARDINFO: u32 = 0x8340; // struct snd_ctl_card_info, 376 bytes
    const PATH_PCM: u32 = 0x84C0;
    const PATH_CTL: u32 = 0x84E0;
    const PATH_BAD: u32 = 0x8500;
    const HANDLE: u32 = 0x8520;
    const FRAME: u32 = 0x8600;
    const PCM: u32 = nr::DEVICE_KIND_PCM as u32;
    const CONTROL: u32 = nr::DEVICE_KIND_CONTROL as u32;
    const NONBLOCK: u32 = nr::DEVICE_NONBLOCK as u32;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut fail_jumps: alloc::vec::Vec<(usize, u8)> = alloc::vec::Vec::new();
    let le = |v: u32| v.to_le_bytes();
    let syscall = |c: &mut alloc::vec::Vec<u8>, number: u64| {
        c.push(0xB8);
        c.extend_from_slice(&le(number as u32));
        c.extend_from_slice(&[0x0F, 0x05]);
    };
    let mov_edi = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBF);
        c.extend_from_slice(&le(v));
    };
    let mov_esi = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBE);
        c.extend_from_slice(&le(v));
    };
    let mov_edx = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.push(0xBA);
        c.extend_from_slice(&le(v));
    };
    let mov_r10d = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.extend_from_slice(&[0x41, 0xBA]);
        c.extend_from_slice(&le(v));
    };
    let mov_r8d = |c: &mut alloc::vec::Vec<u8>, v: u32| {
        c.extend_from_slice(&[0x41, 0xB8]);
        c.extend_from_slice(&le(v));
    };
    // lea rdi / rdx / r10, [rsp + d32]
    let lea_rdi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_rdx = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8D, 0x94, 0x24]);
        c.extend_from_slice(&le(d));
    };
    let lea_r10 = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x4C, 0x8D, 0x94, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // mov rsi, [rsp + d32] -- the handle
    let load_rsi = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x8B, 0xB4, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // mov [rsp + d32], rax
    let store_rax = |c: &mut alloc::vec::Vec<u8>, d: u32| {
        c.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
    };
    // mov dword [rsp + d32], imm32
    let store_imm = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0xC7, 0x84, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    // cmp dword [rsp + d32], imm32
    let cmp_mem = |c: &mut alloc::vec::Vec<u8>, d: u32, v: u32| {
        c.extend_from_slice(&[0x81, 0xBC, 0x24]);
        c.extend_from_slice(&le(d));
        c.extend_from_slice(&le(v));
    };
    // cmp rax, imm32 (sign-extended)
    let cmp_rax = |c: &mut alloc::vec::Vec<u8>, v: i32| {
        c.extend_from_slice(&[0x48, 0x3D]);
        c.extend_from_slice(&v.to_le_bytes());
    };
    // cmp rdx, imm8 (sign-extended)
    let cmp_rdx = |c: &mut alloc::vec::Vec<u8>, v: i8| {
        c.extend_from_slice(&[0x48, 0x83, 0xFA, v as u8]);
    };
    let test_rax = |c: &mut alloc::vec::Vec<u8>| c.extend_from_slice(&[0x48, 0x85, 0xC0]);
    let jcc_fail = |c: &mut alloc::vec::Vec<u8>,
                    fails: &mut alloc::vec::Vec<(usize, u8)>,
                    op: u8,
                    sentinel: u8| {
        c.extend_from_slice(&[0x0F, op, 0, 0, 0, 0]);
        fails.push((c.len() - 4, sentinel));
    };
    // A NUL-terminated string at [rsp + d], a dword at a time.
    let put_str = |c: &mut alloc::vec::Vec<u8>, d: u32, s: &[u8]| {
        let mut bytes = alloc::vec::Vec::from(s);
        bytes.push(0);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        for (i, w) in bytes.chunks(4).enumerate() {
            let word = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            store_imm(c, d + 4 * i as u32, word);
        }
    };
    // ioctl(kind, [rsp + HANDLE] or `handle`, request, arg in r10, flags 0)
    let ioctl = |c: &mut alloc::vec::Vec<u8>, kind: u32, request: u32| {
        mov_edi(c, kind);
        load_rsi(c, HANDLE);
        mov_edx(c, request);
        mov_r8d(c, 0);
        syscall(c, nr::SYS_DEVICE_IOCTL);
    };

    // sub rsp, FRAME -- a native program starts at the stack's very top.
    code.extend_from_slice(&[0x48, 0x81, 0xEC]);
    code.extend_from_slice(&le(FRAME));
    put_str(&mut code, PATH_PCM, b"/dev/snd/pcmC0D0p");
    put_str(&mut code, PATH_CTL, b"/dev/snd/controlC0");
    put_str(&mut code, PATH_BAD, b"/dev/snd/nope");
    // Zero the hw_params and the status: rep stosq over each.
    for (at, len) in [(HWP, 608u32), (STATUS, 152)] {
        lea_rdi(&mut code, at);
        code.extend_from_slice(&[0x31, 0xC0]); // xor eax, eax
        code.push(0xB9); // mov ecx, qwords
        code.extend_from_slice(&le(len / 8));
        code.extend_from_slice(&[0xF3, 0x48, 0xAB]); // rep stosq
    }

    // --- open: a path the door does not serve, then the playback node ---
    lea_rdi(&mut code, PATH_BAD);
    mov_esi(&mut code, 13);
    syscall(&mut code, nr::SYS_DEVICE_OPEN);
    cmp_rax(&mut code, -2); // ENOENT
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC1);
    lea_rdi(&mut code, PATH_PCM);
    mov_esi(&mut code, 17);
    syscall(&mut code, nr::SYS_DEVICE_OPEN);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JS, 0xC2);
    cmp_rdx(&mut code, PCM as i8);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC2);
    store_rax(&mut code, HANDLE);

    // --- configure, prepare, a blocking write of two rings, a drain ---
    lea_r10(&mut code, HWP);
    ioctl(&mut code, PCM, alsa::SNDRV_PCM_IOCTL_HW_PARAMS);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC3);
    mov_r10d(&mut code, 0);
    ioctl(&mut code, PCM, alsa::SNDRV_PCM_IOCTL_PREPARE);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC4);
    // write(PCM, h, DATA, 0x8000, 0): rdx = buf, r10 = len, r8 = flags
    mov_edi(&mut code, PCM);
    load_rsi(&mut code, HANDLE);
    lea_rdx(&mut code, DATA);
    mov_r10d(&mut code, 0x8000);
    mov_r8d(&mut code, 0);
    syscall(&mut code, nr::SYS_DEVICE_WRITE);
    cmp_rax(&mut code, 0x8000);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC5);
    mov_r10d(&mut code, 0);
    ioctl(&mut code, PCM, alsa::SNDRV_PCM_IOCTL_DRAIN);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC6);
    lea_r10(&mut code, STATUS);
    ioctl(&mut code, PCM, alsa::SNDRV_PCM_IOCTL_STATUS);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC7);
    cmp_mem(&mut code, STATUS, crate::ipc::alsa_pcm::STATE_SETUP);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC7);
    mov_r10d(&mut code, 0);
    ioctl(&mut code, PCM, 0x41FF);
    cmp_rax(&mut code, -25); // ENOTTY
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xC8);

    // --- non-blocking, paused so nothing drains the ring between the two
    // writes: one ring goes in, then nothing ---
    for (request, arg, sentinel) in [
        (alsa::SNDRV_PCM_IOCTL_PREPARE, 0u32, 0xC9u8),
        (alsa::SNDRV_PCM_IOCTL_START, 0, 0xCA),
        (alsa::SNDRV_PCM_IOCTL_PAUSE, 1, 0xCB),
    ] {
        mov_r10d(&mut code, arg);
        ioctl(&mut code, PCM, request);
        test_rax(&mut code);
        jcc_fail(&mut code, &mut fail_jumps, JNZ, sentinel);
    }
    mov_edi(&mut code, PCM);
    load_rsi(&mut code, HANDLE);
    lea_rdx(&mut code, DATA);
    mov_r10d(&mut code, 0x8000);
    mov_r8d(&mut code, NONBLOCK);
    syscall(&mut code, nr::SYS_DEVICE_WRITE);
    cmp_rax(&mut code, 0x4000);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xCC);
    mov_edi(&mut code, PCM);
    load_rsi(&mut code, HANDLE);
    lea_rdx(&mut code, DATA);
    mov_r10d(&mut code, 0x1000);
    mov_r8d(&mut code, NONBLOCK);
    syscall(&mut code, nr::SYS_DEVICE_WRITE);
    cmp_rax(&mut code, -11); // EAGAIN
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xCD);
    mov_r10d(&mut code, 0);
    ioctl(&mut code, PCM, alsa::SNDRV_PCM_IOCTL_DROP);
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xCE);

    // --- close, and the handle is no longer held ---
    for sentinel_and_answer in [(0xCFu8, 0i32), (0xD0, -9)] {
        mov_edi(&mut code, PCM);
        load_rsi(&mut code, HANDLE);
        syscall(&mut code, nr::SYS_DEVICE_CLOSE);
        cmp_rax(&mut code, sentinel_and_answer.1);
        jcc_fail(&mut code, &mut fail_jumps, JNZ, sentinel_and_answer.0);
    }

    // --- the control device ---
    lea_rdi(&mut code, PATH_CTL);
    mov_esi(&mut code, 18);
    syscall(&mut code, nr::SYS_DEVICE_OPEN);
    cmp_rax(&mut code, nr::DEVICE_CONTROL_HANDLE as i32);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD1);
    cmp_rdx(&mut code, CONTROL as i8);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD1);
    store_rax(&mut code, HANDLE);
    lea_r10(&mut code, CARDINFO);
    ioctl(
        &mut code,
        CONTROL,
        crate::audio_alsa_ctl::SNDRV_CTL_IOCTL_CARD_INFO,
    );
    test_rax(&mut code);
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD2);
    mov_edi(&mut code, CONTROL);
    load_rsi(&mut code, HANDLE);
    lea_rdx(&mut code, DATA);
    mov_r10d(&mut code, 16);
    mov_r8d(&mut code, 0);
    syscall(&mut code, nr::SYS_DEVICE_READ);
    cmp_rax(&mut code, -22); // EINVAL
    jcc_fail(&mut code, &mut fail_jumps, JNZ, 0xD3);

    // exit(0x5E)
    mov_edi(&mut code, 0x5E);
    syscall(&mut code, u64::from(SYS_EXIT));

    // One failure exit per sentinel.
    let mut fail_at: alloc::vec::Vec<(u8, usize)> = alloc::vec::Vec::new();
    for sentinel in 0xC1u8..=0xD3 {
        fail_at.push((sentinel, code.len()));
        mov_edi(&mut code, u32::from(sentinel));
        syscall(&mut code, u64::from(SYS_EXIT));
    }
    code.push(0xCC); // int3
    for (at, sentinel) in fail_jumps {
        if let Some(&(_, target)) = fail_at.iter().find(|(s, _)| *s == sentinel) {
            let disp = (target as i64 - (at as i64 + 4)) as i32;
            code[at..at + 4].copy_from_slice(&disp.to_le_bytes());
        }
    }
    single_segment_test_elf(&code)
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
pub fn build_linux_fchmodat2_emptypath_test_elf(path_nul: &[u8], mode: u32) -> alloc::vec::Vec<u8> {
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

/// Build a **Linux-ABI** `ET_EXEC` test ELF that interrogates
/// `/dev/input/event0` from ring 3 exactly the way a real input client does.
///
/// This is the only test that exercises the evdev node the way it will
/// actually be used. The kernel-internal self-tests
/// ([`crate::evdev::self_test`], [`crate::evdev_fd::self_test`]) call Rust
/// functions directly, so they cannot catch anything that lives *between* the
/// two: a missing `ioctl` dispatch arm, a capability gate that rejects the
/// wrong thing, an `_IOC` number the syscall layer decodes differently from
/// the way userspace encoded it, a copy-out that faults. Those are precisely
/// the failures a client would hit first and we would never see.
///
/// The order below is not arbitrary — it is the order `libinput`,
/// `evtest` and SDL interrogate a device in, so a break shows up here in the
/// same step it would break for them:
///
/// ```text
///   open("/dev/input/event0", O_RDONLY|O_NONBLOCK)
///   ioctl(EVIOCGVERSION)     -> 0, *arg == EV_VERSION
///   ioctl(EVIOCGID)          -> 0, id.bustype == BUS_I8042
///   ioctl(EVIOCGNAME(64))    -> >0, name[0] == 'A'   ("AT Translated …")
///   ioctl(EVIOCGBIT(0,4))    -> 4,  bit EV_KEY set
///   ioctl(EVIOCGKEY(96))     -> 96
///   ioctl(EVIOCGUNIQ(64))    -> -ENOENT (no serial on a PS/2 device)
///   read(fd, buf, 24)        -> -EAGAIN when idle (never blocks)
///   read(fd, buf, 8)         -> -EINVAL (sub-record buffer)
///   ioctl(EVIOCGRAB, 1)      -> 0      (argument used BY VALUE)
///   ioctl(EVIOCGRAB, 0)      -> 0      (ungrab)
///   ioctl(EVIOCSCLOCKID,&1)  -> 0      (this one IS a pointer)
///   ioctl(fd, 0x1234)        -> -ENOTTY (foreign magic)
///   close(fd)                -> 0
///   exit(0)
/// ```
///
/// Every stage loads its own sentinel into `edi` *before* the comparison that
/// might jump, so the harness can name the failing step from the exit code
/// alone rather than reporting "the evdev test failed".
///
/// With `expect_denied` the program instead only opens, requires `-EACCES`,
/// and exits 0 — proving the `ResourceType::InputDevice` gate actually denies
/// a process that was not granted it. A test that only ever runs *with* the
/// capability cannot tell a working gate from an absent one.
///
/// The `EVIOC*` request numbers are built with [`crate::evdev::ioc`], the same
/// encoder the self-test pins against the real Linux literals, so this program
/// asks with the identical bit pattern a C client would.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]
pub fn build_linux_evdev_test_elf(expect_denied: bool) -> alloc::vec::Vec<u8> {
    use crate::evdev;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Emit `mov edi, imm32` — the exit sentinel for the check that follows.
    /// `mov` leaves the flags alone, which is why it can precede the compare.
    fn sentinel(code: &mut Vec<u8>, value: u32) {
        code.push(0xBF);
        code.extend_from_slice(&value.to_le_bytes());
    }
    /// Emit a `rel32` conditional jump and record the displacement's offset so
    /// the caller can patch it once the target's position is known. `rel32`
    /// rather than `rel8` because the body is far longer than 127 bytes and a
    /// silently-truncated `rel8` would jump into the middle of an instruction.
    fn jcc(code: &mut Vec<u8>, sites: &mut Vec<usize>, cc: u8) {
        code.extend_from_slice(&[0x0F, cc, 0, 0, 0, 0]);
        sites.push(code.len() - 4);
    }
    /// Emit `ioctl(r8, request, <rdx set by `set_rdx`>)`.
    fn ioctl_call(code: &mut Vec<u8>, request: u32, set_rdx: &[u8]) {
        code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
        code.push(0xBE); // mov esi, imm32 (zero-extends into rsi)
        code.extend_from_slice(&request.to_le_bytes());
        code.extend_from_slice(set_rdx);
        code.extend_from_slice(&[0xB8, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,16; syscall
    }

    /// `EVIOCG*(size)` — every read-direction request this program issues.
    fn gread(nr: u32, size: u32) -> u32 {
        crate::evdev::ioc(crate::evdev::IOC_READ, nr, size)
    }

    /// Length of the `EVIOCGKEY` bitmap, used both as the request's size field
    /// and as the `cmp` immediate the return value is checked against. One
    /// constant, so the two cannot drift into a test that always passes.
    const KEY_BITMAP_LEN: u32 = evdev::KEY_BYTES as u32;

    // Condition codes for the 0x0F-prefixed rel32 forms.
    const JS: u8 = 0x88;
    const JNZ: u8 = 0x85;
    const JZ: u8 = 0x84;
    const JLE: u8 = 0x8E;

    // Scratch frame: [rsp+0x00] small result (u32 / struct input_id),
    // [rsp+0x08] the int EVIOCSCLOCKID points at, [rsp+0x10 .. 0x70] the
    // 96-byte general buffer.  Everything stays inside a signed disp8 so no
    // addressing mode needs a 4-byte displacement.
    const SET_RDX_RSP: &[u8] = &[0x48, 0x8D, 0x14, 0x24]; // lea rdx, [rsp]
    const SET_RDX_RSP08: &[u8] = &[0x48, 0x8D, 0x54, 0x24, 0x08]; // lea rdx, [rsp+8]
    const SET_RDX_RSP10: &[u8] = &[0x48, 0x8D, 0x54, 0x24, 0x10]; // lea rdx, [rsp+16]

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: Vec<u8> = Vec::new();
    let mut fail_sites: Vec<usize> = Vec::new();

    // sub rsp, 0x80 — imm32 form; 0x80 does not fit a signed imm8.
    code.extend_from_slice(&[0x48, 0x81, 0xEC, 0x80, 0x00, 0x00, 0x00]);

    // --- open("/dev/input/event0", O_RDONLY|O_NONBLOCK) ---------------------
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xBE, 0x00, 0x08, 0x00, 0x00]); // mov esi, 0o4000
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx (mode 0)
    code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,2; syscall

    if expect_denied {
        // Without the InputDevice capability the open must fail with EACCES.
        // Any other answer — a success above all — means the gate is not
        // doing its job, so it exits with the sentinel rather than 0.
        sentinel(&mut code, 0x21);
        code.extend_from_slice(&[0x48, 0x83, 0xF8, 0xF3]); // cmp rax, -13
        jcc(&mut code, &mut fail_sites, JNZ);
    } else {
        sentinel(&mut code, 0xE1);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
        jcc(&mut code, &mut fail_sites, JS);
        code.extend_from_slice(&[0x49, 0x89, 0xC0]); // mov r8, rax (save fd)

        // --- EVIOCGVERSION --------------------------------------------------
        code.extend_from_slice(&[0xC7, 0x04, 0x24, 0, 0, 0, 0]); // mov dword [rsp], 0
        ioctl_call(&mut code, gread(evdev::EVIOC_NR_GVERSION, 4), SET_RDX_RSP);
        sentinel(&mut code, 0xE2);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
        jcc(&mut code, &mut fail_sites, JNZ);
        sentinel(&mut code, 0xE3);
        code.extend_from_slice(&[0x8B, 0x04, 0x24]); // mov eax, [rsp]
        code.push(0x3D); // cmp eax, imm32
        code.extend_from_slice(&evdev::EV_VERSION.to_le_bytes());
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- EVIOCGID: the bus must be i8042, not a default-zero struct -----
        code.extend_from_slice(&[0x48, 0xC7, 0x04, 0x24, 0, 0, 0, 0]); // mov qword [rsp], 0
        ioctl_call(&mut code, gread(evdev::EVIOC_NR_GID, 8), SET_RDX_RSP);
        sentinel(&mut code, 0xE4);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]);
        jcc(&mut code, &mut fail_sites, JNZ);
        sentinel(&mut code, 0xE5);
        code.extend_from_slice(&[0x0F, 0xB7, 0x04, 0x24]); // movzx eax, word [rsp]
        code.extend_from_slice(&[0x83, 0xF8, evdev::BUS_I8042 as u8]); // cmp eax, imm8
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- EVIOCGNAME(64): "AT Translated Set 2 keyboard" -----------------
        code.extend_from_slice(&[0xC6, 0x44, 0x24, 0x10, 0x00]); // mov byte [rsp+16], 0
        ioctl_call(&mut code, gread(evdev::EVIOC_NR_GNAME, 64), SET_RDX_RSP10);
        sentinel(&mut code, 0xE6);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]);
        jcc(&mut code, &mut fail_sites, JLE); // must be a positive length
        sentinel(&mut code, 0xE7);
        code.extend_from_slice(&[0x80, 0x7C, 0x24, 0x10, b'A']); // cmp byte [rsp+16], 'A'
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- EVIOCGBIT(0, 4): the EV_* map, which must claim EV_KEY ---------
        code.extend_from_slice(&[0xC7, 0x44, 0x24, 0x10, 0, 0, 0, 0]); // mov dword [rsp+16], 0
        ioctl_call(
            &mut code,
            gread(evdev::EVIOC_NR_GBIT_BASE, 4),
            SET_RDX_RSP10,
        );
        sentinel(&mut code, 0xE8);
        code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x04]); // cmp rax, 4
        jcc(&mut code, &mut fail_sites, JNZ);
        sentinel(&mut code, 0xE9);
        code.extend_from_slice(&[0xF6, 0x44, 0x24, 0x10, 0x02]); // test byte [rsp+16], EV_KEY
        jcc(&mut code, &mut fail_sites, JZ);

        // --- EVIOCGKEY(96): the full key-state bitmap comes back ------------
        ioctl_call(
            &mut code,
            gread(evdev::EVIOC_NR_GKEY, KEY_BITMAP_LEN),
            SET_RDX_RSP10,
        );
        sentinel(&mut code, 0xEA);
        code.extend_from_slice(&[0x48, 0x83, 0xF8, KEY_BITMAP_LEN as u8]); // cmp rax, 96
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- EVIOCGUNIQ(64): refused, so the client falls back to GPHYS -----
        ioctl_call(&mut code, gread(evdev::EVIOC_NR_GUNIQ, 64), SET_RDX_RSP10);
        sentinel(&mut code, 0xEB);
        code.extend_from_slice(&[0x48, 0x83, 0xF8, 0xFE]); // cmp rax, -ENOENT
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- read(24) on an idle non-blocking fd ----------------------------
        // EAGAIN is the expected answer, but a keystroke arriving mid-test
        // would legitimately produce data instead, so a positive length is
        // accepted too. What must never happen is 0 (which a client reads as
        // EOF and closes the device on) or any other error.
        code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
        code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x10]); // lea rsi, [rsp+16]
        code.extend_from_slice(&[0xBA, 0x18, 0x00, 0x00, 0x00]); // mov edx, 24
        code.extend_from_slice(&[0x31, 0xC0, 0x0F, 0x05]); // xor eax,eax; syscall
        sentinel(&mut code, 0xEC);
        code.extend_from_slice(&[0x48, 0x83, 0xF8, 0xF5]); // cmp rax, -EAGAIN
        let mut read_ok_sites: Vec<usize> = Vec::new();
        jcc(&mut code, &mut read_ok_sites, JZ);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
        jcc(&mut code, &mut fail_sites, JLE);
        let read_ok = code.len();
        for site in &read_ok_sites {
            let disp = (read_ok as isize) - (*site as isize + 4);
            code[*site..*site + 4].copy_from_slice(&(disp as i32).to_le_bytes());
        }

        // --- read(8): a sub-record buffer is EINVAL, never a short read -----
        code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
        code.extend_from_slice(&[0x48, 0x8D, 0x74, 0x24, 0x10]); // lea rsi, [rsp+16]
        code.extend_from_slice(&[0xBA, 0x08, 0x00, 0x00, 0x00]); // mov edx, 8
        code.extend_from_slice(&[0x31, 0xC0, 0x0F, 0x05]); // xor eax,eax; syscall
        sentinel(&mut code, 0xED);
        code.extend_from_slice(&[0x48, 0x83, 0xF8, 0xEA]); // cmp rax, -EINVAL
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- EVIOCGRAB(1) then EVIOCGRAB(0) ---------------------------------
        // The argument is passed BY VALUE, exactly as libinput passes it
        // (`ioctl(fd, EVIOCGRAB, (void *)1)`). If the kernel ever went back to
        // dereferencing it, this would fault on address 1 and fail here.
        let grab = evdev::ioc(evdev::IOC_WRITE, evdev::EVIOC_NR_GRAB, 4);
        ioctl_call(&mut code, grab, &[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
        sentinel(&mut code, 0xEE);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]);
        jcc(&mut code, &mut fail_sites, JNZ);
        ioctl_call(&mut code, grab, &[0x31, 0xD2]); // xor edx, edx
        sentinel(&mut code, 0xEF);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]);
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- EVIOCSCLOCKID(CLOCK_MONOTONIC) ---------------------------------
        // This one really is a pointer to an int — the asymmetry with
        // EVIOCGRAB above is Linux's, and both halves of it are pinned here.
        code.extend_from_slice(&[0xC7, 0x44, 0x24, 0x08]); // mov dword [rsp+8], imm32
        code.extend_from_slice(&evdev::CLOCK_MONOTONIC.to_le_bytes());
        let sclockid = evdev::ioc(evdev::IOC_WRITE, evdev::EVIOC_NR_SCLOCKID, 4);
        ioctl_call(&mut code, sclockid, SET_RDX_RSP08);
        sentinel(&mut code, 0xF0);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]);
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- an ioctl of foreign magic is ENOTTY, not a wild dispatch -------
        ioctl_call(&mut code, 0x1234, &[0x31, 0xD2]); // xor edx, edx
        sentinel(&mut code, 0xF1);
        code.extend_from_slice(&[0x48, 0x83, 0xF8, 0xE7]); // cmp rax, -ENOTTY
        jcc(&mut code, &mut fail_sites, JNZ);

        // --- close(fd) ------------------------------------------------------
        code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
        code.extend_from_slice(&[0xB8, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,3; syscall
        sentinel(&mut code, 0xF2);
        code.extend_from_slice(&[0x48, 0x85, 0xC0]);
        jcc(&mut code, &mut fail_sites, JNZ);
    }

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // fail: exit(edi) — every check jumps here with its own sentinel loaded.
    let fail = code.len();
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3 — unreachable trap

    for site in &fail_sites {
        let disp = (fail as isize) - (*site as isize + 4);
        code[*site..*site + 4].copy_from_slice(&(disp as i32).to_le_bytes());
    }

    // --- Data layout (same PT_LOAD, after the code) ---
    let path: &[u8] = b"/dev/input/event0\0";
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

/// Build a **Linux-ABI** `ET_EXEC` test ELF that drives a full **virtio-gpu
/// render-node round trip** on `/dev/dri/renderD128` from ring 3 — the 2D
/// resource path that base virtio-gpu services without `VIRTIO_GPU_F_VIRGL`:
///
/// ```text
///   open("/dev/dri/renderD128", O_RDWR, 0)      ; render node fd
///   RESOURCE_CREATE  64x64 B8G8R8A8, target=2D  ; -> res handle, size, stride
///   MAP(res)                                    ; -> fake mmap offset
///   mmap(NULL, 16384, RW, MAP_SHARED, fd, off)  ; -> the guest backing
///   store/load a pattern at the first and last dword of the mapping
///   TRANSFER_TO_HOST(res, full 64x64 box)       ; a real device command
///   WAIT(res)
///   RESOURCE_INFO(res)                          ; size must match
///   GEM_CLOSE(res)                              ; the destroy path
///   RESOURCE_INFO(res) must now FAIL            ; the handle is gone
///   the mapping must still read back            ; frames are refcounted
///   exit(0)
/// ```
///
/// Each step has its own exit sentinel so the harness can name the failing
/// stage: `0xE1` open, `0xE2` RESOURCE_CREATE ioctl, `0xE3` zero res handle,
/// `0xE4` wrong stride, `0xE5` wrong size, `0xE6` MAP ioctl, `0xE7` mmap,
/// `0xE8` pattern at offset 0, `0xE9` pattern at the last dword, `0xEA`
/// TRANSFER_TO_HOST, `0xEB` WAIT, `0xEC` RESOURCE_INFO, `0xED` wrong info
/// size, `0xEE` GEM_CLOSE, `0xEF` the closed handle still resolved, `0xF0` the
/// mapping stopped reading back after GEM_CLOSE.
///
/// The last two are the ones worth having: `0xEF` catches a destroy that
/// forgets to drop the fd's ownership record (leaving a handle that outlives
/// its resource), and `0xF0` catches the opposite mistake — a destroy that
/// frees the backing frames out from under a live mapping, which would hand a
/// recycled frame to some other allocation while userspace still writes to it.
///
/// The stack scratch is laid out once and reused: the 56-byte
/// `drm_virtgpu_resource_create` at `[rsp+0]` is later overwritten by the
/// 44-byte transfer, the 8-byte wait, the 16-byte info and the 8-byte
/// `drm_gem_close`, since none of them overlap in time. `[rsp+64]` holds the
/// map struct and `[rsp+80..112]` the fd, resource handle, mmap offset and
/// mapping address. All displacements stay under 128 so every access uses a
/// one-byte displacement.
///
/// Every conditional branch is a **rel32** jump to a single `fail:` label that
/// exits with whatever `edi` was last set to; `mov` does not touch flags, so
/// the sentinel is loaded *before* the `test`/`cmp` it guards. rel8 would not
/// reach — the body is several hundred bytes.
///
/// Tagged `ELFOSABI_GNU` so the loader treats it as Linux-ABI.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]
pub fn build_linux_virtgpu_resource_test_elf() -> alloc::vec::Vec<u8> {
    use crate::drm::virtgpu_uapi as vg;
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // Geometry: 64x64x4 = 16 KiB, exactly one 16 KiB frame, so the reported
    // mappable size is unambiguous and the whole resource is one mmap.
    const W: u32 = 64;
    const H: u32 = 64;
    const STRIDE: u32 = W * 4;
    const BYTES: u32 = W * H * 4;
    /// `VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM`.
    const FMT: u32 = 1;
    /// Gallium `PIPE_TEXTURE_2D` — the target Linux's non-virgl guard requires.
    const PIPE_TEXTURE_2D: u32 = 2;
    const PATTERN_A: u32 = 0x1122_3344;
    const PATTERN_B: u32 = 0x5566_7788;

    // Stack scratch displacements (all < 128 → disp8 addressing).
    const S: u8 = 0; // reusable struct area (56 bytes)
    const S_MAP: u8 = 64; // drm_virtgpu_map
    const S_FD: u8 = 80;
    const S_RES: u8 = 88;
    const S_OFF: u8 = 96;
    const S_ADDR: u8 = 104;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    // Offsets of rel32 displacement slots that jump to `fail:`.
    let mut fail_sites: alloc::vec::Vec<usize> = alloc::vec::Vec::new();

    // --- emitters -----------------------------------------------------------
    /// `mov dword [rsp+d], imm32`
    fn mov_d(code: &mut alloc::vec::Vec<u8>, d: u8, imm: u32) {
        code.extend_from_slice(&[0xC7, 0x44, 0x24, d]);
        code.extend_from_slice(&imm.to_le_bytes());
    }
    /// `mov edi, imm32` — load the failure sentinel (does not disturb flags).
    fn sentinel(code: &mut alloc::vec::Vec<u8>, v: u32) {
        code.push(0xBF);
        code.extend_from_slice(&v.to_le_bytes());
    }
    /// Emit a two-byte-opcode rel32 jump and record its displacement slot.
    fn jcc(code: &mut alloc::vec::Vec<u8>, sites: &mut alloc::vec::Vec<usize>, op: u8) {
        code.extend_from_slice(&[0x0F, op]);
        sites.push(code.len());
        code.extend_from_slice(&[0u8; 4]);
    }
    /// `ioctl(fd, request, &[rsp+arg])` — leaves the result in rax.
    fn ioctl(code: &mut alloc::vec::Vec<u8>, request: u32, arg: u8) {
        code.extend_from_slice(&[0x48, 0x8B, 0x7C, 0x24, S_FD]); // mov rdi, [rsp+fd]
        code.push(0xBE); // mov esi, imm32 (zero-extends into rsi)
        code.extend_from_slice(&request.to_le_bytes());
        code.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, arg]); // lea rdx, [rsp+arg]
        code.extend_from_slice(&[0xB8, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,16; syscall
    }
    const JS: u8 = 0x88;
    const JNZ: u8 = 0x85;
    const JZ: u8 = 0x84;
    const TEST_RAX: [u8; 3] = [0x48, 0x85, 0xC0];

    // --- open("/dev/dri/renderD128", O_RDWR, 0) -----------------------------
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.extend_from_slice(&[0xBE, 0x02, 0x00, 0x00, 0x00]); // mov esi, 2 (O_RDWR)
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,2; syscall
    sentinel(&mut code, 0xE1);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JS);
    code.extend_from_slice(&[0x48, 0x81, 0xEC, 0x80, 0x00, 0x00, 0x00]); // sub rsp, 128
    code.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, S_FD]); // mov [rsp+fd], rax

    // --- RESOURCE_CREATE ----------------------------------------------------
    mov_d(&mut code, S, PIPE_TEXTURE_2D); // target
    mov_d(&mut code, S + 4, FMT); // format
    mov_d(&mut code, S + 8, 0); // bind
    mov_d(&mut code, S + 12, W); // width
    mov_d(&mut code, S + 16, H); // height
    mov_d(&mut code, S + 20, 1); // depth
    mov_d(&mut code, S + 24, 1); // array_size
    mov_d(&mut code, S + 28, 0); // last_level
    mov_d(&mut code, S + 32, 0); // nr_samples
    mov_d(&mut code, S + 36, 0); // flags
    mov_d(&mut code, S + 40, 0); // bo_handle (0 = allocate)
    mov_d(&mut code, S + 44, 0); // res_handle (out)
    mov_d(&mut code, S + 48, 0); // size (out)
    mov_d(&mut code, S + 52, 0); // stride (out)
    ioctl(&mut code, vg::DRM_IOCTL_VIRTGPU_RESOURCE_CREATE, S);
    sentinel(&mut code, 0xE2);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JNZ);

    // Save the returned handle (mov to eax zero-extends into rax).
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S + 44]); // mov eax, [rsp+res_handle]
    code.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, S_RES]); // mov [rsp+res], rax
    sentinel(&mut code, 0xE3);
    code.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
    jcc(&mut code, &mut fail_sites, JZ); // handle 0 is never valid
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S + 52]); // mov eax, [rsp+stride]
    sentinel(&mut code, 0xE4);
    code.push(0x3D); // cmp eax, imm32
    code.extend_from_slice(&STRIDE.to_le_bytes());
    jcc(&mut code, &mut fail_sites, JNZ);
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S + 48]); // mov eax, [rsp+size]
    sentinel(&mut code, 0xE5);
    code.push(0x3D);
    code.extend_from_slice(&16384u32.to_le_bytes());
    jcc(&mut code, &mut fail_sites, JNZ);

    // --- MAP ----------------------------------------------------------------
    code.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, S_MAP, 0, 0, 0, 0]); // qword offset = 0
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S_RES]); // mov eax, [rsp+res]
    code.extend_from_slice(&[0x89, 0x44, 0x24, S_MAP + 8]); // mov [rsp+map.handle], eax
    mov_d(&mut code, S_MAP + 12, 0); // pad
    ioctl(&mut code, vg::DRM_IOCTL_VIRTGPU_MAP, S_MAP);
    sentinel(&mut code, 0xE6);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JNZ);
    code.extend_from_slice(&[0x48, 0x8B, 0x44, 0x24, S_MAP]); // mov rax, [rsp+map.offset]
    code.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, S_OFF]); // mov [rsp+off], rax

    // --- mmap(NULL, 16384, PROT_READ|PROT_WRITE, MAP_SHARED, fd, off) -------
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xBE, 0x00, 0x40, 0x00, 0x00]); // mov esi, 16384
    code.extend_from_slice(&[0xBA, 0x03, 0x00, 0x00, 0x00]); // mov edx, 3
    code.extend_from_slice(&[0x41, 0xBA, 0x01, 0x00, 0x00, 0x00]); // mov r10d, 1 (MAP_SHARED)
    code.extend_from_slice(&[0x4C, 0x8B, 0x44, 0x24, S_FD]); // mov r8, [rsp+fd]
    code.extend_from_slice(&[0x4C, 0x8B, 0x4C, 0x24, S_OFF]); // mov r9, [rsp+off]
    code.extend_from_slice(&[0xB8, 0x09, 0x00, 0x00, 0x00, 0x0F, 0x05]); // mov eax,9; syscall
    sentinel(&mut code, 0xE7);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JS); // errors come back as small negatives
    code.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, S_ADDR]); // mov [rsp+addr], rax

    // --- write and read back through the mapping ----------------------------
    code.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, S_ADDR]); // mov rcx, [rsp+addr]
    code.extend_from_slice(&[0xC7, 0x01]); // mov dword [rcx], imm32
    code.extend_from_slice(&PATTERN_A.to_le_bytes());
    code.extend_from_slice(&[0x8B, 0x11]); // mov edx, [rcx]
    sentinel(&mut code, 0xE8);
    code.extend_from_slice(&[0x81, 0xFA]); // cmp edx, imm32
    code.extend_from_slice(&PATTERN_A.to_le_bytes());
    jcc(&mut code, &mut fail_sites, JNZ);
    // The last dword of the frame, so a short mapping is caught rather than
    // silently working for the first page.
    code.extend_from_slice(&[0xC7, 0x81]); // mov dword [rcx+disp32], imm32
    code.extend_from_slice(&(BYTES - 4).to_le_bytes());
    code.extend_from_slice(&PATTERN_B.to_le_bytes());
    code.extend_from_slice(&[0x8B, 0x91]); // mov edx, [rcx+disp32]
    code.extend_from_slice(&(BYTES - 4).to_le_bytes());
    sentinel(&mut code, 0xE9);
    code.extend_from_slice(&[0x81, 0xFA]);
    code.extend_from_slice(&PATTERN_B.to_le_bytes());
    jcc(&mut code, &mut fail_sites, JNZ);

    // --- TRANSFER_TO_HOST (full rect) ---------------------------------------
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S_RES]); // mov eax, [rsp+res]
    code.extend_from_slice(&[0x89, 0x04, 0x24]); // mov [rsp], eax (bo_handle)
    mov_d(&mut code, S + 4, 0); // box.x
    mov_d(&mut code, S + 8, 0); // box.y
    mov_d(&mut code, S + 12, 0); // box.z
    mov_d(&mut code, S + 16, W); // box.w
    mov_d(&mut code, S + 20, H); // box.h
    mov_d(&mut code, S + 24, 1); // box.d
    mov_d(&mut code, S + 28, 0); // level
    mov_d(&mut code, S + 32, 0); // offset
    mov_d(&mut code, S + 36, STRIDE); // stride — the honest packed value
    mov_d(&mut code, S + 40, 0); // layer_stride
    ioctl(&mut code, vg::DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST, S);
    sentinel(&mut code, 0xEA);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JNZ);

    // --- WAIT ---------------------------------------------------------------
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S_RES]);
    code.extend_from_slice(&[0x89, 0x04, 0x24]); // wait.handle
    mov_d(&mut code, S + 4, 0); // wait.flags
    ioctl(&mut code, vg::DRM_IOCTL_VIRTGPU_WAIT, S);
    sentinel(&mut code, 0xEB);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JNZ);

    // --- RESOURCE_INFO ------------------------------------------------------
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S_RES]);
    code.extend_from_slice(&[0x89, 0x04, 0x24]); // info.bo_handle
    mov_d(&mut code, S + 4, 0);
    mov_d(&mut code, S + 8, 0);
    mov_d(&mut code, S + 12, 0);
    ioctl(&mut code, vg::DRM_IOCTL_VIRTGPU_RESOURCE_INFO, S);
    sentinel(&mut code, 0xEC);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JNZ);
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S + 8]); // mov eax, [rsp+info.size]
    sentinel(&mut code, 0xED);
    code.push(0x3D);
    code.extend_from_slice(&16384u32.to_le_bytes());
    jcc(&mut code, &mut fail_sites, JNZ);

    // --- GEM_CLOSE (the destroy path) ---------------------------------------
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S_RES]);
    code.extend_from_slice(&[0x89, 0x04, 0x24]); // gem_close.handle
    mov_d(&mut code, S + 4, 0); // pad
    ioctl(&mut code, crate::drm::uapi::DRM_IOCTL_GEM_CLOSE, S);
    sentinel(&mut code, 0xEE);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JNZ);

    // --- the closed handle must no longer resolve ---------------------------
    code.extend_from_slice(&[0x8B, 0x44, 0x24, S_RES]);
    code.extend_from_slice(&[0x89, 0x04, 0x24]);
    mov_d(&mut code, S + 4, 0);
    mov_d(&mut code, S + 8, 0);
    mov_d(&mut code, S + 12, 0);
    ioctl(&mut code, vg::DRM_IOCTL_VIRTGPU_RESOURCE_INFO, S);
    sentinel(&mut code, 0xEF);
    code.extend_from_slice(&TEST_RAX);
    jcc(&mut code, &mut fail_sites, JZ); // success here means the handle leaked

    // --- ...but the mapping must survive the close --------------------------
    code.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, S_ADDR]); // mov rcx, [rsp+addr]
    code.extend_from_slice(&[0x8B, 0x11]); // mov edx, [rcx]
    sentinel(&mut code, 0xF0);
    code.extend_from_slice(&[0x81, 0xFA]);
    code.extend_from_slice(&PATTERN_A.to_le_bytes());
    jcc(&mut code, &mut fail_sites, JNZ);

    // exit(0)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // fail: exit(edi)
    let fail = code.len();
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);
    code.push(0xCC); // int3 — unreachable trap

    for site in &fail_sites {
        let disp = (fail as i64) - (*site as i64 + 4);
        code[*site..*site + 4].copy_from_slice(&(disp as i32).to_le_bytes());
    }

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
pub fn build_linux_fallocate_grow_test_elf(path_nul: &[u8], grow_len: u32) -> alloc::vec::Vec<u8> {
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

/// [`build_linux_exit_elf`]'s image, with a program that never ends by itself:
///
/// ```text
///   mov eax, 34          ; B8 22 00 00 00 -- Linux SYS_pause
///   syscall              ; 0F 05
///   jmp  -9              ; EB F7 -- and again, whatever pause answered
/// ```
///
/// For a self-test that must look at a spawned process before it can exit.
/// A process frees its memory as it exits (`pcb::release_address_space`), so a
/// program that exits at once could take its image away mid-look. Same
/// headers and segment as the exit program, so a layout fact proved of one
/// holds for the other. The rung stops it with `spawn::teardown_fixture`.
#[must_use]
pub fn build_linux_pause_elf() -> alloc::vec::Vec<u8> {
    let mut buf = build_linux_exit_elf(0);
    // The exit program's code: 16 bytes at file offset 120.
    const CODE: usize = 120;
    const LOOP: [u8; 9] = [0xB8, 0x22, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xEB, 0xF7];
    if let Some(code) = buf.get_mut(CODE..CODE.saturating_add(16)) {
        code.fill(0xCC);
        if let Some(head) = code.get_mut(..LOOP.len()) {
            head.copy_from_slice(&LOOP);
        }
    }
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
    let final_flags = if fexecve {
        0x1000u32 | flags_extra
    } else {
        flags_extra
    };
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

    let vaddr_of = |file_off: usize| -> u64 { load_vaddr + (file_off as u64 - code_offset) };
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

/// Build a **native-ABI** ring-3 program that performs the file-handle
/// enumeration attack and reports whether the kernel refused it.
///
/// # What this is testing
///
/// A native file handle is `NEXT_HANDLE.fetch_add(1)` from 1, so handle values
/// are a machine-wide counting sequence, and `OPEN_FILES` is one global table
/// keyed by that number. If the handle is treated as self-authorising, any
/// process can walk 1, 2, 3… and read files it was never granted. The gate
/// under test is `require_file_handle_owner` in `syscall::handlers`.
///
/// # Why ring 3, and why native rather than Linux ABI
///
/// The gate is a no-op in kernel context by design — `caller_pid()` is `None`
/// for a bare kernel task, which legitimately holds unregistered handles — so
/// a kernel-context test can only ever observe the bypass. It must also be
/// *native* ABI: the Linux `read(2)` path goes through its own fd table, not
/// through `sys_fs_read`, so a Linux-ABI probe never reaches the gate.
///
/// # Why a control probe is not optional here
///
/// A test that only asserts "the foreign handle was refused" passes just as
/// happily against a kernel that refuses *everything* — including a
/// regression that broke `SYS_FS_READ` outright, or a spawn that granted no
/// `File` capability. So the program first opens a file of its own and reads
/// a known byte from it. Only once that has succeeded is the refusal of the
/// foreign handle attributable to ownership.
///
/// # Exit codes
///
/// | Exit | Meaning |
/// |---|---|
/// | `0` | pass — own read worked, foreign read refused with `InvalidHandle` |
/// | `1` | **the vulnerability** — the foreign read *succeeded* |
/// | `2` | foreign read failed, but with some error other than `InvalidHandle` |
/// | `0xFB` | own read was *refused* (negative rax) — see below |
/// | `0xFC` | own read returned the wrong byte (fixture is not what we think) |
/// | `0xFD` | own read returned a non-negative count that was not 1 |
/// | `0xFE` | own open failed — no `File` capability, or a missing fixture |
///
/// `0xFB` is the one to read carefully: it means the kernel rejected a read
/// on a handle this process opened itself, which is either a harness fault
/// (a bad buffer pointer — the mistake that cost the first run of this test
/// a boot) or the ownership gate misfiring on a legitimately-owned handle.
/// The latter would be a real regression, and lumping it in with `0xFD`
/// would have hidden it behind a "short read".
///
/// `1` and `2` are kept apart on purpose: `1` is a security failure, while
/// `2` is most likely the gate returning the wrong errno, which is a
/// correctness bug of a different kind (and an information leak — see
/// `require_file_handle_owner` on why the answer must be `InvalidHandle`).
///
/// `path_nul` must be NUL-terminated. `foreign_handle` is a handle the
/// spawning kernel context opened and did **not** register to this process.
#[must_use]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn build_native_handle_ownership_test_elf(
    path_nul: &[u8],
    path_len: u32,
    foreign_handle: u64,
    expect_byte: u8,
) -> alloc::vec::Vec<u8> {
    const SYS_EXIT: u32 = 1;
    const SYS_FS_OPEN: u32 = 610;
    const SYS_FS_READ: u32 = 612;
    const OPEN_READ: u32 = 1 << 0;
    /// `KernelError::InvalidHandle` as it arrives in `rax`.
    const INVALID_HANDLE: i32 = -505;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    /// `mov edi, imm32; mov eax, SYS_EXIT; syscall` — 12 bytes, fixed width,
    /// which is what lets the `jns`/`je` above each one use a known rel8.
    fn exit_with(code: &mut alloc::vec::Vec<u8>, v: u32) {
        code.push(0xBF);
        code.extend_from_slice(&v.to_le_bytes());
        code.extend_from_slice(&[0xB8]);
        code.extend_from_slice(&SYS_EXIT.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]);
    }

    // --- 0. claim a byte of stack to read into --------------------------
    // Entry `rsp` is the stack *top*, which is exclusive: the first mapped
    // byte is below it.  A Linux-ABI binary never notices, because
    // `build_linux_sysv_stack` pushes argv/envp and hands control over with
    // rsp already moved down (0x7fffffff0000 -> 0x7ffffffefec0 in the boot
    // log).  A native-ABI binary gets no such construction and starts
    // exactly at the top, so `mov rsi, rsp` would hand the kernel a
    // one-past-the-end pointer.  That does not fault -- `sys_fs_read`
    // validates the user buffer and returns `InvalidAddress` -- so the
    // symptom is a read that returns -101 instead of 1, i.e. an
    // indistinguishable-looking 0xFD "short read".  Cost a boot to find;
    // hence the comment rather than a bare `sub`.
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x20]); // sub rsp, 32

    // --- 1. open our own file ------------------------------------------
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, &path
    let path_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    code.push(0xBE); // mov esi, path_len
    code.extend_from_slice(&path_len.to_le_bytes());
    code.push(0xBA); // mov edx, OpenFlags::READ
    code.extend_from_slice(&OPEN_READ.to_le_bytes());
    code.push(0xB8); // mov eax, SYS_FS_OPEN
    code.extend_from_slice(&SYS_FS_OPEN.to_le_bytes());
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x79, 0x00]); // jns .have_own
    let jns_open = code.len() - 1;
    exit_with(&mut code, 0xFE);
    let have_own = code.len();
    code[jns_open] = ((have_own as isize) - (jns_open as isize + 1)) as u8;

    // --- 2. read one byte through it (the control) ---------------------
    code.extend_from_slice(&[0x48, 0x89, 0xC7]); // mov rdi, rax  (own handle)
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp
    code.push(0xBA); // mov edx, 1
    code.extend_from_slice(&1u32.to_le_bytes());
    code.push(0xB8); // mov eax, SYS_FS_READ
    code.extend_from_slice(&SYS_FS_READ.to_le_bytes());
    code.extend_from_slice(&[0x0F, 0x05]); // syscall

    // Split "the read was refused" from "the read returned the wrong count"
    // before comparing to 1.  Both are `rax != 1`, but they mean opposite
    // things: a negative rax is the kernel rejecting the call (a bad buffer
    // pointer, a missing capability, the ownership gate itself misfiring on
    // a handle we *do* own), while a non-negative one that is not 1 is a
    // genuine short read.  Folding them together is what made the first
    // run of this test report 0xFD for what was actually `InvalidAddress`.
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x79, 0x00]); // jns .nonneg
    let jns_read = code.len() - 1;
    exit_with(&mut code, 0xFB);
    let nonneg = code.len();
    code[jns_read] = ((nonneg as isize) - (jns_read as isize + 1)) as u8;

    code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x01]); // cmp rax, 1
    code.extend_from_slice(&[0x74, 0x00]); // je .read_ok
    let je_read = code.len() - 1;
    exit_with(&mut code, 0xFD);
    let read_ok = code.len();
    code[je_read] = ((read_ok as isize) - (je_read as isize + 1)) as u8;

    // The byte must be the one staged, or the "control succeeded" claim is
    // about some other file and proves nothing about this one.
    code.extend_from_slice(&[0x80, 0x3C, 0x24]); // cmp byte [rsp], imm8
    code.push(expect_byte);
    code.extend_from_slice(&[0x74, 0x00]); // je .byte_ok
    let je_byte = code.len() - 1;
    exit_with(&mut code, 0xFC);
    let byte_ok = code.len();
    code[je_byte] = ((byte_ok as isize) - (je_byte as isize + 1)) as u8;

    // --- 3. the attack: read through a handle we do not own ------------
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, foreign_handle
    code.extend_from_slice(&foreign_handle.to_le_bytes());
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp
    code.push(0xBA); // mov edx, 1
    code.extend_from_slice(&1u32.to_le_bytes());
    code.push(0xB8); // mov eax, SYS_FS_READ
    code.extend_from_slice(&SYS_FS_READ.to_le_bytes());
    code.extend_from_slice(&[0x0F, 0x05]); // syscall

    // rax >= 0 means the read was allowed -- the vulnerability.
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x78, 0x00]); // js .refused
    let js_attack = code.len() - 1;
    exit_with(&mut code, 1);
    let refused = code.len();
    code[js_attack] = ((refused as isize) - (js_attack as isize + 1)) as u8;

    // Refused -- but with the right errno?
    code.extend_from_slice(&[0x48, 0x3D]); // cmp rax, imm32 (sign-extended)
    code.extend_from_slice(&INVALID_HANDLE.to_le_bytes());
    code.extend_from_slice(&[0x74, 0x00]); // je .pass
    let je_errno = code.len() - 1;
    exit_with(&mut code, 2);
    let pass = code.len();
    code[je_errno] = ((pass as isize) - (je_errno as isize + 1)) as u8;

    exit_with(&mut code, 0);
    code.push(0xCC); // int3 — unreachable

    // --- data ----------------------------------------------------------
    let code_len = code.len();
    let path_off = code_offset as usize + code_len;
    let file_size = path_off + path_nul.len();

    let vaddr_of = |fo: usize| load_vaddr + (fo as u64 - code_offset);
    let pv = vaddr_of(path_off).to_le_bytes();
    code[path_imm..path_imm + 8].copy_from_slice(&pv);

    let mut buf = alloc::vec![0u8; file_size];
    buf[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    buf[4] = 2; // ELFCLASS64
    buf[5] = 1; // ELFDATA2LSB
    buf[6] = EV_CURRENT;
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

    let ph = phdr_offset as usize;
    let seg_len = (file_size - code_offset as usize) as u64;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_W | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len);
    write_u64(&mut buf, ph + 40, seg_len);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    buf[path_off..path_off + path_nul.len()].copy_from_slice(path_nul);

    buf
}

/// Build a **Linux-ABI** test ELF that exercises `openat2(2)` — in particular
/// the `RESOLVE_BENEATH` containment path — end-to-end from ring 3.
///
/// # Why this cannot be a kernel-context test
///
/// The existing `self_test_openat_dirfd` (in `syscall/linux.rs`) drives
/// `dispatch_linux` directly from kernel context, which has **no fd table**.
/// Every real (non-`AT_FDCWD`) `dirfd` therefore returns `EBADF` before
/// [`dirfd_to_guest_dir`](crate::syscall::linux) does any of its actual work.
/// The translation that turns a descriptor into the containment *base* —
/// `handle_path` → `stat_resolved` → `unjail_path_for` — is exactly the part
/// no in-kernel test can reach, and exactly the part lane B asked to see
/// pinned before forwarding libc's `openat2` (see
/// `requests/b-a-yes-forward-openat2-and-here-is-the-shape-we-want.md`).
///
/// # Why the exit code carries a *byte of file content*
///
/// The failure mode that matters here is not "the call errored." It is that a
/// wrong base still **succeeds**: the walk is confined, the descriptor is
/// valid, and the result looks exactly like working containment — it is just
/// confined under the wrong directory. A test that asserts only success
/// cannot see that.
///
/// So the program exits with the *first byte it reads through the descriptor*,
/// and the caller stages a distinct byte in each directory a wrong base could
/// plausibly resolve to. Every wrong answer becomes a different exit code
/// rather than an error, which is what makes the test able to tell "contained
/// under the right directory" from "contained under some directory."
///
/// Encoding of the exit code:
///
/// | Exit | Meaning |
/// |---|---|
/// | `0xFE` | the initial `open(dir)` failed — fixture/staging problem, not a verdict |
/// | `0xFD` | `openat2` succeeded but `read` did not return 1 byte |
/// | `1..=133` | `openat2` failed; the value is the raw errno (e.g. 18 = `EXDEV`) |
/// | `>= 0xC8` | `openat2` succeeded; the value is the byte read (sentinels start at 200, above every errno) |
///
/// # Code
///
/// ```text
///   ; when dir_nul is non-empty: obtain a real dirfd
///   movabs rdi, &dir          ; 48 BF <8>
///   mov    esi, 0x10000       ; BE ..      O_RDONLY|O_DIRECTORY
///   xor    edx, edx           ; 31 D2
///   mov    eax, 2             ; B8 ..      SYS_open
///   syscall                   ; 0F 05
///   test   rax, rax           ; 48 85 C0
///   jns    .have_fd           ; 79 <rel8>   skips the 12-byte block below
///   exit(0xFE)                ; inline, so the branch distance is local
/// .have_fd:
///   mov    rdi, rax           ; 48 89 C7   dirfd
///   ; when dir_nul is empty, the above is replaced by:
///   ;   mov rdi, -100         ; 48 C7 C7 9C FF FF FF   AT_FDCWD
///
///   movabs rsi, &rel          ; 48 BE <8>
///   movabs rdx, &how          ; 48 BA <8>  struct open_how
///   mov    r10d, 24           ; 41 BA ..   sizeof(open_how)
///   mov    eax, 437           ; B8 ..      SYS_openat2
///   syscall                   ; 0F 05
///   test   rax, rax           ; 48 85 C0
///   jns    .opened            ; 79 <rel8>
///   neg    rax                ; 48 F7 D8   exit(errno)
///   mov    rdi, rax           ; 48 89 C7
///   exit(rdi)
/// .opened:
///   mov    rdi, rax           ; 48 89 C7   fd
///   mov    rsi, rsp           ; 48 89 E6   scratch buffer on the stack
///   mov    edx, 1             ; BA ..
///   xor    eax, eax           ; 31 C0      SYS_read
///   syscall                   ; 0F 05
///   cmp    rax, 1             ; 48 83 F8 01
///   je     .read_ok           ; 74 <rel8>
///   exit(0xFD)
/// .read_ok:
///   movzx  edi, byte [rsp]    ; 0F B6 3C 24
///   exit(rdi)
///   int3                      ; CC — unreachable
/// ```
///
/// `dir_nul` and `rel_nul` must be NUL-terminated. An empty `dir_nul` selects
/// the `AT_FDCWD` form, which is how the process-cwd branch of
/// `sys_openat_beneath` is reached.
#[must_use]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn build_linux_openat2_test_elf(
    dir_nul: &[u8],
    rel_nul: &[u8],
    resolve: u64,
) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 (ehdr) + 56 (one phdr)
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let use_dirfd = !dir_nul.is_empty();

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let dir_imm: usize;

    if use_dirfd {
        // movabs rdi, &dir
        code.extend_from_slice(&[0x48, 0xBF]);
        dir_imm = code.len();
        code.extend_from_slice(&[0u8; 8]);
        // mov esi, O_RDONLY|O_DIRECTORY (0o200000)
        code.extend_from_slice(&[0xBE]);
        code.extend_from_slice(&0o200_000u32.to_le_bytes());
        // xor edx, edx  (mode)
        code.extend_from_slice(&[0x31, 0xD2]);
        // mov eax, 2 (SYS_open); syscall
        code.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05]);
        // test rax, rax; jns .have_fd
        code.extend_from_slice(&[0x48, 0x85, 0xC0]);
        code.extend_from_slice(&[0x79, 0x00]);
        let jns_open_rel = code.len() - 1;
        // open_fail: exit(0xFE).  Placed inline immediately after the branch
        // rather than at the end of the function, so the jump distance is a
        // fixed, locally-visible 12 bytes instead of a label that later edits
        // could silently move.
        code.extend_from_slice(&[0xBF, 0xFE, 0x00, 0x00, 0x00]); // mov edi, 0xFE
        code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // exit
        // .have_fd:
        let have_fd = code.len();
        code[jns_open_rel] = ((have_fd as isize) - (jns_open_rel as isize + 1)) as u8;
        // mov rdi, rax  (dirfd)
        code.extend_from_slice(&[0x48, 0x89, 0xC7]);
    } else {
        dir_imm = usize::MAX;
        // mov rdi, -100 (AT_FDCWD), sign-extended imm32
        code.extend_from_slice(&[0x48, 0xC7, 0xC7, 0x9C, 0xFF, 0xFF, 0xFF]);
    }

    // movabs rsi, &rel
    code.extend_from_slice(&[0x48, 0xBE]);
    let rel_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    // movabs rdx, &how
    code.extend_from_slice(&[0x48, 0xBA]);
    let how_imm = code.len();
    code.extend_from_slice(&[0u8; 8]);
    // mov r10d, 24 (sizeof struct open_how)
    code.extend_from_slice(&[0x41, 0xBA, 0x18, 0x00, 0x00, 0x00]);
    // mov eax, 437 (SYS_openat2); syscall
    code.extend_from_slice(&[0xB8, 0xB5, 0x01, 0x00, 0x00, 0x0F, 0x05]);
    // test rax, rax; jns .opened
    code.extend_from_slice(&[0x48, 0x85, 0xC0]);
    code.extend_from_slice(&[0x79, 0x00]);
    let jns_openat2_rel = code.len() - 1;
    // error path: exit(-rax) — the raw errno, so the caller sees *which* refusal.
    code.extend_from_slice(&[0x48, 0xF7, 0xD8]); // neg rax
    code.extend_from_slice(&[0x48, 0x89, 0xC7]); // mov rdi, rax
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // exit

    // .opened:
    let opened = code.len();
    code.extend_from_slice(&[0x48, 0x89, 0xC7]); // mov rdi, rax  (fd)
    code.extend_from_slice(&[0x48, 0x89, 0xE6]); // mov rsi, rsp  (scratch)
    code.extend_from_slice(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
    code.extend_from_slice(&[0x31, 0xC0, 0x0F, 0x05]); // xor eax,eax (SYS_read); syscall
    code.extend_from_slice(&[0x48, 0x83, 0xF8, 0x01]); // cmp rax, 1
    code.extend_from_slice(&[0x74, 0x00]); // je .read_ok
    let je_read_rel = code.len() - 1;
    // read_fail: exit(0xFD)
    code.extend_from_slice(&[0xBF, 0xFD, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]);

    // .read_ok: exit(buf[0]) — the byte that identifies *which* file was opened.
    let read_ok = code.len();
    code.extend_from_slice(&[0x0F, 0xB6, 0x3C, 0x24]); // movzx edi, byte [rsp]
    code.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05]); // exit

    code.push(0xCC); // int3 — unreachable trap

    // Patch the two remaining forward rel8 jumps.  The displacement is
    // measured from the byte *after* the displacement itself.  (The dirfd
    // form's `jns .have_fd` was already patched inline above, where its
    // target was in scope.)
    let jns_openat2_disp = (opened as isize) - (jns_openat2_rel as isize + 1);
    let je_read_disp = (read_ok as isize) - (je_read_rel as isize + 1);
    code[jns_openat2_rel] = jns_openat2_disp as u8;
    code[je_read_rel] = je_read_disp as u8;

    // --- Data layout (same PT_LOAD, after the code) ---
    let code_len = code.len();
    let data_base = code_offset as usize + code_len;
    let dir_off = data_base;
    let dir_end = dir_off + dir_nul.len();
    let rel_off = dir_end;
    let rel_end = rel_off + rel_nul.len();
    // 8-align `struct open_how` — the kernel copies it with copy_from_user,
    // which does not require alignment, but an aligned struct is what a real
    // compiler would emit and keeps the layout honest.
    let how_off = (rel_end + 7) & !7usize;
    let file_size = how_off + 24;

    let vaddr_of = |fo: usize| -> u64 { load_vaddr + (fo as u64 - code_offset) };
    if use_dirfd {
        let dir_vaddr = vaddr_of(dir_off);
        code[dir_imm..dir_imm + 8].copy_from_slice(&dir_vaddr.to_le_bytes());
    }
    let rel_vaddr = vaddr_of(rel_off);
    let how_vaddr = vaddr_of(how_off);
    code[rel_imm..rel_imm + 8].copy_from_slice(&rel_vaddr.to_le_bytes());
    code[how_imm..how_imm + 8].copy_from_slice(&how_vaddr.to_le_bytes());

    // --- Build the file image ---
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
    write_u32(&mut buf, ph + 4, PF_R | PF_W | PF_X);
    write_u64(&mut buf, ph + 8, code_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, seg_len as u64);
    write_u64(&mut buf, ph + 40, seg_len as u64);
    write_u64(&mut buf, ph + 48, 0x1000);

    buf[code_offset as usize..code_offset as usize + code_len].copy_from_slice(&code);
    if use_dirfd {
        buf[dir_off..dir_end].copy_from_slice(dir_nul);
    }
    buf[rel_off..rel_end].copy_from_slice(rel_nul);
    // struct open_how { __u64 flags; __u64 mode; __u64 resolve; }
    // flags = O_RDONLY (0), mode = 0 (no O_CREAT), resolve = caller's.
    write_u64(&mut buf, how_off, 0);
    write_u64(&mut buf, how_off + 8, 0);
    write_u64(&mut buf, how_off + 16, resolve);

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
        0xC6, 0x83, 0x00, 0x40, 0x00, 0x00,
        0x00, // mov byte[rbx+0x4000],sentinel @38 (imm @44)
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
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x14, // mov qword [rsp+24],0x14000000 @29
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, // mov rax, restorer_addr      @38 (imm@40)
        0x48, 0x89, 0x44, 0x24, 0x20, //       mov [rsp+32], rax           @48
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00,
        0x00, // mov qword [rsp+40],0 (mask)   @53
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
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00,
        0x00, // mov qword [rsp+40],0 (mask)  @46
        0xBF, 0x0A, 0x00, 0x00, 0x00, //             mov edi, 10  (SIGUSR1)   @55
        0x48, 0x8D, 0x74, 0x24, 0x10, //             lea rsi, [rsp+16] (&act) @60
        0x31, 0xD2, //                               xor edx, edx (oact=NULL) @65
        0x41, 0xBA, 0x08, 0x00, 0x00, 0x00, //       mov r10d, 8  (sigsetsz)  @67
        0xB8, 0x0D, 0x00, 0x00, 0x00, //             mov eax, 13 (rt_sigaction)@73
        0x0F, 0x05, //                               syscall                  @78
        0x48, 0xC7, 0x44, 0x24, 0x30, 0x00, 0x08, 0x00,
        0x00, // mov qword [rsp+48],0x800 (SIGUSR2) @80
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
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00,
        0x00, // mov qword [rsp+40],0 (mask)  @46
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
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00,
        0x00, // mov qword [rsp+40],0 (mask)  @46
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
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00,
        0x00, // mov qword [rsp+40],0 (mask)  @46
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
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00,
        0x00, // mov qword [rsp+40],0 (mask)  @46
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
        0x48, 0xC7, 0x44, 0x24, 0x18, 0x00, 0x00, 0x00,
        0x04, // mov qword [rsp+24],0x04000000 @22
        0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, //       mov rax, restorer_addr   @31 (imm@33)
        0x48, 0x89, 0x44, 0x24, 0x20, //             mov [rsp+32], rax        @41
        0x48, 0xC7, 0x44, 0x24, 0x28, 0x00, 0x00, 0x00,
        0x00, // mov qword [rsp+40],0 (mask)  @46
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
    buf[cs + code_instructions_len..cs + code_instructions_len + msg_len].copy_from_slice(msg);

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
    // int3. The doc above has always promised this and the code never
    // emitted it, so a FAILED exec fell through into the buffer's zero
    // fill -- `00 00` is `add [rax], al` -- and faulted somewhere with no
    // relation to the defect. The 2026-09-21 boot showed exactly that: a
    // #GP at base+0x16, one byte past this syscall. A breakpoint stops
    // where it broke.
    buf[c + 22] = 0xCC;

    // int3 at c+22 (already filled by safety net above)

    buf
}

/// Build a **native-ABI** `ET_EXEC` test ELF that exercises the *argument
/// validation* of `SYS_PROCESS_SPAWN_EX2` (559) from ring 3.
///
/// # Why this has to be a ring-3 program
///
/// `sys_process_spawn_ex2` is reachable in exactly one way — a userspace
/// pointer to a `SpawnEx2Args` — and almost everything it does before it
/// reaches shared code is *about* that pointer: reading `struct_size` out of
/// the caller's memory, deciding how many bytes to copy, checking that the
/// tail it does not understand is zero, dispatching on `cap_mode`, and
/// decoding a `CapEntryInfo` array.  The kernel-side `spawn::self_test`
/// covers the delegation *policy* by calling `spawn_process_with_caps`
/// directly, and `ex2_copy_plan` covers the size arithmetic exhaustively as
/// pure math — but neither of those crosses the user/kernel boundary, so the
/// whole copy-in path is invisible to them.  This program is the only thing
/// that runs it.
///
/// # Shape
///
/// The program reserves 256 bytes of stack, zeroes them (so every field the
/// probes do not set is `0`, not whatever the loader happened to leave), and
/// then runs a series of probes.  Each probe mutates one or two fields, calls
/// syscall 559 with `rdi = rsp`, and compares `rax` against the expected
/// error; on a mismatch it exits immediately with a probe-specific code, so a
/// failure names the exact rule that broke rather than just "the spawn was
/// wrong".  A clean `exit(0)` means every probe agreed.
///
/// `elf_ptr` is fixed at `0x0000_0030_0000_0000` — a canonical *user* address
/// that is deliberately not mapped.  That is what makes the test able to
/// distinguish two outcomes: a probe **rejected by the argument gate**
/// returns `InvalidArgument` (-3), while a probe that **passes the gate**
/// gets as far as reading the ELF image and returns `InvalidAddress` (-101).
/// So `-101` means "this input was accepted", and the probes are arranged in
/// accept/reject pairs differing in one field, which pins down *which* check
/// fired instead of merely observing that something did.
///
/// # Probes
///
/// | Code | Input | Expect | Proves |
/// |---|---|---|---|
/// | `0x11` | `struct_size = 0` | `-3` | zero size is not "legacy" |
/// | `0x12` | `struct_size = 96` | `-3` | below `SPAWN_EX2_MIN_SIZE` |
/// | `0x13` | `struct_size = 108` | `-3` | not a multiple of 8 |
/// | `0x14` | `struct_size = 4104` | `-3` | above `SPAWN_EX2_MAX_SIZE` |
/// | `0x15` | `struct_size = 104` | `-101` | a short struct is legal, and the missing tail is zero-filled — an unzeroed `cap_mode` would have been rejected |
/// | `0x16` | `struct_size = 192` | `-101` | the exact current size is accepted |
/// | `0x17` | `struct_size = 200`, tail `= 0` | `-101` | a *newer* caller with an all-zero tail is accepted |
/// | `0x18` | `struct_size = 200`, tail `= 1` | `-3` | a non-zero unknown field is refused, never ignored |
/// | `0x19` | `cap_mode = 2` | `-3` | an unknown mode is not clamped to a known one |
/// | `0x1A` | `cap_mode = 1`, `cap_ptr = 0`, `cap_count = 3` | `-3` | a null array with a count is a caller bug, not "no capabilities" |
/// | `0x1B` | `cap_mode = 1`, `cap_ptr = 0`, `cap_count = 0` | `-101` | …but the two spellings of "nothing" agree |
/// | `0x1C` | `cap_ptr` unmapped, `cap_count = 1` | `-101` | the array itself is bounced through `read_user_items` |
/// | `0x1D` | entry `resource_type = 9999` | `-3` | an undefined resource type is refused |
/// | `0x1E` | entry `_reserved[0] = 1` | `-3` | the reserved field is validated, not skipped |
/// | `0x1F` | a well-formed entry | `-101` | …and a clean entry passes the decode |
/// | `0x20` | `cap_mode = 0` with a junk `cap_ptr`/`cap_count` | `-101` | inherit-all ignores the array entirely |
/// | `0x21` | `cwd_ptr` unmapped, `cwd_len = 0` | `-101` | a zero length inherits the parent's directory and the pointer is never read |
/// | `0x22` | `cwd_ptr = 0`, `cwd_len = 1` | `-3` | a null path with a length is a caller bug, not "inherit" |
/// | `0x23` | `cwd_ptr` unmapped, `cwd_len = 4096` | `-3` | over `CWD_MAX_LEN` is refused *before* the pointer is read (a read first would say -101) |
/// | `0x24` | `cwd` = `"a"` | `-3` | a relative directory is refused, not resolved or ignored |
/// | `0x25` | `cwd` = `"/"` | `-101` | …and a canonical one passes the gate |
/// | `0x26` | `pgid_mode = 2` | `-3` | an unknown group mode is refused |
/// | `0x27` | `pgid = 5`, `pgid_mode = 0` | `-3` | a group without its mode is refused, not ignored |
/// | `0x28` | `pgid_mode = 1`, `pgid = 2^31` | `-3` | a group that is not a `pid_t` is refused |
/// | `0x29` | `pgid_mode = 1`, `pgid = 0` | `-101` | …and "a new group" passes the gate |
/// | `0x2A` | `sigmask_set = 2` | `-3` | an unknown mask flag is refused |
/// | `0x2B` | `sigmask = 1`, `sigmask_set = 0` | `-3` | a mask without its flag is refused |
/// | `0x2C` | `sigmask_set = 1`, `sigmask = !0` | `-101` | any mask passes (`SIGKILL`/`SIGSTOP` are dropped later, not refused) |
/// | `0x2D` | `setsid = 2` | `-3` | an unknown session flag is refused |
/// | `0x2E` | `setsid = 1` | `-101` | …and a new session passes the gate |
/// | `0x2F` | `sigdefault = !0` | `-101` | `sigdefault` takes any bits |
/// | `0x30` | `struct_size = 144`, `pgid_mode = 2` beyond it | `-101` | a caller older than the six fields has them read as zero, not from its memory |
///
/// The size probes (`0x16`-`0x18`) moved when `cwd_ptr`/`cwd_len` made the
/// struct 144 bytes (§960, 2026-09-25): aimed at the old 128, "the unknown
/// tail" was `cwd_ptr`, a known field that is accepted, and `0x18` failed.
/// They moved again when the six `posix_spawnattr_t` fields made it 192
/// (2026-10-01), and the scratch entry and path moved past the struct with
/// them -- left at 160 and 200, the entry would have been read as
/// `sigmask_set`.
///
/// # Deliberately out of scope
///
/// A *successful* subset spawn is not attempted here.  `spawn_ex_common`
/// reads the ELF image out of user memory before `spawn_process_inner` runs
/// the delegation check, so with an unmapped `elf_ptr` the delegation verdict
/// can never be reached — and giving this program a real nested ELF would
/// test the loader, not the ABI.  The delegation rules (narrowing allowed,
/// widening and unheld resources refused, whole spawn aborted) are covered by
/// `spawn::self_test` → `test_spawn_capability_subset`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_spawn_ex2_abi_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 + 56
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    /// A canonical user address that is never mapped in a fresh process, so
    /// touching it yields `InvalidAddress` rather than a fault or a success.
    const UNMAPPED: u64 = 0x0000_0030_0000_0000;

    /// `KernelError::InvalidArgument` as it arrives in `rax`.
    const EINVAL: i32 = -3;
    /// `KernelError::InvalidAddress` as it arrives in `rax`.
    const EFAULT: i32 = -101;

    // `SpawnEx2Args` field displacements from `rsp`.
    const F_SIZE: u32 = 0;
    const F_ELF_PTR: u32 = 8;
    const F_ELF_LEN: u32 = 16;
    const F_CAP_MODE: u32 = 104;
    const F_CAP_PTR: u32 = 112;
    const F_CAP_COUNT: u32 = 120;
    /// `cwd_ptr` and `cwd_len`, the two fields the working-directory change
    /// (design-decisions.md §960) added -- which made the struct 144 bytes.
    const F_CWD_PTR: u32 = 128;
    const F_CWD_LEN: u32 = 136;
    /// The six `posix_spawnattr_t` fields, which made the struct 192 bytes.
    const F_PGID_MODE: u32 = 144;
    const F_PGID: u32 = 152;
    const F_SIGMASK_SET: u32 = 160;
    const F_SIGMASK: u32 = 168;
    const F_SIGDEFAULT: u32 = 176;
    const F_SETSID: u32 = 184;
    /// This kernel's `size_of::<SpawnEx2Args>()`.
    const CURRENT: u64 = 192;
    /// The first byte past the struct — the "unknown tail" a newer caller
    /// would have written a new field into.  It was 128 until `cwd_ptr` took
    /// that slot, and 144 until the six attribute fields took theirs; a probe
    /// left aimed at a known field writes something that is accepted, and
    /// 0x18 fails on every boot until it moves.
    const F_TAIL: u32 = 192;
    /// A scratch `CapEntryInfo`, past the struct: `resource_type` and
    /// `_reserved[3]` share this qword, then `rights`, then `resource_id`
    /// (both left zero).
    const F_ENTRY: u32 = 208;
    /// A scratch path for the `cwd` probes: one byte, then zeroes.
    const F_PATH: u32 = 240;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // --- emitters -----------------------------------------------------------
    /// `mov rax, imm` — the short `mov eax` form when the value fits, since it
    /// zero-extends and every value here is unsigned.
    fn mov_rax(code: &mut alloc::vec::Vec<u8>, v: u64) {
        if v <= u64::from(u32::MAX) {
            code.push(0xB8);
            code.extend_from_slice(&(v as u32).to_le_bytes());
        } else {
            code.extend_from_slice(&[0x48, 0xB8]);
            code.extend_from_slice(&v.to_le_bytes());
        }
    }
    /// `mov [rsp+disp32], rax`.  disp32 throughout: the scratch entry sits
    /// past +127, and one uniform form is easier to check by eye than a mix.
    fn store(code: &mut alloc::vec::Vec<u8>, off: u32) {
        code.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]);
        code.extend_from_slice(&off.to_le_bytes());
    }
    /// `mov qword [rsp+off], imm`.
    fn set(code: &mut alloc::vec::Vec<u8>, off: u32, v: u64) {
        mov_rax(code, v);
        store(code, off);
    }
    /// `lea rax, [rsp+disp32]`.
    fn lea_rax(code: &mut alloc::vec::Vec<u8>, off: u32) {
        code.extend_from_slice(&[0x48, 0x8D, 0x84, 0x24]);
        code.extend_from_slice(&off.to_le_bytes());
    }
    /// `spawn_ex2(rsp)`; if `rax != expect`, `exit(fail)` on the spot.
    ///
    /// `rdi` is reloaded from `rsp` every time rather than kept in a
    /// callee-saved register, so a probe depends on nothing surviving the
    /// syscall except `rsp` itself.
    fn probe(code: &mut alloc::vec::Vec<u8>, expect: i32, fail: u32) {
        code.push(0xB8); // mov eax, 559
        code.extend_from_slice(&559u32.to_le_bytes());
        code.extend_from_slice(&[0x48, 0x89, 0xE7]); // mov rdi, rsp
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.extend_from_slice(&[0x48, 0x3D]); // cmp rax, imm32 (sign-extended)
        code.extend_from_slice(&(expect as u32).to_le_bytes());
        code.extend_from_slice(&[0x74, 0x0D]); // je +13 — over the exit block
        code.push(0xBF); // mov edi, <fail>
        code.extend_from_slice(&fail.to_le_bytes());
        code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.push(0xCC); // int3 — exit does not return
    }

    // --- prologue: 256 zeroed bytes of stack --------------------------------
    // Zeroing matters as much as reserving: every probe below sets only the
    // fields it is about, and reads a guaranteed-zero value for the rest.
    code.extend_from_slice(&[0x48, 0x81, 0xEC]); // sub rsp, imm32
    code.extend_from_slice(&256u32.to_le_bytes());
    code.push(0xFC); // cld — `rep stosq` must count upward
    code.extend_from_slice(&[0x48, 0x89, 0xE7]); // mov rdi, rsp
    code.extend_from_slice(&[0x31, 0xC0]); // xor eax, eax
    code.push(0xB9); // mov ecx, 32 (× 8 bytes = 256)
    code.extend_from_slice(&32u32.to_le_bytes());
    code.extend_from_slice(&[0xF3, 0x48, 0xAB]); // rep stosq

    // A non-zero `elf_len` is required before the ELF pointer is even looked
    // at, so set both once: they are the same for every probe.
    set(&mut code, F_ELF_PTR, UNMAPPED);
    set(&mut code, F_ELF_LEN, 16);

    // --- the size gate ------------------------------------------------------
    set(&mut code, F_SIZE, 0);
    probe(&mut code, EINVAL, 0x11);
    set(&mut code, F_SIZE, 96); // SPAWN_EX2_MIN_SIZE - 8
    probe(&mut code, EINVAL, 0x12);
    set(&mut code, F_SIZE, 108); // not a multiple of 8
    probe(&mut code, EINVAL, 0x13);
    set(&mut code, F_SIZE, 4104); // SPAWN_EX2_MAX_SIZE + 8
    probe(&mut code, EINVAL, 0x14);

    // 104 is the shortest legal size: a version-1 caller.  Reaching the ELF
    // read proves both that it was accepted *and* that the fields it stopped
    // short of were zero-filled — `cap_mode` is one of them, and any value but
    // zero there is rejected.
    set(&mut code, F_SIZE, 104);
    probe(&mut code, EFAULT, 0x15);
    set(&mut code, F_SIZE, CURRENT);
    probe(&mut code, EFAULT, 0x16);

    // --- the unknown tail ---------------------------------------------------
    set(&mut code, F_SIZE, CURRENT + 8);
    set(&mut code, F_TAIL, 0);
    probe(&mut code, EFAULT, 0x17);
    set(&mut code, F_TAIL, 1);
    probe(&mut code, EINVAL, 0x18);
    set(&mut code, F_TAIL, 0);
    set(&mut code, F_SIZE, CURRENT);

    // --- cap_mode dispatch --------------------------------------------------
    set(&mut code, F_CAP_MODE, 2);
    probe(&mut code, EINVAL, 0x19);

    // --- the capability array ----------------------------------------------
    set(&mut code, F_CAP_MODE, 1);
    set(&mut code, F_CAP_PTR, 0);
    set(&mut code, F_CAP_COUNT, 3);
    probe(&mut code, EINVAL, 0x1A);
    set(&mut code, F_CAP_COUNT, 0);
    probe(&mut code, EFAULT, 0x1B);
    set(&mut code, F_CAP_PTR, UNMAPPED);
    set(&mut code, F_CAP_COUNT, 1);
    probe(&mut code, EFAULT, 0x1C);

    // Point at the scratch entry for the remaining three.
    lea_rax(&mut code, F_ENTRY);
    store(&mut code, F_CAP_PTR);
    set(&mut code, F_ENTRY, 9999); // resource_type = 9999, _reserved = 0
    probe(&mut code, EINVAL, 0x1D);
    set(&mut code, F_ENTRY, 1 | (1 << 16)); // resource_type = 1, _reserved[0] = 1
    probe(&mut code, EINVAL, 0x1E);
    set(&mut code, F_ENTRY, 1); // resource_type = 1, _reserved = 0
    probe(&mut code, EFAULT, 0x1F);

    // --- inherit-all ignores the array --------------------------------------
    // Only the mode changes here; the pointer and count are junk, so an accept
    // can only mean the array was never consulted.
    set(&mut code, F_CAP_MODE, 0);
    set(&mut code, F_CAP_PTR, UNMAPPED);
    set(&mut code, F_CAP_COUNT, 7);
    probe(&mut code, EFAULT, 0x20);

    // --- the working directory ----------------------------------------------
    // Checked at the gate, before the ELF read, so an accepted `cwd` still ends
    // in -101 and a refused one in -3.  Inherit-all stays on from above, so the
    // capability array plays no part.
    set(&mut code, F_CWD_PTR, UNMAPPED);
    set(&mut code, F_CWD_LEN, 0);
    probe(&mut code, EFAULT, 0x21);
    set(&mut code, F_CWD_PTR, 0);
    set(&mut code, F_CWD_LEN, 1);
    probe(&mut code, EINVAL, 0x22);
    set(&mut code, F_CWD_PTR, UNMAPPED);
    set(&mut code, F_CWD_LEN, 4096); // CWD_MAX_LEN + 1
    probe(&mut code, EINVAL, 0x23);
    lea_rax(&mut code, F_PATH);
    store(&mut code, F_CWD_PTR);
    set(&mut code, F_CWD_LEN, 1);
    set(&mut code, F_PATH, u64::from(b'a'));
    probe(&mut code, EINVAL, 0x24);
    set(&mut code, F_PATH, u64::from(b'/'));
    probe(&mut code, EFAULT, 0x25);
    set(&mut code, F_CWD_LEN, 0);

    // --- the process group, session and signal fields ----------------------
    // Judged at the gate with the rest, before the ELF read: a refused value
    // ends in -3 and an accepted one in -101. Each field goes back to zero
    // after its probes, so each probe changes only what it names.
    set(&mut code, F_PGID_MODE, 2);
    probe(&mut code, EINVAL, 0x26);
    set(&mut code, F_PGID_MODE, 0);
    set(&mut code, F_PGID, 5);
    probe(&mut code, EINVAL, 0x27);
    set(&mut code, F_PGID_MODE, 1);
    set(&mut code, F_PGID, 0x8000_0000);
    probe(&mut code, EINVAL, 0x28);
    set(&mut code, F_PGID, 0);
    probe(&mut code, EFAULT, 0x29);
    set(&mut code, F_PGID_MODE, 0);
    set(&mut code, F_SIGMASK_SET, 2);
    probe(&mut code, EINVAL, 0x2A);
    set(&mut code, F_SIGMASK_SET, 0);
    set(&mut code, F_SIGMASK, 1);
    probe(&mut code, EINVAL, 0x2B);
    set(&mut code, F_SIGMASK_SET, 1);
    set(&mut code, F_SIGMASK, u64::MAX);
    probe(&mut code, EFAULT, 0x2C);
    set(&mut code, F_SIGMASK_SET, 0);
    set(&mut code, F_SIGMASK, 0);
    set(&mut code, F_SETSID, 2);
    probe(&mut code, EINVAL, 0x2D);
    set(&mut code, F_SETSID, 1);
    probe(&mut code, EFAULT, 0x2E);
    set(&mut code, F_SETSID, 0);
    set(&mut code, F_SIGDEFAULT, u64::MAX);
    probe(&mut code, EFAULT, 0x2F);
    set(&mut code, F_SIGDEFAULT, 0);
    // An older caller's struct stops before the six: whatever lies past its
    // end in its memory is not read as them.
    set(&mut code, F_SIZE, 144);
    set(&mut code, F_PGID_MODE, 2);
    probe(&mut code, EFAULT, 0x30);
    set(&mut code, F_PGID_MODE, 0);
    set(&mut code, F_SIZE, CURRENT);

    // --- every probe agreed -------------------------------------------------
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3

    // --- file image ---------------------------------------------------------
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

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, code_len as u64); // p_filesz
    write_u64(&mut buf, ph + 40, code_len as u64); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    buf[code_offset as usize..file_size].copy_from_slice(&code);

    buf
}

/// Wrap ring-3 test `code` in a one-segment ELF image loaded, read and
/// execute, at `0x40_0000_0000` -- the layout every probe program in this
/// file uses.  `code` must be position-independent or linked for that
/// address, and must end in `SYS_EXIT`.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
fn single_segment_test_elf(code: &[u8]) -> alloc::vec::Vec<u8> {
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 + 56
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    let code_len = code.len();
    let file_size = code_offset as usize + code_len;
    let mut buf = alloc::vec![0u8; file_size];

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

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, code_len as u64); // p_filesz
    write_u64(&mut buf, ph + 40, code_len as u64); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    buf[code_offset as usize..file_size].copy_from_slice(code);
    buf
}

/// `exit(status)`: the tail every path of a probe program ends in.
fn emit_exit(code: &mut alloc::vec::Vec<u8>, status: u32) {
    code.push(0xBF); // mov edi, status
    code.extend_from_slice(&status.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3 -- exit does not return
}

/// `SYS_SHM_MAP(handle, MAP_READ | MAP_WRITE)`, retried with a `SYS_YIELD`
/// between tries until it returns an address (left in `rax`), or
/// `exit(fail)` after 2000 tries.
///
/// The retry is for the test harness, not for robustness: the kernel
/// authorizes the process to map the region only once `spawn` has given it a
/// pid, and by then the program may already have asked.
fn emit_shm_map_retry(code: &mut alloc::vec::Vec<u8>, handle: u64, fail: u32) {
    code.extend_from_slice(&[0x41, 0xBF]); // mov r15d, 2000
    code.extend_from_slice(&2000u32.to_le_bytes());
    // retry:                                                      (36 bytes)
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, handle
    code.extend_from_slice(&handle.to_le_bytes());
    code.extend_from_slice(&[0xBE, 0x03, 0x00, 0x00, 0x00]); // mov esi, MAP_READ | MAP_WRITE
    code.extend_from_slice(&[0xB8, 0xE9, 0x00, 0x00, 0x00]); // mov eax, SYS_SHM_MAP (233)
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x7F, 0x16]); // jg +22 -- past the fail exit: mapped
    code.extend_from_slice(&[0x31, 0xC0]); // xor eax, eax (SYS_YIELD)
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0x41, 0xFF, 0xCF]); // dec r15d
    code.extend_from_slice(&[0x75, 0xDC]); // jnz -36 -- retry
    emit_exit(code, fail); //                                          (13 bytes)
    // mapped: the address is in rax.
}

/// Build the waiting half of the cross-process futex test
/// (`proc::spawn::self_test_shm_futex`).
///
/// Maps the shared-memory region `shm_handle` and parks on the futex word at
/// its start -- `while *word == 0 { FUTEX_WAIT(word, 0) }` -- then exits 0.
/// It can only get past the loop if the word turns nonzero, which only the
/// waker process does, and it can only notice while parked if the waker's
/// `FUTEX_WAKE` reaches its queue.
///
/// | Exit | Meaning |
/// |---|---|
/// | `0` | the word read nonzero after a wait |
/// | `0x81` | `SYS_SHM_MAP` never succeeded |
#[must_use]
pub fn build_shm_futex_waiter_elf(shm_handle: u64) -> alloc::vec::Vec<u8> {
    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    emit_shm_map_retry(&mut code, shm_handle, 0x81);
    code.extend_from_slice(&[0x48, 0x89, 0xC3]); // mov rbx, rax -- the word
    // wait_loop:                                                    (20 bytes)
    code.extend_from_slice(&[0x8B, 0x03]); // mov eax, [rbx]
    code.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
    code.extend_from_slice(&[0x75, 0x0E]); // jnz +14 -- done
    code.extend_from_slice(&[0x48, 0x89, 0xDF]); // mov rdi, rbx
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi -- expected 0
    code.extend_from_slice(&[0xB8, 0xD2, 0x00, 0x00, 0x00]); // mov eax, SYS_FUTEX_WAIT (210)
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0xEB, 0xEC]); // jmp -20 -- wait_loop
    // done:
    emit_exit(&mut code, 0);
    single_segment_test_elf(&code)
}

/// Build the waking half of the cross-process futex test
/// (`proc::spawn::self_test_shm_futex`).
///
/// Maps the shared-memory region `shm_handle` twice and uses the *second*
/// mapping, so the address it wakes through is one the waiter never used
/// (the first mapping of a fresh process may well land where the waiter's
/// did).  Stores 1 to the word and `FUTEX_WAKE`s one waiter; the call must
/// report that it woke one.
///
/// | Exit | Meaning |
/// |---|---|
/// | `0` | the wake woke one task |
/// | `0x82`, `0x83` | the first or second `SYS_SHM_MAP` never succeeded |
/// | `0x84` | the two mappings came back at one address |
/// | `0x85` | `FUTEX_WAKE` woke nobody: the waiter's queue was not the word's |
#[must_use]
pub fn build_shm_futex_waker_elf(shm_handle: u64) -> alloc::vec::Vec<u8> {
    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    emit_shm_map_retry(&mut code, shm_handle, 0x82);
    code.extend_from_slice(&[0x48, 0x89, 0xC3]); // mov rbx, rax -- first mapping
    emit_shm_map_retry(&mut code, shm_handle, 0x83);
    code.extend_from_slice(&[0x49, 0x89, 0xC4]); // mov r12, rax -- second mapping
    code.extend_from_slice(&[0x49, 0x39, 0xDC]); // cmp r12, rbx
    code.extend_from_slice(&[0x75, 0x0D]); // jne +13 -- over the exit
    emit_exit(&mut code, 0x84);
    code.extend_from_slice(&[0x41, 0xC7, 0x04, 0x24, 0x01, 0x00, 0x00, 0x00]); // mov dword [r12], 1
    code.extend_from_slice(&[0x4C, 0x89, 0xE7]); // mov rdi, r12
    code.extend_from_slice(&[0xBE, 0x01, 0x00, 0x00, 0x00]); // mov esi, 1 -- wake one
    code.extend_from_slice(&[0xB8, 0xD3, 0x00, 0x00, 0x00]); // mov eax, SYS_FUTEX_WAKE (211)
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0x48, 0x3D, 0x01, 0x00, 0x00, 0x00]); // cmp rax, 1
    code.extend_from_slice(&[0x74, 0x0D]); // je +13 -- over the exit
    emit_exit(&mut code, 0x85);
    emit_exit(&mut code, 0);
    single_segment_test_elf(&code)
}

/// Build the ring-3 probe for `SYS_SHM_MAP_AT`
/// (`proc::spawn::self_test_shm_map_at`).
///
/// It first maps the shared-memory region `shm_handle` with `SYS_SHM_MAP`,
/// retrying until the harness's authorization lands (see
/// `emit_shm_map_retry`), and keeps that mapping in `rbx`.  Then it exits
/// with the code of the first probe that disagrees; `exit(0)` means all did.
///
/// | Code | Call | Expect |
/// |---|---|---|
/// | `0xA1` | `shm_map_at(h, RW, 0x66_0000_0000)` | that address |
/// | `0xA2` | write through `rbx`, read through `0x66_0000_0000` | the same word: one region |
/// | `0xA3` | `shm_map_at` at that address again | `-3`: occupied |
/// | `0xA4` | again with `MAP_FIXED` | the address: replaced |
/// | `0xA5` | read through it | still the word `0xA2` wrote: the region, not a fresh page |
/// | `0xA6` | at `0x66_0000_1000` | `-3`: not 16 KiB-aligned |
/// | `0xA7` | at `0xFFFF_8000_0000_0000` | `-3`: the kernel half |
/// | `0xA8` | at `0x50_0000_0000` | `-3`: below the mmap window |
/// | `0xA9` | at `0` | a positive address the kernel chose... |
/// | `0xAA` | | ...and not `0x66_0000_0000` |
#[must_use]
pub fn build_shm_map_at_probe_elf(shm_handle: u64) -> alloc::vec::Vec<u8> {
    const SYS_SHM_MAP_AT: u32 = 235;
    const RW: u32 = 0x3; // MAP_READ | MAP_WRITE
    const MAP_FIXED: u32 = 0x20;
    const FIXED: u64 = 0x0000_0066_0000_0000;
    const EINVAL: u32 = 0xFFFF_FFFD; // -3, sign-extended by `cmp rax, imm32`
    const WORD: u32 = 0x5A5A_5A5A;

    /// `rax = shm_map_at(handle, flags, addr)`.
    fn map_at(code: &mut alloc::vec::Vec<u8>, handle: u64, flags: u32, addr: u64) {
        code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, handle
        code.extend_from_slice(&handle.to_le_bytes());
        code.push(0xBE); // mov esi, flags
        code.extend_from_slice(&flags.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xBA]); // movabs rdx, addr
        code.extend_from_slice(&addr.to_le_bytes());
        code.push(0xB8); // mov eax, SYS_SHM_MAP_AT
        code.extend_from_slice(&SYS_SHM_MAP_AT.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
    }
    /// `exit(fail)` unless `rax == value`.
    fn expect_rax_u64(code: &mut alloc::vec::Vec<u8>, value: u64, fail: u32) {
        code.extend_from_slice(&[0x48, 0xB9]); // movabs rcx, value
        code.extend_from_slice(&value.to_le_bytes());
        code.extend_from_slice(&[0x48, 0x39, 0xC8]); // cmp rax, rcx
        code.extend_from_slice(&[0x74, 0x0D]); // je +13 -- over the exit
        emit_exit(code, fail);
    }
    /// `exit(fail)` unless `rax == value` (sign-extended from 32 bits).
    fn expect_rax_i32(code: &mut alloc::vec::Vec<u8>, value: u32, fail: u32) {
        code.extend_from_slice(&[0x48, 0x3D]); // cmp rax, imm32
        code.extend_from_slice(&value.to_le_bytes());
        code.extend_from_slice(&[0x74, 0x0D]); // je +13 -- over the exit
        emit_exit(code, fail);
    }
    /// `exit(fail)` unless the word at `FIXED` is `WORD`.
    fn expect_word_at_fixed(code: &mut alloc::vec::Vec<u8>, fail: u32) {
        code.extend_from_slice(&[0x48, 0xB9]); // movabs rcx, FIXED
        code.extend_from_slice(&FIXED.to_le_bytes());
        code.extend_from_slice(&[0x8B, 0x01]); // mov eax, [rcx]
        code.push(0x3D); // cmp eax, WORD
        code.extend_from_slice(&WORD.to_le_bytes());
        code.extend_from_slice(&[0x74, 0x0D]); // je +13 -- over the exit
        emit_exit(code, fail);
    }

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    emit_shm_map_retry(&mut code, shm_handle, 0xA0);
    code.extend_from_slice(&[0x48, 0x89, 0xC3]); // mov rbx, rax -- the first mapping

    map_at(&mut code, shm_handle, RW, FIXED);
    expect_rax_u64(&mut code, FIXED, 0xA1);
    code.extend_from_slice(&[0xC7, 0x03]); // mov dword [rbx], WORD
    code.extend_from_slice(&WORD.to_le_bytes());
    expect_word_at_fixed(&mut code, 0xA2);
    map_at(&mut code, shm_handle, RW, FIXED);
    expect_rax_i32(&mut code, EINVAL, 0xA3);
    map_at(&mut code, shm_handle, RW | MAP_FIXED, FIXED);
    expect_rax_u64(&mut code, FIXED, 0xA4);
    expect_word_at_fixed(&mut code, 0xA5);
    map_at(&mut code, shm_handle, RW, FIXED + 0x1000);
    expect_rax_i32(&mut code, EINVAL, 0xA6);
    map_at(&mut code, shm_handle, RW, 0xFFFF_8000_0000_0000);
    expect_rax_i32(&mut code, EINVAL, 0xA7);
    map_at(&mut code, shm_handle, RW, 0x0000_0050_0000_0000);
    expect_rax_i32(&mut code, EINVAL, 0xA8);
    map_at(&mut code, shm_handle, RW, 0);
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x7F, 0x0D]); // jg +13 -- over the exit
    emit_exit(&mut code, 0xA9);
    code.extend_from_slice(&[0x48, 0xB9]); // movabs rcx, FIXED
    code.extend_from_slice(&FIXED.to_le_bytes());
    code.extend_from_slice(&[0x48, 0x39, 0xC8]); // cmp rax, rcx
    code.extend_from_slice(&[0x75, 0x0D]); // jne +13 -- over the exit
    emit_exit(&mut code, 0xAA);
    emit_exit(&mut code, 0);
    single_segment_test_elf(&code)
}

/// Build the ring-3 probe for native `SYS_MUNMAP`'s argument checks.
///
/// Until 2026-09-26 `sys_munmap` unmapped and freed whatever canonical range a
/// process named, kernel half included (known-issues.md
/// `A-NATIVE-MUNMAP-UNMAPPED-KERNEL-PAGES`).  Only a ring-3 caller exercises
/// that path as an attacker would -- the in-kernel tests cannot, because the
/// handler resolves *the calling process* -- so this program issues the calls
/// itself and exits with the code of the first one whose result disagrees.
/// `exit(0)` means every probe agreed.
///
/// # Why no probe names a *mapped* kernel page
///
/// The kernel-half probe uses an address in the hole between the KASAN shadow
/// and kernel text (`0xFFFF_E800_0000_0000`), which no region maps.  Against
/// the old code that call returned `0` -- "unmapped nothing, success" -- so the
/// probe still fails loudly if the check regresses, without the regression
/// unmapping a kernel stack in the process of being detected.  The in-kernel
/// half, `mm::user::self_test_unmap_user_range`, is the one that puts a real
/// mapped kernel page in front of the teardown.
///
/// # Probes
///
/// | Code | `addr` | `len` | Expect | Proves |
/// |---|---|---|---|---|
/// | `0x31` | `0xFFFF_E800_0000_0000` | `0x4000` | `-3` | the kernel half is refused (was `0`) |
/// | `0x32` | `0x7FFF_FFFF_C000` | `0x8000` | `-3` | a range that runs past `USER_SPACE_END` is refused whole |
/// | `0x33` | `0xFFFF_FFFF_FFFF_C000` | `0x8000` | `-3` | `addr + len` overflowing is refused |
/// | `0x34` | `0x30_0000_0000` | `u64::MAX` | `-3` | `len` rounding up overflowing is refused |
/// | `0x35` | `0x30_0000_0000` | `0` | `-3` | a zero length is EINVAL, as POSIX says (was `0`) |
/// | `0x36` | `0x30_0000_1000` | `0x4000` | `-103` | a start not on a 16 KiB frame is `BadAlignment` |
/// | `0x37` | `0x30_0000_0000` | `0x4000` | `0` | control: an unmapped user range is accepted, and unmaps nothing |
/// | `0x38` | `0x7FFF_FFFF_C000` | `0x4000` | `0` | control: the last user frame, ending exactly at `USER_SPACE_END`, is accepted |
///
/// The two controls are what make the refusals mean something: a handler that
/// refused *everything* would pass `0x31`-`0x36` too.  `0x38` pins the
/// boundary -- an off-by-one in the range check fails it.  (It sits above
/// `USER_STACK_TOP`, `0x7FFF_FFFF_0000`, so nothing is mapped there to lose.)
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_munmap_abi_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 + 56
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    /// Native `SYS_MUNMAP` (`syscall::number`).
    const SYS_MUNMAP: u32 = 21;
    /// `KernelError::InvalidArgument` as it arrives in `rax`.
    const EINVAL: i32 = -3;
    /// `KernelError::BadAlignment` as it arrives in `rax`.
    const EALIGN: i32 = -103;
    /// A canonical user address that is never mapped in a fresh process.
    const UNMAPPED_USER: u64 = 0x0000_0030_0000_0000;
    /// The last 16 KiB frame of the user half.
    const LAST_USER_FRAME: u64 = 0x0000_7FFF_FFFF_C000;
    /// Canonical, kernel half, and in no kernel region (between the KASAN
    /// shadow's end at `0xFFFF_E000_0000_0000` and kernel text).
    const KERNEL_HOLE: u64 = 0xFFFF_E800_0000_0000;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    /// `munmap(addr, len)`; if `rax != expect`, `exit(fail)` on the spot.
    fn probe(code: &mut alloc::vec::Vec<u8>, addr: u64, len: u64, expect: i32, fail: u32) {
        code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, addr
        code.extend_from_slice(&addr.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, len
        code.extend_from_slice(&len.to_le_bytes());
        code.push(0xB8); // mov eax, SYS_MUNMAP
        code.extend_from_slice(&SYS_MUNMAP.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.extend_from_slice(&[0x48, 0x3D]); // cmp rax, imm32 (sign-extended)
        code.extend_from_slice(&(expect as u32).to_le_bytes());
        code.extend_from_slice(&[0x74, 0x0D]); // je +13 — over the exit block
        code.push(0xBF); // mov edi, <fail>
        code.extend_from_slice(&fail.to_le_bytes());
        code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.push(0xCC); // int3 — exit does not return
    }

    probe(&mut code, KERNEL_HOLE, 0x4000, EINVAL, 0x31);
    probe(&mut code, LAST_USER_FRAME, 0x8000, EINVAL, 0x32);
    probe(&mut code, 0xFFFF_FFFF_FFFF_C000, 0x8000, EINVAL, 0x33);
    probe(&mut code, UNMAPPED_USER, u64::MAX, EINVAL, 0x34);
    probe(&mut code, UNMAPPED_USER, 0, EINVAL, 0x35);
    probe(&mut code, UNMAPPED_USER + 0x1000, 0x4000, EALIGN, 0x36);
    probe(&mut code, UNMAPPED_USER, 0x4000, 0, 0x37);
    probe(&mut code, LAST_USER_FRAME, 0x4000, 0, 0x38);

    // --- every probe agreed -------------------------------------------------
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3

    // --- file image ---------------------------------------------------------
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

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, code_len as u64); // p_filesz
    write_u64(&mut buf, ph + 40, code_len as u64); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    buf[code_offset as usize..file_size].copy_from_slice(&code);

    buf
}

/// Build the ring-3 probe for the payload size gates of the channel and UDP
/// send syscalls.
///
/// Each probe asks for a send whose length is over the limit, from a pointer
/// that is **not mapped**.  That combination is the whole test: a syscall that
/// judges the size first answers with the size error; one that copies first
/// fails the copy and answers `InvalidAddress` -- which is what every one of
/// these did until 2026-09-26, having first tried to allocate a kernel buffer
/// of the requested size (known-issues.md
/// `A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC`).  The controls ask for an
/// in-limit length from the same unmapped pointer and must get
/// `InvalidAddress`: the gate refuses sizes, not everything.
///
/// The probes send on objects the program makes first: a channel call refuses
/// a handle its caller does not hold before anything else
/// (`require_ipc_handle`), and so, since 2026-10-02, does a native socket call
/// (`net::native_socket`, which made socket handles their holders'), so a size
/// gate is reached only through a real handle. The gate still comes before
/// any copy of the payload, which is what is pinned. `0x40`: the channel
/// could not be made; `0x49`: the UDP socket could not be bound.
///
/// | Code | Call | Length | Expect |
/// |---|---|---|---|
/// | `0x41` | `channel_send` (201) | 64 KiB + 1 | `-302` MessageTooLarge |
/// | `0x42` | `channel_send_timeout` (208) | 64 KiB + 1 | `-302` |
/// | `0x43` | `channel_send_blocking` (209) | 64 KiB + 1 | `-302` |
/// | `0x44` | `channel_send_caps` (206), no caps | 64 KiB + 1 | `-302` |
/// | `0x45` | `udp_send` (811), own socket | 65,528 (one over) | `-3` InvalidArgument |
/// | `0x46` | `channel_send` | 16 | `-101` InvalidAddress (control) |
/// | `0x47` | `udp_send`, own socket | 16 | `-101` (control) |
/// | `0x48` | `channel_send` | exactly 64 KiB | `-101`: the limit is inclusive |
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_sizegate_abi_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let code_offset: u64 = 120; // 64 + 56
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    const MESSAGE_TOO_LARGE: i32 = -302;
    const EINVAL: i32 = -3;
    const EFAULT: i32 = -101;
    const UNMAPPED: u64 = 0x0000_0030_0000_0000;
    /// Stands for the channel endpoint the program made (rbx), in `probe`.
    const OWN_CHANNEL: u64 = u64::MAX;
    /// Stands for the UDP socket the program bound (r12), in `probe`.
    const OWN_UDP: u64 = u64::MAX - 1;
    /// The port that socket is bound to: one no other self-test uses.
    const UDP_PORT: u32 = 47_900;
    const MAX_MESSAGE: u64 = 64 * 1024;
    const MAX_UDP_PAYLOAD: u64 = 65_535 - 8;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // A channel of the program's own, its first endpoint kept in rbx: the
    // channel calls refuse a handle the caller does not hold before they look
    // at the length (`require_ipc_handle`, since lane F's handle request of
    // 2026-10-01), so their size gates are reached only through a real one.
    // Until rq42 these probes used BOGUS_HANDLE, and 0x41 got InvalidHandle.
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi (flags 0)
    code.extend_from_slice(&[0xB8, 200, 0x00, 0x00, 0x00]); // mov eax, SYS_CHANNEL_CREATE
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x79, 0x0D]); // jns +13 -- over the exit block
    code.push(0xBF); // mov edi, 0x40
    code.extend_from_slice(&0x40u32.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3
    code.extend_from_slice(&[0x48, 0x89, 0xC3]); // mov rbx, rax

    // A UDP socket of the program's own, its handle kept in r12, for the same
    // reason: `udp_send` refuses a handle its caller does not hold before it
    // looks at the length. Until rq46 these probes used a bogus handle, and
    // 0x45 got InvalidHandle (rq45).
    code.push(0xBF); // mov edi, UDP_PORT
    code.extend_from_slice(&UDP_PORT.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x2A, 0x03, 0x00, 0x00]); // mov eax, 810 (SYS_UDP_BIND)
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    code.extend_from_slice(&[0x79, 0x0D]); // jns +13 -- over the exit block
    code.push(0xBF); // mov edi, 0x49
    code.extend_from_slice(&0x49u32.to_le_bytes());
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3
    code.extend_from_slice(&[0x49, 0x89, 0xC4]); // mov r12, rax

    /// `syscall(nr, rdi, rsi, rdx, r10, r8)`; if `rax != expect`, `exit(fail)`.
    /// `rdi` is `regs[0]`, or the channel endpoint in rbx when `regs[0]` is
    /// `OWN_CHANNEL`, or the UDP socket in r12 when it is `OWN_UDP`.
    fn probe(code: &mut alloc::vec::Vec<u8>, nr: u32, regs: [u64; 5], expect: i32, fail: u32) {
        // movabs rdi / rsi / rdx / r10 / r8, imm64
        for (prefix, value) in [
            ([0x48, 0xBF], regs[0]),
            ([0x48, 0xBE], regs[1]),
            ([0x48, 0xBA], regs[2]),
            ([0x49, 0xBA], regs[3]),
            ([0x49, 0xB8], regs[4]),
        ] {
            if prefix == [0x48, 0xBF] && value == OWN_CHANNEL {
                code.extend_from_slice(&[0x48, 0x89, 0xDF]); // mov rdi, rbx
                continue;
            }
            if prefix == [0x48, 0xBF] && value == OWN_UDP {
                code.extend_from_slice(&[0x4C, 0x89, 0xE7]); // mov rdi, r12
                continue;
            }
            code.extend_from_slice(&prefix);
            code.extend_from_slice(&value.to_le_bytes());
        }
        code.push(0xB8); // mov eax, nr
        code.extend_from_slice(&nr.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.extend_from_slice(&[0x48, 0x3D]); // cmp rax, imm32 (sign-extended)
        code.extend_from_slice(&(expect as u32).to_le_bytes());
        code.extend_from_slice(&[0x74, 0x0D]); // je +13 — over the exit block
        code.push(0xBF); // mov edi, <fail>
        code.extend_from_slice(&fail.to_le_bytes());
        code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.push(0xCC); // int3 — exit does not return
    }

    let over = MAX_MESSAGE + 1;
    probe(
        &mut code,
        201,
        [OWN_CHANNEL, UNMAPPED, over, 0, 0],
        MESSAGE_TOO_LARGE,
        0x41,
    );
    probe(
        &mut code,
        208,
        [OWN_CHANNEL, UNMAPPED, over, 0, 0],
        MESSAGE_TOO_LARGE,
        0x42,
    );
    probe(
        &mut code,
        209,
        [OWN_CHANNEL, UNMAPPED, over, 0, 0],
        MESSAGE_TOO_LARGE,
        0x43,
    );
    probe(
        &mut code,
        206,
        [OWN_CHANNEL, UNMAPPED, over, 0, 0],
        MESSAGE_TOO_LARGE,
        0x44,
    );
    probe(
        &mut code,
        811,
        [OWN_UDP, 0x7F00_0001, 1, UNMAPPED, MAX_UDP_PAYLOAD + 1],
        EINVAL,
        0x45,
    );
    probe(
        &mut code,
        201,
        [OWN_CHANNEL, UNMAPPED, 16, 0, 0],
        EFAULT,
        0x46,
    );
    probe(
        &mut code,
        811,
        [OWN_UDP, 0x7F00_0001, 1, UNMAPPED, 16],
        EFAULT,
        0x47,
    );
    probe(
        &mut code,
        201,
        [OWN_CHANNEL, UNMAPPED, MAX_MESSAGE, 0, 0],
        EFAULT,
        0x48,
    );

    // --- every probe agreed -------------------------------------------------
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3

    // --- file image ---------------------------------------------------------
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

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, code_len as u64); // p_filesz
    write_u64(&mut buf, ph + 40, code_len as u64); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    buf[code_offset as usize..file_size].copy_from_slice(&code);

    buf
}

/// Build the ring-3 probe for the per-call copy bound of the pipe and
/// socketpair data syscalls.
///
/// The program has a 1 MiB zero-filled data segment -- exactly one pipe
/// call's maximum -- and asks each call to move **2 GiB** from or into it.
/// A handler that bounces the whole length through the kernel cannot answer
/// that: 2 GiB is more than the vmalloc region holds, and the read side
/// page-walks the full length into the unmapped memory past the segment.
/// Until 2026-09-26 every one of them did (known-issues.md
/// `A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC`).  A handler that copies at
/// most what one call can move answers with one buffer's worth: 64 KiB, an
/// empty pipe's default capacity and a socketpair ring's.
///
/// The `2^63` probes are the other half: the caller's whole claim is still
/// checked as a user span, as Linux's `access_ok` does, so a length that runs
/// past user space is `InvalidAddress` although the copy would stop at 1 MiB.
/// Without that check the write would succeed and the read would block.
///
/// | Code | Call | Length | Expect |
/// |---|---|---|---|
/// | `0x51` | `pipe_create` (220) | -- | two handles |
/// | `0x52` | `pipe_write` (221) | 2 GiB | `65536` |
/// | `0x53` | `pipe_read` (222) | 2 GiB | `65536`, what `0x52` wrote |
/// | `0x54` | `pipe_write` | `2^63` | `-101` InvalidAddress |
/// | `0x55` | `pipe_read` | `2^63` | `-101`, not a block on the empty pipe |
/// | `0x56` | `socketpair_create` (300) | -- | two handles |
/// | `0x57` | `socketpair_send` (301) | 2 GiB | `65536` |
/// | `0x58` | `socketpair_recv` (302) | 2 GiB | `65536` |
/// | `0x59` | `socketpair_send` | `2^63` | `-101` |
/// | `0x5A` | `pty_create` (544) | -- | two handles |
/// | `0x5B` | `pty_master_write` (545) | 2 GiB | `1..=4096`: one input queue |
/// | `0x5C` | `pty_master_try_read` (547) | 2 GiB | `1..=65536`: the echo of `0x5B` |
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_callmax_abi_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const PHNUM: u64 = 2;
    let phdr_offset: u64 = 64;
    let code_offset: u64 = 64 + PHNUM * 56;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // The data segment: one pipe call's worth, committed and zero-filled.
    const DATA_VADDR: u64 = 0x0000_0050_0000_0000;
    const DATA_LEN: u64 = 1024 * 1024;
    const TWO_GIB: u64 = 2 * 1024 * 1024 * 1024;
    const PAST_USER_SPACE: u64 = 1 << 63;
    const ONE_BUFFER: i32 = 64 * 1024;
    // A pty master write takes at most the input queue (`tty::INPUT_QUEUE_CAPACITY`).
    const PTY_INPUT_QUEUE: i32 = 4096;
    const EFAULT: i32 = -101;

    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    /// `jcc` over an `exit(fail)` block: the flags just set decide whether
    /// the program goes on or reports `fail`.
    fn exit_unless(code: &mut alloc::vec::Vec<u8>, jcc: u8, fail: u32) {
        code.extend_from_slice(&[jcc, 0x0D]); // jcc +13 — over the exit block
        code.push(0xBF); // mov edi, <fail>
        code.extend_from_slice(&fail.to_le_bytes());
        code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.push(0xCC); // int3 — exit does not return
    }

    /// A create call that returns a handle pair in `rax`/`rdx`: kept in two
    /// registers the kernel's syscall entry preserves.
    fn create(code: &mut alloc::vec::Vec<u8>, nr: u32, keep: [[u8; 3]; 2], fail: u32) {
        code.push(0xB8); // mov eax, nr
        code.extend_from_slice(&nr.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
        exit_unless(code, 0x79, fail); // jns: a handle, not an error
        code.extend_from_slice(&keep[0]); // mov <first>, rax
        code.extend_from_slice(&keep[1]); // mov <second>, rdx
    }

    /// `nr(handle, rsi, rdx)`, the handle moved into `rdi` from where
    /// `create` kept it; `exit(fail)` unless `rax == expect`.
    fn transfer(
        code: &mut alloc::vec::Vec<u8>,
        handle: [u8; 3],
        nr: u32,
        buf: u64,
        len: u64,
        expect: i32,
        fail: u32,
    ) {
        code.extend_from_slice(&handle); // mov rdi, <handle>
        code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, imm64
        code.extend_from_slice(&buf.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xBA]); // movabs rdx, imm64
        code.extend_from_slice(&len.to_le_bytes());
        code.push(0xB8); // mov eax, nr
        code.extend_from_slice(&nr.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.extend_from_slice(&[0x48, 0x3D]); // cmp rax, imm32 (sign-extended)
        code.extend_from_slice(&(expect as u32).to_le_bytes());
        exit_unless(code, 0x74, fail); // je
    }

    /// `nr(handle, rsi, rdx)`; `exit(fail)` unless `0 < rax <= max`: a call
    /// that succeeds and moves no more than one call's worth.
    fn bounded(
        code: &mut alloc::vec::Vec<u8>,
        handle: [u8; 3],
        nr: u32,
        buf: u64,
        len: u64,
        max: i32,
        fail: u32,
    ) {
        code.extend_from_slice(&handle); // mov rdi, <handle>
        code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, imm64
        code.extend_from_slice(&buf.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xBA]); // movabs rdx, imm64
        code.extend_from_slice(&len.to_le_bytes());
        code.push(0xB8); // mov eax, nr
        code.extend_from_slice(&nr.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
        exit_unless(code, 0x7F, fail); // jg: a positive count
        code.extend_from_slice(&[0x48, 0x3D]); // cmp rax, imm32 (sign-extended)
        code.extend_from_slice(&(max as u32).to_le_bytes());
        exit_unless(code, 0x7E, fail); // jle: no more than `max`
    }

    const MOV_RBX_RAX: [u8; 3] = [0x48, 0x89, 0xC3];
    const MOV_R12_RDX: [u8; 3] = [0x49, 0x89, 0xD4];
    const MOV_R13_RAX: [u8; 3] = [0x49, 0x89, 0xC5];
    const MOV_R14_RDX: [u8; 3] = [0x49, 0x89, 0xD6];
    const RDI_FROM_RBX: [u8; 3] = [0x48, 0x89, 0xDF]; // mov rdi, rbx
    const RDI_FROM_R12: [u8; 3] = [0x4C, 0x89, 0xE7]; // mov rdi, r12
    const RDI_FROM_R13: [u8; 3] = [0x4C, 0x89, 0xEF]; // mov rdi, r13
    const RDI_FROM_R14: [u8; 3] = [0x4C, 0x89, 0xF7]; // mov rdi, r14

    // --- a pipe: rbx = read end, r12 = write end --------------------------
    create(&mut code, 220, [MOV_RBX_RAX, MOV_R12_RDX], 0x51);
    transfer(
        &mut code,
        RDI_FROM_R12,
        221,
        DATA_VADDR,
        TWO_GIB,
        ONE_BUFFER,
        0x52,
    );
    transfer(
        &mut code,
        RDI_FROM_RBX,
        222,
        DATA_VADDR,
        TWO_GIB,
        ONE_BUFFER,
        0x53,
    );
    transfer(
        &mut code,
        RDI_FROM_R12,
        221,
        DATA_VADDR,
        PAST_USER_SPACE,
        EFAULT,
        0x54,
    );
    transfer(
        &mut code,
        RDI_FROM_RBX,
        222,
        DATA_VADDR,
        PAST_USER_SPACE,
        EFAULT,
        0x55,
    );

    // --- a socketpair: r13 = one end, r14 = the other ----------------------
    create(&mut code, 300, [MOV_R13_RAX, MOV_R14_RDX], 0x56);
    transfer(
        &mut code,
        RDI_FROM_R13,
        301,
        DATA_VADDR,
        TWO_GIB,
        ONE_BUFFER,
        0x57,
    );
    transfer(
        &mut code,
        RDI_FROM_R14,
        302,
        DATA_VADDR,
        TWO_GIB,
        ONE_BUFFER,
        0x58,
    );
    transfer(
        &mut code,
        RDI_FROM_R13,
        301,
        DATA_VADDR,
        PAST_USER_SPACE,
        EFAULT,
        0x59,
    );

    // --- a pty: r13 = master, r14 = slave ------------------------------------
    // The segment's first 4 KiB become ordinary keystrokes ('a'), so what the
    // probes see does not hang on how the line discipline treats NUL: each 'a'
    // takes one input-queue slot and echoes one byte.
    code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, DATA_VADDR
    code.extend_from_slice(&DATA_VADDR.to_le_bytes());
    code.extend_from_slice(&[0xB9, 0x00, 0x10, 0x00, 0x00]); // mov ecx, 4096
    code.extend_from_slice(&[0xB0, b'a']); // mov al, 'a'
    code.extend_from_slice(&[0xF3, 0xAA]); // rep stosb
    create(&mut code, 544, [MOV_R13_RAX, MOV_R14_RDX], 0x5A);
    bounded(
        &mut code,
        RDI_FROM_R13,
        545,
        DATA_VADDR,
        TWO_GIB,
        PTY_INPUT_QUEUE,
        0x5B,
    );
    bounded(
        &mut code,
        RDI_FROM_R13,
        547,
        DATA_VADDR,
        TWO_GIB,
        ONE_BUFFER,
        0x5C,
    );

    // --- every probe agreed -------------------------------------------------
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3

    // --- file image ---------------------------------------------------------
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
    write_u16(&mut buf, 16, ET_EXEC);
    write_u16(&mut buf, 18, EM_X86_64);
    write_u32(&mut buf, 20, u32::from(EV_CURRENT));
    write_u64(&mut buf, 24, load_vaddr); // e_entry
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, PHNUM as u16); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0); // e_shnum
    write_u16(&mut buf, 62, 0); // e_shstrndx

    // The code.
    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, code_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, code_len as u64); // p_filesz
    write_u64(&mut buf, ph + 40, code_len as u64); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    // The data: nothing in the file, 1 MiB in memory.
    let ph = ph + ELF64_PHDR_SIZE;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_W);
    write_u64(&mut buf, ph + 8, 0); // p_offset
    write_u64(&mut buf, ph + 16, DATA_VADDR); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, 0); // p_filesz
    write_u64(&mut buf, ph + 40, DATA_LEN); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    buf[code_offset as usize..file_size].copy_from_slice(&code);

    buf
}

/// Build the ring-3 probe for streamed file I/O (`SYS_FS_READ`/`SYS_FS_WRITE`
/// through `FILE_CALL_CHUNK`-sized bounce buffers).
///
/// The program has a 3 MiB zero-filled data segment. It fills 2.5 MiB of it
/// with `A`, `B` and `C` a region apiece -- 1 MiB, 1 MiB, 0.5 MiB, so each
/// chunk boundary falls between two different bytes -- writes that to a file
/// in one call, zeroes the segment, reads the file back with a 2 GiB length,
/// and checks every byte is back where it was. A chunking bug that wrote a
/// chunk twice or out of order passes a byte count; it does not pass this.
///
/// The last call writes 1.5 GiB from the 3 MiB segment: the stream writes
/// what is mapped and stops at the fault, returning the count, as Linux does.
/// Before 2026-09-26 every one of these calls bounced the whole request
/// through one kernel copy (known-issues.md
/// `A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC`): the read's 2 GiB
/// out-buffer was page-walked into the unmapped memory past the segment
/// (EFAULT), and the 1.5 GiB write could not be allocated at all (ENOMEM).
///
/// | Code | Call | Expect |
/// |---|---|---|
/// | `0x61` | `fs_open` (610) `/tmp/.filestream-probe`, read+write+create+truncate | a handle |
/// | `0x62` | `fs_write` (613), 2.5 MiB | `2621440` |
/// | `0x63` | `fs_seek` (614) to 0 | `0` |
/// | `0x64` | `fs_read` (612), 2 GiB | `2621440`: a short read at EOF |
/// | `0x65` | the `A` region back | every byte `A` |
/// | `0x66` | the `B` region back | every byte `B` |
/// | `0x67` | the `C` region back | every byte `C` |
/// | `0x68` | `fs_write`, 1.5 GiB | `3145728`: the mapped 3 MiB, then the fault |
/// | `0x69` | `fs_pwrite` (1081), 4 KiB at offset 0 | `4096` |
/// | `0x6A` | `fs_seek` (614), 0 from the current position | `5767168`: pwrite left it at the end |
/// | `0x6B` | `fs_pread` (1080), 4 KiB at offset 1 MiB | `4096` |
/// | `0x6C` | what 0x6B read | every byte `B` |
/// | `0x6D` | `fs_seek`, 0 from the current position | `5767168`: pread did not move it |
/// | `0x6E` | `fs_pread` at offset 5.5 MiB | `0`: end of file |
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn build_filestream_abi_test_elf() -> alloc::vec::Vec<u8> {
    use alloc::vec;

    const PHNUM: u64 = 2;
    let phdr_offset: u64 = 64;
    let seg_offset: u64 = 64 + PHNUM * 56;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;

    // The path sits at the start of the text segment, the code after it.
    const PATH: &[u8] = b"/tmp/.filestream-probe";
    const CODE_AT: u64 = 32; // PATH.len() rounded up
    const DATA_VADDR: u64 = 0x0000_0050_0000_0000;
    const DATA_LEN: u64 = 3 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const WRITTEN: u64 = 2 * MIB + MIB / 2;
    const TWO_GIB: u64 = 2 * 1024 * MIB;
    const HUGE: u64 = 1536 * MIB;
    const OPEN_RW_CREATE_TRUNC: u64 = 0b1111;

    let path_vaddr = load_vaddr;
    let mut code: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    /// `jcc` over an `exit(fail)` block.
    fn exit_unless(code: &mut alloc::vec::Vec<u8>, jcc: u8, fail: u32) {
        code.extend_from_slice(&[jcc, 0x0D]); // jcc +13 — over the exit block
        code.push(0xBF); // mov edi, <fail>
        code.extend_from_slice(&fail.to_le_bytes());
        code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
        code.push(0xCC); // int3 — exit does not return
    }

    /// `movabs rdi, a; movabs rsi, b; movabs rdx, c; mov eax, nr; syscall`.
    fn call(code: &mut alloc::vec::Vec<u8>, nr: u32, a: Option<u64>, b: u64, c: u64) {
        match a {
            Some(v) => {
                code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, imm64
                code.extend_from_slice(&v.to_le_bytes());
            }
            None => code.extend_from_slice(&[0x48, 0x89, 0xDF]), // mov rdi, rbx
        }
        code.extend_from_slice(&[0x48, 0xBE]); // movabs rsi, imm64
        code.extend_from_slice(&b.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xBA]); // movabs rdx, imm64
        code.extend_from_slice(&c.to_le_bytes());
        code.push(0xB8); // mov eax, nr
        code.extend_from_slice(&nr.to_le_bytes());
        code.extend_from_slice(&[0x0F, 0x05]); // syscall
    }

    /// As [`call`] with the handle in `rdi` and a fourth argument in `r10`.
    fn call4(code: &mut alloc::vec::Vec<u8>, nr: u32, b: u64, c: u64, d: u64) {
        code.extend_from_slice(&[0x49, 0xBA]); // movabs r10, imm64
        code.extend_from_slice(&d.to_le_bytes());
        call(code, nr, None, b, c);
    }

    /// `exit(fail)` unless `rax == expect`.
    fn expect_rax(code: &mut alloc::vec::Vec<u8>, expect: u64, fail: u32) {
        code.extend_from_slice(&[0x48, 0xB9]); // movabs rcx, imm64
        code.extend_from_slice(&expect.to_le_bytes());
        code.extend_from_slice(&[0x48, 0x39, 0xC8]); // cmp rax, rcx
        exit_unless(code, 0x74, fail); // je
    }

    /// `rep stosb` of `al = byte` over `[at, at + len)`.
    fn fill(code: &mut alloc::vec::Vec<u8>, at: u64, len: u64, byte: u8) {
        code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, imm64
        code.extend_from_slice(&at.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xB9]); // movabs rcx, imm64
        code.extend_from_slice(&len.to_le_bytes());
        code.extend_from_slice(&[0xB0, byte]); // mov al, byte
        code.extend_from_slice(&[0xF3, 0xAA]); // rep stosb
    }

    /// `repe scasb` of `al = byte` over `[at, at + len)`; `exit(fail)` on the
    /// first byte that differs. `len` is never 0, so ZF is always the scan's.
    fn verify(code: &mut alloc::vec::Vec<u8>, at: u64, len: u64, byte: u8, fail: u32) {
        code.extend_from_slice(&[0x48, 0xBF]); // movabs rdi, imm64
        code.extend_from_slice(&at.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xB9]); // movabs rcx, imm64
        code.extend_from_slice(&len.to_le_bytes());
        code.extend_from_slice(&[0xB0, byte]); // mov al, byte
        code.extend_from_slice(&[0xF3, 0xAE]); // repe scasb
        exit_unless(code, 0x74, fail); // je: every byte matched
    }

    // Open (create, truncate) and keep the handle in rbx.
    call(
        &mut code,
        610,
        Some(path_vaddr),
        PATH.len() as u64,
        OPEN_RW_CREATE_TRUNC,
    );
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    exit_unless(&mut code, 0x79, 0x61); // jns: a handle
    code.extend_from_slice(&[0x48, 0x89, 0xC3]); // mov rbx, rax

    // A, B, C a region apiece; one write of all 2.5 MiB.
    fill(&mut code, DATA_VADDR, MIB, b'A');
    fill(&mut code, DATA_VADDR + MIB, MIB, b'B');
    fill(&mut code, DATA_VADDR + 2 * MIB, MIB / 2, b'C');
    call(&mut code, 613, None, DATA_VADDR, WRITTEN);
    expect_rax(&mut code, WRITTEN, 0x62);

    // Back to the start; zero the segment so only the read can restore it.
    call(&mut code, 614, None, 0, 0);
    expect_rax(&mut code, 0, 0x63);
    fill(&mut code, DATA_VADDR, WRITTEN, 0);
    call(&mut code, 612, None, DATA_VADDR, TWO_GIB);
    expect_rax(&mut code, WRITTEN, 0x64);
    verify(&mut code, DATA_VADDR, MIB, b'A', 0x65);
    verify(&mut code, DATA_VADDR + MIB, MIB, b'B', 0x66);
    verify(&mut code, DATA_VADDR + 2 * MIB, MIB / 2, b'C', 0x67);

    // Bigger than the vmalloc region: the mapped 3 MiB go, then the fault.
    call(&mut code, 613, None, DATA_VADDR, HUGE);
    expect_rax(&mut code, DATA_LEN, 0x68);

    // Positional transfers: the position (at the end, 5.5 MiB) stays put.
    const FILE_END: u64 = WRITTEN + DATA_LEN;
    const SPARE: u64 = DATA_VADDR + WRITTEN; // past the A/B/C regions
    call4(&mut code, 1081, DATA_VADDR, 4096, 0); // pwrite at 0
    expect_rax(&mut code, 4096, 0x69);
    call(&mut code, 614, None, 0, 1); // seek(0, SEEK_CUR)
    expect_rax(&mut code, FILE_END, 0x6A);
    call4(&mut code, 1080, SPARE, 4096, MIB); // pread the second MiB
    expect_rax(&mut code, 4096, 0x6B);
    verify(&mut code, SPARE, 4096, b'B', 0x6C);
    call(&mut code, 614, None, 0, 1);
    expect_rax(&mut code, FILE_END, 0x6D);
    call4(&mut code, 1080, SPARE, 4096, FILE_END); // pread at end of file
    expect_rax(&mut code, 0, 0x6E);

    // --- every probe agreed -------------------------------------------------
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, SYS_EXIT
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.push(0xCC); // int3

    // --- file image: [path | pad | code] as one R+X segment -----------------
    let text_len = CODE_AT as usize + code.len();
    let file_size = seg_offset as usize + text_len;
    let mut buf = vec![0u8; file_size];

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
    write_u64(&mut buf, 24, load_vaddr + CODE_AT); // e_entry: past the path
    write_u64(&mut buf, 32, phdr_offset); // e_phoff
    write_u64(&mut buf, 40, 0); // e_shoff
    write_u32(&mut buf, 48, 0); // e_flags
    write_u16(&mut buf, 52, ELF64_EHDR_SIZE as u16);
    write_u16(&mut buf, 54, ELF64_PHDR_SIZE as u16);
    write_u16(&mut buf, 56, PHNUM as u16); // e_phnum
    write_u16(&mut buf, 58, ELF64_SHDR_SIZE as u16);
    write_u16(&mut buf, 60, 0); // e_shnum
    write_u16(&mut buf, 62, 0); // e_shstrndx

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_X);
    write_u64(&mut buf, ph + 8, seg_offset); // p_offset
    write_u64(&mut buf, ph + 16, load_vaddr); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, text_len as u64); // p_filesz
    write_u64(&mut buf, ph + 40, text_len as u64); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    let ph = ph + ELF64_PHDR_SIZE;
    write_u32(&mut buf, ph, PT_LOAD);
    write_u32(&mut buf, ph + 4, PF_R | PF_W);
    write_u64(&mut buf, ph + 8, 0); // p_offset
    write_u64(&mut buf, ph + 16, DATA_VADDR); // p_vaddr
    write_u64(&mut buf, ph + 24, 0); // p_paddr
    write_u64(&mut buf, ph + 32, 0); // p_filesz
    write_u64(&mut buf, ph + 40, DATA_LEN); // p_memsz
    write_u64(&mut buf, ph + 48, 0x1000); // p_align

    let text = seg_offset as usize;
    buf[text..text + PATH.len()].copy_from_slice(PATH);
    buf[text + CODE_AT as usize..file_size].copy_from_slice(&code);

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
/// The handler receives a pointer to
/// [`ExceptionContext`](crate::proc::exception::ExceptionContext) in RDI.
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
    test_detect_slateos_native_marker()?;
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
        serial_println!(
            "[elf]   FAIL: expected 1 program header, got {}",
            elf.program_header_count()
        );
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
            serial_println!(
                "[elf]   FAIL: invalid magic should fail: {:?}",
                other.map(|_| ())
            );
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
            serial_println!(
                "[elf]   FAIL: wrong class should fail: {:?}",
                other.map(|_| ())
            );
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
            serial_println!(
                "[elf]   FAIL: wrong machine should fail: {:?}",
                other.map(|_| ())
            );
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
            serial_println!(
                "[elf]   FAIL: truncated data should fail: {:?}",
                other.map(|_| ())
            );
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

    let segments: alloc::vec::Vec<LoadableSegment> = elf.loadable_segments()?.collect();

    if segments.len() != 1 {
        serial_println!(
            "[elf]   FAIL: expected 1 loadable segment, got {}",
            segments.len()
        );
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

    let segments: alloc::vec::Vec<LoadableSegment> = elf.loadable_segments()?.collect();

    if segments.len() != 1 {
        serial_println!(
            "[elf]   FAIL: expected 1 loadable segment, got {}",
            segments.len()
        );
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
        serial_println!("[elf]   FAIL: ELFOSABI_GNU should be detected as Linux",);
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
        serial_println!("[elf]   FAIL: PT_INTERP=/lib/ld-musl-x86_64.so.1 should detect as Linux",);
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
        serial_println!("[elf]   FAIL: PT_INTERP=/system/loader should NOT detect as Linux",);
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
        serial_println!("[elf]   FAIL: PT_GNU_PROPERTY should detect as Linux",);
        return Err(KernelError::InternalError);
    }
    serial_println!("[elf]   Detect Linux ABI: PT_GNU_PROPERTY: OK");
    Ok(())
}

/// Encode one ELF note: header, name, descriptor, each padded to `align`.
fn encode_note(out: &mut alloc::vec::Vec<u8>, name: &[u8], n_type: u32, desc: &[u8], align: usize) {
    let pad = |v: &mut alloc::vec::Vec<u8>| {
        while !v.len().is_multiple_of(align) {
            v.push(0);
        }
    };
    out.extend_from_slice(&u32::try_from(name.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&u32::try_from(desc.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&n_type.to_le_bytes());
    out.extend_from_slice(name);
    pad(out);
    out.extend_from_slice(desc);
    pad(out);
}

/// Build a tiny ELF whose only program header is a `PT_NOTE` over `notes`,
/// with the given `EI_OSABI` and note alignment. For the native-marker tests.
fn build_note_elf(osabi: u8, notes: &[u8], p_align: u64) -> alloc::vec::Vec<u8> {
    use alloc::vec;

    let phdr_offset: u64 = 64;
    let note_offset: u64 = 64 + ELF64_PHDR_SIZE as u64;
    let note_size = notes.len() as u64;
    let load_vaddr: u64 = 0x0000_0040_0000_0000;
    let mut buf = vec![0u8; (note_offset + note_size) as usize];

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

    let ph = phdr_offset as usize;
    write_u32(&mut buf, ph, PT_NOTE);
    write_u32(&mut buf, ph + 4, PF_R);
    write_u64(&mut buf, ph + 8, note_offset);
    write_u64(&mut buf, ph + 16, load_vaddr);
    write_u64(&mut buf, ph + 24, 0);
    write_u64(&mut buf, ph + 32, note_size);
    write_u64(&mut buf, ph + 40, note_size);
    write_u64(&mut buf, ph + 48, p_align);

    if let Some(dst) = buf.get_mut(note_offset as usize..) {
        dst.copy_from_slice(notes);
    }
    buf
}

/// Test 16b: the SlateOS native marker, in both forms, outranks the Linux
/// signals — and nothing else is mistaken for it.
///
/// The case that matters is the second: `ELFOSABI_GNU` *and* the SlateOS note
/// is exactly what a std-based SlateOS Rust program looks like once marked,
/// because LLVM tags it GNU (see `detect_linux_abi`). Without the precedence
/// it runs on the Linux syscall table and dies at its first native syscall.
fn test_detect_slateos_native_marker() -> KernelResult<()> {
    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("[elf]   FAIL: native marker: {}", what);
        Err(KernelError::InternalError)
    };
    let revision = 1u32.to_le_bytes();

    // Form 1: EI_OSABI = 255.
    let mut data = build_test_elf();
    data[EI_OSABI] = ELFOSABI_SLATEOS;
    let elf = ElfFile::parse(&data)?;
    if !elf.has_slateos_marker() || elf.detect_linux_abi() {
        return fail("EI_OSABI=255 should mark the binary native");
    }

    // Form 2, over the GNU tag our Rust userland actually carries.
    let mut notes = alloc::vec::Vec::new();
    encode_note(&mut notes, SLATEOS_NOTE_NAME, NT_SLATEOS_ABI, &revision, 4);
    let data = build_note_elf(ELFOSABI_GNU, &notes, 4);
    let elf = ElfFile::parse(&data)?;
    if !elf.has_slateos_marker() {
        return fail("a SlateOS note was not found");
    }
    if elf.detect_linux_abi() {
        return fail("OSABI GNU + the SlateOS note must be NATIVE -- the marker outranks the tag");
    }

    // The marker is found after an unrelated note, at both note alignments,
    // which proves the walker steps over a note rather than only reading one.
    for align in [4usize, 8] {
        let mut notes = alloc::vec::Vec::new();
        encode_note(&mut notes, b"GNU\0", 1, &[0u8; 16], align); // NT_GNU_ABI_TAG
        encode_note(
            &mut notes,
            SLATEOS_NOTE_NAME,
            NT_SLATEOS_ABI,
            &revision,
            align,
        );
        let data = build_note_elf(ELFOSABI_GNU, &notes, align as u64);
        if !ElfFile::parse(&data)?.has_slateos_marker() {
            return fail("the SlateOS note after a GNU note was missed");
        }
    }

    // Not the marker: the same owner with another type, another owner with
    // the same type, and the owner without its NUL. Each leaves OSABI GNU in
    // charge, so the binary stays Linux.
    let lookalikes: [(&[u8], u32); 3] = [
        (SLATEOS_NOTE_NAME, 2),
        (b"GNU\0", NT_SLATEOS_ABI),
        (b"SlateOS", NT_SLATEOS_ABI),
    ];
    for (name, ty) in lookalikes {
        let mut notes = alloc::vec::Vec::new();
        encode_note(&mut notes, name, ty, &revision, 4);
        let elf_data = build_note_elf(ELFOSABI_GNU, &notes, 4);
        let elf = ElfFile::parse(&elf_data)?;
        if elf.has_slateos_marker() || !elf.detect_linux_abi() {
            return fail("a lookalike note was taken for the marker");
        }
    }

    // Malformed: a name size that runs off the end. No marker, no panic.
    let mut bad = alloc::vec::Vec::new();
    bad.extend_from_slice(&u32::MAX.to_le_bytes());
    bad.extend_from_slice(&4u32.to_le_bytes());
    bad.extend_from_slice(&NT_SLATEOS_ABI.to_le_bytes());
    bad.extend_from_slice(SLATEOS_NOTE_NAME);
    let data = build_note_elf(ELFOSABI_SYSV, &bad, 4);
    if ElfFile::parse(&data)?.has_slateos_marker() {
        return fail("a malformed note was accepted");
    }

    serial_println!(
        "[elf]   Native marker (OSABI 255, SlateOS note over OSABI GNU, note walk at \
         4/8 alignment, lookalikes, malformed): OK"
    );
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
        b"/lib/ld-elf.so.1\0", // FreeBSD's loader.
        b"/libexec/ld.so\0",   // OpenBSD's loader.
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
