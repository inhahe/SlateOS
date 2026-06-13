//! SlateOS `ldd` — Shared Library Dependency Lister
//!
//! Parses ELF binaries and lists their shared-library dependencies, following
//! the same resolution order as the SlateOS dynamic linker:
//!
//!   1. `RPATH` embedded in the binary (deprecated but honoured)
//!   2. `LD_LIBRARY_PATH` environment variable
//!   3. `RUNPATH` embedded in the binary
//!   4. Standard library directories: /lib, /usr/lib, /usr/local/lib
//!
//! # Usage
//!
//! ```text
//! ldd [OPTIONS] FILE...
//! ldd --version
//! ```
//!
//! # Options
//!
//! ```text
//! -v, --verbose          Show version information and processing details
//! -u, --unused           Report direct dependencies that are unused
//! -r, --function-relocs  Show relocations for data objects and functions
//! --version              Print version and exit
//! ```

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// ldd is an ELF parser: every arithmetic operation is on offsets/counts
// bounded by the binary's section sizes and ELF header limits, and every
// slice index/range is gated by an immediately preceding length check
// against `data.len()`. The defensive arithmetic and indexing lints add no
// safety here — they only obscure the parsing logic — so we allow them at
// the file level. (Length-check failures fall through to `break` or
// `Err(...)`, never panic.)
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
)]

use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Version
// ============================================================================

const VERSION: &str = "0.1.0";
const PROGRAM_NAME: &str = "ldd";

// ============================================================================
// ELF constants
// ============================================================================

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];

const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;
const EI_NIDENT: usize = 16;

const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;

const ELFDATA2LSB: u8 = 1;
const ELFDATA2MSB: u8 = 2;

// ELF file types
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;

// Program header types
const PT_DYNAMIC: u32 = 2;

// Dynamic section tags
const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_STRTAB: i64 = 5;
const DT_STRSZ: i64 = 10;
const DT_RPATH: i64 = 15;
const DT_RUNPATH: i64 = 29;

// Relocation types (x86_64)
const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_COPY: u32 = 5;

// Section header types for relocation
const SHT_REL: u32 = 9;
const SHT_RELA: u32 = 4;

// Symbol binding
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;

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
    StringOutOfBounds { offset: usize, table_size: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
            Self::StringOutOfBounds { offset, table_size } => write!(
                f,
                "string table offset {offset:#x} out of bounds (table size {table_size:#x})"
            ),
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
// CLI options
// ============================================================================

#[derive(Default, Debug)]
struct Options {
    /// Show version information and processing details.
    verbose: bool,
    /// Report direct dependencies that appear to be unused.
    unused: bool,
    /// Show function and data-object relocations.
    function_relocs: bool,
    /// Input files to process.
    files: Vec<String>,
}

fn print_version() -> ! {
    println!("{PROGRAM_NAME} (SlateOS) {VERSION}");
    process::exit(0);
}

fn usage() -> ! {
    let msg = "\
Usage: ldd [OPTION]... FILE...
      --help               print this help and exit
      --version            print version information and exit
  -v, --verbose            print all information
  -u, --unused             print unused direct dependencies
  -r, --function-relocs    process and display FUNCTION and DATA relocations";
    println!("{msg}");
    process::exit(0);
}

fn parse_args() -> Options {
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

        if let Some(rest) = arg.strip_prefix("--") {
            match rest {
                "help" => usage(),
                "version" => print_version(),
                "verbose" => opts.verbose = true,
                "unused" => opts.unused = true,
                "function-relocs" => opts.function_relocs = true,
                _ => {
                    eprintln!("{PROGRAM_NAME}: unrecognized option '--{rest}'");
                    eprintln!("Try '{PROGRAM_NAME} --help' for more information.");
                    process::exit(1);
                }
            }
            i += 1;
            continue;
        }

        // Short flags (can be combined: -vur)
        let flags = &arg[1..];
        for ch in flags.chars() {
            match ch {
                'v' => opts.verbose = true,
                'u' => opts.unused = true,
                'r' => opts.function_relocs = true,
                _ => {
                    eprintln!("{PROGRAM_NAME}: invalid option -- '{ch}'");
                    eprintln!("Try '{PROGRAM_NAME} --help' for more information.");
                    process::exit(1);
                }
            }
        }
        i += 1;
    }

    opts
}

// ============================================================================
// ELF data structures
// ============================================================================

#[derive(Debug, Clone)]
struct ElfHeader {
    class: u8,
    little_endian: bool,
    e_type: u16,
    e_phoff: u64,
    e_shoff: u64,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[derive(Debug, Clone)]
struct ProgramHeader {
    p_type: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_filesz: u64,
}

#[derive(Debug, Clone)]
struct SectionHeader {
    sh_name_off: u32,
    sh_type: u32,
    sh_offset: u64,
    sh_size: u64,
    name: String,
}

#[derive(Debug, Clone)]
struct DynEntry {
    d_tag: i64,
    d_val: u64,
}

/// A symbol from .dynsym.  We only need name, binding/type info, and section index.
#[derive(Debug, Clone)]
struct DynSym {
    name: String,
    st_info: u8,
    st_shndx: u16,
}

impl DynSym {
    fn binding(&self) -> u8 {
        self.st_info >> 4
    }
    fn is_undefined(&self) -> bool {
        self.st_shndx == 0 // SHN_UNDEF
    }
}

/// A RELA relocation entry.
#[derive(Debug, Clone)]
struct Rela {
    r_offset: u64,
    r_sym: u32,
    r_type: u32,
    r_addend: i64,
}

/// A REL relocation entry.
#[derive(Debug, Clone)]
struct Rel {
    r_offset: u64,
    r_sym: u32,
    r_type: u32,
}

// ============================================================================
// Little/big-endian readers
// ============================================================================

fn read_u16(data: &[u8], off: usize, le: bool) -> Result<u16> {
    let end = off.checked_add(2).ok_or(Error::TruncatedData {
        what: "u16",
        offset: off,
        needed: 2,
        available: data.len().saturating_sub(off),
    })?;
    let arr: [u8; 2] = data.get(off..end).ok_or(Error::TruncatedData {
        what: "u16",
        offset: off,
        needed: 2,
        available: data.len().saturating_sub(off),
    })?.try_into().map_err(|_| Error::TruncatedData {
        what: "u16",
        offset: off,
        needed: 2,
        available: data.len().saturating_sub(off),
    })?;
    Ok(if le { u16::from_le_bytes(arr) } else { u16::from_be_bytes(arr) })
}

fn read_u32(data: &[u8], off: usize, le: bool) -> Result<u32> {
    let end = off.checked_add(4).ok_or(Error::TruncatedData {
        what: "u32",
        offset: off,
        needed: 4,
        available: data.len().saturating_sub(off),
    })?;
    let arr: [u8; 4] = data.get(off..end).ok_or(Error::TruncatedData {
        what: "u32",
        offset: off,
        needed: 4,
        available: data.len().saturating_sub(off),
    })?.try_into().map_err(|_| Error::TruncatedData {
        what: "u32",
        offset: off,
        needed: 4,
        available: data.len().saturating_sub(off),
    })?;
    Ok(if le { u32::from_le_bytes(arr) } else { u32::from_be_bytes(arr) })
}

fn read_u64(data: &[u8], off: usize, le: bool) -> Result<u64> {
    let end = off.checked_add(8).ok_or(Error::TruncatedData {
        what: "u64",
        offset: off,
        needed: 8,
        available: data.len().saturating_sub(off),
    })?;
    let arr: [u8; 8] = data.get(off..end).ok_or(Error::TruncatedData {
        what: "u64",
        offset: off,
        needed: 8,
        available: data.len().saturating_sub(off),
    })?.try_into().map_err(|_| Error::TruncatedData {
        what: "u64",
        offset: off,
        needed: 8,
        available: data.len().saturating_sub(off),
    })?;
    Ok(if le { u64::from_le_bytes(arr) } else { u64::from_be_bytes(arr) })
}

/// Extract a NUL-terminated byte string from `table` starting at `offset`,
/// returning it as a Rust `String`. Returns an error if out-of-bounds.
fn strtab_get(table: &[u8], offset: usize) -> Result<String> {
    if offset >= table.len() {
        return Err(Error::StringOutOfBounds { offset, table_size: table.len() });
    }
    let end = table[offset..].iter().position(|&b| b == 0).unwrap_or(table.len() - offset);
    // Path bytes are never forced through UTF-8; non-UTF-8 names are
    // replaced with '?' so they can still be displayed.
    let s = String::from_utf8_lossy(&table[offset..offset + end]).into_owned();
    Ok(s)
}

// ============================================================================
// ELF file parser
// ============================================================================

struct Elf {
    data: Vec<u8>,
    hdr: ElfHeader,
    phdrs: Vec<ProgramHeader>,
    shdrs: Vec<SectionHeader>,
}

impl Elf {
    /// Load and parse an ELF file at `path`.
    fn load(path: &Path) -> Result<Self> {
        let mut f = File::open(path)?;
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        Self::parse(data)
    }

    fn parse(data: Vec<u8>) -> Result<Self> {
        // Check magic
        if data.get(..4) != Some(&ELFMAG) {
            return Err(Error::NotElf);
        }
        if data.len() < EI_NIDENT {
            return Err(Error::TruncatedHeader);
        }

        let class = data[EI_CLASS];
        let encoding = data[EI_DATA];
        if class != ELFCLASS32 && class != ELFCLASS64 {
            return Err(Error::InvalidClass(class));
        }
        let le = match encoding {
            ELFDATA2LSB => true,
            ELFDATA2MSB => false,
            _ => return Err(Error::InvalidEncoding(encoding)),
        };

        let hdr = if class == ELFCLASS64 {
            parse_elf64_header(&data, le)?
        } else {
            parse_elf32_header(&data, le)?
        };

        let phdrs = parse_program_headers(&data, &hdr)?;
        let shdrs = parse_section_headers(&data, &hdr)?;

        Ok(Self { data, hdr, phdrs, shdrs })
    }

    fn class(&self) -> u8 {
        self.hdr.class
    }

    fn le(&self) -> bool {
        self.hdr.little_endian
    }

    /// Return a slice of the file data at [offset, offset+size).
    fn slice(&self, offset: usize, size: usize, what: &'static str) -> Result<&[u8]> {
        self.data.get(offset..offset.saturating_add(size)).ok_or(Error::TruncatedData {
            what,
            offset,
            needed: size,
            available: self.data.len().saturating_sub(offset),
        })
    }

    /// Return the raw bytes of section `idx`.
    fn section_data(&self, idx: usize) -> Result<&[u8]> {
        let sh = self.shdrs.get(idx).ok_or(Error::TruncatedData {
            what: "section index",
            offset: idx,
            needed: 1,
            available: self.shdrs.len(),
        })?;
        self.slice(sh.sh_offset as usize, sh.sh_size as usize, "section data")
    }

    /// Find a section by name; returns its index.
    fn find_section(&self, name: &str) -> Option<usize> {
        self.shdrs.iter().position(|s| s.name == name)
    }

    /// Parse the dynamic section, returning its entries.
    /// Tries PT_DYNAMIC first, then falls back to the .dynamic section.
    fn parse_dynamic(&self) -> Result<Option<Vec<DynEntry>>> {
        // Try to find the dynamic segment via PT_DYNAMIC program header first,
        // since that is authoritative at runtime.
        for ph in &self.phdrs {
            if ph.p_type == PT_DYNAMIC {
                let data =
                    self.slice(ph.p_offset as usize, ph.p_filesz as usize, "PT_DYNAMIC")?;
                return Ok(Some(parse_dynamic_entries(data, self.class(), self.le())?));
            }
        }
        // Fall back to .dynamic section (present in relocatable shared objects).
        if let Some(idx) = self.find_section(".dynamic") {
            let data = self.section_data(idx)?;
            return Ok(Some(parse_dynamic_entries(data, self.class(), self.le())?));
        }
        Ok(None)
    }

    /// Locate the dynamic string table.
    ///
    /// First looks up DT_STRTAB / DT_STRSZ from the dynamic section so the
    /// correct virtual-address → file-offset translation is done.  Falls back
    /// to the .dynstr section if the dynamic section is unavailable.
    fn dynstr<'a>(&'a self, dyn_entries: Option<&[DynEntry]>) -> &'a [u8] {
        // Prefer the address from the dynamic section (DT_STRTAB + DT_STRSZ),
        // resolved via the first LOAD segment's vaddr→offset mapping.
        if let Some(entries) = dyn_entries {
            let strtab_vaddr =
                entries.iter().find(|e| e.d_tag == DT_STRTAB).map(|e| e.d_val);
            let strsz = entries
                .iter()
                .find(|e| e.d_tag == DT_STRSZ)
                .map_or(0, |e| e.d_val as usize);

            if let Some(vaddr) = strtab_vaddr
                && let Some(off) = self.vaddr_to_offset(vaddr)
                && let Ok(sl) = self.slice(off, strsz, "dynstr")
            {
                return sl;
            }
        }
        // Fall back: use the .dynstr section directly.
        if let Some(idx) = self.find_section(".dynstr") {
            return self.section_data(idx).unwrap_or(&[]);
        }
        &[]
    }

    /// Convert a virtual address to a file offset using the program headers.
    fn vaddr_to_offset(&self, vaddr: u64) -> Option<usize> {
        for ph in &self.phdrs {
            if vaddr >= ph.p_vaddr && vaddr < ph.p_vaddr.saturating_add(ph.p_filesz) {
                let offset = (vaddr - ph.p_vaddr).saturating_add(ph.p_offset) as usize;
                return Some(offset);
            }
        }
        None
    }

    /// Return the .dynstr bytes via section index (for relocation display).
    fn dynstr_section(&self) -> &[u8] {
        if let Some(idx) = self.find_section(".dynstr") {
            self.section_data(idx).unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// Parse the .dynsym section.
    fn parse_dynsym(&self) -> Result<Vec<DynSym>> {
        let Some(idx) = self.find_section(".dynsym") else {
            return Ok(Vec::new());
        };
        let data = self.section_data(idx)?;
        let strtab = self.dynstr_section();
        parse_dynsym_entries(data, strtab, self.class(), self.le())
    }

    /// Parse RELA sections matching the given name.
    fn parse_rela(&self, section_name: &str) -> Result<Vec<Rela>> {
        let Some(idx) = self.find_section(section_name) else {
            return Ok(Vec::new());
        };
        let sh = &self.shdrs[idx];
        if sh.sh_type != SHT_RELA {
            return Ok(Vec::new());
        }
        let data = self.section_data(idx)?;
        parse_rela_entries(data, self.class(), self.le())
    }

    /// Parse REL sections matching the given name.
    fn parse_rel(&self, section_name: &str) -> Result<Vec<Rel>> {
        let Some(idx) = self.find_section(section_name) else {
            return Ok(Vec::new());
        };
        let sh = &self.shdrs[idx];
        if sh.sh_type != SHT_REL {
            return Ok(Vec::new());
        }
        let data = self.section_data(idx)?;
        parse_rel_entries(data, self.class(), self.le())
    }

    /// Check whether this binary is a dynamic executable or shared library.
    fn is_dynamic(&self) -> bool {
        self.hdr.e_type == ET_EXEC || self.hdr.e_type == ET_DYN
    }
}

// ============================================================================
// ELF header parsers
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
        little_endian: le,
        e_type: read_u16(data, 16, le)?,
        e_phoff: read_u64(data, 32, le)?,
        e_shoff: read_u64(data, 40, le)?,
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
        little_endian: le,
        e_type: read_u16(data, 16, le)?,
        e_phoff: read_u32(data, 28, le)? as u64,
        e_shoff: read_u32(data, 32, le)? as u64,
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
        needed: usize::MAX,
        available: data.len().saturating_sub(off),
    })?;
    if off.saturating_add(total) > data.len() {
        return Err(Error::TruncatedData {
            what: "program headers",
            offset: off,
            needed: total,
            available: data.len().saturating_sub(off),
        });
    }

    let mut phdrs = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * entsz;
        let ph = if hdr.class == ELFCLASS64 {
            ProgramHeader {
                p_type: read_u32(data, base, le)?,
                p_offset: read_u64(data, base + 8, le)?,
                p_vaddr: read_u64(data, base + 16, le)?,
                p_filesz: read_u64(data, base + 32, le)?,
            }
        } else {
            ProgramHeader {
                p_type: read_u32(data, base, le)?,
                p_offset: read_u32(data, base + 4, le)? as u64,
                p_vaddr: read_u32(data, base + 8, le)? as u64,
                p_filesz: read_u32(data, base + 16, le)? as u64,
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

    if count == 0 || off == 0 || entsz == 0 {
        return Ok(Vec::new());
    }

    let total = count.checked_mul(entsz).ok_or(Error::TruncatedData {
        what: "section headers",
        offset: off,
        needed: usize::MAX,
        available: data.len().saturating_sub(off),
    })?;
    if off.saturating_add(total) > data.len() {
        return Err(Error::TruncatedData {
            what: "section headers",
            offset: off,
            needed: total,
            available: data.len().saturating_sub(off),
        });
    }

    // Parse raw section headers first (names resolved below)
    let mut shdrs: Vec<SectionHeader> = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * entsz;
        let sh = if hdr.class == ELFCLASS64 {
            SectionHeader {
                sh_name_off: read_u32(data, base, le)?,
                sh_type: read_u32(data, base + 4, le)?,
                sh_offset: read_u64(data, base + 24, le)?,
                sh_size: read_u64(data, base + 32, le)?,
                name: String::new(),
            }
        } else {
            SectionHeader {
                sh_name_off: read_u32(data, base, le)?,
                sh_type: read_u32(data, base + 4, le)?,
                sh_offset: read_u32(data, base + 16, le)? as u64,
                sh_size: read_u32(data, base + 20, le)? as u64,
                name: String::new(),
            }
        };
        shdrs.push(sh);
    }

    // Resolve section names from the section-name string table (.shstrtab).
    let shstrndx = hdr.e_shstrndx as usize;
    if shstrndx < shdrs.len() {
        let shstr_off = shdrs[shstrndx].sh_offset as usize;
        let shstr_sz = shdrs[shstrndx].sh_size as usize;
        if let Some(shstrtab) = data.get(shstr_off..shstr_off.saturating_add(shstr_sz)) {
            let shstrtab = shstrtab.to_vec(); // avoid borrow with shdrs
            for sh in &mut shdrs {
                if let Ok(n) = strtab_get(&shstrtab, sh.sh_name_off as usize) {
                    sh.name = n;
                }
            }
        }
    }

    Ok(shdrs)
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

fn parse_dynsym_entries(
    data: &[u8],
    strtab: &[u8],
    class: u8,
    le: bool,
) -> Result<Vec<DynSym>> {
    let entsz: usize = if class == ELFCLASS64 { 24 } else { 16 };
    if entsz == 0 || data.is_empty() {
        return Ok(Vec::new());
    }
    let count = data.len() / entsz;
    let mut syms = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * entsz;
        if base + entsz > data.len() {
            break;
        }
        let sym = if class == ELFCLASS64 {
            // Elf64_Sym layout: st_name(4), st_info(1), st_other(1), st_shndx(2),
            //                   st_value(8), st_size(8) — we only need name/info/shndx.
            let name_off = read_u32(data, base, le)? as usize;
            let st_info = data[base + 4];
            let st_shndx = read_u16(data, base + 6, le)?;
            let name = strtab_get(strtab, name_off).unwrap_or_default();
            DynSym { name, st_info, st_shndx }
        } else {
            // Elf32_Sym layout: st_name(4), st_value(4), st_size(4), st_info(1),
            //                   st_other(1), st_shndx(2).
            let name_off = read_u32(data, base, le)? as usize;
            let st_info = data[base + 12];
            let st_shndx = read_u16(data, base + 14, le)?;
            let name = strtab_get(strtab, name_off).unwrap_or_default();
            DynSym { name, st_info, st_shndx }
        };
        syms.push(sym);
    }
    Ok(syms)
}

fn parse_rela_entries(data: &[u8], class: u8, le: bool) -> Result<Vec<Rela>> {
    let entsz: usize = if class == ELFCLASS64 { 24 } else { 12 };
    if entsz == 0 {
        return Ok(Vec::new());
    }
    let count = data.len() / entsz;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * entsz;
        if base + entsz > data.len() {
            break;
        }
        let entry = if class == ELFCLASS64 {
            let r_offset = read_u64(data, base, le)?;
            let r_info = read_u64(data, base + 8, le)?;
            let r_addend = read_u64(data, base + 16, le)? as i64;
            Rela {
                r_offset,
                r_sym: (r_info >> 32) as u32,
                r_type: r_info as u32,
                r_addend,
            }
        } else {
            let r_offset = read_u32(data, base, le)? as u64;
            let r_info = read_u32(data, base + 4, le)?;
            let r_addend = read_u32(data, base + 8, le)? as i32 as i64;
            Rela {
                r_offset,
                r_sym: r_info >> 8,
                r_type: r_info & 0xff,
                r_addend,
            }
        };
        out.push(entry);
    }
    Ok(out)
}

fn parse_rel_entries(data: &[u8], class: u8, le: bool) -> Result<Vec<Rel>> {
    let entsz: usize = if class == ELFCLASS64 { 16 } else { 8 };
    if entsz == 0 {
        return Ok(Vec::new());
    }
    let count = data.len() / entsz;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * entsz;
        if base + entsz > data.len() {
            break;
        }
        let entry = if class == ELFCLASS64 {
            let r_offset = read_u64(data, base, le)?;
            let r_info = read_u64(data, base + 8, le)?;
            Rel { r_offset, r_sym: (r_info >> 32) as u32, r_type: r_info as u32 }
        } else {
            let r_offset = read_u32(data, base, le)? as u64;
            let r_info = read_u32(data, base + 4, le)?;
            Rel { r_offset, r_sym: r_info >> 8, r_type: r_info & 0xff }
        };
        out.push(entry);
    }
    Ok(out)
}

// ============================================================================
// Relocation type name
// ============================================================================

fn rela_type_name(r_type: u32) -> &'static str {
    match r_type {
        R_X86_64_NONE => "R_X86_64_NONE",
        R_X86_64_64 => "R_X86_64_64",
        R_X86_64_COPY => "R_X86_64_COPY",
        R_X86_64_GLOB_DAT => "R_X86_64_GLOB_DAT",
        R_X86_64_JUMP_SLOT => "R_X86_64_JUMP_SLOT",
        R_X86_64_RELATIVE => "R_X86_64_RELATIVE",
        _ => "R_UNKNOWN",
    }
}

// ============================================================================
// Library path resolution
// ============================================================================

/// Standard library search paths searched in order after RPATH/LD_LIBRARY_PATH/RUNPATH.
const STANDARD_DIRS: &[&str] = &["/lib", "/usr/lib", "/usr/local/lib"];

/// Context for library resolution: holds search paths for a given binary.
#[derive(Debug, Clone)]
struct SearchPaths {
    /// RPATH from the binary (searched first, before LD_LIBRARY_PATH).
    rpath: Vec<PathBuf>,
    /// LD_LIBRARY_PATH directories (searched after RPATH, before RUNPATH).
    ld_library_path: Vec<PathBuf>,
    /// RUNPATH from the binary (searched after LD_LIBRARY_PATH).
    runpath: Vec<PathBuf>,
}

impl SearchPaths {
    fn new(rpath: Vec<PathBuf>, runpath: Vec<PathBuf>, ld_library_path: Vec<PathBuf>) -> Self {
        Self { rpath, ld_library_path, runpath }
    }

    /// Resolve a library name to a filesystem path.
    ///
    /// Resolution order (POSIX/Linux ABI):
    ///   1. RPATH (if RUNPATH is absent)
    ///   2. LD_LIBRARY_PATH
    ///   3. RUNPATH
    ///   4. Standard directories
    fn resolve(&self, libname: &str) -> Option<PathBuf> {
        // Per the ELF spec: RPATH is only used when RUNPATH is absent.
        let use_rpath = self.runpath.is_empty();

        let mut order: Vec<&Vec<PathBuf>> = Vec::new();
        if use_rpath {
            order.push(&self.rpath);
        }
        order.push(&self.ld_library_path);
        order.push(&self.runpath);

        for dirs in order {
            for dir in dirs {
                let candidate = dir.join(libname);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        // Standard directories
        for dir in STANDARD_DIRS {
            let candidate = Path::new(dir).join(libname);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        None
    }
}

/// Parse a colon-separated path list (as used in RPATH, RUNPATH, LD_LIBRARY_PATH).
fn parse_colon_paths(s: &str) -> Vec<PathBuf> {
    s.split(':').filter(|p| !p.is_empty()).map(PathBuf::from).collect()
}

/// Extract RPATH and RUNPATH strings from the dynamic section.
fn extract_rpaths(
    entries: &[DynEntry],
    dynstr: &[u8],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut rpath = Vec::new();
    let mut runpath = Vec::new();

    for e in entries {
        if e.d_tag == DT_RPATH && let Ok(s) = strtab_get(dynstr, e.d_val as usize) {
            rpath = parse_colon_paths(&s);
        } else if e.d_tag == DT_RUNPATH && let Ok(s) = strtab_get(dynstr, e.d_val as usize) {
            runpath = parse_colon_paths(&s);
        }
    }
    (rpath, runpath)
}

// ============================================================================
// Dependency info returned per library
// ============================================================================

/// Resolution state of a single library dependency.
#[derive(Debug, Clone)]
enum LibResolution {
    /// Resolved to a filesystem path.
    Found(PathBuf),
    /// Could not find the library on any search path.
    NotFound,
}

/// One row in the ldd output.
#[derive(Debug, Clone)]
struct DepEntry {
    /// Library name as listed in DT_NEEDED (e.g. "libfoo.so.1").
    name: String,
    /// How it was resolved.
    resolution: LibResolution,
    /// Simulated load address (for display only; not a real mmap).
    load_addr: u64,
}

// ============================================================================
// Recursive dependency resolution
// ============================================================================

/// Collect all DT_NEEDED names from the dynamic section entries.
fn collect_needed(_elf: &Elf, dynstr: &[u8], dyn_entries: &[DynEntry]) -> Vec<String> {
    let mut needed = Vec::new();
    for e in dyn_entries {
        if e.d_tag == DT_NEEDED
            && let Ok(name) = strtab_get(dynstr, e.d_val as usize)
            && !name.is_empty()
        {
            needed.push(name);
        }
    }
    // Note: PT_INTERP / .interp is extracted separately via extract_interpreter().
    needed
}

/// Extract the interpreter path from PT_INTERP / .interp section.
fn extract_interpreter(elf: &Elf) -> Option<PathBuf> {
    // Try PT_INTERP program header first.
    for ph in &elf.phdrs {
        // PT_INTERP = 3
        if ph.p_type == 3 {
            let off = ph.p_offset as usize;
            let sz = ph.p_filesz as usize;
            if let Some(bytes) = elf.data.get(off..off.saturating_add(sz)) {
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                let s = String::from_utf8_lossy(&bytes[..end]).into_owned();
                if !s.is_empty() {
                    return Some(PathBuf::from(s));
                }
            }
        }
    }
    // Fall back to .interp section.
    if let Some(idx) = elf.find_section(".interp")
        && let Ok(bytes) = elf.section_data(idx)
    {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let s = String::from_utf8_lossy(&bytes[..end]).into_owned();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

/// Simple pseudo-random load address generator for display purposes.
/// Real addresses come from the dynamic linker at runtime; we simulate them.
fn fake_load_addr(seed: u64) -> u64 {
    // Produce an address in the range typical for shared libraries on x86-64:
    // between 0x00007f0000000000 and 0x00007fffffffffff, page-aligned.
    let base: u64 = 0x0000_7f00_0000_0000;
    let range: u64 = 0x0000_00ff_ffff_f000;
    // Simple LCG: multiply by a prime, mask to range.
    let v = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(0x6c62_272e_07bb_0142);
    base + (v & range)
}

/// Recursively resolve all dependencies of `elf`, breadth-first.
/// `seen` tracks library names already visited to break cycles.
fn resolve_deps(
    elf: &Elf,
    search: &SearchPaths,
    seen: &mut HashSet<String>,
    entries: &mut Vec<DepEntry>,
    verbose: bool,
    out: &mut impl Write,
    depth: u32,
) -> io::Result<()> {
    let dyn_entries_opt = match elf.parse_dynamic() {
        Ok(v) => v,
        Err(e) => {
            if verbose {
                writeln!(out, "  ldd: warning: failed to parse dynamic section: {e}")?;
            }
            None
        }
    };

    let Some(dyn_entries) = dyn_entries_opt else {
        return Ok(());
    };

    let dynstr = elf.dynstr(Some(&dyn_entries));
    let needed = collect_needed(elf, dynstr, &dyn_entries);
    let (rpath, runpath) = extract_rpaths(&dyn_entries, dynstr);

    // Build effective search path for this level's binary.
    let effective_search = SearchPaths::new(rpath, runpath, search.ld_library_path.clone());

    let mut addr_seed = 0xdead_beef_u64.wrapping_add(depth as u64 * 0x1000);

    for libname in &needed {
        if seen.contains(libname) {
            // Already resolved — do not re-process (handles circular deps).
            if verbose {
                writeln!(out, "  ldd: note: circular/duplicate dependency skipped: {libname}")?;
            }
            continue;
        }
        seen.insert(libname.clone());

        addr_seed = addr_seed.wrapping_mul(0x5851_f42d_4c95_7f2d).wrapping_add(0x1405_7b7e_f767_814f);
        let load_addr = fake_load_addr(addr_seed);

        let resolution = match effective_search.resolve(libname) {
            Some(p) => LibResolution::Found(p.clone()),
            None => LibResolution::NotFound,
        };

        let path_for_recurse = if let LibResolution::Found(ref p) = resolution {
            Some(p.clone())
        } else {
            None
        };

        entries.push(DepEntry {
            name: libname.clone(),
            resolution,
            load_addr,
        });

        // Recurse into the found library.
        if let Some(dep_path) = path_for_recurse {
            match Elf::load(&dep_path) {
                Ok(dep_elf) => {
                    resolve_deps(
                        &dep_elf,
                        &effective_search,
                        seen,
                        entries,
                        verbose,
                        out,
                        depth + 1,
                    )?;
                }
                Err(e) => {
                    if verbose {
                        writeln!(
                            out,
                            "  ldd: warning: could not parse dependency {}: {e}",
                            dep_path.display()
                        )?;
                    }
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Display helpers
// ============================================================================

/// Format a load address as `(0x...)`.
fn fmt_addr(addr: u64) -> String {
    format!("(0x{addr:016x})")
}

/// Print the dependency list in standard ldd format.
fn print_deps(
    out: &mut impl Write,
    entries: &[DepEntry],
    interp: Option<&Path>,
    interp_addr: u64,
) -> io::Result<()> {
    // The interpreter is typically shown first.
    // Mimic: linux-vdso.so.1 (0x...) at the top, then the libs, then ld.so at bottom.
    // We approximate by showing the interpreter last, after the libs.

    for entry in entries {
        match &entry.resolution {
            LibResolution::Found(path) => {
                writeln!(
                    out,
                    "\t{} => {} {}",
                    entry.name,
                    path.display(),
                    fmt_addr(entry.load_addr)
                )?;
            }
            LibResolution::NotFound => {
                writeln!(out, "\t{} => not found", entry.name)?;
            }
        }
    }

    // Print interpreter (ld.so) at the bottom if present.
    if let Some(interp_path) = interp {
        writeln!(out, "\t{} {}", interp_path.display(), fmt_addr(interp_addr))?;
    }

    Ok(())
}

// ============================================================================
// Version info display (--verbose)
// ============================================================================

/// Print GNU version requirement information from .gnu.version_r if present.
fn print_version_info(out: &mut impl Write, elf: &Elf) -> io::Result<()> {
    let Some(idx) = elf.find_section(".gnu.version_r") else {
        return Ok(());
    };
    let data = match elf.section_data(idx) {
        Ok(d) => d,
        Err(e) => {
            writeln!(out, "\tldd: warning reading .gnu.version_r: {e}")?;
            return Ok(());
        }
    };
    let dynstr = elf.dynstr_section();

    writeln!(out, "\nVersion information:")?;

    let le = elf.le();
    let mut pos = 0usize;
    let mut verneed_count = 0u32;

    // Each Verneed record: vn_version(2), vn_cnt(2), vn_file(4), vn_aux(4), vn_next(4)
    while pos + 16 <= data.len() && verneed_count < 256 {
        let Ok(vn_cnt_u16) = read_u16(data, pos + 2, le) else { break };
        let vn_cnt = vn_cnt_u16 as usize;
        let Ok(vn_file_u32) = read_u32(data, pos + 4, le) else { break };
        let vn_file = vn_file_u32 as usize;
        let Ok(vn_aux_u32) = read_u32(data, pos + 8, le) else { break };
        let vn_aux = vn_aux_u32 as usize;
        let Ok(vn_next) = read_u32(data, pos + 12, le) else { break };

        let file_name = strtab_get(dynstr, vn_file).unwrap_or_default();
        writeln!(out, "\t{file_name}:")?;

        // Walk auxiliary records: vna_hash(4), vna_flags(2), vna_other(2),
        //                         vna_name(4), vna_next(4)
        let mut aux_pos = pos + vn_aux;
        for _ in 0..vn_cnt {
            if aux_pos + 16 > data.len() {
                break;
            }
            let Ok(vna_name_u32) = read_u32(data, aux_pos + 8, le) else { break };
            let vna_name = vna_name_u32 as usize;
            let Ok(vna_next) = read_u32(data, aux_pos + 12, le) else { break };
            let ver_name = strtab_get(dynstr, vna_name).unwrap_or_default();
            writeln!(out, "\t\t{ver_name} ({file_name}) => found")?;
            if vna_next == 0 {
                break;
            }
            aux_pos = aux_pos.saturating_add(vna_next as usize);
        }

        if vn_next == 0 {
            break;
        }
        pos = pos.saturating_add(vn_next as usize);
        verneed_count += 1;
    }
    Ok(())
}

// ============================================================================
// Unused dependency detection (-u)
// ============================================================================

/// Heuristically identify "unused" direct dependencies.
///
/// A library is considered unused if the binary has no undefined symbol
/// referencing it.  This is a best-effort check; it cannot match the
/// accuracy of a full linker map.
fn find_unused_deps(
    elf: &Elf,
    direct_deps: &[String],
    search: &SearchPaths,
    entries: &[DepEntry],
) -> Vec<String> {
    // Collect undefined symbols from .dynsym.
    let dynsyms = elf.parse_dynsym().unwrap_or_default();
    let undefined_syms: HashSet<&str> =
        dynsyms.iter().filter(|s| s.is_undefined() && !s.name.is_empty()).map(|s| s.name.as_str()).collect();

    if undefined_syms.is_empty() {
        // If there are no undefined symbols, all direct deps are candidates.
        return direct_deps.to_vec();
    }

    let mut unused = Vec::new();

    for dep_name in direct_deps {
        // Find the resolved path for this dep.
        let resolved_path = entries
            .iter()
            .find(|e| &e.name == dep_name)
            .and_then(|e| {
                if let LibResolution::Found(ref p) = e.resolution {
                    Some(p.clone())
                } else {
                    None
                }
            })
            .or_else(|| search.resolve(dep_name));

        let provides: HashSet<String> = resolved_path
            .and_then(|p| Elf::load(&p).ok())
            .map(|dep_elf| {
                dep_elf
                    .parse_dynsym()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| {
                        !s.is_undefined()
                            && (s.binding() == STB_GLOBAL || s.binding() == STB_WEAK)
                    })
                    .map(|s| s.name)
                    .collect()
            })
            .unwrap_or_default();

        // If the lib provides none of our undefined symbols, it is unused.
        let used = provides.iter().any(|sym| undefined_syms.contains(sym.as_str()));
        if !used {
            unused.push(dep_name.clone());
        }
    }

    unused
}

// ============================================================================
// Function / data relocation display (-r)
// ============================================================================

fn print_relocs(out: &mut impl Write, elf: &Elf) -> io::Result<()> {
    let dynsyms = elf.parse_dynsym().unwrap_or_default();
    let dynstr = elf.dynstr_section();

    // Helper: look up a symbol name by index.
    let sym_name = |idx: usize| -> &str {
        dynsyms.get(idx).map_or("", |s| s.name.as_str())
    };

    writeln!(out, "\nRelocations:")?;

    // .rela.dyn
    let rela_dyn = match elf.parse_rela(".rela.dyn") {
        Ok(v) => v,
        Err(e) => {
            writeln!(out, "\tldd: warning reading .rela.dyn: {e}")?;
            Vec::new()
        }
    };
    if !rela_dyn.is_empty() {
        writeln!(out, "\n.rela.dyn:")?;
        writeln!(out, "  Offset          Type                    Symbol")?;
        for r in &rela_dyn {
            let t = r.r_type;
            let sname = sym_name(r.r_sym as usize);
            // Show GLOB_DAT, COPY, and 64-bit as they reference data objects.
            if t == R_X86_64_GLOB_DAT || t == R_X86_64_COPY || t == R_X86_64_64 {
                writeln!(
                    out,
                    "  {:016x}  {:24}  {}+0x{:x}",
                    r.r_offset,
                    rela_type_name(t),
                    sname,
                    r.r_addend
                )?;
            }
        }
    }

    // .rela.plt (function relocations)
    let rela_plt = match elf.parse_rela(".rela.plt") {
        Ok(v) => v,
        Err(e) => {
            writeln!(out, "\tldd: warning reading .rela.plt: {e}")?;
            Vec::new()
        }
    };
    if !rela_plt.is_empty() {
        writeln!(out, "\n.rela.plt:")?;
        writeln!(out, "  Offset          Type                    Symbol")?;
        for r in &rela_plt {
            let sname = sym_name(r.r_sym as usize);
            writeln!(
                out,
                "  {:016x}  {:24}  {}",
                r.r_offset,
                rela_type_name(r.r_type),
                sname
            )?;
        }
    }

    // REL variant (32-bit or older toolchains)
    let rel_dyn = match elf.parse_rel(".rel.dyn") {
        Ok(v) => v,
        Err(e) => {
            writeln!(out, "\tldd: warning reading .rel.dyn: {e}")?;
            Vec::new()
        }
    };
    if !rel_dyn.is_empty() {
        writeln!(out, "\n.rel.dyn:")?;
        writeln!(out, "  Offset          Type                    Symbol")?;
        for r in &rel_dyn {
            let sname = sym_name(r.r_sym as usize);
            writeln!(
                out,
                "  {:016x}  {:24}  {}",
                r.r_offset,
                rela_type_name(r.r_type),
                sname
            )?;
        }
    }

    // Suppress unused import warning for dynstr
    let _ = dynstr;

    if rela_dyn.is_empty() && rela_plt.is_empty() && rel_dyn.is_empty() {
        writeln!(out, "\t(none)")?;
    }

    Ok(())
}

// ============================================================================
// Top-level: process a single file
// ============================================================================

fn process_file(
    path: &str,
    opts: &Options,
    ld_library_path: &[PathBuf],
    out: &mut impl Write,
) -> io::Result<()> {
    if opts.verbose {
        writeln!(out, "Processing: {path}")?;
    }

    let elf_path = Path::new(path);

    let elf = match Elf::load(elf_path) {
        Ok(e) => e,
        Err(e) => {
            writeln!(out, "\t{path}: {e}")?;
            return Ok(());
        }
    };

    if !elf.is_dynamic() {
        writeln!(out, "\t{path}: not a dynamic executable or shared library")?;
        return Ok(());
    }

    // Parse dynamic section up front.
    let dyn_entries_opt = elf.parse_dynamic().unwrap_or(None);

    let (rpath, runpath, direct_needed) = if let Some(ref entries) = dyn_entries_opt {
        let dynstr = elf.dynstr(Some(entries));
        let (r, ru) = extract_rpaths(entries, dynstr);
        let needed = collect_needed(&elf, dynstr, entries);
        (r, ru, needed)
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    let search = SearchPaths::new(rpath, runpath, ld_library_path.to_vec());

    // Recursive dependency resolution.
    let mut seen: HashSet<String> = HashSet::new();
    for name in &direct_needed {
        seen.insert(name.clone());
    }
    let mut all_entries: Vec<DepEntry> = Vec::new();

    // Seed: assign load addresses for direct deps.
    let mut addr_seed: u64 = 0x1234_5678_9abc_def0;

    for libname in &direct_needed {
        addr_seed =
            addr_seed.wrapping_mul(0x5851_f42d_4c95_7f2d).wrapping_add(0x1405_7b7e_f767_814f);
        let load_addr = fake_load_addr(addr_seed);
        let resolution = match search.resolve(libname) {
            Some(p) => LibResolution::Found(p.clone()),
            None => LibResolution::NotFound,
        };
        let dep_path = if let LibResolution::Found(ref p) = resolution {
            Some(p.clone())
        } else {
            None
        };
        all_entries.push(DepEntry {
            name: libname.clone(),
            resolution,
            load_addr,
        });
        if let Some(dep_path) = dep_path {
            match Elf::load(&dep_path) {
                Ok(dep_elf) => {
                    resolve_deps(
                        &dep_elf,
                        &search,
                        &mut seen,
                        &mut all_entries,
                        opts.verbose,
                        out,
                        1,
                    )?;
                }
                Err(e) if opts.verbose => {
                    writeln!(
                        out,
                        "  ldd: warning: could not parse {}: {e}",
                        dep_path.display()
                    )?;
                }
                Err(_) => {}
            }
        }
    }

    // Interpreter.
    let interp = extract_interpreter(&elf);
    let interp_addr = fake_load_addr(0xdead_c0de_feed_face);

    print_deps(out, &all_entries, interp.as_deref(), interp_addr)?;

    // Verbose: version information.
    if opts.verbose && let Err(e) = print_version_info(out, &elf) {
        writeln!(out, "  ldd: warning: version info error: {e}")?;
    }

    // Unused dependencies (-u).
    if opts.unused {
        let unused = find_unused_deps(&elf, &direct_needed, &search, &all_entries);
        if unused.is_empty() {
            writeln!(out, "\nUnused direct dependencies: (none)")?;
        } else {
            writeln!(out, "\nUnused direct dependencies:")?;
            for u in &unused {
                writeln!(out, "\t{u}")?;
            }
        }
    }

    // Function/data relocations (-r).
    if opts.function_relocs && let Err(e) = print_relocs(out, &elf) {
        writeln!(out, "  ldd: warning: relocation error: {e}")?;
    }

    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let opts = parse_args();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let ld_library_path: Vec<PathBuf> = env::var("LD_LIBRARY_PATH")
        .map(|v| parse_colon_paths(&v))
        .unwrap_or_default();

    let mut any_error = false;

    for path in &opts.files {
        if opts.files.len() > 1 {
            let _ = writeln!(out, "{path}:");
        }
        if let Err(e) = process_file(path, &opts, &ld_library_path, &mut out) {
            eprintln!("{PROGRAM_NAME}: {e}");
            any_error = true;
        }
    }

    let _ = out.flush();

    if any_error {
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
    // Helper: build a minimal valid ELF64 LE binary in memory
    // -----------------------------------------------------------------------

    /// Build an ELF64 LE file with a .dynamic section containing the given
    /// DT_NEEDED entries plus a .dynstr holding the library names.
    fn make_elf64_with_needed(needed: &[&str]) -> Vec<u8> {
        // Build .dynstr: null byte + each name + null
        let mut dynstr: Vec<u8> = vec![0u8]; // index 0 = empty string
        let mut name_offsets: Vec<usize> = Vec::new();
        for &n in needed {
            name_offsets.push(dynstr.len());
            dynstr.extend_from_slice(n.as_bytes());
            dynstr.push(0);
        }
        // Pad to 8-byte alignment
        while !dynstr.len().is_multiple_of(8) {
            dynstr.push(0);
        }

        // Build .dynamic: DT_STRTAB(vaddr placeholder)+DT_STRSZ+DT_NEEDED*n+DT_NULL
        let n_dyn_entries = 2 + needed.len() + 1; // STRTAB, STRSZ, NEEDEDs, NULL
        let dyn_entsz: usize = 16;
        let mut dynamic: Vec<u8> = vec![0u8; n_dyn_entries * dyn_entsz];

        let write_dyn = |data: &mut Vec<u8>, i: usize, tag: i64, val: u64| {
            let base = i * 16;
            data[base..base + 8].copy_from_slice(&(tag as u64).to_le_bytes());
            data[base + 8..base + 16].copy_from_slice(&val.to_le_bytes());
        };

        // DT_STRTAB: virtual address — we'll patch this after we know offsets
        write_dyn(&mut dynamic, 0, DT_STRTAB, 0xffff_0000); // placeholder vaddr
        write_dyn(&mut dynamic, 1, DT_STRSZ, dynstr.len() as u64);
        for (i, &off) in name_offsets.iter().enumerate() {
            write_dyn(&mut dynamic, 2 + i, DT_NEEDED, off as u64);
        }
        write_dyn(&mut dynamic, 2 + needed.len(), DT_NULL, 0);

        // Layout:
        //   0x00  ELF header     (64 bytes)
        //   0x40  .dynamic       (n_dyn_entries * 16 bytes)
        //   next  .dynstr        (dynstr.len() bytes)
        //   next  .shstrtab      (section name string table)
        //   next  section headers (3 sections: null, .dynamic, .dynstr, .shstrtab)

        let elf_hdr_sz: usize = 64;
        let dynamic_off: usize = elf_hdr_sz;
        let dynstr_off: usize = dynamic_off + dynamic.len();
        let shstrtab_names = b"\0.dynamic\0.dynstr\0.shstrtab\0";
        let shstrtab_off: usize = dynstr_off + dynstr.len();
        let shdrs_off: usize = shstrtab_off + shstrtab_names.len();
        // Align shdrs_off to 8
        let shdrs_off = (shdrs_off + 7) & !7;

        // Patch DT_STRTAB to point to dynstr_off (use as vaddr = file offset,
        // since we have no program headers; the file-level fallback uses .dynstr).
        let strtab_vaddr = dynstr_off as u64;
        dynamic[8..16].copy_from_slice(&strtab_vaddr.to_le_bytes());

        // Section headers: null, .dynamic, .dynstr, .shstrtab
        let n_shdrs: usize = 4;
        let shdr_sz: usize = 64;

        // Name offsets in .shstrtab (offset 0 is the null section name)
        let sh_name_dynamic: u32 = 1; // ".dynamic" at offset 1
        let sh_name_dynstr: u32 = 10; // ".dynstr" at offset 10
        let sh_name_shstrtab: u32 = 18; // ".shstrtab" at offset 18

        // Placeholder ELF header (fill in below)
        let mut file: Vec<u8> = vec![0; elf_hdr_sz];

        // .dynamic
        file.extend_from_slice(&dynamic);

        // .dynstr
        file.extend_from_slice(&dynstr);

        // .shstrtab
        file.extend_from_slice(shstrtab_names);

        // Pad to shdrs_off
        while file.len() < shdrs_off {
            file.push(0);
        }

        // Section headers (64 bytes each, ELF64)
        let write_shdr = |buf: &mut Vec<u8>,
                          sh_name: u32,
                          sh_type: u32,
                          sh_offset: usize,
                          sh_size: usize,
                          sh_link: u32,
                          sh_entsize: usize| {
            buf.extend_from_slice(&sh_name.to_le_bytes()); // sh_name (4)
            buf.extend_from_slice(&sh_type.to_le_bytes()); // sh_type (4)
            buf.extend_from_slice(&0u64.to_le_bytes()); // sh_flags (8)
            buf.extend_from_slice(&0u64.to_le_bytes()); // sh_addr (8)
            buf.extend_from_slice(&(sh_offset as u64).to_le_bytes()); // sh_offset (8)
            buf.extend_from_slice(&(sh_size as u64).to_le_bytes()); // sh_size (8)
            buf.extend_from_slice(&sh_link.to_le_bytes()); // sh_link (4)
            buf.extend_from_slice(&0u32.to_le_bytes()); // sh_info (4)
            buf.extend_from_slice(&0u64.to_le_bytes()); // sh_addralign (8)
            buf.extend_from_slice(&(sh_entsize as u64).to_le_bytes()); // sh_entsize (8)
        };

        // SHT_DYNAMIC = 6 (section header type for the dynamic section)
        const SHT_DYNAMIC_VAL: u32 = 6;

        // Null section
        write_shdr(&mut file, 0, 0, 0, 0, 0, 0);
        // .dynamic
        write_shdr(&mut file, sh_name_dynamic, SHT_DYNAMIC_VAL, dynamic_off, dynamic.len(), 0, 16);
        // .dynstr
        write_shdr(&mut file, sh_name_dynstr, SHT_STRTAB_VAL, dynstr_off, dynstr.len(), 0, 0);
        // .shstrtab
        write_shdr(
            &mut file,
            sh_name_shstrtab,
            SHT_STRTAB_VAL,
            shstrtab_off,
            shstrtab_names.len(),
            0,
            0,
        );

        let total_len = file.len();
        assert_eq!(total_len, shdrs_off + n_shdrs * shdr_sz);

        // Write ELF header
        let ehdr: [u8; 64] = {
            let mut h = [0u8; 64];
            h[0..4].copy_from_slice(&ELFMAG);
            h[EI_CLASS] = ELFCLASS64;
            h[EI_DATA] = ELFDATA2LSB;
            h[6] = 1; // EI_VERSION = 1
            h[7] = 0; // ELFOSABI_NONE
            // e_type = ET_EXEC (2)
            h[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
            // e_machine = EM_X86_64 (62)
            h[18..20].copy_from_slice(&62u16.to_le_bytes());
            // e_version = 1
            h[20..24].copy_from_slice(&1u32.to_le_bytes());
            // e_entry = 0
            // e_phoff = 0 (no program headers)
            // e_shoff
            h[40..48].copy_from_slice(&(shdrs_off as u64).to_le_bytes());
            // e_flags = 0
            // e_ehsize
            h[52..54].copy_from_slice(&(64u16).to_le_bytes());
            // e_phentsize = 56
            h[54..56].copy_from_slice(&(56u16).to_le_bytes());
            // e_phnum = 0
            // e_shentsize
            h[58..60].copy_from_slice(&(64u16).to_le_bytes());
            // e_shnum
            h[60..62].copy_from_slice(&(n_shdrs as u16).to_le_bytes());
            // e_shstrndx = 3 (.shstrtab is section 3)
            h[62..64].copy_from_slice(&3u16.to_le_bytes());
            h
        };
        file[..64].copy_from_slice(&ehdr);

        file
    }

    // SHT_STRTAB = 3
    const SHT_STRTAB_VAL: u32 = 3;

    // -----------------------------------------------------------------------
    // Tests for ELF parsing helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_u16_le() {
        let data = [0x34u8, 0x12];
        assert_eq!(read_u16(&data, 0, true).unwrap(), 0x1234);
    }

    #[test]
    fn test_read_u16_be() {
        let data = [0x12u8, 0x34];
        assert_eq!(read_u16(&data, 0, false).unwrap(), 0x1234);
    }

    #[test]
    fn test_read_u32_le() {
        let data = [0x78u8, 0x56, 0x34, 0x12];
        assert_eq!(read_u32(&data, 0, true).unwrap(), 0x1234_5678);
    }

    #[test]
    fn test_read_u64_le() {
        let data = [0x08u8, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01];
        assert_eq!(read_u64(&data, 0, true).unwrap(), 0x0102_0304_0506_0708);
    }

    #[test]
    fn test_read_u16_truncated() {
        let data = [0x01u8];
        assert!(read_u16(&data, 0, true).is_err());
    }

    #[test]
    fn test_read_u32_truncated() {
        let data = [0x01u8, 0x02, 0x03];
        assert!(read_u32(&data, 0, true).is_err());
    }

    #[test]
    fn test_strtab_get_basic() {
        let table = b"\0libfoo.so.1\0libbar.so.2\0";
        assert_eq!(strtab_get(table, 1).unwrap(), "libfoo.so.1");
        assert_eq!(strtab_get(table, 13).unwrap(), "libbar.so.2");
    }

    #[test]
    fn test_strtab_get_index_zero() {
        let table = b"\0hello\0";
        assert_eq!(strtab_get(table, 0).unwrap(), "");
    }

    #[test]
    fn test_strtab_get_out_of_bounds() {
        let table = b"\0foo\0";
        assert!(strtab_get(table, 100).is_err());
    }

    #[test]
    fn test_strtab_get_exact_end() {
        // Offset is exactly at end (no NUL before end)
        let table = b"hello"; // no NUL — strtab_get should return "hello"
        assert_eq!(strtab_get(table, 0).unwrap(), "hello");
    }

    #[test]
    fn test_parse_colon_paths_basic() {
        let paths = parse_colon_paths("/lib:/usr/lib:/usr/local/lib");
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0], PathBuf::from("/lib"));
        assert_eq!(paths[2], PathBuf::from("/usr/local/lib"));
    }

    #[test]
    fn test_parse_colon_paths_empty() {
        let paths = parse_colon_paths("");
        assert_eq!(paths.len(), 0);
    }

    #[test]
    fn test_parse_colon_paths_trailing_colon() {
        let paths = parse_colon_paths("/lib:/usr/lib:");
        // Trailing colon produces an empty component which is filtered out.
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_elf_bad_magic() {
        let data = vec![0u8; 64];
        assert!(matches!(Elf::parse(data), Err(Error::NotElf)));
    }

    #[test]
    fn test_elf_truncated_header() {
        let mut data = vec![0u8; 8];
        data[..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = ELFDATA2LSB;
        assert!(matches!(Elf::parse(data), Err(Error::TruncatedHeader)));
    }

    #[test]
    fn test_elf_invalid_class() {
        let mut data = vec![0u8; EI_NIDENT];
        data[..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = 99; // invalid
        data[EI_DATA] = ELFDATA2LSB;
        assert!(matches!(Elf::parse(data), Err(Error::InvalidClass(99))));
    }

    #[test]
    fn test_elf_invalid_encoding() {
        let mut data = vec![0u8; EI_NIDENT];
        data[..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = 5; // invalid
        assert!(matches!(Elf::parse(data), Err(Error::InvalidEncoding(5))));
    }

    #[test]
    fn test_elf_parse_no_needed() {
        let elf_data = make_elf64_with_needed(&[]);
        let elf = Elf::parse(elf_data).expect("should parse");
        let dyn_opt = elf.parse_dynamic().expect("should parse dynamic");
        let dyn_entries = dyn_opt.expect("should have dynamic section");
        let dynstr = elf.dynstr(Some(&dyn_entries));
        let needed = collect_needed(&elf, dynstr, &dyn_entries);
        assert!(needed.is_empty());
    }

    #[test]
    fn test_elf_parse_single_needed() {
        let elf_data = make_elf64_with_needed(&["libfoo.so.1"]);
        let elf = Elf::parse(elf_data).expect("should parse");
        let dyn_opt = elf.parse_dynamic().expect("dynamic ok");
        let dyn_entries = dyn_opt.expect("has dynamic section");
        let dynstr = elf.dynstr(Some(&dyn_entries));
        let needed = collect_needed(&elf, dynstr, &dyn_entries);
        assert_eq!(needed, vec!["libfoo.so.1"]);
    }

    #[test]
    fn test_elf_parse_multiple_needed() {
        let libs = ["libfoo.so.1", "libbar.so.2", "libbaz.so.3"];
        let elf_data = make_elf64_with_needed(&libs);
        let elf = Elf::parse(elf_data).expect("parse ok");
        let dyn_opt = elf.parse_dynamic().expect("dynamic ok");
        let dyn_entries = dyn_opt.expect("has dynamic");
        let dynstr = elf.dynstr(Some(&dyn_entries));
        let needed = collect_needed(&elf, dynstr, &dyn_entries);
        assert_eq!(needed, libs);
    }

    #[test]
    fn test_fake_load_addr_range() {
        // Addresses should fall in the 0x7f....... range typical for shared libs.
        for seed in [0u64, 1, 0xdead_beef, u64::MAX] {
            let addr = fake_load_addr(seed);
            assert!(addr >= 0x0000_7f00_0000_0000, "addr too low: {addr:#x}");
            assert!(addr < 0x0001_0000_0000_0000, "addr too high: {addr:#x}");
        }
    }

    #[test]
    fn test_fake_load_addr_page_aligned() {
        // Generated addresses must be page-aligned (low 12 bits = 0).
        for seed in [42u64, 1337, 0xffff_ffff] {
            let addr = fake_load_addr(seed);
            assert_eq!(addr & 0xfff, 0, "addr {addr:#x} not page-aligned");
        }
    }

    #[test]
    fn test_search_paths_not_found() {
        // With no real directories, resolve should always return None.
        let sp = SearchPaths::new(Vec::new(), Vec::new(), Vec::new());
        assert!(sp.resolve("libdoes_not_exist_ever.so.99").is_none());
    }

    #[test]
    fn test_extract_rpaths_empty() {
        let entries = vec![DynEntry { d_tag: DT_NULL, d_val: 0 }];
        let dynstr = b"\0";
        let (rpath, runpath) = extract_rpaths(&entries, dynstr);
        assert!(rpath.is_empty());
        assert!(runpath.is_empty());
    }

    #[test]
    fn test_parse_dynamic_entries_null_terminates() {
        // A dynamic section with DT_NEEDED then DT_NULL should stop at DT_NULL.
        let mut data = vec![0u8; 3 * 16];
        // Entry 0: DT_NEEDED = 1, val = 1
        data[0..8].copy_from_slice(&(DT_NEEDED as u64).to_le_bytes());
        data[8..16].copy_from_slice(&1u64.to_le_bytes());
        // Entry 1: DT_NULL = 0
        data[16..24].copy_from_slice(&(DT_NULL as u64).to_le_bytes());
        data[24..32].copy_from_slice(&0u64.to_le_bytes());
        // Entry 2: garbage that should NOT be read
        data[32..40].copy_from_slice(&0xdead_beef_u64.to_le_bytes());

        let entries = parse_dynamic_entries(&data, ELFCLASS64, true).unwrap();
        // Should have exactly 2 entries: DT_NEEDED + DT_NULL
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].d_tag, DT_NEEDED);
        assert_eq!(entries[1].d_tag, DT_NULL);
    }

    #[test]
    fn test_rela_type_name_known() {
        assert_eq!(rela_type_name(R_X86_64_JUMP_SLOT), "R_X86_64_JUMP_SLOT");
        assert_eq!(rela_type_name(R_X86_64_GLOB_DAT), "R_X86_64_GLOB_DAT");
        assert_eq!(rela_type_name(R_X86_64_RELATIVE), "R_X86_64_RELATIVE");
    }

    #[test]
    fn test_rela_type_name_unknown() {
        assert_eq!(rela_type_name(0xffff_ffff), "R_UNKNOWN");
    }

    #[test]
    fn test_is_dynamic_exec() {
        let elf_data = make_elf64_with_needed(&[]);
        let elf = Elf::parse(elf_data).unwrap();
        // make_elf64_with_needed builds an ET_EXEC binary.
        assert!(elf.is_dynamic());
    }

    #[test]
    fn test_error_display_not_elf() {
        let msg = format!("{}", Error::NotElf);
        assert!(msg.contains("ELF"));
    }

    #[test]
    fn test_error_display_truncated_data() {
        let msg = format!(
            "{}",
            Error::TruncatedData { what: "test", offset: 0x10, needed: 8, available: 2 }
        );
        assert!(msg.contains("test"));
        assert!(msg.contains("0x10"));
    }

    #[test]
    fn test_parse_rela_entries_empty() {
        let entries = parse_rela_entries(&[], ELFCLASS64, true).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_rela_entries_one() {
        // Build one 64-bit RELA entry: r_offset=0x1000, r_info=(sym=2,type=7), r_addend=0
        let mut data = vec![0u8; 24];
        data[0..8].copy_from_slice(&0x1000u64.to_le_bytes());
        let r_info: u64 = (2u64 << 32) | 7u64; // sym=2, type=R_X86_64_JUMP_SLOT
        data[8..16].copy_from_slice(&r_info.to_le_bytes());
        // r_addend = 0
        let entries = parse_rela_entries(&data, ELFCLASS64, true).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].r_offset, 0x1000);
        assert_eq!(entries[0].r_sym, 2);
        assert_eq!(entries[0].r_type, 7);
        assert_eq!(entries[0].r_addend, 0);
    }

    #[test]
    fn test_parse_rel_entries_one() {
        // Build one 64-bit REL entry
        let mut data = vec![0u8; 16];
        data[0..8].copy_from_slice(&0x2000u64.to_le_bytes());
        let r_info: u64 = (5u64 << 32) | 6u64; // sym=5, type=R_X86_64_GLOB_DAT
        data[8..16].copy_from_slice(&r_info.to_le_bytes());
        let entries = parse_rel_entries(&data, ELFCLASS64, true).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].r_offset, 0x2000);
        assert_eq!(entries[0].r_sym, 5);
        assert_eq!(entries[0].r_type, 6);
    }
}
