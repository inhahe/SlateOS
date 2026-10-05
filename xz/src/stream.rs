//! The `.xz` container: liblzma's `stream_decoder.c`, `block_decoder.c`,
//! `block_header_decoder.c`, `index_hash.c`, `stream_flags_decoder.c` and
//! `filter_flags_decoder.c`, as `xz -d` drives them (with
//! `LZMA_CONCATENATED`: several streams may follow one another, with zero
//! padding between and after them in multiples of four bytes).
//!
//! A stream is a header, blocks, an index of the blocks' sizes and a footer.
//! Everything is checked as liblzma checks it, in its order -- which decides
//! which error a damaged file is reported with, though not whether it is
//! refused: a header's CRC before its flags, every block's sizes against its
//! header and then against the index, the index's own CRC, the footer's
//! record of the index's size and of the check type.
//!
//! liblzma checks the index against the blocks with O(1) memory, by hashing
//! the blocks' sizes and the index's records and comparing hashes; with every
//! block's sizes already in hand, this compares the lists, which accepts and
//! refuses exactly the same indexes.

use alloc::vec::Vec;

use crate::check::{self, Check, CheckState};
use crate::filters::{self, Filter};
use crate::vli::{self, VLI_MAX};
use crate::{Error, Result};

/// `lzma_header_magic` and `lzma_footer_magic`.
pub(crate) const HEADER_MAGIC: [u8; 6] = [0xfd, b'7', b'z', b'X', b'Z', 0x00];
pub(crate) const FOOTER_MAGIC: [u8; 2] = [b'Y', b'Z'];
/// `LZMA_STREAM_HEADER_SIZE`
const STREAM_HEADER_SIZE: usize = 12;
/// `UNPADDED_SIZE_MIN`, `UNPADDED_SIZE_MAX`
const UNPADDED_SIZE_MIN: u64 = 5;
const UNPADDED_SIZE_MAX: u64 = VLI_MAX & !3;

/// What a successful decode found, beyond the bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    /// The streams decoded.
    pub streams: usize,
    /// The blocks decoded, across all streams.
    pub blocks: usize,
    /// Whether some block carried a check this crate cannot compute, and so
    /// was not verified (`xz -d` warns of the same).
    pub unverified: bool,
}

/// A slice of `input` from `start`, `len` long.
fn take(input: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    let end = start.checked_add(len).ok_or(Error::UnexpectedEnd)?;
    input.get(start..end).ok_or(Error::UnexpectedEnd)
}

fn read32le(b: &[u8]) -> u32 {
    let mut a = [0u8; 4];
    if let Some(src) = b.get(..4) {
        a.copy_from_slice(src);
    }
    u32::from_le_bytes(a)
}

/// `stream_flags_decode`: the check type, or [`Error::Unsupported`] for
/// flags with reserved bits set.
fn stream_flags(flags: &[u8]) -> Result<Check> {
    match flags {
        &[0x00, b] if b & 0xf0 == 0 => Check::from_id(b).ok_or(Error::Unsupported),
        _ => Err(Error::Unsupported),
    }
}

/// Decodes a whole `.xz` file onto `out`.
pub(crate) fn decode(input: &[u8], out: &mut Vec<u8>, cap: usize) -> Result<Info> {
    let mut info = Info::default();
    let mut pos = 0usize;
    let mut first = true;
    loop {
        pos = decode_stream(input, pos, out, cap, first, &mut info)?;
        first = false;

        // Stream Padding: zero bytes, a multiple of four, then the end or
        // another stream.
        let mut padding = 0usize;
        loop {
            match input.get(pos) {
                None => {
                    return if padding.is_multiple_of(4) {
                        Ok(info)
                    } else {
                        Err(Error::InvalidPadding)
                    };
                }
                Some(0) => {
                    pos = pos.wrapping_add(1);
                    padding = padding.wrapping_add(1);
                }
                Some(_) => break,
            }
        }
        if !padding.is_multiple_of(4) {
            return Err(Error::InvalidPadding);
        }
    }
}

/// Decodes the stream at `input[start]`, returning the position after its
/// footer.
fn decode_stream(
    input: &[u8],
    start: usize,
    out: &mut Vec<u8>,
    cap: usize,
    first: bool,
    info: &mut Info,
) -> Result<usize> {
    // Stream Header. A later stream with the wrong magic is corrupt data,
    // not a different format (`first_stream`).
    let header = take(input, start, STREAM_HEADER_SIZE)?;
    if header.get(..6) != Some(&HEADER_MAGIC[..]) {
        return Err(if first {
            Error::NotXz
        } else {
            Error::InvalidData
        });
    }
    let flags = header.get(6..8).unwrap_or_default();
    if check::crc32(flags, 0) != read32le(header.get(8..).unwrap_or_default()) {
        return Err(Error::HeaderCrcMismatch);
    }
    let check = stream_flags(flags)?;
    // The header was taken, so these twelve bytes exist.
    let mut pos = start.wrapping_add(STREAM_HEADER_SIZE);

    // Blocks, until the Index Indicator.
    let mut blocks: Vec<(u64, u64)> = Vec::new();
    loop {
        let &b = input.get(pos).ok_or(Error::UnexpectedEnd)?;
        if b == 0x00 {
            break;
        }
        let (next, unpadded, uncompressed) = decode_block(input, pos, check, out, cap, info)?;
        blocks.push((unpadded, uncompressed));
        info.blocks = info.blocks.saturating_add(1);
        pos = next;
    }

    // Index.
    let index_start = pos;
    pos = decode_index(input, pos, &blocks)?;
    let index_size = pos.wrapping_sub(index_start);

    // Stream Footer: the magic first, then the CRC, then the flags.
    let footer = take(input, pos, STREAM_HEADER_SIZE)?;
    if footer.get(10..) != Some(&FOOTER_MAGIC[..]) {
        return Err(Error::InvalidData);
    }
    if check::crc32(footer.get(4..10).unwrap_or_default(), 0)
        != read32le(footer.get(..4).unwrap_or_default())
    {
        return Err(Error::HeaderCrcMismatch);
    }
    let footer_check = stream_flags(footer.get(8..10).unwrap_or_default())?;
    let stored = u64::from(read32le(footer.get(4..8).unwrap_or_default()));
    let backward_size = stored.wrapping_add(1).wrapping_mul(4);
    if backward_size != index_size as u64 {
        return Err(Error::IndexMismatch);
    }
    // `lzma_stream_flags_compare`
    if footer_check != check {
        return Err(Error::IndexMismatch);
    }
    info.streams = info.streams.saturating_add(1);
    Ok(pos.wrapping_add(STREAM_HEADER_SIZE))
}

/// Decodes the block whose header begins at `input[start]` onto `out`;
/// returns the position after it, its Unpadded Size and its Uncompressed
/// Size.
#[allow(clippy::too_many_lines)]
fn decode_block(
    input: &[u8],
    start: usize,
    check: Check,
    out: &mut Vec<u8>,
    cap: usize,
    info: &mut Info,
) -> Result<(usize, u64, u64)> {
    // `lzma_block_header_size_decode`
    let first = input.get(start).copied().ok_or(Error::UnexpectedEnd)?;
    // 8 to 1024 bytes.
    let header_size = usize::from(first).wrapping_add(1).wrapping_mul(4);
    let header = take(input, start, header_size)?;
    let (body, crc) = header.split_at(header_size.wrapping_sub(4));
    if check::crc32(body, 0) != read32le(crc) {
        return Err(Error::HeaderCrcMismatch);
    }
    let flags = body.get(1).copied().unwrap_or(0);
    if flags & 0x3c != 0 {
        return Err(Error::Unsupported);
    }

    // Inside the header, running out of bytes is a corrupt header
    // (liblzma's single-call `lzma_vli_decode` says `LZMA_DATA_ERROR`).
    let in_header = |r: Result<u64>| r.map_err(|_| Error::InvalidData);
    let mut hpos = 2usize;
    let check_size = check.size() as u64;
    let mut compressed_size = None;
    if flags & 0x40 != 0 {
        let size = in_header(vli::decode(body, &mut hpos))?;
        // `lzma_block_unpadded_size(block) == 0`: a zero size, or one whose
        // Unpadded Size would not fit.
        let unpadded = size
            .checked_add((header_size as u64).wrapping_add(check_size))
            .filter(|&u| size != 0 && u <= UNPADDED_SIZE_MAX && size <= VLI_MAX);
        if unpadded.is_none() {
            return Err(Error::InvalidData);
        }
        compressed_size = Some(size);
    }
    let uncompressed_size = if flags & 0x80 != 0 {
        Some(in_header(vli::decode(body, &mut hpos))?)
    } else {
        None
    };

    let filter_count = usize::from(flags & 3) + 1;
    let mut chain = Vec::with_capacity(filter_count);
    for _ in 0..filter_count {
        let id = in_header(vli::decode(body, &mut hpos))?;
        if id >= filters::ID_RESERVED_START {
            return Err(Error::InvalidData);
        }
        let props_size = in_header(vli::decode(body, &mut hpos))?;
        let left = body.len().saturating_sub(hpos) as u64;
        if left < props_size {
            return Err(Error::InvalidData);
        }
        // At most `left` bytes on: inside the header.
        let props_end = hpos.wrapping_add(props_size as usize);
        let props = body.get(hpos..props_end).unwrap_or_default();
        let filter = Filter::decode(id, props);
        hpos = props_end;
        chain.push(filter?);
    }
    // Header Padding: zeros to the CRC.
    if body.get(hpos..).unwrap_or_default().iter().any(|&b| b != 0) {
        return Err(Error::Unsupported);
    }
    filters::validate_chain(&chain)?;
    let Some(&Filter::Lzma2 { dict_size }) = chain.last() else {
        return Err(Error::Unsupported);
    };

    // Compressed Data. A declared Uncompressed Size bounds the output: going
    // past it is corrupt data, not the caller's cap.
    let data_start = start.wrapping_add(header_size);
    let out_start = out.len();
    let block_cap = match uncompressed_size {
        Some(u) => {
            let bound = out_start.saturating_add(usize::try_from(u).unwrap_or(usize::MAX));
            bound.min(cap)
        }
        None => cap,
    };
    let data_end = match crate::lzma2::decode(input, data_start, out, dict_size, block_cap) {
        Ok(end) => end,
        Err(Error::OutputTooLarge) if block_cap < cap => return Err(Error::InvalidData),
        // liblzma's block decoder checks the declared Compressed Size after
        // every call, so input that ran out past it is corrupt, not cut short.
        Err(Error::UnexpectedEnd)
            if compressed_size
                .is_some_and(|c| input.len().saturating_sub(data_start) as u64 > c) =>
        {
            return Err(Error::InvalidData);
        }
        Err(e) => return Err(e),
    };
    let compressed = data_end.wrapping_sub(data_start) as u64;
    let produced = out.len().wrapping_sub(out_start) as u64;
    if compressed_size.is_some_and(|c| c != compressed)
        || uncompressed_size.is_some_and(|u| u != produced)
    {
        return Err(Error::InvalidData);
    }
    // liblzma's `compressed_limit` when the header gives no size.
    if compressed
        > (VLI_MAX & !3)
            .saturating_sub(header_size as u64)
            .saturating_sub(check_size)
    {
        return Err(Error::InvalidData);
    }

    let block_out = out.get_mut(out_start..).unwrap_or_default();
    filters::undo(&chain, block_out);

    // Block Padding: zeros to a multiple of four.
    let mut pos = data_end;
    let mut size = compressed;
    while !size.is_multiple_of(4) {
        let &b = input.get(pos).ok_or(Error::UnexpectedEnd)?;
        pos = pos.wrapping_add(1);
        size = size.wrapping_add(1);
        if b != 0 {
            return Err(Error::InvalidPadding);
        }
    }

    // Check.
    let stored = take(input, pos, check.size())?;
    pos = pos.wrapping_add(check.size());
    let mut state = CheckState::new(check);
    state.update(out.get(out_start..).unwrap_or_default());
    match state.finish() {
        Some(computed) => {
            if computed != stored {
                return Err(Error::CheckMismatch);
            }
        }
        None => {
            if check != Check::NONE {
                info.unverified = true;
            }
        }
    }

    let unpadded = (header_size as u64)
        .saturating_add(compressed)
        .saturating_add(check_size);
    Ok((pos, unpadded, produced))
}

/// Checks the Index at `input[start]` (its indicator already seen) against
/// the blocks decoded; returns the position after it.
fn decode_index(input: &[u8], start: usize, blocks: &[(u64, u64)]) -> Result<usize> {
    let mut pos = start.wrapping_add(1);
    let count = vli::decode(input, &mut pos)?;
    if count != blocks.len() as u64 {
        return Err(Error::IndexMismatch);
    }
    for &(block_unpadded, block_uncompressed) in blocks {
        let unpadded = vli::decode(input, &mut pos)?;
        let uncompressed = vli::decode(input, &mut pos)?;
        if !(UNPADDED_SIZE_MIN..=UNPADDED_SIZE_MAX).contains(&unpadded) {
            return Err(Error::IndexMismatch);
        }
        if unpadded != block_unpadded || uncompressed != block_uncompressed {
            return Err(Error::IndexMismatch);
        }
    }
    // Index Padding: zeros until the index, with its CRC, is a multiple of
    // four bytes long.
    while !pos.wrapping_sub(start).is_multiple_of(4) {
        let &b = input.get(pos).ok_or(Error::UnexpectedEnd)?;
        pos = pos.wrapping_add(1);
        if b != 0 {
            return Err(Error::InvalidPadding);
        }
    }
    let crc = check::crc32(input.get(start..pos).unwrap_or_default(), 0);
    let stored = take(input, pos, 4)?;
    if read32le(stored) != crc {
        return Err(Error::IndexMismatch);
    }
    Ok(pos.wrapping_add(4))
}
