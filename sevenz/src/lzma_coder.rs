//! 7-Zip's LZMA and LZMA2 coders as a 7z folder runs them:
//! `CPP/7zip/Compress/LzmaDecoder.cpp`, and `Lzma2Decoder.cpp` with
//! `C/Lzma2DecMt.c` and `C/MtDec.c`, from the LZMA SDK 26.00 (Igor Pavlov,
//! public domain).
//!
//! The decoders are `lzma_dec.rs` and `lzma2_dec.rs`; this is what drives
//! them and what decides that a stream which decoded is damaged all the
//! same. A 7z folder is extracted whole, so each coder runs in "finish
//! mode": its output must be exactly the size the header gives, and its
//! packed input used to the last byte.
//!
//! LZMA2 is the involved one. Given more than one thread, 7-Zip first walks
//! the chunk headers (`Lzma2Dec_Parse`) to cut the stream into blocks at
//! dictionary resets, then decodes each block on its own up to the size the
//! walk found, and falls back to decoding in sequence where the walk cannot
//! settle a block (`MTDEC_PARSE_OVERFLOW`) or settles one with no output (an
//! allocation of zero bytes fails). On a sound stream the two ways give the
//! same bytes; on a damaged one they can part: a chunk whose declared packed
//! size runs past the end of the stream ends the walked block at the chunk
//! before it, where decoding in sequence decodes the chunk and only then
//! finds the stream short. [`Threads`] says which 7-Zip to be.
//!
//! The thread pool is simulated, not used: blocks are decoded one after
//! another, which is the order 7-Zip writes them in, and the order its
//! "an earlier block failed" interrupts follow -- so the output, and the
//! verdict, are the ones 7-Zip's threads arrive at.

// Positions and sizes are within the input and the output, which index them
// checked; the block arithmetic is `Lzma2DecMt.c`'s, on sizes bounded by
// the output's.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec::Vec;

use crate::lzma_dec::{Finish, LzmaDec, Props, Status};
use crate::lzma2_dec::{Lzma2Dec, ParseStatus, dic_size_from_prop};

/// Which 7-Zip's LZMA2 decoding to reproduce.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Threads {
    /// 7-Zip with one thread (`7z -mmt=off`, or a machine with one
    /// hardware thread): the stream decoded in sequence.
    One,
    /// 7-Zip by default on a machine with more than one hardware thread
    /// and memory for two blocks: the stream walked, cut into blocks, and
    /// each decoded on its own.
    #[default]
    Many,
}

/// A coder's output, and whether 7-Zip would call it sound (`S_OK`) rather
/// than damaged (`S_FALSE`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Coded {
    pub(crate) out: Vec<u8>,
    pub(crate) ok: bool,
}

/// `NCompress::NLzma::CDecoder::Code` in finish mode: `input` is the
/// packed stream, `out_size` the coder's unpacked size. `None` is
/// `E_NOTIMPL`: properties `LzmaDec` cannot use.
pub(crate) fn lzma(props: &[u8], input: &[u8], out_size: usize) -> Option<Coded> {
    let props = Props::decode(props)?;
    let mut dec = LzmaDec::new(props);
    let mut out = Vec::new();
    // `CodeSpec` reads and writes in steps of a megabyte, and `LzmaDec` is
    // built to decide the same whatever the steps; so one call over the
    // whole stream, with FINISH_END at the size.
    let step = dec.decode_to_dic(&mut out, out_size, input, Finish::End);
    let stopped_well = step.fault.is_none()
        && match step.status {
            Status::FinishedWithMark => out.len() == out_size,
            // `outFinished`, with no end marker after it.
            Status::MaybeFinishedWithoutMark => true,
            // Wanting input there is none of: the next read finds nothing,
            // and a call that makes no progress is `S_FALSE`.
            _ => false,
        };
    // `Code`: in finish mode the packed stream must be used up.
    let ok = stopped_well && step.consumed == input.len();
    Some(Coded { out, ok })
}

/// `NCompress::NLzma2::CDecoder::Code` in finish mode, as 7-Zip with
/// `threads` runs it. `None` is `E_NOTIMPL`: a property that is not one
/// byte of at most 40.
pub(crate) fn lzma2(
    props: &[u8],
    input: &[u8],
    out_size: usize,
    threads: Threads,
) -> Option<Coded> {
    let &[prop] = props else {
        return None;
    };
    dic_size_from_prop(prop)?;
    let mut out = Vec::new();
    let (in_processed, ok) = match threads {
        Threads::One => decode_st(prop, input, &mut out, out_size),
        Threads::Many => decode_mt(prop, input, &mut out, out_size),
    };
    // In finish mode, the packed stream must be used up and the output
    // reach its size.
    let ok = ok && in_processed == input.len() && out.len() == out_size;
    Some(Coded { out, ok })
}

/// `Lzma2Dec_Decode_ST`: the stream decoded in sequence onto `out`, to
/// `out_size` bytes in all. Returns the input taken and whether it ended
/// without an error (`SZ_OK`).
fn decode_st(prop: u8, input: &[u8], out: &mut Vec<u8>, out_size: usize) -> (usize, bool) {
    let Some(mut dec) = Lzma2Dec::new(prop) else {
        return (0, false);
    };
    // The loop's steps change nothing `Lzma2Dec` decides (`lzma_coder`'s
    // module docs), so one call to the end, with FINISH_END.
    let step = dec.decode_to_dic(out, out_size, input, Finish::End);
    let ok = step.fault.is_none()
        && match step.status {
            Status::FinishedWithMark => out.len() == out_size,
            // `SZ_ERROR_INPUT_EOF`, or a stop with nothing decided.
            _ => false,
        };
    (step.consumed, ok)
}

/// `MtDec`'s input buffer: the stream is read, and walked, in pieces of
/// this size.
const IN_BUF_SIZE_MT: usize = 1 << 18;
/// Blocks are not cut at a dictionary reset before this much output: "we
/// decode small blocks in one thread".
const SMALL_BLOCK: usize = 1 << 14;

/// `Get_ExpectedBlockSize_From_Dict`: the most output one block may have.
fn expected_block_size(dict_size: u32) -> usize {
    const MIN: u64 = 1 << 20;
    const MAX: u64 = 1 << 28;
    let dict = u64::from(dict_size);
    let mut size = (dict << 2).clamp(MIN, MAX);
    if size < dict {
        size = dict;
    }
    size = (size + (MIN - 1)) & !(MIN - 1);
    usize::try_from(size).unwrap_or(usize::MAX)
}

/// `MTDEC_PARSE_*`: where walking a block's input left it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parsed {
    /// The piece is walked and the block goes on.
    Continue,
    /// The block ends where the next begins.
    New,
    /// The block ends, and with it the stream.
    End,
    /// The walk cannot settle the block: decode in sequence from here.
    Overflow,
}

/// What `Lzma2DecMt_MtCallback_Parse` keeps for a block (its
/// `CLzma2DecMtThread`).
#[derive(Clone, Copy, Debug)]
struct Block {
    in_pre_size: usize,
    out_pre_size: usize,
    parse_status: ParseStatus,
    state: Parsed,
}

impl Block {
    const fn new() -> Self {
        Self {
            in_pre_size: 0,
            out_pre_size: 0,
            parse_status: ParseStatus::NotSpecified,
            state: Parsed::Continue,
        }
    }

    /// Whether the walk saw the block end: then its decoding must end
    /// exactly there (FINISH_END).
    fn finished(&self) -> bool {
        matches!(
            self.parse_status,
            ParseStatus::FinishedWithMark | ParseStatus::NewBlock
        )
    }
}

/// `Lzma2DecMt_MtCallback_Parse`: walks one piece of a block's input.
/// Returns where it left the block and the input it took.
fn parse_piece(
    dec: &mut Lzma2Dec,
    block: &mut Block,
    start_call: bool,
    src: &[u8],
    remaining: usize,
    out_block_max: usize,
) -> (Parsed, usize) {
    if start_call {
        dec.init();
        *block = Block::new();
    }
    // `checkFinishBlock` stays true: a 7z folder is decoded in finish mode.
    let limit = out_block_max.min(remaining);
    let mut src_size = 0usize;
    let mut src_size_point = 0usize;
    let mut dic_pos_point = 0usize;
    let mut overflow = false;
    let mut unpack_rem = 0u32;
    let mut status;
    loop {
        let rest = src.get(src_size..).unwrap_or(&[]);
        let (s, used) = dec.parse(limit.saturating_sub(dec.parse_pos), rest, true);
        status = s;
        src_size += used;
        match status {
            ParseStatus::NewChunk => {
                if dec.unpack_size as usize > out_block_max.saturating_sub(dec.parse_pos) {
                    overflow = true;
                    break;
                }
            }
            ParseStatus::NewBlock => {
                if dec.parse_pos == 0 {
                    continue;
                }
                if dec.parse_pos >= SMALL_BLOCK {
                    break;
                }
                dic_pos_point = dec.parse_pos;
                src_size_point = src_size;
            }
            ParseStatus::NotFinished => {
                overflow = true;
                break;
            }
            _ => {
                unpack_rem = dec.unpack_extra();
                break;
            }
        }
    }
    if dic_pos_point != 0
        && !matches!(
            status,
            ParseStatus::NewBlock | ParseStatus::FinishedWithMark | ParseStatus::NotSpecified
        )
    {
        // Back to the last small block's end.
        status = ParseStatus::NewBlock;
        unpack_rem = 0;
        dec.parse_pos = dic_pos_point;
        src_size = src_size_point;
        overflow = false;
    }

    block.in_pre_size += src_size;
    block.parse_status = status;
    let state = if overflow {
        Parsed::Overflow
    } else {
        let mut dic_pos = dec.parse_pos;
        let state = match status {
            ParseStatus::NeedsMoreInput => Parsed::Continue,
            ParseStatus::NewBlock => {
                // The next block's control byte is the next block's.
                src_size -= 1;
                block.in_pre_size -= 1;
                Parsed::New
            }
            _ => {
                if status != ParseStatus::FinishedWithMark && unpack_rem != 0 {
                    // Room for the most the chunk it stopped in can make.
                    dic_pos += limit.saturating_sub(dic_pos).min(unpack_rem as usize);
                }
                Parsed::End
            }
        };
        block.out_pre_size = dic_pos;
        state
    };
    block.state = state;
    (state, src_size)
}

/// `MtDec_Code` with the `Lzma2DecMt_MtCallback_*`s, one block after
/// another, and `Lzma2DecMt_Decode`'s fall back to [`decode_st`]. Returns
/// the input taken and whether it ended without an error: errors decoding
/// a block are not passed on -- the block's output stops short, and the
/// size checks in [`lzma2`] say so.
#[allow(clippy::too_many_lines)]
fn decode_mt(prop: u8, input: &[u8], out: &mut Vec<u8>, out_size: usize) -> (usize, bool) {
    let dict_size = dic_size_from_prop(prop).unwrap_or(u32::MAX);
    let out_block_max = expected_block_size(dict_size);
    let Some(mut dec) = Lzma2Dec::new(prop) else {
        return (0, false);
    };

    let mut read_pos = 0usize;
    let mut read_was_finished = false;
    // The input after a block cut inside a piece: the next block's start.
    let mut cross = (0usize, 0usize);
    // `mtc.inProcessed`: the input of the blocks written.
    let mut in_processed = 0usize;
    // `outProcessed_Parse`: the output of the blocks walked.
    let mut out_parsed = 0usize;
    // `p->wasInterrupted`: a block failed; nothing after it is written.
    let mut was_interrupted = false;
    // An earlier block's code failed (`MtDec_Interrupt`): later blocks stop
    // before reading.
    let mut need_interrupt = false;
    // `p->overflow || p->isAllocError`: decode in sequence from the first
    // block kept for it.
    let mut fall_back = false;
    let mut kept: Option<(usize, usize)> = None;

    loop {
        if need_interrupt {
            break;
        }
        let mut finish = read_was_finished;
        let mut need_code = false;
        let mut overflow = false;
        let mut in_data_size = 0usize;
        let mut block_start = None;
        let mut first = true;
        let mut cross_size = cross.1 - cross.0;
        let mut block = Block::new();

        // ---------- READ and PARSE ----------
        loop {
            let (piece_start, size);
            if cross_size == 0 {
                piece_start = read_pos;
                size = IN_BUF_SIZE_MT.min(input.len() - read_pos);
                read_pos += size;
                in_data_size += size;
                finish = size != IN_BUF_SIZE_MT;
                if finish {
                    read_was_finished = true;
                }
            } else {
                piece_start = cross.0;
                size = cross_size;
                in_data_size = cross_size;
            }
            block_start.get_or_insert(piece_start);
            let piece = input.get(piece_start..piece_start + size).unwrap_or(&[]);
            let (state, src_size) = parse_piece(
                &mut dec,
                &mut block,
                first,
                piece,
                out_size.saturating_sub(out_parsed),
                out_block_max,
            );
            if state == Parsed::Overflow {
                finish = true;
                overflow = true;
                cross = (0, 0);
                break;
            }
            if cross_size != 0 {
                cross.0 += src_size;
            }
            if state != Parsed::Continue || finish {
                if state == Parsed::End {
                    finish = true;
                }
                need_code = true;
                if src_size == size {
                    cross = (0, 0);
                    break;
                }
                if state == Parsed::End {
                    // What follows the end is not the block's.
                    in_data_size -= size - src_size;
                    break;
                }
                if cross_size == 0 {
                    in_data_size -= size - src_size;
                    cross = (piece_start + src_size, piece_start + size);
                } else {
                    in_data_size = src_size;
                }
                finish = false;
                break;
            }
            first = false;
            if cross_size != 0 {
                cross_size = 0;
                cross = (0, 0);
            }
        }
        if !overflow && matches!(block.state, Parsed::New | Parsed::End) {
            out_parsed += block.out_pre_size;
        }
        let block_start = block_start.unwrap_or(read_pos);

        // ---------- PreCode ----------
        // `codeRes`: `None` is SZ_OK.
        let mut code_failed = false;
        let mut alloc_error = false;
        if need_code {
            if block.in_pre_size == 0 {
                code_failed = true;
            } else if block.out_pre_size == 0 {
                // `MidAlloc(0)` returns no buffer: SZ_ERROR_MEM, and the
                // block is decoded in sequence instead.
                code_failed = true;
                alloc_error = true;
            }
            if code_failed {
                need_code = false;
                finish = true;
            }
        }

        // ---------- CODE ----------
        let block_base = out.len();
        let mut in_code_size = 0usize;
        if need_code {
            dec.init();
            let block_input = input
                .get(block_start..block_start + in_data_size)
                .unwrap_or(&[]);
            let finish_mode = if block.finished() {
                Finish::End
            } else {
                Finish::Any
            };
            let step = dec.decode_to_dic(
                out,
                block_base + block.out_pre_size,
                block_input,
                finish_mode,
            );
            in_code_size = step.consumed;
            let out_code_size = out.len() - block_base;
            let failed = step.fault.is_some()
                || (block.finished()
                    && (step.consumed != block_input.len()
                        || (block.in_pre_size == in_code_size
                            && block.out_pre_size != out_code_size)));
            if failed {
                code_failed = true;
                need_interrupt = true;
            }
        }

        // ---------- WRITE ----------
        let mut write_to_stream = true;
        if was_interrupted {
            write_to_stream = false;
        } else {
            if code_failed {
                was_interrupted = true;
            }
            if alloc_error || overflow {
                was_interrupted = true;
                fall_back = true;
                write_to_stream = false;
            }
        }
        // `Lzma2DecMt_MtCallback_Write`
        let mut need_continue = false;
        let mut can_recode = true;
        let mut write_failed = false;
        if write_to_stream {
            in_processed += in_code_size;
            let out_code_size = out.len() - block_base;
            if !code_failed
                && block.finished()
                && (block.out_pre_size != out_code_size || block.in_pre_size != in_code_size)
            {
                write_failed = true;
                out.truncate(block_base);
            } else {
                can_recode = false;
                need_continue = !matches!(block.state, Parsed::Overflow | Parsed::End);
            }
        } else {
            out.truncate(block_base);
        }
        if write_failed {
            was_interrupted = true;
        }
        if write_failed || (!need_continue && !finish) {
            need_interrupt = true;
        }
        // The input kept to decode in sequence (`numFilledThreads`).
        if can_recode
            && (!need_code || write_failed || was_interrupted || code_failed)
            && (in_data_size != 0 || !finish)
            && kept.is_none()
        {
            kept = Some((block_start, in_data_size));
        }
        if !finish || need_continue {
            continue;
        }
        break;
    }

    if !fall_back {
        return (in_processed, true);
    }
    // `Lzma2Dec_Decode_ST` from the kept input (`MtDec_Read`), then the
    // cross input, then the rest of the stream.
    let Some((start, len)) = kept else {
        return (in_processed, false);
    };
    let kept_end = start + len;
    let (consumed, ok) = if kept_end == read_pos && cross.0 == cross.1 {
        decode_st(prop, input.get(start..).unwrap_or(&[]), out, out_size)
    } else {
        let mut joined = Vec::new();
        joined.extend_from_slice(input.get(start..kept_end).unwrap_or(&[]));
        joined.extend_from_slice(input.get(cross.0..cross.1).unwrap_or(&[]));
        joined.extend_from_slice(input.get(read_pos..).unwrap_or(&[]));
        decode_st(prop, &joined, out, out_size)
    };
    (in_processed + consumed, ok)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    fn sample(n: usize, seed: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity(n);
        let mut x = seed;
        while v.len() < n {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if x >> 29 == 0 {
                v.extend_from_slice(b"seven zip ");
            } else {
                v.push((x >> 24) as u8 & 0x3F);
            }
        }
        v.truncate(n);
        v
    }

    fn stream(data: &[u8]) -> (u8, Vec<u8>) {
        let opts = xz::LzmaOptions::preset(xz::Preset::DEFAULT);
        (opts.lzma2_prop(), xz::lzma2_encode(data, &opts).unwrap())
    }

    /// The chunk headers' offsets in a raw LZMA2 stream.
    fn chunks(input: &[u8]) -> Vec<usize> {
        let mut at = Vec::new();
        let mut i = 0;
        loop {
            let c = input[i];
            at.push(i);
            if c == 0 {
                return at;
            }
            if c & 0x80 == 0 {
                i += 3 + unpacked(input, i);
            } else {
                let pack = usize::from(u16::from_be_bytes([input[i + 3], input[i + 4]])) + 1;
                i += 5 + usize::from(c >= 0xC0) + pack;
            }
        }
    }

    /// The unpacked size of the chunk at `i`.
    fn unpacked(input: &[u8], i: usize) -> usize {
        let c = input[i];
        let high = if c & 0x80 == 0 {
            0
        } else {
            usize::from(c & 0x1F) << 16
        };
        high + usize::from(u16::from_be_bytes([input[i + 1], input[i + 2]])) + 1
    }

    #[test]
    fn block_sizes_are_7zips() {
        assert_eq!(expected_block_size(1 << 12), 1 << 20);
        assert_eq!(expected_block_size(1 << 20), 4 << 20);
        assert_eq!(expected_block_size(64 << 20), 256 << 20);
        assert_eq!(expected_block_size(3 << 29), 3 << 29);
        assert_eq!(expected_block_size(0xFFFF_FFFF), 1 << 32);
    }

    #[test]
    fn a_sound_stream_decodes_either_way() {
        for n in [0usize, 1, 5000, 300_000] {
            let data = sample(n, 7);
            let (prop, input) = stream(&data);
            for threads in [Threads::One, Threads::Many] {
                let c = lzma2(&[prop], &input, data.len(), threads).unwrap();
                assert!(c.ok, "{n} {threads:?}");
                assert_eq!(c.out, data, "{n} {threads:?}");
            }
        }
    }

    #[test]
    fn a_wrong_size_or_trailing_input_is_damage() {
        let data = sample(5000, 3);
        let (prop, mut input) = stream(&data);
        for threads in [Threads::One, Threads::Many] {
            let c = lzma2(&[prop], &input, data.len() - 1, threads).unwrap();
            assert!(!c.ok, "short {threads:?}");
            let c = lzma2(&[prop], &input, data.len() + 1, threads).unwrap();
            assert!(!c.ok, "long {threads:?}");
            assert_eq!(c.out, data, "long {threads:?}");
        }
        input.push(0);
        for threads in [Threads::One, Threads::Many] {
            let c = lzma2(&[prop], &input, data.len(), threads).unwrap();
            assert!(!c.ok, "trailing {threads:?}");
            assert_eq!(c.out, data, "trailing {threads:?}");
        }
    }

    #[test]
    fn properties_7zip_refuses_are_unsupported() {
        assert!(lzma2(&[41], &[0], 0, Threads::Many).is_none());
        assert!(lzma2(&[], &[0], 0, Threads::Many).is_none());
        assert!(lzma2(&[1, 2], &[0], 0, Threads::Many).is_none());
        assert!(lzma(&[225, 0, 0, 1, 0], &[0; 5], 0).is_none());
        assert!(lzma(&[93, 0, 0, 1], &[0; 5], 0).is_none());
    }

    /// A later chunk whose packed size runs past the end of the stream:
    /// walked, the block ends before it; in sequence, it is decoded, and
    /// only then is the stream found short.
    #[test]
    fn a_chunk_running_past_the_end_parts_the_two_ways() {
        let data = sample(400_000, 11);
        let (prop, mut input) = stream(&data);
        let at = chunks(&input);
        assert!(at.len() > 3, "{at:?}");
        let last = at[at.len() - 2];
        assert!(input[last] >= 0x80, "an LZMA chunk");
        let before: usize = at[..at.len() - 2]
            .iter()
            .map(|&i| unpacked(&input, i))
            .sum();
        let header = 5 + usize::from(input[last] >= 0xC0);
        let rest = input.len() - (last + header);
        let pack = rest + 1;
        assert!(pack <= 1 << 16);
        input[last + 3..last + 5].copy_from_slice(&u16::try_from(pack - 1).unwrap().to_be_bytes());
        let many = lzma2(&[prop], &input, data.len(), Threads::Many).unwrap();
        let one = lzma2(&[prop], &input, data.len(), Threads::One).unwrap();
        assert!(!many.ok && !one.ok);
        assert_eq!(many.out.len(), before);
        assert!(many.out == data[..before]);
        assert_eq!(one.out.len(), data.len());
        assert!(one.out == data);
    }

    /// The walk cuts blocks at dictionary resets. A block whose first chunk
    /// declares packed bytes past the end of the stream has nothing to walk
    /// to, so no output to decode into, and is decoded in sequence -- all
    /// of the chunk -- after the blocks before it are written.
    #[test]
    fn a_damaged_first_chunk_sends_its_block_to_the_sequence() {
        let a = sample(100_000, 17);
        let b = sample(30_000, 19);
        let (prop, mut input) = stream(&a);
        let (prop_b, sb) = stream(&b);
        assert_eq!(prop, prop_b);
        assert_eq!(input.pop(), Some(0));
        let second = input.len();
        input.extend_from_slice(&sb);
        assert!(input[second] >= 0xE0, "a dictionary reset");
        let header = 5 + usize::from(input[second] >= 0xC0);
        let pack = input.len() - (second + header) + 1;
        assert!(pack <= 1 << 16);
        input[second + 3..second + 5]
            .copy_from_slice(&u16::try_from(pack - 1).unwrap().to_be_bytes());
        let mut want = a.clone();
        want.extend_from_slice(&b);
        for threads in [Threads::Many, Threads::One] {
            let c = lzma2(&[prop], &input, want.len(), threads).unwrap();
            assert!(!c.ok, "{threads:?}");
            assert!(
                c.out == want,
                "{threads:?}: {} of {}",
                c.out.len(),
                want.len()
            );
        }
    }

    /// Without its end marker, the walk takes the stream as it is; decoded
    /// in sequence, the stream is short.
    #[test]
    fn a_stream_without_its_end_marker_parts_the_two_ways() {
        let data = sample(100_000, 13);
        let (prop, mut input) = stream(&data);
        assert_eq!(input.pop(), Some(0));
        let many = lzma2(&[prop], &input, data.len(), Threads::Many).unwrap();
        let one = lzma2(&[prop], &input, data.len(), Threads::One).unwrap();
        assert!(many.ok);
        assert!(!one.ok);
        assert!(many.out == data && one.out == data);
    }

    #[test]
    fn lzma_with_an_end_marker_after_its_size_is_sound() {
        let data = sample(5000, 5);
        let opts = xz::LzmaOptions::preset(xz::Preset::DEFAULT);
        let lzma_file = xz::compress_lzma(&data, &opts).unwrap();
        let (props, input) = (&lzma_file[..5], &lzma_file[13..]);
        let c = lzma(props, input, data.len()).unwrap();
        assert!(c.ok);
        assert_eq!(c.out, data);
        // Short of its size, the next symbol is no end marker: damage.
        let c = lzma(props, input, data.len() - 1).unwrap();
        assert!(!c.ok);
        assert_eq!(c.out, data[..data.len() - 1]);
        // Its last byte missing: damage, with what decoded kept.
        let c = lzma(props, &input[..input.len() - 1], data.len()).unwrap();
        assert!(!c.ok);
        assert_eq!(c.out, data[..c.out.len()]);
        // The end marker before the size: damage, the data all out.
        let c = lzma(props, input, data.len() + 1).unwrap();
        assert!(!c.ok);
        assert_eq!(c.out, data);
        // A byte after the stream: damage too -- the packed stream must be
        // used up.
        let mut longer = input.to_vec();
        longer.push(0);
        let c = lzma(props, &longer, data.len()).unwrap();
        assert!(!c.ok);
        assert_eq!(c.out, data);
    }
}
