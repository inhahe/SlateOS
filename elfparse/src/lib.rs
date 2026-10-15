//! `elfparse` — the parsing half of SlateOS's ELF64 loader.
//!
//! Reads an x86-64 ELF64 executable from a byte slice and answers what the
//! kernel needs before it commits a single frame to it: whether the file is
//! one it accepts, its entry point, its program headers and loadable
//! segments, the program interpreter it names, which system-call ABI it
//! speaks, and how far its image reaches.
//!
//! The loading half -- frames, page tables, copying segments in -- stays in
//! the kernel (`kernel/src/proc/elf.rs`), which maps [`ElfError`] to its own
//! errors. This half is a crate of its own so that it can be built and fuzzed
//! on the host (`fuzz/`): every byte it reads comes from a program file, and
//! a program file is anybody's.
//!
//! ## Accepted
//!
//! - ELF64, little-endian, version 1 (`EV_CURRENT`), machine `EM_X86_64`;
//! - `ET_EXEC` (static) and `ET_DYN` (PIE, shared object, program
//!   interpreter);
//! - loadable segments that lie within the file and below
//!   [`USER_SPACE_END`].
//!
//! ## Robustness
//!
//! No input makes it panic, loop without end or read outside the slice:
//! every offset is computed with checked arithmetic and every read goes
//! through `get`. A malformed file is an [`ElfError::Malformed`].
//!
//! ## References
//!
//! - System V ABI AMD64 Supplement
//! - ELF-64 Object File Format (TIS, December 1998)
//! - Linux `fs/binfmt_elf.c`

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic)]

// ---------------------------------------------------------------------------
// ELF64 constants
// ---------------------------------------------------------------------------

/// The four bytes every ELF file begins with (`e_ident[0..4]`).
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// `e_ident` index of the file class (32- or 64-bit).
pub const EI_CLASS: usize = 4;
/// `e_ident` index of the byte order.
pub const EI_DATA: usize = 5;
/// `e_ident` index of the identification version.
pub const EI_VERSION: usize = 6;
/// `e_ident` index of the operating-system / ABI identifier.
pub const EI_OSABI: usize = 7;

/// `EI_CLASS` of a 64-bit file.
pub const ELFCLASS64: u8 = 2;
/// `EI_DATA` of a little-endian file.
pub const ELFDATA2LSB: u8 = 1;

/// `EI_OSABI` most toolchains emit whatever the target: a weak signal.
pub const ELFOSABI_SYSV: u8 = 0;
/// `EI_OSABI` of GNU/Linux (`ELFOSABI_LINUX` is the same value): emitted as
/// soon as an object uses a GNU extension.
pub const ELFOSABI_GNU: u8 = 3;
/// SlateOS's own `EI_OSABI`: 255, the top of the range the gABI leaves to
/// the architecture or OS (64-255). One of the two forms of the native
/// marker (design-decisions §33; [`ElfFile::has_slateos_marker`]).
pub const ELFOSABI_SLATEOS: u8 = 255;

/// Owner name of the SlateOS ABI note, NUL-terminated as note names are: the
/// other form of the native marker, a note in a `PT_NOTE` segment.
pub const SLATEOS_NOTE_NAME: &[u8] = b"SlateOS\0";
/// `n_type` of the note that declares "this binary speaks the SlateOS native
/// system-call ABI". Its 4-byte descriptor is the ABI revision, currently 1;
/// any revision is accepted, and the field exists so that a future
/// incompatible ABI can be told apart without a new note type.
pub const NT_SLATEOS_ABI: u32 = 1;

/// `e_type` of a static executable.
pub const ET_EXEC: u16 = 2;
/// `e_type` of a shared object: a PIE, a library, a program interpreter.
pub const ET_DYN: u16 = 3;

/// `e_machine` of x86-64.
pub const EM_X86_64: u16 = 62;

/// `p_type` of a segment to map.
pub const PT_LOAD: u32 = 1;
/// `p_type` of the segment naming the program interpreter.
pub const PT_INTERP: u32 = 3;
/// `p_type` of a segment of notes.
pub const PT_NOTE: u32 = 4;
/// `p_type` of the GNU property note, emitted by binutils and gcc since
/// 2018 and by no BSD toolchain: a strong Linux indicator.
pub const PT_GNU_PROPERTY: u32 = 0x6474_E553;

/// `p_flags` bit: executable.
pub const PF_X: u32 = 0x1;
/// `p_flags` bit: writable.
pub const PF_W: u32 = 0x2;
/// `p_flags` bit: readable.
pub const PF_R: u32 = 0x4;

/// Size of the ELF64 file header.
pub const ELF64_EHDR_SIZE: usize = 64;
/// Size of an ELF64 program header, and the least `e_phentsize` accepted.
pub const ELF64_PHDR_SIZE: usize = 56;
/// Size of an ELF64 section header.
pub const ELF64_SHDR_SIZE: usize = 64;

/// `EI_VERSION` and `e_version` of every ELF file there is.
pub const EV_CURRENT: u8 = 1;

/// The end (exclusive) of x86-64's lower canonical half, where a user
/// program's address space ends: no loadable segment may reach past it.
/// The kernel checks at build time that its own bound is this one.
pub const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a file was refused, or an answer could not be given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    /// The file is not an ELF64 x86-64 executable this loader accepts, or
    /// one of its headers or segments is inconsistent with the file.
    Malformed,
    /// Placing the image at the load bias asked for runs past the end of
    /// the 64-bit address space.
    AddressOverflow,
    /// An alignment that is not a power of two was asked for
    /// ([`ElfFile::image_end`]): the caller's mistake, not the file's.
    BadAlignment,
}

impl core::fmt::Display for ElfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed or unsupported ELF file",
            Self::AddressOverflow => "load bias overflows the address space",
            Self::BadAlignment => "alignment is not a power of two",
        })
    }
}

// ---------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------

/// The ELF64 file header, its fields in native byte order.
#[derive(Debug, Clone, Copy)]
pub struct Elf64Ehdr {
    /// `e_ident[EI_OSABI]`: the operating-system / ABI identifier. See
    /// [`ElfFile::detect_linux_abi`] for what it does and does not say.
    pub e_ident_osabi: u8,
    /// Object file type: [`ET_EXEC`] or [`ET_DYN`] once parsed.
    pub e_type: u16,
    /// Architecture: [`EM_X86_64`] once parsed.
    pub e_machine: u16,
    /// Object file version: 1 once parsed.
    pub e_version: u32,
    /// Virtual address of the entry point (before any load bias).
    pub e_entry: u64,
    /// File offset of the program-header table.
    pub e_phoff: u64,
    /// File offset of the section-header table.
    pub e_shoff: u64,
    /// Processor-specific flags (0 on x86-64).
    pub e_flags: u32,
    /// Size of this header.
    pub e_ehsize: u16,
    /// Size of one program-header entry: at least [`ELF64_PHDR_SIZE`] when
    /// there are any, once parsed.
    pub e_phentsize: u16,
    /// Number of program-header entries.
    pub e_phnum: u16,
    /// Size of one section-header entry.
    pub e_shentsize: u16,
    /// Number of section-header entries.
    pub e_shnum: u16,
    /// Index of the section-name string table.
    pub e_shstrndx: u16,
}

/// One ELF64 program header: a segment.
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    /// Segment type ([`PT_LOAD`], [`PT_INTERP`], ...).
    pub p_type: u32,
    /// Permission bits ([`PF_R`], [`PF_W`], [`PF_X`]).
    pub p_flags: u32,
    /// File offset of the segment's bytes.
    pub p_offset: u64,
    /// Virtual address it is loaded at (before any load bias).
    pub p_vaddr: u64,
    /// Physical address; meaningless with virtual memory.
    pub p_paddr: u64,
    /// Bytes of the segment the file holds.
    pub p_filesz: u64,
    /// Bytes it occupies in memory: the rest past `p_filesz` is zeroed (BSS).
    pub p_memsz: u64,
    /// Alignment: 0 or 1 for none, otherwise a power of two. For a note
    /// segment, the alignment of its notes.
    pub p_align: u64,
}

/// A `PT_LOAD` segment, as the loader maps it.
#[derive(Debug, Clone)]
pub struct LoadableSegment {
    /// Virtual address where it begins (`p_vaddr`, before any load bias).
    pub vaddr: u64,
    /// Bytes to copy from the file.
    pub file_size: u64,
    /// Bytes it occupies in memory, BSS included; never less than
    /// `file_size`.
    pub mem_size: u64,
    /// File offset its bytes start at.
    pub file_offset: u64,
    /// Readable.
    pub readable: bool,
    /// Writable.
    pub writable: bool,
    /// Executable.
    pub executable: bool,
}

// ---------------------------------------------------------------------------
// The parsed file
// ---------------------------------------------------------------------------

/// A parsed ELF64 file: the bytes and their validated file header. Borrows
/// the bytes; allocates nothing.
pub struct ElfFile<'a> {
    data: &'a [u8],
    /// The file header, validated by [`ElfFile::parse`].
    pub header: Elf64Ehdr,
}

impl<'a> ElfFile<'a> {
    /// Parse `data` as an ELF64 x86-64 executable.
    ///
    /// Checks the magic bytes, the class (64-bit), the byte order
    /// (little-endian), both version fields, the machine (x86-64), the type
    /// (`ET_EXEC` or `ET_DYN`), that a program-header entry is at least
    /// [`ELF64_PHDR_SIZE`] bytes when there are any, that the table lies
    /// within the file, and that an `ET_EXEC` has an entry point.
    ///
    /// # Errors
    ///
    /// [`ElfError::Malformed`] if any check fails.
    pub fn parse(data: &'a [u8]) -> Result<Self, ElfError> {
        if data.len() < ELF64_EHDR_SIZE || data.get(..4) != Some(&ELF_MAGIC[..]) {
            return Err(ElfError::Malformed);
        }
        let ident = |i: usize| data.get(i).copied().ok_or(ElfError::Malformed);
        if ident(EI_CLASS)? != ELFCLASS64
            || ident(EI_DATA)? != ELFDATA2LSB
            || ident(EI_VERSION)? != EV_CURRENT
        {
            return Err(ElfError::Malformed);
        }
        let u16_at = |off| read_u16(data, off).ok_or(ElfError::Malformed);
        let u32_at = |off| read_u32(data, off).ok_or(ElfError::Malformed);
        let u64_at = |off| read_u64(data, off).ok_or(ElfError::Malformed);
        let header = Elf64Ehdr {
            e_ident_osabi: ident(EI_OSABI)?,
            e_type: u16_at(16)?,
            e_machine: u16_at(18)?,
            e_version: u32_at(20)?,
            e_entry: u64_at(24)?,
            e_phoff: u64_at(32)?,
            e_shoff: u64_at(40)?,
            e_flags: u32_at(48)?,
            e_ehsize: u16_at(52)?,
            e_phentsize: u16_at(54)?,
            e_phnum: u16_at(56)?,
            e_shentsize: u16_at(58)?,
            e_shnum: u16_at(60)?,
            e_shstrndx: u16_at(62)?,
        };

        if header.e_machine != EM_X86_64
            || (header.e_type != ET_EXEC && header.e_type != ET_DYN)
            || header.e_version != u32::from(EV_CURRENT)
        {
            return Err(ElfError::Malformed);
        }

        // An entry smaller than a program header would have the headers
        // overlap, and a zero-sized one read them all from one offset.
        if header.e_phnum > 0 {
            if usize::from(header.e_phentsize) < ELF64_PHDR_SIZE {
                return Err(ElfError::Malformed);
            }
            let table_len = usize::from(header.e_phnum)
                .checked_mul(usize::from(header.e_phentsize))
                .ok_or(ElfError::Malformed)?;
            let table_end = usize::try_from(header.e_phoff)
                .ok()
                .and_then(|start| start.checked_add(table_len))
                .ok_or(ElfError::Malformed)?;
            if table_end > data.len() {
                return Err(ElfError::Malformed);
            }
        }

        if header.e_type == ET_EXEC && header.e_entry == 0 {
            return Err(ElfError::Malformed);
        }

        Ok(Self { data, header })
    }

    /// The file's bytes, as given to [`ElfFile::parse`].
    #[must_use]
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// The program-header table as the file stores it: `e_phnum` entries of
    /// `e_phentsize` bytes at `e_phoff`. `None` if it runs past the file,
    /// which [`ElfFile::parse`] has already ruled out. What a copy of the
    /// table is made from when no loaded segment holds it.
    #[must_use]
    pub fn phdr_table_bytes(&self) -> Option<&'a [u8]> {
        let start = usize::try_from(self.header.e_phoff).ok()?;
        let len =
            usize::from(self.header.e_phentsize).checked_mul(usize::from(self.header.e_phnum))?;
        self.data.get(start..start.checked_add(len)?)
    }

    /// The virtual address of the entry point, before any load bias.
    #[must_use]
    pub fn entry_point(&self) -> u64 {
        self.header.e_entry
    }

    /// Whether this is a position-independent image (`ET_DYN`).
    #[must_use]
    pub fn is_pie(&self) -> bool {
        self.header.e_type == ET_DYN
    }

    /// How many program headers the file has.
    #[must_use]
    pub fn program_header_count(&self) -> usize {
        usize::from(self.header.e_phnum)
    }

    /// The program header at `index`, or `None` past the last one.
    #[must_use]
    pub fn program_header(&self, index: usize) -> Option<Elf64Phdr> {
        if index >= self.program_header_count() {
            return None;
        }
        let offset = usize::try_from(self.header.e_phoff)
            .ok()?
            .checked_add(index.checked_mul(usize::from(self.header.e_phentsize))?)?;
        let entry = self
            .data
            .get(offset..offset.checked_add(ELF64_PHDR_SIZE)?)?;
        Some(Elf64Phdr {
            p_type: read_u32(entry, 0)?,
            p_flags: read_u32(entry, 4)?,
            p_offset: read_u64(entry, 8)?,
            p_vaddr: read_u64(entry, 16)?,
            p_paddr: read_u64(entry, 24)?,
            p_filesz: read_u64(entry, 32)?,
            p_memsz: read_u64(entry, 40)?,
            p_align: read_u64(entry, 48)?,
        })
    }

    /// The bytes `phdr` says the file holds for its segment
    /// (`p_offset .. p_offset + p_filesz`), or `None` if they run past the
    /// file.
    #[must_use]
    pub fn raw_segment_bytes(&self, phdr: &Elf64Phdr) -> Option<&'a [u8]> {
        let start = usize::try_from(phdr.p_offset).ok()?;
        let len = usize::try_from(phdr.p_filesz).ok()?;
        self.data.get(start..start.checked_add(len)?)
    }

    /// Whether this binary speaks the Linux x86-64 system-call ABI, and so
    /// should have its `syscall`s routed through the kernel's Linux
    /// translation layer rather than the native table.
    ///
    /// ## Signals, most reliable first
    ///
    /// 1. **`e_ident[EI_OSABI]` is [`ELFOSABI_GNU`].** Set by toolchains as
    ///    soon as an object uses a GNU extension (`STT_GNU_IFUNC`, ...).
    ///    Most Linux toolchains leave it [`ELFOSABI_SYSV`], so its absence
    ///    says nothing.
    /// 2. **A `PT_INTERP` naming a Linux dynamic loader**
    ///    ([`is_linux_interp`]): `ld-linux-x86-64` (glibc) or
    ///    `ld-musl-x86_64` (musl). Catches nearly every dynamically linked
    ///    Linux binary, whatever its `EI_OSABI`.
    /// 3. **A [`PT_GNU_PROPERTY`] segment.** Emitted by binutils and gcc
    ///    since 2018 and by no BSD toolchain: catches static GNU/Linux
    ///    binaries built with a recent toolchain.
    ///
    /// `PT_GNU_STACK` and `PT_GNU_RELRO` are deliberately not signals:
    /// FreeBSD's clang emits them too.
    ///
    /// ## Which way it may err
    ///
    /// A native binary taken for a Linux one would have every system call
    /// answered by the wrong table, so the signals are ones a toolchain does
    /// not produce by accident. A Linux binary missed runs under the native
    /// ABI and fails -- no worse than having no Linux ABI at all.
    ///
    /// ## The SlateOS native marker outranks every signal
    ///
    /// A binary carrying [`ElfFile::has_slateos_marker`] is native whatever
    /// else it carries. Signal 1 is not unambiguous for SlateOS's own
    /// toolchain: its target is an LLVM `x86_64-unknown-linux-musl` one,
    /// whose objects are tagged `ELFOSABI_GNU` as soon as they use a GNU
    /// extension, so native programs can arrive with OSABI 3 (`coreutils`'
    /// do). The marker is how design-decisions §33 says native binaries
    /// identify themselves.
    #[must_use]
    pub fn detect_linux_abi(&self) -> bool {
        if self.has_slateos_marker() {
            return false;
        }
        if self.header.e_ident_osabi == ELFOSABI_GNU {
            return true;
        }
        (0..self.program_header_count())
            .filter_map(|i| self.program_header(i))
            .any(|phdr| match phdr.p_type {
                PT_INTERP => self.raw_segment_bytes(&phdr).is_some_and(is_linux_interp),
                PT_GNU_PROPERTY => true,
                _ => false,
            })
    }

    /// Whether this binary declares itself SlateOS-native: the explicit
    /// marker of design-decisions §33, in either of its two forms --
    /// `e_ident[EI_OSABI]` is [`ELFOSABI_SLATEOS`], or a `PT_NOTE` segment
    /// holds a note owned by [`SLATEOS_NOTE_NAME`] of type
    /// [`NT_SLATEOS_ABI`].
    ///
    /// Two forms because they suit different producers: a linker script
    /// keeps a note the C runtime emits, so every binary linked against the
    /// libc carries it with no extra step; a header byte can be stamped
    /// afterwards by whatever stages a binary, for toolchains that cannot be
    /// taught to keep a note.
    #[must_use]
    pub fn has_slateos_marker(&self) -> bool {
        if self.header.e_ident_osabi == ELFOSABI_SLATEOS {
            return true;
        }
        (0..self.program_header_count())
            .filter_map(|i| self.program_header(i))
            .filter(|phdr| phdr.p_type == PT_NOTE)
            .any(|phdr| {
                self.raw_segment_bytes(&phdr).is_some_and(|notes| {
                    notes_contain(notes, phdr.p_align, SLATEOS_NOTE_NAME, NT_SLATEOS_ABI)
                })
            })
    }

    /// The program interpreter the first `PT_INTERP` segment names -- the
    /// dynamic loader the kernel runs in the program's place, such as
    /// `/lib/ld-musl-x86_64.so.1` -- cut at its first NUL.
    ///
    /// `None` for a static binary (no `PT_INTERP`), for a segment that runs
    /// past the file, and for an empty path. Bytes, never `str`: a path may
    /// hold any byte but NUL.
    #[must_use]
    pub fn interp_path(&self) -> Option<&'a [u8]> {
        let phdr = (0..self.program_header_count())
            .filter_map(|i| self.program_header(i))
            .find(|phdr| phdr.p_type == PT_INTERP)?;
        let bytes = self.raw_segment_bytes(&phdr)?;
        // A C string in the file, as Linux's `load_elf_interp` reads it; one
        // with no NUL is taken whole.
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        bytes.get(..end).filter(|path| !path.is_empty())
    }

    /// The `PT_LOAD` segments, all checked before the first is yielded: its
    /// memory size is at least its file size, its bytes lie within the file,
    /// and its addresses neither wrap nor reach past [`USER_SPACE_END`] (for
    /// `ET_DYN` too: a PIE may start at 0, but must still fit).
    ///
    /// # Errors
    ///
    /// [`ElfError::Malformed`] if any `PT_LOAD` segment fails a check, or a
    /// program header cannot be read.
    pub fn loadable_segments(&self) -> Result<LoadableSegments<'_, 'a>, ElfError> {
        for i in 0..self.program_header_count() {
            let phdr = self.program_header(i).ok_or(ElfError::Malformed)?;
            if phdr.p_type != PT_LOAD {
                continue;
            }
            if phdr.p_memsz < phdr.p_filesz {
                return Err(ElfError::Malformed);
            }
            self.raw_segment_bytes(&phdr).ok_or(ElfError::Malformed)?;
            let end = phdr
                .p_vaddr
                .checked_add(phdr.p_memsz)
                .ok_or(ElfError::Malformed)?;
            if end > USER_SPACE_END {
                return Err(ElfError::Malformed);
            }
        }
        Ok(LoadableSegments {
            elf: self,
            index: 0,
        })
    }

    /// The end of the image loaded at `bias`: the highest end of any
    /// loadable segment, rounded up to `align` (a power of two, the
    /// loader's page size). Where a Linux program's `brk` heap begins
    /// (`fs/binfmt_elf.c`, `set_brk`). 0 if there are no loadable segments.
    ///
    /// # Errors
    ///
    /// [`ElfError::Malformed`] as from [`ElfFile::loadable_segments`];
    /// [`ElfError::AddressOverflow`] if the bias or the rounding passes
    /// `u64::MAX`; [`ElfError::BadAlignment`] if `align` is not a power of
    /// two.
    pub fn image_end(&self, bias: u64, align: u64) -> Result<u64, ElfError> {
        if !align.is_power_of_two() {
            return Err(ElfError::BadAlignment);
        }
        let mask = align.wrapping_sub(1);
        let mut highest = 0u64;
        for seg in self.loadable_segments()? {
            let end = seg
                .vaddr
                .checked_add(bias)
                .and_then(|start| start.checked_add(seg.mem_size))
                .and_then(|end| end.checked_add(mask))
                .ok_or(ElfError::AddressOverflow)?
                & !mask;
            highest = highest.max(end);
        }
        Ok(highest)
    }
}

/// The `PT_LOAD` segments of an [`ElfFile`], in program-header order, from
/// [`ElfFile::loadable_segments`]. Segments of no size are skipped.
pub struct LoadableSegments<'e, 'a> {
    elf: &'e ElfFile<'a>,
    index: usize,
}

impl Iterator for LoadableSegments<'_, '_> {
    type Item = LoadableSegment;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.elf.program_header_count() {
            let phdr = self.elf.program_header(self.index)?;
            self.index = self.index.saturating_add(1);
            if phdr.p_type != PT_LOAD || phdr.p_memsz == 0 {
                continue;
            }
            return Some(LoadableSegment {
                vaddr: phdr.p_vaddr,
                file_size: phdr.p_filesz,
                mem_size: phdr.p_memsz,
                file_offset: phdr.p_offset,
                readable: phdr.p_flags & PF_R != 0,
                writable: phdr.p_flags & PF_W != 0,
                executable: phdr.p_flags & PF_X != 0,
            });
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Notes, interpreter names, integers
// ---------------------------------------------------------------------------

/// Whether the note segment image `notes` holds a note owned by `name` (the
/// exact bytes, NUL included) with type `n_type`.
///
/// Each note is `Elf64_Nhdr { n_namesz, n_descsz, n_type }` (three `u32`s),
/// then the name, then the descriptor, each padded to the notes' alignment:
/// 4 for ordinary notes, 8 for `.note.gnu.property`. The segment's `p_align`
/// says which; anything but 8 is taken as 4, which is what every toolchain
/// emits for a 4-aligned note segment.
///
/// Every size in a note is the file's to choose: a malformed note ends the
/// walk with `false`. Each note advances the walk by at least 12 bytes, so it
/// ends within `notes.len() / 12` notes.
#[must_use]
pub fn notes_contain(notes: &[u8], p_align: u64, name: &[u8], n_type: u32) -> bool {
    let align: usize = if p_align == 8 { 8 } else { 4 };
    let mask = align.wrapping_sub(1);
    let pad = |x: usize| x.checked_add(mask).map(|v| v & !mask);
    let mut off = 0usize;
    while off < notes.len() {
        let Some(note) = Note::at(notes, off, pad) else {
            return false;
        };
        if note.n_type == n_type && notes.get(note.name.clone()) == Some(name) {
            return true;
        }
        if note.next <= off {
            return false;
        }
        off = note.next;
    }
    false
}

/// One note of a note segment, as [`notes_contain`] walks them.
struct Note {
    n_type: u32,
    /// Where its name lies in the segment.
    name: core::ops::Range<usize>,
    /// Where the next note begins.
    next: usize,
}

impl Note {
    /// The note at `off` of `notes`, its fields padded by `pad`; `None` if
    /// it runs past the segment.
    fn at(notes: &[u8], off: usize, pad: impl Fn(usize) -> Option<usize>) -> Option<Self> {
        let namesz = usize::try_from(read_u32(notes, off)?).ok()?;
        let descsz = usize::try_from(read_u32(notes, off.checked_add(4)?)?).ok()?;
        let n_type = read_u32(notes, off.checked_add(8)?)?;
        let name_start = off.checked_add(12)?;
        let name_end = name_start.checked_add(namesz)?;
        let desc_end = pad(name_end)?.checked_add(descsz)?;
        if desc_end > notes.len() {
            return None;
        }
        Some(Self {
            n_type,
            name: name_start..name_end,
            next: pad(desc_end)?,
        })
    }
}

/// Whether `bytes`, a `PT_INTERP` segment's image, names a Linux dynamic
/// loader: its path up to the first NUL contains `ld-linux-x86-64` (glibc,
/// as in `/lib64/ld-linux-x86-64.so.2`) or `ld-musl-x86_64` (musl, as in
/// `/lib/ld-musl-x86_64.so.1`). No other x86-64 system ships a loader by
/// either name.
#[must_use]
pub fn is_linux_interp(bytes: &[u8]) -> bool {
    let path = bytes
        .iter()
        .position(|&b| b == 0)
        .and_then(|nul| bytes.get(..nul))
        .unwrap_or(bytes);
    contains_subslice(path, b"ld-linux-x86-64") || contains_subslice(path, b"ld-musl-x86_64")
}

/// Whether `needle` occurs in `haystack`. An empty needle occurs anywhere.
fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

/// The little-endian `u16` at `data[off..]`, or `None` past the end.
fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(off..off.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// The little-endian `u32` at `data[off..]`, or `None` past the end.
fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(off..off.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// The little-endian `u64` at `data[off..]`, or `None` past the end.
fn read_u64(data: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(off..off.checked_add(8)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests;
