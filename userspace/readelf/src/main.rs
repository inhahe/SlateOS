//! OurOS ELF Binary Inspector
//!
//! Parses and displays information from ELF32 and ELF64 binary files.
//! Supports both little-endian and big-endian ELF files.
//!
//! # Usage
//!
//! ```text
//! readelf -h   binary        # ELF file header
//! readelf -l   binary        # Program headers (segments)
//! readelf -S   binary        # Section headers
//! readelf -s   binary        # Symbol tables
//! readelf -r   binary        # Relocations
//! readelf -d   binary        # Dynamic section
//! readelf -n   binary        # Note sections
//! readelf -a   binary        # All of the above
//! readelf -e   binary        # Headers (-h -l -S)
//! readelf -x N binary        # Hex dump of section N
//! readelf -W   binary        # Wide output (no 80-column truncation)
//! ```

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// readelf parses ELF32/ELF64 binary headers, section and program tables,
// symbols, relocations, and notes. Arithmetic is on offsets bounded by
// ELF header limits, and slice/index operations are gated by length
// checks against the file buffer. Errors return Err, never panic.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
)]

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
const EI_VERSION: usize = 6;
const EI_OSABI: usize = 7;
const EI_NIDENT: usize = 16;

const ELFCLASSNONE: u8 = 0;
const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;

const ELFDATANONE: u8 = 0;
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
const ELFOSABI_OUROS: u8 = 255; // OurOS custom ABI marker

// Program header types (p_type)
const PT_NULL: u32 = 0;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const PT_SHLIB: u32 = 5;
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
const SHT_SHLIB: u32 = 10;
const SHT_DYNSYM: u32 = 11;
const SHT_INIT_ARRAY: u32 = 14;
const SHT_FINI_ARRAY: u32 = 15;
const SHT_GNU_HASH: u32 = 0x6fff_fef5;
const SHT_GNU_VERSYM: u32 = 0x6fff_fff0;
const SHT_GNU_VERNEED: u32 = 0x6fff_fffe;
const SHT_GNU_VERDEF: u32 = 0x6fff_fffd;

// Section header flags
const SHF_WRITE: u64 = 1;
const SHF_ALLOC: u64 = 2;
const SHF_EXECINSTR: u64 = 4;
const SHF_MERGE: u64 = 16;
const SHF_STRINGS: u64 = 32;
const SHF_INFO_LINK: u64 = 64;
const SHF_LINK_ORDER: u64 = 128;
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

// Symbol visibility (lower 2 bits of st_other)
const STV_DEFAULT: u8 = 0;
const STV_INTERNAL: u8 = 1;
const STV_HIDDEN: u8 = 2;
const STV_PROTECTED: u8 = 3;

// Special section indices
const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xfff1;
const SHN_COMMON: u16 = 0xfff2;

// Dynamic section tags (d_tag)
const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_PLTRELSZ: i64 = 2;
const DT_PLTGOT: i64 = 3;
const DT_HASH: i64 = 4;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;
const DT_STRSZ: i64 = 10;
const DT_SYMENT: i64 = 11;
const DT_INIT: i64 = 12;
const DT_FINI: i64 = 13;
const DT_SONAME: i64 = 14;
const DT_RPATH: i64 = 15;
const DT_SYMBOLIC: i64 = 16;
const DT_REL: i64 = 17;
const DT_RELSZ: i64 = 18;
const DT_RELENT: i64 = 19;
const DT_PLTREL: i64 = 20;
const DT_DEBUG: i64 = 21;
const DT_TEXTREL: i64 = 22;
const DT_JMPREL: i64 = 23;
const DT_BIND_NOW: i64 = 24;
const DT_FLAGS: i64 = 30;
const DT_FLAGS_1: i64 = 0x6fff_fffb_u32 as i64;
const DT_GNU_HASH: i64 = 0x6fff_fef5_u32 as i64;

// Note types
const NT_GNU_ABI_TAG: u32 = 1;
const NT_GNU_HWCAP: u32 = 2;
const NT_GNU_BUILD_ID: u32 = 3;
const NT_GNU_GOLD_VERSION: u32 = 4;

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum Error {
    Io(io::Error),
    NotElf,
    TruncatedHeader,
    TruncatedData { what: &'static str, offset: usize, needed: usize, available: usize },
    InvalidClass(u8),
    InvalidEncoding(u8),
    InvalidIndex { what: &'static str, idx: usize },
    BadUtf8 { what: &'static str },
    SectionNotFound(String),
    InvalidHexDumpTarget(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::NotElf => write!(f, "not an ELF file (bad magic)"),
            Self::TruncatedHeader => write!(f, "file too small to contain ELF header"),
            Self::TruncatedData { what, offset, needed, available } => write!(
                f,
                "{what}: truncated data at offset {offset:#x}: need {needed}, have {available}"
            ),
            Self::InvalidClass(c) => write!(f, "unknown ELF class: {c}"),
            Self::InvalidEncoding(e) => write!(f, "unknown ELF data encoding: {e}"),
            Self::InvalidIndex { what, idx } => write!(f, "{what}: index {idx} out of range"),
            Self::BadUtf8 { what } => write!(f, "{what}: contains invalid UTF-8"),
            Self::SectionNotFound(name) => write!(f, "section not found: {name}"),
            Self::InvalidHexDumpTarget(s) => write!(f, "invalid hex-dump target: {s}"),
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
// Options
// ============================================================================

/// What the user asked us to display.
#[derive(Default)]
struct Options {
    file_header: bool,
    program_headers: bool,
    section_headers: bool,
    symbols: bool,
    relocs: bool,
    dynamic: bool,
    notes: bool,
    /// Hex-dump targets: either a section name or a decimal/hex index string.
    hex_dumps: Vec<String>,
    /// Do not truncate output to 80 columns.
    wide: bool,
    /// Input files.
    files: Vec<String>,
}

fn usage() -> ! {
    let msg = "\
Usage: readelf <option(s)> elf-file(s)

Display information about the contents of ELF format files.

Options:
  -a, --all               Equivalent to: -h -l -S -s -r -d -n
  -h, --file-header       Display the ELF file header
  -l, --program-headers,
      --segments          Display the program headers
  -S, --section-headers,
      --sections          Display the section headers
  -e, --headers           Equivalent to: -h -l -S
  -s, --syms,
      --symbols           Display the symbol table
  -r, --relocs            Display the relocation sections
  -d, --dynamic           Display the dynamic section (if present)
  -n, --notes             Display the core notes (if present)
  -x <number or name>,
  --hex-dump=<number or name>
                          Dump the contents of section <number|name> as bytes
  -W, --wide              Allow output width to exceed 80 characters
  -H, --help              Display this information";
    eprintln!("{msg}");
    process::exit(0);
}

fn parse_args() -> Result<Options> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = Options::default();
    let mut i = 0;
    let mut end_of_opts = false;

    if args.is_empty() {
        usage();
    }

    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') {
            opts.files.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options
        if let Some(rest) = arg.strip_prefix("--") {
            match rest {
                "all" => {
                    opts.file_header = true;
                    opts.program_headers = true;
                    opts.section_headers = true;
                    opts.symbols = true;
                    opts.relocs = true;
                    opts.dynamic = true;
                    opts.notes = true;
                }
                "file-header" => opts.file_header = true,
                "program-headers" | "segments" => opts.program_headers = true,
                "section-headers" | "sections" => opts.section_headers = true,
                "headers" => {
                    opts.file_header = true;
                    opts.program_headers = true;
                    opts.section_headers = true;
                }
                "syms" | "symbols" => opts.symbols = true,
                "relocs" => opts.relocs = true,
                "dynamic" => opts.dynamic = true,
                "notes" => opts.notes = true,
                "wide" => opts.wide = true,
                "help" => usage(),
                _ if rest.starts_with("hex-dump=") => {
                    let val = &rest["hex-dump=".len()..];
                    opts.hex_dumps.push(val.to_string());
                }
                _ => {
                    return Err(Error::Io(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unknown option: --{rest}"),
                    )));
                }
            }
            i += 1;
            continue;
        }

        // Short options (possibly clustered: -hls)
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'a' => {
                    opts.file_header = true;
                    opts.program_headers = true;
                    opts.section_headers = true;
                    opts.symbols = true;
                    opts.relocs = true;
                    opts.dynamic = true;
                    opts.notes = true;
                }
                'h' => opts.file_header = true,
                'H' => usage(),
                'l' => opts.program_headers = true,
                'S' => opts.section_headers = true,
                'e' => {
                    opts.file_header = true;
                    opts.program_headers = true;
                    opts.section_headers = true;
                }
                's' => opts.symbols = true,
                'r' => opts.relocs = true,
                'd' => opts.dynamic = true,
                'n' => opts.notes = true,
                'W' => opts.wide = true,
                'x' => {
                    // -x requires an argument: rest of cluster or next argv
                    let val: String = if j + 1 < chars.len() {
                        let v: String = chars[j + 1..].iter().collect();
                        j = chars.len(); // consumed all remaining chars
                        v
                    } else if i + 1 < args.len() {
                        i += 1;
                        args[i].clone()
                    } else {
                        return Err(Error::Io(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "option -x requires an argument",
                        )));
                    };
                    opts.hex_dumps.push(val);
                    continue;
                }
                c => {
                    return Err(Error::Io(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unknown option: -{c}"),
                    )));
                }
            }
            j += 1;
        }
        i += 1;
    }

    // Default: show everything if user only gave a filename
    if !opts.file_header
        && !opts.program_headers
        && !opts.section_headers
        && !opts.symbols
        && !opts.relocs
        && !opts.dynamic
        && !opts.notes
        && opts.hex_dumps.is_empty()
    {
        opts.file_header = true;
        opts.program_headers = true;
        opts.section_headers = true;
        opts.symbols = true;
        opts.relocs = true;
        opts.dynamic = true;
        opts.notes = true;
    }

    Ok(opts)
}

// ============================================================================
// ELF byte-reader helpers
// ============================================================================

/// Read a u16 from `data` at `offset` with the given endianness.
fn read_u16(data: &[u8], offset: usize, little_endian: bool) -> Result<u16> {
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
    Ok(if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

/// Read a u32 from `data` at `offset` with the given endianness.
fn read_u32(data: &[u8], offset: usize, little_endian: bool) -> Result<u32> {
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
    let bytes = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]];
    Ok(if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

/// Read a u64 from `data` at `offset` with the given endianness.
fn read_u64(data: &[u8], offset: usize, little_endian: bool) -> Result<u64> {
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
        data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
        data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
    ];
    Ok(if little_endian {
        u64::from_le_bytes(bytes)
    } else {
        u64::from_be_bytes(bytes)
    })
}

/// Read a null-terminated C string from `data` at `offset`.
/// Returns an empty string if `offset == 0` or at end of `data`.
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

/// Parsed ELF file header (fields normalised to u64 for both 32/64-bit files).
#[derive(Debug, Clone)]
struct ElfHeader {
    /// ELF class: 1 = 32-bit, 2 = 64-bit.
    class: u8,
    /// Data encoding: 1 = LSB, 2 = MSB.
    data: u8,
    /// Whether data is little-endian (derived from `data`).
    little_endian: bool,
    /// ELF version (always 1).
    version: u8,
    /// OS/ABI.
    osabi: u8,
    /// ABI version.
    abi_version: u8,
    /// File type: ET_REL, ET_EXEC, ET_DYN, etc.
    e_type: u16,
    /// Target machine.
    e_machine: u16,
    /// Object file version.
    e_version: u32,
    /// Entry point virtual address.
    e_entry: u64,
    /// Program header table file offset.
    e_phoff: u64,
    /// Section header table file offset.
    e_shoff: u64,
    /// Processor-specific flags.
    e_flags: u32,
    /// ELF header size in bytes.
    e_ehsize: u16,
    /// Program header entry size.
    e_phentsize: u16,
    /// Number of program header entries.
    e_phnum: u16,
    /// Section header entry size.
    e_shentsize: u16,
    /// Number of section header entries.
    e_shnum: u16,
    /// Section name string table index.
    e_shstrndx: u16,
}

/// Parsed program header entry.
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

/// Parsed section header entry.
#[derive(Debug, Clone)]
struct SectionHeader {
    /// Index into the section name string table.
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
    /// Resolved name (from shstrtab).
    name: String,
}

/// Parsed symbol table entry.
#[derive(Debug, Clone)]
struct Symbol {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
    /// Resolved name.
    name: String,
}

impl Symbol {
    fn binding(&self) -> u8 {
        self.st_info >> 4
    }
    fn sym_type(&self) -> u8 {
        self.st_info & 0x0f
    }
    fn visibility(&self) -> u8 {
        self.st_other & 0x03
    }
}

/// A Rel relocation entry (no addend).
#[derive(Debug, Clone)]
struct Rel {
    r_offset: u64,
    r_info: u64,
}

impl Rel {
    fn sym_index(&self, class: u8) -> u32 {
        if class == ELFCLASS64 {
            (self.r_info >> 32) as u32
        } else {
            (self.r_info >> 8) as u32
        }
    }
    fn rel_type(&self, class: u8) -> u32 {
        if class == ELFCLASS64 {
            (self.r_info & 0xffff_ffff) as u32
        } else {
            (self.r_info & 0xff) as u32
        }
    }
}

/// A Rela relocation entry (with addend).
#[derive(Debug, Clone)]
struct Rela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}

impl Rela {
    fn sym_index(&self, class: u8) -> u32 {
        if class == ELFCLASS64 {
            (self.r_info >> 32) as u32
        } else {
            (self.r_info >> 8) as u32
        }
    }
    fn rel_type(&self, class: u8) -> u32 {
        if class == ELFCLASS64 {
            (self.r_info & 0xffff_ffff) as u32
        } else {
            (self.r_info & 0xff) as u32
        }
    }
}

/// A dynamic section entry.
#[derive(Debug, Clone)]
struct DynEntry {
    d_tag: i64,
    d_val: u64, // also used as d_ptr
}

/// A note entry.
#[derive(Debug, Clone)]
struct Note {
    name: String,
    note_type: u32,
    desc: Vec<u8>,
}

// ============================================================================
// ELF file — parsed representation
// ============================================================================

struct Elf<'a> {
    data: &'a [u8],
    header: ElfHeader,
    phdrs: Vec<ProgramHeader>,
    shdrs: Vec<SectionHeader>,
}

impl<'a> Elf<'a> {
    fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < EI_NIDENT {
            return Err(Error::TruncatedHeader);
        }
        if data[..4] != ELFMAG {
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

        let header = if class == ELFCLASS64 {
            parse_elf64_header(data, le)?
        } else {
            parse_elf32_header(data, le)?
        };

        let phdrs = parse_program_headers(data, &header)?;
        let shdrs = parse_section_headers(data, &header)?;

        Ok(Self { data, header, phdrs, shdrs })
    }

    fn le(&self) -> bool {
        self.header.little_endian
    }

    fn class(&self) -> u8 {
        self.header.class
    }

    /// Return the raw bytes of a section, by index.
    fn section_data(&self, idx: usize) -> Result<&[u8]> {
        let sh = self.shdrs.get(idx).ok_or(Error::InvalidIndex {
            what: "section",
            idx,
        })?;
        // SHT_NOBITS sections have no file data
        if sh.sh_type == SHT_NOBITS {
            return Ok(&[]);
        }
        let off = sh.sh_offset as usize;
        let sz = sh.sh_size as usize;
        let end = off.checked_add(sz).ok_or(Error::TruncatedData {
            what: "section data",
            offset: off,
            needed: sz,
            available: self.data.len(),
        })?;
        if end > self.data.len() {
            return Err(Error::TruncatedData {
                what: "section data",
                offset: off,
                needed: sz,
                available: self.data.len(),
            });
        }
        Ok(&self.data[off..end])
    }

    /// Find section index by name. Returns None if not found.
    fn find_section(&self, name: &str) -> Option<usize> {
        self.shdrs.iter().position(|s| s.name == name)
    }

    /// Return all parsed symbols from the given section index (SHT_SYMTAB or SHT_DYNSYM).
    fn parse_symbols(&self, sec_idx: usize) -> Result<Vec<Symbol>> {
        let sh = self.shdrs.get(sec_idx).ok_or(Error::InvalidIndex {
            what: "symtab section",
            idx: sec_idx,
        })?;
        let strtab_idx = sh.sh_link as usize;
        let sec_data = self.section_data(sec_idx)?;
        let strtab = if strtab_idx < self.shdrs.len() {
            self.section_data(strtab_idx)?
        } else {
            &[]
        };
        parse_symbol_table(sec_data, strtab, self.class(), self.le())
    }

    /// Return dynamic entries from the .dynamic section, if present.
    fn parse_dynamic(&self) -> Result<Option<Vec<DynEntry>>> {
        let Some(idx) = self.find_section(".dynamic") else {
            return Ok(None);
        };
        let data = self.section_data(idx)?;
        Ok(Some(parse_dynamic_entries(data, self.class(), self.le())?))
    }

    /// Parse relocations from a section, returning them as (Rel[], Rela[], name).
    fn parse_reloc_section(&self, sec_idx: usize) -> Result<(Vec<Rel>, Vec<Rela>, String)> {
        let sh = &self.shdrs[sec_idx];
        let name = sh.name.clone();
        let sec_data = self.section_data(sec_idx)?;
        if sh.sh_type == SHT_RELA {
            let relas = parse_rela_table(sec_data, self.class(), self.le())?;
            Ok((Vec::new(), relas, name))
        } else {
            let rels = parse_rel_table(sec_data, self.class(), self.le())?;
            Ok((rels, Vec::new(), name))
        }
    }

    /// Parse notes from a raw data blob (PT_NOTE or SHT_NOTE contents).
    fn parse_notes_data(data: &[u8], little_endian: bool) -> Vec<Note> {
        parse_note_entries(data, little_endian)
    }

    /// Return the string table for the dynamic section's DT_STRTAB, if available.
    fn dynstr(&self) -> &[u8] {
        if let Some(idx) = self.find_section(".dynstr") {
            self.section_data(idx).unwrap_or(&[])
        } else {
            &[]
        }
    }
}

// ============================================================================
// Header parsers
// ============================================================================

fn parse_elf64_header(data: &[u8], le: bool) -> Result<ElfHeader> {
    const NEEDED: usize = 64;
    if data.len() < NEEDED {
        return Err(Error::TruncatedData {
            what: "ELF64 header",
            offset: 0,
            needed: NEEDED,
            available: data.len(),
        });
    }
    Ok(ElfHeader {
        class: data[EI_CLASS],
        data: data[EI_DATA],
        little_endian: le,
        version: data[EI_VERSION],
        osabi: data[EI_OSABI],
        abi_version: data[8],
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
}

fn parse_elf32_header(data: &[u8], le: bool) -> Result<ElfHeader> {
    const NEEDED: usize = 52;
    if data.len() < NEEDED {
        return Err(Error::TruncatedData {
            what: "ELF32 header",
            offset: 0,
            needed: NEEDED,
            available: data.len(),
        });
    }
    Ok(ElfHeader {
        class: data[EI_CLASS],
        data: data[EI_DATA],
        little_endian: le,
        version: data[EI_VERSION],
        osabi: data[EI_OSABI],
        abi_version: data[8],
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

fn parse_program_headers(data: &[u8], hdr: &ElfHeader) -> Result<Vec<ProgramHeader>> {
    let count = hdr.e_phnum as usize;
    let off = hdr.e_phoff as usize;
    let entsz = hdr.e_phentsize as usize;
    let le = hdr.little_endian;

    if count == 0 || off == 0 {
        return Ok(Vec::new());
    }
    let total = count.checked_mul(entsz).ok_or(Error::TruncatedData {
        what: "program headers",
        offset: off,
        needed: 0,
        available: data.len(),
    })?;
    let end = off.checked_add(total).ok_or(Error::TruncatedData {
        what: "program headers",
        offset: off,
        needed: total,
        available: data.len(),
    })?;
    if end > data.len() {
        return Err(Error::TruncatedData {
            what: "program headers",
            offset: off,
            needed: total,
            available: data.len(),
        });
    }

    let mut phdrs = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * entsz;
        let ph = if hdr.class == ELFCLASS64 {
            ProgramHeader {
                p_type: read_u32(data, base, le)?,
                p_flags: read_u32(data, base + 4, le)?,
                p_offset: read_u64(data, base + 8, le)?,
                p_vaddr: read_u64(data, base + 16, le)?,
                p_paddr: read_u64(data, base + 24, le)?,
                p_filesz: read_u64(data, base + 32, le)?,
                p_memsz: read_u64(data, base + 40, le)?,
                p_align: read_u64(data, base + 48, le)?,
            }
        } else {
            ProgramHeader {
                p_type: read_u32(data, base, le)?,
                p_offset: read_u32(data, base + 4, le)? as u64,
                p_vaddr: read_u32(data, base + 8, le)? as u64,
                p_paddr: read_u32(data, base + 12, le)? as u64,
                p_filesz: read_u32(data, base + 16, le)? as u64,
                p_memsz: read_u32(data, base + 20, le)? as u64,
                p_flags: read_u32(data, base + 24, le)?,
                p_align: read_u32(data, base + 28, le)? as u64,
            }
        };
        phdrs.push(ph);
    }
    Ok(phdrs)
}

fn parse_section_headers(data: &[u8], hdr: &ElfHeader) -> Result<Vec<SectionHeader>> {
    let count = hdr.e_shnum as usize;
    let off = hdr.e_shoff as usize;
    let entsz = hdr.e_shentsize as usize;
    let le = hdr.little_endian;

    if count == 0 || off == 0 {
        return Ok(Vec::new());
    }
    let total = count.checked_mul(entsz).ok_or(Error::TruncatedData {
        what: "section headers",
        offset: off,
        needed: 0,
        available: data.len(),
    })?;
    let end = off.checked_add(total).ok_or(Error::TruncatedData {
        what: "section headers",
        offset: off,
        needed: total,
        available: data.len(),
    })?;
    if end > data.len() {
        return Err(Error::TruncatedData {
            what: "section headers",
            offset: off,
            needed: total,
            available: data.len(),
        });
    }

    let mut shdrs: Vec<SectionHeader> = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * entsz;
        let sh = if hdr.class == ELFCLASS64 {
            SectionHeader {
                sh_name: read_u32(data, base, le)?,
                sh_type: read_u32(data, base + 4, le)?,
                sh_flags: read_u64(data, base + 8, le)?,
                sh_addr: read_u64(data, base + 16, le)?,
                sh_offset: read_u64(data, base + 24, le)?,
                sh_size: read_u64(data, base + 32, le)?,
                sh_link: read_u32(data, base + 40, le)?,
                sh_info: read_u32(data, base + 44, le)?,
                sh_addralign: read_u64(data, base + 48, le)?,
                sh_entsize: read_u64(data, base + 56, le)?,
                name: String::new(),
            }
        } else {
            SectionHeader {
                sh_name: read_u32(data, base, le)?,
                sh_type: read_u32(data, base + 4, le)?,
                sh_flags: read_u32(data, base + 8, le)? as u64,
                sh_addr: read_u32(data, base + 12, le)? as u64,
                sh_offset: read_u32(data, base + 16, le)? as u64,
                sh_size: read_u32(data, base + 20, le)? as u64,
                sh_link: read_u32(data, base + 24, le)?,
                sh_info: read_u32(data, base + 28, le)?,
                sh_addralign: read_u32(data, base + 32, le)? as u64,
                sh_entsize: read_u32(data, base + 36, le)? as u64,
                name: String::new(),
            }
        };
        shdrs.push(sh);
    }

    // Resolve names from shstrtab
    let shstrndx = hdr.e_shstrndx as usize;
    if shstrndx < shdrs.len() {
        let shstr_off = shdrs[shstrndx].sh_offset as usize;
        let shstr_sz = shdrs[shstrndx].sh_size as usize;
        let shstr_end = shstr_off.saturating_add(shstr_sz);
        let shstrtab: &[u8] = if shstr_end <= data.len() {
            &data[shstr_off..shstr_end]
        } else {
            &[]
        };
        for sh in &mut shdrs {
            let name_off = sh.sh_name as usize;
            sh.name = read_cstr(shstrtab, name_off)
                .unwrap_or("")
                .to_string();
        }
    }

    Ok(shdrs)
}

fn parse_symbol_table(
    sec_data: &[u8],
    strtab: &[u8],
    class: u8,
    le: bool,
) -> Result<Vec<Symbol>> {
    let entsz: usize = if class == ELFCLASS64 { 24 } else { 16 };
    if sec_data.is_empty() {
        return Ok(Vec::new());
    }
    let count = sec_data.len() / entsz;
    let mut syms = Vec::with_capacity(count);

    for i in 0..count {
        let base = i * entsz;
        if base + entsz > sec_data.len() {
            break;
        }
        let sym = if class == ELFCLASS64 {
            Symbol {
                st_name: read_u32(sec_data, base, le)?,
                st_info: sec_data[base + 4],
                st_other: sec_data[base + 5],
                st_shndx: read_u16(sec_data, base + 6, le)?,
                st_value: read_u64(sec_data, base + 8, le)?,
                st_size: read_u64(sec_data, base + 16, le)?,
                name: String::new(),
            }
        } else {
            Symbol {
                st_name: read_u32(sec_data, base, le)?,
                st_value: read_u32(sec_data, base + 4, le)? as u64,
                st_size: read_u32(sec_data, base + 8, le)? as u64,
                st_info: sec_data[base + 12],
                st_other: sec_data[base + 13],
                st_shndx: read_u16(sec_data, base + 14, le)?,
                name: String::new(),
            }
        };

        let mut sym = sym;
        sym.name = read_cstr(strtab, sym.st_name as usize)
            .unwrap_or("")
            .to_string();
        syms.push(sym);
    }
    Ok(syms)
}

fn parse_rel_table(sec_data: &[u8], class: u8, le: bool) -> Result<Vec<Rel>> {
    let entsz: usize = if class == ELFCLASS64 { 16 } else { 8 };
    let count = sec_data.len() / entsz;
    let mut rels = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * entsz;
        if base + entsz > sec_data.len() {
            break;
        }
        let rel = if class == ELFCLASS64 {
            Rel {
                r_offset: read_u64(sec_data, base, le)?,
                r_info: read_u64(sec_data, base + 8, le)?,
            }
        } else {
            Rel {
                r_offset: read_u32(sec_data, base, le)? as u64,
                r_info: read_u32(sec_data, base + 4, le)? as u64,
            }
        };
        rels.push(rel);
    }
    Ok(rels)
}

fn parse_rela_table(sec_data: &[u8], class: u8, le: bool) -> Result<Vec<Rela>> {
    let entsz: usize = if class == ELFCLASS64 { 24 } else { 12 };
    let count = sec_data.len() / entsz;
    let mut relas = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * entsz;
        if base + entsz > sec_data.len() {
            break;
        }
        let rela = if class == ELFCLASS64 {
            Rela {
                r_offset: read_u64(sec_data, base, le)?,
                r_info: read_u64(sec_data, base + 8, le)?,
                r_addend: read_u64(sec_data, base + 16, le)? as i64,
            }
        } else {
            Rela {
                r_offset: read_u32(sec_data, base, le)? as u64,
                r_info: read_u32(sec_data, base + 4, le)? as u64,
                r_addend: read_u32(sec_data, base + 8, le)? as i32 as i64,
            }
        };
        relas.push(rela);
    }
    Ok(relas)
}

fn parse_dynamic_entries(sec_data: &[u8], class: u8, le: bool) -> Result<Vec<DynEntry>> {
    let entsz: usize = if class == ELFCLASS64 { 16 } else { 8 };
    let count = sec_data.len() / entsz;
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * entsz;
        if base + entsz > sec_data.len() {
            break;
        }
        let (tag, val) = if class == ELFCLASS64 {
            (
                read_u64(sec_data, base, le)? as i64,
                read_u64(sec_data, base + 8, le)?,
            )
        } else {
            (
                read_u32(sec_data, base, le)? as i32 as i64,
                read_u32(sec_data, base + 4, le)? as u64,
            )
        };
        entries.push(DynEntry { d_tag: tag, d_val: val });
        if tag == DT_NULL {
            break;
        }
    }
    Ok(entries)
}

fn parse_note_entries(data: &[u8], le: bool) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut pos = 0usize;

    while pos + 12 <= data.len() {
        let namesz = match read_u32(data, pos, le) {
            Ok(v) => v as usize,
            Err(_) => break,
        };
        let descsz = match read_u32(data, pos + 4, le) {
            Ok(v) => v as usize,
            Err(_) => break,
        };
        let note_type = match read_u32(data, pos + 8, le) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += 12;

        // Name: namesz bytes, padded to 4-byte boundary
        let name_end = pos.saturating_add(namesz);
        if name_end > data.len() {
            break;
        }
        let name_bytes = &data[pos..name_end];
        let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(namesz);
        let name = std::str::from_utf8(&name_bytes[..name_len])
            .unwrap_or("?")
            .to_string();
        pos = (pos + namesz + 3) & !3; // align to 4

        // Descriptor: descsz bytes, padded to 4-byte boundary
        let desc_end = pos.saturating_add(descsz);
        if desc_end > data.len() {
            break;
        }
        let desc = data[pos..desc_end].to_vec();
        pos = (pos + descsz + 3) & !3; // align to 4

        notes.push(Note { name, note_type, desc });
    }
    notes
}

// ============================================================================
// Name/description helpers
// ============================================================================

fn elf_type_name(t: u16) -> String {
    match t {
        ET_NONE => "NONE (None)".to_string(),
        ET_REL => "REL (Relocatable file)".to_string(),
        ET_EXEC => "EXEC (Executable file)".to_string(),
        ET_DYN => "DYN (Shared object file)".to_string(),
        ET_CORE => "CORE (Core file)".to_string(),
        0xfe00..=0xfeff => format!("OS Specific: ({t:#06x})"),
        0xff00..=0xffff => format!("Processor Specific: ({t:#06x})"),
        _ => format!("<unknown>: {t:#06x}"),
    }
}

fn machine_name(m: u16) -> &'static str {
    match m {
        EM_NONE => "None",
        EM_386 => "Intel 80386",
        EM_MIPS => "MIPS R3000",
        EM_PPC => "PowerPC",
        EM_PPC64 => "PowerPC64",
        EM_ARM => "ARM",
        EM_X86_64 => "Advanced Micro Devices X86-64",
        EM_AARCH64 => "AArch64",
        EM_RISCV => "RISC-V",
        _ => "<unknown>",
    }
}

fn osabi_name(a: u8) -> &'static str {
    match a {
        ELFOSABI_NONE => "UNIX - System V",
        ELFOSABI_LINUX => "Linux",
        ELFOSABI_FREEBSD => "FreeBSD",
        ELFOSABI_OUROS => "OurOS",
        _ => "<unknown>",
    }
}

fn class_name(c: u8) -> &'static str {
    match c {
        ELFCLASSNONE => "none",
        ELFCLASS32 => "ELF32",
        ELFCLASS64 => "ELF64",
        _ => "<unknown>",
    }
}

fn data_name(d: u8) -> &'static str {
    match d {
        ELFDATANONE => "none",
        ELFDATA2LSB => "2's complement, little endian",
        ELFDATA2MSB => "2's complement, big endian",
        _ => "<unknown>",
    }
}

fn phdr_type_name(t: u32) -> String {
    match t {
        PT_NULL => "NULL".to_string(),
        PT_LOAD => "LOAD".to_string(),
        PT_DYNAMIC => "DYNAMIC".to_string(),
        PT_INTERP => "INTERP".to_string(),
        PT_NOTE => "NOTE".to_string(),
        PT_SHLIB => "SHLIB".to_string(),
        PT_PHDR => "PHDR".to_string(),
        PT_TLS => "TLS".to_string(),
        PT_GNU_STACK => "GNU_STACK".to_string(),
        PT_GNU_RELRO => "GNU_RELRO".to_string(),
        PT_GNU_EH_FRAME => "GNU_EH_FRAME".to_string(),
        0x6000_0000..=0x6fff_ffff => format!("LOOS+{:#x}", t - 0x6000_0000),
        0x7000_0000..=0x7fff_ffff => format!("LOPROC+{:#x}", t - 0x7000_0000),
        _ => format!("0x{t:08x}"),
    }
}

fn phdr_flags_str(flags: u32) -> String {
    let r = if flags & PF_R != 0 { 'R' } else { ' ' };
    let w = if flags & PF_W != 0 { 'W' } else { ' ' };
    let x = if flags & PF_X != 0 { 'E' } else { ' ' };
    format!("{r}{w}{x}")
}

fn shdr_type_name(t: u32) -> &'static str {
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
        SHT_SHLIB => "SHLIB",
        SHT_DYNSYM => "DYNSYM",
        SHT_INIT_ARRAY => "INIT_ARRAY",
        SHT_FINI_ARRAY => "FINI_ARRAY",
        SHT_GNU_HASH => "GNU_HASH",
        SHT_GNU_VERSYM => "VERSYM",
        SHT_GNU_VERNEED => "VERNEED",
        SHT_GNU_VERDEF => "VERDEF",
        _ => "UNKNOWN",
    }
}

fn shdr_flags_str(flags: u64) -> String {
    let mut s = String::with_capacity(8);
    if flags & SHF_WRITE != 0 { s.push('W'); }
    if flags & SHF_ALLOC != 0 { s.push('A'); }
    if flags & SHF_EXECINSTR != 0 { s.push('X'); }
    if flags & SHF_MERGE != 0 { s.push('M'); }
    if flags & SHF_STRINGS != 0 { s.push('S'); }
    if flags & SHF_INFO_LINK != 0 { s.push('I'); }
    if flags & SHF_LINK_ORDER != 0 { s.push('L'); }
    if flags & SHF_TLS != 0 { s.push('T'); }
    s
}

fn sym_binding_name(b: u8) -> &'static str {
    match b {
        STB_LOCAL => "LOCAL",
        STB_GLOBAL => "GLOBAL",
        STB_WEAK => "WEAK",
        _ => "<unknown>",
    }
}

fn sym_type_name(t: u8) -> &'static str {
    match t {
        STT_NOTYPE => "NOTYPE",
        STT_OBJECT => "OBJECT",
        STT_FUNC => "FUNC",
        STT_SECTION => "SECTION",
        STT_FILE => "FILE",
        STT_COMMON => "COMMON",
        STT_TLS => "TLS",
        _ => "<unknown>",
    }
}

fn sym_visibility_name(v: u8) -> &'static str {
    match v {
        STV_DEFAULT => "DEFAULT",
        STV_INTERNAL => "INTERNAL",
        STV_HIDDEN => "HIDDEN",
        STV_PROTECTED => "PROTECTED",
        _ => "DEFAULT",
    }
}

fn sym_shndx_name(n: u16) -> String {
    match n {
        SHN_UNDEF => "UND".to_string(),
        SHN_ABS => "ABS".to_string(),
        SHN_COMMON => "COM".to_string(),
        _ => format!("{n}"),
    }
}

fn dynentry_tag_name(tag: i64) -> String {
    match tag {
        DT_NULL => "(NULL)".to_string(),
        DT_NEEDED => "(NEEDED)".to_string(),
        DT_PLTRELSZ => "(PLTRELSZ)".to_string(),
        DT_PLTGOT => "(PLTGOT)".to_string(),
        DT_HASH => "(HASH)".to_string(),
        DT_STRTAB => "(STRTAB)".to_string(),
        DT_SYMTAB => "(SYMTAB)".to_string(),
        DT_RELA => "(RELA)".to_string(),
        DT_RELASZ => "(RELASZ)".to_string(),
        DT_RELAENT => "(RELAENT)".to_string(),
        DT_STRSZ => "(STRSZ)".to_string(),
        DT_SYMENT => "(SYMENT)".to_string(),
        DT_INIT => "(INIT)".to_string(),
        DT_FINI => "(FINI)".to_string(),
        DT_SONAME => "(SONAME)".to_string(),
        DT_RPATH => "(RPATH)".to_string(),
        DT_SYMBOLIC => "(SYMBOLIC)".to_string(),
        DT_REL => "(REL)".to_string(),
        DT_RELSZ => "(RELSZ)".to_string(),
        DT_RELENT => "(RELENT)".to_string(),
        DT_PLTREL => "(PLTREL)".to_string(),
        DT_DEBUG => "(DEBUG)".to_string(),
        DT_TEXTREL => "(TEXTREL)".to_string(),
        DT_JMPREL => "(JMPREL)".to_string(),
        DT_BIND_NOW => "(BIND_NOW)".to_string(),
        DT_FLAGS => "(FLAGS)".to_string(),
        DT_FLAGS_1 => "(FLAGS_1)".to_string(),
        DT_GNU_HASH => "(GNU_HASH)".to_string(),
        _ => format!("(0x{tag:08x})"),
    }
}

fn note_type_name(owner: &str, note_type: u32) -> String {
    if owner == "GNU" {
        match note_type {
            NT_GNU_ABI_TAG => "NT_GNU_ABI_TAG".to_string(),
            NT_GNU_HWCAP => "NT_GNU_HWCAP".to_string(),
            NT_GNU_BUILD_ID => "NT_GNU_BUILD_ID".to_string(),
            NT_GNU_GOLD_VERSION => "NT_GNU_GOLD_VERSION".to_string(),
            _ => format!("Unknown note type: {note_type:#x}"),
        }
    } else {
        format!("{note_type:#x}")
    }
}

// ============================================================================
// Display functions
// ============================================================================

/// Truncate a string to `max` bytes (at a char boundary) if not in wide mode.
fn maybe_truncate(s: &str, max: usize, wide: bool) -> &str {
    if wide || s.len() <= max {
        return s;
    }
    // Find the largest char boundary <= max
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn display_file_header(out: &mut impl Write, elf: &Elf<'_>) -> Result<()> {
    let h = &elf.header;
    writeln!(out, "ELF Header:")?;
    write!(out, "  Magic:   ")?;
    for b in &elf.data[..EI_NIDENT] {
        write!(out, " {b:02x}")?;
    }
    writeln!(out)?;
    writeln!(out, "  Class:                             {}", class_name(h.class))?;
    writeln!(out, "  Data:                              {}", data_name(h.data))?;
    writeln!(out, "  Version:                           {} (current)", h.version)?;
    writeln!(out, "  OS/ABI:                            {}", osabi_name(h.osabi))?;
    writeln!(out, "  ABI Version:                       {}", h.abi_version)?;
    writeln!(out, "  Type:                              {}", elf_type_name(h.e_type))?;
    writeln!(out, "  Machine:                           {}", machine_name(h.e_machine))?;
    writeln!(out, "  Version:                           {:#x}", h.e_version)?;
    writeln!(out, "  Entry point address:               {:#x}", h.e_entry)?;
    writeln!(out, "  Start of program headers:          {} (bytes into file)", h.e_phoff)?;
    writeln!(out, "  Start of section headers:          {} (bytes into file)", h.e_shoff)?;
    writeln!(out, "  Flags:                             {:#x}", h.e_flags)?;
    writeln!(out, "  Size of this header:               {} (bytes)", h.e_ehsize)?;
    writeln!(out, "  Size of program headers:           {} (bytes)", h.e_phentsize)?;
    writeln!(out, "  Number of program headers:         {}", h.e_phnum)?;
    writeln!(out, "  Size of section headers:           {} (bytes)", h.e_shentsize)?;
    writeln!(out, "  Number of section headers:         {}", h.e_shnum)?;
    writeln!(out, "  Section header string table index: {}", h.e_shstrndx)?;
    Ok(())
}

fn display_program_headers(out: &mut impl Write, elf: &Elf<'_>, wide: bool) -> Result<()> {
    let h = &elf.header;
    if elf.phdrs.is_empty() {
        writeln!(out, "\nThere are no program headers in this file.")?;
        return Ok(());
    }
    writeln!(out, "\nElf file type is {}", elf_type_name(h.e_type))?;
    writeln!(out, "Entry point {:#x}", h.e_entry)?;
    writeln!(
        out,
        "There are {} program headers, starting at offset {}",
        h.e_phnum, h.e_phoff
    )?;
    writeln!(out)?;
    writeln!(out, "Program Headers:")?;

    if h.class == ELFCLASS64 {
        writeln!(
            out,
            "  Type           Offset   VirtAddr           PhysAddr           FileSiz  MemSiz   Flg Align"
        )?;
    } else {
        writeln!(
            out,
            "  Type           Offset   VirtAddr   PhysAddr   FileSiz MemSiz  Flg Align"
        )?;
    }

    for ph in &elf.phdrs {
        let type_name = phdr_type_name(ph.p_type);
        let flags = phdr_flags_str(ph.p_flags);
        if h.class == ELFCLASS64 {
            writeln!(
                out,
                "  {:<14} {:#08x} {:#018x} {:#018x} {:#08x} {:#08x} {flags} {:#x}",
                maybe_truncate(&type_name, 14, wide),
                ph.p_offset,
                ph.p_vaddr,
                ph.p_paddr,
                ph.p_filesz,
                ph.p_memsz,
                ph.p_align
            )?;
        } else {
            writeln!(
                out,
                "  {:<14} {:#08x} {:#010x} {:#010x} {:#07x} {:#07x} {flags} {:#x}",
                maybe_truncate(&type_name, 14, wide),
                ph.p_offset,
                ph.p_vaddr,
                ph.p_paddr,
                ph.p_filesz,
                ph.p_memsz,
                ph.p_align
            )?;
        }
        // Print interpreter path for PT_INTERP
        if ph.p_type == PT_INTERP {
            let off = ph.p_offset as usize;
            let sz = ph.p_filesz as usize;
            let end = off.saturating_add(sz);
            if end <= elf.data.len() {
                let interp = &elf.data[off..end];
                let len = interp.iter().position(|&b| b == 0).unwrap_or(sz);
                if let Ok(s) = std::str::from_utf8(&interp[..len]) {
                    writeln!(out, "      [Requesting program interpreter: {s}]")?;
                }
            }
        }
    }

    // Section-to-segment mapping
    if !elf.shdrs.is_empty() {
        writeln!(out, "\n Section to Segment mapping:")?;
        writeln!(out, "  Segment Sections...")?;
        for (pi, ph) in elf.phdrs.iter().enumerate() {
            let names: Vec<&str> = elf
                .shdrs
                .iter()
                .filter(|sh| {
                    sh.sh_type != SHT_NULL
                        && sh.sh_flags & SHF_ALLOC != 0
                        && sh.sh_offset >= ph.p_offset
                        && sh.sh_offset + sh.sh_size <= ph.p_offset + ph.p_filesz
                })
                .map(|sh| sh.name.as_str())
                .collect();
            writeln!(out, "   {:02}     {}", pi, names.join(" "))?;
        }
    }

    Ok(())
}

fn display_section_headers(out: &mut impl Write, elf: &Elf<'_>, wide: bool) -> Result<()> {
    if elf.shdrs.is_empty() {
        writeln!(out, "\nThere are no sections in this file.")?;
        return Ok(());
    }
    writeln!(out, "\nThere are {} section headers, starting at offset {:#x}:",
        elf.header.e_shnum, elf.header.e_shoff)?;
    writeln!(out)?;
    writeln!(out, "Section Headers:")?;

    if elf.header.class == ELFCLASS64 {
        writeln!(
            out,
            "  [Nr] Name              Type             Address          Offset\n       Size             EntSize          Flags  Link  Info  Align"
        )?;
    } else {
        writeln!(
            out,
            "  [Nr] Name              Type            Addr     Off    Size   ES Flg Lk Inf Al"
        )?;
    }

    for (i, sh) in elf.shdrs.iter().enumerate() {
        let type_name = shdr_type_name(sh.sh_type);
        let flags = shdr_flags_str(sh.sh_flags);
        let name = maybe_truncate(&sh.name, 17, wide);
        if elf.header.class == ELFCLASS64 {
            writeln!(
                out,
                "  [{i:2}] {name:<17} {type_name:<16} {:#016x}  {:#08x}\n       {:#016x} {:#016x} {flags:<6} {:<5} {:<5} {:<5}",
                sh.sh_addr,
                sh.sh_offset,
                sh.sh_size,
                sh.sh_entsize,
                sh.sh_link,
                sh.sh_info,
                sh.sh_addralign,
            )?;
        } else {
            writeln!(
                out,
                "  [{i:2}] {name:<17} {type_name:<15} {:#08x} {:#06x} {:#06x} {:02x} {flags:<3} {:<2} {:<3} {:<2}",
                sh.sh_addr,
                sh.sh_offset,
                sh.sh_size,
                sh.sh_entsize,
                sh.sh_link,
                sh.sh_info,
                sh.sh_addralign,
            )?;
        }
    }

    writeln!(out, "Key to Flags:")?;
    writeln!(out, "  W (write), A (alloc), X (execute), M (merge), S (strings), I (info),")?;
    writeln!(out, "  L (link order), T (TLS)")?;
    Ok(())
}

fn display_symbols(out: &mut impl Write, elf: &Elf<'_>, wide: bool) -> Result<()> {
    let mut found_any = false;
    for (idx, sh) in elf.shdrs.iter().enumerate() {
        if sh.sh_type != SHT_SYMTAB && sh.sh_type != SHT_DYNSYM {
            continue;
        }
        found_any = true;
        let syms = elf.parse_symbols(idx)?;
        writeln!(out, "\nSymbol table '{}' contains {} entries:", sh.name, syms.len())?;
        if elf.header.class == ELFCLASS64 {
            writeln!(out, "   Num:    Value          Size Type    Bind   Vis      Ndx Name")?;
        } else {
            writeln!(out, "   Num:    Value Size Type    Bind   Vis      Ndx Name")?;
        }
        for (i, sym) in syms.iter().enumerate() {
            let name = maybe_truncate(&sym.name, 25, wide);
            let binding = sym_binding_name(sym.binding());
            let sym_type = sym_type_name(sym.sym_type());
            let vis = sym_visibility_name(sym.visibility());
            let ndx = sym_shndx_name(sym.st_shndx);
            if elf.header.class == ELFCLASS64 {
                writeln!(
                    out,
                    "  {i:5}: {:#016x} {:<5} {:<7} {:<6} {:<8} {:<3} {name}",
                    sym.st_value, sym.st_size, sym_type, binding, vis, ndx
                )?;
            } else {
                writeln!(
                    out,
                    "  {i:5}: {:#08x} {:<4} {:<7} {:<6} {:<8} {:<3} {name}",
                    sym.st_value, sym.st_size, sym_type, binding, vis, ndx
                )?;
            }
        }
    }
    if !found_any {
        writeln!(out, "\nNo symbol tables found in file.")?;
    }
    Ok(())
}

fn display_relocs(out: &mut impl Write, elf: &Elf<'_>) -> Result<()> {
    let mut found_any = false;

    // Find .symtab or .dynsym for symbol names in reloc output
    let sym_sec = elf
        .shdrs
        .iter()
        .position(|s| s.sh_type == SHT_DYNSYM)
        .or_else(|| elf.shdrs.iter().position(|s| s.sh_type == SHT_SYMTAB));
    let syms = if let Some(idx) = sym_sec {
        elf.parse_symbols(idx).unwrap_or_default()
    } else {
        Vec::new()
    };

    for idx in 0..elf.shdrs.len() {
        let sh = &elf.shdrs[idx];
        if sh.sh_type != SHT_REL && sh.sh_type != SHT_RELA {
            continue;
        }
        found_any = true;
        let (rels, relas, name) = elf.parse_reloc_section(idx)?;
        let is_rela = sh.sh_type == SHT_RELA;
        let count = if is_rela { relas.len() } else { rels.len() };
        writeln!(out, "\nRelocation section '{name}' at offset {:#x} contains {count} entries:",
            sh.sh_offset)?;
        if elf.header.class == ELFCLASS64 {
            if is_rela {
                writeln!(out, "  Offset          Info           Type           Sym. Value    Sym. Name + Addend")?;
            } else {
                writeln!(out, "  Offset          Info           Type           Sym. Value    Sym. Name")?;
            }
        } else if is_rela {
            writeln!(out, " Offset     Info    Type            Sym.Value  Sym. Name + Addend")?;
        } else {
            writeln!(out, " Offset     Info    Type            Sym.Value  Sym. Name")?;
        }

        if is_rela {
            for rela in &relas {
                let sym_idx = rela.sym_index(elf.class()) as usize;
                let rel_type = rela.rel_type(elf.class());
                let (sym_val, sym_name) = if sym_idx > 0 && sym_idx < syms.len() {
                    (syms[sym_idx].st_value, syms[sym_idx].name.as_str())
                } else {
                    (0, "")
                };
                if elf.header.class == ELFCLASS64 {
                    writeln!(
                        out,
                        "{:#016x}  {:#016x} {rel_type:<16x} {sym_val:#016x} {sym_name} + {:#x}",
                        rela.r_offset, rela.r_info, rela.r_addend
                    )?;
                } else {
                    writeln!(
                        out,
                        "{:#08x}  {:#08x} {rel_type:<16x} {sym_val:#08x} {sym_name} + {:#x}",
                        rela.r_offset, rela.r_info, rela.r_addend
                    )?;
                }
            }
        } else {
            for rel in &rels {
                let sym_idx = rel.sym_index(elf.class()) as usize;
                let rel_type = rel.rel_type(elf.class());
                let (sym_val, sym_name) = if sym_idx > 0 && sym_idx < syms.len() {
                    (syms[sym_idx].st_value, syms[sym_idx].name.as_str())
                } else {
                    (0, "")
                };
                if elf.header.class == ELFCLASS64 {
                    writeln!(
                        out,
                        "{:#016x}  {:#016x} {rel_type:<16x} {sym_val:#016x} {sym_name}",
                        rel.r_offset, rel.r_info
                    )?;
                } else {
                    writeln!(
                        out,
                        "{:#08x}  {:#08x} {rel_type:<16x} {sym_val:#08x} {sym_name}",
                        rel.r_offset, rel.r_info
                    )?;
                }
            }
        }
    }

    if !found_any {
        writeln!(out, "\nThere are no relocations in this file.")?;
    }
    Ok(())
}

fn display_dynamic(out: &mut impl Write, elf: &Elf<'_>) -> Result<()> {
    let Some(entries) = elf.parse_dynamic()? else {
        writeln!(out, "\nThere is no dynamic section in this file.")?;
        return Ok(());
    };

    let dynstr = elf.dynstr();

    writeln!(out, "\nDynamic section at offset {:#x} contains {} entries:",
        elf.shdrs.iter()
            .find(|s| s.sh_type == SHT_DYNAMIC)
            .map_or(0, |s| s.sh_offset),
        entries.len())?;
    writeln!(out, "  Tag        Type                         Name/Value")?;

    for entry in &entries {
        let tag_name = dynentry_tag_name(entry.d_tag);
        let value_str = match entry.d_tag {
            DT_NEEDED | DT_SONAME | DT_RPATH => {
                let name = read_cstr(dynstr, entry.d_val as usize).unwrap_or("?");
                format!("Shared library: [{name}]")
            }
            DT_FLAGS | DT_FLAGS_1 => format!("{:#x}", entry.d_val),
            DT_PLTREL => {
                if entry.d_val == SHT_RELA as u64 {
                    "RELA".to_string()
                } else {
                    "REL".to_string()
                }
            }
            _ => format!("{:#x}", entry.d_val),
        };
        writeln!(out, " {:#010x} {tag_name:<32} {value_str}", entry.d_tag)?;
    }
    Ok(())
}

fn display_notes(out: &mut impl Write, elf: &Elf<'_>) -> Result<()> {
    let mut found_any = false;

    // Notes from SHT_NOTE sections
    for sh in &elf.shdrs {
        if sh.sh_type != SHT_NOTE {
            continue;
        }
        found_any = true;
        let data = match elf.section_data(elf.find_section(&sh.name).unwrap_or(usize::MAX)) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let notes = Elf::parse_notes_data(data, elf.le());
        writeln!(out, "\nDisplaying notes found in: {}", sh.name)?;
        writeln!(out, "  Owner                Data size\tDescription")?;
        for note in &notes {
            writeln!(
                out,
                "  {:<20} {:#010x}\t{}",
                note.name,
                note.desc.len(),
                note_type_name(&note.name, note.note_type)
            )?;
            if note.name == "GNU" && note.note_type == NT_GNU_BUILD_ID {
                write!(out, "    Build ID: ")?;
                for b in &note.desc {
                    write!(out, "{b:02x}")?;
                }
                writeln!(out)?;
            } else if note.name == "GNU" && note.note_type == NT_GNU_ABI_TAG && note.desc.len() >= 16 {
                let os = read_u32(&note.desc, 0, elf.le()).unwrap_or(0);
                let major = read_u32(&note.desc, 4, elf.le()).unwrap_or(0);
                let minor = read_u32(&note.desc, 8, elf.le()).unwrap_or(0);
                let patch = read_u32(&note.desc, 12, elf.le()).unwrap_or(0);
                let os_name = match os {
                    0 => "Linux",
                    1 => "GNU",
                    2 => "Solaris2",
                    3 => "FreeBSD",
                    _ => "Unknown",
                };
                writeln!(out, "    OS: {os_name}, ABI: {major}.{minor}.{patch}")?;
            }
        }
    }

    // Notes from PT_NOTE segments (for core files)
    for ph in &elf.phdrs {
        if ph.p_type != PT_NOTE {
            continue;
        }
        found_any = true;
        let off = ph.p_offset as usize;
        let sz = ph.p_filesz as usize;
        let end = off.saturating_add(sz);
        if end > elf.data.len() {
            continue;
        }
        let notes = Elf::parse_notes_data(&elf.data[off..end], elf.le());
        writeln!(out, "\nNotes at offset {off:#x} with length {sz:#x}:")?;
        writeln!(out, "  Owner                Data size\tDescription")?;
        for note in &notes {
            writeln!(
                out,
                "  {:<20} {:#010x}\t{}",
                note.name,
                note.desc.len(),
                note_type_name(&note.name, note.note_type)
            )?;
        }
    }

    if !found_any {
        writeln!(out, "\nThere are no note segments in this file.")?;
    }
    Ok(())
}

fn hex_dump_section(
    out: &mut impl Write,
    sh: &SectionHeader,
    data: &[u8],
    base_addr: u64,
) -> Result<()> {
    if data.is_empty() {
        writeln!(out, "  NOTE: This section has no data to dump.")?;
        return Ok(());
    }

    const COLS: usize = 16;
    let mut offset = 0usize;
    while offset < data.len() {
        let end = (offset + COLS).min(data.len());
        let chunk = &data[offset..end];
        let addr = base_addr + offset as u64;

        write!(out, "  0x{addr:08x} ")?;
        // Hex bytes in groups of 4
        for (i, &b) in chunk.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                write!(out, " ")?;
            }
            write!(out, "{b:02x}")?;
        }
        // Padding if last row is short
        let pad = COLS - chunk.len();
        for i in 0..pad {
            if (chunk.len() + i).is_multiple_of(4) {
                write!(out, " ")?;
            }
            write!(out, "  ")?;
        }
        write!(out, "  ")?;
        // ASCII column
        for &b in chunk {
            let c = if b >= 0x20 && b < 0x7f { b as char } else { '.' };
            write!(out, "{c}")?;
        }
        writeln!(out)?;
        offset += COLS;
    }
    // Suppress unused-variable warning for sh in call sites
    let _ = sh;
    Ok(())
}

/// Clean entry point for hex dump: resolves section and delegates.
fn do_hex_dump(out: &mut impl Write, elf: &Elf<'_>, target: &str) -> Result<()> {
    let sec_idx = if let Some(hex) = target.strip_prefix("0x").or_else(|| target.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16)
            .map_err(|_| Error::InvalidHexDumpTarget(target.to_string()))?
    } else if target.chars().all(|c| c.is_ascii_digit()) {
        target
            .parse::<usize>()
            .map_err(|_| Error::InvalidHexDumpTarget(target.to_string()))?
    } else {
        elf.find_section(target)
            .ok_or_else(|| Error::SectionNotFound(target.to_string()))?
    };

    if sec_idx >= elf.shdrs.len() {
        return Err(Error::InvalidIndex { what: "section", idx: sec_idx });
    }
    let sh = &elf.shdrs[sec_idx];
    let data = elf.section_data(sec_idx)?;
    writeln!(out, "\nHex dump of section '{}' (index {sec_idx}):", sh.name)?;
    hex_dump_section(out, sh, data, sh.sh_addr)
}

// ============================================================================
// Main entry
// ============================================================================

fn process_file(out: &mut impl Write, path: &str, opts: &Options) -> Result<()> {
    let mut f = File::open(path)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;

    let elf = Elf::parse(&data)?;

    writeln!(out, "\nFile: {path}")?;

    if opts.file_header {
        display_file_header(out, &elf)?;
    }
    if opts.program_headers {
        display_program_headers(out, &elf, opts.wide)?;
    }
    if opts.section_headers {
        display_section_headers(out, &elf, opts.wide)?;
    }
    if opts.symbols {
        display_symbols(out, &elf, opts.wide)?;
    }
    if opts.relocs {
        display_relocs(out, &elf)?;
    }
    if opts.dynamic {
        display_dynamic(out, &elf)?;
    }
    if opts.notes {
        display_notes(out, &elf)?;
    }
    for target in &opts.hex_dumps {
        do_hex_dump(out, &elf, target)?;
    }

    Ok(())
}

fn run() -> Result<()> {
    let opts = parse_args()?;
    if opts.files.is_empty() {
        eprintln!("readelf: no input files");
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut had_error = false;
    for path in &opts.files {
        if let Err(e) = process_file(&mut out, path, &opts) {
            eprintln!("readelf: {path}: {e}");
            had_error = true;
        }
    }
    out.flush()?;

    if had_error {
        process::exit(1);
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("readelf: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Byte-reading helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_u16_le() {
        let data = [0x01u8, 0x02];
        assert_eq!(read_u16(&data, 0, true).unwrap(), 0x0201);
    }

    #[test]
    fn test_read_u16_be() {
        let data = [0x01u8, 0x02];
        assert_eq!(read_u16(&data, 0, false).unwrap(), 0x0102);
    }

    #[test]
    fn test_read_u16_truncated() {
        let data = [0x01u8];
        assert!(read_u16(&data, 0, true).is_err());
    }

    #[test]
    fn test_read_u32_le() {
        let data = [0x01u8, 0x02, 0x03, 0x04];
        assert_eq!(read_u32(&data, 0, true).unwrap(), 0x0403_0201);
    }

    #[test]
    fn test_read_u32_be() {
        let data = [0x01u8, 0x02, 0x03, 0x04];
        assert_eq!(read_u32(&data, 0, false).unwrap(), 0x0102_0304);
    }

    #[test]
    fn test_read_u64_le() {
        let data = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        assert_eq!(read_u64(&data, 0, true).unwrap(), 0x0807_0605_0403_0201);
    }

    #[test]
    fn test_read_u64_be() {
        let data = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        assert_eq!(read_u64(&data, 0, false).unwrap(), 0x0102_0304_0506_0708);
    }

    #[test]
    fn test_read_u64_truncated() {
        let data = [0u8; 4];
        assert!(read_u64(&data, 0, true).is_err());
    }

    #[test]
    fn test_read_u32_offset() {
        let data = [0x00u8, 0x00, 0xAB, 0xCD, 0xEF, 0x01];
        assert_eq!(read_u32(&data, 2, false).unwrap(), 0xABCD_EF01);
    }

    #[test]
    fn test_read_cstr_basic() {
        let data = b"hello\0world";
        assert_eq!(read_cstr(data, 0).unwrap(), "hello");
    }

    #[test]
    fn test_read_cstr_offset() {
        let data = b"hello\0world\0";
        assert_eq!(read_cstr(data, 6).unwrap(), "world");
    }

    #[test]
    fn test_read_cstr_out_of_bounds() {
        let data = b"hello";
        assert_eq!(read_cstr(data, 100).unwrap(), "");
    }

    #[test]
    fn test_read_cstr_no_null() {
        let data = b"hello";
        assert_eq!(read_cstr(data, 0).unwrap(), "hello");
    }

    // -----------------------------------------------------------------------
    // ELF magic / header rejection
    // -----------------------------------------------------------------------

    #[test]
    fn test_not_elf_bad_magic() {
        let data = [0u8; 64];
        assert!(matches!(Elf::parse(&data), Err(Error::NotElf)));
    }

    #[test]
    fn test_truncated_header() {
        let data = [0x7f, b'E', b'L', b'F'];
        assert!(matches!(Elf::parse(&data), Err(Error::TruncatedHeader)));
    }

    #[test]
    fn test_invalid_class() {
        let mut data = [0u8; 64];
        data[..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = 99; // invalid
        data[EI_DATA] = ELFDATA2LSB;
        assert!(matches!(Elf::parse(&data), Err(Error::InvalidClass(99))));
    }

    #[test]
    fn test_invalid_encoding() {
        let mut data = [0u8; 64];
        data[..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = 99; // invalid
        assert!(matches!(Elf::parse(&data), Err(Error::InvalidEncoding(99))));
    }

    // -----------------------------------------------------------------------
    // Minimal valid ELF64 header (no phdrs/shdrs)
    // -----------------------------------------------------------------------

    fn make_minimal_elf64() -> Vec<u8> {
        let mut data = vec![0u8; 64];
        data[..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = ELFDATA2LSB;
        data[EI_VERSION] = 1;
        data[EI_OSABI] = ELFOSABI_NONE;
        // e_type = ET_EXEC at offset 16
        data[16] = ET_EXEC as u8;
        // e_machine = EM_X86_64 at offset 18
        let machine = EM_X86_64.to_le_bytes();
        data[18] = machine[0];
        data[19] = machine[1];
        // e_version = 1
        data[20] = 1;
        // e_ehsize = 64
        data[52] = 64;
        // e_phentsize = 56
        data[54] = 56;
        // e_shentsize = 64
        data[58] = 64;
        data
    }

    #[test]
    fn test_minimal_elf64_parses() {
        let data = make_minimal_elf64();
        let elf = Elf::parse(&data).unwrap();
        assert_eq!(elf.header.class, ELFCLASS64);
        assert_eq!(elf.header.e_type, ET_EXEC);
        assert_eq!(elf.header.e_machine, EM_X86_64);
        assert!(elf.header.little_endian);
        assert!(elf.phdrs.is_empty());
        assert!(elf.shdrs.is_empty());
    }

    #[test]
    fn test_minimal_elf64_no_sections_display() {
        let data = make_minimal_elf64();
        let elf = Elf::parse(&data).unwrap();
        let mut out = Vec::new();
        display_section_headers(&mut out, &elf, false).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("no sections"));
    }

    #[test]
    fn test_minimal_elf64_no_phdrs_display() {
        let data = make_minimal_elf64();
        let elf = Elf::parse(&data).unwrap();
        let mut out = Vec::new();
        display_program_headers(&mut out, &elf, false).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("no program headers"));
    }

    // -----------------------------------------------------------------------
    // Minimal valid ELF32 header
    // -----------------------------------------------------------------------

    fn make_minimal_elf32() -> Vec<u8> {
        let mut data = vec![0u8; 52];
        data[..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS32;
        data[EI_DATA] = ELFDATA2LSB;
        data[EI_VERSION] = 1;
        // e_type = ET_REL
        data[16] = ET_REL as u8;
        // e_machine = EM_386
        data[18] = EM_386 as u8;
        // e_version = 1
        data[20] = 1;
        // e_ehsize = 52
        data[40] = 52;
        // e_phentsize = 32
        data[42] = 32;
        // e_shentsize = 40
        data[46] = 40;
        data
    }

    #[test]
    fn test_minimal_elf32_parses() {
        let data = make_minimal_elf32();
        let elf = Elf::parse(&data).unwrap();
        assert_eq!(elf.header.class, ELFCLASS32);
        assert_eq!(elf.header.e_type, ET_REL);
        assert_eq!(elf.header.e_machine, EM_386);
    }

    // -----------------------------------------------------------------------
    // Name helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_elf_type_name() {
        assert!(elf_type_name(ET_EXEC).contains("EXEC"));
        assert!(elf_type_name(ET_DYN).contains("DYN"));
        assert!(elf_type_name(ET_NONE).contains("NONE"));
        assert!(elf_type_name(ET_CORE).contains("CORE"));
        assert!(elf_type_name(ET_REL).contains("REL"));
    }

    #[test]
    fn test_machine_name() {
        assert_eq!(machine_name(EM_X86_64), "Advanced Micro Devices X86-64");
        assert_eq!(machine_name(EM_ARM), "ARM");
        assert_eq!(machine_name(EM_AARCH64), "AArch64");
        assert_eq!(machine_name(EM_RISCV), "RISC-V");
        assert_eq!(machine_name(EM_386), "Intel 80386");
    }

    #[test]
    fn test_phdr_type_name() {
        assert_eq!(phdr_type_name(PT_LOAD), "LOAD");
        assert_eq!(phdr_type_name(PT_DYNAMIC), "DYNAMIC");
        assert_eq!(phdr_type_name(PT_NOTE), "NOTE");
        assert_eq!(phdr_type_name(PT_GNU_STACK), "GNU_STACK");
    }

    #[test]
    fn test_shdr_type_name() {
        assert_eq!(shdr_type_name(SHT_PROGBITS), "PROGBITS");
        assert_eq!(shdr_type_name(SHT_SYMTAB), "SYMTAB");
        assert_eq!(shdr_type_name(SHT_STRTAB), "STRTAB");
        assert_eq!(shdr_type_name(SHT_RELA), "RELA");
        assert_eq!(shdr_type_name(SHT_DYNSYM), "DYNSYM");
        assert_eq!(shdr_type_name(SHT_NOTE), "NOTE");
    }

    #[test]
    fn test_shdr_flags_str() {
        assert_eq!(shdr_flags_str(SHF_WRITE | SHF_ALLOC | SHF_EXECINSTR), "WAX");
        assert_eq!(shdr_flags_str(SHF_ALLOC), "A");
        assert_eq!(shdr_flags_str(0), "");
    }

    #[test]
    fn test_phdr_flags_str() {
        assert_eq!(phdr_flags_str(PF_R | PF_W | PF_X), "RWE");
        assert_eq!(phdr_flags_str(PF_R | PF_X), "R E");
        assert_eq!(phdr_flags_str(PF_R), "R  ");
    }

    #[test]
    fn test_sym_binding_name() {
        assert_eq!(sym_binding_name(STB_LOCAL), "LOCAL");
        assert_eq!(sym_binding_name(STB_GLOBAL), "GLOBAL");
        assert_eq!(sym_binding_name(STB_WEAK), "WEAK");
    }

    #[test]
    fn test_sym_type_name() {
        assert_eq!(sym_type_name(STT_FUNC), "FUNC");
        assert_eq!(sym_type_name(STT_OBJECT), "OBJECT");
        assert_eq!(sym_type_name(STT_NOTYPE), "NOTYPE");
        assert_eq!(sym_type_name(STT_SECTION), "SECTION");
        assert_eq!(sym_type_name(STT_FILE), "FILE");
    }

    #[test]
    fn test_sym_shndx_name() {
        assert_eq!(sym_shndx_name(SHN_UNDEF), "UND");
        assert_eq!(sym_shndx_name(SHN_ABS), "ABS");
        assert_eq!(sym_shndx_name(SHN_COMMON), "COM");
        assert_eq!(sym_shndx_name(5), "5");
    }

    // -----------------------------------------------------------------------
    // Note parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_note_parsing_empty() {
        let notes = parse_note_entries(&[], true);
        assert!(notes.is_empty());
    }

    #[test]
    fn test_note_parsing_truncated() {
        // Only 8 bytes — not enough for even a minimal note header (12 bytes)
        let data = [0u8; 8];
        let notes = parse_note_entries(&data, true);
        assert!(notes.is_empty());
    }

    #[test]
    fn test_note_parsing_basic() {
        // Build a minimal GNU build-id note
        // namesz=4, descsz=4, type=NT_GNU_BUILD_ID, name="GNU\0", desc=[1,2,3,4]
        let mut note = Vec::new();
        note.extend_from_slice(&4u32.to_le_bytes()); // namesz
        note.extend_from_slice(&4u32.to_le_bytes()); // descsz
        note.extend_from_slice(&NT_GNU_BUILD_ID.to_le_bytes()); // type
        note.extend_from_slice(b"GNU\0"); // name (already 4 bytes, no padding needed)
        note.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]); // desc
        let notes = parse_note_entries(&note, true);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].name, "GNU");
        assert_eq!(notes[0].note_type, NT_GNU_BUILD_ID);
        assert_eq!(notes[0].desc, [0xde, 0xad, 0xbe, 0xef]);
    }

    // -----------------------------------------------------------------------
    // Rel / Rela parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_rel_table_64() {
        // Two REL entries: each 16 bytes (offset 8 + info 8)
        let mut data = Vec::new();
        data.extend_from_slice(&0x1000u64.to_le_bytes()); // r_offset
        data.extend_from_slice(&0x0001_0000_0001u64.to_le_bytes()); // r_info (sym=1, type=1)
        data.extend_from_slice(&0x2000u64.to_le_bytes());
        data.extend_from_slice(&0x0002_0000_0002u64.to_le_bytes());
        let rels = parse_rel_table(&data, ELFCLASS64, true).unwrap();
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0].r_offset, 0x1000);
        assert_eq!(rels[1].r_offset, 0x2000);
    }

    #[test]
    fn test_parse_rela_table_64() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x4000u64.to_le_bytes()); // r_offset
        data.extend_from_slice(&0x0001_0000_0001u64.to_le_bytes()); // r_info
        data.extend_from_slice(&(-8i64).to_le_bytes()); // r_addend
        let relas = parse_rela_table(&data, ELFCLASS64, true).unwrap();
        assert_eq!(relas.len(), 1);
        assert_eq!(relas[0].r_offset, 0x4000);
        assert_eq!(relas[0].r_addend, -8);
    }

    // -----------------------------------------------------------------------
    // Hex dump helper
    // -----------------------------------------------------------------------

    #[test]
    fn test_hex_dump_section_empty() {
        let sh = SectionHeader {
            sh_name: 0,
            sh_type: SHT_PROGBITS,
            sh_flags: 0,
            sh_addr: 0,
            sh_offset: 0,
            sh_size: 0,
            sh_link: 0,
            sh_info: 0,
            sh_addralign: 1,
            sh_entsize: 0,
            name: ".test".to_string(),
        };
        let mut out = Vec::new();
        hex_dump_section(&mut out, &sh, &[], 0).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("no data"));
    }

    #[test]
    fn test_hex_dump_section_data() {
        let sh = SectionHeader {
            sh_name: 0,
            sh_type: SHT_PROGBITS,
            sh_flags: 0,
            sh_addr: 0x1000,
            sh_offset: 0,
            sh_size: 4,
            sh_link: 0,
            sh_info: 0,
            sh_addralign: 1,
            sh_entsize: 0,
            name: ".test".to_string(),
        };
        let data = [0xde, 0xad, 0xbe, 0xef];
        let mut out = Vec::new();
        hex_dump_section(&mut out, &sh, &data, 0x1000).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("dead"));
        assert!(s.contains("beef") || s.contains("be")); // bytes present
        assert!(s.contains("0x00001000"));
    }

    // -----------------------------------------------------------------------
    // Symbol helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_symbol_binding_type_extraction() {
        let sym = Symbol {
            st_name: 0,
            st_info: (STB_GLOBAL << 4) | STT_FUNC,
            st_other: STV_HIDDEN,
            st_shndx: 1,
            st_value: 0x1000,
            st_size: 64,
            name: "foo".to_string(),
        };
        assert_eq!(sym.binding(), STB_GLOBAL);
        assert_eq!(sym.sym_type(), STT_FUNC);
        assert_eq!(sym.visibility(), STV_HIDDEN);
    }

    #[test]
    fn test_symbol_visibility_names() {
        assert_eq!(sym_visibility_name(STV_DEFAULT), "DEFAULT");
        assert_eq!(sym_visibility_name(STV_HIDDEN), "HIDDEN");
        assert_eq!(sym_visibility_name(STV_PROTECTED), "PROTECTED");
        assert_eq!(sym_visibility_name(STV_INTERNAL), "INTERNAL");
    }

    // -----------------------------------------------------------------------
    // osabi / class / data name
    // -----------------------------------------------------------------------

    #[test]
    fn test_osabi_names() {
        assert_eq!(osabi_name(ELFOSABI_NONE), "UNIX - System V");
        assert_eq!(osabi_name(ELFOSABI_LINUX), "Linux");
        assert_eq!(osabi_name(ELFOSABI_OUROS), "OurOS");
    }

    #[test]
    fn test_class_data_names() {
        assert_eq!(class_name(ELFCLASS32), "ELF32");
        assert_eq!(class_name(ELFCLASS64), "ELF64");
        assert_eq!(data_name(ELFDATA2LSB), "2's complement, little endian");
        assert_eq!(data_name(ELFDATA2MSB), "2's complement, big endian");
    }

    // -----------------------------------------------------------------------
    // maybe_truncate
    // -----------------------------------------------------------------------

    #[test]
    fn test_maybe_truncate_wide() {
        let s = "a".repeat(100);
        assert_eq!(maybe_truncate(&s, 10, true).len(), 100);
    }

    #[test]
    fn test_maybe_truncate_narrow() {
        let s = "a".repeat(20);
        assert_eq!(maybe_truncate(&s, 10, false).len(), 10);
    }

    #[test]
    fn test_maybe_truncate_short_string() {
        let s = "hello";
        assert_eq!(maybe_truncate(s, 10, false), "hello");
    }

    // -----------------------------------------------------------------------
    // File header display smoke test
    // -----------------------------------------------------------------------

    #[test]
    fn test_display_file_header_smoke() {
        let data = make_minimal_elf64();
        let elf = Elf::parse(&data).unwrap();
        let mut out = Vec::new();
        display_file_header(&mut out, &elf).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("ELF Header:"));
        assert!(s.contains("Magic:"));
        assert!(s.contains("ELF64"));
        assert!(s.contains("little endian"));
        assert!(s.contains("EXEC"));
        assert!(s.contains("X86-64"));
    }

    // -----------------------------------------------------------------------
    // Rel sym_index / rel_type extraction
    // -----------------------------------------------------------------------

    #[test]
    fn test_rel_info_extraction_64() {
        let r = Rel { r_offset: 0, r_info: 0x0000_0002_0000_0001 };
        assert_eq!(r.sym_index(ELFCLASS64), 2);
        assert_eq!(r.rel_type(ELFCLASS64), 1);
    }

    #[test]
    fn test_rel_info_extraction_32() {
        // For ELF32: sym = r_info >> 8, type = r_info & 0xff
        let r = Rel { r_offset: 0, r_info: (5u64 << 8) | 7 };
        assert_eq!(r.sym_index(ELFCLASS32), 5);
        assert_eq!(r.rel_type(ELFCLASS32), 7);
    }

    // -----------------------------------------------------------------------
    // dynentry_tag_name
    // -----------------------------------------------------------------------

    #[test]
    fn test_dynentry_tag_names() {
        assert_eq!(dynentry_tag_name(DT_NEEDED), "(NEEDED)");
        assert_eq!(dynentry_tag_name(DT_SONAME), "(SONAME)");
        assert_eq!(dynentry_tag_name(DT_NULL), "(NULL)");
        assert_eq!(dynentry_tag_name(DT_STRTAB), "(STRTAB)");
    }
}
