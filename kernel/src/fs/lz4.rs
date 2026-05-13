//! LZ4 block and frame format compression / decompression.
//!
//! Implements the LZ4 block format (raw sequences of literal/match pairs)
//! and the LZ4 frame format (file-level framing with magic number, frame
//! descriptor, data blocks, and optional content checksum).
//!
//! ## LZ4 block format
//!
//! ```text
//! [token] [extra literal len?] [literals] [offset:u16 LE] [extra match len?]
//!   repeated N times ...
//! [token] [extra literal len?] [literals]   ← last sequence (no match)
//! ```
//!
//! Token byte: `(literal_len << 4) | match_len`
//! - literal_len 0–14 inline; 15 → variable-length encoding follows
//! - match_len 0–14 inline (decoded = field + MINMATCH = field + 4);
//!   15 → variable-length encoding follows
//! - Last sequence has no match portion (no offset, no extra match len)
//!
//! ## LZ4 frame format (v1.6.3 / spec 1.6.1)
//!
//! ```text
//! ┌────────────────────────────────────────────┐
//! │ Magic number: 0x184D2204 (LE)             │  4 bytes
//! ├────────────────────────────────────────────┤
//! │ Frame descriptor:                         │
//! │   FLG byte, BD byte, [Content Size],      │
//! │   Header Checksum (xxHash-32 >> 8)        │  3–15 bytes
//! ├────────────────────────────────────────────┤
//! │ Data block 0:                             │
//! │   Block Size (4 bytes LE)                 │
//! │     bit 31 = 1 → uncompressed             │
//! │   Block data                              │
//! ├────────────────────────────────────────────┤
//! │ ...more data blocks...                    │
//! ├────────────────────────────────────────────┤
//! │ End mark: 0x00000000 (4 bytes)            │
//! ├────────────────────────────────────────────┤
//! │ Content checksum (xxHash-32, optional)     │  4 bytes
//! └────────────────────────────────────────────┘
//! ```
//!
//! ## References
//!
//! - LZ4 block format: <https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md>
//! - LZ4 frame format: <https://github.com/lz4/lz4/blob/dev/doc/lz4_Frame_format.md>
//! - xxHash-32: <https://github.com/Cyan4973/xxHash/blob/dev/doc/xxhash_spec.md>

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// LZ4 frame magic number (little-endian 0x184D2204).
const FRAME_MAGIC: u32 = 0x04224D18;

/// Minimum match length — matches shorter than 4 bytes aren't encoded.
const MINMATCH: usize = 4;

/// Maximum back-reference distance (64 KiB - 1).
const MAX_DISTANCE: usize = 65535;

/// Hash table size for the compressor (power of 2).
/// 16384 entries × 4 bytes = 64 KiB — good trade-off for general data.
const HASH_TABLE_SIZE: usize = 1 << 14;
const HASH_TABLE_MASK: usize = HASH_TABLE_SIZE - 1;

/// Default maximum block size (64 KiB — block size ID 4).
const DEFAULT_BLOCK_MAX_SIZE: usize = 64 * 1024;

/// The last 5 bytes of the input are always emitted as literals
/// (the encoder must not search for matches in the last 5 bytes,
/// because a match needs ≥4 bytes and at least 1 trailing literal).
const LAST_LITERALS: usize = 5;

/// The last 12 bytes of a match-copy must not overlap with the output end
/// (for safe fast-copy implementations).  We use a simpler check:
/// ensure at least MFLIMIT bytes remain when looking for matches.
const MFLIMIT: usize = 12;

// ---------------------------------------------------------------------------
// xxHash-32 (for frame / content checksums)
// ---------------------------------------------------------------------------

/// xxHash-32 constants.
const XXH_PRIME32_1: u32 = 0x9E3779B1;
const XXH_PRIME32_2: u32 = 0x85EBCA77;
const XXH_PRIME32_3: u32 = 0xC2B2AE3D;
const XXH_PRIME32_4: u32 = 0x27D4EB2F;
const XXH_PRIME32_5: u32 = 0x165667B1;

/// Compute xxHash-32 of `data` with the given `seed`.
fn xxhash32(data: &[u8], seed: u32) -> u32 {
    let len = data.len();
    let mut h: u32;

    if len >= 16 {
        let mut v1 = seed.wrapping_add(XXH_PRIME32_1).wrapping_add(XXH_PRIME32_2);
        let mut v2 = seed.wrapping_add(XXH_PRIME32_2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(XXH_PRIME32_1);

        let limit = len - 16;
        let mut i: usize = 0;
        while i <= limit {
            v1 = xxh32_round(v1, read_le32(data, i));
            v2 = xxh32_round(v2, read_le32(data, i + 4));
            v3 = xxh32_round(v3, read_le32(data, i + 8));
            v4 = xxh32_round(v4, read_le32(data, i + 12));
            i += 16;
        }

        h = v1.rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
    } else {
        h = seed.wrapping_add(XXH_PRIME32_5);
    }

    h = h.wrapping_add(len as u32);

    // Consume remaining 4-byte chunks.
    let mut i = len & !15; // Start after the 16-byte-aligned blocks
    if len < 16 {
        i = 0;
    }
    while i + 4 <= len {
        h = h.wrapping_add(read_le32(data, i).wrapping_mul(XXH_PRIME32_3));
        h = h.rotate_left(17).wrapping_mul(XXH_PRIME32_4);
        i += 4;
    }

    // Consume remaining bytes.
    while i < len {
        h = h.wrapping_add(u32::from(data[i]).wrapping_mul(XXH_PRIME32_5));
        h = h.rotate_left(11).wrapping_mul(XXH_PRIME32_1);
        i += 1;
    }

    // Final avalanche.
    h ^= h >> 15;
    h = h.wrapping_mul(XXH_PRIME32_2);
    h ^= h >> 13;
    h = h.wrapping_mul(XXH_PRIME32_3);
    h ^= h >> 16;
    h
}

#[inline(always)]
fn xxh32_round(acc: u32, input: u32) -> u32 {
    acc.wrapping_add(input.wrapping_mul(XXH_PRIME32_2))
        .rotate_left(13)
        .wrapping_mul(XXH_PRIME32_1)
}

// ---------------------------------------------------------------------------
// Little-endian helpers
// ---------------------------------------------------------------------------

#[inline(always)]
fn read_le16(data: &[u8], off: usize) -> u16 {
    let b0 = *data.get(off).unwrap_or(&0);
    let b1 = *data.get(off + 1).unwrap_or(&0);
    u16::from(b0) | (u16::from(b1) << 8)
}

#[inline(always)]
fn read_le32(data: &[u8], off: usize) -> u32 {
    let b0 = *data.get(off).unwrap_or(&0);
    let b1 = *data.get(off + 1).unwrap_or(&0);
    let b2 = *data.get(off + 2).unwrap_or(&0);
    let b3 = *data.get(off + 3).unwrap_or(&0);
    u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16) | (u32::from(b3) << 24)
}

// ---------------------------------------------------------------------------
// LZ4 block format — decompression
// ---------------------------------------------------------------------------

/// Decompress an LZ4 block.
///
/// `src` contains a raw LZ4-compressed block (no frame headers).
/// `max_output` is the maximum expected decompressed size; prevents
/// runaway allocation on corrupted input.
///
/// # Errors
///
/// Returns `CorruptedData` if the block is malformed (truncated token,
/// invalid back-reference, etc.).
pub fn decompress_block(src: &[u8], max_output: usize) -> KernelResult<Vec<u8>> {
    let mut out = Vec::with_capacity(max_output.min(src.len().saturating_mul(4)));
    let slen = src.len();
    let mut ip: usize = 0; // Input position.

    loop {
        if ip >= slen {
            break;
        }

        // --- Read token ---
        let token = src[ip];
        ip += 1;

        // --- Literal length ---
        let mut lit_len = usize::from(token >> 4);
        if lit_len == 15 {
            loop {
                if ip >= slen {
                    return Err(KernelError::CorruptedData);
                }
                let extra = src[ip];
                ip += 1;
                lit_len = lit_len.wrapping_add(usize::from(extra));
                if extra != 255 {
                    break;
                }
            }
        }

        // --- Copy literals ---
        if ip.wrapping_add(lit_len) > slen {
            return Err(KernelError::CorruptedData);
        }
        if out.len().wrapping_add(lit_len) > max_output {
            return Err(KernelError::CorruptedData);
        }
        out.extend_from_slice(&src[ip..ip.wrapping_add(lit_len)]);
        ip = ip.wrapping_add(lit_len);

        // After the last literal run there is no match portion.
        if ip >= slen {
            break;
        }

        // --- Match offset ---
        if ip + 2 > slen {
            return Err(KernelError::CorruptedData);
        }
        let offset = usize::from(read_le16(src, ip));
        ip += 2;

        if offset == 0 {
            return Err(KernelError::CorruptedData);
        }
        if offset > out.len() {
            return Err(KernelError::CorruptedData);
        }

        // --- Match length ---
        let mut match_len = usize::from(token & 0x0F).wrapping_add(MINMATCH);
        if (token & 0x0F) == 15 {
            loop {
                if ip >= slen {
                    return Err(KernelError::CorruptedData);
                }
                let extra = src[ip];
                ip += 1;
                match_len = match_len.wrapping_add(usize::from(extra));
                if extra != 255 {
                    break;
                }
            }
        }

        if out.len().wrapping_add(match_len) > max_output {
            return Err(KernelError::CorruptedData);
        }

        // --- Copy match (may overlap — byte-by-byte is correct) ---
        let match_start = out.len().wrapping_sub(offset);
        for i in 0..match_len {
            let b = out[match_start.wrapping_add(i)];
            out.push(b);
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// LZ4 block format — compression
// ---------------------------------------------------------------------------

/// Hash 4 bytes for the match-finder hash table.
#[inline(always)]
fn hash4(data: &[u8], pos: usize) -> usize {
    let v = read_le32(data, pos);
    // Knuth multiplicative hash, shift to table size.
    ((v.wrapping_mul(2654435761)) >> (32 - 14)) as usize & HASH_TABLE_MASK
}

/// Compress `src` into a raw LZ4 block (no frame headers).
///
/// Returns the compressed data.  If the input is empty, returns an
/// empty Vec (the frame layer handles this).
pub fn compress_block(src: &[u8]) -> Vec<u8> {
    if src.is_empty() {
        return Vec::new();
    }

    let slen = src.len();

    // For very short inputs, just emit as literals.
    if slen < MFLIMIT {
        return emit_all_literals(src);
    }

    let mut out = Vec::with_capacity(slen);
    let mut hash_table = vec![0u32; HASH_TABLE_SIZE];

    let mut anchor: usize = 0; // Start of current literal run.
    let mut ip: usize = 0; // Current input position.

    let match_limit = slen.saturating_sub(LAST_LITERALS);

    ip += 1; // Skip first byte — no previous data to match against.

    loop {
        if ip >= match_limit {
            break;
        }

        // --- Find a match ---
        let h = hash4(src, ip);
        let candidate = hash_table[h] as usize;
        hash_table[h] = ip as u32;

        // Check if the candidate is valid: within range and 4-byte match.
        let dist = ip.wrapping_sub(candidate);
        if dist == 0 || dist > MAX_DISTANCE || candidate >= ip
            || read_le32(src, candidate) != read_le32(src, ip)
        {
            ip += 1;
            continue;
        }

        // --- Extend match forward ---
        // LZ4 supports overlapping matches (offset < match_length) — the
        // decoder copies byte-by-byte so repeating patterns are produced
        // correctly.  We compare against the original source data which
        // already has all bytes in place.
        let match_start_src = candidate;
        let match_start_dst = ip;
        let mut mlen: usize = MINMATCH;

        while match_start_dst.wrapping_add(mlen) < slen {
            if src[match_start_src.wrapping_add(mlen)]
                != src[match_start_dst.wrapping_add(mlen)]
            {
                break;
            }
            mlen += 1;
        }

        // Limit match so it doesn't eat into the last LAST_LITERALS bytes.
        let max_mlen = slen.saturating_sub(LAST_LITERALS).saturating_sub(match_start_dst);
        if mlen > max_mlen {
            mlen = max_mlen;
        }
        if mlen < MINMATCH {
            ip += 1;
            continue;
        }

        // --- Emit sequence: literals + match ---
        let lit_len = ip.wrapping_sub(anchor);
        emit_sequence(&mut out, src, anchor, lit_len, dist, mlen);

        ip = ip.wrapping_add(mlen);
        anchor = ip;

        // Update hash for skipped positions.
        if ip < match_limit {
            hash_table[hash4(src, ip.wrapping_sub(2))] = ip.wrapping_sub(2) as u32;
        }
    }

    // --- Emit remaining literals ---
    let remaining = slen.wrapping_sub(anchor);
    emit_last_literals(&mut out, src, anchor, remaining);

    out
}

/// Emit a complete LZ4 sequence (literals + match).
fn emit_sequence(
    out: &mut Vec<u8>,
    src: &[u8],
    lit_start: usize,
    lit_len: usize,
    offset: usize,
    match_len: usize,
) {
    let ml_field = match_len.wrapping_sub(MINMATCH);

    // Token byte.
    let lit_token = if lit_len >= 15 { 15u8 } else { lit_len as u8 };
    let ml_token = if ml_field >= 15 { 15u8 } else { ml_field as u8 };
    out.push((lit_token << 4) | ml_token);

    // Extra literal length bytes.
    if lit_len >= 15 {
        let mut rem = lit_len.wrapping_sub(15);
        while rem >= 255 {
            out.push(255);
            rem = rem.wrapping_sub(255);
        }
        out.push(rem as u8);
    }

    // Literal data.
    let end = lit_start.wrapping_add(lit_len).min(src.len());
    out.extend_from_slice(&src[lit_start..end]);

    // Match offset (LE u16).
    out.push(offset as u8);
    out.push((offset >> 8) as u8);

    // Extra match length bytes.
    if ml_field >= 15 {
        let mut rem = ml_field.wrapping_sub(15);
        while rem >= 255 {
            out.push(255);
            rem = rem.wrapping_sub(255);
        }
        out.push(rem as u8);
    }
}

/// Emit all remaining data as a final literal-only sequence (no match).
fn emit_last_literals(out: &mut Vec<u8>, src: &[u8], start: usize, len: usize) {
    if len == 0 {
        return;
    }

    // Token byte (match_len = 0 means no match portion).
    let lit_token = if len >= 15 { 15u8 } else { len as u8 };
    out.push(lit_token << 4);

    // Extra literal length bytes.
    if len >= 15 {
        let mut rem = len.wrapping_sub(15);
        while rem >= 255 {
            out.push(255);
            rem = rem.wrapping_sub(255);
        }
        out.push(rem as u8);
    }

    // Literal data.
    let end = start.wrapping_add(len).min(src.len());
    out.extend_from_slice(&src[start..end]);
}

/// Emit the entire input as a single literal-only block (for short inputs).
fn emit_all_literals(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + 10);
    emit_last_literals(&mut out, src, 0, src.len());
    out
}

// ---------------------------------------------------------------------------
// LZ4 frame format — decompression
// ---------------------------------------------------------------------------

/// Decompress an LZ4 frame (the standard `.lz4` file format).
///
/// Parses the frame header, decompresses each data block, verifies the
/// optional content checksum, and returns the original data.
///
/// # Errors
///
/// Returns `CorruptedData` on magic mismatch, unsupported version,
/// truncated data, bad header checksum, bad content checksum, or any
/// block-level error.
pub fn decompress(data: &[u8]) -> KernelResult<Vec<u8>> {
    if data.len() < 7 {
        return Err(KernelError::CorruptedData);
    }

    // --- Magic ---
    let magic = read_le32(data, 0);
    if magic != FRAME_MAGIC {
        return Err(KernelError::CorruptedData);
    }

    // --- Frame descriptor ---
    let flg = data[4];
    let bd = data[5];

    let version = (flg >> 6) & 0x03;
    if version != 1 {
        return Err(KernelError::CorruptedData);
    }

    let block_independence = (flg >> 5) & 1 != 0;
    let _block_checksum = (flg >> 4) & 1 != 0;
    let content_size_present = (flg >> 3) & 1 != 0;
    let content_checksum = (flg >> 2) & 1 != 0;
    let _dict_id_present = flg & 1 != 0;

    let _block_max_size_id = (bd >> 4) & 0x07;
    let _ = block_independence; // We handle both modes the same (no dict).

    let mut pos: usize = 6;

    // Optional content size (8 bytes LE).
    let _content_size: u64 = if content_size_present {
        if pos + 8 > data.len() {
            return Err(KernelError::CorruptedData);
        }
        let lo = read_le32(data, pos) as u64;
        let hi = read_le32(data, pos + 4) as u64;
        pos += 8;
        lo | (hi << 32)
    } else {
        0
    };

    // Header checksum (xxHash-32 of FLG+BD+optional fields, >> 8, lowest byte).
    let hc_byte = *data.get(pos).ok_or(KernelError::CorruptedData)?;
    let descriptor_bytes = &data[4..pos];
    let hc_computed = ((xxhash32(descriptor_bytes, 0) >> 8) & 0xFF) as u8;
    if hc_byte != hc_computed {
        return Err(KernelError::CorruptedData);
    }
    pos += 1;

    // --- Data blocks ---
    let block_max = block_max_size_from_id(_block_max_size_id);
    let mut output = Vec::new();

    loop {
        if pos + 4 > data.len() {
            return Err(KernelError::CorruptedData);
        }

        let block_header = read_le32(data, pos);
        pos += 4;

        // End mark.
        if block_header == 0 {
            break;
        }

        let is_uncompressed = (block_header >> 31) != 0;
        let block_size = (block_header & 0x7FFF_FFFF) as usize;

        if block_size > block_max || pos + block_size > data.len() {
            return Err(KernelError::CorruptedData);
        }

        if is_uncompressed {
            output.extend_from_slice(&data[pos..pos + block_size]);
        } else {
            let block_data = &data[pos..pos + block_size];
            let decompressed = decompress_block(block_data, block_max)?;
            output.extend_from_slice(&decompressed);
        }

        pos += block_size;

        // Optional per-block checksum (4 bytes, xxHash-32).
        if _block_checksum {
            if pos + 4 > data.len() {
                return Err(KernelError::CorruptedData);
            }
            // We skip verification of per-block checksum for now
            // (content checksum covers the whole frame).
            pos += 4;
        }
    }

    // --- Content checksum ---
    if content_checksum {
        if pos + 4 > data.len() {
            return Err(KernelError::CorruptedData);
        }
        let expected = read_le32(data, pos);
        let computed = xxhash32(&output, 0);
        if expected != computed {
            return Err(KernelError::CorruptedData);
        }
        // pos += 4;
    }

    Ok(output)
}

/// Map block-max-size ID to actual byte limit.
fn block_max_size_from_id(id: u8) -> usize {
    match id {
        4 => 64 * 1024,       //  64 KiB
        5 => 256 * 1024,      // 256 KiB
        6 => 1024 * 1024,     //   1 MiB
        7 => 4 * 1024 * 1024, //   4 MiB
        _ => DEFAULT_BLOCK_MAX_SIZE,
    }
}

// ---------------------------------------------------------------------------
// LZ4 frame format — compression
// ---------------------------------------------------------------------------

/// Compress `src` into a standard LZ4 frame (`.lz4` file format).
///
/// Produces a single frame with:
/// - Version 01, block independence, no dict
/// - Block max size ID 4 (64 KiB)
/// - Content checksum enabled
/// - Content size present
///
/// Compatible with the `lz4` CLI tool (`lz4 -d file.lz4`).
pub fn compress(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len().wrapping_add(64));

    // --- Magic ---
    extend4_val(&mut out, FRAME_MAGIC);

    // --- Frame descriptor ---
    // FLG: version=01, block_independence=1, content_size=1, content_checksum=1
    let flg: u8 = (1 << 6)  // version = 01
               | (1 << 5)   // block independence
               | (1 << 3)   // content size present
               | (1 << 2);  // content checksum
    // BD: block max size ID = 4 (64 KiB)
    let bd: u8 = 4 << 4;

    out.push(flg);
    out.push(bd);

    // Content size (8 bytes LE).
    let size = src.len() as u64;
    for i in 0..8u32 {
        out.push((size >> (i * 8)) as u8);
    }

    // Header checksum — xxHash-32 of descriptor bytes (FLG + BD + content size).
    let desc_start = out.len() - 10; // FLG(1) + BD(1) + content_size(8) = 10
    let hc = ((xxhash32(&out[desc_start..], 0) >> 8) & 0xFF) as u8;
    out.push(hc);

    // --- Data blocks ---
    let mut offset: usize = 0;
    while offset < src.len() {
        let chunk_end = (offset + DEFAULT_BLOCK_MAX_SIZE).min(src.len());
        let chunk = &src[offset..chunk_end];

        let compressed = compress_block(chunk);

        // If compressed is not smaller, store uncompressed.
        if compressed.len() >= chunk.len() {
            // Uncompressed block: bit 31 set.
            let block_header = (chunk.len() as u32) | 0x8000_0000;
            extend4_val(&mut out, block_header);
            out.extend_from_slice(chunk);
        } else {
            let block_header = compressed.len() as u32;
            extend4_val(&mut out, block_header);
            out.extend_from_slice(&compressed);
        }

        offset = chunk_end;
    }

    // --- End mark ---
    extend4_val(&mut out, 0);

    // --- Content checksum ---
    let checksum = xxhash32(src, 0);
    extend4_val(&mut out, checksum);

    out
}

/// Helper: push a LE u32.
fn extend4_val(out: &mut Vec<u8>, val: u32) {
    out.push(val as u8);
    out.push((val >> 8) as u8);
    out.push((val >> 16) as u8);
    out.push((val >> 24) as u8);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run LZ4 module self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[lz4] Running self-test...");

    // --- Test 1: block-level round-trip (short text) ---
    {
        let input = b"Hello, world! Hello, world! Hello, hello!";
        let compressed = compress_block(input);
        let decompressed = decompress_block(&compressed, 256)?;
        if decompressed.as_slice() != input {
            serial_println!("[lz4]   ERROR: block round-trip mismatch");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[lz4]   block round-trip OK ({}B -> {}B)", input.len(), compressed.len());
    }

    // --- Test 2: frame-level round-trip ---
    {
        let input = b"The quick brown fox jumps over the lazy dog. \
                       The quick brown fox jumps over the lazy dog.";
        let frame = compress(input);
        let decompressed = decompress(&frame)?;
        if decompressed.as_slice() != input {
            serial_println!("[lz4]   ERROR: frame round-trip mismatch");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[lz4]   frame round-trip OK ({}B -> {}B)", input.len(), frame.len());
    }

    // --- Test 3: empty input ---
    {
        let input: &[u8] = b"";
        let frame = compress(input);
        let decompressed = decompress(&frame)?;
        if !decompressed.is_empty() {
            serial_println!("[lz4]   ERROR: empty input produced non-empty output");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[lz4]   empty input OK");
    }

    // --- Test 4: incompressible data (random-ish) ---
    {
        let mut input = vec![0u8; 256];
        // Generate pseudo-random data using a simple LCG.
        let mut state: u32 = 0xDEAD_BEEF;
        for byte in input.iter_mut() {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            *byte = (state >> 16) as u8;
        }
        let frame = compress(&input);
        let decompressed = decompress(&frame)?;
        if decompressed != input {
            serial_println!("[lz4]   ERROR: incompressible round-trip mismatch");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[lz4]   incompressible data OK ({}B -> {}B)", input.len(), frame.len());
    }

    // --- Test 5: highly repetitive data ---
    {
        let input = vec![0x42u8; 4096];
        let frame = compress(&input);
        let decompressed = decompress(&frame)?;
        if decompressed != input {
            serial_println!("[lz4]   ERROR: repetitive round-trip mismatch");
            return Err(KernelError::CorruptedData);
        }
        let ratio = if !frame.is_empty() {
            (frame.len() * 100) / input.len()
        } else {
            0
        };
        serial_println!("[lz4]   repetitive data OK (4096B -> {}B, {}%)", frame.len(), ratio);
    }

    // --- Test 6: magic validation ---
    {
        let bad_data = [0xAA; 32];
        if decompress(&bad_data).is_ok() {
            serial_println!("[lz4]   ERROR: bad magic not detected");
            return Err(KernelError::CorruptedData);
        }
        if decompress(&[]).is_ok() {
            serial_println!("[lz4]   ERROR: empty data not rejected");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[lz4]   magic validation OK");
    }

    // --- Test 7: content checksum verification ---
    {
        let input = b"checksum test data here";
        let mut frame = compress(input);
        // Corrupt the last byte (part of the content checksum).
        if let Some(last) = frame.last_mut() {
            *last ^= 0xFF;
        }
        match decompress(&frame) {
            Err(KernelError::CorruptedData) => {}
            Ok(_) => {
                serial_println!("[lz4]   ERROR: corrupted checksum not detected");
                return Err(KernelError::CorruptedData);
            }
            Err(e) => return Err(e),
        }
        serial_println!("[lz4]   content checksum verification OK");
    }

    // --- Test 8: xxHash-32 known vectors ---
    {
        // From the xxHash reference: xxh32("", 0) = 0x02CC5D05
        let h_empty = xxhash32(b"", 0);
        if h_empty != 0x02CC5D05 {
            serial_println!("[lz4]   ERROR: xxHash-32(\"\") = {:#010X}, expected 0x02CC5D05", h_empty);
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[lz4]   xxHash-32 known vectors OK");
    }

    // --- Test 9: multi-block (data > 64 KiB) ---
    {
        // Create data larger than one block (64 KiB).
        let mut input = Vec::with_capacity(100_000);
        for i in 0u32..25_000 {
            input.push((i & 0xFF) as u8);
            input.push(((i >> 8) & 0xFF) as u8);
            input.push(((i >> 16) & 0xFF) as u8);
            input.push(((i >> 24) & 0xFF) as u8);
        }
        let frame = compress(&input);
        let decompressed = decompress(&frame)?;
        if decompressed != input {
            serial_println!("[lz4]   ERROR: multi-block round-trip mismatch");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[lz4]   multi-block OK (100000B -> {}B)", frame.len());
    }

    serial_println!("[lz4] Self-test passed.");
    Ok(())
}
