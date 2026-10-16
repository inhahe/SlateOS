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
//! - **Incompressible page**: if the compressed form would not be smaller
//!   than the original, or would not fit the caller's buffer, the answer is
//!   `None`, and the caller decides: the compressed swap tier refuses the
//!   page, a disk area stores it raw.
//! - **Allocation**: [`compress_into`] and [`decompress_into`] work in the
//!   caller's buffers and allocate nothing: swap compresses under its lock,
//!   during reclaim, when an allocation may fail.

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

/// The scratch [`compress_into`] finds matches with: one entry per hash
/// bucket, overwritten by every call.
pub const HASH_TABLE_ENTRIES: usize = HASH_TABLE_SIZE;

/// Compress `input` into `output`, finding matches with `table` (at least
/// [`HASH_TABLE_ENTRIES`] entries, overwritten), and allocate nothing.
///
/// Answers the compressed length, or `None` when it would not fit in
/// `output` or would not be smaller than `input` -- so the size of `output`
/// is the most a caller is willing to keep, and a page that does not shrink
/// that far is refused while it is being compressed rather than after. An
/// all-zero input is the one byte [`ZERO_PAGE_MARKER`]; an empty one is
/// empty.
///
/// For a caller that must not allocate: the compressed swap tier compresses
/// under its lock, during reclaim, when an allocation may fail.
pub fn compress_into(input: &[u8], output: &mut [u8], table: &mut [u32]) -> Option<usize> {
    if input.is_empty() {
        return Some(0);
    }
    if is_all_zero(input) {
        *output.first_mut()? = ZERO_PAGE_MARKER;
        return Some(1);
    }
    let table = table.get_mut(..HASH_TABLE_SIZE)?;
    table.fill(0);

    let input_len = input.len();
    let mut out = Out {
        buf: output,
        len: 0,
    };
    let mut ip = 0usize; // Input position (current scan point).
    let mut anchor = 0usize; // Start of unmatched literal run.

    // We need at least MIN_MATCH bytes to encode a match at the end.
    let input_limit = input_len.saturating_sub(5);
    let match_limit = input_len.saturating_sub(MIN_MATCH);

    while ip < input_limit {
        // Hash the 4 bytes at the current position.
        let h = hash4(input, ip);
        let candidate = table.get(h).copied().unwrap_or(0) as usize;

        // Update the hash table with the current position.
        if let Some(slot) = table.get_mut(h) {
            #[allow(clippy::cast_possible_truncation)]
            {
                *slot = ip as u32;
            }
        }

        // Check if the candidate matches.
        let offset = ip.wrapping_sub(candidate);
        if offset == 0 || offset > 0xFFFF || candidate >= ip || !matches_at(input, candidate, ip) {
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
        write_sequence(
            &mut out,
            input,
            anchor,
            literal_len,
            offset,
            match_len,
            false,
        )?;

        // Advance past the match.
        ip = ip.saturating_add(match_len);
        anchor = ip;
    }

    // Write the final literal run (no match).
    if anchor < input_len {
        let lit_len = input_len.saturating_sub(anchor);
        write_sequence(&mut out, input, anchor, lit_len, 0, 0, true)?;
    }

    // Only answer if it is actually smaller.
    let max_size = input_len.saturating_mul(MAX_OUTPUT_FACTOR);
    if out.len >= max_size {
        return None;
    }
    Some(out.len)
}

/// Decompress data produced by [`compress_into`] into `output`, whose length
/// is the size the data decompresses to. Allocates nothing.
///
/// # Errors
///
/// [`CompressError`] if the data is malformed, or does not decompress to
/// exactly `output.len()` bytes.
pub fn decompress_into(input: &[u8], output: &mut [u8]) -> Result<(), CompressError> {
    let expected = output.len();
    if input.is_empty() {
        return if expected == 0 {
            Ok(())
        } else {
            Err(CompressError::SizeMismatch { expected, got: 0 })
        };
    }

    // Check for all-zero page marker.
    if input.len() == 1 && input.first().copied() == Some(ZERO_PAGE_MARKER) {
        output.fill(0);
        return Ok(());
    }

    let mut pos = 0usize; // Bytes of `output` written.
    let mut ip = 0usize; // Position in input.
    let overflow = |got: usize| CompressError::SizeMismatch { expected, got };

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
        let literal_end = ip.saturating_add(literal_len);
        let literals = input.get(ip..literal_end).ok_or(CompressError::Truncated)?;
        let out_end = pos.saturating_add(literal_len);
        output
            .get_mut(pos..out_end)
            .ok_or_else(|| overflow(out_end))?
            .copy_from_slice(literals);
        pos = out_end;
        ip = literal_end;

        // If we've reached the end of input, this was the last sequence
        // (no match component).
        if ip >= input.len() {
            break;
        }

        // --- Match ---
        // Read 2-byte little-endian offset.
        let lo = *input.get(ip).ok_or(CompressError::Truncated)? as u16;
        let hi = *input
            .get(ip.saturating_add(1))
            .ok_or(CompressError::Truncated)? as u16;
        ip = ip.saturating_add(2);
        let offset = usize::from((hi << 8) | lo);

        if offset == 0 || offset > pos {
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

        // Copy byte by byte from `offset` back, which handles a match that
        // overlaps what it is writing.
        let out_end = pos.saturating_add(match_len);
        if out_end > expected {
            return Err(overflow(out_end));
        }
        while pos < out_end {
            let b = output
                .get(pos.wrapping_sub(offset))
                .copied()
                .ok_or(CompressError::InvalidOffset)?;
            *output.get_mut(pos).ok_or_else(|| overflow(out_end))? = b;
            pos = pos.saturating_add(1);
        }
    }

    if pos != expected {
        return Err(CompressError::SizeMismatch { expected, got: pos });
    }
    Ok(())
}

/// Whether `compressed`, from [`compress_into`], is an all-zero page's
/// encoding: what a caller may store as "zeros" without keeping any bytes.
#[must_use]
pub fn is_zero_encoding(compressed: &[u8]) -> bool {
    compressed == [ZERO_PAGE_MARKER]
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

/// A slice being filled from the front: what [`compress_into`] writes its
/// output through, so that running out of room is an answer, not a panic.
struct Out<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Out<'_> {
    /// Append `b`; `None` if the slice is full.
    fn push(&mut self, b: u8) -> Option<()> {
        *self.buf.get_mut(self.len)? = b;
        self.len = self.len.saturating_add(1);
        Some(())
    }

    /// Append `bytes`; `None` if they do not fit.
    fn extend(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.len.checked_add(bytes.len())?;
        self.buf.get_mut(self.len..end)?.copy_from_slice(bytes);
        self.len = end;
        Some(())
    }
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

/// Write a literal+match sequence to the output.
///
/// `None` if it does not fit, or the literal range lies outside `input`.
fn write_sequence(
    out: &mut Out<'_>,
    input: &[u8],
    literal_start: usize,
    literal_len: usize,
    offset: usize,
    match_len: usize,
    is_last: bool,
) -> Option<()> {
    // Token byte.
    let lit_field = if literal_len >= 15 {
        15u8
    } else {
        literal_len as u8
    };
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
    out.push(token)?;

    // Extra literal length bytes.
    if literal_len >= 15 {
        write_extra_len(out, literal_len.saturating_sub(15))?;
    }

    // Literal bytes.
    out.extend(input.get(literal_start..literal_start.saturating_add(literal_len))?)?;

    // Match (offset + optional extra length) — only if not last sequence.
    if !is_last {
        // 2-byte LE offset.
        #[allow(clippy::cast_possible_truncation)]
        {
            out.push((offset & 0xFF) as u8)?;
            out.push(((offset >> 8) & 0xFF) as u8)?;
        }

        // Extra match length bytes.
        let adj_match = match_len.saturating_sub(MIN_MATCH);
        if adj_match >= 15 {
            write_extra_len(out, adj_match.saturating_sub(15))?;
        }
    }

    Some(())
}

/// A length's continuation bytes: 255 for each whole 255, then the rest.
fn write_extra_len(out: &mut Out<'_>, mut remaining: usize) -> Option<()> {
    while remaining >= 255 {
        out.push(255)?;
        remaining = remaining.saturating_sub(255);
    }
    #[allow(clippy::cast_possible_truncation)]
    out.push(remaining as u8)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run self-test for the compression module.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    /// `input` compressed into a buffer its own size, as the swap path does.
    fn compress(input: &[u8]) -> Option<Vec<u8>> {
        let mut output = vec![0u8; input.len()];
        let mut table = vec![0u32; HASH_TABLE_ENTRIES];
        let len = compress_into(input, &mut output, &mut table)?;
        output.truncate(len);
        Some(output)
    }

    /// `input` decompressed to `output_len` bytes.
    fn decompress(input: &[u8], output_len: usize) -> Result<Vec<u8>, CompressError> {
        let mut output = vec![0u8; output_len];
        decompress_into(input, &mut output)?;
        Ok(output)
    }

    serial_println!("[compress] Running self-test...");

    // --- All-zero page ---
    {
        let zeros = vec![0u8; 16384];
        let compressed = compress(&zeros).expect("zero page should compress");
        assert_eq!(compressed.len(), 1, "zero page should be 1 byte");
        assert_eq!(compressed[0], ZERO_PAGE_MARKER);

        let decompressed = decompress(&compressed, 16384).expect("zero page decompression");
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

        let decompressed =
            decompress(&compressed, data.len()).expect("decompression should succeed");
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

        let decompressed =
            decompress(&compressed, data.len()).expect("decompression should succeed");
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
            let decompressed = decompress(c, data.len()).expect("small input decompression");
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

    // --- Into a caller's buffers: the output's size is a limit, kept ---
    {
        let mut data = vec![0u8; 16384];
        for (i, byte) in data.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            {
                *byte = (i & 0xFF) as u8;
            }
        }
        let mut table = vec![0u32; HASH_TABLE_ENTRIES];
        let mut out = vec![0u8; 8192];
        let n = compress_into(&data, &mut out, &mut table).expect("fits in 8 KiB");
        let mut back = vec![0u8; 16384];
        decompress_into(out.get(..n).unwrap_or(&[]), &mut back).expect("roundtrip");
        assert_eq!(back, data, "into-buffer roundtrip");
        // One byte less room than it needs: refused, nothing written past it.
        let mut tight = vec![0u8; n.saturating_add(1)];
        let short = n.saturating_sub(1);
        assert_eq!(
            compress_into(&data, tight.get_mut(..short).unwrap_or(&mut []), &mut table),
            None,
            "an output too small is a refusal"
        );
        assert_eq!(
            tight.get(short).copied(),
            Some(0),
            "nothing written past the limit"
        );
        // A table shorter than HASH_TABLE_ENTRIES: refused, not misused.
        assert_eq!(compress_into(&data, &mut out, &mut [0u32; 16]), None);
        // The wrong size to decompress into: an error, not a short page.
        let mut small = vec![0u8; 100];
        assert!(decompress_into(out.get(..n).unwrap_or(&[]), &mut small).is_err());
        serial_println!("[compress]   Into a caller's buffers, its size a limit: OK");
    }

    // --- Incompressible: refused, whatever room there is ---
    {
        let mut data = vec![0u8; 16384];
        let mut x: u32 = 0x1234_5678;
        for byte in &mut data {
            // xorshift32: no 4-byte sequence repeats within a page.
            x ^= x.wrapping_shl(13);
            x ^= x.wrapping_shr(17);
            x ^= x.wrapping_shl(5);
            *byte = x.to_be_bytes()[0];
        }
        assert!(compress(&data).is_none(), "random data does not shrink");
        serial_println!("[compress]   Incompressible page refused: OK");
    }

    serial_println!("[compress] Self-test PASSED");
    Ok(())
}
