//! OurOS zip/unzip archive utility.
//!
//! Multi-personality binary: detects operating mode from `argv[0]`.
//!
//! # Modes
//!
//! - **zip**: create or update ZIP archives
//! - **unzip**: extract files from ZIP archives
//!
//! # Format
//!
//! Implements the classic ZIP format (PKWARE Application Note):
//! - Local file headers (signature 0x04034b50)
//! - Central directory entries (signature 0x02014b50)
//! - End of central directory record (signature 0x06054b50)
//! - Compression methods: Stored (0) and DEFLATE (8)
//! - CRC32 checksums with standard polynomial 0xEDB88320
//! - DEFLATE: LZ77 + fixed Huffman encoding; full decompression including
//!   stored, fixed, and dynamic Huffman blocks

#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    // Size/ratio values are only ever shown to the user as rounded
    // human-readable strings; f64 precision loss is immaterial here.
    clippy::cast_precision_loss,
    // star_pi/star_si and similar paired indices are clearer kept parallel.
    clippy::similar_names,
    // Table-header rows pass column labels as positional args; inlining the
    // literals would break alignment with the width-specified data columns.
    clippy::print_literal,
    clippy::wildcard_imports,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::module_name_repetitions,
    clippy::struct_excessive_bools,
    clippy::doc_markdown,
)]

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::SystemTime;

// Note: all I/O goes through std (std::fs / std::io / std::process), which
// reaches native OurOS syscalls via the posix libc layer.  A previous
// hand-rolled syscall stub here hardcoded Linux numbers (WRITE=1=SYS_EXIT,
// OPEN=2=SYS_TASK_ID, EXIT=60=SYS_SYSCTL_GET, ...) that collide with
// unrelated native syscalls; it was dead code and has been removed.

// ============================================================================
// CRC32 (polynomial 0xEDB88320 — same as PKZIP standard)
// ============================================================================

const fn make_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = make_crc32_table();

/// Update a running CRC32 with a slice of bytes.
fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        let idx = ((c ^ u32::from(b)) & 0xFF) as usize;
        c = CRC32_TABLE[idx] ^ (c >> 8);
    }
    !c
}

/// Compute the CRC32 of a complete byte slice.
fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

// ============================================================================
// DOS date/time encoding (ZIP uses MS-DOS date/time fields)
// ============================================================================

/// Encode a `SystemTime` into a DOS date (high 16 bits) and DOS time (low 16 bits).
///
/// DOS date: bits 15-9 = year-1980, bits 8-5 = month (1-12), bits 4-0 = day (1-31)
/// DOS time: bits 15-11 = hours (0-23), bits 10-5 = minutes (0-59), bits 4-0 = seconds/2
fn encode_dos_datetime(t: SystemTime) -> (u16, u16) {
    // Fall back to a fixed timestamp on error: 1980-01-01 00:00:00
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    // Convert Unix epoch seconds to a rough calendar date.
    // We use a simple Gregorian calendar calculation (no leap-second awareness).
    let (year, month, day, hour, minute, second) = unix_secs_to_datetime(secs);

    let dos_year = year.saturating_sub(1980).min(127) as u16;
    let dos_date = (dos_year << 9) | ((month as u16) << 5) | (day as u16);
    let dos_time = ((hour as u16) << 11) | ((minute as u16) << 5) | ((second / 2) as u16);
    (dos_date, dos_time)
}

/// Convert Unix epoch seconds to (year, month, day, hour, minute, second).
fn unix_secs_to_datetime(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let second = (secs % 60) as u32;
    let minutes_total = secs / 60;
    let minute = (minutes_total % 60) as u32;
    let hours_total = minutes_total / 60;
    let hour = (hours_total % 24) as u32;
    let days_total = hours_total / 24;

    // Gregorian calendar from days since 1970-01-01
    // Using algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days_total as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };

    (yr as u32, m as u32, d as u32, hour, minute, second)
}

// ============================================================================
// Constants
// ============================================================================

/// Local file header signature.
const SIG_LOCAL: u32 = 0x0403_4B50;
/// Central directory file header signature.
const SIG_CENTRAL: u32 = 0x0201_4B50;
/// End of central directory signature.
const SIG_EOCD: u32 = 0x0605_4B50;

/// Compression method: stored (no compression).
const METHOD_STORED: u16 = 0;
/// Compression method: deflated.
const METHOD_DEFLATE: u16 = 8;

/// Version needed to extract: 2.0 (for DEFLATE).
const VERSION_NEEDED_DEFLATE: u16 = 20;
/// Version needed to extract: 1.0 (for stored).
const VERSION_NEEDED_STORED: u16 = 10;
/// Version made by: Unix/MS-DOS compatible at spec 2.0.
const VERSION_MADE_BY: u16 = 0x0314; // 3 = Unix, 20 = version 2.0

/// General purpose bit flag: data descriptor present.
#[allow(dead_code)]
const GP_DATA_DESCRIPTOR: u16 = 1 << 3;

// ============================================================================
// DEFLATE constants and tables
// ============================================================================

const MAX_DIST: usize = 32768;
const MAX_MATCH: usize = 258;
const MIN_MATCH: usize = 3;
const HASH_SIZE: usize = 65536;

/// (base_length, extra_bits) for length codes 257..=285.
const LENGTH_TABLE: [(u16, u8); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];

/// (base_distance, extra_bits) for distance codes 0..=29.
const DISTANCE_TABLE: [(u16, u8); 30] = [
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 1),
    (7, 1),
    (9, 2),
    (13, 2),
    (17, 3),
    (25, 3),
    (33, 4),
    (49, 4),
    (65, 5),
    (97, 5),
    (129, 6),
    (193, 6),
    (257, 7),
    (385, 7),
    (513, 8),
    (769, 8),
    (1025, 9),
    (1537, 9),
    (2049, 10),
    (3073, 10),
    (4097, 11),
    (6145, 11),
    (8193, 12),
    (12289, 12),
    (16385, 13),
    (24577, 13),
];

fn length_code(len: usize) -> usize {
    match len {
        3..=10 => len - 3,
        11..=12 => 8,
        13..=14 => 9,
        15..=16 => 10,
        17..=18 => 11,
        19..=22 => 12,
        23..=26 => 13,
        27..=30 => 14,
        31..=34 => 15,
        35..=42 => 16,
        43..=50 => 17,
        51..=58 => 18,
        59..=66 => 19,
        67..=82 => 20,
        83..=98 => 21,
        99..=114 => 22,
        115..=130 => 23,
        131..=162 => 24,
        163..=194 => 25,
        195..=226 => 26,
        227..=257 => 27,
        _ => 28, // 258 and above → code 285
    }
}

fn distance_code(dist: usize) -> usize {
    match dist {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        5..=6 => 4,
        7..=8 => 5,
        9..=12 => 6,
        13..=16 => 7,
        17..=24 => 8,
        25..=32 => 9,
        33..=48 => 10,
        49..=64 => 11,
        65..=96 => 12,
        97..=128 => 13,
        129..=192 => 14,
        193..=256 => 15,
        257..=384 => 16,
        385..=512 => 17,
        513..=768 => 18,
        769..=1024 => 19,
        1025..=1536 => 20,
        1537..=2048 => 21,
        2049..=3072 => 22,
        3073..=4096 => 23,
        4097..=6144 => 24,
        6145..=8192 => 25,
        8193..=12288 => 26,
        12289..=16384 => 27,
        16385..=24576 => 28,
        _ => 29,
    }
}

/// Encode a literal/length symbol with fixed Huffman codes (RFC 1951 §3.2.6).
/// Returns (code_value, bit_count). Code value is MSB-first; caller reverses bits.
fn fixed_litlen_encode(sym: u16) -> (u32, u32) {
    match sym {
        0..=143 => (0b0011_0000 + u32::from(sym), 8),
        144..=255 => (0b1_1001_0000 + u32::from(sym) - 144, 9),
        256..=279 => (u32::from(sym) - 256, 7),
        280..=287 => (0b1100_0000 + u32::from(sym) - 280, 8),
        _ => (0, 0),
    }
}

fn reverse_bits(v: u32, n: u32) -> u32 {
    let mut r = 0u32;
    let mut val = v;
    for _ in 0..n {
        r = (r << 1) | (val & 1);
        val >>= 1;
    }
    r
}

// ============================================================================
// Bit writer (LSB-first, as DEFLATE requires)
// ============================================================================

struct BitWriter<W: Write> {
    inner: W,
    buf: u64,
    bits: u32,
}

impl<W: Write> BitWriter<W> {
    fn new(inner: W) -> Self {
        Self { inner, buf: 0, bits: 0 }
    }

    fn write_bits(&mut self, val: u32, n: u32) -> io::Result<()> {
        self.buf |= u64::from(val) << self.bits;
        self.bits += n;
        while self.bits >= 8 {
            self.inner.write_all(&[(self.buf & 0xFF) as u8])?;
            self.buf >>= 8;
            self.bits -= 8;
        }
        Ok(())
    }

    fn finish(mut self) -> io::Result<W> {
        if self.bits > 0 {
            self.inner.write_all(&[(self.buf & 0xFF) as u8])?;
        }
        Ok(self.inner)
    }
}

// ============================================================================
// Bit reader (LSB-first)
// ============================================================================

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    bits: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, buf: 0, bits: 0 }
    }

    fn refill(&mut self) -> Result<(), String> {
        if self.pos >= self.data.len() {
            return Err("deflate: unexpected end of input".to_string());
        }
        self.buf |= u32::from(self.data[self.pos]) << self.bits;
        self.pos += 1;
        self.bits += 8;
        Ok(())
    }

    fn read_bits(&mut self, n: u32) -> Result<u32, String> {
        while self.bits < n {
            self.refill()?;
        }
        let val = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.bits -= n;
        Ok(val)
    }

    fn align_to_byte(&mut self) {
        let waste = self.bits % 8;
        self.buf >>= waste;
        self.bits -= waste;
    }

    fn read_u16_le_aligned(&mut self) -> Result<u16, String> {
        self.align_to_byte();
        let lo = self.read_bits(8)?;
        let hi = self.read_bits(8)?;
        Ok((lo | (hi << 8)) as u16)
    }
}

// ============================================================================
// Dynamic Huffman decoder
// ============================================================================

struct HuffmanTable {
    counts: [u16; 16],
    symbols: Vec<u16>,
    first_code: [u32; 16],
    first_sym: [u32; 16],
    max_len: u32,
}

impl HuffmanTable {
    fn from_lengths(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 0 {
                counts[l as usize] = counts[l as usize]
                    .checked_add(1)
                    .ok_or_else(|| "huffman: too many symbols at one length".to_string())?;
            }
        }

        let mut first_code = [0u32; 16];
        let mut code: u32 = 0;
        for bits in 1..16 {
            code = (code + u32::from(counts[bits - 1])) << 1;
            first_code[bits] = code;
        }

        let mut first_sym = [0u32; 16];
        let mut pos: u32 = 0;
        for bits in 1..16 {
            first_sym[bits] = pos;
            pos += u32::from(counts[bits]);
        }
        let total_syms = pos as usize;

        let mut symbols = vec![0u16; total_syms];
        let mut offsets = first_sym;
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                let idx = offsets[l as usize] as usize;
                if idx < total_syms {
                    symbols[idx] = sym as u16;
                }
                offsets[l as usize] += 1;
            }
        }

        let max_len = lengths.iter().copied().fold(0u8, u8::max) as u32;

        Ok(HuffmanTable { counts, symbols, first_code, first_sym, max_len })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, String> {
        let mut code: u32 = 0;
        for bits in 1..=self.max_len {
            let b = reader.read_bits(1)?;
            code = (code << 1) | b;
            let count = u32::from(self.counts[bits as usize]);
            let f = self.first_code[bits as usize];
            if count > 0 && code >= f && code < f + count {
                let idx = (self.first_sym[bits as usize] + (code - f)) as usize;
                return self.symbols.get(idx).copied().ok_or_else(|| {
                    "huffman: symbol index out of range".to_string()
                });
            }
        }
        Err("huffman: no symbol found".to_string())
    }
}

fn fixed_litlen_table() -> Result<HuffmanTable, String> {
    let mut lengths = [0u8; 288];
    for item in &mut lengths[0..=143] { *item = 8; }
    for item in &mut lengths[144..=255] { *item = 9; }
    for item in &mut lengths[256..=279] { *item = 7; }
    for item in &mut lengths[280..=287] { *item = 8; }
    HuffmanTable::from_lengths(&lengths)
}

fn fixed_dist_table() -> Result<HuffmanTable, String> {
    HuffmanTable::from_lengths(&[5u8; 32])
}

// ============================================================================
// DEFLATE decompressor
// ============================================================================

fn deflate_decompress(reader: &mut BitReader<'_>, output: &mut Vec<u8>) -> Result<(), String> {
    loop {
        let bfinal = reader.read_bits(1).map_err(|e| format!("deflate: BFINAL: {e}"))?;
        let btype = reader.read_bits(2).map_err(|e| format!("deflate: BTYPE: {e}"))?;

        match btype {
            0 => {
                // Stored block.
                let len = reader.read_u16_le_aligned()
                    .map_err(|e| format!("deflate: stored LEN: {e}"))? as usize;
                let nlen = reader.read_u16_le_aligned()
                    .map_err(|e| format!("deflate: stored NLEN: {e}"))? as usize;
                if (len ^ nlen) != 0xFFFF {
                    return Err(format!(
                        "deflate: stored block LEN/NLEN mismatch ({len:#x} ^ {nlen:#x})"
                    ));
                }
                // Drain buffered bits, then copy from raw slice.
                let start = output.len();
                output.resize(start + len, 0);
                let mut i = start;
                while reader.bits >= 8 && i < start + len {
                    output[i] = (reader.buf & 0xFF) as u8;
                    reader.buf >>= 8;
                    reader.bits -= 8;
                    i += 1;
                }
                let remaining = start + len - i;
                if remaining > 0 {
                    let end = reader.pos + remaining;
                    if end > reader.data.len() {
                        return Err("deflate: stored block data truncated".to_string());
                    }
                    output[i..start + len].copy_from_slice(&reader.data[reader.pos..end]);
                    reader.pos = end;
                }
            }
            1 => {
                let litlen = fixed_litlen_table()
                    .map_err(|e| format!("deflate: fixed litlen table: {e}"))?;
                let dist = fixed_dist_table()
                    .map_err(|e| format!("deflate: fixed dist table: {e}"))?;
                decode_huffman_block(reader, output, &litlen, &dist)
                    .map_err(|e| format!("deflate: fixed block: {e}"))?;
            }
            2 => {
                let (litlen, dist) = decode_dynamic_headers(reader)
                    .map_err(|e| format!("deflate: dynamic header: {e}"))?;
                decode_huffman_block(reader, output, &litlen, &dist)
                    .map_err(|e| format!("deflate: dynamic block: {e}"))?;
            }
            _ => return Err(format!("deflate: reserved BTYPE {btype}")),
        }

        if bfinal == 1 {
            break;
        }
    }
    Ok(())
}

fn decode_huffman_block(
    reader: &mut BitReader<'_>,
    output: &mut Vec<u8>,
    litlen: &HuffmanTable,
    dist: &HuffmanTable,
) -> Result<(), String> {
    loop {
        let sym = litlen.decode(reader)?;
        match sym {
            0..=255 => output.push(sym as u8),
            256 => break, // end of block
            257..=285 => {
                let lc = (sym - 257) as usize;
                if lc >= LENGTH_TABLE.len() {
                    return Err(format!("deflate: length code {lc} out of range"));
                }
                let (base_len, extra_bits) = LENGTH_TABLE[lc];
                let extra = if extra_bits > 0 {
                    reader.read_bits(u32::from(extra_bits))? as u16
                } else {
                    0
                };
                let match_len = (base_len + extra) as usize;

                let dc = dist.decode(reader)? as usize;
                if dc >= DISTANCE_TABLE.len() {
                    return Err(format!("deflate: distance code {dc} out of range"));
                }
                let (base_dist, extra_dbits) = DISTANCE_TABLE[dc];
                let dist_extra = if extra_dbits > 0 {
                    reader.read_bits(u32::from(extra_dbits))? as u16
                } else {
                    0
                };
                let match_dist = (base_dist + dist_extra) as usize;

                if match_dist > output.len() {
                    return Err(format!(
                        "deflate: back-ref dist {match_dist} > output len {}",
                        output.len()
                    ));
                }
                let copy_start = output.len() - match_dist;
                for i in 0..match_len {
                    let b = output[copy_start + (i % match_dist)];
                    output.push(b);
                }
            }
            _ => return Err(format!("deflate: invalid litlen symbol {sym}")),
        }
    }
    Ok(())
}

fn decode_dynamic_headers(
    reader: &mut BitReader<'_>,
) -> Result<(HuffmanTable, HuffmanTable), String> {
    const CLEN_ORDER: [usize; 19] =
        [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

    let hlit = reader.read_bits(5)? as usize + 257;
    let hdist = reader.read_bits(5)? as usize + 1;
    let hclen = reader.read_bits(4)? as usize + 4;

    let mut clen_lengths = [0u8; 19];
    for i in 0..hclen {
        clen_lengths[CLEN_ORDER[i]] = reader.read_bits(3)? as u8;
    }
    let clen_table = HuffmanTable::from_lengths(&clen_lengths)?;

    let total = hlit + hdist;
    let mut lengths = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = clen_table.decode(reader)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err("deflate: RLE code 16 at start".to_string());
                }
                let prev = lengths[i - 1];
                let repeat = reader.read_bits(2)? as usize + 3;
                for _ in 0..repeat {
                    if i >= total { break; }
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 => {
                let repeat = reader.read_bits(3)? as usize + 3;
                for _ in 0..repeat {
                    if i >= total { break; }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            18 => {
                let repeat = reader.read_bits(7)? as usize + 11;
                for _ in 0..repeat {
                    if i >= total { break; }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            _ => return Err(format!("deflate: invalid clen symbol {sym}")),
        }
    }

    let litlen_table = HuffmanTable::from_lengths(&lengths[..hlit])?;
    let dist_table = HuffmanTable::from_lengths(&lengths[hlit..])?;
    Ok((litlen_table, dist_table))
}

// ============================================================================
// DEFLATE compressor (LZ77 + fixed Huffman)
// ============================================================================

#[derive(Clone, Copy)]
enum Token {
    Literal(u8),
    Match { dist: u16, len: u16 },
}

#[inline]
fn hash3(data: &[u8], pos: usize) -> usize {
    if pos + 2 >= data.len() {
        return 0;
    }
    let h = u32::from(data[pos])
        .wrapping_mul(2_654_435_761)
        ^ u32::from(data[pos + 1]).wrapping_mul(1_234_567)
        ^ u32::from(data[pos + 2]);
    (h as usize) & (HASH_SIZE - 1)
}

fn lz77_compress(input: &[u8], level: u8) -> Vec<Token> {
    let n = input.len();
    if n == 0 {
        return Vec::new();
    }

    let max_chain: usize = match level {
        1 => 4,
        2 => 8,
        3 => 16,
        4 => 32,
        5 => 64,
        6 => 128,
        7 => 256,
        8 => 512,
        _ => 1024,
    };

    let mut tokens = Vec::with_capacity(n);
    let mut head = vec![usize::MAX; HASH_SIZE];
    let mut prev = vec![usize::MAX; MAX_DIST];
    let mut pos = 0;

    while pos < n {
        if pos + MIN_MATCH > n {
            tokens.push(Token::Literal(input[pos]));
            pos += 1;
            continue;
        }

        let h = hash3(input, pos);
        let mut best_len = MIN_MATCH - 1;
        let mut best_dist = 0usize;
        let mut cur = head[h];
        let mut chain_len = 0;

        while cur != usize::MAX && chain_len < max_chain {
            let dist = pos.wrapping_sub(cur);
            if dist == 0 || dist > MAX_DIST { break; }
            let max_cmp = (n - pos).min(MAX_MATCH);
            let mut mlen = 0;
            while mlen < max_cmp && input[pos + mlen] == input[cur + mlen] {
                mlen += 1;
            }
            if mlen > best_len {
                best_len = mlen;
                best_dist = dist;
                if best_len >= MAX_MATCH { break; }
            }
            cur = prev[cur % MAX_DIST];
            chain_len += 1;
        }

        prev[pos % MAX_DIST] = head[h];
        head[h] = pos;

        if best_len >= MIN_MATCH {
            tokens.push(Token::Match { dist: best_dist as u16, len: best_len as u16 });
            for k in 1..best_len {
                if pos + k + MIN_MATCH <= n {
                    let hk = hash3(input, pos + k);
                    prev[(pos + k) % MAX_DIST] = head[hk];
                    head[hk] = pos + k;
                }
            }
            pos += best_len;
        } else {
            tokens.push(Token::Literal(input[pos]));
            pos += 1;
        }
    }

    tokens
}

/// Compress `input` into a raw DEFLATE stream (no gzip wrapper).
fn deflate_compress(input: &[u8], level: u8) -> Result<Vec<u8>, String> {
    if level == 0 {
        return Ok(deflate_compress_stored(input));
    }

    let tokens = lz77_compress(input, level);
    let mut output = Vec::new();
    let mut bw = BitWriter::new(&mut output);

    bw.write_bits(1, 1).map_err(|e| format!("deflate: BFINAL: {e}"))?;
    bw.write_bits(1, 2).map_err(|e| format!("deflate: BTYPE: {e}"))?;

    for token in &tokens {
        match *token {
            Token::Literal(b) => {
                let (bits, n) = fixed_litlen_encode(u16::from(b));
                bw.write_bits(reverse_bits(bits, n), n)
                    .map_err(|e| format!("deflate: literal: {e}"))?;
            }
            Token::Match { dist, len } => {
                let lc = length_code(len as usize);
                let (bits, n) = fixed_litlen_encode((lc + 257) as u16);
                bw.write_bits(reverse_bits(bits, n), n)
                    .map_err(|e| format!("deflate: len code: {e}"))?;

                let (base_len, extra_len_bits) = LENGTH_TABLE[lc];
                if extra_len_bits > 0 {
                    bw.write_bits(
                        u32::from(len) - u32::from(base_len),
                        u32::from(extra_len_bits),
                    )
                    .map_err(|e| format!("deflate: len extra: {e}"))?;
                }

                let dc = distance_code(dist as usize);
                bw.write_bits(reverse_bits(dc as u32, 5), 5)
                    .map_err(|e| format!("deflate: dist code: {e}"))?;

                let (base_dist, extra_dist_bits) = DISTANCE_TABLE[dc];
                if extra_dist_bits > 0 {
                    bw.write_bits(
                        u32::from(dist) - u32::from(base_dist),
                        u32::from(extra_dist_bits),
                    )
                    .map_err(|e| format!("deflate: dist extra: {e}"))?;
                }
            }
        }
    }

    // End-of-block symbol 256.
    let (bits, n) = fixed_litlen_encode(256);
    bw.write_bits(reverse_bits(bits, n), n)
        .map_err(|e| format!("deflate: EOB: {e}"))?;

    bw.finish().map_err(|e| format!("deflate: flush: {e}"))?;
    Ok(output)
}

fn deflate_compress_stored(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pos = 0;
    loop {
        let chunk_end = (pos + 65_535).min(input.len());
        let chunk = &input[pos..chunk_end];
        let len = chunk.len() as u16;
        let nlen = !len;
        let is_final = chunk_end == input.len();
        output.push(u8::from(is_final)); // BFINAL (BTYPE=00 stored)
        output.push((len & 0xFF) as u8);
        output.push((len >> 8) as u8);
        output.push((nlen & 0xFF) as u8);
        output.push((nlen >> 8) as u8);
        output.extend_from_slice(chunk);
        pos = chunk_end;
        if is_final { break; }
    }
    output
}

// ============================================================================
// ZIP archive structures
// ============================================================================

/// A single file entry in a ZIP archive.
#[derive(Debug, Clone)]
struct ZipEntry {
    /// File name (path within the archive, forward-slash separated).
    name: String,
    /// Compression method.
    method: u16,
    /// DOS modification date.
    mod_date: u16,
    /// DOS modification time.
    mod_time: u16,
    /// CRC32 of uncompressed data.
    crc32: u32,
    /// Compressed size in bytes.
    compressed_size: u32,
    /// Uncompressed size in bytes.
    uncompressed_size: u32,
    /// Offset of the local file header from the start of the archive.
    local_header_offset: u32,
    /// File comment (usually empty).
    comment: String,
    /// External file attributes (Unix permissions in high 16 bits).
    external_attrs: u32,
    /// Internal file attributes.
    internal_attrs: u16,
}

// ============================================================================
// ZIP reader
// ============================================================================

/// Parse all central directory entries from a ZIP archive byte slice.
fn zip_read_central_directory(data: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let eocd_offset = find_eocd(data)?;
    let eocd = &data[eocd_offset..];

    if eocd.len() < 22 {
        return Err("zip: EOCD too short".to_string());
    }

    let cd_count = u16::from_le_bytes([eocd[8], eocd[9]]) as usize;
    let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as usize;
    let cd_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as usize;

    if cd_offset + cd_size > data.len() {
        return Err(format!(
            "zip: central directory at offset {cd_offset} + size {cd_size} exceeds file length {}",
            data.len()
        ));
    }

    let mut entries = Vec::with_capacity(cd_count);
    let mut pos = cd_offset;

    for entry_idx in 0..cd_count {
        if pos + 46 > data.len() {
            return Err(format!(
                "zip: central directory entry {entry_idx} truncated"
            ));
        }
        let sig = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
        if sig != SIG_CENTRAL {
            return Err(format!(
                "zip: expected central dir signature at {pos:#x}, got {sig:#010x}"
            ));
        }

        let method = u16::from_le_bytes([data[pos+10], data[pos+11]]);
        let mod_time = u16::from_le_bytes([data[pos+12], data[pos+13]]);
        let mod_date = u16::from_le_bytes([data[pos+14], data[pos+15]]);
        let entry_crc = u32::from_le_bytes([data[pos+16], data[pos+17], data[pos+18], data[pos+19]]);
        let comp_size = u32::from_le_bytes([data[pos+20], data[pos+21], data[pos+22], data[pos+23]]);
        let uncomp_size = u32::from_le_bytes([data[pos+24], data[pos+25], data[pos+26], data[pos+27]]);
        let fname_len = u16::from_le_bytes([data[pos+28], data[pos+29]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos+30], data[pos+31]]) as usize;
        let comment_len = u16::from_le_bytes([data[pos+32], data[pos+33]]) as usize;
        let internal_attrs = u16::from_le_bytes([data[pos+36], data[pos+37]]);
        let external_attrs = u32::from_le_bytes([data[pos+38], data[pos+39], data[pos+40], data[pos+41]]);
        let lhdr_offset = u32::from_le_bytes([data[pos+42], data[pos+43], data[pos+44], data[pos+45]]);

        let name_start = pos + 46;
        let name_end = name_start + fname_len;
        let comment_start = name_end + extra_len;
        let comment_end = comment_start + comment_len;

        if comment_end > data.len() {
            return Err(format!("zip: entry {entry_idx} name/comment extends beyond data"));
        }

        let name = String::from_utf8_lossy(&data[name_start..name_end]).into_owned();
        let comment = String::from_utf8_lossy(&data[comment_start..comment_end]).into_owned();

        entries.push(ZipEntry {
            name,
            method,
            mod_date,
            mod_time,
            crc32: entry_crc,
            compressed_size: comp_size,
            uncompressed_size: uncomp_size,
            local_header_offset: lhdr_offset,
            comment,
            external_attrs,
            internal_attrs,
        });

        pos = comment_end;
    }

    Ok(entries)
}

/// Locate the End of Central Directory record by scanning backwards from the end.
fn find_eocd(data: &[u8]) -> Result<usize, String> {
    if data.len() < 22 {
        return Err("zip: file too small to contain EOCD".to_string());
    }
    // EOCD has a variable-length comment (up to 65535 bytes) at the end.
    let search_start = data.len().saturating_sub(22 + 65535);
    let search_end = data.len() - 22;

    // Scan backwards (EOCD is usually near the end).
    let mut i = search_end;
    loop {
        if data[i] == 0x50
            && data[i + 1] == 0x4B
            && data[i + 2] == 0x05
            && data[i + 3] == 0x06
        {
            // Verify the comment length matches.
            let comment_len = u16::from_le_bytes([data[i + 20], data[i + 21]]) as usize;
            if i + 22 + comment_len == data.len() {
                return Ok(i);
            }
        }
        if i == search_start { break; }
        i -= 1;
    }
    Err("zip: EOCD record not found".to_string())
}

/// Extract the compressed data for a given entry from the archive.
///
/// Returns `(compressed_bytes, actual_crc, actual_comp_size, actual_uncomp_size)`.
/// The last three are from the local header (which may be more up-to-date for
/// entries written with data descriptors).
fn zip_read_local_data<'a>(data: &'a [u8], entry: &ZipEntry) -> Result<&'a [u8], String> {
    let offset = entry.local_header_offset as usize;
    if offset + 30 > data.len() {
        return Err(format!(
            "zip: local header for '{}' at offset {offset:#x} exceeds file",
            entry.name
        ));
    }

    let sig = u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
    if sig != SIG_LOCAL {
        return Err(format!(
            "zip: expected local file header for '{}' at {offset:#x}, got {sig:#010x}",
            entry.name
        ));
    }

    let fname_len = u16::from_le_bytes([data[offset+26], data[offset+27]]) as usize;
    let extra_len = u16::from_le_bytes([data[offset+28], data[offset+29]]) as usize;
    let data_start = offset + 30 + fname_len + extra_len;
    let comp_size = entry.compressed_size as usize;

    if data_start + comp_size > data.len() {
        return Err(format!(
            "zip: compressed data for '{}' at {data_start:#x}+{comp_size} exceeds file",
            entry.name
        ));
    }

    Ok(&data[data_start..data_start + comp_size])
}

/// Decompress a single ZIP entry and verify its CRC32.
fn zip_extract_entry(data: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    let compressed = zip_read_local_data(data, entry)?;

    let output = match entry.method {
        METHOD_STORED => compressed.to_vec(),
        METHOD_DEFLATE => {
            let mut out = Vec::new();
            let mut reader = BitReader::new(compressed);
            deflate_decompress(&mut reader, &mut out)
                .map_err(|e| format!("zip: '{}': {e}", entry.name))?;
            out
        }
        other => {
            return Err(format!(
                "zip: '{}': unsupported compression method {other}",
                entry.name
            ))
        }
    };

    // Verify sizes.
    if output.len() != entry.uncompressed_size as usize {
        return Err(format!(
            "zip: '{}': size mismatch: expected {}, got {}",
            entry.name, entry.uncompressed_size, output.len()
        ));
    }

    // Verify CRC32.
    let computed = crc32(&output);
    if computed != entry.crc32 {
        return Err(format!(
            "zip: '{}': CRC32 mismatch: expected {:#010x}, got {:#010x}",
            entry.name, entry.crc32, computed
        ));
    }

    Ok(output)
}

// ============================================================================
// ZIP writer
// ============================================================================

/// Builds a ZIP archive in memory.
struct ZipWriter {
    buf: Vec<u8>,
    entries: Vec<ZipEntry>,
}

impl ZipWriter {
    fn new() -> Self {
        Self { buf: Vec::new(), entries: Vec::new() }
    }

    /// Add a file to the archive.
    ///
    /// `name` is the stored path (forward slashes, no leading slash).
    /// `data` is the raw (uncompressed) file contents.
    /// `level` is the compression level (0 = stored, 1-9 = deflate).
    /// `mod_date` and `mod_time` are DOS-encoded date/time.
    fn add_file(
        &mut self,
        name: &str,
        data: &[u8],
        level: u8,
        mod_date: u16,
        mod_time: u16,
    ) -> Result<(), String> {
        let (method, compressed) = if level == 0 {
            (METHOD_STORED, data.to_vec())
        } else {
            let comp = deflate_compress(data, level)?;
            // Only use deflate if it actually shrinks the data.
            if comp.len() < data.len() {
                (METHOD_DEFLATE, comp)
            } else {
                (METHOD_STORED, data.to_vec())
            }
        };

        let file_crc = crc32(data);
        let local_offset = self.buf.len() as u32;
        let version_needed = if method == METHOD_DEFLATE {
            VERSION_NEEDED_DEFLATE
        } else {
            VERSION_NEEDED_STORED
        };

        // Local file header.
        let name_bytes = name.as_bytes();
        write_u32_le(&mut self.buf, SIG_LOCAL);
        write_u16_le(&mut self.buf, version_needed);
        write_u16_le(&mut self.buf, 0); // general purpose bit flag
        write_u16_le(&mut self.buf, method);
        write_u16_le(&mut self.buf, mod_time);
        write_u16_le(&mut self.buf, mod_date);
        write_u32_le(&mut self.buf, file_crc);
        write_u32_le(&mut self.buf, compressed.len() as u32);
        write_u32_le(&mut self.buf, data.len() as u32);
        write_u16_le(&mut self.buf, name_bytes.len() as u16);
        write_u16_le(&mut self.buf, 0); // extra field length
        self.buf.extend_from_slice(name_bytes);

        // File data.
        self.buf.extend_from_slice(&compressed);

        self.entries.push(ZipEntry {
            name: name.to_string(),
            method,
            mod_date,
            mod_time,
            crc32: file_crc,
            compressed_size: compressed.len() as u32,
            uncompressed_size: data.len() as u32,
            local_header_offset: local_offset,
            comment: String::new(),
            external_attrs: 0,
            internal_attrs: 0,
        });

        Ok(())
    }

    /// Add a directory entry (stored, no data).
    fn add_directory(
        &mut self,
        name: &str,
        mod_date: u16,
        mod_time: u16,
    ) {
        // Directory names must end with '/'.
        let dir_name = if name.ends_with('/') {
            name.to_string()
        } else {
            format!("{name}/")
        };

        let local_offset = self.buf.len() as u32;
        let name_bytes = dir_name.as_bytes();

        write_u32_le(&mut self.buf, SIG_LOCAL);
        write_u16_le(&mut self.buf, VERSION_NEEDED_STORED);
        write_u16_le(&mut self.buf, 0);
        write_u16_le(&mut self.buf, METHOD_STORED);
        write_u16_le(&mut self.buf, mod_time);
        write_u16_le(&mut self.buf, mod_date);
        write_u32_le(&mut self.buf, 0); // crc
        write_u32_le(&mut self.buf, 0); // compressed size
        write_u32_le(&mut self.buf, 0); // uncompressed size
        write_u16_le(&mut self.buf, name_bytes.len() as u16);
        write_u16_le(&mut self.buf, 0);
        self.buf.extend_from_slice(name_bytes);
        // No data.

        self.entries.push(ZipEntry {
            name: dir_name,
            method: METHOD_STORED,
            mod_date,
            mod_time,
            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            local_header_offset: local_offset,
            comment: String::new(),
            external_attrs: 0x0010_0000, // directory attribute for Unix
            internal_attrs: 0,
        });
    }

    /// Finish the archive and return the complete ZIP bytes.
    fn finish(mut self) -> Vec<u8> {
        let cd_offset = self.buf.len() as u32;

        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            let comment_bytes = entry.comment.as_bytes();

            write_u32_le(&mut self.buf, SIG_CENTRAL);
            write_u16_le(&mut self.buf, VERSION_MADE_BY);
            let version_needed = if entry.method == METHOD_DEFLATE {
                VERSION_NEEDED_DEFLATE
            } else {
                VERSION_NEEDED_STORED
            };
            write_u16_le(&mut self.buf, version_needed);
            write_u16_le(&mut self.buf, 0); // general purpose bit flag
            write_u16_le(&mut self.buf, entry.method);
            write_u16_le(&mut self.buf, entry.mod_time);
            write_u16_le(&mut self.buf, entry.mod_date);
            write_u32_le(&mut self.buf, entry.crc32);
            write_u32_le(&mut self.buf, entry.compressed_size);
            write_u32_le(&mut self.buf, entry.uncompressed_size);
            write_u16_le(&mut self.buf, name_bytes.len() as u16);
            write_u16_le(&mut self.buf, 0); // extra field length
            write_u16_le(&mut self.buf, comment_bytes.len() as u16);
            write_u16_le(&mut self.buf, 0); // disk number start
            write_u16_le(&mut self.buf, entry.internal_attrs);
            write_u32_le(&mut self.buf, entry.external_attrs);
            write_u32_le(&mut self.buf, entry.local_header_offset);
            self.buf.extend_from_slice(name_bytes);
            self.buf.extend_from_slice(comment_bytes);
        }

        let cd_size = self.buf.len() as u32 - cd_offset;
        let entry_count = self.entries.len() as u16;

        // End of central directory record.
        write_u32_le(&mut self.buf, SIG_EOCD);
        write_u16_le(&mut self.buf, 0); // disk number
        write_u16_le(&mut self.buf, 0); // disk with start of CD
        write_u16_le(&mut self.buf, entry_count);
        write_u16_le(&mut self.buf, entry_count);
        write_u32_le(&mut self.buf, cd_size);
        write_u32_le(&mut self.buf, cd_offset);
        write_u16_le(&mut self.buf, 0); // archive comment length

        self.buf
    }
}

// ============================================================================
// Binary write helpers
// ============================================================================

#[inline]
fn write_u16_le(buf: &mut Vec<u8>, val: u16) {
    buf.extend_from_slice(&val.to_le_bytes());
}

#[inline]
fn write_u32_le(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
}

// ============================================================================
// File system helpers
// ============================================================================

/// Read an entire file into a `Vec<u8>`.
fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| format!("{}: read error: {e}", path.display()))?;
    Ok(data)
}

/// Write bytes to a file.
fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(data)
        .map_err(|e| format!("{}: write error: {e}", path.display()))?;
    Ok(())
}

/// Get modification time of a file, or epoch on error.
fn file_mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Create parent directories for a path, if they don't exist.
fn create_parent_dirs(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| format!("{}: mkdir: {e}", parent.display()))?;
    }
    Ok(())
}

/// Glob-style pattern matching (supports `*` and `?`).
///
/// `*` matches any sequence of characters (not crossing directory boundaries).
/// For simplicity in this implementation, `*` matches any sequence including `/`.
fn glob_matches(pattern: &str, name: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_inner(pat: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    // `star_pi` records the pattern position just after the most recent `*`;
    // `star_si` records how far into `s` that `*` has been stretched so far.
    let mut star_pi: Option<usize> = None;
    let mut star_si = 0usize;

    // Advance through the string. Each branch makes progress; the star-backtrack
    // case only ever increments `star_si` up to `s.len()`, so this terminates.
    while si < s.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = Some(pi);
            star_si = si;
            pi += 1;
        } else if let Some(sp) = star_pi {
            // Backtrack: let the last `*` swallow one more character of `s`.
            pi = sp + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }

    // String consumed: any trailing pattern must be all `*` to match.
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Format file size as a human-readable string.
fn human_size(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    }
}

/// Decode a DOS date/time back to a display string.
fn dos_datetime_str(mod_date: u16, mod_time: u16) -> String {
    let year = 1980 + ((mod_date >> 9) & 0x7F);
    let month = (mod_date >> 5) & 0x0F;
    let day = mod_date & 0x1F;
    let hour = (mod_time >> 11) & 0x1F;
    let minute = (mod_time >> 5) & 0x3F;
    let second = (mod_time & 0x1F) * 2;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

// ============================================================================
// Recursive directory listing
// ============================================================================

/// Collect all files (recursively) under `dir`, recording them relative to `base`.
fn collect_files(
    dir: &Path,
    base: &Path,
    junk_paths: bool,
    excludes: &[String],
    files_out: &mut Vec<(PathBuf, String)>,
    errors: &mut Vec<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(format!("{}: {e}", dir.display()));
            return;
        }
    };

    let mut children: Vec<PathBuf> = entries
        .filter_map(|e| {
            match e {
                Ok(de) => Some(de.path()),
                Err(err) => {
                    errors.push(format!("{}: {err}", dir.display()));
                    None
                }
            }
        })
        .collect();
    children.sort();

    for child in &children {
        let arc_name = archive_name(child, base, junk_paths);
        if excludes.iter().any(|p| glob_matches(p, &arc_name)) {
            continue;
        }
        if child.is_dir() {
            collect_files(child, base, junk_paths, excludes, files_out, errors);
        } else {
            files_out.push((child.clone(), arc_name));
        }
    }
}

/// Compute the archive name for a file.
///
/// If `junk_paths` is true, only the filename is stored (no directory component).
/// Otherwise the path relative to `base` is stored, using forward slashes.
fn archive_name(path: &Path, base: &Path, junk_paths: bool) -> String {
    if junk_paths {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        path.strip_prefix(base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

// ============================================================================
// CLI option types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolMode {
    Zip,
    Unzip,
}

/// Options for zip mode.
struct ZipOptions {
    /// Archive file path.
    archive: String,
    /// Source files/dirs to add.
    sources: Vec<String>,
    /// Compression level (0-9).
    level: u8,
    /// Recurse into directories.
    recursive: bool,
    /// Junk paths (store only filename).
    junk_paths: bool,
    /// Verbose output.
    verbose: bool,
    /// Quiet (suppress normal output).
    quiet: bool,
    /// Patterns to exclude.
    excludes: Vec<String>,
    /// Update mode: only add newer files.
    update: bool,
}

/// Options for unzip mode.
struct UnzipOptions {
    /// Archive file path.
    archive: String,
    /// Specific files to extract (empty = all).
    files: Vec<String>,
    /// Output directory.
    dest_dir: String,
    /// List contents instead of extracting.
    list: bool,
    /// Test integrity only.
    test: bool,
    /// Overwrite without prompting.
    overwrite: bool,
    /// Never overwrite.
    no_overwrite: bool,
    /// Verbose listing.
    verbose: bool,
    /// Quiet (suppress normal output).
    quiet: bool,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn detect_mode(argv0: &str) -> ToolMode {
    let base = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(argv0);
    if base == "unzip" { ToolMode::Unzip } else { ToolMode::Zip }
}

fn parse_zip_args(args: &[String]) -> Result<ZipOptions, String> {
    if args.is_empty() {
        return Err("zip: no arguments (try -h for help)".to_string());
    }

    let mut level: u8 = 6;
    let mut recursive = false;
    let mut junk_paths = false;
    let mut verbose = false;
    let mut quiet = false;
    let mut excludes: Vec<String> = Vec::new();
    let mut update = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            i += 1;
            while i < args.len() {
                positional.push(args[i].clone());
                i += 1;
            }
            break;
        }
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for ch in arg[1..].chars() {
                match ch {
                    'r' => recursive = true,
                    'j' => junk_paths = true,
                    'v' => verbose = true,
                    'q' => quiet = true,
                    'u' => update = true,
                    'x' => {
                        i += 1;
                        if i >= args.len() {
                            return Err("zip: -x requires a pattern argument".to_string());
                        }
                        excludes.push(args[i].clone());
                    }
                    '0'..='9' => level = ch as u8 - b'0',
                    'h' => {
                        print_zip_usage();
                        process::exit(0);
                    }
                    c => return Err(format!("zip: unknown option: -{c}")),
                }
            }
        } else if arg == "--help" {
            print_zip_usage();
            process::exit(0);
        } else {
            positional.push(arg.to_string());
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("zip: no archive name specified".to_string());
    }

    let archive = positional[0].clone();
    let sources = positional[1..].to_vec();

    Ok(ZipOptions {
        archive,
        sources,
        level,
        recursive,
        junk_paths,
        verbose,
        quiet,
        excludes,
        update,
    })
}

fn parse_unzip_args(args: &[String]) -> Result<UnzipOptions, String> {
    if args.is_empty() {
        return Err("unzip: no arguments (try -h for help)".to_string());
    }

    let mut dest_dir = String::from(".");
    let mut list = false;
    let mut test = false;
    let mut overwrite = false;
    let mut no_overwrite = false;
    let mut verbose = false;
    let mut quiet = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            i += 1;
            while i < args.len() {
                positional.push(args[i].clone());
                i += 1;
            }
            break;
        }
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for ch in arg[1..].chars() {
                match ch {
                    'd' => {
                        i += 1;
                        if i >= args.len() {
                            return Err("unzip: -d requires a directory argument".to_string());
                        }
                        dest_dir.clone_from(&args[i]);
                    }
                    'l' => list = true,
                    't' => test = true,
                    'o' => overwrite = true,
                    'n' => no_overwrite = true,
                    'v' => verbose = true,
                    'q' => quiet = true,
                    'h' => {
                        print_unzip_usage();
                        process::exit(0);
                    }
                    c => return Err(format!("unzip: unknown option: -{c}")),
                }
            }
        } else if arg == "--help" {
            print_unzip_usage();
            process::exit(0);
        } else {
            positional.push(arg.to_string());
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("unzip: no archive name specified".to_string());
    }

    let archive = positional[0].clone();
    let files = positional[1..].to_vec();

    Ok(UnzipOptions {
        archive,
        files,
        dest_dir,
        list,
        test,
        overwrite,
        no_overwrite,
        verbose,
        quiet,
    })
}

// ============================================================================
// zip mode implementation
// ============================================================================

fn run_zip(opts: &ZipOptions) -> Result<(), String> {
    if opts.sources.is_empty() {
        return Err("zip: no source files specified".to_string());
    }

    // If archive already exists and we're in update mode, load it.
    let mut existing: Vec<ZipEntry> = Vec::new();
    let mut existing_data: Vec<u8> = Vec::new();
    let archive_path = Path::new(&opts.archive);

    if opts.update && archive_path.exists() {
        existing_data = read_file(archive_path)?;
        existing = zip_read_central_directory(&existing_data)
            .map_err(|e| format!("zip: reading existing archive: {e}"))?;
    }

    let mut writer = ZipWriter::new();
    let mut total_files = 0u64;
    let mut total_bytes = 0u64;
    let mut errors: Vec<String> = Vec::new();

    for source in &opts.sources {
        let path = Path::new(source);

        if !path.exists() {
            errors.push(format!("zip: {source}: No such file or directory"));
            continue;
        }

        if path.is_dir() {
            if opts.recursive {
                let mut files: Vec<(PathBuf, String)> = Vec::new();
                collect_files(
                    path,
                    path,
                    opts.junk_paths,
                    &opts.excludes,
                    &mut files,
                    &mut errors,
                );
                // Also add the directory entry itself.
                let dir_arc_name = archive_name(path, path.parent().unwrap_or(path), opts.junk_paths);
                if !dir_arc_name.is_empty() {
                    let (dd, dt) = encode_dos_datetime(file_mtime(path));
                    writer.add_directory(&dir_arc_name, dd, dt);
                }
                for (fpath, arc_name) in &files {
                    if let Err(e) = add_one_file(
                        &mut writer,
                        fpath,
                        arc_name,
                        opts.level,
                        opts.update,
                        &existing,
                        &existing_data,
                        opts.verbose,
                        opts.quiet,
                        &mut total_files,
                        &mut total_bytes,
                    ) {
                        errors.push(e);
                    }
                }
            } else if !opts.quiet {
                eprintln!("zip: {source}: is a directory -- ignored (use -r for recursive)");
            }
            continue;
        }

        let arc_name = if opts.junk_paths {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            source.replace('\\', "/")
        };

        if opts.excludes.iter().any(|p| glob_matches(p, &arc_name)) {
            continue;
        }

        if let Err(e) = add_one_file(
            &mut writer,
            path,
            &arc_name,
            opts.level,
            opts.update,
            &existing,
            &existing_data,
            opts.verbose,
            opts.quiet,
            &mut total_files,
            &mut total_bytes,
        ) {
            errors.push(e);
        }
    }

    let archive_bytes = writer.finish();
    write_file(archive_path, &archive_bytes)?;

    if !opts.quiet {
        eprintln!(
            "zip: {total_files} file(s), {} → {} (archive: {})",
            human_size(total_bytes),
            opts.archive,
            human_size(archive_bytes.len() as u64),
        );
    }

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("{e}");
        }
        return Err(format!("zip: {} error(s) occurred", errors.len()));
    }

    Ok(())
}

#[allow(clippy::similar_names)]
fn add_one_file(
    writer: &mut ZipWriter,
    path: &Path,
    arc_name: &str,
    level: u8,
    update: bool,
    existing: &[ZipEntry],
    existing_data: &[u8],
    verbose: bool,
    quiet: bool,
    total_files: &mut u64,
    total_bytes: &mut u64,
) -> Result<(), String> {
    // In update mode, check if a newer version already exists in the archive.
    if update
        && let Some(existing_entry) = existing.iter().find(|e| e.name == arc_name)
    {
        let file_mtime = encode_dos_datetime(file_mtime(path));
        let entry_mtime = (existing_entry.mod_date, existing_entry.mod_time);
        if entry_mtime >= file_mtime {
            // Existing entry is as new or newer; copy it.
            let comp_data = zip_read_local_data(existing_data, existing_entry)
                .map_err(|e| format!("zip: update mode: {e}"))?;
            writer.buf.extend_from_slice(comp_data); // simplified copy
            writer.entries.push(existing_entry.clone());
            return Ok(());
        }
    }

    let data = read_file(path)?;
    let (mod_date, mod_time) = encode_dos_datetime(file_mtime(path));

    writer.add_file(arc_name, &data, level, mod_date, mod_time)?;

    if verbose && !quiet {
        let last = writer.entries.last();
        let comp_size = last.map_or(0, |e| e.compressed_size as u64);
        let ratio = if data.is_empty() {
            0.0
        } else {
            100.0 * (1.0 - comp_size as f64 / data.len() as f64)
        };
        eprintln!("  adding: {arc_name} (deflated {ratio:.0}%)");
    }

    *total_files += 1;
    *total_bytes += data.len() as u64;
    Ok(())
}

// ============================================================================
// unzip mode implementation
// ============================================================================

fn run_unzip(opts: &UnzipOptions) -> Result<(), String> {
    let archive_path = Path::new(&opts.archive);
    let archive_data = read_file(archive_path)
        .map_err(|e| format!("unzip: cannot open {}: {e}", opts.archive))?;

    let entries = zip_read_central_directory(&archive_data)
        .map_err(|e| format!("unzip: {}: {e}", opts.archive))?;

    if opts.list || opts.verbose {
        list_archive(&entries, opts);
        return Ok(());
    }

    if opts.test {
        return test_archive(&archive_data, &entries, opts);
    }

    extract_archive(&archive_data, &entries, opts)
}

fn list_archive(entries: &[ZipEntry], opts: &UnzipOptions) {
    // Pre-rendered dashed separator rows (avoids passing empty literals to
    // width/fill format specifiers).
    const SEP_VERBOSE: &str =
        "---------- ----- ---------- ----------  ----------------  --------------------";
    const SEP_PLAIN: &str = "----------  ----------------  --------------------";

    if !opts.quiet {
        if opts.verbose {
            println!(
                "{:>10} {:>5} {:>10} {:>10}  {}  {}",
                "Length", "Method", "Compressed", "Ratio", "Date/Time", "Name"
            );
            println!("{SEP_VERBOSE}");
        } else {
            println!("{:>10}  {}  {}", "Length", "Date/Time        ", "Name");
            println!("{SEP_PLAIN}");
        }
    }

    let mut total_uncomp = 0u64;
    let mut total_comp = 0u64;
    let mut count = 0usize;

    for entry in entries {
        if !opts.files.is_empty()
            && !opts.files.iter().any(|f| glob_matches(f, &entry.name))
        {
            continue;
        }
        let dt = dos_datetime_str(entry.mod_date, entry.mod_time);
        if opts.verbose {
            let method_str = match entry.method {
                METHOD_STORED => "Stored",
                METHOD_DEFLATE => "Defl:N",
                m => Box::leak(format!("{m}").into_boxed_str()),
            };
            let ratio = if entry.uncompressed_size == 0 {
                0.0
            } else {
                100.0 * (1.0 - entry.compressed_size as f64 / entry.uncompressed_size as f64)
            };
            println!(
                "{:>10} {:>6} {:>10} {:>9.0}%  {}  {}",
                entry.uncompressed_size,
                method_str,
                entry.compressed_size,
                ratio,
                dt,
                entry.name
            );
        } else {
            println!("{:>10}  {}  {}", entry.uncompressed_size, dt, entry.name);
        }
        total_uncomp += u64::from(entry.uncompressed_size);
        total_comp += u64::from(entry.compressed_size);
        count += 1;
    }

    if !opts.quiet {
        if opts.verbose {
            println!("{SEP_VERBOSE}");
            let ratio = if total_uncomp == 0 {
                0.0
            } else {
                100.0 * (1.0 - total_comp as f64 / total_uncomp as f64)
            };
            println!(
                "{total_uncomp:>10}          {total_comp:>10} {ratio:>9.0}%                    {count} files"
            );
        } else {
            println!("{SEP_PLAIN}");
            println!("{total_uncomp:>10}                    {count} files");
        }
    }
}

fn test_archive(
    archive_data: &[u8],
    entries: &[ZipEntry],
    opts: &UnzipOptions,
) -> Result<(), String> {
    let mut errors = 0usize;

    for entry in entries {
        if !opts.files.is_empty()
            && !opts.files.iter().any(|f| glob_matches(f, &entry.name))
        {
            continue;
        }
        if entry.name.ends_with('/') {
            continue; // skip directory entries
        }
        match zip_extract_entry(archive_data, entry) {
            Ok(_) => {
                if !opts.quiet {
                    println!("    testing: {}   OK", entry.name);
                }
            }
            Err(e) => {
                eprintln!("    testing: {}   FAILED: {e}", entry.name);
                errors += 1;
            }
        }
    }

    if errors > 0 {
        Err(format!("unzip: {errors} error(s) during test"))
    } else {
        if !opts.quiet {
            println!("No errors detected in archive.");
        }
        Ok(())
    }
}

fn extract_archive(
    archive_data: &[u8],
    entries: &[ZipEntry],
    opts: &UnzipOptions,
) -> Result<(), String> {
    let dest = Path::new(&opts.dest_dir);
    let mut errors: Vec<String> = Vec::new();
    let mut extracted = 0usize;

    for entry in entries {
        if !opts.files.is_empty()
            && !opts.files.iter().any(|f| glob_matches(f, &entry.name))
        {
            continue;
        }

        // Sanitize path: reject absolute paths and `..` components.
        if entry.name.starts_with('/') || entry.name.contains("../") || entry.name == ".." {
            eprintln!("unzip: skipping unsafe path: {}", entry.name);
            continue;
        }

        let out_path = dest.join(&entry.name);

        if entry.name.ends_with('/') {
            // Directory entry.
            if let Err(e) = fs::create_dir_all(&out_path) {
                errors.push(format!("{}: mkdir: {e}", out_path.display()));
            }
            continue;
        }

        // Overwrite logic.
        if out_path.exists() {
            if opts.no_overwrite {
                if !opts.quiet {
                    println!("unzip: not overwriting {}", out_path.display());
                }
                continue;
            }
            if !opts.overwrite {
                // Default: overwrite.
                // (Interactive prompting is not implemented; we default to overwrite.)
            }
        }

        if let Err(e) = create_parent_dirs(&out_path) {
            errors.push(e);
            continue;
        }

        match zip_extract_entry(archive_data, entry) {
            Ok(data) => {
                if let Err(e) = write_file(&out_path, &data) {
                    errors.push(e);
                } else {
                    if !opts.quiet {
                        println!("  inflating: {}", out_path.display());
                    }
                    extracted += 1;
                }
            }
            Err(e) => {
                errors.push(format!("unzip: {e}"));
            }
        }
    }

    if !opts.quiet {
        println!("unzip: extracted {extracted} file(s) to '{}'", dest.display());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        for e in &errors {
            eprintln!("{e}");
        }
        Err(format!("unzip: {} error(s)", errors.len()))
    }
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_zip_usage() {
    eprintln!(
        "\
Usage: zip [OPTIONS] archive.zip file1 file2 ...

Create or update a ZIP archive.

Options:
  -r          Recurse into directories
  -j          Junk paths (store only filenames)
  -0 to -9    Compression level (0=stored, default: 6)
  -v          Verbose output
  -q          Quiet
  -x PATTERN  Exclude files matching PATTERN (may repeat)
  -u          Update: only add files newer than archive entries
  -h          Show this help"
    );
}

fn print_unzip_usage() {
    eprintln!(
        "\
Usage: unzip [OPTIONS] archive.zip [file ...]

Extract files from a ZIP archive.

Options:
  -d DIR      Extract to DIR (default: current directory)
  -l          List contents
  -v          Verbose listing
  -t          Test integrity
  -o          Overwrite files without prompting
  -n          Never overwrite existing files
  -q          Quiet
  -h          Show this help"
    );
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map_or("zip", String::as_str);
    let mode = detect_mode(argv0);

    let cli_args = if args.len() > 1 { &args[1..] } else { &[] as &[String] };

    let result = match mode {
        ToolMode::Zip => {
            match parse_zip_args(cli_args) {
                Ok(opts) => run_zip(&opts),
                Err(e) => Err(e),
            }
        }
        ToolMode::Unzip => {
            match parse_unzip_args(cli_args) {
                Ok(opts) => run_unzip(&opts),
                Err(e) => Err(e),
            }
        }
    };

    if let Err(e) = result {
        eprintln!("{e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- CRC32 ----

    #[test]
    fn test_crc32_empty() {
        assert_eq!(crc32(b""), 0x0000_0000);
    }

    #[test]
    fn test_crc32_known() {
        // CRC32("123456789") = 0xCBF43926 per ISO 3309.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn test_crc32_hello() {
        assert_eq!(crc32(b"Hello, World!"), 0xEC4A_C3D0);
    }

    #[test]
    fn test_crc32_incremental() {
        let data = b"Hello, World!";
        let full = crc32(data);
        let partial = crc32_update(0, &data[..7]);
        let incr = crc32_update(partial, &data[7..]);
        assert_eq!(full, incr);
    }

    // ---- DOS date/time ----

    #[test]
    fn test_dos_datetime_epoch() {
        let (date, time) = encode_dos_datetime(SystemTime::UNIX_EPOCH);
        // 1970-01-01 00:00:00 → year=1970, but DOS epoch starts at 1980.
        // Year offset: 1970-1980 = -10 → clamped to 0 (1980-01-01).
        let year = 1980 + ((date >> 9) & 0x7F);
        assert_eq!(year, 1980);
        let _ = time; // just check it doesn't panic
    }

    #[test]
    fn test_dos_datetime_str_known() {
        // Encode known date 2023-06-15 12:30:00.
        let year_offset: u16 = 2023 - 1980;
        let mod_date: u16 = (year_offset << 9) | (6 << 5) | 15;
        let mod_time: u16 = (12 << 11) | (30 << 5) | 0;
        let s = dos_datetime_str(mod_date, mod_time);
        assert_eq!(s, "2023-06-15 12:30:00");
    }

    // ---- Unix datetime conversion ----

    #[test]
    fn test_unix_datetime_epoch() {
        let (y, mo, d, h, mi, s) = unix_secs_to_datetime(0);
        assert_eq!(y, 1970);
        assert_eq!(mo, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(mi, 0);
        assert_eq!(s, 0);
    }

    #[test]
    fn test_unix_datetime_known() {
        // 2023-01-01 00:00:00 UTC = 1672531200
        let (y, mo, d, h, mi, s) = unix_secs_to_datetime(1_672_531_200);
        assert_eq!(y, 2023);
        assert_eq!(mo, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(mi, 0);
        assert_eq!(s, 0);
    }

    // ---- Glob matching ----

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_matches("foo.txt", "foo.txt"));
        assert!(!glob_matches("foo.txt", "bar.txt"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_matches("*.txt", "hello.txt"));
        assert!(glob_matches("*.txt", ".txt"));
        assert!(!glob_matches("*.txt", "hello.rs"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_matches("f?o", "foo"));
        assert!(glob_matches("f?o", "fxo"));
        assert!(!glob_matches("f?o", "fo"));
    }

    #[test]
    fn test_glob_star_prefix_suffix() {
        assert!(glob_matches("*.log", "access.log"));
        assert!(glob_matches("log*", "logfile"));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("*", ""));
    }

    #[test]
    fn test_glob_no_match_terminates() {
        // Regression: a `*` followed by a suffix that never matches used to
        // spin forever (si ran past the end of the string unbounded).
        assert!(!glob_matches("*.txt", "hello.rs"));
        assert!(!glob_matches("*abc", "xyz"));
        assert!(!glob_matches("a*z", "abc"));
        assert!(!glob_matches("foo*", "fo"));
    }

    #[test]
    fn test_glob_multiple_stars() {
        assert!(glob_matches("*a*b*", "xaybz"));
        assert!(glob_matches("a*b*c", "abc"));
        assert!(glob_matches("**", "anything"));
        assert!(!glob_matches("a*b*c", "abx"));
    }

    // ---- DEFLATE compressor/decompressor ----

    #[test]
    fn test_deflate_roundtrip_empty() {
        let input = b"";
        for level in 0u8..=3 {
            let comp = deflate_compress(input, level).unwrap();
            let mut out = Vec::new();
            let mut reader = BitReader::new(&comp);
            deflate_decompress(&mut reader, &mut out).unwrap();
            assert_eq!(out.as_slice(), input, "level={level}");
        }
    }

    #[test]
    fn test_deflate_roundtrip_short() {
        let input = b"Hello, DEFLATE world!";
        let comp = deflate_compress(input, 6).unwrap();
        let mut out = Vec::new();
        let mut reader = BitReader::new(&comp);
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out.as_slice(), input);
    }

    #[test]
    fn test_deflate_roundtrip_repeated() {
        let input: Vec<u8> = (0u8..=9).cycle().take(2000).collect();
        for level in 1u8..=6 {
            let comp = deflate_compress(&input, level).unwrap();
            assert!(
                comp.len() < input.len(),
                "level={level}: compressed ({}) should be < input ({})",
                comp.len(), input.len()
            );
            let mut out = Vec::new();
            let mut reader = BitReader::new(&comp);
            deflate_decompress(&mut reader, &mut out).unwrap();
            assert_eq!(out, input, "level={level}");
        }
    }

    #[test]
    fn test_deflate_roundtrip_binary() {
        let input: Vec<u8> = (0u8..=255).cycle().take(3000).collect();
        let comp = deflate_compress(&input, 6).unwrap();
        let mut out = Vec::new();
        let mut reader = BitReader::new(&comp);
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn test_deflate_all_same_byte() {
        let input = vec![0xAAu8; 1024];
        let comp = deflate_compress(&input, 9).unwrap();
        let mut out = Vec::new();
        let mut reader = BitReader::new(&comp);
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn test_deflate_stored_block() {
        let input = b"stored block test data";
        let comp = deflate_compress_stored(input);
        let mut out = Vec::new();
        let mut reader = BitReader::new(&comp);
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out.as_slice(), input);
    }

    // ---- ZIP archive round-trips ----

    #[test]
    fn test_zip_single_file_stored() {
        let mut writer = ZipWriter::new();
        writer.add_file("hello.txt", b"Hello, ZIP!", 0, 0, 0).unwrap();
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "hello.txt");
        assert_eq!(entries[0].method, METHOD_STORED);

        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, b"Hello, ZIP!");
    }

    #[test]
    fn test_zip_single_file_deflate() {
        // Compressible input so deflate actually shrinks it.
        let input: Vec<u8> = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec();
        let mut writer = ZipWriter::new();
        writer.add_file("rep.bin", &input, 6, 0, 0).unwrap();
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 1);

        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, input);
    }

    #[test]
    fn test_zip_multiple_files() {
        let mut writer = ZipWriter::new();
        writer.add_file("a.txt", b"file A", 6, 0, 0).unwrap();
        writer.add_file("b.txt", b"file B contents here", 6, 0, 0).unwrap();
        writer.add_file("c.txt", b"", 0, 0, 0).unwrap();
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 3);

        let a = zip_extract_entry(&archive, &entries[0]).unwrap();
        let b = zip_extract_entry(&archive, &entries[1]).unwrap();
        let c = zip_extract_entry(&archive, &entries[2]).unwrap();

        assert_eq!(a, b"file A");
        assert_eq!(b, b"file B contents here");
        assert_eq!(c, b"");
    }

    #[test]
    fn test_zip_directory_entry() {
        let mut writer = ZipWriter::new();
        writer.add_directory("subdir", 0, 0);
        writer.add_file("subdir/file.txt", b"inside dir", 0, 0, 0).unwrap();
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].name.ends_with('/'));
        assert_eq!(entries[1].name, "subdir/file.txt");
    }

    #[test]
    fn test_zip_empty_archive() {
        let writer = ZipWriter::new();
        let archive = writer.finish();
        let entries = zip_read_central_directory(&archive).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_zip_crc_mismatch_detected() {
        let mut writer = ZipWriter::new();
        writer.add_file("test.txt", b"test data", 0, 0, 0).unwrap();
        let mut archive = writer.finish();

        // Corrupt a byte in the file data region.
        let entries = zip_read_central_directory(&archive).unwrap();
        let offset = entries[0].local_header_offset as usize;
        // Data starts after 30-byte header + filename length.
        let fname_len = entries[0].name.len();
        let data_offset = offset + 30 + fname_len;
        if data_offset < archive.len() {
            archive[data_offset] ^= 0xFF;
        }

        let result = zip_extract_entry(&archive, &entries[0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_zip_find_eocd_basic() {
        let writer = ZipWriter::new();
        let archive = writer.finish();
        let offset = find_eocd(&archive).unwrap();
        let sig = u32::from_le_bytes([
            archive[offset], archive[offset+1], archive[offset+2], archive[offset+3],
        ]);
        assert_eq!(sig, SIG_EOCD);
    }

    #[test]
    fn test_zip_large_file() {
        // 64 KiB of repeating data — exercises stored-block path and hash chains.
        let input: Vec<u8> = (0u8..=255).cycle().take(65536).collect();
        let mut writer = ZipWriter::new();
        writer.add_file("large.bin", &input, 6, 0, 0).unwrap();
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, input);
    }

    #[test]
    fn test_zip_name_with_path() {
        let mut writer = ZipWriter::new();
        writer.add_file("a/b/c.txt", b"nested", 0, 0, 0).unwrap();
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries[0].name, "a/b/c.txt");

        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, b"nested");
    }

    // ---- archive_name helper ----

    #[test]
    fn test_archive_name_no_junk() {
        let path = Path::new("some/dir/file.txt");
        let base = Path::new("some/dir");
        let name = archive_name(path, base, false);
        assert_eq!(name, "file.txt");
    }

    #[test]
    fn test_archive_name_junk_paths() {
        let path = Path::new("some/dir/file.txt");
        let base = Path::new("some");
        let name = archive_name(path, base, true);
        assert_eq!(name, "file.txt");
    }

    // ---- LZ77 ----

    #[test]
    fn test_lz77_empty() {
        let tokens = lz77_compress(b"", 6);
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_lz77_short_no_match() {
        let tokens = lz77_compress(b"ab", 6);
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0], Token::Literal(b'a')));
        assert!(matches!(tokens[1], Token::Literal(b'b')));
    }

    #[test]
    fn test_lz77_finds_back_ref() {
        let tokens = lz77_compress(b"aaaaaa", 9);
        let has_match = tokens.iter().any(|t| matches!(t, Token::Match { .. }));
        assert!(has_match, "LZ77 should find a back-reference in 'aaaaaa'");
    }

    // ---- human_size ----

    #[test]
    fn test_human_size_bytes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn test_human_size_kib() {
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_human_size_mib() {
        assert_eq!(human_size(1024 * 1024), "1.0 MiB");
    }

    // ---- detect_mode ----

    #[test]
    fn test_detect_mode_zip() {
        assert_eq!(detect_mode("zip"), ToolMode::Zip);
        assert_eq!(detect_mode("/usr/bin/zip"), ToolMode::Zip);
    }

    #[test]
    fn test_detect_mode_unzip() {
        assert_eq!(detect_mode("unzip"), ToolMode::Unzip);
        assert_eq!(detect_mode("/bin/unzip"), ToolMode::Unzip);
    }

    // ---- parse_zip_args ----

    #[test]
    fn test_parse_zip_args_basic() {
        let args: Vec<String> = vec!["out.zip".into(), "file.txt".into()];
        let opts = parse_zip_args(&args).unwrap();
        assert_eq!(opts.archive, "out.zip");
        assert_eq!(opts.sources, vec!["file.txt".to_string()]);
        assert_eq!(opts.level, 6);
        assert!(!opts.recursive);
    }

    #[test]
    fn test_parse_zip_args_flags() {
        let args: Vec<String> = vec![
            "-r".into(), "-j".into(), "-9".into(), "-v".into(),
            "out.zip".into(), "dir/".into(),
        ];
        let opts = parse_zip_args(&args).unwrap();
        assert!(opts.recursive);
        assert!(opts.junk_paths);
        assert_eq!(opts.level, 9);
        assert!(opts.verbose);
    }

    #[test]
    fn test_parse_zip_args_exclude() {
        let args: Vec<String> = vec![
            "-x".into(), "*.log".into(),
            "out.zip".into(), "src/".into(),
        ];
        let opts = parse_zip_args(&args).unwrap();
        assert_eq!(opts.excludes, vec!["*.log".to_string()]);
    }

    // ---- parse_unzip_args ----

    #[test]
    fn test_parse_unzip_args_basic() {
        let args: Vec<String> = vec!["archive.zip".into()];
        let opts = parse_unzip_args(&args).unwrap();
        assert_eq!(opts.archive, "archive.zip");
        assert_eq!(opts.dest_dir, ".");
        assert!(!opts.list);
    }

    #[test]
    fn test_parse_unzip_args_flags() {
        let args: Vec<String> = vec![
            "-l".into(), "-d".into(), "/tmp/out".into(), "archive.zip".into(),
        ];
        let opts = parse_unzip_args(&args).unwrap();
        assert!(opts.list);
        assert_eq!(opts.dest_dir, "/tmp/out");
    }

    #[test]
    fn test_parse_unzip_args_specific_files() {
        let args: Vec<String> = vec![
            "archive.zip".into(), "a.txt".into(), "b.txt".into(),
        ];
        let opts = parse_unzip_args(&args).unwrap();
        assert_eq!(opts.files, vec!["a.txt".to_string(), "b.txt".to_string()]);
    }
}
