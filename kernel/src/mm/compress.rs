//! Fast in-memory page compression for zswap/zram.
//!
//! Implements a minimal LZ4-like compressor optimized for 16 KiB pages.
//! The algorithm uses a hash table to find 4-byte matches in a sliding
//! window, then encodes runs of literals and back-references.
//!
//! ## Why not full LZ4?
//!
//! Full LZ4 has many edge cases around frame headers, block sizes, and
//! checksums that add complexity without benefiting single-page
//! compression.  This is a simplified "LZ4 core" that handles the
//! match-finding and encoding only.
//!
//! ## Wire format
//!
//! ```text
//! [token] [extra literal len?] [literals] [offset:u16 LE] [extra match len?]
//!   repeated N times ...
//! [token] [extra literal len?] [literals]   ← last sequence (no match)
//! ```
//!
//! Token byte: `(literal_len << 4) | match_len`
//! - literal_len: 0–14 inline, 15 → read additional bytes (each 255
//!   adds to total, non-255 terminates)
//! - match_len: 0–14 inline (actual min match = 4, so decoded
//!   length = field + 4), 15 → additional bytes
//! - Last sequence has match_len = 0 and no offset field
//!
//! ## Special cases
//!
//! - **All-zero page**: encoded as a single magic byte `0xFF` (1 byte).
//!   Very common for freshly mapped BSS and stack pages.
//! - **Incompressible page**: if compressed size ≥ original, returns
//!   `None` (caller stores uncompressed).

use alloc::vec;
use alloc::vec::Vec;

/// Magic byte indicating an all-zero page (single-byte encoding).
const ZERO_PAGE_MARKER: u8 = 0xFF;

/// Minimum match length (matches < 4 bytes aren't worth encoding).
const MIN_MATCH: usize = 4;

/// Hash table size (must be power of 2).  4096 entries covers 16 KiB
/// pages well — each entry is an offset into the source data.
const HASH_TABLE_SIZE: usize = 4096;
const HASH_TABLE_MASK: usize = HASH_TABLE_SIZE - 1;

/// Maximum output size factor.  If compressed output exceeds this
/// fraction of input, compression is not worthwhile.
/// We allow up to 100% of input size (incompressible data).
const MAX_OUTPUT_FACTOR: usize = 1;

/// Compress a page.
///
/// Returns `Some(compressed_data)` if compression is beneficial (the
/// output is smaller than the input), or `None` if the data is
/// incompressible.
///
/// # Special cases
///
/// - All-zero input → returns `vec![0xFF]` (1 byte).
/// - Incompressible → returns `None`.
pub fn compress(input: &[u8]) -> Option<Vec<u8>> {
    if input.is_empty() {
        return Some(Vec::new());
    }

    // Fast path: check for all-zero page.
    if is_all_zero(input) {
        return Some(vec![ZERO_PAGE_MARKER]);
    }

    let input_len = input.len();
    // Worst case: each byte is a literal → ~(1 + input_len) output.
    // Pre-allocate a reasonable buffer.
    let mut output = Vec::with_capacity(input_len);

    // Hash table: maps 4-byte sequence hash → position in input.
    let mut hash_table = vec![0u32; HASH_TABLE_SIZE];

    let mut ip = 0usize; // Input position (current scan point).
    let mut anchor = 0usize; // Start of unmatched literal run.

    // We need at least MIN_MATCH bytes to encode a match at the end.
    let input_limit = input_len.saturating_sub(5);
    let match_limit = input_len.saturating_sub(MIN_MATCH);

    while ip < input_limit {
        // Hash the 4 bytes at the current position.
        let h = hash4(input, ip);
        let candidate = hash_table.get(h).copied().unwrap_or(0) as usize;

        // Update the hash table with the current position.
        if let Some(slot) = hash_table.get_mut(h) {
            #[allow(clippy::cast_possible_truncation)]
            {
                *slot = ip as u32;
            }
        }

        // Check if the candidate matches.
        let offset = ip.wrapping_sub(candidate);
        if offset == 0
            || offset > 0xFFFF
            || candidate >= ip
            || !matches_at(input, candidate, ip)
        {
            ip += 1;
            continue;
        }

        // Found a match!  Extend the match forward as far as possible.
        let match_len = extend_match(input, candidate, ip, match_limit);

        if match_len < MIN_MATCH {
            ip += 1;
            continue;
        }

        // Encode: [literals from anchor..ip] [match at offset, match_len]
        let literal_len = ip.saturating_sub(anchor);

        // Write the token and the literal/match sequence.
        if !write_sequence(
            &mut output,
            input,
            anchor,
            literal_len,
            offset,
            match_len,
            false,
        ) {
            return None; // Output overflow.
        }

        // Advance past the match.
        ip = ip.saturating_add(match_len);
        anchor = ip;
    }

    // Write the final literal run (no match).
    let final_literal_len = input_len.saturating_sub(anchor);
    if final_literal_len > 0 || anchor < input_len {
        let lit_len = input_len.saturating_sub(anchor);
        if !write_sequence(&mut output, input, anchor, lit_len, 0, 0, true) {
            return None;
        }
    }

    // Only return compressed if it's actually smaller.
    let max_size = input_len.saturating_mul(MAX_OUTPUT_FACTOR);
    if output.len() >= max_size {
        return None;
    }

    Some(output)
}

/// Decompress data produced by [`compress`].
///
/// `output_len` is the expected decompressed size (the original page
/// size).  Returns `Err` if the compressed data is malformed.
pub fn decompress(input: &[u8], output_len: usize) -> Result<Vec<u8>, CompressError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    // Check for all-zero page marker.
    if input.len() == 1 && input.first().copied() == Some(ZERO_PAGE_MARKER) {
        return Ok(vec![0u8; output_len]);
    }

    let mut output = Vec::with_capacity(output_len);
    let mut ip = 0usize; // Position in input.

    while ip < input.len() {
        let token = *input.get(ip).ok_or(CompressError::Truncated)?;
        ip += 1;

        // --- Literals ---
        #[allow(clippy::arithmetic_side_effects)]
        let mut literal_len = ((token >> 4) & 0x0F) as usize;
        if literal_len == 15 {
            loop {
                let extra = *input.get(ip).ok_or(CompressError::Truncated)?;
                ip += 1;
                literal_len = literal_len.saturating_add(extra as usize);
                if extra != 255 {
                    break;
                }
            }
        }

        // Copy literals.
        if ip.saturating_add(literal_len) > input.len() {
            return Err(CompressError::Truncated);
        }
        let literal_end = ip.saturating_add(literal_len);
        for &b in input.get(ip..literal_end).ok_or(CompressError::Truncated)? {
            output.push(b);
        }
        ip = literal_end;

        // If we've reached the end of input, this was the last sequence
        // (no match component).
        if ip >= input.len() {
            break;
        }

        // --- Match ---
        // Read 2-byte little-endian offset.
        let lo = *input.get(ip).ok_or(CompressError::Truncated)? as u16;
        let hi = *input.get(ip.saturating_add(1)).ok_or(CompressError::Truncated)? as u16;
        ip = ip.saturating_add(2);
        let offset = (hi << 8) | lo;

        if offset == 0 {
            return Err(CompressError::InvalidOffset);
        }

        #[allow(clippy::arithmetic_side_effects)]
        let mut match_len = (token & 0x0F) as usize;
        match_len = match_len.saturating_add(MIN_MATCH); // Min match = 4.

        if match_len >= MIN_MATCH + 15 {
            // match field was 15 → read extra length bytes.
            loop {
                let extra = *input.get(ip).ok_or(CompressError::Truncated)?;
                ip += 1;
                match_len = match_len.saturating_add(extra as usize);
                if extra != 255 {
                    break;
                }
            }
        }

        // Copy from the match position in the output buffer.
        let match_start = output.len().wrapping_sub(offset as usize);
        if match_start >= output.len() {
            return Err(CompressError::InvalidOffset);
        }

        // Copy byte-by-byte (handles overlapping matches).
        for i in 0..match_len {
            let src_idx = match_start.saturating_add(i % (offset as usize));
            let b = output.get(src_idx).copied().ok_or(CompressError::InvalidOffset)?;
            output.push(b);
        }
    }

    if output.len() != output_len {
        return Err(CompressError::SizeMismatch {
            expected: output_len,
            got: output.len(),
        });
    }

    Ok(output)
}

/// Compression error types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressError {
    /// Input data is truncated (unexpected end of stream).
    Truncated,
    /// Match offset points outside the output buffer.
    InvalidOffset,
    /// Decompressed size doesn't match the expected output length.
    SizeMismatch { expected: usize, got: usize },
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if the input is all zeros.
fn is_all_zero(data: &[u8]) -> bool {
    // Check 8 bytes at a time for speed.
    let (prefix, chunks, suffix) = {
        // SAFETY: alignment cast for u64 chunks is safe because we
        // only read through it.
        unsafe { data.align_to::<u64>() }
    };
    prefix.iter().all(|&b| b == 0)
        && chunks.iter().all(|&w| w == 0)
        && suffix.iter().all(|&b| b == 0)
}

/// Hash 4 bytes at `pos` into a hash table index.
#[inline]
fn hash4(data: &[u8], pos: usize) -> usize {
    let v = read_u32_le(data, pos);
    // Multiplicative hash (Knuth's golden ratio).
    #[allow(clippy::arithmetic_side_effects)]
    let h = v.wrapping_mul(2654435761) >> 20;
    (h as usize) & HASH_TABLE_MASK
}

/// Read a little-endian u32 from `data` at `pos`.
#[inline]
fn read_u32_le(data: &[u8], pos: usize) -> u32 {
    // Bounds check — return 0 for OOB.
    if pos.saturating_add(4) > data.len() {
        return 0;
    }
    let b = data.get(pos..pos.saturating_add(4)).unwrap_or(&[0; 4]);
    u32::from_le_bytes([
        b.first().copied().unwrap_or(0),
        b.get(1).copied().unwrap_or(0),
        b.get(2).copied().unwrap_or(0),
        b.get(3).copied().unwrap_or(0),
    ])
}

/// Check if 4 bytes match at positions `a` and `b`.
#[inline]
fn matches_at(data: &[u8], a: usize, b: usize) -> bool {
    read_u32_le(data, a) == read_u32_le(data, b)
}

/// Extend a match starting at `src` and `dst` forward, returning the
/// total match length.
fn extend_match(data: &[u8], src: usize, dst: usize, limit: usize) -> usize {
    let mut len = MIN_MATCH; // We already know 4 bytes match.

    while dst.saturating_add(len) < limit
        && src.saturating_add(len) < data.len()
        && data.get(src.saturating_add(len)) == data.get(dst.saturating_add(len))
    {
        len = len.saturating_add(1);
    }

    len
}

/// Write a literal+match sequence to the output buffer.
///
/// Returns `false` if the output would exceed reasonable bounds.
fn write_sequence(
    output: &mut Vec<u8>,
    input: &[u8],
    literal_start: usize,
    literal_len: usize,
    offset: usize,
    match_len: usize,
    is_last: bool,
) -> bool {
    // Token byte.
    let lit_field = if literal_len >= 15 { 15u8 } else { literal_len as u8 };
    let mat_field = if is_last {
        0u8
    } else if match_len.saturating_sub(MIN_MATCH) >= 15 {
        15u8
    } else {
        #[allow(clippy::cast_possible_truncation)]
        {
            match_len.saturating_sub(MIN_MATCH) as u8
        }
    };

    #[allow(clippy::arithmetic_side_effects)]
    let token = (lit_field << 4) | mat_field;
    output.push(token);

    // Extra literal length bytes.
    if literal_len >= 15 {
        let mut remaining = literal_len.saturating_sub(15);
        loop {
            if remaining >= 255 {
                output.push(255);
                remaining = remaining.saturating_sub(255);
            } else {
                #[allow(clippy::cast_possible_truncation)]
                output.push(remaining as u8);
                break;
            }
        }
    }

    // Literal bytes.
    let lit_end = literal_start.saturating_add(literal_len);
    if let Some(lits) = input.get(literal_start..lit_end) {
        output.extend_from_slice(lits);
    } else {
        return false;
    }

    // Match (offset + optional extra length) — only if not last sequence.
    if !is_last {
        // 2-byte LE offset.
        #[allow(clippy::cast_possible_truncation)]
        {
            output.push((offset & 0xFF) as u8);
            output.push(((offset >> 8) & 0xFF) as u8);
        }

        // Extra match length bytes.
        let adj_match = match_len.saturating_sub(MIN_MATCH);
        if adj_match >= 15 {
            let mut remaining = adj_match.saturating_sub(15);
            loop {
                if remaining >= 255 {
                    output.push(255);
                    remaining = remaining.saturating_sub(255);
                } else {
                    #[allow(clippy::cast_possible_truncation)]
                    output.push(remaining as u8);
                    break;
                }
            }
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run self-test for the compression module.
pub fn self_test() {
    use crate::serial_println;
    serial_println!("[compress] Running self-test...");

    // --- All-zero page ---
    {
        let zeros = vec![0u8; 16384];
        let compressed = compress(&zeros).expect("zero page should compress");
        assert_eq!(compressed.len(), 1, "zero page should be 1 byte");
        assert_eq!(compressed[0], ZERO_PAGE_MARKER);

        let decompressed = decompress(&compressed, 16384)
            .expect("zero page decompression");
        assert_eq!(decompressed, zeros, "zero page roundtrip");

        serial_println!("[compress]   Zero page: 16384 → 1 byte: OK");
    }

    // --- Highly compressible (repeating pattern) ---
    {
        let mut data = vec![0u8; 16384];
        // Repeating 256-byte pattern.
        for (i, byte) in data.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            {
                *byte = (i & 0xFF) as u8;
            }
        }

        let compressed = compress(&data).expect("repeating pattern should compress");
        assert!(
            compressed.len() < data.len(),
            "compressed should be smaller: {} vs {}",
            compressed.len(),
            data.len()
        );

        let decompressed = decompress(&compressed, data.len())
            .expect("decompression should succeed");
        assert_eq!(decompressed, data, "roundtrip should preserve data");

        serial_println!(
            "[compress]   Repeating pattern: {} → {} bytes ({:.0}%): OK",
            data.len(),
            compressed.len(),
            (compressed.len() as f64 / data.len() as f64) * 100.0
        );
    }

    // --- Mostly-zero page with sparse data ---
    {
        let mut data = vec![0u8; 16384];
        // Sprinkle some non-zero bytes.
        for i in (0..data.len()).step_by(1024) {
            if let Some(b) = data.get_mut(i) {
                *b = 0xAB;
            }
            if let Some(b) = data.get_mut(i.saturating_add(1)) {
                *b = 0xCD;
            }
        }

        let compressed = compress(&data).expect("sparse data should compress");
        assert!(
            compressed.len() < data.len(),
            "sparse page should compress well"
        );

        let decompressed = decompress(&compressed, data.len())
            .expect("decompression should succeed");
        assert_eq!(decompressed, data, "roundtrip should preserve data");

        serial_println!(
            "[compress]   Sparse page: {} → {} bytes ({:.0}%): OK",
            data.len(),
            compressed.len(),
            (compressed.len() as f64 / data.len() as f64) * 100.0
        );
    }

    // --- Small input ---
    {
        let data = b"Hello, compressed world!";
        let compressed = compress(data);
        if let Some(ref c) = compressed {
            let decompressed = decompress(c, data.len())
                .expect("small input decompression");
            assert_eq!(&decompressed, data, "small input roundtrip");
        }
        // It's OK if small input doesn't compress (returns None).
        serial_println!("[compress]   Small input: OK");
    }

    // --- Empty input ---
    {
        let empty: &[u8] = &[];
        let compressed = compress(empty).expect("empty should work");
        assert!(compressed.is_empty());
        let decompressed = decompress(&compressed, 0).expect("empty decompression");
        assert!(decompressed.is_empty());
        serial_println!("[compress]   Empty input: OK");
    }

    serial_println!("[compress] Self-test PASSED");
}
