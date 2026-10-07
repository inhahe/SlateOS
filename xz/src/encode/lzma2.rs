//! The LZMA2 encoder: liblzma's `lzma2_encoder.c`.
//!
//! LZMA2 cuts the LZMA stream into chunks of at most 2 MiB of input and
//! 64 KiB of output, each with a header saying its sizes and what it resets.
//! A chunk that did not compress is stored instead, and the LZMA state is
//! reset after it -- the encoder has to back out of the bits it spent.

// Chunk sizes are at most 2 MiB and 64 KiB.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec::Vec;

use super::LzmaOptions;
use super::lzma::{Encoder, LZMA2_CHUNK_MAX};
use super::mf::{MATCH_LEN_MAX, Mf};
use crate::lzma::Props;

/// `LZMA2_UNCOMPRESSED_MAX`: an LZMA chunk's input, at most.
const UNCOMPRESSED_MAX: usize = 1 << 21;

/// Encodes `data` as a raw LZMA2 stream, end marker included, into `out`.
/// The options are valid.
pub(crate) fn encode(data: &[u8], opt: &LzmaOptions, out: &mut Vec<u8>) {
    let mut mf = Mf::new(
        data,
        opt.dict_size,
        opt.match_finder,
        opt.nice_len,
        opt.depth,
    );
    let mut lzma = Encoder::new(opt);
    let props = Props {
        lc: opt.lc,
        lp: opt.lp,
        pb: opt.pb,
    }
    .to_byte();

    let mut need_properties = true;
    let mut need_state_reset = false;
    let mut need_dictionary_reset = true;
    let mut chunk = Vec::with_capacity(LZMA2_CHUNK_MAX as usize);

    loop {
        // SEQ_INIT
        if mf.unencoded() == 0 {
            out.push(0x00);
            return;
        }
        if need_state_reset {
            lzma.reset(opt);
        }

        // SEQ_LZMA_ENCODE: as much as the chunk takes. An LZMA symbol may be
        // as long as MATCH_LEN_MAX, so the input stops that far short of
        // the chunk's limit.
        let read_start = mf.position();
        let limit = read_start.saturating_add(UNCOMPRESSED_MAX - MATCH_LEN_MAX as usize);
        chunk.clear();
        lzma.encode(&mut mf, &mut chunk, Some(limit));
        let mut uncompressed = mf.position().saturating_sub(read_start);

        if chunk.len() >= uncompressed {
            // Stored: the bytes the match finder ran ahead over go too, and
            // the LZMA state starts again after them.
            uncompressed = uncompressed.saturating_add(mf.read_ahead as usize);
            mf.read_ahead = 0;
            out.push(if need_dictionary_reset { 0x01 } else { 0x02 });
            need_dictionary_reset = false;
            let size = uncompressed.saturating_sub(1);
            out.push((size >> 8) as u8);
            out.push(size as u8);
            need_state_reset = true;
            let end = mf.read_pos;
            if let Some(bytes) = data.get(end.saturating_sub(uncompressed)..end) {
                out.extend_from_slice(bytes);
            }
            continue;
        }

        // An LZMA chunk: `lzma2_header_lzma`.
        let control = if need_properties {
            if need_dictionary_reset { 0xe0 } else { 0xc0 }
        } else if need_state_reset {
            0xa0
        } else {
            0x80
        };
        let size = uncompressed.saturating_sub(1);
        out.push(control | ((size >> 16) as u8 & 0x1f));
        out.push((size >> 8) as u8);
        out.push(size as u8);
        let size = chunk.len().saturating_sub(1);
        out.push((size >> 8) as u8);
        out.push(size as u8);
        if need_properties {
            out.push(props);
        }
        need_properties = false;
        need_state_reset = false;
        need_dictionary_reset = false;
        out.extend_from_slice(&chunk);
    }
}

/// `lzma_lzma2_props_encode`: the dictionary-size byte, the smallest size
/// of the form 2^n or 2^n + 2^(n-1) at least `dict_size`.
pub(crate) const fn dict_size_prop(dict_size: u32) -> u8 {
    let mut d = if dict_size < 4096 { 4096 } else { dict_size };
    d -= 1;
    d |= d >> 2;
    d |= d >> 3;
    d |= d >> 4;
    d |= d >> 8;
    d |= d >> 16;
    if d == u32::MAX {
        40
    } else {
        (super::price::dist_slot(d + 1) - 24) as u8
    }
}
