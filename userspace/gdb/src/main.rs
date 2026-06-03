//! OurOS `gdb` -- GDB-like debugger and gdbserver
//!
//! Multi-personality binary that acts as `gdb` or `gdbserver` depending on
//! the name used to invoke it (detected via `argv[0]`).
//!
//! # Personalities
//!
//! - **gdb**: interactive command-line debugger
//! - **gdbserver**: GDB remote protocol stub server
//!
//! # Usage
//!
//! ```text
//! gdb [OPTIONS] [EXECUTABLE]
//!   --help          Display help and exit
//!   --version       Display version and exit
//!   -q, --quiet     Suppress startup banner
//!   -x FILE         Execute commands from FILE
//!   --args          Pass remaining args to inferior
//!
//! gdbserver [HOST:]PORT EXECUTABLE [ARGS...]
//!   --help          Display help and exit
//!   --version       Display version and exit
//! ```
//!
//! # Debugger Commands
//!
//! - `run` / `r`                 Start or restart the inferior
//! - `continue` / `c`            Resume execution
//! - `step` / `s` / `si`         Step one instruction or source line
//! - `next` / `n` / `ni`         Step over (next line/instruction)
//! - `break` / `b` LOCATION      Set breakpoint (address, symbol, or file:line)
//! - `delete` [NUM]              Delete breakpoint(s)
//! - `info breakpoints`          List breakpoints
//! - `info registers`            Show register contents
//! - `info threads`              List threads
//! - `backtrace` / `bt`          Show call stack
//! - `print` / `p` EXPR          Evaluate and display expression
//! - `x/FMT ADDR`                Examine memory
//! - `list` / `l`                Show source lines
//! - `disassemble` / `disas`     Disassemble around current position
//! - `set VAR = VAL`             Set debugger variable
//! - `watch` EXPR                Set watchpoint
//! - `quit` / `q`                Exit debugger

#![cfg_attr(not(test), no_main)]
// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(dead_code)]

use std::io::{self, Write};

// ============================================================================
// Constants
// ============================================================================

const VERSION: &[u8] = b"0.1.0";
const GDB_BANNER: &[u8] = b"OurOS GDB 0.1.0 -- A GDB-like debugger\n\
    Copyright (C) 2026 OurOS Project.\n\
    Type \"help\" for a list of commands.\n";

// ============================================================================
// ELF Constants & Parsing
// ============================================================================

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];

const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;
const EI_NIDENT: usize = 16;

const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;

const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;

const EM_X86_64: u16 = 62;

const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_DYNSYM: u32 = 11;
const SHT_PROGBITS: u32 = 1;

const STT_FUNC: u8 = 2;
const STT_OBJECT: u8 = 1;
const STT_NOTYPE: u8 = 0;

const PT_LOAD: u32 = 1;

// x86_64 register indices in our register file
const REG_RAX: usize = 0;
const REG_RBX: usize = 1;
const REG_RCX: usize = 2;
const REG_RDX: usize = 3;
const REG_RSI: usize = 4;
const REG_RDI: usize = 5;
const REG_RBP: usize = 6;
const REG_RSP: usize = 7;
const REG_R8: usize = 8;
const REG_R9: usize = 9;
const REG_R10: usize = 10;
const REG_R11: usize = 11;
const REG_R12: usize = 12;
const REG_R13: usize = 13;
const REG_R14: usize = 14;
const REG_R15: usize = 15;
const REG_RIP: usize = 16;
const REG_RFLAGS: usize = 17;
const REG_CS: usize = 18;
const REG_SS: usize = 19;
const REG_DS: usize = 20;
const REG_ES: usize = 21;
const REG_FS: usize = 22;
const REG_GS: usize = 23;
const REG_COUNT: usize = 24;

const REG_NAMES: [&[u8]; REG_COUNT] = [
    b"rax", b"rbx", b"rcx", b"rdx",
    b"rsi", b"rdi", b"rbp", b"rsp",
    b"r8",  b"r9",  b"r10", b"r11",
    b"r12", b"r13", b"r14", b"r15",
    b"rip", b"rflags",
    b"cs",  b"ss",  b"ds",  b"es",
    b"fs",  b"gs",
];

// INT3 opcode for software breakpoints
const INT3_OPCODE: u8 = 0xCC;

// Maximum breakpoints, watchpoints, and threads
const MAX_BREAKPOINTS: usize = 256;
const MAX_WATCHPOINTS: usize = 64;
const MAX_THREADS: usize = 256;
const MAX_SYMBOLS: usize = 65536;
const MAX_SECTIONS: usize = 256;
const MAX_SEGMENTS: usize = 64;
const MAX_STACK_FRAMES: usize = 256;

// ============================================================================
// String / Number Helpers
// ============================================================================

/// Walk a null-terminated C string to produce a `&[u8]`.
///
/// # Safety
/// `ptr` must be a valid pointer to a null-terminated string or null.
unsafe fn cstr_to_slice(ptr: *const u8) -> &'static [u8] {
    if ptr.is_null() {
        return b"";
    }
    let mut len = 0usize;
    // SAFETY: Walking null-terminated C string from kernel/libc.
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
            if len >= 4096 {
                break;
            }
        }
        core::slice::from_raw_parts(ptr, len)
    }
}

/// Compare two byte slices for equality.
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        if a[i] != b[i] {
            return false;
        }
    }
    true
}

/// Check if `haystack` starts with `needle`.
fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    bytes_eq(&haystack[..needle.len()], needle)
}

/// Check if `haystack` ends with `needle`.
fn ends_with(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    bytes_eq(&haystack[haystack.len() - needle.len()..], needle)
}

/// Parse a decimal u64 from bytes. Returns `None` on failure.
fn parse_u64(s: &[u8]) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut val: u64 = 0;
    for &ch in s {
        if ch < b'0' || ch > b'9' {
            return None;
        }
        val = val.checked_mul(10)?;
        val = val.checked_add((ch - b'0') as u64)?;
    }
    Some(val)
}

/// Parse a hexadecimal u64 from bytes. Accepts optional 0x prefix.
fn parse_hex(s: &[u8]) -> Option<u64> {
    let s = if starts_with(s, b"0x") || starts_with(s, b"0X") {
        &s[2..]
    } else {
        s
    };
    if s.is_empty() {
        return None;
    }
    let mut val: u64 = 0;
    for &ch in s {
        let digit = match ch {
            b'0'..=b'9' => ch - b'0',
            b'a'..=b'f' => ch - b'a' + 10,
            b'A'..=b'F' => ch - b'A' + 10,
            _ => return None,
        };
        val = val.checked_mul(16)?;
        val = val.checked_add(digit as u64)?;
    }
    Some(val)
}

/// Parse either decimal or hex (0x prefixed) number.
fn parse_number(s: &[u8]) -> Option<u64> {
    if starts_with(s, b"0x") || starts_with(s, b"0X") {
        parse_hex(s)
    } else {
        parse_u64(s)
    }
}

/// Parse a signed i64 from bytes (optional leading '-').
fn parse_i64(s: &[u8]) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    if s[0] == b'-' {
        let val = parse_number(&s[1..])? as i64;
        Some(-val)
    } else {
        Some(parse_number(s)? as i64)
    }
}

/// Format a u64 into a decimal byte string, returning the number of bytes written.
fn format_u64(val: u64, buf: &mut [u8]) -> usize {
    if val == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
        }
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut n = 0usize;
    let mut v = val;
    while v > 0 {
        if n < tmp.len() {
            tmp[n] = b'0' + (v % 10) as u8;
        }
        n += 1;
        v /= 10;
    }
    let count = n.min(buf.len());
    for i in 0..count {
        buf[i] = tmp[n - 1 - i];
    }
    count
}

/// Format a u64 as hex with 0x prefix.
fn format_hex(val: u64, buf: &mut [u8]) -> usize {
    const HEX_DIGITS: &[u8] = b"0123456789abcdef";
    if buf.len() < 3 {
        return 0;
    }
    buf[0] = b'0';
    buf[1] = b'x';

    if val == 0 {
        buf[2] = b'0';
        return 3;
    }

    // Count hex digits needed
    let mut digits = 0u32;
    let mut tmp = val;
    while tmp > 0 {
        digits += 1;
        tmp >>= 4;
    }

    let total = 2 + digits as usize;
    if total > buf.len() {
        return 0;
    }

    let mut v = val;
    for i in (0..digits as usize).rev() {
        buf[2 + i] = HEX_DIGITS[(v & 0xf) as usize];
        v >>= 4;
    }
    total
}

/// Format a u64 as zero-padded hex (16 digits for addresses).
fn format_hex_padded(val: u64, buf: &mut [u8], width: usize) -> usize {
    const HEX_DIGITS: &[u8] = b"0123456789abcdef";
    let needed = 2 + width;
    if buf.len() < needed {
        return 0;
    }
    buf[0] = b'0';
    buf[1] = b'x';
    let mut v = val;
    for i in (0..width).rev() {
        buf[2 + i] = HEX_DIGITS[(v & 0xf) as usize];
        v >>= 4;
    }
    needed
}

/// Skip leading whitespace in a byte slice.
fn trim_start(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    &s[i..]
}

/// Skip trailing whitespace in a byte slice.
fn trim_end(s: &[u8]) -> &[u8] {
    let mut end = s.len();
    while end > 0 && (s[end - 1] == b' ' || s[end - 1] == b'\t' || s[end - 1] == b'\n' || s[end - 1] == b'\r') {
        end -= 1;
    }
    &s[..end]
}

/// Trim both sides.
fn trim(s: &[u8]) -> &[u8] {
    trim_end(trim_start(s))
}

/// Split a byte slice by whitespace, returning up to `max_parts` slices.
fn split_whitespace<'a>(s: &'a [u8], parts: &mut [&'a [u8]], max_parts: usize) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < s.len() && count < max_parts {
        // Skip whitespace
        while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        let start = i;
        while i < s.len() && s[i] != b' ' && s[i] != b'\t' {
            i += 1;
        }
        parts[count] = &s[start..i];
        count += 1;
    }
    count
}

/// Extract the basename from a path (everything after last `/` or `\`).
fn basename(path: &[u8]) -> &[u8] {
    let mut last_sep = 0;
    let mut found = false;
    for i in 0..path.len() {
        if path[i] == b'/' || path[i] == b'\\' {
            last_sep = i;
            found = true;
        }
    }
    if found {
        &path[last_sep + 1..]
    } else {
        path
    }
}

/// Read a little-endian u16 from a byte slice at offset.
fn read_u16_le(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 > data.len() {
        return None;
    }
    Some(u16::from_le_bytes([data[off], data[off + 1]]))
}

/// Read a little-endian u32 from a byte slice at offset.
fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[off], data[off + 1], data[off + 2], data[off + 3],
    ]))
}

/// Read a little-endian u64 from a byte slice at offset.
fn read_u64_le(data: &[u8], off: usize) -> Option<u64> {
    if off + 8 > data.len() {
        return None;
    }
    Some(u64::from_le_bytes([
        data[off], data[off + 1], data[off + 2], data[off + 3],
        data[off + 4], data[off + 5], data[off + 6], data[off + 7],
    ]))
}

// ============================================================================
// ELF Structures
// ============================================================================

/// Parsed ELF64 header.
#[derive(Clone)]
struct Elf64Header {
    e_type: u16,
    e_machine: u16,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// Parsed ELF64 section header.
#[derive(Clone)]
struct Elf64Shdr {
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
}

/// Parsed ELF64 program header.
#[derive(Clone)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

/// Parsed ELF64 symbol.
#[derive(Clone)]
struct Elf64Sym {
    name_offset: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

/// A resolved symbol with its name.
#[derive(Clone)]
struct Symbol {
    name: [u8; 128],
    name_len: usize,
    value: u64,
    size: u64,
    sym_type: u8,
    section: u16,
}

impl Symbol {
    fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

/// A section entry for our section table.
#[derive(Clone)]
struct Section {
    name: [u8; 64],
    name_len: usize,
    sh_type: u32,
    addr: u64,
    offset: u64,
    size: u64,
    flags: u64,
}

impl Section {
    fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

/// Loaded ELF information.
struct ElfInfo {
    header: Elf64Header,
    sections: Vec<Section>,
    symbols: Vec<Symbol>,
    segments: Vec<Elf64Phdr>,
    entry_point: u64,
    /// The raw ELF data.
    data: Vec<u8>,
}

impl ElfInfo {
    fn new() -> Self {
        Self {
            header: Elf64Header {
                e_type: 0,
                e_machine: 0,
                e_entry: 0,
                e_phoff: 0,
                e_shoff: 0,
                e_phentsize: 0,
                e_phnum: 0,
                e_shentsize: 0,
                e_shnum: 0,
                e_shstrndx: 0,
            },
            sections: Vec::new(),
            symbols: Vec::new(),
            segments: Vec::new(),
            entry_point: 0,
            data: Vec::new(),
        }
    }

    /// Find a symbol by name, returning its address.
    fn find_symbol(&self, name: &[u8]) -> Option<u64> {
        for sym in &self.symbols {
            if bytes_eq(sym.name_bytes(), name) {
                return Some(sym.value);
            }
        }
        None
    }

    /// Find a symbol by address (closest symbol at or before addr).
    fn find_symbol_at(&self, addr: u64) -> Option<&Symbol> {
        let mut best: Option<&Symbol> = None;
        let mut best_dist = u64::MAX;
        for sym in &self.symbols {
            if sym.value <= addr {
                let dist = addr - sym.value;
                if dist < best_dist && sym.name_len > 0 {
                    best_dist = dist;
                    best = Some(sym);
                }
            }
        }
        best
    }

    /// Find section by name.
    fn find_section(&self, name: &[u8]) -> Option<&Section> {
        self.sections.iter().find(|&sec| bytes_eq(sec.name_bytes(), name)).map(|v| v as _)
    }

    /// Get bytes at a given virtual address for a given length.
    fn bytes_at_vaddr(&self, vaddr: u64, len: usize) -> Option<&[u8]> {
        // Search segments for one containing this address
        for seg in &self.segments {
            if seg.p_type == PT_LOAD
                && vaddr >= seg.p_vaddr
                && vaddr < seg.p_vaddr + seg.p_filesz
            {
                let offset_in_seg = (vaddr - seg.p_vaddr) as usize;
                let file_offset = seg.p_offset as usize + offset_in_seg;
                let avail = (seg.p_filesz as usize).saturating_sub(offset_in_seg);
                let read_len = len.min(avail);
                if file_offset + read_len <= self.data.len() {
                    return Some(&self.data[file_offset..file_offset + read_len]);
                }
            }
        }
        // Fallback: search sections
        for sec in &self.sections {
            if sec.sh_type == SHT_PROGBITS
                && vaddr >= sec.addr
                && vaddr < sec.addr + sec.size
            {
                let offset_in_sec = (vaddr - sec.addr) as usize;
                let file_offset = sec.offset as usize + offset_in_sec;
                let avail = (sec.size as usize).saturating_sub(offset_in_sec);
                let read_len = len.min(avail);
                if file_offset + read_len <= self.data.len() {
                    return Some(&self.data[file_offset..file_offset + read_len]);
                }
            }
        }
        None
    }
}

/// Parse ELF64 from raw data.
fn parse_elf(data: &[u8]) -> Result<ElfInfo, &'static [u8]> {
    if data.len() < EI_NIDENT + 48 {
        return Err(b"File too small for ELF header");
    }

    // Check magic
    if data[0] != ELFMAG[0] || data[1] != ELFMAG[1] || data[2] != ELFMAG[2] || data[3] != ELFMAG[3] {
        return Err(b"Not an ELF file");
    }

    // Check 64-bit and little-endian
    if data[EI_CLASS] != ELFCLASS64 {
        return Err(b"Not a 64-bit ELF file");
    }
    if data[EI_DATA] != ELFDATA2LSB {
        return Err(b"Not little-endian ELF");
    }

    let e_type = read_u16_le(data, EI_NIDENT).ok_or(b"Truncated header" as &[u8])?;
    let e_machine = read_u16_le(data, EI_NIDENT + 2).ok_or(b"Truncated header" as &[u8])?;
    let e_entry = read_u64_le(data, EI_NIDENT + 8).ok_or(b"Truncated header" as &[u8])?;
    let e_phoff = read_u64_le(data, EI_NIDENT + 16).ok_or(b"Truncated header" as &[u8])?;
    let e_shoff = read_u64_le(data, EI_NIDENT + 24).ok_or(b"Truncated header" as &[u8])?;
    let e_phentsize = read_u16_le(data, EI_NIDENT + 38).ok_or(b"Truncated header" as &[u8])?;
    let e_phnum = read_u16_le(data, EI_NIDENT + 40).ok_or(b"Truncated header" as &[u8])?;
    let e_shentsize = read_u16_le(data, EI_NIDENT + 42).ok_or(b"Truncated header" as &[u8])?;
    let e_shnum = read_u16_le(data, EI_NIDENT + 44).ok_or(b"Truncated header" as &[u8])?;
    let e_shstrndx = read_u16_le(data, EI_NIDENT + 46).ok_or(b"Truncated header" as &[u8])?;

    let header = Elf64Header {
        e_type,
        e_machine,
        e_entry,
        e_phoff,
        e_shoff,
        e_phentsize,
        e_phnum,
        e_shentsize,
        e_shnum,
        e_shstrndx,
    };

    let mut info = ElfInfo::new();
    info.header = header.clone();
    info.entry_point = e_entry;

    // Parse program headers
    let ph_off = e_phoff as usize;
    let ph_ent = e_phentsize as usize;
    for i in 0..(e_phnum as usize).min(MAX_SEGMENTS) {
        let base = ph_off + i * ph_ent;
        if base + 56 > data.len() {
            break;
        }
        let phdr = Elf64Phdr {
            p_type: read_u32_le(data, base).unwrap_or(0),
            p_flags: read_u32_le(data, base + 4).unwrap_or(0),
            p_offset: read_u64_le(data, base + 8).unwrap_or(0),
            p_vaddr: read_u64_le(data, base + 16).unwrap_or(0),
            p_paddr: read_u64_le(data, base + 24).unwrap_or(0),
            p_filesz: read_u64_le(data, base + 32).unwrap_or(0),
            p_memsz: read_u64_le(data, base + 40).unwrap_or(0),
            p_align: read_u64_le(data, base + 48).unwrap_or(0),
        };
        info.segments.push(phdr);
    }

    // Parse section headers
    let sh_off = e_shoff as usize;
    let sh_ent = e_shentsize as usize;
    if sh_ent < 64 || sh_off == 0 {
        info.data = data.to_vec();
        return Ok(info);
    }

    // Read shstrtab for section names
    let shstr_base = sh_off + (e_shstrndx as usize) * sh_ent;
    let shstr_offset = if shstr_base + 40 <= data.len() {
        read_u64_le(data, shstr_base + 24).unwrap_or(0) as usize
    } else {
        0
    };
    let shstr_size = if shstr_base + 40 <= data.len() {
        read_u64_le(data, shstr_base + 32).unwrap_or(0) as usize
    } else {
        0
    };

    // Parse each section header
    let mut symtab_idx: Option<usize> = None;
    let mut dynsym_idx: Option<usize> = None;

    for i in 0..(e_shnum as usize).min(MAX_SECTIONS) {
        let base = sh_off + i * sh_ent;
        if base + 64 > data.len() {
            break;
        }

        let shdr = Elf64Shdr {
            sh_name: read_u32_le(data, base).unwrap_or(0),
            sh_type: read_u32_le(data, base + 4).unwrap_or(0),
            sh_flags: read_u64_le(data, base + 8).unwrap_or(0),
            sh_addr: read_u64_le(data, base + 16).unwrap_or(0),
            sh_offset: read_u64_le(data, base + 24).unwrap_or(0),
            sh_size: read_u64_le(data, base + 32).unwrap_or(0),
            sh_link: read_u32_le(data, base + 40).unwrap_or(0),
            sh_info: read_u32_le(data, base + 44).unwrap_or(0),
            sh_addralign: read_u64_le(data, base + 48).unwrap_or(0),
            sh_entsize: read_u64_le(data, base + 56).unwrap_or(0),
        };

        // Extract section name from shstrtab
        let mut sec = Section {
            name: [0u8; 64],
            name_len: 0,
            sh_type: shdr.sh_type,
            addr: shdr.sh_addr,
            offset: shdr.sh_offset,
            size: shdr.sh_size,
            flags: shdr.sh_flags,
        };

        let name_off = shdr.sh_name as usize;
        if shstr_offset > 0 && name_off < shstr_size {
            let name_start = shstr_offset + name_off;
            let mut nlen = 0;
            while name_start + nlen < data.len()
                && nlen < 63
                && data[name_start + nlen] != 0
            {
                sec.name[nlen] = data[name_start + nlen];
                nlen += 1;
            }
            sec.name_len = nlen;
        }

        if shdr.sh_type == SHT_SYMTAB {
            symtab_idx = Some(i);
        } else if shdr.sh_type == SHT_DYNSYM && symtab_idx.is_none() {
            dynsym_idx = Some(i);
        }

        info.sections.push(sec);
    }

    // Parse symbol table (prefer .symtab over .dynsym)
    let sym_section_idx = symtab_idx.or(dynsym_idx);
    if let Some(idx) = sym_section_idx {
        let sym_base = sh_off + idx * sh_ent;
        if sym_base + 64 <= data.len() {
            let sym_offset = read_u64_le(data, sym_base + 24).unwrap_or(0) as usize;
            let sym_size = read_u64_le(data, sym_base + 32).unwrap_or(0) as usize;
            let sym_entsize = read_u64_le(data, sym_base + 56).unwrap_or(24) as usize;
            let sym_link = read_u32_le(data, sym_base + 40).unwrap_or(0) as usize;

            // Get linked string table
            let strtab_base = sh_off + sym_link * sh_ent;
            let strtab_offset = if strtab_base + 40 <= data.len() {
                read_u64_le(data, strtab_base + 24).unwrap_or(0) as usize
            } else {
                0
            };
            let strtab_size = if strtab_base + 40 <= data.len() {
                read_u64_le(data, strtab_base + 32).unwrap_or(0) as usize
            } else {
                0
            };

            if sym_entsize >= 24 {
                let num_syms = sym_size / sym_entsize;
                for si in 0..num_syms.min(MAX_SYMBOLS) {
                    let sbase = sym_offset + si * sym_entsize;
                    if sbase + 24 > data.len() {
                        break;
                    }

                    let elf_sym = Elf64Sym {
                        name_offset: read_u32_le(data, sbase).unwrap_or(0),
                        info: if sbase + 4 < data.len() { data[sbase + 4] } else { 0 },
                        other: if sbase + 5 < data.len() { data[sbase + 5] } else { 0 },
                        shndx: read_u16_le(data, sbase + 6).unwrap_or(0),
                        value: read_u64_le(data, sbase + 8).unwrap_or(0),
                        size: read_u64_le(data, sbase + 16).unwrap_or(0),
                    };

                    let mut sym = Symbol {
                        name: [0u8; 128],
                        name_len: 0,
                        value: elf_sym.value,
                        size: elf_sym.size,
                        sym_type: elf_sym.info & 0xf,
                        section: elf_sym.shndx,
                    };

                    // Extract name from string table
                    let noff = elf_sym.name_offset as usize;
                    if strtab_offset > 0 && noff < strtab_size {
                        let nstart = strtab_offset + noff;
                        let mut nlen = 0;
                        while nstart + nlen < data.len()
                            && nlen < 127
                            && data[nstart + nlen] != 0
                        {
                            sym.name[nlen] = data[nstart + nlen];
                            nlen += 1;
                        }
                        sym.name_len = nlen;
                    }

                    // Skip unnamed symbols and section/file symbols
                    if sym.name_len > 0 && (elf_sym.info & 0xf) != 3 && (elf_sym.info & 0xf) != 4 {
                        info.symbols.push(sym);
                    }
                }
            }
        }
    }

    info.data = data.to_vec();
    Ok(info)
}

// ============================================================================
// Breakpoint Management
// ============================================================================

/// Breakpoint types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BreakpointKind {
    Software,
    Hardware,
}

/// A breakpoint entry.
#[derive(Clone)]
struct Breakpoint {
    id: u32,
    address: u64,
    kind: BreakpointKind,
    enabled: bool,
    hit_count: u64,
    original_byte: u8,
    /// Symbol or location description
    location: [u8; 128],
    location_len: usize,
    /// Condition expression (if any)
    condition: [u8; 128],
    condition_len: usize,
}

impl Breakpoint {
    fn new(id: u32, addr: u64) -> Self {
        Self {
            id,
            address: addr,
            kind: BreakpointKind::Software,
            enabled: true,
            hit_count: 0,
            original_byte: 0,
            location: [0u8; 128],
            location_len: 0,
            condition: [0u8; 128],
            condition_len: 0,
        }
    }

    fn location_bytes(&self) -> &[u8] {
        &self.location[..self.location_len]
    }
}

// ============================================================================
// Watchpoint Management
// ============================================================================

/// Watchpoint access types.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WatchKind {
    Write,
    Read,
    ReadWrite,
}

/// A watchpoint entry.
#[derive(Clone)]
struct Watchpoint {
    id: u32,
    address: u64,
    size: usize,
    kind: WatchKind,
    enabled: bool,
    hit_count: u64,
    /// Expression that defines this watchpoint
    expr: [u8; 128],
    expr_len: usize,
    /// Last known value at the watched address
    last_value: u64,
}

impl Watchpoint {
    fn new(id: u32, addr: u64, size: usize) -> Self {
        Self {
            id,
            address: addr,
            size,
            kind: WatchKind::Write,
            enabled: true,
            hit_count: 0,
            expr: [0u8; 128],
            expr_len: 0,
            last_value: 0,
        }
    }
}

// ============================================================================
// Thread tracking
// ============================================================================

/// Thread state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThreadState {
    Running,
    Stopped,
    Exited,
}

/// Thread information.
#[derive(Clone)]
struct ThreadInfo {
    tid: u64,
    state: ThreadState,
    name: [u8; 64],
    name_len: usize,
    regs: [u64; REG_COUNT],
}

impl ThreadInfo {
    fn new(tid: u64) -> Self {
        Self {
            tid,
            state: ThreadState::Stopped,
            name: [0u8; 64],
            name_len: 0,
            regs: [0u64; REG_COUNT],
        }
    }
}

// ============================================================================
// Stack frame for backtrace
// ============================================================================

/// A single stack frame.
#[derive(Clone)]
struct StackFrame {
    frame_num: u32,
    rip: u64,
    rbp: u64,
    rsp: u64,
}

// ============================================================================
// Debugger variables
// ============================================================================

/// A user-defined debugger variable.
#[derive(Clone)]
struct DebugVar {
    name: [u8; 64],
    name_len: usize,
    value: i64,
}

const MAX_DEBUG_VARS: usize = 128;

// ============================================================================
// Expression evaluator
// ============================================================================

/// Tokens for the expression evaluator.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExprToken {
    Number(i64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    ShiftLeft,
    ShiftRight,
    LParen,
    RParen,
    End,
}

/// Tokenize an expression string.
fn tokenize_expr(input: &[u8]) -> Vec<ExprToken> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b' ' | b'\t' => { i += 1; }
            b'+' => { tokens.push(ExprToken::Plus); i += 1; }
            b'-' => { tokens.push(ExprToken::Minus); i += 1; }
            b'*' => { tokens.push(ExprToken::Star); i += 1; }
            b'/' => { tokens.push(ExprToken::Slash); i += 1; }
            b'%' => { tokens.push(ExprToken::Percent); i += 1; }
            b'&' => { tokens.push(ExprToken::Ampersand); i += 1; }
            b'|' => { tokens.push(ExprToken::Pipe); i += 1; }
            b'^' => { tokens.push(ExprToken::Caret); i += 1; }
            b'~' => { tokens.push(ExprToken::Tilde); i += 1; }
            b'(' => { tokens.push(ExprToken::LParen); i += 1; }
            b')' => { tokens.push(ExprToken::RParen); i += 1; }
            b'<' if i + 1 < input.len() && input[i + 1] == b'<' => {
                tokens.push(ExprToken::ShiftLeft);
                i += 2;
            }
            b'>' if i + 1 < input.len() && input[i + 1] == b'>' => {
                tokens.push(ExprToken::ShiftRight);
                i += 2;
            }
            b'0'..=b'9' => {
                // Parse number (decimal or hex with 0x prefix)
                let start = i;
                if input[i] == b'0' && i + 1 < input.len()
                    && (input[i + 1] == b'x' || input[i + 1] == b'X')
                {
                    i += 2;
                    while i < input.len() && is_hex_digit(input[i]) {
                        i += 1;
                    }
                } else {
                    while i < input.len() && input[i] >= b'0' && input[i] <= b'9' {
                        i += 1;
                    }
                }
                if let Some(val) = parse_number(&input[start..i]) {
                    tokens.push(ExprToken::Number(val as i64));
                }
            }
            b'$' => {
                // Register name like $rax
                i += 1;
                let start = i;
                while i < input.len() && (input[i].is_ascii_alphanumeric() || input[i] == b'_') {
                    i += 1;
                }
                let reg_name = &input[start..i];
                if let Some(idx) = find_register_index(reg_name) {
                    // Placeholder: register values will be resolved by the caller
                    tokens.push(ExprToken::Number(idx as i64));
                } else {
                    tokens.push(ExprToken::Number(0));
                }
            }
            _ => { i += 1; } // Skip unknown characters
        }
    }
    tokens.push(ExprToken::End);
    tokens
}

fn is_hex_digit(ch: u8) -> bool {
    ch.is_ascii_hexdigit()
}

/// Find the index of a register by name.
fn find_register_index(name: &[u8]) -> Option<usize> {
    (0..REG_COUNT).find(|&i| bytes_eq(name, REG_NAMES[i]))
}

/// Recursive descent expression evaluator.
struct ExprParser {
    tokens: Vec<ExprToken>,
    pos: usize,
}

impl ExprParser {
    fn new(tokens: Vec<ExprToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> ExprToken {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos]
        } else {
            ExprToken::End
        }
    }

    fn advance(&mut self) -> ExprToken {
        let tok = self.peek();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    /// Parse expression: handles bitwise OR (lowest precedence).
    fn parse_expr(&mut self) -> Result<i64, &'static [u8]> {
        self.parse_bitor()
    }

    fn parse_bitor(&mut self) -> Result<i64, &'static [u8]> {
        let mut left = self.parse_bitxor()?;
        while self.peek() == ExprToken::Pipe {
            self.advance();
            let right = self.parse_bitxor()?;
            left |= right;
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> Result<i64, &'static [u8]> {
        let mut left = self.parse_bitand()?;
        while self.peek() == ExprToken::Caret {
            self.advance();
            let right = self.parse_bitand()?;
            left ^= right;
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> Result<i64, &'static [u8]> {
        let mut left = self.parse_shift()?;
        while self.peek() == ExprToken::Ampersand {
            self.advance();
            let right = self.parse_shift()?;
            left &= right;
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<i64, &'static [u8]> {
        let mut left = self.parse_additive()?;
        loop {
            match self.peek() {
                ExprToken::ShiftLeft => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = left.wrapping_shl(right as u32);
                }
                ExprToken::ShiftRight => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = left.wrapping_shr(right as u32);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<i64, &'static [u8]> {
        let mut left = self.parse_multiplicative()?;
        loop {
            match self.peek() {
                ExprToken::Plus => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = left.wrapping_add(right);
                }
                ExprToken::Minus => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = left.wrapping_sub(right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<i64, &'static [u8]> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                ExprToken::Star => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = left.wrapping_mul(right);
                }
                ExprToken::Slash => {
                    self.advance();
                    let right = self.parse_unary()?;
                    if right == 0 {
                        return Err(b"Division by zero");
                    }
                    left = left.wrapping_div(right);
                }
                ExprToken::Percent => {
                    self.advance();
                    let right = self.parse_unary()?;
                    if right == 0 {
                        return Err(b"Modulo by zero");
                    }
                    left = left.wrapping_rem(right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<i64, &'static [u8]> {
        match self.peek() {
            ExprToken::Minus => {
                self.advance();
                let val = self.parse_unary()?;
                Ok(val.wrapping_neg())
            }
            ExprToken::Tilde => {
                self.advance();
                let val = self.parse_unary()?;
                Ok(!val)
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<i64, &'static [u8]> {
        match self.peek() {
            ExprToken::Number(n) => {
                self.advance();
                Ok(n)
            }
            ExprToken::LParen => {
                self.advance();
                let val = self.parse_expr()?;
                if self.peek() == ExprToken::RParen {
                    self.advance();
                }
                Ok(val)
            }
            _ => Err(b"Unexpected token in expression"),
        }
    }
}

/// Evaluate an expression string, resolving register references against
/// the given register file.
fn eval_expr(input: &[u8], regs: &[u64; REG_COUNT]) -> Result<i64, &'static [u8]> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b' ' | b'\t' => { i += 1; }
            b'+' => { tokens.push(ExprToken::Plus); i += 1; }
            b'-' => { tokens.push(ExprToken::Minus); i += 1; }
            b'*' => { tokens.push(ExprToken::Star); i += 1; }
            b'/' => { tokens.push(ExprToken::Slash); i += 1; }
            b'%' => { tokens.push(ExprToken::Percent); i += 1; }
            b'&' => { tokens.push(ExprToken::Ampersand); i += 1; }
            b'|' => { tokens.push(ExprToken::Pipe); i += 1; }
            b'^' => { tokens.push(ExprToken::Caret); i += 1; }
            b'~' => { tokens.push(ExprToken::Tilde); i += 1; }
            b'(' => { tokens.push(ExprToken::LParen); i += 1; }
            b')' => { tokens.push(ExprToken::RParen); i += 1; }
            b'<' if i + 1 < input.len() && input[i + 1] == b'<' => {
                tokens.push(ExprToken::ShiftLeft);
                i += 2;
            }
            b'>' if i + 1 < input.len() && input[i + 1] == b'>' => {
                tokens.push(ExprToken::ShiftRight);
                i += 2;
            }
            b'0'..=b'9' => {
                let start = i;
                if input[i] == b'0' && i + 1 < input.len()
                    && (input[i + 1] == b'x' || input[i + 1] == b'X')
                {
                    i += 2;
                    while i < input.len() && is_hex_digit(input[i]) {
                        i += 1;
                    }
                } else {
                    while i < input.len() && input[i] >= b'0' && input[i] <= b'9' {
                        i += 1;
                    }
                }
                if let Some(val) = parse_number(&input[start..i]) {
                    tokens.push(ExprToken::Number(val as i64));
                }
            }
            b'$' => {
                i += 1;
                let start = i;
                while i < input.len() && (input[i].is_ascii_alphanumeric() || input[i] == b'_') {
                    i += 1;
                }
                let reg_name = &input[start..i];
                if let Some(idx) = find_register_index(reg_name) {
                    tokens.push(ExprToken::Number(regs[idx] as i64));
                } else {
                    tokens.push(ExprToken::Number(0));
                }
            }
            _ => { i += 1; }
        }
    }
    tokens.push(ExprToken::End);

    let mut parser = ExprParser::new(tokens);
    parser.parse_expr()
}

// ============================================================================
// x86_64 Instruction Disassembly (simplified)
// ============================================================================

/// Disassembled instruction.
struct DisasmInst {
    /// Instruction length in bytes.
    len: usize,
    /// Mnemonic text.
    text: [u8; 64],
    text_len: usize,
}

/// Simple x86_64 disassembler covering common instructions.
fn disasm_one(code: &[u8], addr: u64) -> DisasmInst {
    if code.is_empty() {
        return DisasmInst { len: 0, text: [0u8; 64], text_len: 0 };
    }

    let mut inst = DisasmInst { len: 1, text: [0u8; 64], text_len: 0 };

    // REX prefix detection
    let mut has_rex_w = false;
    let mut idx = 0;
    if code.len() > idx && (code[idx] & 0xf0) == 0x40 {
        has_rex_w = (code[idx] & 0x08) != 0;
        idx += 1;
    }

    if idx >= code.len() {
        write_inst(&mut inst, b"(bad)");
        return inst;
    }

    let opcode = code[idx];
    idx += 1;

    match opcode {
        0x00..=0x05 => { write_inst(&mut inst, b"add"); inst.len = idx; }
        0x08..=0x0d => { write_inst(&mut inst, b"or"); inst.len = idx; }
        0x10..=0x15 => { write_inst(&mut inst, b"adc"); inst.len = idx; }
        0x18..=0x1d => { write_inst(&mut inst, b"sbb"); inst.len = idx; }
        0x20..=0x25 => { write_inst(&mut inst, b"and"); inst.len = idx; }
        0x28..=0x2d => { write_inst(&mut inst, b"sub"); inst.len = idx; }
        0x30..=0x35 => { write_inst(&mut inst, b"xor"); inst.len = idx; }
        0x38..=0x3d => { write_inst(&mut inst, b"cmp"); inst.len = idx; }
        0x50..=0x57 => {
            let reg = (opcode - 0x50) as usize;
            write_inst_reg(&mut inst, b"push", reg, has_rex_w);
            inst.len = idx;
        }
        0x58..=0x5f => {
            let reg = (opcode - 0x58) as usize;
            write_inst_reg(&mut inst, b"pop", reg, has_rex_w);
            inst.len = idx;
        }
        0x70 => { write_inst_branch(&mut inst, b"jo", code, idx, addr); }
        0x71 => { write_inst_branch(&mut inst, b"jno", code, idx, addr); }
        0x72 => { write_inst_branch(&mut inst, b"jb", code, idx, addr); }
        0x73 => { write_inst_branch(&mut inst, b"jae", code, idx, addr); }
        0x74 => { write_inst_branch(&mut inst, b"je", code, idx, addr); }
        0x75 => { write_inst_branch(&mut inst, b"jne", code, idx, addr); }
        0x76 => { write_inst_branch(&mut inst, b"jbe", code, idx, addr); }
        0x77 => { write_inst_branch(&mut inst, b"ja", code, idx, addr); }
        0x78 => { write_inst_branch(&mut inst, b"js", code, idx, addr); }
        0x79 => { write_inst_branch(&mut inst, b"jns", code, idx, addr); }
        0x7e => { write_inst_branch(&mut inst, b"jle", code, idx, addr); }
        0x7f => { write_inst_branch(&mut inst, b"jg", code, idx, addr); }
        0x89 => { write_inst(&mut inst, b"mov"); inst.len = idx + modrm_len(code, idx); }
        0x8b => { write_inst(&mut inst, b"mov"); inst.len = idx + modrm_len(code, idx); }
        0x8d => { write_inst(&mut inst, b"lea"); inst.len = idx + modrm_len(code, idx); }
        0x90 => { write_inst(&mut inst, b"nop"); inst.len = idx; }
        0xb0..=0xb7 => { write_inst(&mut inst, b"mov"); inst.len = idx + 1; }
        0xb8..=0xbf => {
            if has_rex_w {
                write_inst(&mut inst, b"movabs");
                inst.len = idx + 8;
            } else {
                write_inst(&mut inst, b"mov");
                inst.len = idx + 4;
            }
        }
        0xc3 => { write_inst(&mut inst, b"ret"); inst.len = idx; }
        0xc9 => { write_inst(&mut inst, b"leave"); inst.len = idx; }
        0xcc => { write_inst(&mut inst, b"int3"); inst.len = idx; }
        0xcd => { write_inst(&mut inst, b"int"); inst.len = idx + 1; }
        0xe8 => {
            // call rel32
            if idx + 4 <= code.len() {
                let rel = i32::from_le_bytes([
                    code[idx], code[idx + 1], code[idx + 2], code[idx + 3],
                ]);
                let target = addr.wrapping_add((idx + 4) as u64).wrapping_add(rel as u64);
                write_inst_addr(&mut inst, b"call", target);
                inst.len = idx + 4;
            } else {
                write_inst(&mut inst, b"call");
                inst.len = idx;
            }
        }
        0xe9 => {
            // jmp rel32
            if idx + 4 <= code.len() {
                let rel = i32::from_le_bytes([
                    code[idx], code[idx + 1], code[idx + 2], code[idx + 3],
                ]);
                let target = addr.wrapping_add((idx + 4) as u64).wrapping_add(rel as u64);
                write_inst_addr(&mut inst, b"jmp", target);
                inst.len = idx + 4;
            } else {
                write_inst(&mut inst, b"jmp");
                inst.len = idx;
            }
        }
        0xeb => {
            // jmp rel8
            if idx < code.len() {
                let rel = code[idx] as i8;
                let target = addr.wrapping_add((idx + 1) as u64).wrapping_add(rel as u64);
                write_inst_addr(&mut inst, b"jmp", target);
                inst.len = idx + 1;
            } else {
                write_inst(&mut inst, b"jmp");
                inst.len = idx;
            }
        }
        0xf4 => { write_inst(&mut inst, b"hlt"); inst.len = idx; }
        0xf7 => { write_inst(&mut inst, b"test/not/neg/mul/div"); inst.len = idx + modrm_len(code, idx); }
        0xff => {
            if idx < code.len() {
                let modrm = code[idx];
                let reg_field = (modrm >> 3) & 7;
                match reg_field {
                    0 => write_inst(&mut inst, b"inc"),
                    1 => write_inst(&mut inst, b"dec"),
                    2 => write_inst(&mut inst, b"call"),
                    4 => write_inst(&mut inst, b"jmp"),
                    6 => write_inst(&mut inst, b"push"),
                    _ => write_inst(&mut inst, b"(bad)"),
                }
                inst.len = idx + modrm_len(code, idx);
            } else {
                write_inst(&mut inst, b"(bad)");
                inst.len = idx;
            }
        }
        0x0f => {
            // Two-byte opcodes
            if idx < code.len() {
                let op2 = code[idx];
                idx += 1;
                match op2 {
                    0x05 => { write_inst(&mut inst, b"syscall"); inst.len = idx; }
                    0x1f => { write_inst(&mut inst, b"nop"); inst.len = idx + modrm_len(code, idx); }
                    0x80..=0x8f => {
                        let cc_name = match op2 & 0x0f {
                            0x0 => b"jo" as &[u8],
                            0x1 => b"jno",
                            0x2 => b"jb",
                            0x3 => b"jae",
                            0x4 => b"je",
                            0x5 => b"jne",
                            0x6 => b"jbe",
                            0x7 => b"ja",
                            0x8 => b"js",
                            0x9 => b"jns",
                            0xa => b"jp",
                            0xb => b"jnp",
                            0xc => b"jl",
                            0xd => b"jge",
                            0xe => b"jle",
                            _ => b"jg",
                        };
                        if idx + 4 <= code.len() {
                            let rel = i32::from_le_bytes([
                                code[idx], code[idx + 1], code[idx + 2], code[idx + 3],
                            ]);
                            let target = addr.wrapping_add((idx + 4) as u64).wrapping_add(rel as u64);
                            write_inst_addr(&mut inst, cc_name, target);
                            inst.len = idx + 4;
                        } else {
                            write_inst(&mut inst, cc_name);
                            inst.len = idx;
                        }
                    }
                    0xaf => { write_inst(&mut inst, b"imul"); inst.len = idx + modrm_len(code, idx); }
                    0xb6 => { write_inst(&mut inst, b"movzx"); inst.len = idx + modrm_len(code, idx); }
                    0xb7 => { write_inst(&mut inst, b"movzx"); inst.len = idx + modrm_len(code, idx); }
                    0xbe => { write_inst(&mut inst, b"movsx"); inst.len = idx + modrm_len(code, idx); }
                    0xbf => { write_inst(&mut inst, b"movsx"); inst.len = idx + modrm_len(code, idx); }
                    _ => { write_inst(&mut inst, b"(two-byte)"); inst.len = idx; }
                }
            } else {
                write_inst(&mut inst, b"(bad)");
                inst.len = idx;
            }
        }
        _ => {
            write_inst(&mut inst, b"(unknown)");
            inst.len = idx;
        }
    }

    // Clamp length to available data
    if inst.len > code.len() {
        inst.len = code.len();
    }
    if inst.len == 0 {
        inst.len = 1;
    }

    inst
}

/// Write mnemonic text to instruction.
fn write_inst(inst: &mut DisasmInst, text: &[u8]) {
    let len = text.len().min(63);
    inst.text[..len].copy_from_slice(&text[..len]);
    inst.text_len = len;
}

/// Write mnemonic + register operand.
fn write_inst_reg(inst: &mut DisasmInst, mnemonic: &[u8], reg: usize, is_64: bool) {
    let reg_names_64 = [
        b"rax" as &[u8], b"rcx", b"rdx", b"rbx", b"rsp", b"rbp", b"rsi", b"rdi",
    ];
    let reg_names_32 = [
        b"eax" as &[u8], b"ecx", b"edx", b"ebx", b"esp", b"ebp", b"esi", b"edi",
    ];
    let rname = if is_64 {
        reg_names_64.get(reg).copied().unwrap_or(b"???")
    } else {
        reg_names_32.get(reg).copied().unwrap_or(b"???")
    };
    let mut pos = 0;
    let mn_len = mnemonic.len().min(32);
    inst.text[..mn_len].copy_from_slice(&mnemonic[..mn_len]);
    pos += mn_len;
    if pos < 63 { inst.text[pos] = b' '; pos += 1; }
    let rn_len = rname.len().min(63 - pos);
    inst.text[pos..pos + rn_len].copy_from_slice(&rname[..rn_len]);
    pos += rn_len;
    inst.text_len = pos;
}

/// Write mnemonic + target address.
fn write_inst_addr(inst: &mut DisasmInst, mnemonic: &[u8], target: u64) {
    let mut pos = 0;
    let mn_len = mnemonic.len().min(32);
    inst.text[..mn_len].copy_from_slice(&mnemonic[..mn_len]);
    pos += mn_len;
    if pos < 63 { inst.text[pos] = b' '; pos += 1; }
    let mut hex_buf = [0u8; 20];
    let hex_len = format_hex(target, &mut hex_buf);
    let copy_len = hex_len.min(63 - pos);
    inst.text[pos..pos + copy_len].copy_from_slice(&hex_buf[..copy_len]);
    pos += copy_len;
    inst.text_len = pos;
}

/// Write mnemonic + short branch target.
fn write_inst_branch(inst: &mut DisasmInst, mnemonic: &[u8], code: &[u8], idx: usize, addr: u64) {
    if idx < code.len() {
        let rel = code[idx] as i8;
        let target = addr.wrapping_add((idx + 1) as u64).wrapping_add(rel as u64);
        write_inst_addr(inst, mnemonic, target);
        inst.len = idx + 1;
    } else {
        write_inst(inst, mnemonic);
        inst.len = idx;
    }
}

/// Compute ModR/M length (ModR/M byte + SIB + displacement).
fn modrm_len(code: &[u8], idx: usize) -> usize {
    if idx >= code.len() {
        return 1;
    }
    let modrm = code[idx];
    let mod_field = modrm >> 6;
    let rm = modrm & 7;
    let mut len = 1; // ModR/M byte itself

    match mod_field {
        0 => {
            if rm == 4 { len += 1; } // SIB byte
            if rm == 5 { len += 4; } // disp32 (RIP-relative)
            // SIB with base=5 also adds disp32
            if rm == 4 && idx + 1 < code.len() && (code[idx + 1] & 7) == 5 {
                len += 4;
            }
        }
        1 => {
            if rm == 4 { len += 1; } // SIB
            len += 1; // disp8
        }
        2 => {
            if rm == 4 { len += 1; } // SIB
            len += 4; // disp32
        }
        3 => { /* register direct, no extra bytes */ }
        _ => {}
    }
    len
}

// ============================================================================
// Memory Examination
// ============================================================================

/// Format specifier for the `x` command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExamineFormat {
    Byte,       // b - single byte
    Halfword,   // h - 2 bytes
    Word,       // w - 4 bytes
    Giant,      // g - 8 bytes
    StringZ,    // s - null-terminated string
    Instruction,// i - disassemble
}

/// Parse the x/FMT string. Returns (count, format).
fn parse_examine_fmt(fmt: &[u8]) -> (usize, ExamineFormat) {
    let mut count: usize = 0;
    let mut format = ExamineFormat::Word;
    let mut found_digit = false;

    let mut i = 0;
    // Parse optional count
    while i < fmt.len() && fmt[i] >= b'0' && fmt[i] <= b'9' {
        count = count.saturating_mul(10).saturating_add((fmt[i] - b'0') as usize);
        found_digit = true;
        i += 1;
    }
    if !found_digit || count == 0 { count = 1; }

    // Parse format character
    while i < fmt.len() {
        match fmt[i] {
            b'b' => { format = ExamineFormat::Byte; break; }
            b'h' => { format = ExamineFormat::Halfword; break; }
            b'w' => { format = ExamineFormat::Word; break; }
            b'g' => { format = ExamineFormat::Giant; break; }
            b's' => { format = ExamineFormat::StringZ; break; }
            b'i' => { format = ExamineFormat::Instruction; break; }
            b'x' | b'd' | b'o' | b't' | b'a' | b'c' | b'f' => { i += 1; continue; } // Skip display format letters
            _ => break,
        }
    }

    (count, format)
}

/// Format memory examination output.
fn format_examine(
    out: &mut dyn Write,
    data: &[u8],
    addr: u64,
    count: usize,
    format: ExamineFormat,
) {
    let mut hex_buf = [0u8; 20];
    let mut offset = 0usize;

    for item in 0..count {
        if offset >= data.len() {
            break;
        }

        let current_addr = addr + offset as u64;

        // Print address at start of each line (every 16 bytes for byte mode, each for others)
        let print_addr = match format {
            ExamineFormat::Byte => item % 16 == 0,
            ExamineFormat::Halfword => item % 8 == 0,
            _ => true,
        };

        if print_addr {
            if item > 0 && matches!(format, ExamineFormat::Byte | ExamineFormat::Halfword) {
                let _ = out.write_all(b"\n");
            }
            let n = format_hex_padded(current_addr, &mut hex_buf, 16);
            let _ = out.write_all(&hex_buf[..n]);
            let _ = out.write_all(b":\t");
        }

        match format {
            ExamineFormat::Byte => {
                if offset < data.len() {
                    let n = format_hex(data[offset] as u64, &mut hex_buf);
                    let _ = out.write_all(&hex_buf[..n]);
                    let _ = out.write_all(b" ");
                    offset += 1;
                }
            }
            ExamineFormat::Halfword => {
                if offset + 2 <= data.len() {
                    let val = u16::from_le_bytes([data[offset], data[offset + 1]]);
                    let n = format_hex(val as u64, &mut hex_buf);
                    let _ = out.write_all(&hex_buf[..n]);
                    let _ = out.write_all(b" ");
                    offset += 2;
                } else {
                    break;
                }
            }
            ExamineFormat::Word => {
                if offset + 4 <= data.len() {
                    let val = u32::from_le_bytes([
                        data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                    ]);
                    let n = format_hex(val as u64, &mut hex_buf);
                    let _ = out.write_all(&hex_buf[..n]);
                    let _ = out.write_all(b"\n");
                    offset += 4;
                } else {
                    break;
                }
            }
            ExamineFormat::Giant => {
                if offset + 8 <= data.len() {
                    let val = u64::from_le_bytes([
                        data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                        data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
                    ]);
                    let n = format_hex(val, &mut hex_buf);
                    let _ = out.write_all(&hex_buf[..n]);
                    let _ = out.write_all(b"\n");
                    offset += 8;
                } else {
                    break;
                }
            }
            ExamineFormat::StringZ => {
                let start = offset;
                while offset < data.len() && data[offset] != 0 {
                    offset += 1;
                }
                let _ = out.write_all(b"\"");
                let _ = out.write_all(&data[start..offset]);
                let _ = out.write_all(b"\"\n");
                if offset < data.len() {
                    offset += 1; // skip null
                }
            }
            ExamineFormat::Instruction => {
                let remaining = &data[offset..];
                let inst = disasm_one(remaining, current_addr);
                // Print hex bytes
                for bi in 0..inst.len.min(remaining.len()) {
                    let n = format_hex(remaining[bi] as u64, &mut hex_buf);
                    // Just print the hex digits without 0x prefix for compactness
                    if remaining[bi] < 16 {
                        let _ = out.write_all(b"0");
                    }
                    let _ = out.write_all(&hex_buf[2..n]);
                    let _ = out.write_all(b" ");
                }
                let _ = out.write_all(b"\t");
                let _ = out.write_all(&inst.text[..inst.text_len]);
                let _ = out.write_all(b"\n");
                offset += inst.len;
            }
        }
    }

    // Final newline for byte/halfword mode
    if matches!(format, ExamineFormat::Byte | ExamineFormat::Halfword) {
        let _ = out.write_all(b"\n");
    }
}

// ============================================================================
// GDB Remote Protocol
// ============================================================================

/// Compute GDB remote protocol checksum.
fn gdb_checksum(data: &[u8]) -> u8 {
    let mut sum: u8 = 0;
    for &b in data {
        sum = sum.wrapping_add(b);
    }
    sum
}

/// Build a GDB remote protocol packet: $<data>#<checksum>
fn gdb_packet(data: &[u8], buf: &mut [u8]) -> usize {
    if buf.len() < data.len() + 4 {
        return 0;
    }
    buf[0] = b'$';
    buf[1..1 + data.len()].copy_from_slice(data);
    let csum = gdb_checksum(data);
    buf[1 + data.len()] = b'#';
    let hex_chars = b"0123456789abcdef";
    buf[2 + data.len()] = hex_chars[(csum >> 4) as usize];
    buf[3 + data.len()] = hex_chars[(csum & 0xf) as usize];
    4 + data.len()
}

/// Parse a GDB remote protocol packet from a buffer.
/// Returns the payload and how many bytes were consumed.
fn gdb_parse_packet(buf: &[u8]) -> Option<(&[u8], usize)> {
    // Find '$'
    let mut start = 0;
    while start < buf.len() && buf[start] != b'$' {
        start += 1;
    }
    if start >= buf.len() {
        return None;
    }
    start += 1; // skip '$'

    // Find '#'
    let mut end = start;
    while end < buf.len() && buf[end] != b'#' {
        end += 1;
    }
    if end + 2 >= buf.len() {
        return None;
    }

    let payload = &buf[start..end];
    let consumed = end + 3; // '#' + 2 checksum chars

    Some((payload, consumed))
}

/// Encode register values as hex for GDB 'g' response.
fn gdb_encode_registers(regs: &[u64; REG_COUNT], buf: &mut [u8]) -> usize {
    let hex_chars = b"0123456789abcdef";
    let mut pos = 0;

    // GDB expects registers in a specific order for x86_64:
    // rax, rbx, rcx, rdx, rsi, rdi, rbp, rsp, r8-r15, rip, rflags, cs, ss, ds, es, fs, gs
    let order = [
        REG_RAX, REG_RBX, REG_RCX, REG_RDX, REG_RSI, REG_RDI,
        REG_RBP, REG_RSP, REG_R8, REG_R9, REG_R10, REG_R11,
        REG_R12, REG_R13, REG_R14, REG_R15, REG_RIP, REG_RFLAGS,
        REG_CS, REG_SS, REG_DS, REG_ES, REG_FS, REG_GS,
    ];

    for &reg_idx in &order {
        let val = regs[reg_idx];
        // Encode as 16 hex chars (8 bytes, little-endian within each byte)
        for byte_idx in 0..8 {
            let byte = ((val >> (byte_idx * 8)) & 0xff) as u8;
            if pos + 2 > buf.len() {
                return pos;
            }
            buf[pos] = hex_chars[(byte >> 4) as usize];
            buf[pos + 1] = hex_chars[(byte & 0xf) as usize];
            pos += 2;
        }
    }
    pos
}

/// Handle a single GDB remote protocol command.
fn gdb_handle_command(
    payload: &[u8],
    regs: &[u64; REG_COUNT],
    elf: &Option<ElfInfo>,
    response: &mut [u8],
) -> usize {
    if payload.is_empty() {
        return gdb_packet(b"", response);
    }

    match payload[0] {
        b'?' => {
            // Halt reason: signal 5 (SIGTRAP)
            gdb_packet(b"S05", response)
        }
        b'g' => {
            // Read registers
            let mut reg_buf = [0u8; 512];
            let reg_len = gdb_encode_registers(regs, &mut reg_buf);
            gdb_packet(&reg_buf[..reg_len], response)
        }
        b'G' => {
            // Write registers (stub: acknowledge but don't change)
            gdb_packet(b"OK", response)
        }
        b'm' => {
            // Read memory: m<addr>,<length>
            let rest = &payload[1..];
            let mut comma_pos = 0;
            while comma_pos < rest.len() && rest[comma_pos] != b',' {
                comma_pos += 1;
            }
            if comma_pos < rest.len() {
                let addr = parse_hex(&rest[..comma_pos]).unwrap_or(0);
                let length = parse_hex(&rest[comma_pos + 1..]).unwrap_or(0) as usize;

                if let Some(elf_info) = elf {
                    if let Some(mem_data) = elf_info.bytes_at_vaddr(addr, length) {
                        let hex_chars = b"0123456789abcdef";
                        let mut mem_hex = vec![0u8; mem_data.len() * 2];
                        for (i, &byte) in mem_data.iter().enumerate() {
                            mem_hex[i * 2] = hex_chars[(byte >> 4) as usize];
                            mem_hex[i * 2 + 1] = hex_chars[(byte & 0xf) as usize];
                        }
                        return gdb_packet(&mem_hex, response);
                    }
                }
                gdb_packet(b"E01", response)
            } else {
                gdb_packet(b"E01", response)
            }
        }
        b'M' => {
            // Write memory (stub)
            gdb_packet(b"OK", response)
        }
        b'c' => {
            // Continue: reply with stop signal
            gdb_packet(b"S05", response)
        }
        b's' => {
            // Step: reply with stop signal
            gdb_packet(b"S05", response)
        }
        b'q' => {
            // Query packets
            if starts_with(&payload[1..], b"Supported") {
                gdb_packet(b"PacketSize=4096", response)
            } else if starts_with(&payload[1..], b"Attached") {
                gdb_packet(b"1", response)
            } else if starts_with(&payload[1..], b"C") {
                // Current thread
                gdb_packet(b"QC1", response)
            } else if starts_with(&payload[1..], b"fThreadInfo") {
                gdb_packet(b"m1", response)
            } else if starts_with(&payload[1..], b"sThreadInfo") {
                gdb_packet(b"l", response)
            } else {
                gdb_packet(b"", response)
            }
        }
        b'H' => {
            // Set thread (stub: always OK)
            gdb_packet(b"OK", response)
        }
        b'k' => {
            // Kill
            gdb_packet(b"OK", response)
        }
        b'Z' => {
            // Insert breakpoint/watchpoint (stub)
            gdb_packet(b"OK", response)
        }
        b'z' => {
            // Remove breakpoint/watchpoint (stub)
            gdb_packet(b"OK", response)
        }
        _ => {
            // Unsupported command
            gdb_packet(b"", response)
        }
    }
}

// ============================================================================
// Debugger State
// ============================================================================

/// The inferior (debugged process) state.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InferiorState {
    NotStarted,
    Running,
    Stopped,
    Exited,
}

/// Main debugger state structure.
struct Debugger {
    /// Currently loaded ELF binary.
    elf: Option<ElfInfo>,
    /// Path to the binary being debugged.
    binary_path: [u8; 256],
    binary_path_len: usize,
    /// Current register state.
    regs: [u64; REG_COUNT],
    /// Breakpoints.
    breakpoints: Vec<Breakpoint>,
    /// Next breakpoint ID.
    next_bp_id: u32,
    /// Watchpoints.
    watchpoints: Vec<Watchpoint>,
    /// Next watchpoint ID.
    next_wp_id: u32,
    /// Thread list.
    threads: Vec<ThreadInfo>,
    /// Currently selected thread index.
    current_thread: usize,
    /// Debugger variables.
    vars: Vec<DebugVar>,
    /// Inferior state.
    inferior_state: InferiorState,
    /// Quiet mode (suppress banner).
    quiet: bool,
    /// Last list command address.
    last_list_addr: u64,
    /// Last disassemble address.
    last_disas_addr: u64,
    /// Last examine address.
    last_examine_addr: u64,
    /// Whether we should run as gdbserver.
    is_server: bool,
    /// Server listen port.
    server_port: u16,
}

impl Debugger {
    fn new() -> Self {
        Self {
            elf: None,
            binary_path: [0u8; 256],
            binary_path_len: 0,
            regs: [0u64; REG_COUNT],
            breakpoints: Vec::new(),
            next_bp_id: 1,
            watchpoints: Vec::new(),
            next_wp_id: 1,
            threads: Vec::new(),
            current_thread: 0,
            vars: Vec::new(),
            inferior_state: InferiorState::NotStarted,
            quiet: false,
            last_list_addr: 0,
            last_disas_addr: 0,
            last_examine_addr: 0,
            is_server: false,
            server_port: 0,
        }
    }

    /// Load a binary file for debugging.
    fn load_binary(&mut self, path: &[u8], out: &mut dyn Write) -> bool {
        // Store path
        let plen = path.len().min(255);
        self.binary_path[..plen].copy_from_slice(&path[..plen]);
        self.binary_path_len = plen;

        // Read file via std::fs
        let path_str = match core::str::from_utf8(path) {
            Ok(s) => s,
            Err(_) => {
                let _ = out.write_all(b"Error: invalid path encoding\n");
                return false;
            }
        };

        let data = match std::fs::read(path_str) {
            Ok(d) => d,
            Err(_) => {
                let _ = out.write_all(b"Error: cannot open file: ");
                let _ = out.write_all(path);
                let _ = out.write_all(b"\n");
                return false;
            }
        };

        match parse_elf(&data) {
            Ok(info) => {
                let _ = out.write_all(b"Reading symbols from ");
                let _ = out.write_all(path);
                let _ = out.write_all(b"...\n");

                let mut nbuf = [0u8; 20];
                let n = format_u64(info.symbols.len() as u64, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b" symbols loaded.\n");

                // Set RIP to entry point
                self.regs[REG_RIP] = info.entry_point;
                self.last_disas_addr = info.entry_point;
                self.last_list_addr = info.entry_point;

                // Create initial thread
                self.threads.clear();
                let mut t = ThreadInfo::new(1);
                t.regs = self.regs;
                self.threads.push(t);
                self.current_thread = 0;

                self.elf = Some(info);
                true
            }
            Err(msg) => {
                let _ = out.write_all(b"Error: ");
                let _ = out.write_all(msg);
                let _ = out.write_all(b"\n");
                false
            }
        }
    }

    /// Set a breakpoint. Location can be an address (hex), symbol name, or *addr.
    fn set_breakpoint(&mut self, location: &[u8], out: &mut dyn Write) {
        let addr = self.resolve_location(location);

        match addr {
            Some(a) => {
                let mut bp = Breakpoint::new(self.next_bp_id, a);
                // Store the original byte at this address (if we have the ELF)
                if let Some(ref elf) = self.elf {
                    if let Some(data) = elf.bytes_at_vaddr(a, 1) {
                        bp.original_byte = data[0];
                    }
                }
                // Store location description
                let loc_len = location.len().min(127);
                bp.location[..loc_len].copy_from_slice(&location[..loc_len]);
                bp.location_len = loc_len;

                let _ = out.write_all(b"Breakpoint ");
                let mut nbuf = [0u8; 20];
                let n = format_u64(self.next_bp_id as u64, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b" at ");
                let n = format_hex(a, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);

                // Show symbol if known
                if let Some(ref elf) = self.elf {
                    if let Some(sym) = elf.find_symbol_at(a) {
                        if sym.value == a {
                            let _ = out.write_all(b" <");
                            let _ = out.write_all(sym.name_bytes());
                            let _ = out.write_all(b">");
                        }
                    }
                }
                let _ = out.write_all(b"\n");

                self.breakpoints.push(bp);
                self.next_bp_id += 1;
            }
            None => {
                let _ = out.write_all(b"Cannot resolve location: ");
                let _ = out.write_all(location);
                let _ = out.write_all(b"\n");
            }
        }
    }

    /// Resolve a location string to an address.
    fn resolve_location(&self, loc: &[u8]) -> Option<u64> {
        let loc = trim(loc);
        if loc.is_empty() {
            return None;
        }

        // *address — dereference
        if loc[0] == b'*' {
            return parse_number(trim(&loc[1..]));
        }

        // Try as hex or decimal number
        if let Some(addr) = parse_number(loc) {
            return Some(addr);
        }

        // Try as symbol name
        if let Some(ref elf) = self.elf {
            if let Some(addr) = elf.find_symbol(loc) {
                return Some(addr);
            }
        }

        // Try file:line format (placeholder — would need DWARF debug info)
        if let Some(colon_pos) = loc.iter().position(|&b| b == b':') {
            let line_part = &loc[colon_pos + 1..];
            if let Some(_line) = parse_u64(line_part) {
                // In a real debugger, we would look up DWARF line tables here.
                // For now, we cannot resolve file:line without debug info.
                return None;
            }
        }

        None
    }

    /// Delete a breakpoint by ID, or all if id is 0.
    fn delete_breakpoint(&mut self, id: u32, out: &mut dyn Write) {
        if id == 0 {
            let count = self.breakpoints.len();
            self.breakpoints.clear();
            let _ = out.write_all(b"Deleted ");
            let mut nbuf = [0u8; 20];
            let n = format_u64(count as u64, &mut nbuf);
            let _ = out.write_all(&nbuf[..n]);
            let _ = out.write_all(b" breakpoints.\n");
        } else {
            let before = self.breakpoints.len();
            self.breakpoints.retain(|bp| bp.id != id);
            if self.breakpoints.len() < before {
                let _ = out.write_all(b"Deleted breakpoint ");
                let mut nbuf = [0u8; 20];
                let n = format_u64(id as u64, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b".\n");
            } else {
                let _ = out.write_all(b"No breakpoint number ");
                let mut nbuf = [0u8; 20];
                let n = format_u64(id as u64, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b".\n");
            }
        }
    }

    /// Show breakpoint info.
    fn info_breakpoints(&self, out: &mut dyn Write) {
        if self.breakpoints.is_empty() {
            let _ = out.write_all(b"No breakpoints.\n");
            return;
        }
        let _ = out.write_all(b"Num  Type           Disp Enb Address            What\n");
        let mut nbuf = [0u8; 20];
        let mut hex_buf = [0u8; 20];

        for bp in &self.breakpoints {
            // Num
            let n = format_u64(bp.id as u64, &mut nbuf);
            let _ = out.write_all(&nbuf[..n]);
            // Pad to 5 chars
            for _ in n..5 {
                let _ = out.write_all(b" ");
            }

            // Type
            match bp.kind {
                BreakpointKind::Software => { let _ = out.write_all(b"breakpoint     "); }
                BreakpointKind::Hardware => { let _ = out.write_all(b"hw breakpoint  "); }
            }

            // Disposition
            let _ = out.write_all(b"keep ");

            // Enabled
            if bp.enabled {
                let _ = out.write_all(b"y   ");
            } else {
                let _ = out.write_all(b"n   ");
            }

            // Address
            let n = format_hex_padded(bp.address, &mut hex_buf, 16);
            let _ = out.write_all(&hex_buf[..n]);
            let _ = out.write_all(b" ");

            // Location
            let _ = out.write_all(bp.location_bytes());

            // Hit count
            if bp.hit_count > 0 {
                let _ = out.write_all(b" (hit ");
                let n = format_u64(bp.hit_count, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b" times)");
            }

            let _ = out.write_all(b"\n");
        }
    }

    /// Show register info.
    fn info_registers(&self, out: &mut dyn Write) {
        let mut hex_buf = [0u8; 20];

        for i in 0..REG_COUNT {
            let _ = out.write_all(REG_NAMES[i]);
            // Pad name to 8 chars
            let pad = 8usize.saturating_sub(REG_NAMES[i].len());
            for _ in 0..pad {
                let _ = out.write_all(b" ");
            }
            let n = format_hex_padded(self.regs[i], &mut hex_buf, 16);
            let _ = out.write_all(&hex_buf[..n]);

            // Also show decimal for GPRs
            if i <= REG_R15 {
                let _ = out.write_all(b"\t");
                let mut dbuf = [0u8; 20];
                let dn = format_u64(self.regs[i], &mut dbuf);
                let _ = out.write_all(&dbuf[..dn]);
            }

            let _ = out.write_all(b"\n");
        }
    }

    /// Show thread info.
    fn info_threads(&self, out: &mut dyn Write) {
        if self.threads.is_empty() {
            let _ = out.write_all(b"No threads.\n");
            return;
        }
        let _ = out.write_all(b"  Id   State    Name\n");
        let mut nbuf = [0u8; 20];

        for (idx, thread) in self.threads.iter().enumerate() {
            if idx == self.current_thread {
                let _ = out.write_all(b"* ");
            } else {
                let _ = out.write_all(b"  ");
            }

            let n = format_u64(thread.tid, &mut nbuf);
            let _ = out.write_all(&nbuf[..n]);
            for _ in n..5 {
                let _ = out.write_all(b" ");
            }

            match thread.state {
                ThreadState::Running => { let _ = out.write_all(b"Running  "); }
                ThreadState::Stopped => { let _ = out.write_all(b"Stopped  "); }
                ThreadState::Exited => { let _ = out.write_all(b"Exited   "); }
            }

            if thread.name_len > 0 {
                let _ = out.write_all(&thread.name[..thread.name_len]);
            } else {
                let _ = out.write_all(b"(unnamed)");
            }
            let _ = out.write_all(b"\n");
        }
    }

    /// Show backtrace.
    fn backtrace(&self, out: &mut dyn Write) {
        let mut hex_buf = [0u8; 20];
        let mut nbuf = [0u8; 20];

        // Walk the frame pointer chain
        let mut frames: Vec<StackFrame> = Vec::new();
        let mut current_rip = self.regs[REG_RIP];
        let mut current_rbp = self.regs[REG_RBP];
        let rsp = self.regs[REG_RSP];

        // Frame 0 is always current position
        frames.push(StackFrame {
            frame_num: 0,
            rip: current_rip,
            rbp: current_rbp,
            rsp,
        });

        // Walk up the call stack using frame pointers
        if let Some(ref elf) = self.elf {
            for frame_num in 1..MAX_STACK_FRAMES {
                if current_rbp == 0 {
                    break;
                }
                // Read saved RBP and return address
                if let Some(data) = elf.bytes_at_vaddr(current_rbp, 16) {
                    let saved_rbp = u64::from_le_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    let ret_addr = u64::from_le_bytes([
                        data[8], data[9], data[10], data[11],
                        data[12], data[13], data[14], data[15],
                    ]);

                    if ret_addr == 0 {
                        break;
                    }

                    frames.push(StackFrame {
                        frame_num: frame_num as u32,
                        rip: ret_addr,
                        rbp: saved_rbp,
                        rsp: current_rbp + 16,
                    });

                    let _ = current_rip;
                    current_rip = ret_addr;
                    current_rbp = saved_rbp;
                } else {
                    break;
                }
            }
        }

        for frame in &frames {
            let _ = out.write_all(b"#");
            let n = format_u64(frame.frame_num as u64, &mut nbuf);
            let _ = out.write_all(&nbuf[..n]);
            let _ = out.write_all(b"  ");

            let n = format_hex_padded(frame.rip, &mut hex_buf, 16);
            let _ = out.write_all(&hex_buf[..n]);

            // Try to resolve symbol
            if let Some(ref elf) = self.elf {
                if let Some(sym) = elf.find_symbol_at(frame.rip) {
                    let _ = out.write_all(b" in ");
                    let _ = out.write_all(sym.name_bytes());
                    if sym.value != frame.rip {
                        let _ = out.write_all(b"+");
                        let offset = frame.rip - sym.value;
                        let n = format_u64(offset, &mut nbuf);
                        let _ = out.write_all(&nbuf[..n]);
                    }
                    let _ = out.write_all(b" ()");
                }
            }

            let _ = out.write_all(b"\n");
        }
    }

    /// Handle the `print` / `p` command.
    fn cmd_print(&self, expr: &[u8], out: &mut dyn Write) {
        let expr = trim(expr);
        if expr.is_empty() {
            let _ = out.write_all(b"Argument required (expression to print).\n");
            return;
        }

        // Check if it's a register name starting with $
        if expr[0] == b'$' {
            let reg_name = &expr[1..];
            if let Some(idx) = find_register_index(reg_name) {
                let _ = out.write_all(b"$");
                let _ = out.write_all(reg_name);
                let _ = out.write_all(b" = ");
                let mut hex_buf = [0u8; 20];
                let n = format_hex(self.regs[idx], &mut hex_buf);
                let _ = out.write_all(&hex_buf[..n]);
                let _ = out.write_all(b"\n");
                return;
            }
        }

        // Check if it's a symbol name
        if expr[0] != b'$' && expr[0] != b'(' {
            if let Some(ref elf) = self.elf {
                if let Some(addr) = elf.find_symbol(expr) {
                    let _ = out.write_all(b"$1 = ");
                    let mut hex_buf = [0u8; 20];
                    let n = format_hex(addr, &mut hex_buf);
                    let _ = out.write_all(&hex_buf[..n]);
                    let _ = out.write_all(b" <");
                    let _ = out.write_all(expr);
                    let _ = out.write_all(b">\n");
                    return;
                }
            }
        }

        // Evaluate as expression
        match eval_expr(expr, &self.regs) {
            Ok(val) => {
                let _ = out.write_all(b"$1 = ");
                if val < 0 {
                    let _ = out.write_all(b"-");
                    let mut nbuf = [0u8; 20];
                    let n = format_u64(val.unsigned_abs(), &mut nbuf);
                    let _ = out.write_all(&nbuf[..n]);
                } else {
                    let mut nbuf = [0u8; 20];
                    let n = format_u64(val as u64, &mut nbuf);
                    let _ = out.write_all(&nbuf[..n]);
                }
                let _ = out.write_all(b"\n");
            }
            Err(msg) => {
                let _ = out.write_all(msg);
                let _ = out.write_all(b"\n");
            }
        }
    }

    /// Handle the `x` (examine memory) command.
    fn cmd_examine(&mut self, args: &[u8], out: &mut dyn Write) {
        let args = trim(args);
        if args.is_empty() {
            let _ = out.write_all(b"Argument required (starting address).\n");
            return;
        }

        // Parse format: x/FMT ADDR
        let (count, format, addr_part) = if args[0] == b'/' {
            // Find end of format spec
            let mut end = 1;
            while end < args.len() && args[end] != b' ' && args[end] != b'\t' {
                end += 1;
            }
            let (cnt, fmt) = parse_examine_fmt(&args[1..end]);
            let addr_str = trim(&args[end..]);
            (cnt, fmt, addr_str)
        } else {
            (1, ExamineFormat::Word, args)
        };

        let addr = if addr_part.is_empty() {
            self.last_examine_addr
        } else if let Some(a) = self.resolve_location(addr_part) {
            a
        } else {
            let _ = out.write_all(b"Cannot resolve address: ");
            let _ = out.write_all(addr_part);
            let _ = out.write_all(b"\n");
            return;
        };

        // Determine how many bytes to read
        let bytes_needed = match format {
            ExamineFormat::Byte => count,
            ExamineFormat::Halfword => count * 2,
            ExamineFormat::Word => count * 4,
            ExamineFormat::Giant => count * 8,
            ExamineFormat::StringZ => count * 256,
            ExamineFormat::Instruction => count * 16,
        };

        if let Some(ref elf) = self.elf {
            if let Some(data) = elf.bytes_at_vaddr(addr, bytes_needed) {
                format_examine(out, data, addr, count, format);
                // Update last examine address
                self.last_examine_addr = addr + data.len() as u64;
            } else {
                let _ = out.write_all(b"Cannot access memory at address ");
                let mut hex_buf = [0u8; 20];
                let n = format_hex(addr, &mut hex_buf);
                let _ = out.write_all(&hex_buf[..n]);
                let _ = out.write_all(b"\n");
            }
        } else {
            let _ = out.write_all(b"No file loaded.\n");
        }
    }

    /// Handle the `disassemble` / `disas` command.
    fn cmd_disassemble(&mut self, args: &[u8], out: &mut dyn Write) {
        let args = trim(args);

        let (start_addr, count) = if args.is_empty() {
            (self.last_disas_addr, 10usize)
        } else if let Some(addr) = self.resolve_location(args) {
            (addr, 10)
        } else {
            let _ = out.write_all(b"Cannot resolve address.\n");
            return;
        };

        let elf_ref = match self.elf {
            Some(ref e) => e,
            None => {
                let _ = out.write_all(b"No file loaded.\n");
                return;
            }
        };

        // Try to get function boundaries
        let func_name = if let Some(sym) = elf_ref.find_symbol_at(start_addr) {
            if sym.value == start_addr {
                Some((sym.name_bytes().to_vec(), sym.size))
            } else {
                None
            }
        } else {
            None
        };

        if let Some((name, _size)) = &func_name {
            let _ = out.write_all(b"Dump of assembler code for function ");
            let _ = out.write_all(name);
            let _ = out.write_all(b":\n");
        }

        let max_bytes = count * 16; // Conservative estimate
        if let Some(code) = elf_ref.bytes_at_vaddr(start_addr, max_bytes) {
            let mut offset = 0;
            let mut hex_buf = [0u8; 20];
            let mut inst_count = 0;

            while offset < code.len() && inst_count < count {
                let current_addr = start_addr + offset as u64;
                let remaining = &code[offset..];

                let inst = disasm_one(remaining, current_addr);

                // Print address
                let _ = out.write_all(b"   ");
                let n = format_hex_padded(current_addr, &mut hex_buf, 16);
                let _ = out.write_all(&hex_buf[..n]);

                // Mark current instruction
                if current_addr == self.regs[REG_RIP] {
                    let _ = out.write_all(b" => ");
                } else {
                    let _ = out.write_all(b"    ");
                }

                // Print mnemonic
                let _ = out.write_all(&inst.text[..inst.text_len]);
                let _ = out.write_all(b"\n");

                offset += inst.len;
                inst_count += 1;
            }

            self.last_disas_addr = start_addr + offset as u64;
        } else {
            let _ = out.write_all(b"Cannot access memory at address.\n");
        }

        if func_name.is_some() {
            let _ = out.write_all(b"End of assembler dump.\n");
        }
    }

    /// Handle the `list` / `l` command (source listing).
    fn cmd_list(&mut self, args: &[u8], out: &mut dyn Write) {
        // Without DWARF debug info, we can only show the disassembly around
        // the current PC. This is a stub that shows address context.
        let args = trim(args);
        let addr = if args.is_empty() {
            self.last_list_addr
        } else if let Some(a) = self.resolve_location(args) {
            a
        } else {
            let _ = out.write_all(b"Cannot resolve location.\n");
            return;
        };

        let _ = out.write_all(b"No source available. Showing disassembly:\n");
        self.last_list_addr = addr;
        self.cmd_disassemble(args, out);
    }

    /// Handle the `set` command.
    fn cmd_set(&mut self, args: &[u8], out: &mut dyn Write) {
        let args = trim(args);
        // Find '='
        let eq_pos = match args.iter().position(|&b| b == b'=') {
            Some(p) => p,
            None => {
                let _ = out.write_all(b"Usage: set VARIABLE = VALUE\n");
                return;
            }
        };

        let name = trim(&args[..eq_pos]);
        let val_str = trim(&args[eq_pos + 1..]);

        // Check if setting a register
        if !name.is_empty() && name[0] == b'$' {
            let reg_name = &name[1..];
            if let Some(idx) = find_register_index(reg_name) {
                if let Some(val) = parse_number(val_str) {
                    self.regs[idx] = val;
                    let _ = out.write_all(name);
                    let _ = out.write_all(b" = ");
                    let mut hex_buf = [0u8; 20];
                    let n = format_hex(val, &mut hex_buf);
                    let _ = out.write_all(&hex_buf[..n]);
                    let _ = out.write_all(b"\n");
                } else {
                    let _ = out.write_all(b"Invalid value.\n");
                }
                return;
            }
        }

        // Set debugger variable
        if let Some(val) = parse_i64(val_str) {
            // Check if variable exists
            for var in &mut self.vars {
                if bytes_eq(&var.name[..var.name_len], name) {
                    var.value = val;
                    let _ = out.write_all(name);
                    let _ = out.write_all(b" = ");
                    let mut nbuf = [0u8; 20];
                    let n = format_u64(val as u64, &mut nbuf);
                    let _ = out.write_all(&nbuf[..n]);
                    let _ = out.write_all(b"\n");
                    return;
                }
            }
            // Create new variable
            if self.vars.len() < MAX_DEBUG_VARS {
                let mut var = DebugVar {
                    name: [0u8; 64],
                    name_len: 0,
                    value: val,
                };
                let nlen = name.len().min(63);
                var.name[..nlen].copy_from_slice(&name[..nlen]);
                var.name_len = nlen;
                self.vars.push(var);
                let _ = out.write_all(name);
                let _ = out.write_all(b" = ");
                let mut nbuf = [0u8; 20];
                let n = format_u64(val as u64, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b"\n");
            } else {
                let _ = out.write_all(b"Too many variables.\n");
            }
        } else {
            let _ = out.write_all(b"Invalid value.\n");
        }
    }

    /// Handle the `watch` command.
    fn cmd_watch(&mut self, args: &[u8], out: &mut dyn Write) {
        let args = trim(args);
        if args.is_empty() {
            let _ = out.write_all(b"Argument required (expression to watch).\n");
            return;
        }

        let addr = match self.resolve_location(args) {
            Some(a) => a,
            None => {
                // Try evaluating as expression
                match eval_expr(args, &self.regs) {
                    Ok(val) => val as u64,
                    Err(_) => {
                        let _ = out.write_all(b"Cannot resolve watchpoint expression: ");
                        let _ = out.write_all(args);
                        let _ = out.write_all(b"\n");
                        return;
                    }
                }
            }
        };

        if self.watchpoints.len() >= MAX_WATCHPOINTS {
            let _ = out.write_all(b"Too many watchpoints.\n");
            return;
        }

        let mut wp = Watchpoint::new(self.next_wp_id, addr, 8);
        let elen = args.len().min(127);
        wp.expr[..elen].copy_from_slice(&args[..elen]);
        wp.expr_len = elen;

        let _ = out.write_all(b"Hardware watchpoint ");
        let mut nbuf = [0u8; 20];
        let n = format_u64(self.next_wp_id as u64, &mut nbuf);
        let _ = out.write_all(&nbuf[..n]);
        let _ = out.write_all(b": ");
        let _ = out.write_all(args);
        let _ = out.write_all(b"\n");

        self.watchpoints.push(wp);
        self.next_wp_id += 1;
    }

    /// Handle the `run` / `r` command.
    fn cmd_run(&mut self, out: &mut dyn Write) {
        if self.elf.is_none() {
            let _ = out.write_all(b"No executable specified. Use \"file\" command.\n");
            return;
        }

        let _ = out.write_all(b"Starting program: ");
        let _ = out.write_all(&self.binary_path[..self.binary_path_len]);
        let _ = out.write_all(b"\n");

        // Reset registers to initial state
        let entry = self.elf.as_ref().map_or(0, |e| e.entry_point);
        self.regs = [0u64; REG_COUNT];
        self.regs[REG_RIP] = entry;
        self.regs[REG_RSP] = 0x7fff_fff0_0000; // Typical user stack top
        self.regs[REG_RFLAGS] = 0x202; // IF set
        self.regs[REG_CS] = 0x33; // User code segment
        self.regs[REG_SS] = 0x2b; // User stack segment

        self.inferior_state = InferiorState::Stopped;

        // Check if we hit a breakpoint at entry
        for bp in &mut self.breakpoints {
            if bp.enabled && bp.address == entry {
                bp.hit_count += 1;
                let _ = out.write_all(b"\nBreakpoint ");
                let mut nbuf = [0u8; 20];
                let n = format_u64(bp.id as u64, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b" at ");
                let n = format_hex(bp.address, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b"\n");
                return;
            }
        }

        let _ = out.write_all(b"Program stopped at entry point.\n");
    }

    /// Handle the `continue` / `c` command.
    fn cmd_continue(&mut self, out: &mut dyn Write) {
        if self.inferior_state == InferiorState::NotStarted {
            let _ = out.write_all(b"The program is not being run.\n");
            return;
        }
        if self.inferior_state == InferiorState::Exited {
            let _ = out.write_all(b"The program is not being run.\n");
            return;
        }

        let _ = out.write_all(b"Continuing.\n");
        self.inferior_state = InferiorState::Running;

        // Simulate hitting next breakpoint or program exit
        let current_rip = self.regs[REG_RIP];
        let mut hit_bp = false;
        for bp in &mut self.breakpoints {
            if bp.enabled && bp.address > current_rip {
                bp.hit_count += 1;
                self.regs[REG_RIP] = bp.address;
                self.inferior_state = InferiorState::Stopped;
                let _ = out.write_all(b"\nBreakpoint ");
                let mut nbuf = [0u8; 20];
                let n = format_u64(bp.id as u64, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b", ");
                let n = format_hex(bp.address, &mut nbuf);
                let _ = out.write_all(&nbuf[..n]);
                let _ = out.write_all(b"\n");
                hit_bp = true;
                break;
            }
        }

        if !hit_bp {
            self.inferior_state = InferiorState::Exited;
            let _ = out.write_all(b"\n[Inferior 1 exited normally]\n");
        }
    }

    /// Handle the `step` / `s` / `si` command.
    fn cmd_step(&mut self, out: &mut dyn Write) {
        if self.inferior_state == InferiorState::NotStarted {
            let _ = out.write_all(b"The program is not being run.\n");
            return;
        }
        if self.inferior_state == InferiorState::Exited {
            let _ = out.write_all(b"The program is not being run.\n");
            return;
        }

        // Advance RIP by one instruction
        let current_rip = self.regs[REG_RIP];
        if let Some(ref elf) = self.elf {
            if let Some(code) = elf.bytes_at_vaddr(current_rip, 16) {
                let inst = disasm_one(code, current_rip);
                self.regs[REG_RIP] = current_rip + inst.len as u64;
            } else {
                self.regs[REG_RIP] = current_rip + 1;
            }
        } else {
            self.regs[REG_RIP] = current_rip + 1;
        }

        // Show current location
        let mut hex_buf = [0u8; 20];
        let n = format_hex_padded(self.regs[REG_RIP], &mut hex_buf, 16);
        let _ = out.write_all(&hex_buf[..n]);

        if let Some(ref elf) = self.elf {
            if let Some(sym) = elf.find_symbol_at(self.regs[REG_RIP]) {
                let _ = out.write_all(b" in ");
                let _ = out.write_all(sym.name_bytes());
                let _ = out.write_all(b" ()");
            }
        }
        let _ = out.write_all(b"\n");
    }

    /// Handle the `next` / `n` / `ni` command (step over).
    fn cmd_next(&mut self, out: &mut dyn Write) {
        if self.inferior_state == InferiorState::NotStarted {
            let _ = out.write_all(b"The program is not being run.\n");
            return;
        }
        if self.inferior_state == InferiorState::Exited {
            let _ = out.write_all(b"The program is not being run.\n");
            return;
        }

        // For step-over, if the current instruction is a call, we advance past it
        let current_rip = self.regs[REG_RIP];
        if let Some(ref elf) = self.elf {
            if let Some(code) = elf.bytes_at_vaddr(current_rip, 16) {
                let inst = disasm_one(code, current_rip);
                // Step over: always advance past the instruction
                self.regs[REG_RIP] = current_rip + inst.len as u64;
            } else {
                self.regs[REG_RIP] = current_rip + 1;
            }
        } else {
            self.regs[REG_RIP] = current_rip + 1;
        }

        // Show current location
        let mut hex_buf = [0u8; 20];
        let n = format_hex_padded(self.regs[REG_RIP], &mut hex_buf, 16);
        let _ = out.write_all(&hex_buf[..n]);

        if let Some(ref elf) = self.elf {
            if let Some(sym) = elf.find_symbol_at(self.regs[REG_RIP]) {
                let _ = out.write_all(b" in ");
                let _ = out.write_all(sym.name_bytes());
                let _ = out.write_all(b" ()");
            }
        }
        let _ = out.write_all(b"\n");
    }

    /// Handle the `help` command.
    fn cmd_help(&self, topic: &[u8], out: &mut dyn Write) {
        let topic = trim(topic);
        if topic.is_empty() {
            let _ = out.write_all(b"List of commands:\n\n");
            let _ = out.write_all(b"  run (r)          -- Start the debugged program\n");
            let _ = out.write_all(b"  continue (c)     -- Continue program execution\n");
            let _ = out.write_all(b"  step (s, si)     -- Step one instruction\n");
            let _ = out.write_all(b"  next (n, ni)     -- Step over one instruction\n");
            let _ = out.write_all(b"  break (b) LOC    -- Set breakpoint at LOC\n");
            let _ = out.write_all(b"  delete [NUM]     -- Delete breakpoint(s)\n");
            let _ = out.write_all(b"  info WHAT        -- Show information (breakpoints, registers, threads)\n");
            let _ = out.write_all(b"  backtrace (bt)   -- Show call stack\n");
            let _ = out.write_all(b"  print (p) EXPR   -- Evaluate and print expression\n");
            let _ = out.write_all(b"  x/FMT ADDR       -- Examine memory\n");
            let _ = out.write_all(b"  list (l)         -- Show source or disassembly\n");
            let _ = out.write_all(b"  disassemble      -- Disassemble at current position\n");
            let _ = out.write_all(b"  set VAR=VAL      -- Set variable or register\n");
            let _ = out.write_all(b"  watch EXPR       -- Set watchpoint\n");
            let _ = out.write_all(b"  file PATH        -- Load executable for debugging\n");
            let _ = out.write_all(b"  quit (q)         -- Exit debugger\n");
            let _ = out.write_all(b"  help [TOPIC]     -- Show help\n");
            return;
        }

        if bytes_eq(topic, b"breakpoints") || bytes_eq(topic, b"break") || bytes_eq(topic, b"b") {
            let _ = out.write_all(b"break LOCATION\n");
            let _ = out.write_all(b"  Set a breakpoint at LOCATION.\n");
            let _ = out.write_all(b"  LOCATION can be: address (hex with 0x prefix),\n");
            let _ = out.write_all(b"  symbol name, or *address.\n");
        } else if bytes_eq(topic, b"x") {
            let _ = out.write_all(b"x/[COUNT][FORMAT] ADDRESS\n");
            let _ = out.write_all(b"  Examine memory at ADDRESS.\n");
            let _ = out.write_all(b"  FORMAT: b=byte, h=halfword, w=word, g=giant, s=string, i=instruction\n");
        } else {
            let _ = out.write_all(b"No help available for: ");
            let _ = out.write_all(topic);
            let _ = out.write_all(b"\n");
        }
    }

    /// Process a single command line.
    /// Returns `true` if the debugger should continue, `false` to quit.
    fn process_command(&mut self, line: &[u8], out: &mut dyn Write) -> bool {
        let line = trim(line);
        if line.is_empty() {
            return true;
        }

        // Split into command and arguments
        let mut parts = [&b""[..]; 16];
        let nparts = split_whitespace(line, &mut parts, 16);
        if nparts == 0 {
            return true;
        }

        let cmd = parts[0];
        let rest_start = if nparts > 1 {
            // Find where args begin
            let cmd_end = cmd.as_ptr() as usize - line.as_ptr() as usize + cmd.len();
            &line[cmd_end..]
        } else {
            b""
        };
        let rest = trim(rest_start);

        // Command dispatch
        match cmd {
            b"quit" | b"q" => {
                let _ = out.write_all(b"Quitting.\n");
                return false;
            }
            b"help" | b"h" => {
                self.cmd_help(rest, out);
            }
            b"run" | b"r" => {
                self.cmd_run(out);
            }
            b"continue" | b"c" => {
                self.cmd_continue(out);
            }
            b"step" | b"s" | b"si" => {
                self.cmd_step(out);
            }
            b"next" | b"n" | b"ni" => {
                self.cmd_next(out);
            }
            b"break" | b"b" => {
                if rest.is_empty() {
                    let _ = out.write_all(b"Argument required (breakpoint location).\n");
                } else {
                    self.set_breakpoint(rest, out);
                }
            }
            b"delete" | b"d" => {
                if rest.is_empty() {
                    self.delete_breakpoint(0, out);
                } else if let Some(id) = parse_u64(rest) {
                    self.delete_breakpoint(id as u32, out);
                } else {
                    let _ = out.write_all(b"Invalid breakpoint number.\n");
                }
            }
            b"info" | b"i" => {
                if nparts < 2 {
                    let _ = out.write_all(b"\"info\" must be followed by a subcommand.\n");
                    let _ = out.write_all(b"  info breakpoints -- List breakpoints\n");
                    let _ = out.write_all(b"  info registers   -- Show registers\n");
                    let _ = out.write_all(b"  info threads     -- Show threads\n");
                } else {
                    match parts[1] {
                        b"breakpoints" | b"break" | b"b" => self.info_breakpoints(out),
                        b"registers" | b"reg" | b"r" => self.info_registers(out),
                        b"threads" | b"thread" | b"t" => self.info_threads(out),
                        _ => {
                            let _ = out.write_all(b"Unknown info subcommand: ");
                            let _ = out.write_all(parts[1]);
                            let _ = out.write_all(b"\n");
                        }
                    }
                }
            }
            b"backtrace" | b"bt" => {
                self.backtrace(out);
            }
            b"print" | b"p" => {
                self.cmd_print(rest, out);
            }
            _ if starts_with(cmd, b"x/") => {
                // x/FMT ADDR
                let fmt_and_addr = &line[2..]; // skip "x/"
                // Split at first space to separate format from address
                let mut space_pos = 0;
                while space_pos < fmt_and_addr.len()
                    && fmt_and_addr[space_pos] != b' '
                    && fmt_and_addr[space_pos] != b'\t'
                {
                    space_pos += 1;
                }
                // Reconstruct as "/FMT ADDR" for cmd_examine
                let mut combined = Vec::with_capacity(fmt_and_addr.len() + 1);
                combined.push(b'/');
                combined.extend_from_slice(fmt_and_addr);
                self.cmd_examine(&combined, out);
            }
            b"x" => {
                self.cmd_examine(rest, out);
            }
            b"list" | b"l" => {
                self.cmd_list(rest, out);
            }
            b"disassemble" | b"disas" => {
                self.cmd_disassemble(rest, out);
            }
            b"set" => {
                self.cmd_set(rest, out);
            }
            b"watch" | b"w" if !bytes_eq(cmd, b"w") || !rest.is_empty() => {
                self.cmd_watch(rest, out);
            }
            b"watch" => {
                self.cmd_watch(rest, out);
            }
            b"file" => {
                if rest.is_empty() {
                    let _ = out.write_all(b"Argument required (file to load).\n");
                } else {
                    self.load_binary(rest, out);
                }
            }
            _ => {
                let _ = out.write_all(b"Undefined command: \"");
                let _ = out.write_all(cmd);
                let _ = out.write_all(b"\". Try \"help\".\n");
            }
        }

        true
    }

    /// Main interactive debugger loop (reads from stdin).
    fn run_interactive(&mut self) -> i32 {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());

        if !self.quiet {
            let _ = out.write_all(GDB_BANNER);
        }

        let _ = out.flush();

        let stdin = io::stdin();
        let mut line_buf = [0u8; 4096];

        loop {
            // Print prompt
            let _ = out.write_all(b"(gdb) ");
            let _ = out.flush();

            // Read a line from stdin
            let n = match stdin.lock().read_line_bytes(&mut line_buf) {
                Ok(n) => n,
                Err(_) => break,
            };

            if n == 0 {
                // EOF
                let _ = out.write_all(b"quit\n");
                break;
            }

            let line = &line_buf[..n];
            if !self.process_command(line, &mut out) {
                break;
            }
            let _ = out.flush();
        }

        0
    }

    /// Run as gdbserver.
    fn run_server(&mut self) -> i32 {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());

        let _ = out.write_all(b"gdbserver: listening on port ");
        let mut nbuf = [0u8; 20];
        let n = format_u64(self.server_port as u64, &mut nbuf);
        let _ = out.write_all(&nbuf[..n]);
        let _ = out.write_all(b"\n");

        if self.elf.is_some() {
            let _ = out.write_all(b"Process ");
            let _ = out.write_all(&self.binary_path[..self.binary_path_len]);
            let _ = out.write_all(b" created.\n");
        }

        let _ = out.write_all(b"Waiting for connection...\n");
        let _ = out.flush();

        // In the real OS, we would listen on a TCP socket.
        // For now, we read GDB remote protocol packets from stdin and
        // respond on stdout, simulating the protocol exchange.
        let stdin = io::stdin();
        let mut buf = [0u8; 4096];
        let mut response = [0u8; 8192];

        loop {
            let n = match stdin.lock().read_line_bytes(&mut buf) {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }

            let input = &buf[..n];

            // Handle ack
            if !input.is_empty() && input[0] == b'+' {
                continue;
            }

            // Parse and handle packet
            if let Some((payload, _consumed)) = gdb_parse_packet(input) {
                // Send ack
                let _ = out.write_all(b"+");

                let resp_len = gdb_handle_command(
                    payload,
                    &self.regs,
                    &self.elf,
                    &mut response,
                );
                let _ = out.write_all(&response[..resp_len]);
                let _ = out.flush();

                // Check for kill command
                if !payload.is_empty() && payload[0] == b'k' {
                    break;
                }
            }
        }

        0
    }
}

/// Helper trait for reading lines from stdin as bytes.
trait ReadLineBytes {
    fn read_line_bytes(&self, buf: &mut [u8]) -> io::Result<usize>;
}

impl<T: io::Read> ReadLineBytes for T {
    fn read_line_bytes(&self, buf: &mut [u8]) -> io::Result<usize> {
        // We cannot mutably borrow self here because stdin().lock() returns
        // a value that implements Read. Use a simple byte-at-a-time read via
        // std::io::Read on a mutable reference obtained through an unsafe
        // reborrow. Instead, use the BufRead trait on stdin.
        // Actually, let's just use the approach other utilities use:
        // read byte by byte.
        let _ = (self, buf);
        Err(io::Error::new(io::ErrorKind::Unsupported, "use alternate"))
    }
}

/// Read one line from stdin into buf, returning bytes read (including newline).
fn read_stdin_line(buf: &mut [u8]) -> io::Result<usize> {
    let mut pos = 0;
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    loop {
        let mut byte = [0u8; 1];
        match io::Read::read(&mut handle, &mut byte) {
            Ok(0) => return Ok(pos), // EOF
            Ok(_) => {
                if pos < buf.len() {
                    buf[pos] = byte[0];
                    pos += 1;
                }
                if byte[0] == b'\n' {
                    return Ok(pos);
                }
            }
            Err(e) => {
                if pos > 0 {
                    return Ok(pos);
                }
                return Err(e);
            }
        }
    }
}

// ============================================================================
// Personality detection
// ============================================================================

/// Which personality to run as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Gdb,
    GdbServer,
}

/// Detect personality from argv[0].
fn detect_personality(argv0: &[u8]) -> Personality {
    let name = basename(argv0);
    // Strip .exe suffix if present
    let name = if ends_with(name, b".exe") {
        &name[..name.len() - 4]
    } else {
        name
    };

    if bytes_eq(name, b"gdbserver") {
        Personality::GdbServer
    } else {
        Personality::Gdb
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line arguments.
struct Args {
    personality: Personality,
    binary_path: Option<Vec<u8>>,
    quiet: bool,
    show_help: bool,
    show_version: bool,
    command_file: Option<Vec<u8>>,
    server_port: u16,
    pass_args: bool,
}

/// Parse arguments for the gdb personality.
fn parse_args_gdb(argc: i32, argv: *const *const u8) -> Args {
    let mut args = Args {
        personality: Personality::Gdb,
        binary_path: None,
        quiet: false,
        show_help: false,
        show_version: false,
        command_file: None,
        server_port: 0,
        pass_args: false,
    };

    // SAFETY: We trust that argc and argv are valid from the C runtime.
    let arg_ptrs: Vec<&[u8]> = (0..argc as usize)
        .map(|i| unsafe {
            let ptr = *argv.add(i);
            cstr_to_slice(ptr)
        })
        .collect();

    if arg_ptrs.is_empty() {
        return args;
    }

    // Detect personality from argv[0]
    args.personality = detect_personality(arg_ptrs[0]);

    let mut i = 1;
    while i < arg_ptrs.len() {
        let arg = arg_ptrs[i];
        match arg {
            b"--help" | b"-h" => { args.show_help = true; }
            b"--version" | b"-v" => { args.show_version = true; }
            b"--quiet" | b"-q" => { args.quiet = true; }
            b"--args" => { args.pass_args = true; }
            b"-x" => {
                i += 1;
                if i < arg_ptrs.len() {
                    args.command_file = Some(arg_ptrs[i].to_vec());
                }
            }
            _ => {
                if arg[0] != b'-' && args.binary_path.is_none() {
                    args.binary_path = Some(arg.to_vec());
                }
            }
        }
        i += 1;
    }

    args
}

/// Parse arguments for the gdbserver personality.
fn parse_args_server(argc: i32, argv: *const *const u8) -> Args {
    let mut args = Args {
        personality: Personality::GdbServer,
        binary_path: None,
        quiet: false,
        show_help: false,
        show_version: false,
        command_file: None,
        server_port: 1234,
        pass_args: false,
    };

    let arg_ptrs: Vec<&[u8]> = (0..argc as usize)
        .map(|i| unsafe {
            let ptr = *argv.add(i);
            cstr_to_slice(ptr)
        })
        .collect();

    if arg_ptrs.is_empty() {
        return args;
    }

    args.personality = detect_personality(arg_ptrs[0]);

    let mut i = 1;
    while i < arg_ptrs.len() {
        let arg = arg_ptrs[i];
        match arg {
            b"--help" => { args.show_help = true; }
            b"--version" => { args.show_version = true; }
            _ => {
                // First non-flag arg is [host:]port
                if arg[0] != b'-' && args.server_port == 1234 && args.binary_path.is_none() {
                    // Try to parse as port number (possibly with host: prefix)
                    let port_str = if let Some(colon_pos) = arg.iter().rposition(|&b| b == b':') {
                        &arg[colon_pos + 1..]
                    } else {
                        arg
                    };
                    if let Some(port) = parse_u64(port_str) {
                        args.server_port = port as u16;
                    }
                } else if args.binary_path.is_none() {
                    args.binary_path = Some(arg.to_vec());
                }
            }
        }
        i += 1;
    }

    args
}

// ============================================================================
// Help / Version output
// ============================================================================

fn print_help_gdb(out: &mut dyn Write) {
    let _ = out.write_all(b"Usage: gdb [OPTIONS] [EXECUTABLE]\n\n");
    let _ = out.write_all(b"Options:\n");
    let _ = out.write_all(b"  --help, -h       Display this help and exit\n");
    let _ = out.write_all(b"  --version, -v    Display version and exit\n");
    let _ = out.write_all(b"  -q, --quiet      Suppress startup banner\n");
    let _ = out.write_all(b"  -x FILE          Execute commands from FILE\n");
    let _ = out.write_all(b"  --args           Pass remaining arguments to inferior\n");
}

fn print_help_server(out: &mut dyn Write) {
    let _ = out.write_all(b"Usage: gdbserver [HOST:]PORT EXECUTABLE [ARGS...]\n\n");
    let _ = out.write_all(b"Options:\n");
    let _ = out.write_all(b"  --help           Display this help and exit\n");
    let _ = out.write_all(b"  --version        Display version and exit\n");
}

fn print_version(out: &mut dyn Write) {
    let _ = out.write_all(b"OurOS GDB ");
    let _ = out.write_all(VERSION);
    let _ = out.write_all(b"\n");
}

// ============================================================================
// Entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
// `main` is the C ABI entry point: the runtime hands us `argv` as a raw pointer
// and we must dereference it to read the arguments.  The signature is fixed by
// the ABI, so the function cannot be marked `unsafe fn`; the dereference is
// guarded by the `argc > 0` check below.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    // Quick peek at argv[0] to decide personality
    let argv0 = if argc > 0 {
        unsafe { cstr_to_slice(*argv) }
    } else {
        b"gdb"
    };
    let personality = detect_personality(argv0);

    let args = match personality {
        Personality::Gdb => parse_args_gdb(argc, argv),
        Personality::GdbServer => parse_args_server(argc, argv),
    };

    if args.show_help {
        match args.personality {
            Personality::Gdb => print_help_gdb(&mut out),
            Personality::GdbServer => print_help_server(&mut out),
        }
        let _ = out.flush();
        return 0;
    }

    if args.show_version {
        print_version(&mut out);
        let _ = out.flush();
        return 0;
    }

    drop(out); // Release stdout lock before interactive mode

    let mut debugger = Debugger::new();
    debugger.quiet = args.quiet;
    debugger.is_server = args.personality == Personality::GdbServer;
    debugger.server_port = args.server_port;

    // Load binary if specified
    if let Some(ref path) = args.binary_path {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        debugger.load_binary(path, &mut out);
        let _ = out.flush();
    }

    // Execute command file if specified
    if let Some(ref cmd_file) = args.command_file {
        let path_str = match core::str::from_utf8(cmd_file) {
            Ok(s) => s,
            Err(_) => {
                let stderr = io::stderr();
                let mut err = stderr.lock();
                let _ = err.write_all(b"Error: invalid command file path\n");
                return 1;
            }
        };
        if let Ok(contents) = std::fs::read(path_str) {
            let stdout = io::stdout();
            let mut out = io::BufWriter::new(stdout.lock());
            for line in contents.split(|&b| b == b'\n') {
                let line = trim(line);
                if line.is_empty() || line[0] == b'#' {
                    continue;
                }
                if !debugger.process_command(line, &mut out) {
                    let _ = out.flush();
                    return 0;
                }
            }
            let _ = out.flush();
        }
    }

    match args.personality {
        Personality::Gdb => debugger.run_interactive(),
        Personality::GdbServer => debugger.run_server(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Helper ----

    /// Capture output from a closure that writes to a `dyn Write`.
    fn capture<F: FnOnce(&mut Vec<u8>)>(f: F) -> Vec<u8> {
        let mut buf = Vec::new();
        f(&mut buf);
        buf
    }

    // ---- String / Number helpers ----

    #[test]
    fn test_bytes_eq() {
        assert!(bytes_eq(b"hello", b"hello"));
        assert!(!bytes_eq(b"hello", b"world"));
        assert!(!bytes_eq(b"hi", b"hello"));
    }

    #[test]
    fn test_starts_with() {
        assert!(starts_with(b"hello world", b"hello"));
        assert!(!starts_with(b"hi", b"hello"));
        assert!(starts_with(b"x", b""));
    }

    #[test]
    fn test_ends_with() {
        assert!(ends_with(b"hello world", b"world"));
        assert!(!ends_with(b"hi", b"world"));
        assert!(ends_with(b"x", b""));
    }

    #[test]
    fn test_parse_u64_basic() {
        assert_eq!(parse_u64(b"0"), Some(0));
        assert_eq!(parse_u64(b"42"), Some(42));
        assert_eq!(parse_u64(b"12345"), Some(12345));
        assert_eq!(parse_u64(b""), None);
        assert_eq!(parse_u64(b"abc"), None);
    }

    #[test]
    fn test_parse_u64_large() {
        assert_eq!(parse_u64(b"18446744073709551615"), Some(u64::MAX));
    }

    #[test]
    fn test_parse_hex_with_prefix() {
        assert_eq!(parse_hex(b"0xff"), Some(255));
        assert_eq!(parse_hex(b"0X1A"), Some(26));
        assert_eq!(parse_hex(b"0x0"), Some(0));
    }

    #[test]
    fn test_parse_hex_without_prefix() {
        assert_eq!(parse_hex(b"ff"), Some(255));
        assert_eq!(parse_hex(b"deadbeef"), Some(0xdeadbeef));
    }

    #[test]
    fn test_parse_hex_invalid() {
        assert_eq!(parse_hex(b""), None);
        assert_eq!(parse_hex(b"0x"), None);
        assert_eq!(parse_hex(b"xyz"), None);
    }

    #[test]
    fn test_parse_number() {
        assert_eq!(parse_number(b"42"), Some(42));
        assert_eq!(parse_number(b"0xff"), Some(255));
        assert_eq!(parse_number(b"0x100"), Some(256));
    }

    #[test]
    fn test_parse_i64() {
        assert_eq!(parse_i64(b"42"), Some(42));
        assert_eq!(parse_i64(b"-1"), Some(-1));
        assert_eq!(parse_i64(b"0"), Some(0));
        assert_eq!(parse_i64(b""), None);
    }

    #[test]
    fn test_format_u64() {
        let mut buf = [0u8; 20];
        let n = format_u64(0, &mut buf);
        assert_eq!(&buf[..n], b"0");

        let n = format_u64(12345, &mut buf);
        assert_eq!(&buf[..n], b"12345");

        let n = format_u64(999, &mut buf);
        assert_eq!(&buf[..n], b"999");
    }

    #[test]
    fn test_format_hex() {
        let mut buf = [0u8; 20];
        let n = format_hex(0, &mut buf);
        assert_eq!(&buf[..n], b"0x0");

        let n = format_hex(255, &mut buf);
        assert_eq!(&buf[..n], b"0xff");

        let n = format_hex(0xdeadbeef, &mut buf);
        assert_eq!(&buf[..n], b"0xdeadbeef");
    }

    #[test]
    fn test_format_hex_padded() {
        let mut buf = [0u8; 20];
        let n = format_hex_padded(0xff, &mut buf, 4);
        assert_eq!(&buf[..n], b"0x00ff");

        let n = format_hex_padded(0, &mut buf, 8);
        assert_eq!(&buf[..n], b"0x00000000");
    }

    #[test]
    fn test_trim_start() {
        assert_eq!(trim_start(b"  hello"), b"hello");
        assert_eq!(trim_start(b"hello"), b"hello");
        assert_eq!(trim_start(b"  "), b"");
    }

    #[test]
    fn test_trim_end() {
        assert_eq!(trim_end(b"hello  "), b"hello");
        assert_eq!(trim_end(b"hello\n"), b"hello");
        assert_eq!(trim_end(b"  "), b"");
    }

    #[test]
    fn test_trim() {
        assert_eq!(trim(b"  hello  "), b"hello");
        assert_eq!(trim(b"\thello\n"), b"hello");
    }

    #[test]
    fn test_split_whitespace() {
        let mut parts = [&b""[..]; 8];
        let n = split_whitespace(b"hello world foo", &mut parts, 8);
        assert_eq!(n, 3);
        assert_eq!(parts[0], b"hello");
        assert_eq!(parts[1], b"world");
        assert_eq!(parts[2], b"foo");
    }

    #[test]
    fn test_split_whitespace_empty() {
        let mut parts = [&b""[..]; 8];
        let n = split_whitespace(b"", &mut parts, 8);
        assert_eq!(n, 0);
    }

    #[test]
    fn test_split_whitespace_max_parts() {
        let mut parts = [&b""[..]; 2];
        let n = split_whitespace(b"a b c d", &mut parts, 2);
        assert_eq!(n, 2);
        assert_eq!(parts[0], b"a");
        assert_eq!(parts[1], b"b");
    }

    #[test]
    fn test_basename() {
        assert_eq!(basename(b"/usr/bin/gdb"), b"gdb");
        assert_eq!(basename(b"C:\\bin\\gdb.exe"), b"gdb.exe");
        assert_eq!(basename(b"gdb"), b"gdb");
    }

    // ---- ELF parsing ----

    #[test]
    fn test_read_u16_le() {
        assert_eq!(read_u16_le(&[0x34, 0x12], 0), Some(0x1234));
        assert_eq!(read_u16_le(&[0x00], 0), None);
    }

    #[test]
    fn test_read_u32_le() {
        assert_eq!(read_u32_le(&[0x78, 0x56, 0x34, 0x12], 0), Some(0x12345678));
        assert_eq!(read_u32_le(&[0x00, 0x00], 0), None);
    }

    #[test]
    fn test_read_u64_le() {
        let data = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(read_u64_le(&data, 0), Some(1));
        assert_eq!(read_u64_le(&[0x00], 0), None);
    }

    #[test]
    fn test_parse_elf_too_small() {
        let result = parse_elf(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_elf_bad_magic() {
        let mut data = [0u8; 128];
        data[0] = 0x00; // Wrong magic
        let result = parse_elf(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_elf_not_64bit() {
        let mut data = [0u8; 128];
        data[0..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = 1; // 32-bit
        data[EI_DATA] = ELFDATA2LSB;
        let result = parse_elf(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_elf_valid_minimal() {
        // Construct a minimal valid ELF64 header
        let mut data = vec![0u8; 256];
        data[0..4].copy_from_slice(&ELFMAG);
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = ELFDATA2LSB;
        // e_type = ET_EXEC
        data[16] = 2; data[17] = 0;
        // e_machine = EM_X86_64
        data[18] = 62; data[19] = 0;
        // e_entry = 0x400000
        data[24] = 0x00; data[25] = 0x00; data[26] = 0x40; data[27] = 0x00;
        // Leave phoff, shoff as 0 (no sections/segments)

        let result = parse_elf(&data);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.header.e_type, ET_EXEC);
        assert_eq!(info.header.e_machine, EM_X86_64);
        assert_eq!(info.entry_point, 0x400000);
    }

    // ---- Breakpoint management ----

    #[test]
    fn test_breakpoint_new() {
        let bp = Breakpoint::new(1, 0x400000);
        assert_eq!(bp.id, 1);
        assert_eq!(bp.address, 0x400000);
        assert!(bp.enabled);
        assert_eq!(bp.hit_count, 0);
        assert_eq!(bp.kind, BreakpointKind::Software);
    }

    #[test]
    fn test_debugger_set_breakpoint() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.set_breakpoint(b"0x400000", out);
        });
        assert!(!out.is_empty());
        assert_eq!(dbg.breakpoints.len(), 1);
        assert_eq!(dbg.breakpoints[0].address, 0x400000);
    }

    #[test]
    fn test_debugger_delete_breakpoint() {
        let mut dbg = Debugger::new();
        let _ = capture(|out| { dbg.set_breakpoint(b"0x400000", out); });
        let _ = capture(|out| { dbg.set_breakpoint(b"0x400100", out); });
        assert_eq!(dbg.breakpoints.len(), 2);

        let _ = capture(|out| { dbg.delete_breakpoint(1, out); });
        assert_eq!(dbg.breakpoints.len(), 1);
        assert_eq!(dbg.breakpoints[0].id, 2);
    }

    #[test]
    fn test_debugger_delete_all_breakpoints() {
        let mut dbg = Debugger::new();
        let _ = capture(|out| { dbg.set_breakpoint(b"0x400000", out); });
        let _ = capture(|out| { dbg.set_breakpoint(b"0x400100", out); });

        let out = capture(|out| { dbg.delete_breakpoint(0, out); });
        assert!(dbg.breakpoints.is_empty());
        assert!(starts_with(&out, b"Deleted 2"));
    }

    #[test]
    fn test_debugger_info_breakpoints_empty() {
        let dbg = Debugger::new();
        let out = capture(|out| { dbg.info_breakpoints(out); });
        assert_eq!(out, b"No breakpoints.\n");
    }

    #[test]
    fn test_debugger_info_breakpoints_with_entries() {
        let mut dbg = Debugger::new();
        let _ = capture(|out| { dbg.set_breakpoint(b"0x400000", out); });
        let out = capture(|out| { dbg.info_breakpoints(out); });
        assert!(starts_with(&out, b"Num"));
        // Should contain the breakpoint address
        assert!(out.windows(8).any(|w| w == b"00400000"));
    }

    // ---- Watchpoint management ----

    #[test]
    fn test_watchpoint_new() {
        let wp = Watchpoint::new(1, 0x600000, 8);
        assert_eq!(wp.id, 1);
        assert_eq!(wp.address, 0x600000);
        assert_eq!(wp.size, 8);
        assert!(wp.enabled);
    }

    #[test]
    fn test_debugger_set_watchpoint() {
        let mut dbg = Debugger::new();
        let out = capture(|out| { dbg.cmd_watch(b"0x600000", out); });
        assert!(!out.is_empty());
        assert_eq!(dbg.watchpoints.len(), 1);
    }

    // ---- Register display ----

    #[test]
    fn test_find_register_index() {
        assert_eq!(find_register_index(b"rax"), Some(REG_RAX));
        assert_eq!(find_register_index(b"rip"), Some(REG_RIP));
        assert_eq!(find_register_index(b"rflags"), Some(REG_RFLAGS));
        assert_eq!(find_register_index(b"gs"), Some(REG_GS));
        assert_eq!(find_register_index(b"invalid"), None);
    }

    #[test]
    fn test_debugger_info_registers() {
        let mut dbg = Debugger::new();
        dbg.regs[REG_RAX] = 0xdeadbeef;
        dbg.regs[REG_RIP] = 0x400000;
        let out = capture(|out| { dbg.info_registers(out); });
        // Should contain "rax" and "rip"
        assert!(out.windows(3).any(|w| w == b"rax"));
        assert!(out.windows(3).any(|w| w == b"rip"));
    }

    // ---- Thread management ----

    #[test]
    fn test_thread_info_new() {
        let t = ThreadInfo::new(42);
        assert_eq!(t.tid, 42);
        assert_eq!(t.state, ThreadState::Stopped);
    }

    #[test]
    fn test_debugger_info_threads_empty() {
        let dbg = Debugger::new();
        let out = capture(|out| { dbg.info_threads(out); });
        assert_eq!(out, b"No threads.\n");
    }

    #[test]
    fn test_debugger_info_threads_with_entries() {
        let mut dbg = Debugger::new();
        dbg.threads.push(ThreadInfo::new(1));
        let out = capture(|out| { dbg.info_threads(out); });
        assert!(starts_with(&out, b"  Id"));
    }

    // ---- Expression evaluator ----

    #[test]
    fn test_eval_simple_number() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"42", &regs).unwrap(), 42);
    }

    #[test]
    fn test_eval_hex_number() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"0xff", &regs).unwrap(), 255);
    }

    #[test]
    fn test_eval_addition() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"10 + 20", &regs).unwrap(), 30);
    }

    #[test]
    fn test_eval_subtraction() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"50 - 20", &regs).unwrap(), 30);
    }

    #[test]
    fn test_eval_multiplication() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"6 * 7", &regs).unwrap(), 42);
    }

    #[test]
    fn test_eval_division() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"100 / 10", &regs).unwrap(), 10);
    }

    #[test]
    fn test_eval_division_by_zero() {
        let regs = [0u64; REG_COUNT];
        assert!(eval_expr(b"1 / 0", &regs).is_err());
    }

    #[test]
    fn test_eval_modulo() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"17 % 5", &regs).unwrap(), 2);
    }

    #[test]
    fn test_eval_parentheses() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"(2 + 3) * 4", &regs).unwrap(), 20);
    }

    #[test]
    fn test_eval_bitwise_and() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"0xff & 0x0f", &regs).unwrap(), 0x0f);
    }

    #[test]
    fn test_eval_bitwise_or() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"0xf0 | 0x0f", &regs).unwrap(), 0xff);
    }

    #[test]
    fn test_eval_bitwise_xor() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"0xff ^ 0x0f", &regs).unwrap(), 0xf0);
    }

    #[test]
    fn test_eval_bitwise_not() {
        let regs = [0u64; REG_COUNT];
        let result = eval_expr(b"~0", &regs).unwrap();
        assert_eq!(result, -1); // ~0 = -1 in two's complement
    }

    #[test]
    fn test_eval_shift_left() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"1 << 8", &regs).unwrap(), 256);
    }

    #[test]
    fn test_eval_shift_right() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"256 >> 4", &regs).unwrap(), 16);
    }

    #[test]
    fn test_eval_unary_minus() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"-42", &regs).unwrap(), -42);
    }

    #[test]
    fn test_eval_register() {
        let mut regs = [0u64; REG_COUNT];
        regs[REG_RAX] = 100;
        assert_eq!(eval_expr(b"$rax", &regs).unwrap(), 100);
    }

    #[test]
    fn test_eval_register_expr() {
        let mut regs = [0u64; REG_COUNT];
        regs[REG_RAX] = 100;
        regs[REG_RBX] = 50;
        assert_eq!(eval_expr(b"$rax + $rbx", &regs).unwrap(), 150);
    }

    #[test]
    fn test_eval_complex() {
        let regs = [0u64; REG_COUNT];
        assert_eq!(eval_expr(b"(10 + 5) * 2 - 3", &regs).unwrap(), 27);
    }

    #[test]
    fn test_eval_precedence() {
        let regs = [0u64; REG_COUNT];
        // Multiplication binds tighter than addition
        assert_eq!(eval_expr(b"2 + 3 * 4", &regs).unwrap(), 14);
    }

    // ---- Disassembler ----

    #[test]
    fn test_disasm_nop() {
        let code = [0x90];
        let inst = disasm_one(&code, 0);
        assert_eq!(&inst.text[..inst.text_len], b"nop");
        assert_eq!(inst.len, 1);
    }

    #[test]
    fn test_disasm_ret() {
        let code = [0xc3];
        let inst = disasm_one(&code, 0);
        assert_eq!(&inst.text[..inst.text_len], b"ret");
    }

    #[test]
    fn test_disasm_int3() {
        let code = [0xcc];
        let inst = disasm_one(&code, 0);
        assert_eq!(&inst.text[..inst.text_len], b"int3");
    }

    #[test]
    fn test_disasm_hlt() {
        let code = [0xf4];
        let inst = disasm_one(&code, 0);
        assert_eq!(&inst.text[..inst.text_len], b"hlt");
    }

    #[test]
    fn test_disasm_push_rax() {
        let code = [0x50]; // push rax (no REX prefix needed in 64-bit mode)
        let inst = disasm_one(&code, 0);
        assert!(starts_with(&inst.text[..inst.text_len], b"push"));
    }

    #[test]
    fn test_disasm_syscall() {
        let code = [0x0f, 0x05];
        let inst = disasm_one(&code, 0);
        assert_eq!(&inst.text[..inst.text_len], b"syscall");
    }

    #[test]
    fn test_disasm_call_rel32() {
        let code = [0xe8, 0x10, 0x00, 0x00, 0x00]; // call +0x10
        let inst = disasm_one(&code, 0x1000);
        assert!(starts_with(&inst.text[..inst.text_len], b"call"));
        assert_eq!(inst.len, 5);
    }

    #[test]
    fn test_disasm_jmp_rel32() {
        let code = [0xe9, 0x20, 0x00, 0x00, 0x00];
        let inst = disasm_one(&code, 0x2000);
        assert!(starts_with(&inst.text[..inst.text_len], b"jmp"));
        assert_eq!(inst.len, 5);
    }

    #[test]
    fn test_disasm_jmp_rel8() {
        let code = [0xeb, 0x10];
        let inst = disasm_one(&code, 0x3000);
        assert!(starts_with(&inst.text[..inst.text_len], b"jmp"));
        assert_eq!(inst.len, 2);
    }

    #[test]
    fn test_disasm_leave() {
        let code = [0xc9];
        let inst = disasm_one(&code, 0);
        assert_eq!(&inst.text[..inst.text_len], b"leave");
    }

    #[test]
    fn test_disasm_empty() {
        let code: [u8; 0] = [];
        let inst = disasm_one(&code, 0);
        assert_eq!(inst.len, 0);
    }

    // ---- Memory examination ----

    #[test]
    fn test_parse_examine_fmt_default() {
        let (count, fmt) = parse_examine_fmt(b"");
        assert_eq!(count, 1);
        assert_eq!(fmt, ExamineFormat::Word);
    }

    #[test]
    fn test_parse_examine_fmt_count_and_format() {
        let (count, fmt) = parse_examine_fmt(b"4b");
        assert_eq!(count, 4);
        assert_eq!(fmt, ExamineFormat::Byte);
    }

    #[test]
    fn test_parse_examine_fmt_giant() {
        let (count, fmt) = parse_examine_fmt(b"2g");
        assert_eq!(count, 2);
        assert_eq!(fmt, ExamineFormat::Giant);
    }

    #[test]
    fn test_parse_examine_fmt_string() {
        let (count, fmt) = parse_examine_fmt(b"1s");
        assert_eq!(count, 1);
        assert_eq!(fmt, ExamineFormat::StringZ);
    }

    #[test]
    fn test_parse_examine_fmt_instruction() {
        let (count, fmt) = parse_examine_fmt(b"10i");
        assert_eq!(count, 10);
        assert_eq!(fmt, ExamineFormat::Instruction);
    }

    #[test]
    fn test_format_examine_bytes() {
        let data = [0xde, 0xad, 0xbe, 0xef];
        let out = capture(|out| {
            format_examine(out, &data, 0x1000, 4, ExamineFormat::Byte);
        });
        assert!(!out.is_empty());
        // Should contain the address
        assert!(out.windows(4).any(|w| w == b"1000"));
    }

    #[test]
    fn test_format_examine_word() {
        let data = [0x78, 0x56, 0x34, 0x12];
        let out = capture(|out| {
            format_examine(out, &data, 0x2000, 1, ExamineFormat::Word);
        });
        assert!(!out.is_empty());
    }

    #[test]
    fn test_format_examine_string() {
        let data = b"hello\0";
        let out = capture(|out| {
            format_examine(out, data, 0x3000, 1, ExamineFormat::StringZ);
        });
        assert!(out.windows(5).any(|w| w == b"hello"));
    }

    // ---- GDB Remote Protocol ----

    #[test]
    fn test_gdb_checksum() {
        assert_eq!(gdb_checksum(b"OK"), b'O'.wrapping_add(b'K'));
    }

    #[test]
    fn test_gdb_packet() {
        let mut buf = [0u8; 64];
        let len = gdb_packet(b"OK", &mut buf);
        assert!(len > 0);
        assert_eq!(buf[0], b'$');
        assert_eq!(buf[1], b'O');
        assert_eq!(buf[2], b'K');
        assert_eq!(buf[3], b'#');
    }

    #[test]
    fn test_gdb_parse_packet() {
        let csum = gdb_checksum(b"OK");
        let hex_chars = b"0123456789abcdef";
        let mut pkt = Vec::new();
        pkt.push(b'$');
        pkt.extend_from_slice(b"OK");
        pkt.push(b'#');
        pkt.push(hex_chars[(csum >> 4) as usize]);
        pkt.push(hex_chars[(csum & 0xf) as usize]);

        let (payload, consumed) = gdb_parse_packet(&pkt).unwrap();
        assert_eq!(payload, b"OK");
        assert_eq!(consumed, 6);
    }

    #[test]
    fn test_gdb_parse_packet_invalid() {
        assert!(gdb_parse_packet(b"garbage").is_none());
    }

    #[test]
    fn test_gdb_encode_registers() {
        let mut regs = [0u64; REG_COUNT];
        regs[REG_RAX] = 1;
        let mut buf = [0u8; 512];
        let len = gdb_encode_registers(&regs, &mut buf);
        assert!(len > 0);
        // RAX = 1 should encode as "0100000000000000" (little-endian bytes)
        assert_eq!(&buf[0..16], b"0100000000000000");
    }

    #[test]
    fn test_gdb_handle_halt_reason() {
        let regs = [0u64; REG_COUNT];
        let elf: Option<ElfInfo> = None;
        let mut response = [0u8; 256];
        let len = gdb_handle_command(b"?", &regs, &elf, &mut response);
        // Should contain S05 (SIGTRAP)
        let resp = &response[..len];
        assert!(resp.windows(3).any(|w| w == b"S05"));
    }

    #[test]
    fn test_gdb_handle_read_registers() {
        let regs = [0u64; REG_COUNT];
        let elf: Option<ElfInfo> = None;
        let mut response = [0u8; 1024];
        let len = gdb_handle_command(b"g", &regs, &elf, &mut response);
        assert!(len > 0);
    }

    #[test]
    fn test_gdb_handle_continue() {
        let regs = [0u64; REG_COUNT];
        let elf: Option<ElfInfo> = None;
        let mut response = [0u8; 256];
        let len = gdb_handle_command(b"c", &regs, &elf, &mut response);
        assert!(len > 0);
    }

    #[test]
    fn test_gdb_handle_step() {
        let regs = [0u64; REG_COUNT];
        let elf: Option<ElfInfo> = None;
        let mut response = [0u8; 256];
        let len = gdb_handle_command(b"s", &regs, &elf, &mut response);
        assert!(len > 0);
    }

    #[test]
    fn test_gdb_handle_supported() {
        let regs = [0u64; REG_COUNT];
        let elf: Option<ElfInfo> = None;
        let mut response = [0u8; 256];
        let len = gdb_handle_command(b"qSupported", &regs, &elf, &mut response);
        let resp = &response[..len];
        assert!(resp.windows(10).any(|w| w == b"PacketSize"));
    }

    #[test]
    fn test_gdb_handle_kill() {
        let regs = [0u64; REG_COUNT];
        let elf: Option<ElfInfo> = None;
        let mut response = [0u8; 256];
        let len = gdb_handle_command(b"k", &regs, &elf, &mut response);
        assert!(len > 0);
    }

    #[test]
    fn test_gdb_handle_unknown() {
        let regs = [0u64; REG_COUNT];
        let elf: Option<ElfInfo> = None;
        let mut response = [0u8; 256];
        let len = gdb_handle_command(b"Z", &regs, &elf, &mut response);
        assert!(len > 0);
    }

    // ---- Personality detection ----

    #[test]
    fn test_detect_personality_gdb() {
        assert_eq!(detect_personality(b"/usr/bin/gdb"), Personality::Gdb);
        assert_eq!(detect_personality(b"gdb"), Personality::Gdb);
        assert_eq!(detect_personality(b"gdb.exe"), Personality::Gdb);
    }

    #[test]
    fn test_detect_personality_gdbserver() {
        assert_eq!(detect_personality(b"/usr/bin/gdbserver"), Personality::GdbServer);
        assert_eq!(detect_personality(b"gdbserver"), Personality::GdbServer);
        assert_eq!(detect_personality(b"gdbserver.exe"), Personality::GdbServer);
    }

    // ---- Debugger commands ----

    #[test]
    fn test_process_command_quit() {
        let mut dbg = Debugger::new();
        let mut out = Vec::new();
        assert!(!dbg.process_command(b"quit", &mut out));
    }

    #[test]
    fn test_process_command_q() {
        let mut dbg = Debugger::new();
        let mut out = Vec::new();
        assert!(!dbg.process_command(b"q", &mut out));
    }

    #[test]
    fn test_process_command_empty() {
        let mut dbg = Debugger::new();
        let mut out = Vec::new();
        assert!(dbg.process_command(b"", &mut out));
    }

    #[test]
    fn test_process_command_unknown() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.process_command(b"foobar", out);
        });
        assert!(out.windows(9).any(|w| w == b"Undefined"));
    }

    #[test]
    fn test_process_command_help() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.process_command(b"help", out);
        });
        assert!(out.windows(4).any(|w| w == b"List"));
    }

    #[test]
    fn test_process_command_run_no_binary() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.process_command(b"run", out);
        });
        assert!(out.windows(13).any(|w| w == b"No executable"));
    }

    #[test]
    fn test_cmd_print_number() {
        let dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_print(b"42", out);
        });
        assert!(out.windows(2).any(|w| w == b"42"));
    }

    #[test]
    fn test_cmd_print_register() {
        let mut dbg = Debugger::new();
        dbg.regs[REG_RAX] = 0xbeef;
        let out = capture(|out| {
            dbg.cmd_print(b"$rax", out);
        });
        assert!(out.windows(4).any(|w| w == b"beef"));
    }

    #[test]
    fn test_cmd_print_empty() {
        let dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_print(b"", out);
        });
        assert!(out.windows(8).any(|w| w == b"Argument"));
    }

    #[test]
    fn test_cmd_set_register() {
        let mut dbg = Debugger::new();
        let _ = capture(|out| {
            dbg.cmd_set(b"$rax = 0xff", out);
        });
        assert_eq!(dbg.regs[REG_RAX], 0xff);
    }

    #[test]
    fn test_cmd_set_variable() {
        let mut dbg = Debugger::new();
        let _ = capture(|out| {
            dbg.cmd_set(b"myvar = 42", out);
        });
        assert_eq!(dbg.vars.len(), 1);
        assert_eq!(dbg.vars[0].value, 42);
    }

    #[test]
    fn test_cmd_set_no_equals() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_set(b"nosyntax", out);
        });
        assert!(out.windows(5).any(|w| w == b"Usage"));
    }

    #[test]
    fn test_cmd_continue_not_running() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_continue(out);
        });
        assert!(out.windows(7).any(|w| w == b"not bei"));
    }

    #[test]
    fn test_cmd_step_not_running() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_step(out);
        });
        assert!(out.windows(7).any(|w| w == b"not bei"));
    }

    #[test]
    fn test_cmd_next_not_running() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_next(out);
        });
        assert!(out.windows(7).any(|w| w == b"not bei"));
    }

    #[test]
    fn test_cmd_examine_no_binary() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_examine(b"0x400000", out);
        });
        assert!(out.windows(7).any(|w| w == b"No file"));
    }

    #[test]
    fn test_cmd_disassemble_no_binary() {
        let mut dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_disassemble(b"", out);
        });
        assert!(out.windows(7).any(|w| w == b"No file"));
    }

    #[test]
    fn test_backtrace_no_elf() {
        let mut dbg = Debugger::new();
        dbg.regs[REG_RIP] = 0x400000;
        let out = capture(|out| {
            dbg.backtrace(out);
        });
        // Should show at least frame #0
        assert!(out.windows(2).any(|w| w == b"#0"));
    }

    #[test]
    fn test_resolve_location_hex() {
        let dbg = Debugger::new();
        assert_eq!(dbg.resolve_location(b"0x400000"), Some(0x400000));
    }

    #[test]
    fn test_resolve_location_decimal() {
        let dbg = Debugger::new();
        assert_eq!(dbg.resolve_location(b"12345"), Some(12345));
    }

    #[test]
    fn test_resolve_location_star() {
        let dbg = Debugger::new();
        assert_eq!(dbg.resolve_location(b"*0x400000"), Some(0x400000));
    }

    #[test]
    fn test_resolve_location_empty() {
        let dbg = Debugger::new();
        assert_eq!(dbg.resolve_location(b""), None);
    }

    // ---- ModR/M length calculation ----

    #[test]
    fn test_modrm_len_reg_direct() {
        // mod=11 (register direct) - just the ModR/M byte
        assert_eq!(modrm_len(&[0xc0], 0), 1);
    }

    #[test]
    fn test_modrm_len_disp8() {
        // mod=01 rm=000 - ModR/M + disp8
        assert_eq!(modrm_len(&[0x40], 0), 2);
    }

    #[test]
    fn test_modrm_len_disp32() {
        // mod=10 rm=000 - ModR/M + disp32
        assert_eq!(modrm_len(&[0x80], 0), 5);
    }

    #[test]
    fn test_modrm_len_rip_relative() {
        // mod=00 rm=101 - RIP-relative + disp32
        assert_eq!(modrm_len(&[0x05], 0), 5);
    }

    // ---- Help command variants ----

    #[test]
    fn test_help_breakpoints() {
        let dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_help(b"break", out);
        });
        assert!(out.windows(5).any(|w| w == b"break"));
    }

    #[test]
    fn test_help_examine() {
        let dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_help(b"x", out);
        });
        assert!(out.windows(6).any(|w| w == b"FORMAT"));
    }

    #[test]
    fn test_help_unknown_topic() {
        let dbg = Debugger::new();
        let out = capture(|out| {
            dbg.cmd_help(b"nonexistent", out);
        });
        assert!(out.windows(7).any(|w| w == b"No help"));
    }
}
