//! OurOS ELF Object File Analysis Tools
//!
//! Multi-personality binary that acts as `objdump`, `nm`, or `size` depending
//! on the name used to invoke it (detected via `argv[0]`).
//!
//! # Personalities
//!
//! - **objdump**: disassemble and display information from ELF object files
//! - **nm**: list symbols from ELF object files
//! - **size**: display section sizes of ELF object files
//!
//! # Usage
//!
//! ```text
//! objdump -f binary           # file headers
//! objdump -h binary           # section headers
//! objdump -d binary           # disassemble
//! objdump -t binary           # symbol table
//! objdump -x binary           # all headers
//! nm binary                   # list symbols
//! nm -n binary                # sort by address
//! size binary                 # section sizes (Berkeley)
//! size -A binary              # section sizes (SysV)
//! ```

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(dead_code)]

use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::process;

// ============================================================================
// ELF constants
// ============================================================================

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];

const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;
const EI_OSABI: usize = 7;
const EI_NIDENT: usize = 16;

const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;

const ELFDATA2LSB: u8 = 1;
const ELFDATA2MSB: u8 = 2;

// ELF file types (e_type)
const ET_NONE: u16 = 0;
const ET_REL: u16 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const ET_CORE: u16 = 4;

// Machine architectures (e_machine)
const EM_NONE: u16 = 0;
const EM_386: u16 = 3;
const EM_MIPS: u16 = 8;
const EM_PPC: u16 = 20;
const EM_PPC64: u16 = 21;
const EM_ARM: u16 = 40;
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;
const EM_RISCV: u16 = 243;

// OS/ABI values
const ELFOSABI_NONE: u8 = 0;
const ELFOSABI_LINUX: u8 = 3;
const ELFOSABI_FREEBSD: u8 = 9;

// Program header types (p_type)
const PT_NULL: u32 = 0;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const PT_PHDR: u32 = 6;
const PT_TLS: u32 = 7;
const PT_GNU_STACK: u32 = 0x6474_e551;
const PT_GNU_RELRO: u32 = 0x6474_e552;
const PT_GNU_EH_FRAME: u32 = 0x6474_e550;

// Program header flags
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

// Section header types (sh_type)
const SHT_NULL: u32 = 0;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHT_HASH: u32 = 5;
const SHT_DYNAMIC: u32 = 6;
const SHT_NOTE: u32 = 7;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;
const SHT_DYNSYM: u32 = 11;
const SHT_INIT_ARRAY: u32 = 14;
const SHT_FINI_ARRAY: u32 = 15;
const SHT_GNU_HASH: u32 = 0x6fff_fef5;
const SHT_GNU_VERSYM: u32 = 0x6fff_fff0;
const SHT_GNU_VERNEED: u32 = 0x6fff_fffe;

// Section header flags
const SHF_WRITE: u64 = 1;
const SHF_ALLOC: u64 = 2;
const SHF_EXECINSTR: u64 = 4;
const SHF_MERGE: u64 = 16;
const SHF_STRINGS: u64 = 32;
const SHF_INFO_LINK: u64 = 64;
const SHF_TLS: u64 = 1024;

// Symbol binding (upper nibble of st_info)
const STB_LOCAL: u8 = 0;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;

// Symbol type (lower nibble of st_info)
const STT_NOTYPE: u8 = 0;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const STT_SECTION: u8 = 3;
const STT_FILE: u8 = 4;
const STT_COMMON: u8 = 5;
const STT_TLS: u8 = 6;

// Special section indices
const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xfff1;
const SHN_COMMON: u16 = 0xfff2;

// Relocation types for x86_64
const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_GOT32: u32 = 3;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_32: u32 = 10;
const R_X86_64_32S: u32 = 11;
const R_X86_64_PC64: u32 = 24;

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum Error {
    Io(io::Error),
    NotElf,
    TruncatedHeader,
    TruncatedData {
        what: &'static str,
        offset: usize,
        needed: usize,
        available: usize,
    },
    InvalidClass(u8),
    InvalidEncoding(u8),
    BadUtf8 {
        what: &'static str,
    },
    SectionNotFound(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::NotElf => write!(f, "not an ELF file (bad magic)"),
            Self::TruncatedHeader => write!(f, "file too small to contain ELF header"),
            Self::TruncatedData {
                what,
                offset,
                needed,
                available,
            } => write!(
                f,
                "{what}: truncated data at offset {offset:#x}: need {needed}, have {available}"
            ),
            Self::InvalidClass(c) => write!(f, "unknown ELF class: {c}"),
            Self::InvalidEncoding(e) => write!(f, "unknown ELF data encoding: {e}"),
            Self::BadUtf8 { what } => write!(f, "{what}: contains invalid UTF-8"),
            Self::SectionNotFound(name) => write!(f, "section not found: {name}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

// ============================================================================
// Personality detection
// ============================================================================

/// Which tool personality we are running as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Objdump,
    Nm,
    Size,
}

fn detect_personality() -> Personality {
    let argv0 = env::args().next().unwrap_or_default();
    let name = argv0
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(&argv0);
    // Strip .exe suffix for Windows compatibility
    let name = name.strip_suffix(".exe").unwrap_or(name);
    if name.ends_with("nm") {
        Personality::Nm
    } else if name.ends_with("size") {
        Personality::Size
    } else {
        Personality::Objdump
    }
}

// ============================================================================
// Byte-reader helpers
// ============================================================================

fn read_u16(data: &[u8], offset: usize, le: bool) -> Result<u16> {
    let end = offset.checked_add(2).ok_or(Error::TruncatedData {
        what: "u16",
        offset,
        needed: 2,
        available: data.len(),
    })?;
    if end > data.len() {
        return Err(Error::TruncatedData {
            what: "u16",
            offset,
            needed: 2,
            available: data.len(),
        });
    }
    let bytes = [data[offset], data[offset + 1]];
    Ok(if le {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

fn read_u32(data: &[u8], offset: usize, le: bool) -> Result<u32> {
    let end = offset.checked_add(4).ok_or(Error::TruncatedData {
        what: "u32",
        offset,
        needed: 4,
        available: data.len(),
    })?;
    if end > data.len() {
        return Err(Error::TruncatedData {
            what: "u32",
            offset,
            needed: 4,
            available: data.len(),
        });
    }
    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];
    Ok(if le {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

fn read_i32(data: &[u8], offset: usize, le: bool) -> Result<i32> {
    let end = offset.checked_add(4).ok_or(Error::TruncatedData {
        what: "i32",
        offset,
        needed: 4,
        available: data.len(),
    })?;
    if end > data.len() {
        return Err(Error::TruncatedData {
            what: "i32",
            offset,
            needed: 4,
            available: data.len(),
        });
    }
    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];
    Ok(if le {
        i32::from_le_bytes(bytes)
    } else {
        i32::from_be_bytes(bytes)
    })
}

fn read_u64(data: &[u8], offset: usize, le: bool) -> Result<u64> {
    let end = offset.checked_add(8).ok_or(Error::TruncatedData {
        what: "u64",
        offset,
        needed: 8,
        available: data.len(),
    })?;
    if end > data.len() {
        return Err(Error::TruncatedData {
            what: "u64",
            offset,
            needed: 8,
            available: data.len(),
        });
    }
    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ];
    Ok(if le {
        u64::from_le_bytes(bytes)
    } else {
        u64::from_be_bytes(bytes)
    })
}

fn read_i64(data: &[u8], offset: usize, le: bool) -> Result<i64> {
    let end = offset.checked_add(8).ok_or(Error::TruncatedData {
        what: "i64",
        offset,
        needed: 8,
        available: data.len(),
    })?;
    if end > data.len() {
        return Err(Error::TruncatedData {
            what: "i64",
            offset,
            needed: 8,
            available: data.len(),
        });
    }
    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ];
    Ok(if le {
        i64::from_le_bytes(bytes)
    } else {
        i64::from_be_bytes(bytes)
    })
}

fn read_cstr(data: &[u8], offset: usize) -> Result<&str> {
    if offset >= data.len() {
        return Ok("");
    }
    let bytes = &data[offset..];
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..len]).map_err(|_| Error::BadUtf8 { what: "string" })
}

// ============================================================================
// ELF parsed structures
// ============================================================================

#[derive(Debug, Clone)]
struct ElfHeader {
    class: u8,
    data: u8,
    little_endian: bool,
    osabi: u8,
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[derive(Debug, Clone)]
struct ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[derive(Debug, Clone)]
struct SectionHeader {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
    name: String,
}

#[derive(Debug, Clone)]
struct Symbol {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
    name: String,
}

impl Symbol {
    fn binding(&self) -> u8 {
        self.st_info >> 4
    }
    fn sym_type(&self) -> u8 {
        self.st_info & 0xf
    }
}

#[derive(Debug, Clone)]
struct Relocation {
    r_offset: u64,
    r_info: u64,
    r_addend: Option<i64>,
    sym_name: String,
}

impl Relocation {
    fn sym_idx_64(&self) -> u32 {
        (self.r_info >> 32) as u32
    }
    fn rel_type_64(&self) -> u32 {
        (self.r_info & 0xffff_ffff) as u32
    }
    fn sym_idx_32(&self) -> u32 {
        (self.r_info >> 8) as u32
    }
    fn rel_type_32(&self) -> u32 {
        (self.r_info & 0xff) as u32
    }
}

/// A fully parsed ELF file.
struct ElfFile {
    data: Vec<u8>,
    header: ElfHeader,
    program_headers: Vec<ProgramHeader>,
    sections: Vec<SectionHeader>,
}

// ============================================================================
// ELF parsing
// ============================================================================

fn parse_elf_header(data: &[u8]) -> Result<ElfHeader> {
    if data.len() < EI_NIDENT {
        return Err(Error::TruncatedHeader);
    }
    if data[0..4] != ELFMAG {
        return Err(Error::NotElf);
    }

    let class = data[EI_CLASS];
    if class != ELFCLASS32 && class != ELFCLASS64 {
        return Err(Error::InvalidClass(class));
    }

    let encoding = data[EI_DATA];
    if encoding != ELFDATA2LSB && encoding != ELFDATA2MSB {
        return Err(Error::InvalidEncoding(encoding));
    }
    let le = encoding == ELFDATA2LSB;
    let osabi = data[EI_OSABI];

    let min_size = if class == ELFCLASS64 { 64 } else { 52 };
    if data.len() < min_size {
        return Err(Error::TruncatedHeader);
    }

    if class == ELFCLASS64 {
        Ok(ElfHeader {
            class,
            data: encoding,
            little_endian: le,
            osabi,
            e_type: read_u16(data, 16, le)?,
            e_machine: read_u16(data, 18, le)?,
            e_version: read_u32(data, 20, le)?,
            e_entry: read_u64(data, 24, le)?,
            e_phoff: read_u64(data, 32, le)?,
            e_shoff: read_u64(data, 40, le)?,
            e_flags: read_u32(data, 48, le)?,
            e_ehsize: read_u16(data, 52, le)?,
            e_phentsize: read_u16(data, 54, le)?,
            e_phnum: read_u16(data, 56, le)?,
            e_shentsize: read_u16(data, 58, le)?,
            e_shnum: read_u16(data, 60, le)?,
            e_shstrndx: read_u16(data, 62, le)?,
        })
    } else {
        Ok(ElfHeader {
            class,
            data: encoding,
            little_endian: le,
            osabi,
            e_type: read_u16(data, 16, le)?,
            e_machine: read_u16(data, 18, le)?,
            e_version: read_u32(data, 20, le)?,
            e_entry: read_u32(data, 24, le)? as u64,
            e_phoff: read_u32(data, 28, le)? as u64,
            e_shoff: read_u32(data, 32, le)? as u64,
            e_flags: read_u32(data, 36, le)?,
            e_ehsize: read_u16(data, 40, le)?,
            e_phentsize: read_u16(data, 42, le)?,
            e_phnum: read_u16(data, 44, le)?,
            e_shentsize: read_u16(data, 46, le)?,
            e_shnum: read_u16(data, 48, le)?,
            e_shstrndx: read_u16(data, 50, le)?,
        })
    }
}

fn parse_program_headers(data: &[u8], hdr: &ElfHeader) -> Result<Vec<ProgramHeader>> {
    let mut phdrs = Vec::with_capacity(hdr.e_phnum as usize);
    for i in 0..hdr.e_phnum as usize {
        let off = hdr.e_phoff as usize + i * hdr.e_phentsize as usize;
        let le = hdr.little_endian;
        if hdr.class == ELFCLASS64 {
            phdrs.push(ProgramHeader {
                p_type: read_u32(data, off, le)?,
                p_flags: read_u32(data, off + 4, le)?,
                p_offset: read_u64(data, off + 8, le)?,
                p_vaddr: read_u64(data, off + 16, le)?,
                p_paddr: read_u64(data, off + 24, le)?,
                p_filesz: read_u64(data, off + 32, le)?,
                p_memsz: read_u64(data, off + 40, le)?,
                p_align: read_u64(data, off + 48, le)?,
            });
        } else {
            phdrs.push(ProgramHeader {
                p_type: read_u32(data, off, le)?,
                p_offset: read_u32(data, off + 4, le)? as u64,
                p_vaddr: read_u32(data, off + 8, le)? as u64,
                p_paddr: read_u32(data, off + 12, le)? as u64,
                p_filesz: read_u32(data, off + 16, le)? as u64,
                p_memsz: read_u32(data, off + 20, le)? as u64,
                p_flags: read_u32(data, off + 24, le)?,
                p_align: read_u32(data, off + 28, le)? as u64,
            });
        }
    }
    Ok(phdrs)
}

fn parse_section_headers(data: &[u8], hdr: &ElfHeader) -> Result<Vec<SectionHeader>> {
    let mut shdrs = Vec::with_capacity(hdr.e_shnum as usize);
    for i in 0..hdr.e_shnum as usize {
        let off = hdr.e_shoff as usize + i * hdr.e_shentsize as usize;
        let le = hdr.little_endian;
        if hdr.class == ELFCLASS64 {
            shdrs.push(SectionHeader {
                sh_name: read_u32(data, off, le)?,
                sh_type: read_u32(data, off + 4, le)?,
                sh_flags: read_u64(data, off + 8, le)?,
                sh_addr: read_u64(data, off + 16, le)?,
                sh_offset: read_u64(data, off + 24, le)?,
                sh_size: read_u64(data, off + 32, le)?,
                sh_link: read_u32(data, off + 40, le)?,
                sh_info: read_u32(data, off + 44, le)?,
                sh_addralign: read_u64(data, off + 48, le)?,
                sh_entsize: read_u64(data, off + 56, le)?,
                name: String::new(),
            });
        } else {
            shdrs.push(SectionHeader {
                sh_name: read_u32(data, off, le)?,
                sh_type: read_u32(data, off + 4, le)?,
                sh_flags: read_u32(data, off + 8, le)? as u64,
                sh_addr: read_u32(data, off + 12, le)? as u64,
                sh_offset: read_u32(data, off + 16, le)? as u64,
                sh_size: read_u32(data, off + 20, le)? as u64,
                sh_link: read_u32(data, off + 24, le)?,
                sh_info: read_u32(data, off + 28, le)?,
                sh_addralign: read_u32(data, off + 32, le)? as u64,
                sh_entsize: read_u32(data, off + 36, le)? as u64,
                name: String::new(),
            });
        }
    }

    // Resolve section names from shstrtab
    if (hdr.e_shstrndx as usize) < shdrs.len() {
        let strtab_off = shdrs[hdr.e_shstrndx as usize].sh_offset as usize;
        let strtab_size = shdrs[hdr.e_shstrndx as usize].sh_size as usize;
        if strtab_off + strtab_size <= data.len() {
            let strtab = &data[strtab_off..strtab_off + strtab_size];
            for shdr in &mut shdrs {
                if (shdr.sh_name as usize) < strtab.len() {
                    if let Ok(n) = read_cstr(strtab, shdr.sh_name as usize) {
                        shdr.name = n.to_string();
                    }
                }
            }
        }
    }

    Ok(shdrs)
}

fn parse_symbols(
    data: &[u8],
    hdr: &ElfHeader,
    symtab: &SectionHeader,
    sections: &[SectionHeader],
) -> Result<Vec<Symbol>> {
    let le = hdr.little_endian;
    let strtab_idx = symtab.sh_link as usize;
    let strtab_data: &[u8] = if strtab_idx < sections.len() {
        let st = &sections[strtab_idx];
        let start = st.sh_offset as usize;
        let end = start + st.sh_size as usize;
        if end <= data.len() {
            &data[start..end]
        } else {
            &[]
        }
    } else {
        &[]
    };

    let entry_size = if symtab.sh_entsize > 0 {
        symtab.sh_entsize as usize
    } else if hdr.class == ELFCLASS64 {
        24
    } else {
        16
    };

    let count = if entry_size > 0 {
        symtab.sh_size as usize / entry_size
    } else {
        0
    };

    let mut syms = Vec::with_capacity(count);
    for i in 0..count {
        let off = symtab.sh_offset as usize + i * entry_size;
        let sym = if hdr.class == ELFCLASS64 {
            Symbol {
                st_name: read_u32(data, off, le)?,
                st_info: data.get(off + 4).copied().unwrap_or(0),
                st_other: data.get(off + 5).copied().unwrap_or(0),
                st_shndx: read_u16(data, off + 6, le)?,
                st_value: read_u64(data, off + 8, le)?,
                st_size: read_u64(data, off + 16, le)?,
                name: String::new(),
            }
        } else {
            Symbol {
                st_name: read_u32(data, off, le)?,
                st_value: read_u32(data, off + 4, le)? as u64,
                st_size: read_u32(data, off + 8, le)? as u64,
                st_info: data.get(off + 12).copied().unwrap_or(0),
                st_other: data.get(off + 13).copied().unwrap_or(0),
                st_shndx: read_u16(data, off + 14, le)?,
                name: String::new(),
            }
        };
        syms.push(sym);
    }

    // Resolve symbol names
    for sym in &mut syms {
        if sym.st_name > 0 && (sym.st_name as usize) < strtab_data.len() {
            if let Ok(n) = read_cstr(strtab_data, sym.st_name as usize) {
                sym.name = n.to_string();
            }
        }
    }

    Ok(syms)
}

fn parse_relocations(
    data: &[u8],
    hdr: &ElfHeader,
    rel_section: &SectionHeader,
    sections: &[SectionHeader],
) -> Result<Vec<Relocation>> {
    let le = hdr.little_endian;
    let is_rela = rel_section.sh_type == SHT_RELA;

    // Get associated symbol table for name resolution
    let sym_section = sections.get(rel_section.sh_link as usize);
    let symbols = if let Some(ss) = sym_section {
        parse_symbols(data, hdr, ss, sections).unwrap_or_default()
    } else {
        Vec::new()
    };

    let entry_size = if rel_section.sh_entsize > 0 {
        rel_section.sh_entsize as usize
    } else if hdr.class == ELFCLASS64 {
        if is_rela { 24 } else { 16 }
    } else if is_rela {
        12
    } else {
        8
    };

    let count = if entry_size > 0 {
        rel_section.sh_size as usize / entry_size
    } else {
        0
    };

    let mut relocs = Vec::with_capacity(count);
    for i in 0..count {
        let off = rel_section.sh_offset as usize + i * entry_size;
        let (r_offset, r_info, r_addend) = if hdr.class == ELFCLASS64 {
            let offset = read_u64(data, off, le)?;
            let info = read_u64(data, off + 8, le)?;
            let addend = if is_rela {
                Some(read_i64(data, off + 16, le)?)
            } else {
                None
            };
            (offset, info, addend)
        } else {
            let offset = read_u32(data, off, le)? as u64;
            let info = read_u32(data, off + 4, le)? as u64;
            let addend = if is_rela {
                Some(read_i32(data, off + 8, le)? as i64)
            } else {
                None
            };
            (offset, info, addend)
        };

        let sym_idx = if hdr.class == ELFCLASS64 {
            (r_info >> 32) as usize
        } else {
            (r_info >> 8) as usize
        };

        let sym_name = symbols
            .get(sym_idx)
            .map(|s| s.name.clone())
            .unwrap_or_default();

        relocs.push(Relocation {
            r_offset,
            r_info,
            r_addend,
            sym_name,
        });
    }

    Ok(relocs)
}

fn parse_elf(data: Vec<u8>) -> Result<ElfFile> {
    let header = parse_elf_header(&data)?;
    let program_headers = parse_program_headers(&data, &header)?;
    let sections = parse_section_headers(&data, &header)?;
    Ok(ElfFile {
        data,
        header,
        program_headers,
        sections,
    })
}

// ============================================================================
// Name helpers
// ============================================================================

fn file_type_str(t: u16) -> &'static str {
    match t {
        ET_NONE => "NONE (No file type)",
        ET_REL => "REL (Relocatable file)",
        ET_EXEC => "EXEC (Executable file)",
        ET_DYN => "DYN (Shared object file)",
        ET_CORE => "CORE (Core file)",
        _ => "Unknown",
    }
}

fn machine_str(m: u16) -> &'static str {
    match m {
        EM_NONE => "None",
        EM_386 => "Intel 80386",
        EM_MIPS => "MIPS RS3000",
        EM_PPC => "PowerPC",
        EM_PPC64 => "PowerPC64",
        EM_ARM => "ARM",
        EM_X86_64 => "Advanced Micro Devices X86-64",
        EM_AARCH64 => "AArch64",
        EM_RISCV => "RISC-V",
        _ => "Unknown",
    }
}

fn osabi_str(o: u8) -> &'static str {
    match o {
        ELFOSABI_NONE => "UNIX - System V",
        ELFOSABI_LINUX => "UNIX - Linux",
        ELFOSABI_FREEBSD => "UNIX - FreeBSD",
        255 => "OurOS",
        _ => "Unknown",
    }
}

fn section_type_str(t: u32) -> &'static str {
    match t {
        SHT_NULL => "NULL",
        SHT_PROGBITS => "PROGBITS",
        SHT_SYMTAB => "SYMTAB",
        SHT_STRTAB => "STRTAB",
        SHT_RELA => "RELA",
        SHT_HASH => "HASH",
        SHT_DYNAMIC => "DYNAMIC",
        SHT_NOTE => "NOTE",
        SHT_NOBITS => "NOBITS",
        SHT_REL => "REL",
        SHT_DYNSYM => "DYNSYM",
        SHT_INIT_ARRAY => "INIT_ARRAY",
        SHT_FINI_ARRAY => "FINI_ARRAY",
        SHT_GNU_HASH => "GNU_HASH",
        SHT_GNU_VERSYM => "GNU_VERSYM",
        SHT_GNU_VERNEED => "GNU_VERNEED",
        _ => "UNKNOWN",
    }
}

fn phdr_type_str(t: u32) -> &'static str {
    match t {
        PT_NULL => "NULL",
        PT_LOAD => "LOAD",
        PT_DYNAMIC => "DYNAMIC",
        PT_INTERP => "INTERP",
        PT_NOTE => "NOTE",
        PT_PHDR => "PHDR",
        PT_TLS => "TLS",
        PT_GNU_STACK => "GNU_STACK",
        PT_GNU_RELRO => "GNU_RELRO",
        PT_GNU_EH_FRAME => "GNU_EH_FRAME",
        _ => "UNKNOWN",
    }
}

fn reloc_type_str_x86_64(t: u32) -> &'static str {
    match t {
        R_X86_64_NONE => "R_X86_64_NONE",
        R_X86_64_64 => "R_X86_64_64",
        R_X86_64_PC32 => "R_X86_64_PC32",
        R_X86_64_GOT32 => "R_X86_64_GOT32",
        R_X86_64_PLT32 => "R_X86_64_PLT32",
        R_X86_64_GLOB_DAT => "R_X86_64_GLOB_DAT",
        R_X86_64_JUMP_SLOT => "R_X86_64_JUMP_SLOT",
        R_X86_64_RELATIVE => "R_X86_64_RELATIVE",
        R_X86_64_32 => "R_X86_64_32",
        R_X86_64_32S => "R_X86_64_32S",
        R_X86_64_PC64 => "R_X86_64_PC64",
        _ => "UNKNOWN",
    }
}

/// Classify symbol for nm output (T/t, D/d, B/b, U, W/w, A, R/r, etc.)
fn nm_symbol_type(sym: &Symbol, sections: &[SectionHeader]) -> char {
    if sym.st_shndx == SHN_UNDEF {
        if sym.binding() == STB_WEAK {
            return 'w';
        }
        return 'U';
    }
    if sym.st_shndx == SHN_ABS {
        return if sym.binding() == STB_LOCAL { 'a' } else { 'A' };
    }
    if sym.st_shndx == SHN_COMMON {
        return if sym.binding() == STB_LOCAL { 'c' } else { 'C' };
    }
    if sym.binding() == STB_WEAK {
        // Weak defined symbol
        if sym.sym_type() == STT_OBJECT {
            return 'V';
        }
        return 'W';
    }

    let is_global = sym.binding() == STB_GLOBAL;

    // Look at the section to determine type
    if let Some(sec) = sections.get(sym.st_shndx as usize) {
        let flags = sec.sh_flags;
        let sh_type = sec.sh_type;

        if sh_type == SHT_NOBITS {
            return if is_global { 'B' } else { 'b' };
        }
        if flags & SHF_EXECINSTR != 0 {
            return if is_global { 'T' } else { 't' };
        }
        if flags & SHF_WRITE != 0 {
            return if is_global { 'D' } else { 'd' };
        }
        if flags & SHF_ALLOC != 0 {
            return if is_global { 'R' } else { 'r' };
        }
        // Non-allocatable section
        return if is_global { 'N' } else { 'n' };
    }

    // Fallback based on symbol type
    match sym.sym_type() {
        STT_FUNC => {
            if is_global {
                'T'
            } else {
                't'
            }
        }
        STT_OBJECT => {
            if is_global {
                'D'
            } else {
                'd'
            }
        }
        _ => '?',
    }
}

fn section_flags_str(flags: u64) -> String {
    let mut s = String::new();
    if flags & SHF_WRITE != 0 {
        s.push('W');
    }
    if flags & SHF_ALLOC != 0 {
        s.push('A');
    }
    if flags & SHF_EXECINSTR != 0 {
        s.push('X');
    }
    if flags & SHF_MERGE != 0 {
        s.push('M');
    }
    if flags & SHF_STRINGS != 0 {
        s.push('S');
    }
    if flags & SHF_INFO_LINK != 0 {
        s.push('I');
    }
    if flags & SHF_TLS != 0 {
        s.push('T');
    }
    s
}

fn phdr_flags_str(flags: u32) -> String {
    let mut s = String::with_capacity(3);
    s.push(if flags & PF_R != 0 { 'R' } else { ' ' });
    s.push(if flags & PF_W != 0 { 'W' } else { ' ' });
    s.push(if flags & PF_X != 0 { 'E' } else { ' ' });
    s
}

// ============================================================================
// x86_64 basic disassembler
// ============================================================================

/// Disassemble one x86_64 instruction at `code[offset..]`.
/// Returns (mnemonic string, number of bytes consumed).
fn disasm_one(code: &[u8], offset: usize, base_addr: u64) -> (String, usize) {
    if offset >= code.len() {
        return ("(end)".to_string(), 0);
    }

    let remaining = &code[offset..];
    if remaining.is_empty() {
        return ("(end)".to_string(), 0);
    }

    let b0 = remaining[0];

    // REX prefix detection
    let has_rex_w = (0x48..=0x4f).contains(&b0);
    let actual = if has_rex_w && remaining.len() > 1 {
        &remaining[1..]
    } else {
        remaining
    };
    let rex_offset = if has_rex_w { 1usize } else { 0usize };

    if actual.is_empty() {
        let hex: String = remaining.iter().map(|b| format!("{b:02x} ")).collect();
        return (format!(".byte  {hex}"), 1);
    }

    let op = actual[0];

    // NOP (single-byte)
    if !has_rex_w && op == 0x90 {
        return ("nop".to_string(), 1);
    }

    // Multi-byte NOP (0x0f 0x1f ...)
    if op == 0x0f && actual.len() > 1 && actual[1] == 0x1f {
        // Figure out the length from the mod/rm byte
        if actual.len() > 2 {
            let modrm = actual[2];
            let modrm_mod = modrm >> 6;
            let rm = modrm & 7;
            let base_len = 3 + rex_offset;
            let extra = match modrm_mod {
                0 => {
                    if rm == 4 {
                        1
                    } else {
                        0
                    }
                } // SIB
                1 => {
                    if rm == 4 {
                        2
                    } else {
                        1
                    }
                } // SIB + disp8
                2 => {
                    if rm == 4 {
                        5
                    } else {
                        4
                    }
                } // SIB + disp32
                _ => 0,
            };
            let total = base_len + extra;
            let total = total.min(remaining.len());
            return (format!("nop    ({}B)", total), total);
        }
        return ("nop    (multi)".to_string(), 2 + rex_offset);
    }

    // RET
    if op == 0xc3 {
        return ("ret".to_string(), 1 + rex_offset);
    }
    // RET imm16
    if op == 0xc2 && actual.len() >= 3 {
        let imm = u16::from_le_bytes([actual[1], actual[2]]);
        return (format!("ret    {imm:#x}"), 3 + rex_offset);
    }

    // INT imm8
    if op == 0xcd && actual.len() >= 2 {
        return (format!("int    {:#x}", actual[1]), 2 + rex_offset);
    }
    // INT3
    if op == 0xcc {
        return ("int3".to_string(), 1 + rex_offset);
    }

    // SYSCALL (0x0f 0x05)
    if op == 0x0f && actual.len() > 1 && actual[1] == 0x05 {
        return ("syscall".to_string(), 2 + rex_offset);
    }
    // SYSRET (0x0f 0x07)
    if op == 0x0f && actual.len() > 1 && actual[1] == 0x07 {
        return ("sysret".to_string(), 2 + rex_offset);
    }

    // HLT
    if op == 0xf4 {
        return ("hlt".to_string(), 1 + rex_offset);
    }

    // CLI / STI
    if op == 0xfa {
        return ("cli".to_string(), 1 + rex_offset);
    }
    if op == 0xfb {
        return ("sti".to_string(), 1 + rex_offset);
    }

    // PUSH r64 (0x50-0x57)
    if (0x50..=0x57).contains(&op) {
        let reg_names = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
        let reg_names_ext = ["r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];
        let idx = (op - 0x50) as usize;
        let name = if has_rex_w && (b0 & 1) != 0 {
            reg_names_ext.get(idx).unwrap_or(&"???")
        } else {
            reg_names.get(idx).unwrap_or(&"???")
        };
        return (format!("push   {name}"), 1 + rex_offset);
    }

    // POP r64 (0x58-0x5f)
    if (0x58..=0x5f).contains(&op) {
        let reg_names = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
        let reg_names_ext = ["r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];
        let idx = (op - 0x58) as usize;
        let name = if has_rex_w && (b0 & 1) != 0 {
            reg_names_ext.get(idx).unwrap_or(&"???")
        } else {
            reg_names.get(idx).unwrap_or(&"???")
        };
        return (format!("pop    {name}"), 1 + rex_offset);
    }

    // CALL rel32 (0xe8)
    if op == 0xe8 && actual.len() >= 5 {
        let rel = i32::from_le_bytes([actual[1], actual[2], actual[3], actual[4]]);
        let target =
            (base_addr as i64 + (offset + 5 + rex_offset) as i64 + rel as i64) as u64;
        return (format!("call   {target:#x}"), 5 + rex_offset);
    }

    // JMP rel32 (0xe9)
    if op == 0xe9 && actual.len() >= 5 {
        let rel = i32::from_le_bytes([actual[1], actual[2], actual[3], actual[4]]);
        let target =
            (base_addr as i64 + (offset + 5 + rex_offset) as i64 + rel as i64) as u64;
        return (format!("jmp    {target:#x}"), 5 + rex_offset);
    }

    // JMP rel8 (0xeb)
    if op == 0xeb && actual.len() >= 2 {
        let rel = actual[1] as i8;
        let target =
            (base_addr as i64 + (offset + 2 + rex_offset) as i64 + rel as i64) as u64;
        return (format!("jmp    {target:#x}"), 2 + rex_offset);
    }

    // Jcc rel8 (0x70-0x7f)
    if (0x70..=0x7f).contains(&op) && actual.len() >= 2 {
        let cc_names = [
            "jo", "jno", "jb", "jnb", "jz", "jnz", "jbe", "jnbe", "js", "jns", "jp",
            "jnp", "jl", "jnl", "jle", "jnle",
        ];
        let cc = (op - 0x70) as usize;
        let rel = actual[1] as i8;
        let target =
            (base_addr as i64 + (offset + 2 + rex_offset) as i64 + rel as i64) as u64;
        let mnem = cc_names.get(cc).unwrap_or(&"j??");
        return (format!("{mnem:<6} {target:#x}"), 2 + rex_offset);
    }

    // Jcc rel32 (0x0f 0x80-0x8f)
    if op == 0x0f && actual.len() > 1 && (0x80..=0x8f).contains(&actual[1]) {
        let cc_names = [
            "jo", "jno", "jb", "jnb", "jz", "jnz", "jbe", "jnbe", "js", "jns", "jp",
            "jnp", "jl", "jnl", "jle", "jnle",
        ];
        let cc = (actual[1] - 0x80) as usize;
        if actual.len() >= 6 {
            let rel = i32::from_le_bytes([actual[2], actual[3], actual[4], actual[5]]);
            let target = (base_addr as i64
                + (offset + 6 + rex_offset) as i64
                + rel as i64) as u64;
            let mnem = cc_names.get(cc).unwrap_or(&"j??");
            return (format!("{mnem:<6} {target:#x}"), 6 + rex_offset);
        }
    }

    // MOV r64, imm64 (0xb8-0xbf with REX.W)
    if has_rex_w && (0xb8..=0xbf).contains(&op) && actual.len() >= 9 {
        let reg_names = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
        let reg_names_ext = ["r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];
        let idx = (op - 0xb8) as usize;
        let name = if (b0 & 1) != 0 {
            reg_names_ext.get(idx).unwrap_or(&"???")
        } else {
            reg_names.get(idx).unwrap_or(&"???")
        };
        let imm = u64::from_le_bytes([
            actual[1], actual[2], actual[3], actual[4], actual[5], actual[6], actual[7],
            actual[8],
        ]);
        return (format!("movabs {name},{imm:#x}"), 10);
    }

    // MOV r32, imm32 (0xb8-0xbf without REX.W)
    if !has_rex_w && (0xb8..=0xbf).contains(&op) && remaining.len() >= 5 {
        let reg_names = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
        let idx = (op - 0xb8) as usize;
        let name = reg_names.get(idx).unwrap_or(&"???");
        let imm = u32::from_le_bytes([remaining[1], remaining[2], remaining[3], remaining[4]]);
        return (format!("mov    {name},{imm:#x}"), 5);
    }

    // MOV r/m64, r64 (0x89) and MOV r64, r/m64 (0x8b) - register-to-register only
    if (op == 0x89 || op == 0x8b) && actual.len() >= 2 {
        let modrm = actual[1];
        let modrm_mod = modrm >> 6;
        let reg_field = ((modrm >> 3) & 7) as usize;
        let rm_field = (modrm & 7) as usize;
        if modrm_mod == 3 {
            // Register direct
            let reg_names_64 = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
            let reg_names_32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
            let names = if has_rex_w {
                &reg_names_64[..]
            } else {
                &reg_names_32[..]
            };
            let r = names.get(reg_field).unwrap_or(&"???");
            let m = names.get(rm_field).unwrap_or(&"???");
            if op == 0x89 {
                return (format!("mov    {m},{r}"), 2 + rex_offset);
            }
            return (format!("mov    {r},{m}"), 2 + rex_offset);
        }
    }

    // XOR r/m, r (0x31) - register form for common xor reg,reg
    if op == 0x31 && actual.len() >= 2 {
        let modrm = actual[1];
        let modrm_mod = modrm >> 6;
        if modrm_mod == 3 {
            let reg_field = ((modrm >> 3) & 7) as usize;
            let rm_field = (modrm & 7) as usize;
            let reg_names_64 = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
            let reg_names_32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
            let names = if has_rex_w {
                &reg_names_64[..]
            } else {
                &reg_names_32[..]
            };
            let r = names.get(reg_field).unwrap_or(&"???");
            let m = names.get(rm_field).unwrap_or(&"???");
            return (format!("xor    {m},{r}"), 2 + rex_offset);
        }
    }

    // SUB r/m, imm8 (0x83 /5)
    if op == 0x83 && actual.len() >= 3 {
        let modrm = actual[1];
        let modrm_mod = modrm >> 6;
        let op_ext = (modrm >> 3) & 7;
        let rm_field = (modrm & 7) as usize;
        if modrm_mod == 3 {
            let reg_names_64 = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
            let reg_names_32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
            let names = if has_rex_w {
                &reg_names_64[..]
            } else {
                &reg_names_32[..]
            };
            let r = names.get(rm_field).unwrap_or(&"???");
            let imm = actual[2] as i8;
            let op_name = match op_ext {
                0 => "add",
                1 => "or",
                4 => "and",
                5 => "sub",
                7 => "cmp",
                _ => "???",
            };
            return (format!("{op_name:<6} {r},{imm:#x}"), 3 + rex_offset);
        }
    }

    // LEA (0x8d)
    if op == 0x8d && actual.len() >= 2 {
        let modrm = actual[1];
        let reg_field = ((modrm >> 3) & 7) as usize;
        static REG64: [&str; 8] = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
        static REG32: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
        let r = if has_rex_w {
            REG64.get(reg_field).unwrap_or(&"???")
        } else {
            REG32.get(reg_field).unwrap_or(&"???")
        };
        // Simplified: just show lea with register dest
        return (format!("lea    {r},[...]"), 2 + rex_offset);
    }

    // LEAVE (0xc9)
    if op == 0xc9 {
        return ("leave".to_string(), 1 + rex_offset);
    }

    // CLD/STD
    if op == 0xfc {
        return ("cld".to_string(), 1 + rex_offset);
    }
    if op == 0xfd {
        return ("std".to_string(), 1 + rex_offset);
    }

    // ENDBR64 (0xf3 0x0f 0x1e 0xfa)
    if !has_rex_w
        && b0 == 0xf3
        && remaining.len() >= 4
        && remaining[1] == 0x0f
        && remaining[2] == 0x1e
        && remaining[3] == 0xfa
    {
        return ("endbr64".to_string(), 4);
    }

    // Fallback: hex dump of unrecognized byte
    let hex: String = remaining
        .iter()
        .take(1)
        .map(|b| format!("{b:02x}"))
        .collect();
    (format!(".byte  0x{hex}"), 1)
}

// ============================================================================
// Objdump options
// ============================================================================

#[derive(Default)]
struct ObjdumpOpts {
    file_headers: bool,
    section_headers: bool,
    disassemble: bool,
    syms: bool,
    dynamic_syms: bool,
    reloc: bool,
    dynamic_reloc: bool,
    all_headers: bool,
    full_contents: bool,
    private_headers: bool,
    section_filter: Option<String>,
    start_address: Option<u64>,
    stop_address: Option<u64>,
    files: Vec<String>,
}

fn parse_addr(s: &str) -> Option<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_objdump_args() -> ObjdumpOpts {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = ObjdumpOpts::default();

    if args.is_empty() {
        eprintln!("Usage: objdump <option(s)> <file(s)>");
        eprintln!("  -f  Display file header");
        eprintln!("  -h  Display section headers");
        eprintln!("  -d  Disassemble executable sections");
        eprintln!("  -t  Display symbol table");
        eprintln!("  -T  Display dynamic symbol table");
        eprintln!("  -r  Display relocations");
        eprintln!("  -R  Display dynamic relocations");
        eprintln!("  -x  Display all headers");
        eprintln!("  -s  Display full contents (hex dump)");
        eprintln!("  -p  Display program headers");
        eprintln!("  -j <section>  Restrict to section");
        eprintln!("  --start-address=ADDR");
        eprintln!("  --stop-address=ADDR");
        process::exit(1);
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        // Long options
        if let Some(rest) = arg.strip_prefix("--") {
            if rest == "file-headers" {
                opts.file_headers = true;
            } else if rest == "section-headers" || rest == "headers" {
                opts.section_headers = true;
            } else if rest == "disassemble" {
                opts.disassemble = true;
            } else if rest == "syms" {
                opts.syms = true;
            } else if rest == "dynamic-syms" {
                opts.dynamic_syms = true;
            } else if rest == "reloc" {
                opts.reloc = true;
            } else if rest == "dynamic-reloc" {
                opts.dynamic_reloc = true;
            } else if rest == "all-headers" {
                opts.all_headers = true;
            } else if rest == "full-contents" {
                opts.full_contents = true;
            } else if rest == "private-headers" {
                opts.private_headers = true;
            } else if let Some(val) = rest.strip_prefix("section=") {
                opts.section_filter = Some(val.to_string());
            } else if let Some(val) = rest.strip_prefix("start-address=") {
                opts.start_address = parse_addr(val);
            } else if let Some(val) = rest.strip_prefix("stop-address=") {
                opts.stop_address = parse_addr(val);
            }
            i += 1;
            continue;
        }

        // Short options
        if arg.starts_with('-') && arg.len() > 1 {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                match chars[j] {
                    'f' => opts.file_headers = true,
                    'h' => opts.section_headers = true,
                    'd' => opts.disassemble = true,
                    't' => opts.syms = true,
                    'T' => opts.dynamic_syms = true,
                    'r' => opts.reloc = true,
                    'R' => opts.dynamic_reloc = true,
                    'x' => opts.all_headers = true,
                    's' => opts.full_contents = true,
                    'p' => opts.private_headers = true,
                    'j' => {
                        // -j section or -jsection
                        let rest_str: String = chars[j + 1..].iter().collect();
                        if rest_str.is_empty() {
                            i += 1;
                            if i < args.len() {
                                opts.section_filter = Some(args[i].clone());
                            }
                        } else {
                            opts.section_filter = Some(rest_str);
                        }
                        j = chars.len(); // consumed rest
                        continue;
                    }
                    _ => {}
                }
                j += 1;
            }
            i += 1;
            continue;
        }

        // File argument
        opts.files.push(arg.clone());
        i += 1;
    }

    // -x implies file + section + syms + reloc + dynamic + program headers
    if opts.all_headers {
        opts.file_headers = true;
        opts.section_headers = true;
        opts.syms = true;
        opts.reloc = true;
        opts.dynamic_syms = true;
        opts.dynamic_reloc = true;
        opts.private_headers = true;
    }

    opts
}

// ============================================================================
// nm options
// ============================================================================

#[derive(Default)]
struct NmOpts {
    numeric_sort: bool,
    reverse_sort: bool,
    no_sort: bool,
    extern_only: bool,
    undefined_only: bool,
    dynamic: bool,
    print_file_name: bool,
    print_size: bool,
    radix: char, // 'x', 'd', 'o'
    files: Vec<String>,
}

fn parse_nm_args() -> NmOpts {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = NmOpts {
        radix: 'x',
        ..NmOpts::default()
    };

    if args.is_empty() {
        eprintln!("Usage: nm [option(s)] [file(s)]");
        eprintln!("  -n  Sort by address");
        eprintln!("  -r  Reverse sort");
        eprintln!("  -p  No sort");
        eprintln!("  -g  External symbols only");
        eprintln!("  -u  Undefined symbols only");
        eprintln!("  -D  Dynamic symbols");
        eprintln!("  -A  Print file name");
        eprintln!("  -S  Print symbol size");
        eprintln!("  -t <radix>  Output radix (d, o, x)");
        process::exit(1);
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if let Some(rest) = arg.strip_prefix("--") {
            match rest {
                "numeric-sort" => opts.numeric_sort = true,
                "reverse-sort" => opts.reverse_sort = true,
                "no-sort" => opts.no_sort = true,
                "extern-only" => opts.extern_only = true,
                "undefined-only" => opts.undefined_only = true,
                "dynamic" => opts.dynamic = true,
                "print-file-name" => opts.print_file_name = true,
                "print-size" => opts.print_size = true,
                _ => {
                    if let Some(val) = rest.strip_prefix("radix=") {
                        if let Some(c) = val.chars().next() {
                            opts.radix = c;
                        }
                    }
                }
            }
            i += 1;
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                match chars[j] {
                    'n' => opts.numeric_sort = true,
                    'r' => opts.reverse_sort = true,
                    'p' => opts.no_sort = true,
                    'g' => opts.extern_only = true,
                    'u' => opts.undefined_only = true,
                    'D' => opts.dynamic = true,
                    'A' | 'o' => opts.print_file_name = true,
                    'S' => opts.print_size = true,
                    't' => {
                        // -t radix
                        let rest_str: String = chars[j + 1..].iter().collect();
                        if rest_str.is_empty() {
                            i += 1;
                            if i < args.len() {
                                if let Some(c) = args[i].chars().next() {
                                    opts.radix = c;
                                }
                            }
                        } else {
                            if let Some(c) = rest_str.chars().next() {
                                opts.radix = c;
                            }
                        }
                        j = chars.len();
                        continue;
                    }
                    _ => {}
                }
                j += 1;
            }
            i += 1;
            continue;
        }

        opts.files.push(arg.clone());
        i += 1;
    }

    opts
}

// ============================================================================
// size options
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum SizeFormat {
    Berkeley,
    SysV,
}

#[derive(Default)]
struct SizeOpts {
    format: Option<SizeFormat>,
    totals: bool,
    radix: u32, // 10, 8, or 16
    files: Vec<String>,
}

fn parse_size_args() -> SizeOpts {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = SizeOpts {
        format: None,
        totals: false,
        radix: 10,
        files: Vec::new(),
    };

    if args.is_empty() {
        eprintln!("Usage: size [option(s)] [file(s)]");
        eprintln!("  -A  SysV format");
        eprintln!("  -B  Berkeley format (default)");
        eprintln!("  -t  Show totals");
        eprintln!("  -d  Decimal radix");
        eprintln!("  -o  Octal radix");
        eprintln!("  -x  Hex radix");
        eprintln!("  --radix=N  Radix (8, 10, 16)");
        process::exit(1);
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if let Some(rest) = arg.strip_prefix("--") {
            if rest == "format=sysv" {
                opts.format = Some(SizeFormat::SysV);
            } else if rest == "format=berkeley" {
                opts.format = Some(SizeFormat::Berkeley);
            } else if rest == "totals" {
                opts.totals = true;
            } else if let Some(val) = rest.strip_prefix("radix=") {
                opts.radix = val.parse().unwrap_or(10);
            }
            i += 1;
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'A' => opts.format = Some(SizeFormat::SysV),
                    'B' => opts.format = Some(SizeFormat::Berkeley),
                    't' => opts.totals = true,
                    'd' => opts.radix = 10,
                    'o' => opts.radix = 8,
                    'x' => opts.radix = 16,
                    _ => {}
                }
            }
            i += 1;
            continue;
        }

        opts.files.push(arg.clone());
        i += 1;
    }

    opts
}

// ============================================================================
// Objdump display
// ============================================================================

fn display_file_header(w: &mut impl Write, elf: &ElfFile, filename: &str) -> Result<()> {
    let h = &elf.header;
    writeln!(w)?;
    writeln!(w, "{filename}:     file format elf{}-{}",
        if h.class == ELFCLASS64 { "64" } else { "32" },
        if h.little_endian { "little" } else { "big" })?;
    writeln!(w, "architecture: {}, flags 0x{:08x}:", machine_str(h.e_machine), h.e_flags)?;
    writeln!(w, "start address 0x{:016x}", h.e_entry)?;
    writeln!(w)?;
    writeln!(w, "ELF Header:")?;
    writeln!(w, "  Type:                              {}", file_type_str(h.e_type))?;
    writeln!(w, "  Machine:                           {}", machine_str(h.e_machine))?;
    writeln!(w, "  Version:                           0x{:x}", h.e_version)?;
    writeln!(w, "  Entry point address:               0x{:x}", h.e_entry)?;
    writeln!(w, "  Start of program headers:          {} (bytes into file)", h.e_phoff)?;
    writeln!(w, "  Start of section headers:          {} (bytes into file)", h.e_shoff)?;
    writeln!(w, "  Flags:                             0x{:x}", h.e_flags)?;
    writeln!(w, "  Size of this header:               {} (bytes)", h.e_ehsize)?;
    writeln!(w, "  Size of program headers:           {} (bytes)", h.e_phentsize)?;
    writeln!(w, "  Number of program headers:         {}", h.e_phnum)?;
    writeln!(w, "  Size of section headers:           {} (bytes)", h.e_shentsize)?;
    writeln!(w, "  Number of section headers:         {}", h.e_shnum)?;
    writeln!(w, "  Section header string table index: {}", h.e_shstrndx)?;
    writeln!(w, "  OS/ABI:                            {}", osabi_str(h.osabi))?;
    Ok(())
}

fn display_section_headers(w: &mut impl Write, elf: &ElfFile, _filename: &str) -> Result<()> {
    writeln!(w)?;
    writeln!(w, "Sections:")?;
    writeln!(w, "Idx Name          Size      VMA               LMA               File off  Algn  Flags")?;
    for (i, s) in elf.sections.iter().enumerate() {
        writeln!(
            w,
            "{:3} {:13} {:08x}  {:016x}  {:016x}  {:08x}  2**{:<2} {}",
            i,
            if s.name.is_empty() {
                "(none)"
            } else {
                &s.name
            },
            s.sh_size,
            s.sh_addr,
            s.sh_addr,
            s.sh_offset,
            if s.sh_addralign > 0 {
                (s.sh_addralign as f64).log2() as u32
            } else {
                0
            },
            section_flags_str(s.sh_flags),
        )?;
    }
    Ok(())
}

fn display_program_headers(w: &mut impl Write, elf: &ElfFile) -> Result<()> {
    writeln!(w)?;
    writeln!(w, "Program Header:")?;
    writeln!(
        w,
        "  Type           Offset             VirtAddr           PhysAddr           FileSiz            MemSiz             Flags  Align"
    )?;
    for ph in &elf.program_headers {
        writeln!(
            w,
            "  {:14} 0x{:016x} 0x{:016x} 0x{:016x} 0x{:016x} 0x{:016x} {}  0x{:x}",
            phdr_type_str(ph.p_type),
            ph.p_offset,
            ph.p_vaddr,
            ph.p_paddr,
            ph.p_filesz,
            ph.p_memsz,
            phdr_flags_str(ph.p_flags),
            ph.p_align,
        )?;
    }
    Ok(())
}

fn display_symbols(w: &mut impl Write, elf: &ElfFile, _filename: &str, dynamic: bool) -> Result<()> {
    let target_type = if dynamic { SHT_DYNSYM } else { SHT_SYMTAB };
    let label = if dynamic {
        "DYNAMIC SYMBOL TABLE"
    } else {
        "SYMBOL TABLE"
    };

    for sec in &elf.sections {
        if sec.sh_type != target_type {
            continue;
        }
        let syms = parse_symbols(&elf.data, &elf.header, sec, &elf.sections)?;
        writeln!(w)?;
        writeln!(w, "{label}:")?;

        for sym in &syms {
            let bind = match sym.binding() {
                STB_LOCAL => "l",
                STB_GLOBAL => "g",
                STB_WEAK => "w",
                _ => " ",
            };
            let kind = match sym.sym_type() {
                STT_NOTYPE => " ",
                STT_OBJECT => "O",
                STT_FUNC => "F",
                STT_SECTION => "S",
                STT_FILE => "f",
                STT_COMMON => "C",
                STT_TLS => "T",
                _ => " ",
            };
            let sec_name = if sym.st_shndx == SHN_UNDEF {
                "*UND*"
            } else if sym.st_shndx == SHN_ABS {
                "*ABS*"
            } else if sym.st_shndx == SHN_COMMON {
                "*COM*"
            } else {
                elf.sections
                    .get(sym.st_shndx as usize)
                    .map(|s| s.name.as_str())
                    .unwrap_or("???")
            };
            writeln!(
                w,
                "{:016x} {}{} {:13} {:08x} {}",
                sym.st_value, bind, kind, sec_name, sym.st_size, sym.name,
            )?;
        }
    }
    Ok(())
}

fn display_relocations(
    w: &mut impl Write,
    elf: &ElfFile,
    dynamic_only: bool,
) -> Result<()> {
    for sec in &elf.sections {
        if sec.sh_type != SHT_REL && sec.sh_type != SHT_RELA {
            continue;
        }
        // For dynamic relocations, check if the associated symtab is DYNSYM
        if dynamic_only {
            let linked = elf.sections.get(sec.sh_link as usize);
            if linked.map_or(true, |s| s.sh_type != SHT_DYNSYM) {
                continue;
            }
        } else {
            let linked = elf.sections.get(sec.sh_link as usize);
            if linked.map_or(false, |s| s.sh_type == SHT_DYNSYM) {
                continue;
            }
        }

        let relocs = parse_relocations(&elf.data, &elf.header, sec, &elf.sections)?;
        writeln!(w)?;
        writeln!(
            w,
            "RELOCATION RECORDS FOR [{}]:",
            sec.name
        )?;
        writeln!(w, "OFFSET           TYPE              VALUE")?;
        for r in &relocs {
            let (_sym_idx, rtype) = if elf.header.class == ELFCLASS64 {
                (r.sym_idx_64(), r.rel_type_64())
            } else {
                (r.sym_idx_32(), r.rel_type_32())
            };
            let type_str = if elf.header.e_machine == EM_X86_64 {
                reloc_type_str_x86_64(rtype)
            } else {
                "UNKNOWN"
            };
            let addend_str = if let Some(a) = r.r_addend {
                if a >= 0 {
                    format!("+{a:#x}")
                } else {
                    format!("{a:#x}")
                }
            } else {
                String::new()
            };
            writeln!(
                w,
                "{:016x} {:17} {}{}",
                r.r_offset, type_str, r.sym_name, addend_str,
            )?;
        }
    }
    Ok(())
}

fn display_disassembly(
    w: &mut impl Write,
    elf: &ElfFile,
    filter: Option<&str>,
    start_addr: Option<u64>,
    stop_addr: Option<u64>,
) -> Result<()> {
    for sec in &elf.sections {
        // Only disassemble executable sections (or .text-like)
        if sec.sh_flags & SHF_EXECINSTR == 0 && sec.sh_type != SHT_PROGBITS {
            continue;
        }
        if sec.sh_flags & SHF_EXECINSTR == 0 {
            continue;
        }
        if let Some(f) = filter {
            if sec.name != f {
                continue;
            }
        }

        let sec_start = sec.sh_offset as usize;
        let sec_size = sec.sh_size as usize;
        if sec_start + sec_size > elf.data.len() {
            continue;
        }
        let code = &elf.data[sec_start..sec_start + sec_size];
        let base = sec.sh_addr;

        writeln!(w)?;
        writeln!(w, "Disassembly of section {}:", sec.name)?;
        writeln!(w)?;

        let mut offset = 0;
        while offset < code.len() {
            let addr = base + offset as u64;
            if let Some(start) = start_addr {
                if addr < start {
                    offset += 1;
                    continue;
                }
            }
            if let Some(stop) = stop_addr {
                if addr >= stop {
                    break;
                }
            }

            let (mnemonic, consumed) = disasm_one(code, offset, base);
            if consumed == 0 {
                break;
            }

            // Print hex bytes
            let hex: String = code[offset..offset + consumed]
                .iter()
                .map(|b| format!("{b:02x} "))
                .collect();
            writeln!(w, "  {:8x}:\t{:24}\t{}", addr, hex.trim_end(), mnemonic)?;
            offset += consumed;
        }
    }
    Ok(())
}

fn display_full_contents(
    w: &mut impl Write,
    elf: &ElfFile,
    filter: Option<&str>,
) -> Result<()> {
    for sec in &elf.sections {
        if sec.sh_type == SHT_NULL || sec.sh_type == SHT_NOBITS {
            continue;
        }
        if let Some(f) = filter {
            if sec.name != f {
                continue;
            }
        }

        let sec_start = sec.sh_offset as usize;
        let sec_size = sec.sh_size as usize;
        if sec_size == 0 {
            continue;
        }
        if sec_start + sec_size > elf.data.len() {
            continue;
        }
        let sec_data = &elf.data[sec_start..sec_start + sec_size];

        writeln!(w)?;
        writeln!(w, "Contents of section {}:", sec.name)?;

        let mut offset = 0;
        while offset < sec_data.len() {
            let addr = sec.sh_addr + offset as u64;
            let chunk_len = (sec_data.len() - offset).min(16);
            let chunk = &sec_data[offset..offset + chunk_len];

            // Hex part
            let mut hex = String::new();
            for (i, &b) in chunk.iter().enumerate() {
                if i > 0 && i % 4 == 0 {
                    hex.push(' ');
                }
                hex.push_str(&format!("{b:02x}"));
            }

            // ASCII part
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if b >= 0x20 && b < 0x7f {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();

            writeln!(w, " {:04x} {:36}  {}", addr, hex, ascii)?;
            offset += chunk_len;
        }
    }
    Ok(())
}

fn run_objdump() -> Result<()> {
    let opts = parse_objdump_args();
    let stdout = io::stdout();
    let mut w = BufWriter::new(stdout.lock());

    for filename in &opts.files {
        let mut file = File::open(filename).map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("{filename}: {e}"),
            ))
        })?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        let elf = parse_elf(data)?;

        if opts.file_headers {
            display_file_header(&mut w, &elf, filename)?;
        }
        if opts.private_headers {
            display_program_headers(&mut w, &elf)?;
        }
        if opts.section_headers {
            display_section_headers(&mut w, &elf, filename)?;
        }
        if opts.syms {
            display_symbols(&mut w, &elf, filename, false)?;
        }
        if opts.dynamic_syms {
            display_symbols(&mut w, &elf, filename, true)?;
        }
        if opts.reloc {
            display_relocations(&mut w, &elf, false)?;
        }
        if opts.dynamic_reloc {
            display_relocations(&mut w, &elf, true)?;
        }
        if opts.disassemble {
            display_disassembly(
                &mut w,
                &elf,
                opts.section_filter.as_deref(),
                opts.start_address,
                opts.stop_address,
            )?;
        }
        if opts.full_contents {
            display_full_contents(&mut w, &elf, opts.section_filter.as_deref())?;
        }
    }
    Ok(())
}

// ============================================================================
// nm display
// ============================================================================

fn format_nm_value(val: u64, radix: char) -> String {
    match radix {
        'd' => format!("{:016}", val),
        'o' => format!("{:022o}", val),
        _ => format!("{:016x}", val),
    }
}

fn run_nm() -> Result<()> {
    let opts = parse_nm_args();
    let stdout = io::stdout();
    let mut w = BufWriter::new(stdout.lock());

    for filename in &opts.files {
        let mut file = File::open(filename).map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("{filename}: {e}"),
            ))
        })?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        let elf = parse_elf(data)?;

        let target_type = if opts.dynamic { SHT_DYNSYM } else { SHT_SYMTAB };

        for sec in &elf.sections {
            if sec.sh_type != target_type {
                continue;
            }

            let mut syms = parse_symbols(&elf.data, &elf.header, sec, &elf.sections)?;

            // Filter
            if opts.extern_only {
                syms.retain(|s| s.binding() == STB_GLOBAL || s.binding() == STB_WEAK);
            }
            if opts.undefined_only {
                syms.retain(|s| s.st_shndx == SHN_UNDEF);
            }

            // Skip null symbol at index 0
            if !syms.is_empty() && syms[0].name.is_empty() && syms[0].st_value == 0 {
                syms.remove(0);
            }

            // Sort
            if opts.no_sort {
                // No sorting
            } else if opts.numeric_sort {
                syms.sort_by(|a, b| a.st_value.cmp(&b.st_value));
            } else {
                // Default: sort by name
                syms.sort_by(|a, b| a.name.cmp(&b.name));
            }

            if opts.reverse_sort {
                syms.reverse();
            }

            for sym in &syms {
                let sym_char = nm_symbol_type(sym, &elf.sections);

                let prefix = if opts.print_file_name {
                    format!("{filename}:")
                } else {
                    String::new()
                };

                if sym.st_shndx == SHN_UNDEF {
                    write!(w, "{prefix}{:16} ", "")?;
                } else {
                    write!(
                        w,
                        "{prefix}{} ",
                        format_nm_value(sym.st_value, opts.radix),
                    )?;
                }

                if opts.print_size {
                    write!(
                        w,
                        "{} ",
                        format_nm_value(sym.st_size, opts.radix),
                    )?;
                }

                writeln!(w, "{sym_char} {}", sym.name)?;
            }
        }
    }
    Ok(())
}

// ============================================================================
// size display
// ============================================================================

fn format_size_val(val: u64, radix: u32) -> String {
    match radix {
        8 => format!("0{val:o}"),
        16 => format!("0x{val:x}"),
        _ => format!("{val}"),
    }
}

fn run_size() -> Result<()> {
    let opts = parse_size_args();
    let stdout = io::stdout();
    let mut w = BufWriter::new(stdout.lock());

    let format = opts.format.unwrap_or(SizeFormat::Berkeley);

    let mut total_text: u64 = 0;
    let mut total_data: u64 = 0;
    let mut total_bss: u64 = 0;

    for filename in &opts.files {
        let mut file = File::open(filename).map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("{filename}: {e}"),
            ))
        })?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        let elf = parse_elf(data)?;

        match format {
            SizeFormat::Berkeley => {
                let mut text_size: u64 = 0;
                let mut data_size: u64 = 0;
                let mut bss_size: u64 = 0;

                for sec in &elf.sections {
                    if sec.sh_flags & SHF_ALLOC == 0 {
                        continue;
                    }
                    if sec.sh_type == SHT_NOBITS {
                        bss_size += sec.sh_size;
                    } else if sec.sh_flags & SHF_EXECINSTR != 0 {
                        text_size += sec.sh_size;
                    } else if sec.sh_flags & SHF_WRITE != 0 {
                        data_size += sec.sh_size;
                    } else {
                        // Read-only data counts as text in Berkeley format
                        text_size += sec.sh_size;
                    }
                }

                let dec_total = text_size + data_size + bss_size;

                if filename == &opts.files[0] {
                    writeln!(
                        w,
                        "   text\t   data\t    bss\t    dec\t    hex\tfilename"
                    )?;
                }
                writeln!(
                    w,
                    "{:>7}\t{:>7}\t{:>7}\t{:>7}\t{:>7x}\t{}",
                    format_size_val(text_size, opts.radix),
                    format_size_val(data_size, opts.radix),
                    format_size_val(bss_size, opts.radix),
                    dec_total,
                    dec_total,
                    filename,
                )?;

                total_text += text_size;
                total_data += data_size;
                total_bss += bss_size;
            }
            SizeFormat::SysV => {
                writeln!(w, "{filename}  :")?;
                writeln!(w, "section           size      addr")?;

                let mut total: u64 = 0;
                for sec in &elf.sections {
                    if sec.sh_type == SHT_NULL {
                        continue;
                    }
                    if sec.sh_flags & SHF_ALLOC == 0 && sec.sh_type != SHT_SYMTAB && sec.sh_type != SHT_STRTAB {
                        continue;
                    }
                    writeln!(
                        w,
                        "{:17} {:>10}  {:>10}",
                        sec.name,
                        format_size_val(sec.sh_size, opts.radix),
                        format_size_val(sec.sh_addr, opts.radix),
                    )?;
                    total += sec.sh_size;
                }
                writeln!(
                    w,
                    "Total             {:>10}",
                    format_size_val(total, opts.radix),
                )?;
                writeln!(w)?;
            }
        }
    }

    if opts.totals && format == SizeFormat::Berkeley && opts.files.len() > 1 {
        let dec_total = total_text + total_data + total_bss;
        writeln!(
            w,
            "{:>7}\t{:>7}\t{:>7}\t{:>7}\t{:>7x}\t(TOTALS)",
            format_size_val(total_text, opts.radix),
            format_size_val(total_data, opts.radix),
            format_size_val(total_bss, opts.radix),
            dec_total,
            dec_total,
        )?;
    }

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let personality = detect_personality();
    let result = match personality {
        Personality::Objdump => run_objdump(),
        Personality::Nm => run_nm(),
        Personality::Size => run_size(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --------------- Test ELF data builders ---------------

    /// Build a minimal ELF64 LE header (64 bytes).
    fn make_elf64_header(
        e_type: u16,
        e_machine: u16,
        e_entry: u64,
        e_phoff: u64,
        e_shoff: u64,
        e_phnum: u16,
        e_shnum: u16,
        e_shstrndx: u16,
    ) -> Vec<u8> {
        let mut h = vec![0u8; 64];
        // Magic
        h[0] = 0x7f;
        h[1] = b'E';
        h[2] = b'L';
        h[3] = b'F';
        h[EI_CLASS] = ELFCLASS64;
        h[EI_DATA] = ELFDATA2LSB;
        h[6] = 1; // version
        h[EI_OSABI] = ELFOSABI_NONE;
        // e_type
        h[16..18].copy_from_slice(&e_type.to_le_bytes());
        // e_machine
        h[18..20].copy_from_slice(&e_machine.to_le_bytes());
        // e_version
        h[20..24].copy_from_slice(&1u32.to_le_bytes());
        // e_entry
        h[24..32].copy_from_slice(&e_entry.to_le_bytes());
        // e_phoff
        h[32..40].copy_from_slice(&e_phoff.to_le_bytes());
        // e_shoff
        h[40..48].copy_from_slice(&e_shoff.to_le_bytes());
        // e_flags
        h[48..52].copy_from_slice(&0u32.to_le_bytes());
        // e_ehsize
        h[52..54].copy_from_slice(&64u16.to_le_bytes());
        // e_phentsize
        h[54..56].copy_from_slice(&56u16.to_le_bytes());
        // e_phnum
        h[56..58].copy_from_slice(&e_phnum.to_le_bytes());
        // e_shentsize
        h[58..60].copy_from_slice(&64u16.to_le_bytes());
        // e_shnum
        h[60..62].copy_from_slice(&e_shnum.to_le_bytes());
        // e_shstrndx
        h[62..64].copy_from_slice(&e_shstrndx.to_le_bytes());
        h
    }

    /// Build a minimal ELF32 LE header (52 bytes).
    fn make_elf32_header(
        e_type: u16,
        e_machine: u16,
        e_entry: u32,
        e_shoff: u32,
        e_shnum: u16,
        e_shstrndx: u16,
    ) -> Vec<u8> {
        let mut h = vec![0u8; 52];
        h[0] = 0x7f;
        h[1] = b'E';
        h[2] = b'L';
        h[3] = b'F';
        h[EI_CLASS] = ELFCLASS32;
        h[EI_DATA] = ELFDATA2LSB;
        h[6] = 1;
        h[EI_OSABI] = ELFOSABI_NONE;
        h[16..18].copy_from_slice(&e_type.to_le_bytes());
        h[18..20].copy_from_slice(&e_machine.to_le_bytes());
        h[20..24].copy_from_slice(&1u32.to_le_bytes());
        h[24..28].copy_from_slice(&e_entry.to_le_bytes());
        h[28..32].copy_from_slice(&0u32.to_le_bytes()); // e_phoff
        h[32..36].copy_from_slice(&e_shoff.to_le_bytes());
        h[36..40].copy_from_slice(&0u32.to_le_bytes()); // e_flags
        h[40..42].copy_from_slice(&52u16.to_le_bytes()); // e_ehsize
        h[42..44].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
        h[44..46].copy_from_slice(&0u16.to_le_bytes()); // e_phnum
        h[46..48].copy_from_slice(&40u16.to_le_bytes()); // e_shentsize
        h[48..50].copy_from_slice(&e_shnum.to_le_bytes());
        h[50..52].copy_from_slice(&e_shstrndx.to_le_bytes());
        h
    }

    /// Append a 64-byte ELF64 section header to `buf`.
    fn append_shdr64(
        buf: &mut Vec<u8>,
        sh_name: u32,
        sh_type: u32,
        sh_flags: u64,
        sh_addr: u64,
        sh_offset: u64,
        sh_size: u64,
        sh_link: u32,
        sh_info: u32,
        sh_addralign: u64,
        sh_entsize: u64,
    ) {
        buf.extend_from_slice(&sh_name.to_le_bytes());
        buf.extend_from_slice(&sh_type.to_le_bytes());
        buf.extend_from_slice(&sh_flags.to_le_bytes());
        buf.extend_from_slice(&sh_addr.to_le_bytes());
        buf.extend_from_slice(&sh_offset.to_le_bytes());
        buf.extend_from_slice(&sh_size.to_le_bytes());
        buf.extend_from_slice(&sh_link.to_le_bytes());
        buf.extend_from_slice(&sh_info.to_le_bytes());
        buf.extend_from_slice(&sh_addralign.to_le_bytes());
        buf.extend_from_slice(&sh_entsize.to_le_bytes());
    }

    /// Append a 40-byte ELF32 section header to `buf`.
    fn append_shdr32(
        buf: &mut Vec<u8>,
        sh_name: u32,
        sh_type: u32,
        sh_flags: u32,
        sh_addr: u32,
        sh_offset: u32,
        sh_size: u32,
        sh_link: u32,
        sh_info: u32,
        sh_addralign: u32,
        sh_entsize: u32,
    ) {
        buf.extend_from_slice(&sh_name.to_le_bytes());
        buf.extend_from_slice(&sh_type.to_le_bytes());
        buf.extend_from_slice(&sh_flags.to_le_bytes());
        buf.extend_from_slice(&sh_addr.to_le_bytes());
        buf.extend_from_slice(&sh_offset.to_le_bytes());
        buf.extend_from_slice(&sh_size.to_le_bytes());
        buf.extend_from_slice(&sh_link.to_le_bytes());
        buf.extend_from_slice(&sh_info.to_le_bytes());
        buf.extend_from_slice(&sh_addralign.to_le_bytes());
        buf.extend_from_slice(&sh_entsize.to_le_bytes());
    }

    /// Append an ELF64 symbol table entry (24 bytes) to `buf`.
    fn append_sym64(
        buf: &mut Vec<u8>,
        st_name: u32,
        st_info: u8,
        st_other: u8,
        st_shndx: u16,
        st_value: u64,
        st_size: u64,
    ) {
        buf.extend_from_slice(&st_name.to_le_bytes());
        buf.push(st_info);
        buf.push(st_other);
        buf.extend_from_slice(&st_shndx.to_le_bytes());
        buf.extend_from_slice(&st_value.to_le_bytes());
        buf.extend_from_slice(&st_size.to_le_bytes());
    }

    /// Build a complete minimal test ELF64 with sections and symbols.
    fn make_test_elf64() -> Vec<u8> {
        // Layout:
        //   0x000: ELF header (64 bytes)
        //   0x040: .text section data (16 bytes of NOPs + RET)
        //   0x050: .data section data (8 bytes)
        //   0x058: .bss (NOBITS, 16 bytes)
        //   0x058: .shstrtab data
        //   0x0xx: .strtab data
        //   0x0xx: .symtab data
        //   section headers start after

        // Section name strings: \0.text\0.data\0.bss\0.shstrtab\0.strtab\0.symtab\0
        let shstrtab: Vec<u8> = b"\0.text\0.data\0.bss\0.shstrtab\0.strtab\0.symtab\0".to_vec();
        // name offsets: .text=1, .data=7, .bss=13, .shstrtab=18, .strtab=29, .symtab=37

        // Symbol name strings: \0main\0data_val\0bss_zero\0
        let strtab: Vec<u8> = b"\0main\0data_val\0bss_zero\0".to_vec();
        // name offsets: main=1, data_val=6, bss_zero=15

        // .text data: series of NOPs + RET
        let text_data: Vec<u8> = vec![
            0x90, 0x90, 0x90, 0x90, // 4 NOPs
            0x55, // push rbp
            0x48, 0x89, 0xe5, // mov rbp, rsp (REX.W + mov r/m64, r64)
            0xc3, // ret
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding
        ];

        // .data content
        let data_data: Vec<u8> = vec![0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

        // Build the file
        let text_offset: u64 = 64;
        let text_size: u64 = text_data.len() as u64;
        let data_offset: u64 = text_offset + text_size;
        let data_size: u64 = data_data.len() as u64;
        let bss_size: u64 = 16;
        let shstrtab_offset: u64 = data_offset + data_size;
        let shstrtab_size: u64 = shstrtab.len() as u64;
        let strtab_offset: u64 = shstrtab_offset + shstrtab_size;
        let strtab_size: u64 = strtab.len() as u64;

        // Build symbol table
        let mut symtab_data = Vec::new();
        // Symbol 0: null
        append_sym64(&mut symtab_data, 0, 0, 0, SHN_UNDEF, 0, 0);
        // Symbol 1: main (global, func, in .text section idx=1, addr=0x1004)
        let main_info = (STB_GLOBAL << 4) | STT_FUNC;
        append_sym64(&mut symtab_data, 1, main_info, 0, 1, 0x1004, 5);
        // Symbol 2: data_val (global, object, in .data section idx=2, addr=0x2000)
        let data_info = (STB_GLOBAL << 4) | STT_OBJECT;
        append_sym64(&mut symtab_data, 6, data_info, 0, 2, 0x2000, 8);
        // Symbol 3: bss_zero (global, object, in .bss section idx=3, addr=0x3000)
        let bss_info = (STB_GLOBAL << 4) | STT_OBJECT;
        append_sym64(&mut symtab_data, 15, bss_info, 0, 3, 0x3000, 16);

        let symtab_offset: u64 = strtab_offset + strtab_size;
        let symtab_size: u64 = symtab_data.len() as u64;

        // Section headers start after symtab
        let shoff: u64 = symtab_offset + symtab_size;
        // Sections: 0=NULL, 1=.text, 2=.data, 3=.bss, 4=.shstrtab, 5=.strtab, 6=.symtab
        let e_shnum: u16 = 7;
        let e_shstrndx: u16 = 4;

        let mut file = make_elf64_header(
            ET_EXEC,
            EM_X86_64,
            0x1000,
            0,     // no program headers for simplicity
            shoff,
            0,     // phnum
            e_shnum,
            e_shstrndx,
        );

        // Append section data
        file.extend_from_slice(&text_data);
        file.extend_from_slice(&data_data);
        file.extend_from_slice(&shstrtab);
        file.extend_from_slice(&strtab);
        file.extend_from_slice(&symtab_data);

        // Now append section headers
        // 0: NULL
        append_shdr64(&mut file, 0, SHT_NULL, 0, 0, 0, 0, 0, 0, 0, 0);
        // 1: .text
        append_shdr64(
            &mut file,
            1, // name offset in shstrtab
            SHT_PROGBITS,
            SHF_ALLOC | SHF_EXECINSTR,
            0x1000,
            text_offset,
            text_size,
            0,
            0,
            16,
            0,
        );
        // 2: .data
        append_shdr64(
            &mut file,
            7, // name offset
            SHT_PROGBITS,
            SHF_ALLOC | SHF_WRITE,
            0x2000,
            data_offset,
            data_size,
            0,
            0,
            8,
            0,
        );
        // 3: .bss
        append_shdr64(
            &mut file,
            13, // name offset
            SHT_NOBITS,
            SHF_ALLOC | SHF_WRITE,
            0x3000,
            0, // NOBITS has no file data
            bss_size,
            0,
            0,
            16,
            0,
        );
        // 4: .shstrtab
        append_shdr64(
            &mut file,
            18,
            SHT_STRTAB,
            0,
            0,
            shstrtab_offset,
            shstrtab_size,
            0,
            0,
            1,
            0,
        );
        // 5: .strtab
        append_shdr64(
            &mut file,
            29,
            SHT_STRTAB,
            0,
            0,
            strtab_offset,
            strtab_size,
            0,
            0,
            1,
            0,
        );
        // 6: .symtab (link=5 for strtab, info=1 for first global symbol)
        append_shdr64(
            &mut file,
            37,
            SHT_SYMTAB,
            0,
            0,
            symtab_offset,
            symtab_size,
            5, // link to .strtab
            1, // info: first non-local symbol
            8,
            24, // entsize for ELF64 symbol
        );

        file
    }

    // --------------- ELF header parsing tests ---------------

    #[test]
    fn test_parse_elf64_header() {
        let data = make_test_elf64();
        let hdr = parse_elf_header(&data).unwrap();
        assert_eq!(hdr.class, ELFCLASS64);
        assert_eq!(hdr.data, ELFDATA2LSB);
        assert!(hdr.little_endian);
        assert_eq!(hdr.e_type, ET_EXEC);
        assert_eq!(hdr.e_machine, EM_X86_64);
        assert_eq!(hdr.e_entry, 0x1000);
    }

    #[test]
    fn test_parse_elf32_header() {
        let data = make_elf32_header(ET_REL, EM_386, 0, 52, 0, 0);
        let hdr = parse_elf_header(&data).unwrap();
        assert_eq!(hdr.class, ELFCLASS32);
        assert_eq!(hdr.e_type, ET_REL);
        assert_eq!(hdr.e_machine, EM_386);
    }

    #[test]
    fn test_parse_not_elf() {
        let data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
                        0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
                        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                        0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
                        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
                        0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
                        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
                        0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f];
        assert!(matches!(parse_elf_header(&data), Err(Error::NotElf)));
    }

    #[test]
    fn test_truncated_header() {
        let data = vec![0x7f, b'E', b'L', b'F'];
        assert!(matches!(parse_elf_header(&data), Err(Error::TruncatedHeader)));
    }

    #[test]
    fn test_invalid_class() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = 99;
        data[EI_DATA] = ELFDATA2LSB;
        assert!(matches!(parse_elf_header(&data), Err(Error::InvalidClass(99))));
    }

    #[test]
    fn test_invalid_encoding() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = 99;
        assert!(matches!(parse_elf_header(&data), Err(Error::InvalidEncoding(99))));
    }

    #[test]
    fn test_big_endian_header() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = ELFDATA2MSB;
        data[6] = 1;
        // e_type = ET_EXEC = 2 in big-endian
        data[16..18].copy_from_slice(&ET_EXEC.to_be_bytes());
        data[18..20].copy_from_slice(&EM_X86_64.to_be_bytes());
        data[20..24].copy_from_slice(&1u32.to_be_bytes());
        data[24..32].copy_from_slice(&0x400000u64.to_be_bytes());
        data[52..54].copy_from_slice(&64u16.to_be_bytes());
        data[54..56].copy_from_slice(&56u16.to_be_bytes());
        data[58..60].copy_from_slice(&64u16.to_be_bytes());

        let hdr = parse_elf_header(&data).unwrap();
        assert!(!hdr.little_endian);
        assert_eq!(hdr.e_type, ET_EXEC);
        assert_eq!(hdr.e_machine, EM_X86_64);
        assert_eq!(hdr.e_entry, 0x400000);
    }

    // --------------- Section parsing tests ---------------

    #[test]
    fn test_parse_sections() {
        let data = make_test_elf64();
        let hdr = parse_elf_header(&data).unwrap();
        let sections = parse_section_headers(&data, &hdr).unwrap();
        assert_eq!(sections.len(), 7);
        assert_eq!(sections[0].sh_type, SHT_NULL);
        assert_eq!(sections[1].name, ".text");
        assert_eq!(sections[1].sh_type, SHT_PROGBITS);
        assert!(sections[1].sh_flags & SHF_EXECINSTR != 0);
        assert_eq!(sections[2].name, ".data");
        assert!(sections[2].sh_flags & SHF_WRITE != 0);
        assert_eq!(sections[3].name, ".bss");
        assert_eq!(sections[3].sh_type, SHT_NOBITS);
    }

    #[test]
    fn test_section_flags_str() {
        assert_eq!(section_flags_str(SHF_WRITE | SHF_ALLOC), "WA");
        assert_eq!(section_flags_str(SHF_ALLOC | SHF_EXECINSTR), "AX");
        assert_eq!(section_flags_str(0), "");
        assert_eq!(section_flags_str(SHF_WRITE | SHF_ALLOC | SHF_EXECINSTR), "WAX");
        assert_eq!(section_flags_str(SHF_MERGE | SHF_STRINGS), "MS");
    }

    #[test]
    fn test_section_type_names() {
        assert_eq!(section_type_str(SHT_NULL), "NULL");
        assert_eq!(section_type_str(SHT_PROGBITS), "PROGBITS");
        assert_eq!(section_type_str(SHT_SYMTAB), "SYMTAB");
        assert_eq!(section_type_str(SHT_STRTAB), "STRTAB");
        assert_eq!(section_type_str(SHT_NOBITS), "NOBITS");
        assert_eq!(section_type_str(SHT_RELA), "RELA");
        assert_eq!(section_type_str(SHT_REL), "REL");
        assert_eq!(section_type_str(SHT_DYNAMIC), "DYNAMIC");
        assert_eq!(section_type_str(SHT_DYNSYM), "DYNSYM");
    }

    // --------------- Symbol parsing tests ---------------

    #[test]
    fn test_parse_symbols() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let symtab = elf.sections.iter().find(|s| s.sh_type == SHT_SYMTAB).unwrap();
        let syms = parse_symbols(&elf.data, &elf.header, symtab, &elf.sections).unwrap();
        assert_eq!(syms.len(), 4);
        // Symbol 0 is null
        assert_eq!(syms[0].st_value, 0);
        // Symbol 1: main
        assert_eq!(syms[1].name, "main");
        assert_eq!(syms[1].binding(), STB_GLOBAL);
        assert_eq!(syms[1].sym_type(), STT_FUNC);
        assert_eq!(syms[1].st_value, 0x1004);
        // Symbol 2: data_val
        assert_eq!(syms[2].name, "data_val");
        assert_eq!(syms[2].sym_type(), STT_OBJECT);
        assert_eq!(syms[2].st_value, 0x2000);
        // Symbol 3: bss_zero
        assert_eq!(syms[3].name, "bss_zero");
        assert_eq!(syms[3].st_value, 0x3000);
    }

    // --------------- nm symbol type classification tests ---------------

    #[test]
    fn test_nm_type_text_global() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let symtab = elf.sections.iter().find(|s| s.sh_type == SHT_SYMTAB).unwrap();
        let syms = parse_symbols(&elf.data, &elf.header, symtab, &elf.sections).unwrap();
        // main is a global function in .text -> 'T'
        assert_eq!(nm_symbol_type(&syms[1], &elf.sections), 'T');
    }

    #[test]
    fn test_nm_type_data_global() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let symtab = elf.sections.iter().find(|s| s.sh_type == SHT_SYMTAB).unwrap();
        let syms = parse_symbols(&elf.data, &elf.header, symtab, &elf.sections).unwrap();
        // data_val is a global object in .data -> 'D'
        assert_eq!(nm_symbol_type(&syms[2], &elf.sections), 'D');
    }

    #[test]
    fn test_nm_type_bss_global() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let symtab = elf.sections.iter().find(|s| s.sh_type == SHT_SYMTAB).unwrap();
        let syms = parse_symbols(&elf.data, &elf.header, symtab, &elf.sections).unwrap();
        // bss_zero is in .bss -> 'B'
        assert_eq!(nm_symbol_type(&syms[3], &elf.sections), 'B');
    }

    #[test]
    fn test_nm_type_undefined() {
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_GLOBAL << 4) | STT_NOTYPE,
            st_other: 0,
            st_shndx: SHN_UNDEF,
            st_value: 0,
            st_size: 0,
            name: "undef_sym".to_string(),
        };
        assert_eq!(nm_symbol_type(&sym, &[]), 'U');
    }

    #[test]
    fn test_nm_type_absolute() {
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_GLOBAL << 4) | STT_NOTYPE,
            st_other: 0,
            st_shndx: SHN_ABS,
            st_value: 0x100,
            st_size: 0,
            name: "abs_sym".to_string(),
        };
        assert_eq!(nm_symbol_type(&sym, &[]), 'A');
    }

    #[test]
    fn test_nm_type_absolute_local() {
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_LOCAL << 4) | STT_NOTYPE,
            st_other: 0,
            st_shndx: SHN_ABS,
            st_value: 0x100,
            st_size: 0,
            name: "abs_local".to_string(),
        };
        assert_eq!(nm_symbol_type(&sym, &[]), 'a');
    }

    #[test]
    fn test_nm_type_weak_undefined() {
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_WEAK << 4) | STT_NOTYPE,
            st_other: 0,
            st_shndx: SHN_UNDEF,
            st_value: 0,
            st_size: 0,
            name: "weak_undef".to_string(),
        };
        assert_eq!(nm_symbol_type(&sym, &[]), 'w');
    }

    #[test]
    fn test_nm_type_weak_defined() {
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_WEAK << 4) | STT_FUNC,
            st_other: 0,
            st_shndx: 1,
            st_value: 0x1000,
            st_size: 16,
            name: "weak_func".to_string(),
        };
        assert_eq!(nm_symbol_type(&sym, &[]), 'W');
    }

    #[test]
    fn test_nm_type_common() {
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_GLOBAL << 4) | STT_OBJECT,
            st_other: 0,
            st_shndx: SHN_COMMON,
            st_value: 4,
            st_size: 4,
            name: "common_var".to_string(),
        };
        assert_eq!(nm_symbol_type(&sym, &[]), 'C');
    }

    #[test]
    fn test_nm_type_local_text() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        // Create a local function symbol in .text (section index 1)
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_LOCAL << 4) | STT_FUNC,
            st_other: 0,
            st_shndx: 1,
            st_value: 0x1000,
            st_size: 4,
            name: "local_func".to_string(),
        };
        assert_eq!(nm_symbol_type(&sym, &elf.sections), 't');
    }

    // --------------- Disassembly tests ---------------

    #[test]
    fn test_disasm_nop() {
        let code = [0x90];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "nop");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_ret() {
        let code = [0xc3];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "ret");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_push_rbp() {
        let code = [0x55];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "push   rbp");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_pop_rdi() {
        let code = [0x5f];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "pop    rdi");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_syscall() {
        let code = [0x0f, 0x05];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "syscall");
        assert_eq!(size, 2);
    }

    #[test]
    fn test_disasm_int3() {
        let code = [0xcc];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "int3");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_int_0x80() {
        let code = [0xcd, 0x80];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "int    0x80");
        assert_eq!(size, 2);
    }

    #[test]
    fn test_disasm_hlt() {
        let code = [0xf4];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(mnem, "hlt");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_call_rel32() {
        // CALL +0x100 from address 0x1000
        // target = 0x1000 + 5 + 0x100 = 0x1105
        let code = [0xe8, 0x00, 0x01, 0x00, 0x00];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(size, 5);
        assert!(mnem.starts_with("call"), "got: {mnem}");
        assert!(mnem.contains("0x1105"), "got: {mnem}");
    }

    #[test]
    fn test_disasm_jmp_rel32() {
        // JMP +0x10 from address 0x2000
        let code = [0xe9, 0x10, 0x00, 0x00, 0x00];
        let (mnem, size) = disasm_one(&code, 0, 0x2000);
        assert_eq!(size, 5);
        assert!(mnem.starts_with("jmp"), "got: {mnem}");
    }

    #[test]
    fn test_disasm_jmp_rel8() {
        // JMP +5 from address 0x1000
        let code = [0xeb, 0x05];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(size, 2);
        assert!(mnem.starts_with("jmp"), "got: {mnem}");
    }

    #[test]
    fn test_disasm_jz_rel8() {
        // JZ +3 from address 0x1000
        let code = [0x74, 0x03];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(size, 2);
        assert!(mnem.starts_with("jz"), "got: {mnem}");
    }

    #[test]
    fn test_disasm_mov_eax_imm32() {
        let code = [0xb8, 0x01, 0x00, 0x00, 0x00];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(size, 5);
        assert!(mnem.contains("mov"), "got: {mnem}");
        assert!(mnem.contains("eax"), "got: {mnem}");
    }

    #[test]
    fn test_disasm_mov_rax_imm64_rex() {
        // REX.W + MOV rax, imm64
        let code = [0x48, 0xb8, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let (mnem, size) = disasm_one(&code, 0, 0x1000);
        assert_eq!(size, 10);
        assert!(mnem.contains("movabs"), "got: {mnem}");
    }

    #[test]
    fn test_disasm_cli_sti() {
        let (mnem, size) = disasm_one(&[0xfa], 0, 0);
        assert_eq!(mnem, "cli");
        assert_eq!(size, 1);
        let (mnem, size) = disasm_one(&[0xfb], 0, 0);
        assert_eq!(mnem, "sti");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_leave() {
        let (mnem, size) = disasm_one(&[0xc9], 0, 0);
        assert_eq!(mnem, "leave");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_disasm_xor_eax_eax() {
        // xor eax, eax (common zero idiom)
        let code = [0x31, 0xc0];
        let (mnem, size) = disasm_one(&code, 0, 0);
        assert_eq!(size, 2);
        assert!(mnem.contains("xor"), "got: {mnem}");
        assert!(mnem.contains("eax"), "got: {mnem}");
    }

    // --------------- Byte reader tests ---------------

    #[test]
    fn test_read_u16_le() {
        let data = [0x34, 0x12];
        assert_eq!(read_u16(&data, 0, true).unwrap(), 0x1234);
    }

    #[test]
    fn test_read_u16_be() {
        let data = [0x12, 0x34];
        assert_eq!(read_u16(&data, 0, false).unwrap(), 0x1234);
    }

    #[test]
    fn test_read_u32_le() {
        let data = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_u32(&data, 0, true).unwrap(), 0x1234_5678);
    }

    #[test]
    fn test_read_u64_le() {
        let data = [0xef, 0xcd, 0xab, 0x90, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_u64(&data, 0, true).unwrap(), 0x1234_5678_90ab_cdef);
    }

    #[test]
    fn test_read_truncated() {
        let data = [0x01];
        assert!(read_u16(&data, 0, true).is_err());
        assert!(read_u32(&data, 0, true).is_err());
        assert!(read_u64(&data, 0, true).is_err());
    }

    #[test]
    fn test_read_cstr() {
        let data = b"hello\0world\0";
        assert_eq!(read_cstr(data, 0).unwrap(), "hello");
        assert_eq!(read_cstr(data, 6).unwrap(), "world");
    }

    #[test]
    fn test_read_cstr_at_end() {
        let data = b"abc";
        assert_eq!(read_cstr(data, 0).unwrap(), "abc"); // no null, reads to end
        assert_eq!(read_cstr(data, 100).unwrap(), ""); // past end
    }

    // --------------- Name helper tests ---------------

    #[test]
    fn test_file_type_str() {
        assert_eq!(file_type_str(ET_EXEC), "EXEC (Executable file)");
        assert_eq!(file_type_str(ET_DYN), "DYN (Shared object file)");
        assert_eq!(file_type_str(ET_REL), "REL (Relocatable file)");
        assert_eq!(file_type_str(0xffff), "Unknown");
    }

    #[test]
    fn test_machine_str() {
        assert_eq!(machine_str(EM_X86_64), "Advanced Micro Devices X86-64");
        assert_eq!(machine_str(EM_AARCH64), "AArch64");
        assert_eq!(machine_str(EM_386), "Intel 80386");
    }

    #[test]
    fn test_osabi_str() {
        assert_eq!(osabi_str(ELFOSABI_NONE), "UNIX - System V");
        assert_eq!(osabi_str(ELFOSABI_LINUX), "UNIX - Linux");
        assert_eq!(osabi_str(255), "OurOS");
    }

    #[test]
    fn test_phdr_flags_str() {
        assert_eq!(phdr_flags_str(PF_R | PF_X), "R E");
        assert_eq!(phdr_flags_str(PF_R | PF_W), "RW ");
        assert_eq!(phdr_flags_str(PF_R | PF_W | PF_X), "RWE");
        assert_eq!(phdr_flags_str(0), "   ");
    }

    #[test]
    fn test_reloc_type_str() {
        assert_eq!(reloc_type_str_x86_64(R_X86_64_64), "R_X86_64_64");
        assert_eq!(reloc_type_str_x86_64(R_X86_64_PC32), "R_X86_64_PC32");
        assert_eq!(reloc_type_str_x86_64(R_X86_64_PLT32), "R_X86_64_PLT32");
        assert_eq!(reloc_type_str_x86_64(999), "UNKNOWN");
    }

    // --------------- Size calculation tests ---------------

    #[test]
    fn test_size_berkeley_calculation() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();

        let mut text_size: u64 = 0;
        let mut data_size: u64 = 0;
        let mut bss_size: u64 = 0;

        for sec in &elf.sections {
            if sec.sh_flags & SHF_ALLOC == 0 {
                continue;
            }
            if sec.sh_type == SHT_NOBITS {
                bss_size += sec.sh_size;
            } else if sec.sh_flags & SHF_EXECINSTR != 0 {
                text_size += sec.sh_size;
            } else if sec.sh_flags & SHF_WRITE != 0 {
                data_size += sec.sh_size;
            } else {
                text_size += sec.sh_size;
            }
        }

        // .text is 16 bytes
        assert_eq!(text_size, 16);
        // .data is 8 bytes
        assert_eq!(data_size, 8);
        // .bss is 16 bytes
        assert_eq!(bss_size, 16);
    }

    // --------------- Personality detection tests ---------------

    #[test]
    fn test_personality_from_name() {
        // We can't easily test detect_personality() since it reads argv[0],
        // but we can test the name matching logic directly.
        fn match_name(name: &str) -> Personality {
            let name = name.strip_suffix(".exe").unwrap_or(name);
            if name.ends_with("nm") {
                Personality::Nm
            } else if name.ends_with("size") {
                Personality::Size
            } else {
                Personality::Objdump
            }
        }

        assert_eq!(match_name("objdump"), Personality::Objdump);
        assert_eq!(match_name("nm"), Personality::Nm);
        assert_eq!(match_name("size"), Personality::Size);
        assert_eq!(match_name("objdump.exe"), Personality::Objdump);
        assert_eq!(match_name("nm.exe"), Personality::Nm);
        assert_eq!(match_name("size.exe"), Personality::Size);
        assert_eq!(match_name("/usr/bin/nm"), Personality::Nm);
        assert_eq!(match_name("C:\\bin\\objdump.exe"), Personality::Objdump);
        assert_eq!(match_name("something_else"), Personality::Objdump);
    }

    // --------------- Format helpers tests ---------------

    #[test]
    fn test_format_nm_value() {
        assert_eq!(format_nm_value(0x1234, 'x'), "0000000000001234");
        assert_eq!(format_nm_value(42, 'd'), "0000000000000042");
        assert_eq!(format_nm_value(0o77, 'o'), "0000000000000000000077");
    }

    #[test]
    fn test_format_size_val() {
        assert_eq!(format_size_val(42, 10), "42");
        assert_eq!(format_size_val(42, 8), "052");
        assert_eq!(format_size_val(42, 16), "0x2a");
    }

    #[test]
    fn test_parse_addr_hex() {
        assert_eq!(parse_addr("0x1234"), Some(0x1234));
        assert_eq!(parse_addr("0X1234"), Some(0x1234));
    }

    #[test]
    fn test_parse_addr_decimal() {
        assert_eq!(parse_addr("1234"), Some(1234));
    }

    // --------------- Full ELF file parsing test ---------------

    #[test]
    fn test_full_elf_parse() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        assert_eq!(elf.header.e_type, ET_EXEC);
        assert_eq!(elf.sections.len(), 7);
        assert!(elf.program_headers.is_empty());
    }

    // --------------- ELF32 section parsing test ---------------

    #[test]
    fn test_elf32_sections() {
        // Minimal ELF32 with 2 sections: NULL + .shstrtab
        let shstrtab_data = b"\0.shstrtab\0";
        let shstrtab_off: u32 = 52; // right after header
        let shoff: u32 = shstrtab_off + shstrtab_data.len() as u32;

        let mut file = make_elf32_header(ET_REL, EM_386, 0, shoff, 2, 1);
        file.extend_from_slice(shstrtab_data);

        // Section 0: NULL
        append_shdr32(&mut file, 0, SHT_NULL, 0, 0, 0, 0, 0, 0, 0, 0);
        // Section 1: .shstrtab
        append_shdr32(
            &mut file,
            1,
            SHT_STRTAB,
            0,
            0,
            shstrtab_off,
            shstrtab_data.len() as u32,
            0,
            0,
            1,
            0,
        );

        let elf = parse_elf(file).unwrap();
        assert_eq!(elf.header.class, ELFCLASS32);
        assert_eq!(elf.sections.len(), 2);
        assert_eq!(elf.sections[1].name, ".shstrtab");
    }

    // --------------- objdump output tests ---------------

    #[test]
    fn test_display_file_header_output() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let mut output = Vec::new();
        display_file_header(&mut output, &elf, "test.elf").unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("elf64-little"));
        assert!(text.contains("EXEC"));
        assert!(text.contains("X86-64"));
        assert!(text.contains("0x1000"));
    }

    #[test]
    fn test_display_section_headers_output() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let mut output = Vec::new();
        display_section_headers(&mut output, &elf, "test.elf").unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains(".text"));
        assert!(text.contains(".data"));
        assert!(text.contains(".bss"));
    }

    #[test]
    fn test_display_symbols_output() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let mut output = Vec::new();
        display_symbols(&mut output, &elf, "test.elf", false).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("main"));
        assert!(text.contains("data_val"));
        assert!(text.contains("bss_zero"));
    }

    #[test]
    fn test_display_disassembly_output() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let mut output = Vec::new();
        display_disassembly(&mut output, &elf, None, None, None).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Disassembly of section .text"));
        assert!(text.contains("nop"));
        assert!(text.contains("ret"));
    }

    #[test]
    fn test_display_full_contents_output() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let mut output = Vec::new();
        display_full_contents(&mut output, &elf, Some(".data")).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Contents of section .data"));
        assert!(text.contains("42")); // our data value
    }

    #[test]
    fn test_disassembly_address_range() {
        let data = make_test_elf64();
        let elf = parse_elf(data).unwrap();
        let mut output = Vec::new();
        // Only disassemble from 0x1004 to 0x1008
        display_disassembly(&mut output, &elf, None, Some(0x1004), Some(0x1008)).unwrap();
        let text = String::from_utf8(output).unwrap();
        // Should not contain the NOP at 0x1000
        let lines: Vec<&str> = text.lines().collect();
        let code_lines: Vec<&&str> = lines.iter().filter(|l| l.contains("1000:")).collect();
        assert!(code_lines.is_empty(), "should not contain address 0x1000 instructions");
    }
}
