//! The containers: an `.xz` stream as `xz` writes it single-threaded
//! (liblzma's `stream_encoder.c`, `block_encoder.c`,
//! `block_header_encoder.c`, `filter_flags_encoder.c`, `index_encoder.c`,
//! `stream_flags_encoder.c`), and an `.lzma` file (`alone_encoder.c`).
//!
//! `xz` writes a block header without sizes -- it does not know them when
//! the header goes out -- so only the index records them. With
//! `--block-size`, each block is compressed alone: a new encoder, a new
//! dictionary, the filters restarted.

// Header and index sizes are a few hundred bytes.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec::Vec;

use super::lzma::Encoder;
use super::mf::Mf;
use super::{LzmaOptions, PreFilter, XzOptions, lzma2};
use crate::check::{CheckState, crc32};
use crate::filters::{
    ID_ARM, ID_ARMTHUMB, ID_DELTA, ID_IA64, ID_LZMA2, ID_POWERPC, ID_SPARC, ID_X86, bcj,
    delta_encode,
};
use crate::lzma::Props;
use crate::{Bcj, vli};

/// The stream header's magic bytes, and the footer's.
const HEADER_MAGIC: [u8; 6] = [0xfd, b'7', b'z', b'X', b'Z', 0x00];
const FOOTER_MAGIC: [u8; 2] = [b'Y', b'Z'];

/// Writes `data` as one `.xz` stream. The options are valid.
pub(crate) fn xz(data: &[u8], opt: &XzOptions) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 64);
    let flags = [0x00, opt.check.id()];

    // Stream header.
    out.extend_from_slice(&HEADER_MAGIC);
    out.extend_from_slice(&flags);
    out.extend_from_slice(&crc32(&flags, 0).to_le_bytes());

    // Blocks: none for empty input, as `xz` writes none.
    let block = match opt.block_size {
        Some(size) => usize::try_from(size).unwrap_or(usize::MAX).max(1),
        None => data.len().max(1),
    };
    let header = block_header(opt);
    let mut records: Vec<(u64, u64)> = Vec::new();
    for piece in data.chunks(block) {
        out.extend_from_slice(&header);
        let start = out.len();
        let mut filtered = piece.to_vec();
        for filter in &opt.filters {
            match *filter {
                PreFilter::Delta { distance } => delta_encode(&mut filtered, distance),
                PreFilter::Bcj { arch, start } => {
                    bcj(arch, start, true, &mut filtered);
                }
            }
        }
        lzma2::encode(&filtered, &opt.lzma2, &mut out);
        let compressed = out.len().saturating_sub(start);
        // Block padding, then the check of the block's original bytes.
        while out.len().saturating_sub(start) % 4 != 0 {
            out.push(0);
        }
        let mut check = CheckState::new(opt.check);
        check.update(piece);
        let check = check.finish().unwrap_or_default();
        out.extend_from_slice(&check);
        let unpadded = header
            .len()
            .saturating_add(compressed)
            .saturating_add(check.len());
        records.push((unpadded as u64, piece.len() as u64));
    }

    // Index: indicator, count, records, padding, CRC-32.
    let index_start = out.len();
    out.push(0x00);
    vli::encode(records.len() as u64, &mut out);
    for &(unpadded, uncompressed) in &records {
        vli::encode(unpadded, &mut out);
        vli::encode(uncompressed, &mut out);
    }
    while out.len().saturating_sub(index_start) % 4 != 0 {
        out.push(0x00);
    }
    let index_crc = crc32(out.get(index_start..).unwrap_or(&[]), 0);
    out.extend_from_slice(&index_crc.to_le_bytes());
    let index_size = out.len().saturating_sub(index_start);

    // Stream footer: CRC-32, backward size, flags, magic.
    let backward = ((index_size / 4).saturating_sub(1) as u32).to_le_bytes();
    let mut fields = [0u8; 6];
    fields[..4].copy_from_slice(&backward);
    fields[4..].copy_from_slice(&flags);
    out.extend_from_slice(&crc32(&fields, 0).to_le_bytes());
    out.extend_from_slice(&fields);
    out.extend_from_slice(&FOOTER_MAGIC);
    out
}

/// `lzma_block_header_encode` with no sizes: the header size byte, the
/// flags (the filter count less one), the filter flags, padding to a
/// multiple of four, and a CRC-32.
fn block_header(opt: &XzOptions) -> Vec<u8> {
    let mut body = Vec::new();
    for filter in &opt.filters {
        match *filter {
            PreFilter::Delta { distance } => {
                vli::encode(ID_DELTA, &mut body);
                body.push(1);
                body.push(distance.saturating_sub(1) as u8);
            }
            PreFilter::Bcj { arch, start } => {
                let id = match arch {
                    Bcj::X86 => ID_X86,
                    Bcj::PowerPc => ID_POWERPC,
                    Bcj::Ia64 => ID_IA64,
                    Bcj::Arm => ID_ARM,
                    Bcj::ArmThumb => ID_ARMTHUMB,
                    Bcj::Sparc => ID_SPARC,
                };
                vli::encode(id, &mut body);
                // `lzma_simple_props_size`: no properties for offset 0.
                if start == 0 {
                    body.push(0);
                } else {
                    body.push(4);
                    body.extend_from_slice(&start.to_le_bytes());
                }
            }
        }
    }
    vli::encode(ID_LZMA2, &mut body);
    body.push(1);
    body.push(lzma2::dict_size_prop(opt.lzma2.dict_size));

    // Size byte + flags + body + CRC-32, rounded up to four.
    let size = (2 + body.len() + 4 + 3) & !3;
    let mut header = Vec::with_capacity(size);
    header.push(((size - 4) / 4) as u8);
    header.push(opt.filters.len() as u8);
    header.extend_from_slice(&body);
    header.resize(size - 4, 0);
    let crc = crc32(&header, 0);
    header.extend_from_slice(&crc.to_le_bytes());
    header
}

/// Writes `data` as an `.lzma` file (`lzma_alone_encoder`): the properties,
/// the dictionary size rounded up to 2^n or 2^n + 2^(n-1), an unknown size,
/// and LZMA ending in the end-of-payload marker. The options are valid.
pub(crate) fn lzma(data: &[u8], opt: &LzmaOptions) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 32);
    out.push(
        Props {
            lc: opt.lc,
            lp: opt.lp,
            pb: opt.pb,
        }
        .to_byte(),
    );
    let mut d = opt.dict_size.saturating_sub(1);
    d |= d >> 2;
    d |= d >> 3;
    d |= d >> 4;
    d |= d >> 8;
    d |= d >> 16;
    if d != u32::MAX {
        d = d.wrapping_add(1);
    }
    out.extend_from_slice(&d.to_le_bytes());
    out.extend_from_slice(&[0xff; 8]);
    raw_lzma1(data, opt, &mut out);
    out
}

/// A raw LZMA stream with the end-of-payload marker, as liblzma's LZMA1
/// encoder always writes it.
pub(crate) fn raw_lzma1(data: &[u8], opt: &LzmaOptions, out: &mut Vec<u8>) {
    let mut mf = Mf::new(
        data,
        opt.dict_size,
        opt.match_finder,
        opt.nice_len,
        opt.depth,
    );
    let mut enc = Encoder::new(opt);
    enc.encode(&mut mf, out, None);
}
