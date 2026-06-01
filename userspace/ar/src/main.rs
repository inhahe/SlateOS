//! OurOS Archive and Object File Tools
//!
//! Multi-personality binary that acts as `ar`, `ranlib`, or `strip` depending
//! on the name used to invoke it (detected via `argv[0]`).
//!
//! # Personalities
//!
//! - **ar**: create, modify, and extract from static library archives
//! - **ranlib**: regenerate the symbol table index of an archive
//! - **strip**: remove symbol tables and debug sections from ELF files
//!
//! # Usage
//!
//! ```text
//! ar r libfoo.a foo.o bar.o     # insert/replace members
//! ar t libfoo.a                 # list members
//! ar x libfoo.a                 # extract all members
//! ar d libfoo.a bar.o           # delete member
//! ar q libfoo.a baz.o           # quick append
//! ar p libfoo.a foo.o           # print member to stdout
//! ranlib libfoo.a               # regenerate symbol table
//! strip binary                  # strip all symbols
//! strip -g binary               # strip debug only
//! ```

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(dead_code)]

use std::env;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::process;

// ============================================================================
// Archive constants
// ============================================================================

/// Archive file magic: `!<arch>\n`
const AR_MAGIC: &[u8; 8] = b"!<arch>\n";

/// Archive member header terminator
const AR_FMAG: &[u8; 2] = b"`\n";

/// Archive header size in bytes
const AR_HDR_SIZE: usize = 60;

/// BSD extended name prefix
const AR_BSD_NAME_PREFIX: &str = "#1/";

/// GNU/SysV symbol table name
const AR_SYMTAB_NAME: &str = "/";

/// GNU/SysV string table name
const AR_STRTAB_NAME: &str = "//";

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

// Section header types
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;

// Section header flags
const SHF_ALLOC: u64 = 0x2;

// Symbol binding (upper 4 bits of st_info)
const STB_LOCAL: u8 = 0;

// Symbol type (lower 4 bits of st_info)
const STT_SECTION: u8 = 3;
const STT_FILE: u8 = 4;

// Special section indices
const SHN_UNDEF: u16 = 0;

// ============================================================================
// Archive header
// ============================================================================

/// Parsed archive member header.
#[derive(Debug, Clone)]
struct ArHeader {
    /// Member name (decoded from header or extended name table).
    name: String,
    /// Modification timestamp (seconds since epoch).
    mtime: u64,
    /// Owner UID.
    uid: u32,
    /// Owner GID.
    gid: u32,
    /// File mode (octal).
    mode: u32,
    /// Size of member data in bytes.
    size: u64,
}

impl ArHeader {
    /// Format as a 60-byte archive header line.
    /// If the name is too long for the 16-byte field, returns `None` (caller
    /// must use extended name encoding).
    fn to_bytes_short(&self, name_override: Option<&str>) -> Option<Vec<u8>> {
        let name = name_override.unwrap_or(&self.name);
        if name.len() > 15 {
            return None;
        }
        let mut hdr = Vec::with_capacity(AR_HDR_SIZE);
        // Name field: name + "/" padded to 16 bytes
        let name_field = format!("{name}/");
        write!(hdr, "{:<16}", name_field).ok()?;
        write!(hdr, "{:<12}", self.mtime).ok()?;
        write!(hdr, "{:<6}", self.uid).ok()?;
        write!(hdr, "{:<6}", self.gid).ok()?;
        write!(hdr, "{:<8o}", self.mode).ok()?;
        write!(hdr, "{:<10}", self.size).ok()?;
        hdr.extend_from_slice(AR_FMAG);
        debug_assert_eq!(hdr.len(), AR_HDR_SIZE);
        Some(hdr)
    }

    /// Format using BSD extended name encoding (`#1/N` prefix, name prepended
    /// to data).
    fn to_bytes_bsd(&self, extra_data_size: u64) -> Vec<u8> {
        let name_len = self.name.len();
        let padded_name_len = (name_len + 3) & !3; // pad to 4-byte boundary
        let total_size = extra_data_size + padded_name_len as u64;
        let mut hdr = Vec::with_capacity(AR_HDR_SIZE);
        let name_field = format!("#1/{padded_name_len}");
        // These writes to a Vec cannot fail, but we use write! for formatting
        let _ = write!(hdr, "{:<16}", name_field);
        let _ = write!(hdr, "{:<12}", self.mtime);
        let _ = write!(hdr, "{:<6}", self.uid);
        let _ = write!(hdr, "{:<6}", self.gid);
        let _ = write!(hdr, "{:<8o}", self.mode);
        let _ = write!(hdr, "{:<10}", total_size);
        hdr.extend_from_slice(AR_FMAG);
        hdr
    }

    /// Format using GNU/SysV extended name encoding (`/N` referencing string
    /// table offset).
    fn to_bytes_gnu(&self, strtab_offset: usize) -> Vec<u8> {
        let mut hdr = Vec::with_capacity(AR_HDR_SIZE);
        let name_field = format!("/{strtab_offset}");
        let _ = write!(hdr, "{:<16}", name_field);
        let _ = write!(hdr, "{:<12}", self.mtime);
        let _ = write!(hdr, "{:<6}", self.uid);
        let _ = write!(hdr, "{:<6}", self.gid);
        let _ = write!(hdr, "{:<8o}", self.mode);
        let _ = write!(hdr, "{:<10}", self.size);
        hdr.extend_from_slice(AR_FMAG);
        hdr
    }
}

/// An archive member: header + raw data.
#[derive(Debug, Clone)]
struct ArMember {
    header: ArHeader,
    data: Vec<u8>,
}

/// A complete archive.
#[derive(Debug, Clone)]
struct Archive {
    members: Vec<ArMember>,
}

impl Archive {
    fn new() -> Self {
        Self {
            members: Vec::new(),
        }
    }

    /// Parse an archive from raw bytes.
    fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < AR_MAGIC.len() {
            return Err("file too small to be an archive".into());
        }
        if &data[..8] != AR_MAGIC.as_slice() {
            return Err("not an archive (bad magic)".into());
        }

        let mut offset = 8;
        let mut members = Vec::new();
        let mut gnu_strtab: Option<Vec<u8>> = None;

        while offset < data.len() {
            if offset + AR_HDR_SIZE > data.len() {
                break;
            }
            let hdr_bytes = &data[offset..offset + AR_HDR_SIZE];

            // Verify fmag
            if &hdr_bytes[58..60] != AR_FMAG {
                return Err(format!("bad header magic at offset {offset}"));
            }

            let raw_name = std::str::from_utf8(&hdr_bytes[0..16])
                .map_err(|e| format!("invalid name field: {e}"))?
                .trim_end();
            let mtime = parse_header_field(&hdr_bytes[16..28])?;
            let uid = parse_header_field(&hdr_bytes[28..34])? as u32;
            let gid = parse_header_field(&hdr_bytes[34..40])? as u32;
            let mode = parse_header_octal(&hdr_bytes[40..48])?;
            let size = parse_header_field(&hdr_bytes[48..58])?;

            offset += AR_HDR_SIZE;
            let data_start = offset;
            let data_end = data_start + size as usize;
            if data_end > data.len() {
                return Err(format!(
                    "member data extends past end of archive at offset {data_start}"
                ));
            }
            let member_data = &data[data_start..data_end];

            // Decode name
            let (name, actual_data) = if raw_name == "//" {
                // GNU/SysV string table — store it, skip as member
                gnu_strtab = Some(member_data.to_vec());
                offset = align2(data_end);
                continue;
            } else if raw_name == "/" {
                // Symbol table — skip as member
                offset = align2(data_end);
                continue;
            } else if raw_name.starts_with('#') && raw_name.contains('/') {
                // BSD extended name: #1/N
                decode_bsd_name(raw_name, member_data)?
            } else if raw_name.starts_with('/') && raw_name.len() > 1 {
                // GNU/SysV extended name: /N
                let idx_str = &raw_name[1..];
                let idx: usize = idx_str
                    .parse()
                    .map_err(|_| format!("bad GNU name index: {idx_str}"))?;
                let strtab = gnu_strtab
                    .as_ref()
                    .ok_or("GNU extended name before string table")?;
                let gnu_name = read_gnu_strtab_entry(strtab, idx)?;
                (gnu_name, member_data)
            } else {
                // Short name — strip trailing '/'
                let name = raw_name.trim_end_matches('/').to_string();
                (name, member_data)
            };

            members.push(ArMember {
                header: ArHeader {
                    name,
                    mtime,
                    uid,
                    gid,
                    mode,
                    size: actual_data.len() as u64,
                },
                data: actual_data.to_vec(),
            });

            offset = align2(data_end);
        }

        Ok(Archive { members })
    }

    /// Serialize the archive to bytes, optionally including a symbol table.
    fn serialize(&self, write_symtab: bool) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(AR_MAGIC);

        // Build GNU string table for long names
        let mut gnu_strtab = Vec::new();
        let mut name_offsets: Vec<Option<usize>> = Vec::new();

        for member in &self.members {
            if member.header.name.len() > 15 {
                let off = gnu_strtab.len();
                name_offsets.push(Some(off));
                gnu_strtab.extend_from_slice(member.header.name.as_bytes());
                gnu_strtab.extend_from_slice(b"/\n");
            } else {
                name_offsets.push(None);
            }
        }

        // Write symbol table if requested
        if write_symtab {
            let symtab_data = self.build_symbol_table();
            if !symtab_data.is_empty() {
                let mut sym_hdr = Vec::with_capacity(AR_HDR_SIZE);
                let _ = write!(sym_hdr, "{:<16}", "/");
                let _ = write!(sym_hdr, "{:<12}", 0);
                let _ = write!(sym_hdr, "{:<6}", 0);
                let _ = write!(sym_hdr, "{:<6}", 0);
                let _ = write!(sym_hdr, "{:<8}", 0);
                let _ = write!(sym_hdr, "{:<10}", symtab_data.len());
                sym_hdr.extend_from_slice(AR_FMAG);
                out.extend_from_slice(&sym_hdr);
                out.extend_from_slice(&symtab_data);
                if !symtab_data.len().is_multiple_of(2) {
                    out.push(b'\n');
                }
            }
        }

        // Write GNU string table if needed
        if !gnu_strtab.is_empty() {
            let mut strtab_hdr = Vec::with_capacity(AR_HDR_SIZE);
            let _ = write!(strtab_hdr, "{:<16}", "//");
            let _ = write!(strtab_hdr, "{:<12}", 0);
            let _ = write!(strtab_hdr, "{:<6}", 0);
            let _ = write!(strtab_hdr, "{:<6}", 0);
            let _ = write!(strtab_hdr, "{:<8}", 0);
            let _ = write!(strtab_hdr, "{:<10}", gnu_strtab.len());
            strtab_hdr.extend_from_slice(AR_FMAG);
            out.extend_from_slice(&strtab_hdr);
            out.extend_from_slice(&gnu_strtab);
            if gnu_strtab.len() % 2 != 0 {
                out.push(b'\n');
            }
        }

        // Write members
        for (i, member) in self.members.iter().enumerate() {
            let hdr = if let Some(strtab_off) = name_offsets[i] {
                member.header.to_bytes_gnu(strtab_off)
            } else {
                // Short name path — always succeeds for names <= 15 chars
                member
                    .header
                    .to_bytes_short(None)
                    .expect("name fits in short field")
            };
            out.extend_from_slice(&hdr);
            out.extend_from_slice(&member.data);
            if member.data.len() % 2 != 0 {
                out.push(b'\n');
            }
        }

        out
    }

    /// Build a symbol table (archive index) by scanning ELF `.symtab` sections.
    /// Returns the raw bytes of the symbol table entry for the archive, or
    /// empty if no symbols found.
    fn build_symbol_table(&self) -> Vec<u8> {
        let mut symbols: Vec<(String, u32)> = Vec::new();

        // We need to know the offset of each member in the final archive.
        // This is a chicken-and-egg problem since the symtab size affects
        // offsets. We do two passes: first collect symbols, then compute
        // offsets after we know the symtab size.

        // First pass: collect symbol names per member index
        let mut member_symbols: Vec<Vec<String>> = Vec::new();
        for member in &self.members {
            let syms = extract_elf_symbols(&member.data);
            member_symbols.push(syms);
        }

        let total_symbols: usize = member_symbols.iter().map(Vec::len).sum();
        if total_symbols == 0 {
            return Vec::new();
        }

        // Compute symbol name bytes needed
        let name_bytes: usize = member_symbols
            .iter()
            .flat_map(|v| v.iter())
            .map(|s| s.len() + 1) // null terminated
            .sum();

        // Symbol table format (big-endian):
        //   u32: number of symbols
        //   u32[n]: offsets to member headers
        //   char[]: null-terminated symbol names
        let symtab_data_size = 4 + total_symbols * 4 + name_bytes;

        // Now compute member offsets accounting for: magic(8) + symtab_hdr(60)
        // + symtab_data + padding + strtab + members
        let symtab_padded = symtab_data_size + (symtab_data_size % 2);
        let mut strtab_size = 0usize;
        for member in &self.members {
            if member.header.name.len() > 15 {
                strtab_size += member.header.name.len() + 2; // name + "/\n"
            }
        }
        let strtab_padded = if strtab_size > 0 {
            AR_HDR_SIZE + strtab_size + (strtab_size % 2)
        } else {
            0
        };

        let first_member_offset = 8 + AR_HDR_SIZE + symtab_padded + strtab_padded;
        let mut member_offsets = Vec::with_capacity(self.members.len());
        let mut cur_offset = first_member_offset;
        for member in &self.members {
            member_offsets.push(cur_offset as u32);
            let data_len = member.data.len();
            cur_offset += AR_HDR_SIZE + data_len + (data_len % 2);
        }

        // Build the final table
        for (i, syms) in member_symbols.iter().enumerate() {
            for sym_name in syms {
                symbols.push((sym_name.clone(), member_offsets[i]));
            }
        }

        let mut result = Vec::with_capacity(symtab_data_size);
        // Number of symbols (big-endian)
        result.extend_from_slice(&(symbols.len() as u32).to_be_bytes());
        // Offsets (big-endian)
        for (_, offset) in &symbols {
            result.extend_from_slice(&offset.to_be_bytes());
        }
        // Names (null-terminated)
        for (name, _) in &symbols {
            result.extend_from_slice(name.as_bytes());
            result.push(0);
        }

        result
    }

    /// Find the index of a member by name.
    fn find_member(&self, name: &str) -> Option<usize> {
        self.members.iter().position(|m| m.header.name == name)
    }
}

// ============================================================================
// Archive helper functions
// ============================================================================

/// Parse a numeric field from an archive header (decimal, possibly blank).
fn parse_header_field(field: &[u8]) -> Result<u64, String> {
    let s = std::str::from_utf8(field)
        .map_err(|e| format!("invalid header field: {e}"))?
        .trim();
    if s.is_empty() {
        return Ok(0);
    }
    s.parse::<u64>()
        .map_err(|e| format!("bad numeric field '{s}': {e}"))
}

/// Parse an octal field from an archive header.
fn parse_header_octal(field: &[u8]) -> Result<u32, String> {
    let s = std::str::from_utf8(field)
        .map_err(|e| format!("invalid header field: {e}"))?
        .trim();
    if s.is_empty() {
        return Ok(0);
    }
    u32::from_str_radix(s, 8).map_err(|e| format!("bad octal field '{s}': {e}"))
}

/// Decode a BSD extended name (`#1/N` format).
fn decode_bsd_name<'a>(raw_name: &str, member_data: &'a [u8]) -> Result<(String, &'a [u8]), String> {
    let prefix = AR_BSD_NAME_PREFIX;
    if !raw_name.starts_with(prefix) {
        return Err(format!("expected BSD name prefix, got: {raw_name}"));
    }
    let len_str = &raw_name[prefix.len()..];
    let name_len: usize = len_str
        .parse()
        .map_err(|_| format!("bad BSD name length: {len_str}"))?;
    if name_len > member_data.len() {
        return Err("BSD name length exceeds member data".into());
    }
    let name_bytes = &member_data[..name_len];
    // Strip trailing NUL padding
    let name = std::str::from_utf8(name_bytes)
        .map_err(|e| format!("invalid BSD name: {e}"))?
        .trim_end_matches('\0')
        .to_string();
    let actual_data = &member_data[name_len..];
    Ok((name, actual_data))
}

/// Read a name from the GNU/SysV string table at the given offset.
fn read_gnu_strtab_entry(strtab: &[u8], offset: usize) -> Result<String, String> {
    if offset >= strtab.len() {
        return Err(format!(
            "GNU strtab offset {offset} out of range (len {})",
            strtab.len()
        ));
    }
    // Names end with "/\n" or just "\n"
    let mut end = offset;
    while end < strtab.len() && strtab[end] != b'\n' {
        end += 1;
    }
    let name_bytes = &strtab[offset..end];
    let name = std::str::from_utf8(name_bytes)
        .map_err(|e| format!("invalid GNU strtab entry: {e}"))?
        .trim_end_matches('/');
    Ok(name.to_string())
}

/// Align to 2-byte boundary.
fn align2(offset: usize) -> usize {
    (offset + 1) & !1
}

// ============================================================================
// ELF parsing (for symbol extraction and strip)
// ============================================================================

/// Minimal ELF header information.
#[derive(Debug, Clone)]
struct ElfInfo {
    class: u8,         // ELFCLASS32 or ELFCLASS64
    little_endian: bool,
    shoff: u64,        // section header table offset
    shentsize: u16,    // section header entry size
    shnum: u16,        // number of section headers
    shstrndx: u16,     // section name string table index
}

/// Parsed ELF section header.
#[derive(Debug, Clone)]
struct ElfSection {
    name_offset: u32,
    sh_type: u32,
    flags: u64,
    offset: u64,
    size: u64,
    link: u32,
    entsize: u64,
    name: String,      // resolved name
}

/// Parsed ELF symbol.
#[derive(Debug, Clone)]
struct ElfSymbol {
    name_offset: u32,
    info: u8,
    shndx: u16,
    name: String,
}

/// Read a u16 from a byte slice with given endianness.
fn read_u16(data: &[u8], offset: usize, little_endian: bool) -> u16 {
    let bytes = [
        *data.get(offset).unwrap_or(&0),
        *data.get(offset + 1).unwrap_or(&0),
    ];
    if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    }
}

/// Read a u32 from a byte slice with given endianness.
fn read_u32(data: &[u8], offset: usize, little_endian: bool) -> u32 {
    let mut bytes = [0u8; 4];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = *data.get(offset + i).unwrap_or(&0);
    }
    if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    }
}

/// Read a u64 from a byte slice with given endianness.
fn read_u64(data: &[u8], offset: usize, little_endian: bool) -> u64 {
    let mut bytes = [0u8; 8];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = *data.get(offset + i).unwrap_or(&0);
    }
    if little_endian {
        u64::from_le_bytes(bytes)
    } else {
        u64::from_be_bytes(bytes)
    }
}

/// Write a u16 into a byte slice.
fn write_u16(data: &mut [u8], offset: usize, value: u16, little_endian: bool) {
    let bytes = if little_endian {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    if offset + 2 <= data.len() {
        data[offset..offset + 2].copy_from_slice(&bytes);
    }
}

/// Write a u32 into a byte slice.
fn write_u32(data: &mut [u8], offset: usize, value: u32, little_endian: bool) {
    let bytes = if little_endian {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    if offset + 4 <= data.len() {
        data[offset..offset + 4].copy_from_slice(&bytes);
    }
}

/// Write a u64 into a byte slice.
fn write_u64(data: &mut [u8], offset: usize, value: u64, little_endian: bool) {
    let bytes = if little_endian {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    if offset + 8 <= data.len() {
        data[offset..offset + 8].copy_from_slice(&bytes);
    }
}

/// Parse the ELF header from raw data.
fn parse_elf_header(data: &[u8]) -> Option<ElfInfo> {
    if data.len() < EI_NIDENT {
        return None;
    }
    if data[0..4] != ELFMAG {
        return None;
    }
    let class = data[EI_CLASS];
    let little_endian = data[EI_DATA] == ELFDATA2LSB;

    match class {
        ELFCLASS32 => {
            if data.len() < 52 {
                return None;
            }
            let shoff = read_u32(data, 32, little_endian) as u64;
            let shentsize = read_u16(data, 46, little_endian);
            let shnum = read_u16(data, 48, little_endian);
            let shstrndx = read_u16(data, 50, little_endian);
            Some(ElfInfo {
                class,
                little_endian,
                shoff,
                shentsize,
                shnum,
                shstrndx,
            })
        }
        ELFCLASS64 => {
            if data.len() < 64 {
                return None;
            }
            let shoff = read_u64(data, 40, little_endian);
            let shentsize = read_u16(data, 58, little_endian);
            let shnum = read_u16(data, 60, little_endian);
            let shstrndx = read_u16(data, 62, little_endian);
            Some(ElfInfo {
                class,
                little_endian,
                shoff,
                shentsize,
                shnum,
                shstrndx,
            })
        }
        _ => None,
    }
}

/// Parse all section headers from ELF data.
fn parse_elf_sections(data: &[u8], info: &ElfInfo) -> Vec<ElfSection> {
    let mut sections = Vec::new();
    let shoff = info.shoff as usize;

    for i in 0..info.shnum as usize {
        let offset = shoff + i * info.shentsize as usize;
        if offset + info.shentsize as usize > data.len() {
            break;
        }
        let sec = if info.class == ELFCLASS64 {
            ElfSection {
                name_offset: read_u32(data, offset, info.little_endian),
                sh_type: read_u32(data, offset + 4, info.little_endian),
                flags: read_u64(data, offset + 8, info.little_endian),
                offset: read_u64(data, offset + 24, info.little_endian),
                size: read_u64(data, offset + 32, info.little_endian),
                link: read_u32(data, offset + 40, info.little_endian),
                entsize: read_u64(data, offset + 56, info.little_endian),
                name: String::new(),
            }
        } else {
            ElfSection {
                name_offset: read_u32(data, offset, info.little_endian),
                sh_type: read_u32(data, offset + 4, info.little_endian),
                flags: read_u32(data, offset + 8, info.little_endian) as u64,
                offset: read_u32(data, offset + 16, info.little_endian) as u64,
                size: read_u32(data, offset + 20, info.little_endian) as u64,
                link: read_u32(data, offset + 24, info.little_endian),
                entsize: read_u32(data, offset + 36, info.little_endian) as u64,
                name: String::new(),
            }
        };
        sections.push(sec);
    }

    // Resolve section names from the section header string table
    if (info.shstrndx as usize) < sections.len() {
        let shstrtab_offset = sections[info.shstrndx as usize].offset as usize;
        let shstrtab_size = sections[info.shstrndx as usize].size as usize;
        if shstrtab_offset + shstrtab_size <= data.len() {
            let strtab = &data[shstrtab_offset..shstrtab_offset + shstrtab_size];
            for sec in &mut sections {
                sec.name = read_cstring(strtab, sec.name_offset as usize);
            }
        }
    }

    sections
}

/// Read a null-terminated C string from a byte slice.
fn read_cstring(data: &[u8], offset: usize) -> String {
    if offset >= data.len() {
        return String::new();
    }
    let mut end = offset;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }
    String::from_utf8_lossy(&data[offset..end]).into_owned()
}

/// Extract globally-visible symbol names from an ELF object file.
/// Used for building the archive symbol table (archive index).
fn extract_elf_symbols(data: &[u8]) -> Vec<String> {
    let info = match parse_elf_header(data) {
        Some(i) => i,
        None => return Vec::new(),
    };

    let sections = parse_elf_sections(data, &info);
    let mut symbols = Vec::new();

    for sec in &sections {
        if sec.sh_type != SHT_SYMTAB {
            continue;
        }
        let sym_offset = sec.offset as usize;
        let sym_size = sec.size as usize;
        let entsize = if sec.entsize > 0 {
            sec.entsize as usize
        } else if info.class == ELFCLASS64 {
            24
        } else {
            16
        };

        // Get the associated string table
        let strtab_idx = sec.link as usize;
        if strtab_idx >= sections.len() {
            continue;
        }
        let strtab_offset = sections[strtab_idx].offset as usize;
        let strtab_size = sections[strtab_idx].size as usize;
        if strtab_offset + strtab_size > data.len() {
            continue;
        }
        let strtab = &data[strtab_offset..strtab_offset + strtab_size];

        // Iterate symbols
        let mut off = sym_offset;
        while off + entsize <= sym_offset + sym_size && off + entsize <= data.len() {
            let sym = parse_elf_symbol(data, off, &info);
            off += entsize;

            // Skip undefined, local, section, and file symbols
            let binding = sym.info >> 4;
            let sym_type = sym.info & 0xf;
            if sym.shndx == SHN_UNDEF {
                continue;
            }
            if binding == STB_LOCAL {
                continue;
            }
            if sym_type == STT_SECTION || sym_type == STT_FILE {
                continue;
            }
            let name = read_cstring(strtab, sym.name_offset as usize);
            if !name.is_empty() {
                symbols.push(name);
            }
        }
    }

    symbols
}

/// Parse a single ELF symbol table entry.
fn parse_elf_symbol(data: &[u8], offset: usize, info: &ElfInfo) -> ElfSymbol {
    if info.class == ELFCLASS64 {
        ElfSymbol {
            name_offset: read_u32(data, offset, info.little_endian),
            info: *data.get(offset + 4).unwrap_or(&0),
            shndx: read_u16(data, offset + 6, info.little_endian),
            name: String::new(),
        }
    } else {
        ElfSymbol {
            name_offset: read_u32(data, offset, info.little_endian),
            info: *data.get(offset + 12).unwrap_or(&0),
            shndx: read_u16(data, offset + 14, info.little_endian),
            name: String::new(),
        }
    }
}

// ============================================================================
// Section strippability
// ============================================================================

/// Returns true if a section should be removed by `strip -s` (strip-all).
fn is_strip_all_section(name: &str, sh_type: u32, flags: u64) -> bool {
    // Always preserve SHF_ALLOC sections
    if flags & SHF_ALLOC != 0 {
        return false;
    }
    // Remove symbol tables (but not .dynsym/.dynstr)
    if name == ".symtab" || name == ".strtab" {
        return true;
    }
    // Remove debug sections
    if name.starts_with(".debug_") || name.starts_with(".zdebug_") {
        return true;
    }
    // Remove other non-essential sections
    if name == ".comment" || name == ".note.GNU-stack" {
        return true;
    }
    // Remove by type
    if sh_type == SHT_SYMTAB {
        return true;
    }
    false
}

/// Returns true if a section should be removed by `strip -g` (strip-debug).
fn is_strip_debug_section(name: &str, _sh_type: u32, flags: u64) -> bool {
    if flags & SHF_ALLOC != 0 {
        return false;
    }
    name.starts_with(".debug_") || name.starts_with(".zdebug_")
}

/// Returns true if a section should be removed by `strip --strip-unneeded`.
/// Removes symbols not referenced by relocations (approximation: same as
/// strip-all for our purposes, since we don't track relocation references).
fn is_strip_unneeded_section(name: &str, sh_type: u32, flags: u64) -> bool {
    is_strip_all_section(name, sh_type, flags)
}

// ============================================================================
// Strip implementation
// ============================================================================

/// Options for the strip operation.
#[derive(Debug, Clone)]
struct StripOptions {
    strip_all: bool,
    strip_debug: bool,
    strip_unneeded: bool,
    keep_symbols: Vec<String>,
    output_file: Option<String>,
    preserve_dates: bool,
    verbose: bool,
}

impl StripOptions {
    fn new() -> Self {
        Self {
            strip_all: true, // default mode
            strip_debug: false,
            strip_unneeded: false,
            keep_symbols: Vec::new(),
            output_file: None,
            preserve_dates: false,
            verbose: false,
        }
    }
}

/// Strip an ELF file: remove sections based on the provided options.
/// Returns the modified ELF data.
fn strip_elf(data: &[u8], opts: &StripOptions) -> Result<Vec<u8>, String> {
    let info = parse_elf_header(data).ok_or("not a valid ELF file")?;
    let sections = parse_elf_sections(data, &info);

    if sections.is_empty() {
        return Ok(data.to_vec());
    }

    // Determine which sections to keep
    let mut keep = vec![true; sections.len()];
    // Always keep index 0 (SHT_NULL)
    for (i, sec) in sections.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let should_remove = if opts.strip_debug && !opts.strip_all && !opts.strip_unneeded {
            is_strip_debug_section(&sec.name, sec.sh_type, sec.flags)
        } else if opts.strip_unneeded {
            is_strip_unneeded_section(&sec.name, sec.sh_type, sec.flags)
        } else {
            is_strip_all_section(&sec.name, sec.sh_type, sec.flags)
        };

        if should_remove {
            // Check keep-symbol: if this is .symtab and we have keep-symbols,
            // we might still want to keep it. For simplicity, if any
            // keep-symbol is specified and this is .symtab, keep it.
            keep[i] = !opts.keep_symbols.is_empty() && (sec.name == ".symtab" || sec.name == ".strtab");
        }
    }

    // Also remove sections whose link target is removed (e.g., if .symtab is
    // removed, its associated .strtab that is only referenced by it can go too)
    // But be careful not to remove .shstrtab.
    let shstrtab_idx = info.shstrndx as usize;
    for i in 0..sections.len() {
        if !keep[i] {
            // Find sections that link to this one
            for j in 0..sections.len() {
                if sections[j].link as usize == i && j != shstrtab_idx {
                    // If the linking section is also being removed or is a
                    // strtab for a removed symtab, mark it too. But only if
                    // it isn't needed by a kept section.
                    let needed_by_kept = sections.iter().enumerate().any(|(k, s)| {
                        k != j && keep[k] && s.link as usize == j
                    });
                    if !needed_by_kept && sections[j].sh_type == SHT_STRTAB && j != shstrtab_idx {
                        keep[j] = false;
                    }
                }
            }
        }
    }

    // Always keep .shstrtab
    if shstrtab_idx < keep.len() {
        keep[shstrtab_idx] = true;
    }

    // Build the new ELF
    rebuild_elf(data, &info, &sections, &keep)
}

/// Rebuild an ELF file, including only the sections marked as kept.
fn rebuild_elf(
    data: &[u8],
    info: &ElfInfo,
    sections: &[ElfSection],
    keep: &[bool],
) -> Result<Vec<u8>, String> {
    // Map old section indices to new indices
    let mut new_index = vec![0u16; sections.len()];
    let mut new_count: u16 = 0;
    for (i, &kept) in keep.iter().enumerate() {
        if kept {
            new_index[i] = new_count;
            new_count += 1;
        }
    }

    let elf_header_size: usize = if info.class == ELFCLASS64 { 64 } else { 52 };
    let shent_size = info.shentsize as usize;

    // Copy the ELF header
    let mut output = data[..elf_header_size].to_vec();

    // Copy program headers if present
    let (phoff, phentsize, phnum) = if info.class == ELFCLASS64 {
        (
            read_u64(data, 32, info.little_endian),
            read_u16(data, 54, info.little_endian),
            read_u16(data, 56, info.little_endian),
        )
    } else {
        (
            read_u32(data, 28, info.little_endian) as u64,
            read_u16(data, 42, info.little_endian),
            read_u16(data, 44, info.little_endian),
        )
    };

    if phnum > 0 && phoff > 0 {
        let ph_start = phoff as usize;
        let ph_size = phnum as usize * phentsize as usize;
        // Align output
        while output.len() < ph_start {
            output.push(0);
        }
        if ph_start + ph_size <= data.len() {
            // If program headers come right after ELF header, they're already
            // in the right place
            if output.len() <= ph_start {
                output.resize(ph_start, 0);
                output.extend_from_slice(&data[ph_start..ph_start + ph_size]);
            }
        }
    }

    // Copy kept section data
    let mut section_new_offsets = vec![0u64; sections.len()];
    for (i, sec) in sections.iter().enumerate() {
        if !keep[i] || i == 0 {
            continue;
        }
        // Align to 8 bytes
        while !output.len().is_multiple_of(8) {
            output.push(0);
        }
        section_new_offsets[i] = output.len() as u64;
        let sec_start = sec.offset as usize;
        let sec_end = sec_start + sec.size as usize;
        if sec_end <= data.len() && sec.size > 0 {
            output.extend_from_slice(&data[sec_start..sec_end]);
        }
    }

    // Align before section headers
    while !output.len().is_multiple_of(8) {
        output.push(0);
    }
    let new_shoff = output.len() as u64;

    // Write new section headers
    for (i, sec) in sections.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        let old_sh_start = info.shoff as usize + i * shent_size;
        if old_sh_start + shent_size > data.len() {
            continue;
        }
        let mut sh_data = data[old_sh_start..old_sh_start + shent_size].to_vec();

        // Update offset
        if i != 0 {
            if info.class == ELFCLASS64 {
                write_u64(&mut sh_data, 24, section_new_offsets[i], info.little_endian);
            } else {
                write_u32(
                    &mut sh_data,
                    16,
                    section_new_offsets[i] as u32,
                    info.little_endian,
                );
            }
        }

        // Update link field to new index
        let old_link = sec.link as usize;
        if old_link < new_index.len() {
            let link_offset = if info.class == ELFCLASS64 { 40 } else { 24 };
            write_u32(&mut sh_data, link_offset, new_index[old_link] as u32, info.little_endian);
        }

        output.extend_from_slice(&sh_data);
    }

    // Update ELF header: e_shoff, e_shnum, e_shstrndx
    if info.class == ELFCLASS64 {
        write_u64(&mut output, 40, new_shoff, info.little_endian);
        write_u16(&mut output, 60, new_count, info.little_endian);
        write_u16(
            &mut output,
            62,
            new_index[info.shstrndx as usize],
            info.little_endian,
        );
    } else {
        write_u32(&mut output, 32, new_shoff as u32, info.little_endian);
        write_u16(&mut output, 48, new_count, info.little_endian);
        write_u16(
            &mut output,
            50,
            new_index[info.shstrndx as usize],
            info.little_endian,
        );
    }

    Ok(output)
}

// ============================================================================
// ar operations
// ============================================================================

/// Options/modifiers for ar operations.
#[derive(Debug, Clone)]
struct ArOptions {
    operation: char,         // r, d, t, x, q, p
    verbose: bool,           // v
    create_silently: bool,   // c
    write_symtab: bool,      // s
    update_only: bool,       // u
    deterministic: bool,     // D
    position_after: Option<String>,  // a <member>
    position_before: Option<String>, // b/i <member>
}

impl ArOptions {
    fn new() -> Self {
        Self {
            operation: '\0',
            verbose: false,
            create_silently: false,
            write_symtab: false,
            update_only: false,
            deterministic: false,
            position_after: None,
            position_before: None,
        }
    }
}

/// Parse ar command-line flags and return (options, archive_path, member_files).
fn parse_ar_args(args: &[String]) -> Result<(ArOptions, String, Vec<String>), String> {
    if args.is_empty() {
        return Err("no operation specified".into());
    }

    let mut opts = ArOptions::new();
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;

    // First arg is the flags string (like "rv" or "rcs")
    let flags = &args[0];
    let mut chars = flags.chars().peekable();

    for ch in chars {
        match ch {
            'r' | 'd' | 't' | 'x' | 'q' | 'p' => {
                if opts.operation != '\0' {
                    return Err(format!(
                        "multiple operations: '{}' and '{}'",
                        opts.operation, ch
                    ));
                }
                opts.operation = ch;
            }
            'v' => opts.verbose = true,
            'c' => opts.create_silently = true,
            's' => opts.write_symtab = true,
            'u' => opts.update_only = true,
            'D' => opts.deterministic = true,
            'a' => {
                // Next positional arg is the position member name
                i += 1;
                if i >= args.len() - 1 {
                    return Err("'a' requires a member name argument".into());
                }
                // The member name follows the flags block
                // We'll handle it after parsing flags
                break;
            }
            'b' | 'i' => {
                i += 1;
                if i >= args.len() - 1 {
                    return Err(format!("'{ch}' requires a member name argument"));
                }
                break;
            }
            '-' => {} // allow leading dash
            _ => {
                return Err(format!("unknown flag: '{ch}'"));
            }
        }
    }
    i += 1;

    // Handle position modifier arguments
    if opts.operation != '\0' {
        // Check if we broke out due to a/b/i
        let last_flag = flags.chars().last().unwrap_or('\0');
        if last_flag == 'a' || flags.contains('a') {
            if i < args.len() {
                opts.position_after = Some(args[i].clone());
                i += 1;
            }
        } else if last_flag == 'b' || last_flag == 'i' || flags.contains('b') || flags.contains('i')
        {
            if i < args.len() {
                opts.position_before = Some(args[i].clone());
                i += 1;
            }
        }
    }

    // Remaining args: archive path then member files
    for j in i..args.len() {
        positional.push(args[j].clone());
    }

    if opts.operation == '\0' && !opts.write_symtab {
        return Err("no operation specified".into());
    }

    // If only 's' was given with no operation, treat as "update symtab"
    if opts.operation == '\0' && opts.write_symtab {
        opts.operation = 's';
    }

    if positional.is_empty() {
        return Err("no archive specified".into());
    }

    let archive_path = positional.remove(0);
    Ok((opts, archive_path, positional))
}

/// Execute the ar operation.
fn run_ar(args: &[String]) -> Result<(), String> {
    let (opts, archive_path, member_files) = parse_ar_args(args)?;

    match opts.operation {
        'r' => ar_replace(&opts, &archive_path, &member_files),
        'd' => ar_delete(&opts, &archive_path, &member_files),
        't' => ar_list(&opts, &archive_path),
        'x' => ar_extract(&opts, &archive_path, &member_files),
        'q' => ar_quick_append(&opts, &archive_path, &member_files),
        'p' => ar_print(&opts, &archive_path, &member_files),
        's' => ar_update_symtab(&archive_path),
        _ => Err(format!("unknown operation: '{}'", opts.operation)),
    }
}

/// `ar r` — insert or replace members.
fn ar_replace(opts: &ArOptions, archive_path: &str, member_files: &[String]) -> Result<(), String> {
    let mut archive = load_or_create_archive(archive_path, opts.create_silently)?;

    for file_path in member_files {
        let member_name = member_basename(file_path);
        let file_data =
            fs::read(file_path).map_err(|e| format!("cannot read '{file_path}': {e}"))?;

        let file_mtime = if opts.deterministic {
            0
        } else {
            get_file_mtime(file_path).unwrap_or(0)
        };

        let new_member = ArMember {
            header: ArHeader {
                name: member_name.clone(),
                mtime: file_mtime,
                // OurOS `ar` always normalises ownership/mode: uid/gid are written
                // as 0 and mode as 0o100644 regardless of the `-U` (non-deterministic)
                // flag.  Preserving the file's real uid/gid/mode in non-deterministic
                // mode is not yet implemented (see todo.txt), so these are constants.
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: file_data.len() as u64,
            },
            data: file_data,
        };

        if let Some(existing_idx) = archive.find_member(&member_name) {
            if opts.update_only {
                // Only replace if new file is newer
                let old_mtime = archive.members[existing_idx].header.mtime;
                if new_member.header.mtime <= old_mtime {
                    continue;
                }
            }
            archive.members[existing_idx] = new_member;
            if opts.verbose {
                eprintln!("r - {member_name}");
            }
        } else {
            // Insert at position
            let insert_idx = find_insert_position(&archive, opts);
            archive.members.insert(insert_idx, new_member);
            if opts.verbose {
                eprintln!("a - {member_name}");
            }
        }
    }

    let serialized = archive.serialize(opts.write_symtab);
    fs::write(archive_path, serialized).map_err(|e| format!("cannot write '{archive_path}': {e}"))
}

/// `ar d` — delete members.
fn ar_delete(opts: &ArOptions, archive_path: &str, member_names: &[String]) -> Result<(), String> {
    let data = fs::read(archive_path).map_err(|e| format!("cannot read '{archive_path}': {e}"))?;
    let mut archive = Archive::parse(&data)?;

    for name in member_names {
        if let Some(idx) = archive.find_member(name) {
            archive.members.remove(idx);
            if opts.verbose {
                eprintln!("d - {name}");
            }
        } else {
            eprintln!("ar: '{name}': no such member");
        }
    }

    let serialized = archive.serialize(opts.write_symtab);
    fs::write(archive_path, serialized).map_err(|e| format!("cannot write '{archive_path}': {e}"))
}

/// `ar t` — list members.
fn ar_list(opts: &ArOptions, archive_path: &str) -> Result<(), String> {
    let data = fs::read(archive_path).map_err(|e| format!("cannot read '{archive_path}': {e}"))?;
    let archive = Archive::parse(&data)?;

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    for member in &archive.members {
        if opts.verbose {
            let _ = writeln!(
                out,
                "{:o} {}/{} {:>6} {} {}",
                member.header.mode,
                member.header.uid,
                member.header.gid,
                member.header.size,
                format_timestamp(member.header.mtime),
                member.header.name,
            );
        } else {
            let _ = writeln!(out, "{}", member.header.name);
        }
    }

    Ok(())
}

/// `ar x` — extract members.
fn ar_extract(opts: &ArOptions, archive_path: &str, member_names: &[String]) -> Result<(), String> {
    let data = fs::read(archive_path).map_err(|e| format!("cannot read '{archive_path}': {e}"))?;
    let archive = Archive::parse(&data)?;

    let extract_all = member_names.is_empty();

    for member in &archive.members {
        if !extract_all && !member_names.iter().any(|n| n == &member.header.name) {
            continue;
        }
        if opts.verbose {
            eprintln!("x - {}", member.header.name);
        }
        fs::write(&member.header.name, &member.data)
            .map_err(|e| format!("cannot write '{}': {e}", member.header.name))?;
    }

    Ok(())
}

/// `ar q` — quick append (no duplicate check).
fn ar_quick_append(
    opts: &ArOptions,
    archive_path: &str,
    member_files: &[String],
) -> Result<(), String> {
    let mut archive = load_or_create_archive(archive_path, true)?;

    for file_path in member_files {
        let member_name = member_basename(file_path);
        let file_data =
            fs::read(file_path).map_err(|e| format!("cannot read '{file_path}': {e}"))?;

        let file_mtime = if opts.deterministic {
            0
        } else {
            get_file_mtime(file_path).unwrap_or(0)
        };

        archive.members.push(ArMember {
            header: ArHeader {
                name: member_name.clone(),
                mtime: file_mtime,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: file_data.len() as u64,
            },
            data: file_data,
        });

        if opts.verbose {
            eprintln!("a - {member_name}");
        }
    }

    let serialized = archive.serialize(opts.write_symtab);
    fs::write(archive_path, serialized).map_err(|e| format!("cannot write '{archive_path}': {e}"))
}

/// `ar p` — print member contents to stdout.
fn ar_print(opts: &ArOptions, archive_path: &str, member_names: &[String]) -> Result<(), String> {
    let data = fs::read(archive_path).map_err(|e| format!("cannot read '{archive_path}': {e}"))?;
    let archive = Archive::parse(&data)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let print_all = member_names.is_empty();

    for member in &archive.members {
        if !print_all && !member_names.iter().any(|n| n == &member.header.name) {
            continue;
        }
        if opts.verbose {
            eprintln!("\n<{}>", member.header.name);
        }
        let _ = out.write_all(&member.data);
    }

    Ok(())
}

/// Update the symbol table of an existing archive (used by `ranlib` and `ar s`).
fn ar_update_symtab(archive_path: &str) -> Result<(), String> {
    let data = fs::read(archive_path).map_err(|e| format!("cannot read '{archive_path}': {e}"))?;
    let archive = Archive::parse(&data)?;
    let serialized = archive.serialize(true);
    fs::write(archive_path, serialized).map_err(|e| format!("cannot write '{archive_path}': {e}"))
}

// ============================================================================
// ar helper functions
// ============================================================================

/// Load an existing archive, or create a new empty one.
fn load_or_create_archive(path: &str, silent: bool) -> Result<Archive, String> {
    if Path::new(path).exists() {
        let data = fs::read(path).map_err(|e| format!("cannot read '{path}': {e}"))?;
        Archive::parse(&data)
    } else {
        if !silent {
            eprintln!("ar: creating {path}");
        }
        Ok(Archive::new())
    }
}

/// Extract the basename from a file path for use as the archive member name.
fn member_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Determine the insertion position based on position modifiers.
fn find_insert_position(archive: &Archive, opts: &ArOptions) -> usize {
    if let Some(ref after_name) = opts.position_after {
        if let Some(idx) = archive.find_member(after_name) {
            return idx + 1;
        }
    }
    if let Some(ref before_name) = opts.position_before {
        if let Some(idx) = archive.find_member(before_name) {
            return idx;
        }
    }
    archive.members.len()
}

/// Get file modification time as seconds since epoch.
fn get_file_mtime(path: &str) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Format a Unix timestamp for verbose listing.
fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "0                   ".to_string();
    }
    // Simple formatting: just show the raw timestamp
    // A full implementation would format as "Mon DD HH:MM YYYY"
    format!("{ts}")
}

// ============================================================================
// ranlib mode
// ============================================================================

/// Parse ranlib arguments and run.
fn run_ranlib(args: &[String]) -> Result<(), String> {
    let mut archive_path = None;
    let mut _deterministic = false;

    for arg in args {
        if arg == "-D" {
            _deterministic = true;
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            archive_path = Some(arg.clone());
        }
    }

    let path = archive_path.ok_or("no archive specified")?;
    ar_update_symtab(&path)
}

// ============================================================================
// strip mode
// ============================================================================

/// Parse strip command-line arguments.
fn parse_strip_args(args: &[String]) -> Result<(StripOptions, Vec<String>), String> {
    let mut opts = StripOptions::new();
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-s" | "--strip-all" => {
                opts.strip_all = true;
                opts.strip_debug = false;
                opts.strip_unneeded = false;
            }
            "-g" | "--strip-debug" | "-S" => {
                opts.strip_debug = true;
                opts.strip_all = false;
                opts.strip_unneeded = false;
            }
            "--strip-unneeded" => {
                opts.strip_unneeded = true;
                opts.strip_all = false;
                opts.strip_debug = false;
            }
            "-K" | "--keep-symbol" => {
                i += 1;
                if i >= args.len() {
                    return Err("-K requires a symbol name".into());
                }
                opts.keep_symbols.push(args[i].clone());
            }
            "-o" => {
                i += 1;
                if i >= args.len() {
                    return Err("-o requires an output file".into());
                }
                opts.output_file = Some(args[i].clone());
            }
            "-p" | "--preserve-dates" => {
                opts.preserve_dates = true;
            }
            "-v" | "--verbose" => {
                opts.verbose = true;
            }
            other => {
                if let Some(sym) = other.strip_prefix("--keep-symbol=") {
                    opts.keep_symbols.push(sym.to_string());
                } else if other.starts_with('-') {
                    return Err(format!("unknown option: {other}"));
                } else {
                    files.push(other.to_string());
                }
            }
        }
        i += 1;
    }

    if files.is_empty() {
        return Err("no input files".into());
    }

    Ok((opts, files))
}

/// Run strip on the given files.
fn run_strip(args: &[String]) -> Result<(), String> {
    let (opts, files) = parse_strip_args(args)?;

    for file_path in &files {
        let data =
            fs::read(file_path).map_err(|e| format!("cannot read '{file_path}': {e}"))?;

        let stripped = strip_elf(&data, &opts)?;

        let output_path = opts.output_file.as_deref().unwrap_or(file_path.as_str());

        if opts.verbose {
            eprintln!("strip: {file_path}");
        }

        // Read access/modification times before writing if preserve_dates
        #[cfg(not(test))]
        let _times = if opts.preserve_dates {
            // Store for later restoration
            let metadata = fs::metadata(file_path).ok();
            metadata.and_then(|m| {
                let accessed = m.accessed().ok()?;
                let modified = m.modified().ok()?;
                Some((accessed, modified))
            })
        } else {
            None
        };

        fs::write(output_path, &stripped)
            .map_err(|e| format!("cannot write '{output_path}': {e}"))?;

        // Restore timestamps if requested
        // Note: standard Rust doesn't provide set_file_times; on OurOS this
        // would use a platform-specific API. For now we accept this limitation.
    }

    Ok(())
}

// ============================================================================
// Personality detection and main entry point
// ============================================================================

/// Detect which personality to use based on argv[0].
fn detect_personality(argv0: &str) -> &'static str {
    let basename = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(argv0);

    if basename == "ranlib" {
        "ranlib"
    } else if basename == "strip" {
        "strip"
    } else {
        "ar"
    }
}

fn print_ar_usage() {
    eprintln!("Usage: ar [flags] archive [member...]");
    eprintln!("Operations: r(eplace) d(elete) t(oc) x(tract) q(uick append) p(rint)");
    eprintln!("Modifiers: v(erbose) c(reate) s(ymtab) u(pdate) D(eterministic)");
    eprintln!("           a <member> (after) b/i <member> (before)");
}

fn print_ranlib_usage() {
    eprintln!("Usage: ranlib [-D] archive");
}

fn print_strip_usage() {
    eprintln!("Usage: strip [options] file...");
    eprintln!("Options:");
    eprintln!("  -s, --strip-all       Remove all symbols (default)");
    eprintln!("  -g, --strip-debug     Remove debug sections only");
    eprintln!("  --strip-unneeded      Remove unneeded symbols");
    eprintln!("  -K, --keep-symbol=SYM Keep symbol SYM");
    eprintln!("  -o FILE               Write output to FILE");
    eprintln!("  -p, --preserve-dates  Preserve access/modification times");
    eprintln!("  -v, --verbose         Verbose output");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(String::as_str).unwrap_or("ar");
    let personality = detect_personality(argv0);

    let tool_args: Vec<String> = args[1..].to_vec();

    let result = match personality {
        "ranlib" => {
            if tool_args.is_empty() || tool_args.iter().any(|a| a == "--help" || a == "-h") {
                print_ranlib_usage();
                process::exit(if tool_args.is_empty() { 1 } else { 0 });
            }
            run_ranlib(&tool_args)
        }
        "strip" => {
            if tool_args.is_empty() || tool_args.iter().any(|a| a == "--help" || a == "-h") {
                print_strip_usage();
                process::exit(if tool_args.is_empty() { 1 } else { 0 });
            }
            run_strip(&tool_args)
        }
        _ => {
            // ar mode
            if tool_args.is_empty() || tool_args.iter().any(|a| a == "--help" || a == "-h") {
                print_ar_usage();
                process::exit(if tool_args.is_empty() { 1 } else { 0 });
            }
            run_ar(&tool_args)
        }
    };

    if let Err(e) = result {
        eprintln!("{personality}: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helper: build a minimal archive from members --

    fn make_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut ar = Archive::new();
        for (name, data) in members {
            ar.members.push(ArMember {
                header: ArHeader {
                    name: (*name).to_string(),
                    mtime: 1234567890,
                    uid: 1000,
                    gid: 1000,
                    mode: 0o100644,
                    size: data.len() as u64,
                },
                data: data.to_vec(),
            });
        }
        ar.serialize(false)
    }

    fn make_archive_with_symtab(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut ar = Archive::new();
        for (name, data) in members {
            ar.members.push(ArMember {
                header: ArHeader {
                    name: (*name).to_string(),
                    mtime: 0,
                    uid: 0,
                    gid: 0,
                    mode: 0o100644,
                    size: data.len() as u64,
                },
                data: data.to_vec(),
            });
        }
        ar.serialize(true)
    }

    // -- Helper: build a minimal 64-bit little-endian ELF with sections --

    fn make_elf64(sections: &[(&str, u32, u64, &[u8])]) -> Vec<u8> {
        // sections: (name, sh_type, flags, data)
        // We'll build: ELF header + section data + shstrtab + section headers

        let elf_hdr_size = 64usize;
        let shent_size = 64usize;

        // Build shstrtab content
        let mut shstrtab = vec![0u8]; // first byte is null
        let mut name_offsets: Vec<u32> = Vec::new();
        // index 0 = SHT_NULL
        name_offsets.push(0);
        for (name, _, _, _) in sections {
            name_offsets.push(shstrtab.len() as u32);
            shstrtab.extend_from_slice(name.as_bytes());
            shstrtab.push(0);
        }
        // Add .shstrtab itself
        let shstrtab_name_offset = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".shstrtab\0");

        // Layout: ELF header | section data... | shstrtab data | section headers
        let mut data_offset = elf_hdr_size;
        let mut section_offsets: Vec<u64> = Vec::new();
        let mut section_sizes: Vec<u64> = Vec::new();

        // SHT_NULL has no data
        section_offsets.push(0);
        section_sizes.push(0);

        for (_, _, _, sec_data) in sections {
            section_offsets.push(data_offset as u64);
            section_sizes.push(sec_data.len() as u64);
            data_offset += sec_data.len();
            // align to 8
            data_offset = (data_offset + 7) & !7;
        }

        // shstrtab section data
        let shstrtab_offset = data_offset as u64;
        let shstrtab_size = shstrtab.len() as u64;
        data_offset += shstrtab.len();
        data_offset = (data_offset + 7) & !7;

        let shoff = data_offset as u64;
        let shnum = (sections.len() + 2) as u16; // NULL + user sections + .shstrtab
        let shstrndx = shnum - 1;

        // Build the ELF
        let total_size = data_offset + shnum as usize * shent_size;
        let mut elf = vec![0u8; total_size];

        // ELF header
        elf[0..4].copy_from_slice(&ELFMAG);
        elf[EI_CLASS] = ELFCLASS64;
        elf[EI_DATA] = ELFDATA2LSB;
        elf[6] = 1; // EV_CURRENT
        // e_type = ET_REL
        write_u16(&mut elf, 16, 1, true);
        // e_machine = EM_X86_64
        write_u16(&mut elf, 18, 62, true);
        // e_version
        write_u32(&mut elf, 20, 1, true);
        // e_ehsize
        write_u16(&mut elf, 52, 64, true);
        // e_shentsize
        write_u16(&mut elf, 58, shent_size as u16, true);
        // e_shoff
        write_u64(&mut elf, 40, shoff, true);
        // e_shnum
        write_u16(&mut elf, 60, shnum, true);
        // e_shstrndx
        write_u16(&mut elf, 62, shstrndx, true);

        // Copy section data
        for (i, (_, _, _, sec_data)) in sections.iter().enumerate() {
            let off = section_offsets[i + 1] as usize;
            if !sec_data.is_empty() {
                elf[off..off + sec_data.len()].copy_from_slice(sec_data);
            }
        }

        // Copy shstrtab data
        elf[shstrtab_offset as usize..shstrtab_offset as usize + shstrtab.len()]
            .copy_from_slice(&shstrtab);

        // Write section headers
        let sh_base = shoff as usize;

        // SHT_NULL (index 0) - already zeroed

        // User sections
        for (i, (_, sh_type, flags, _)) in sections.iter().enumerate() {
            let idx = i + 1;
            let sh_off = sh_base + idx * shent_size;
            // sh_name
            write_u32(&mut elf, sh_off, name_offsets[idx], true);
            // sh_type
            write_u32(&mut elf, sh_off + 4, *sh_type, true);
            // sh_flags
            write_u64(&mut elf, sh_off + 8, *flags, true);
            // sh_offset
            write_u64(&mut elf, sh_off + 24, section_offsets[idx], true);
            // sh_size
            write_u64(&mut elf, sh_off + 32, section_sizes[idx], true);
        }

        // .shstrtab section header
        let shstrtab_sh_off = sh_base + (shnum - 1) as usize * shent_size;
        write_u32(&mut elf, shstrtab_sh_off, shstrtab_name_offset, true);
        write_u32(&mut elf, shstrtab_sh_off + 4, SHT_STRTAB, true);
        write_u64(&mut elf, shstrtab_sh_off + 24, shstrtab_offset, true);
        write_u64(&mut elf, shstrtab_sh_off + 32, shstrtab_size, true);

        elf
    }

    /// Build a minimal ELF with a symbol table containing the given global symbols.
    fn make_elf64_with_symbols(symbol_names: &[&str]) -> Vec<u8> {
        // We'll create: .text (SHF_ALLOC), .symtab, .strtab, .shstrtab

        let elf_hdr_size = 64usize;
        let shent_size = 64usize;

        // Build .strtab
        let mut strtab = vec![0u8]; // null byte at start
        let mut sym_name_offsets: Vec<u32> = Vec::new();
        for name in symbol_names {
            sym_name_offsets.push(strtab.len() as u32);
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
        }

        // Build .symtab entries (24 bytes each for ELF64)
        // First entry is always STN_UNDEF (all zeros)
        let sym_entry_size = 24usize;
        let mut symtab_data = vec![0u8; sym_entry_size]; // STN_UNDEF
        for (i, _name) in symbol_names.iter().enumerate() {
            let mut entry = vec![0u8; sym_entry_size];
            // st_name
            write_u32(&mut entry, 0, sym_name_offsets[i], true);
            // st_info: STB_GLOBAL (1) << 4 | STT_FUNC (2)
            entry[4] = (1 << 4) | 2;
            // st_shndx: 1 (.text section)
            write_u16(&mut entry, 6, 1, true);
            // st_value
            write_u64(&mut entry, 8, (i as u64 + 1) * 0x10, true);
            // st_size
            write_u64(&mut entry, 16, 0x10, true);
            symtab_data.extend_from_slice(&entry);
        }

        // .text section data (just some bytes)
        let text_data = vec![0xCC; 64]; // int3 opcodes as placeholder

        // Build .shstrtab
        let section_names: &[&str] = &[".text", ".symtab", ".strtab", ".shstrtab"];
        let mut shstrtab = vec![0u8];
        let mut shstrtab_offsets: Vec<u32> = Vec::new();
        shstrtab_offsets.push(0); // SHT_NULL
        for name in section_names {
            shstrtab_offsets.push(shstrtab.len() as u32);
            shstrtab.extend_from_slice(name.as_bytes());
            shstrtab.push(0);
        }

        // Layout
        let shnum: u16 = 5; // NULL + .text + .symtab + .strtab + .shstrtab
        let shstrndx: u16 = 4;

        let mut offset = elf_hdr_size;

        // .text
        let text_offset = offset;
        offset += text_data.len();
        offset = (offset + 7) & !7;

        // .symtab
        let symtab_offset = offset;
        offset += symtab_data.len();
        offset = (offset + 7) & !7;

        // .strtab
        let strtab_offset = offset;
        offset += strtab.len();
        offset = (offset + 7) & !7;

        // .shstrtab
        let shstrtab_data_offset = offset;
        offset += shstrtab.len();
        offset = (offset + 7) & !7;

        let shoff = offset;
        let total_size = offset + shnum as usize * shent_size;
        let mut elf = vec![0u8; total_size];

        // ELF header
        elf[0..4].copy_from_slice(&ELFMAG);
        elf[EI_CLASS] = ELFCLASS64;
        elf[EI_DATA] = ELFDATA2LSB;
        elf[6] = 1;
        write_u16(&mut elf, 16, 1, true); // ET_REL
        write_u16(&mut elf, 18, 62, true); // EM_X86_64
        write_u32(&mut elf, 20, 1, true);
        write_u16(&mut elf, 52, 64, true);
        write_u16(&mut elf, 58, shent_size as u16, true);
        write_u64(&mut elf, 40, shoff as u64, true);
        write_u16(&mut elf, 60, shnum, true);
        write_u16(&mut elf, 62, shstrndx, true);

        // Copy data
        elf[text_offset..text_offset + text_data.len()].copy_from_slice(&text_data);
        elf[symtab_offset..symtab_offset + symtab_data.len()].copy_from_slice(&symtab_data);
        elf[strtab_offset..strtab_offset + strtab.len()].copy_from_slice(&strtab);
        elf[shstrtab_data_offset..shstrtab_data_offset + shstrtab.len()]
            .copy_from_slice(&shstrtab);

        // Section headers
        let sh = |idx: usize| shoff + idx * shent_size;

        // 0: SHT_NULL - already zero

        // 1: .text
        write_u32(&mut elf, sh(1), shstrtab_offsets[1], true);
        write_u32(&mut elf, sh(1) + 4, 1, true); // SHT_PROGBITS
        write_u64(&mut elf, sh(1) + 8, SHF_ALLOC, true);
        write_u64(&mut elf, sh(1) + 24, text_offset as u64, true);
        write_u64(&mut elf, sh(1) + 32, text_data.len() as u64, true);

        // 2: .symtab
        write_u32(&mut elf, sh(2), shstrtab_offsets[2], true);
        write_u32(&mut elf, sh(2) + 4, SHT_SYMTAB, true);
        write_u64(&mut elf, sh(2) + 24, symtab_offset as u64, true);
        write_u64(&mut elf, sh(2) + 32, symtab_data.len() as u64, true);
        write_u32(&mut elf, sh(2) + 40, 3, true); // link = .strtab index
        write_u64(&mut elf, sh(2) + 56, sym_entry_size as u64, true);

        // 3: .strtab
        write_u32(&mut elf, sh(3), shstrtab_offsets[3], true);
        write_u32(&mut elf, sh(3) + 4, SHT_STRTAB, true);
        write_u64(&mut elf, sh(3) + 24, strtab_offset as u64, true);
        write_u64(&mut elf, sh(3) + 32, strtab.len() as u64, true);

        // 4: .shstrtab
        write_u32(&mut elf, sh(4), shstrtab_offsets[4], true);
        write_u32(&mut elf, sh(4) + 4, SHT_STRTAB, true);
        write_u64(&mut elf, sh(4) + 24, shstrtab_data_offset as u64, true);
        write_u64(&mut elf, sh(4) + 32, shstrtab.len() as u64, true);

        elf
    }

    // ====================================================================
    // Archive magic and header parsing tests
    // ====================================================================

    #[test]
    fn test_archive_magic() {
        let data = make_archive(&[("test.o", b"hello")]);
        assert_eq!(&data[..8], AR_MAGIC.as_slice());
    }

    #[test]
    fn test_archive_magic_bad() {
        let bad = b"NOT_ARCH";
        assert!(Archive::parse(bad).is_err());
    }

    #[test]
    fn test_archive_too_small() {
        assert!(Archive::parse(b"!<arch").is_err());
    }

    #[test]
    fn test_header_fmag() {
        let data = make_archive(&[("a.o", b"x")]);
        // The first member header starts at offset 8
        assert_eq!(&data[66..68], AR_FMAG.as_slice());
    }

    #[test]
    fn test_parse_header_field_decimal() {
        assert_eq!(parse_header_field(b"1234567890  ").unwrap(), 1234567890);
    }

    #[test]
    fn test_parse_header_field_blank() {
        assert_eq!(parse_header_field(b"            ").unwrap(), 0);
    }

    #[test]
    fn test_parse_header_octal() {
        assert_eq!(parse_header_octal(b"100644  ").unwrap(), 0o100644);
    }

    // ====================================================================
    // Member name parsing tests
    // ====================================================================

    #[test]
    fn test_short_name_roundtrip() {
        let data = make_archive(&[("foo.o", b"data")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members.len(), 1);
        assert_eq!(ar.members[0].header.name, "foo.o");
    }

    #[test]
    fn test_short_name_max_length() {
        // 15 chars is the max for short names (name + "/" must fit in 16)
        let name = "123456789012345";
        let data = make_archive(&[(name, b"data")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members[0].header.name, name);
    }

    #[test]
    fn test_long_name_gnu_roundtrip() {
        // Names > 15 chars trigger GNU extended name encoding
        let name = "very_long_object_filename.o";
        let data = make_archive(&[(name, b"data")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members[0].header.name, name);
    }

    #[test]
    fn test_bsd_name_decode() {
        let raw_name = "#1/8";
        let member_data = b"test.o\0\0real_data";
        let (name, actual) = decode_bsd_name(raw_name, member_data).unwrap();
        assert_eq!(name, "test.o");
        assert_eq!(actual, b"real_data");
    }

    #[test]
    fn test_bsd_name_exact_length() {
        let raw_name = "#1/4";
        let member_data = b"ab.odata";
        let (name, actual) = decode_bsd_name(raw_name, member_data).unwrap();
        assert_eq!(name, "ab.o");
        assert_eq!(actual, b"data");
    }

    #[test]
    fn test_gnu_strtab_entry() {
        let strtab = b"first.o/\nsecond_long_name.o/\n";
        let name1 = read_gnu_strtab_entry(strtab, 0).unwrap();
        assert_eq!(name1, "first.o");
        let name2 = read_gnu_strtab_entry(strtab, 9).unwrap();
        assert_eq!(name2, "second_long_name.o");
    }

    #[test]
    fn test_gnu_strtab_out_of_range() {
        let strtab = b"foo/\n";
        assert!(read_gnu_strtab_entry(strtab, 100).is_err());
    }

    // ====================================================================
    // Member insertion and replacement tests
    // ====================================================================

    #[test]
    fn test_insert_new_member() {
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "a.o".into(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: 3,
            },
            data: b"aaa".to_vec(),
        });
        assert_eq!(ar.members.len(), 1);
        assert_eq!(ar.members[0].header.name, "a.o");
    }

    #[test]
    fn test_replace_existing_member() {
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "a.o".into(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: 3,
            },
            data: b"old".to_vec(),
        });

        // Replace
        if let Some(idx) = ar.find_member("a.o") {
            ar.members[idx].data = b"new".to_vec();
            ar.members[idx].header.size = 3;
        }

        assert_eq!(ar.members.len(), 1);
        assert_eq!(ar.members[0].data, b"new");
    }

    #[test]
    fn test_insert_preserves_order() {
        let data = make_archive(&[("a.o", b"1"), ("b.o", b"2"), ("c.o", b"3")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members[0].header.name, "a.o");
        assert_eq!(ar.members[1].header.name, "b.o");
        assert_eq!(ar.members[2].header.name, "c.o");
    }

    // ====================================================================
    // Member deletion tests
    // ====================================================================

    #[test]
    fn test_delete_member() {
        let data = make_archive(&[("a.o", b"1"), ("b.o", b"2"), ("c.o", b"3")]);
        let mut ar = Archive::parse(&data).unwrap();
        if let Some(idx) = ar.find_member("b.o") {
            ar.members.remove(idx);
        }
        assert_eq!(ar.members.len(), 2);
        assert_eq!(ar.members[0].header.name, "a.o");
        assert_eq!(ar.members[1].header.name, "c.o");
    }

    #[test]
    fn test_delete_first_member() {
        let data = make_archive(&[("a.o", b"1"), ("b.o", b"2")]);
        let mut ar = Archive::parse(&data).unwrap();
        ar.members.remove(0);
        assert_eq!(ar.members.len(), 1);
        assert_eq!(ar.members[0].header.name, "b.o");
    }

    #[test]
    fn test_delete_last_member() {
        let data = make_archive(&[("a.o", b"1"), ("b.o", b"2")]);
        let mut ar = Archive::parse(&data).unwrap();
        ar.members.pop();
        assert_eq!(ar.members.len(), 1);
        assert_eq!(ar.members[0].header.name, "a.o");
    }

    #[test]
    fn test_delete_nonexistent() {
        let data = make_archive(&[("a.o", b"1")]);
        let ar = Archive::parse(&data).unwrap();
        assert!(ar.find_member("nope.o").is_none());
    }

    // ====================================================================
    // Table of contents tests
    // ====================================================================

    #[test]
    fn test_list_members() {
        let data = make_archive(&[("x.o", b"xx"), ("y.o", b"yy")]);
        let ar = Archive::parse(&data).unwrap();
        let names: Vec<&str> = ar.members.iter().map(|m| m.header.name.as_str()).collect();
        assert_eq!(names, vec!["x.o", "y.o"]);
    }

    #[test]
    fn test_list_empty_archive() {
        let data = make_archive(&[]);
        let ar = Archive::parse(&data).unwrap();
        assert!(ar.members.is_empty());
    }

    // ====================================================================
    // Member extraction tests
    // ====================================================================

    #[test]
    fn test_extract_data_integrity() {
        let original = b"Hello, World! This is test data for extraction.";
        let data = make_archive(&[("test.o", original)]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members[0].data, original);
    }

    #[test]
    fn test_extract_binary_data() {
        let binary: Vec<u8> = (0..=255).collect();
        let data = make_archive(&[("binary.o", &binary)]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members[0].data, binary);
    }

    #[test]
    fn test_extract_specific_member() {
        let data = make_archive(&[("a.o", b"aaa"), ("b.o", b"bbb"), ("c.o", b"ccc")]);
        let ar = Archive::parse(&data).unwrap();
        let idx = ar.find_member("b.o").unwrap();
        assert_eq!(ar.members[idx].data, b"bbb");
    }

    // ====================================================================
    // Quick append tests
    // ====================================================================

    #[test]
    fn test_quick_append_adds_duplicate() {
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "a.o".into(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: 3,
            },
            data: b"old".to_vec(),
        });
        // Quick append: no duplicate check
        ar.members.push(ArMember {
            header: ArHeader {
                name: "a.o".into(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: 3,
            },
            data: b"new".to_vec(),
        });
        assert_eq!(ar.members.len(), 2);
        assert_eq!(ar.members[0].data, b"old");
        assert_eq!(ar.members[1].data, b"new");
    }

    #[test]
    fn test_quick_append_serializes() {
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "x.o".into(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: 4,
            },
            data: b"data".to_vec(),
        });
        let serialized = ar.serialize(false);
        let parsed = Archive::parse(&serialized).unwrap();
        assert_eq!(parsed.members.len(), 1);
    }

    // ====================================================================
    // Verbose mode formatting tests
    // ====================================================================

    #[test]
    fn test_verbose_format_has_mode() {
        let hdr = ArHeader {
            name: "test.o".into(),
            mtime: 0,
            uid: 1000,
            gid: 1000,
            mode: 0o100644,
            size: 100,
        };
        let formatted = format!(
            "{:o} {}/{} {:>6} {} {}",
            hdr.mode, hdr.uid, hdr.gid, hdr.size,
            format_timestamp(hdr.mtime), hdr.name
        );
        assert!(formatted.contains("100644"));
        assert!(formatted.contains("1000/1000"));
        assert!(formatted.contains("test.o"));
    }

    #[test]
    fn test_verbose_format_size_alignment() {
        let hdr = ArHeader {
            name: "f.o".into(),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
            size: 42,
        };
        let formatted = format!("{:>6}", hdr.size);
        assert_eq!(formatted, "    42");
    }

    // ====================================================================
    // Deterministic mode tests
    // ====================================================================

    #[test]
    fn test_deterministic_zeroed_timestamps() {
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "a.o".into(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: 4,
            },
            data: b"data".to_vec(),
        });
        let serialized = ar.serialize(false);
        let parsed = Archive::parse(&serialized).unwrap();
        assert_eq!(parsed.members[0].header.mtime, 0);
        assert_eq!(parsed.members[0].header.uid, 0);
        assert_eq!(parsed.members[0].header.gid, 0);
    }

    #[test]
    fn test_deterministic_reproducible() {
        let members: &[(&str, &[u8])] = &[("a.o", b"aaa"), ("b.o", b"bbb")];
        let data1 = make_archive(members);
        let data2 = make_archive(members);
        assert_eq!(data1, data2);
    }

    // ====================================================================
    // Position modifier tests
    // ====================================================================

    #[test]
    fn test_position_after() {
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "a.o".into(), mtime: 0, uid: 0, gid: 0, mode: 0o100644, size: 1,
            },
            data: b"a".to_vec(),
        });
        ar.members.push(ArMember {
            header: ArHeader {
                name: "c.o".into(), mtime: 0, uid: 0, gid: 0, mode: 0o100644, size: 1,
            },
            data: b"c".to_vec(),
        });

        let opts = ArOptions {
            operation: 'r',
            verbose: false,
            create_silently: true,
            write_symtab: false,
            update_only: false,
            deterministic: false,
            position_after: Some("a.o".into()),
            position_before: None,
        };

        let insert_idx = find_insert_position(&ar, &opts);
        assert_eq!(insert_idx, 1); // after a.o (index 0) = index 1

        ar.members.insert(insert_idx, ArMember {
            header: ArHeader {
                name: "b.o".into(), mtime: 0, uid: 0, gid: 0, mode: 0o100644, size: 1,
            },
            data: b"b".to_vec(),
        });

        assert_eq!(ar.members[0].header.name, "a.o");
        assert_eq!(ar.members[1].header.name, "b.o");
        assert_eq!(ar.members[2].header.name, "c.o");
    }

    #[test]
    fn test_position_before() {
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "a.o".into(), mtime: 0, uid: 0, gid: 0, mode: 0o100644, size: 1,
            },
            data: b"a".to_vec(),
        });
        ar.members.push(ArMember {
            header: ArHeader {
                name: "c.o".into(), mtime: 0, uid: 0, gid: 0, mode: 0o100644, size: 1,
            },
            data: b"c".to_vec(),
        });

        let opts = ArOptions {
            operation: 'r',
            verbose: false,
            create_silently: true,
            write_symtab: false,
            update_only: false,
            deterministic: false,
            position_after: None,
            position_before: Some("c.o".into()),
        };

        let insert_idx = find_insert_position(&ar, &opts);
        assert_eq!(insert_idx, 1); // before c.o (index 1) = index 1

        ar.members.insert(insert_idx, ArMember {
            header: ArHeader {
                name: "b.o".into(), mtime: 0, uid: 0, gid: 0, mode: 0o100644, size: 1,
            },
            data: b"b".to_vec(),
        });

        assert_eq!(ar.members[0].header.name, "a.o");
        assert_eq!(ar.members[1].header.name, "b.o");
        assert_eq!(ar.members[2].header.name, "c.o");
    }

    #[test]
    fn test_position_after_nonexistent_appends() {
        let ar = Archive::new();
        let opts = ArOptions {
            operation: 'r',
            verbose: false,
            create_silently: true,
            write_symtab: false,
            update_only: false,
            deterministic: false,
            position_after: Some("nonexistent.o".into()),
            position_before: None,
        };
        let idx = find_insert_position(&ar, &opts);
        assert_eq!(idx, 0); // appends to end (empty archive)
    }

    // ====================================================================
    // ELF header parsing tests
    // ====================================================================

    #[test]
    fn test_elf_magic_detection() {
        let elf = make_elf64(&[(".text", 1, SHF_ALLOC, &[0xCC; 16])]);
        let info = parse_elf_header(&elf).unwrap();
        assert_eq!(info.class, ELFCLASS64);
        assert!(info.little_endian);
    }

    #[test]
    fn test_elf_not_elf() {
        assert!(parse_elf_header(b"not an elf file").is_none());
    }

    #[test]
    fn test_elf_too_small() {
        assert!(parse_elf_header(b"\x7fEL").is_none());
    }

    #[test]
    fn test_elf_sections_parsed() {
        let elf = make_elf64(&[
            (".text", 1, SHF_ALLOC, &[0xCC; 16]),
            (".data", 1, SHF_ALLOC, &[0; 32]),
        ]);
        let info = parse_elf_header(&elf).unwrap();
        let sections = parse_elf_sections(&elf, &info);
        // SHT_NULL + .text + .data + .shstrtab = 4
        assert_eq!(sections.len(), 4);
        assert_eq!(sections[1].name, ".text");
        assert_eq!(sections[2].name, ".data");
    }

    #[test]
    fn test_elf_section_flags() {
        let elf = make_elf64(&[(".text", 1, SHF_ALLOC, &[0xCC; 16])]);
        let info = parse_elf_header(&elf).unwrap();
        let sections = parse_elf_sections(&elf, &info);
        assert_eq!(sections[1].flags & SHF_ALLOC, SHF_ALLOC);
    }

    // ====================================================================
    // ELF symbol extraction tests
    // ====================================================================

    #[test]
    fn test_extract_symbols_from_elf() {
        let elf = make_elf64_with_symbols(&["foo", "bar", "baz"]);
        let syms = extract_elf_symbols(&elf);
        assert_eq!(syms.len(), 3);
        assert!(syms.contains(&"foo".to_string()));
        assert!(syms.contains(&"bar".to_string()));
        assert!(syms.contains(&"baz".to_string()));
    }

    #[test]
    fn test_extract_symbols_no_symtab() {
        let elf = make_elf64(&[(".text", 1, SHF_ALLOC, &[0xCC; 16])]);
        let syms = extract_elf_symbols(&elf);
        assert!(syms.is_empty());
    }

    #[test]
    fn test_extract_symbols_not_elf() {
        let syms = extract_elf_symbols(b"not elf data");
        assert!(syms.is_empty());
    }

    // ====================================================================
    // Strip section identification tests
    // ====================================================================

    #[test]
    fn test_strip_all_removes_symtab() {
        assert!(is_strip_all_section(".symtab", SHT_SYMTAB, 0));
    }

    #[test]
    fn test_strip_all_removes_strtab() {
        assert!(is_strip_all_section(".strtab", SHT_STRTAB, 0));
    }

    #[test]
    fn test_strip_all_removes_debug() {
        assert!(is_strip_all_section(".debug_info", 0, 0));
        assert!(is_strip_all_section(".debug_line", 0, 0));
        assert!(is_strip_all_section(".debug_abbrev", 0, 0));
    }

    #[test]
    fn test_strip_all_removes_comment() {
        assert!(is_strip_all_section(".comment", 0, 0));
    }

    #[test]
    fn test_strip_all_removes_gnu_stack() {
        assert!(is_strip_all_section(".note.GNU-stack", 0, 0));
    }

    #[test]
    fn test_strip_preserves_alloc() {
        assert!(!is_strip_all_section(".text", 1, SHF_ALLOC));
        assert!(!is_strip_all_section(".data", 1, SHF_ALLOC));
        assert!(!is_strip_all_section(".rodata", 1, SHF_ALLOC));
    }

    #[test]
    fn test_strip_preserves_dynsym() {
        // .dynsym has SHF_ALLOC set
        assert!(!is_strip_all_section(".dynsym", 11, SHF_ALLOC));
        assert!(!is_strip_all_section(".dynstr", SHT_STRTAB, SHF_ALLOC));
    }

    #[test]
    fn test_strip_debug_only_sections() {
        assert!(is_strip_debug_section(".debug_info", 0, 0));
        assert!(is_strip_debug_section(".zdebug_info", 0, 0));
        assert!(!is_strip_debug_section(".symtab", SHT_SYMTAB, 0));
        assert!(!is_strip_debug_section(".comment", 0, 0));
    }

    #[test]
    fn test_strip_unneeded_sections() {
        assert!(is_strip_unneeded_section(".symtab", SHT_SYMTAB, 0));
        assert!(is_strip_unneeded_section(".debug_info", 0, 0));
        assert!(!is_strip_unneeded_section(".text", 1, SHF_ALLOC));
    }

    // ====================================================================
    // Strip ELF tests
    // ====================================================================

    #[test]
    fn test_strip_removes_debug_sections() {
        let elf = make_elf64(&[
            (".text", 1, SHF_ALLOC, &[0xCC; 16]),
            (".debug_info", 0, 0, &[0; 32]),
            (".debug_line", 0, 0, &[0; 16]),
        ]);
        let opts = StripOptions {
            strip_all: true,
            strip_debug: false,
            strip_unneeded: false,
            keep_symbols: Vec::new(),
            output_file: None,
            preserve_dates: false,
            verbose: false,
        };
        let stripped = strip_elf(&elf, &opts).unwrap();
        let info = parse_elf_header(&stripped).unwrap();
        let sections = parse_elf_sections(&stripped, &info);
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        assert!(!names.contains(&".debug_info"));
        assert!(!names.contains(&".debug_line"));
        assert!(names.contains(&".text"));
    }

    #[test]
    fn test_strip_debug_mode_keeps_symtab() {
        let elf = make_elf64_with_symbols(&["my_func"]);
        // Verify symtab exists
        let info_before = parse_elf_header(&elf).unwrap();
        let secs_before = parse_elf_sections(&elf, &info_before);
        assert!(secs_before.iter().any(|s| s.name == ".symtab"));

        let opts = StripOptions {
            strip_all: false,
            strip_debug: true,
            strip_unneeded: false,
            keep_symbols: Vec::new(),
            output_file: None,
            preserve_dates: false,
            verbose: false,
        };
        let stripped = strip_elf(&elf, &opts).unwrap();
        let info = parse_elf_header(&stripped).unwrap();
        let sections = parse_elf_sections(&stripped, &info);
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&".symtab"));
    }

    #[test]
    fn test_strip_keep_symbol() {
        let elf = make_elf64_with_symbols(&["keep_me"]);
        let opts = StripOptions {
            strip_all: true,
            strip_debug: false,
            strip_unneeded: false,
            keep_symbols: vec!["keep_me".into()],
            output_file: None,
            preserve_dates: false,
            verbose: false,
        };
        let stripped = strip_elf(&elf, &opts).unwrap();
        let info = parse_elf_header(&stripped).unwrap();
        let sections = parse_elf_sections(&stripped, &info);
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        // With keep-symbol, .symtab and .strtab should be preserved
        assert!(names.contains(&".symtab"));
    }

    #[test]
    fn test_strip_not_elf() {
        let opts = StripOptions::new();
        assert!(strip_elf(b"not elf", &opts).is_err());
    }

    // ====================================================================
    // Ranlib / symbol table tests
    // ====================================================================

    #[test]
    fn test_symtab_generation() {
        let elf = make_elf64_with_symbols(&["func_a", "func_b"]);
        let data = make_archive_with_symtab(&[("code.o", &elf)]);
        // The archive should start with magic
        assert_eq!(&data[..8], AR_MAGIC.as_slice());
        // After magic, first header should be "/" (symtab)
        let name_field = std::str::from_utf8(&data[8..24]).unwrap().trim();
        assert_eq!(name_field, "/");
    }

    #[test]
    fn test_symtab_big_endian_count() {
        let elf = make_elf64_with_symbols(&["sym1", "sym2", "sym3"]);
        let ar = Archive {
            members: vec![ArMember {
                header: ArHeader {
                    name: "test.o".into(),
                    mtime: 0,
                    uid: 0,
                    gid: 0,
                    mode: 0o100644,
                    size: elf.len() as u64,
                },
                data: elf,
            }],
        };
        let symtab = ar.build_symbol_table();
        assert!(!symtab.is_empty());
        // First 4 bytes: symbol count in big-endian
        let count = u32::from_be_bytes([symtab[0], symtab[1], symtab[2], symtab[3]]);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_symtab_names_present() {
        let elf = make_elf64_with_symbols(&["alpha", "beta"]);
        let ar = Archive {
            members: vec![ArMember {
                header: ArHeader {
                    name: "lib.o".into(),
                    mtime: 0,
                    uid: 0,
                    gid: 0,
                    mode: 0o100644,
                    size: elf.len() as u64,
                },
                data: elf,
            }],
        };
        let symtab = ar.build_symbol_table();
        // The names section should contain "alpha\0" and "beta\0"
        let names_start = 4 + 2 * 4; // 4 (count) + 2 * 4 (offsets)
        let names_bytes = &symtab[names_start..];
        let names_str = String::from_utf8_lossy(names_bytes);
        assert!(names_str.contains("alpha"));
        assert!(names_str.contains("beta"));
    }

    #[test]
    fn test_symtab_no_symbols() {
        let ar = Archive {
            members: vec![ArMember {
                header: ArHeader {
                    name: "empty.o".into(),
                    mtime: 0,
                    uid: 0,
                    gid: 0,
                    mode: 0o100644,
                    size: 4,
                },
                data: b"data".to_vec(), // not ELF
            }],
        };
        let symtab = ar.build_symbol_table();
        assert!(symtab.is_empty());
    }

    #[test]
    fn test_ranlib_roundtrip() {
        let elf = make_elf64_with_symbols(&["my_symbol"]);
        let mut ar = Archive::new();
        ar.members.push(ArMember {
            header: ArHeader {
                name: "obj.o".into(),
                mtime: 0,
                uid: 0,
                gid: 0,
                mode: 0o100644,
                size: elf.len() as u64,
            },
            data: elf,
        });
        let with_index = ar.serialize(true);
        let parsed = Archive::parse(&with_index).unwrap();
        assert_eq!(parsed.members.len(), 1);
        assert_eq!(parsed.members[0].header.name, "obj.o");
    }

    // ====================================================================
    // Personality detection tests
    // ====================================================================

    #[test]
    fn test_personality_ar() {
        assert_eq!(detect_personality("ar"), "ar");
        assert_eq!(detect_personality("/usr/bin/ar"), "ar");
        assert_eq!(detect_personality("./ar"), "ar");
    }

    #[test]
    fn test_personality_ranlib() {
        assert_eq!(detect_personality("ranlib"), "ranlib");
        assert_eq!(detect_personality("/usr/bin/ranlib"), "ranlib");
    }

    #[test]
    fn test_personality_strip() {
        assert_eq!(detect_personality("strip"), "strip");
        assert_eq!(detect_personality("/usr/bin/strip"), "strip");
    }

    #[test]
    fn test_personality_unknown_defaults_ar() {
        assert_eq!(detect_personality("something_else"), "ar");
        assert_eq!(detect_personality("mytools"), "ar");
    }

    #[test]
    fn test_personality_with_extension() {
        assert_eq!(detect_personality("ar.exe"), "ar");
        assert_eq!(detect_personality("ranlib.exe"), "ranlib");
        assert_eq!(detect_personality("strip.exe"), "strip");
    }

    // ====================================================================
    // Edge case tests
    // ====================================================================

    #[test]
    fn test_empty_archive_roundtrip() {
        let ar = Archive::new();
        let data = ar.serialize(false);
        assert_eq!(&data[..8], AR_MAGIC.as_slice());
        let parsed = Archive::parse(&data).unwrap();
        assert!(parsed.members.is_empty());
    }

    #[test]
    fn test_single_member_roundtrip() {
        let data = make_archive(&[("only.o", b"single member data")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members.len(), 1);
        assert_eq!(ar.members[0].header.name, "only.o");
        assert_eq!(ar.members[0].data, b"single member data");
    }

    #[test]
    fn test_large_name_roundtrip() {
        let name = "this_is_a_very_long_object_file_name_that_exceeds_sixteen_characters.o";
        let data = make_archive(&[(name, b"data")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members[0].header.name, name);
    }

    #[test]
    fn test_empty_member_data() {
        let data = make_archive(&[("empty.o", b"")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members[0].data.len(), 0);
    }

    #[test]
    fn test_odd_size_padding() {
        // Members with odd sizes should be padded to even boundaries
        let data = make_archive(&[("a.o", b"odd"), ("b.o", b"next")]);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members.len(), 2);
        assert_eq!(ar.members[0].data, b"odd");
        assert_eq!(ar.members[1].data, b"next");
    }

    #[test]
    fn test_many_members() {
        let members: Vec<(String, Vec<u8>)> = (0..50)
            .map(|i| (format!("m{i:03}.o"), format!("data_{i}").into_bytes()))
            .collect();
        let member_refs: Vec<(&str, &[u8])> = members.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let data = make_archive(&member_refs);
        let ar = Archive::parse(&data).unwrap();
        assert_eq!(ar.members.len(), 50);
        for (i, member) in ar.members.iter().enumerate() {
            assert_eq!(member.header.name, format!("m{i:03}.o"));
        }
    }

    #[test]
    fn test_align2() {
        assert_eq!(align2(0), 0);
        assert_eq!(align2(1), 2);
        assert_eq!(align2(2), 2);
        assert_eq!(align2(3), 4);
        assert_eq!(align2(100), 100);
        assert_eq!(align2(101), 102);
    }

    #[test]
    fn test_member_basename() {
        assert_eq!(member_basename("foo.o"), "foo.o");
        assert_eq!(member_basename("/path/to/bar.o"), "bar.o");
        assert_eq!(member_basename("./local.o"), "local.o");
    }

    #[test]
    fn test_ar_header_short_name_bytes() {
        let hdr = ArHeader {
            name: "test.o".into(),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
            size: 100,
        };
        let bytes = hdr.to_bytes_short(None).unwrap();
        assert_eq!(bytes.len(), AR_HDR_SIZE);
        assert_eq!(&bytes[58..60], AR_FMAG.as_slice());
    }

    #[test]
    fn test_ar_header_too_long_returns_none() {
        let hdr = ArHeader {
            name: "a_very_long_name_that_wont_fit.o".into(),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
            size: 0,
        };
        assert!(hdr.to_bytes_short(None).is_none());
    }

    #[test]
    fn test_parse_ar_args_basic() {
        let args: Vec<String> = ["rcs", "libfoo.a", "foo.o", "bar.o"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (opts, archive, members) = parse_ar_args(&args).unwrap();
        assert_eq!(opts.operation, 'r');
        assert!(opts.create_silently);
        assert!(opts.write_symtab);
        assert_eq!(archive, "libfoo.a");
        assert_eq!(members, vec!["foo.o", "bar.o"]);
    }

    #[test]
    fn test_parse_ar_args_no_op() {
        let args: Vec<String> = vec!["v".into()];
        assert!(parse_ar_args(&args).is_err());
    }

    #[test]
    fn test_parse_strip_args_defaults() {
        let args: Vec<String> = ["binary"].iter().map(|s| s.to_string()).collect();
        let (opts, files) = parse_strip_args(&args).unwrap();
        assert!(opts.strip_all);
        assert!(!opts.strip_debug);
        assert_eq!(files, vec!["binary"]);
    }

    #[test]
    fn test_parse_strip_args_debug() {
        let args: Vec<String> = ["-g", "binary"].iter().map(|s| s.to_string()).collect();
        let (opts, _) = parse_strip_args(&args).unwrap();
        assert!(opts.strip_debug);
        assert!(!opts.strip_all);
    }

    #[test]
    fn test_parse_strip_args_keep_symbol() {
        let args: Vec<String> = ["-K", "main", "binary"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (opts, _) = parse_strip_args(&args).unwrap();
        assert_eq!(opts.keep_symbols, vec!["main"]);
    }

    #[test]
    fn test_parse_strip_args_keep_symbol_equals() {
        let args: Vec<String> = ["--keep-symbol=main", "binary"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (opts, _) = parse_strip_args(&args).unwrap();
        assert_eq!(opts.keep_symbols, vec!["main"]);
    }

    #[test]
    fn test_parse_strip_args_no_files() {
        let args: Vec<String> = ["-s"].iter().map(|s| s.to_string()).collect();
        assert!(parse_strip_args(&args).is_err());
    }

    #[test]
    fn test_read_cstring() {
        let data = b"hello\0world\0";
        assert_eq!(read_cstring(data, 0), "hello");
        assert_eq!(read_cstring(data, 6), "world");
    }

    #[test]
    fn test_read_cstring_at_end() {
        let data = b"abc";
        assert_eq!(read_cstring(data, 0), "abc");
    }

    #[test]
    fn test_read_cstring_empty() {
        let data = b"\0rest";
        assert_eq!(read_cstring(data, 0), "");
    }

    #[test]
    fn test_read_cstring_out_of_bounds() {
        let data = b"abc";
        assert_eq!(read_cstring(data, 100), "");
    }
}
