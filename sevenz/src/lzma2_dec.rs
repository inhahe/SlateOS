//! LZMA2 decoding exactly as 7-Zip does it: `C/Lzma2Dec.c` from the LZMA
//! SDK 26.00 (Igor Pavlov, public domain), ported for the 7z reader.
//!
//! An LZMA2 stream is a run of chunks, each a header -- a control byte and
//! sizes -- and either stored bytes or an LZMA range-coded run with its own
//! packed size, ending at a zero control byte. `Lzma2Dec` hands each LZMA
//! chunk to [`LzmaDec`] with exactly its packed size of input, which is
//! where it parts from liblzma (see `lzma_dec.rs`). [`Lzma2Dec::parse`] walks
//! the chunk headers without decoding, as 7-Zip's multi-threaded decoder
//! does to cut a stream into blocks (`lzma_coder.rs`).
//!
//! The control byte:
//!
//! | byte | chunk |
//! |---|---|
//! | `00` | end of the stream |
//! | `01` | stored, dictionary reset |
//! | `02` | stored |
//! | `100uuuuu` | LZMA |
//! | `101uuuuu` | LZMA, state reset |
//! | `110uuuuu` | LZMA, state reset, new properties |
//! | `111uuuuu` | LZMA, state reset, new properties, dictionary reset |

// Sizes here are a chunk's: at most 2^21 unpacked and 2^16 packed, and
// never decremented past the zero the code checks for first.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec::Vec;

use crate::lzma_dec::{Fault, Finish, LzmaDec, Props, Status, Step};

/// `LZMA2_CONTROL_COPY_RESET_DIC`
const CONTROL_COPY_RESET_DIC: u8 = 1;
/// `LZMA2_LCLP_MAX`
const LCLP_MAX: u32 = 4;

/// `ELzma2State`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Control,
    Unpack0,
    Unpack1,
    Pack0,
    Pack1,
    Prop,
    Data,
    DataCont,
    Finished,
    Error,
}

/// `ELzma2ParseStatus`: what [`Lzma2Dec::parse`] stopped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParseStatus {
    /// `LZMA_STATUS_NOT_SPECIFIED`: the stream is damaged.
    NotSpecified,
    /// `LZMA_STATUS_FINISHED_WITH_MARK`: its end.
    FinishedWithMark,
    /// `LZMA_STATUS_NOT_FINISHED`: the output limit.
    NotFinished,
    /// `LZMA_STATUS_NEEDS_MORE_INPUT`
    NeedsMoreInput,
    /// `LZMA2_PARSE_STATUS_NEW_BLOCK`: a chunk that resets the dictionary,
    /// its control byte read.
    NewBlock,
    /// `LZMA2_PARSE_STATUS_NEW_CHUNK`: an LZMA chunk's header read.
    NewChunk,
}

/// `LZMA2_DIC_SIZE_FROM_PROP`, with 40's `0xFFFFFFFF`; `None` above 40
/// (`SZ_ERROR_UNSUPPORTED`).
pub(crate) fn dic_size_from_prop(prop: u8) -> Option<u32> {
    match prop {
        0..=39 => Some((2 | u32::from(prop & 1)) << (prop / 2 + 11)),
        40 => Some(0xFFFF_FFFF),
        _ => None,
    }
}

/// `CLzma2Dec`
#[derive(Clone, Debug)]
pub(crate) struct Lzma2Dec {
    pub(crate) decoder: LzmaDec,
    state: State,
    control: u8,
    need_init_level: u8,
    is_extra_mode: bool,
    pub(crate) unpack_size: u32,
    pack_size: u32,
    /// `decoder.dicPos` while parsing: the output the chunks parsed so far
    /// will make. (Decoding, the output's length is the position.)
    pub(crate) parse_pos: usize,
}

impl Lzma2Dec {
    /// `Lzma2Dec_Allocate(prop)` and `Lzma2Dec_Init`; `None` for a property
    /// above 40.
    pub(crate) fn new(prop: u8) -> Option<Self> {
        // `Lzma2Dec_GetOldProps`: the probabilities are sized for lc + lp of
        // 4, the most a chunk may set.
        let props = Props {
            lc: LCLP_MAX,
            lp: 0,
            pb: 0,
            dic_size: dic_size_from_prop(prop)?.max(1 << 12),
        };
        let mut dec = Self {
            decoder: LzmaDec::new(props),
            state: State::Control,
            control: 0,
            need_init_level: 0,
            is_extra_mode: false,
            unpack_size: 0,
            pack_size: 0,
            parse_pos: 0,
        };
        dec.init();
        Some(dec)
    }

    /// `Lzma2Dec_Init`
    pub(crate) fn init(&mut self) {
        self.state = State::Control;
        self.need_init_level = 0xE0;
        self.is_extra_mode = false;
        self.unpack_size = 0;
        self.parse_pos = 0;
        self.decoder.init_dic_and_state(true, true);
    }

    /// `Lzma2Dec_GetUnpackExtra`: the most output the chunk being parsed
    /// may still make.
    pub(crate) fn unpack_extra(&self) -> u32 {
        if self.is_extra_mode {
            self.unpack_size
        } else {
            0
        }
    }

    fn is_uncompressed(&self) -> bool {
        self.control & 0x80 == 0
    }

    /// `Lzma2Dec_UpdateState`: one header byte.
    fn update_state(&mut self, b: u8) -> State {
        match self.state {
            State::Control => {
                self.is_extra_mode = false;
                self.control = b;
                if b == 0 {
                    return State::Finished;
                }
                if self.is_uncompressed() {
                    if b == CONTROL_COPY_RESET_DIC {
                        self.need_init_level = 0xC0;
                    } else if b > 2 || self.need_init_level == 0xE0 {
                        return State::Error;
                    }
                } else {
                    if b < self.need_init_level {
                        return State::Error;
                    }
                    self.need_init_level = 0;
                    self.unpack_size = u32::from(b & 0x1F) << 16;
                }
                State::Unpack0
            }
            State::Unpack0 => {
                self.unpack_size |= u32::from(b) << 8;
                State::Unpack1
            }
            State::Unpack1 => {
                self.unpack_size |= u32::from(b);
                self.unpack_size += 1;
                if self.is_uncompressed() {
                    State::Data
                } else {
                    State::Pack0
                }
            }
            State::Pack0 => {
                self.pack_size = u32::from(b) << 8;
                State::Pack1
            }
            State::Pack1 => {
                self.pack_size |= u32::from(b);
                self.pack_size += 1;
                if self.control & 0x40 != 0 {
                    State::Prop
                } else {
                    State::Data
                }
            }
            State::Prop => {
                if b >= 9 * 5 * 5 {
                    return State::Error;
                }
                let lc = u32::from(b % 9);
                let b = b / 9;
                self.decoder.prop.pb = u32::from(b / 5);
                let lp = u32::from(b % 5);
                if lc + lp > LCLP_MAX {
                    return State::Error;
                }
                self.decoder.prop.lc = lc;
                self.decoder.prop.lp = lp;
                State::Data
            }
            _ => State::Error,
        }
    }

    /// `Lzma2Dec_DecodeToDic`: decodes `src` onto `out` up to `dic_limit`
    /// bytes in all.
    pub(crate) fn decode_to_dic(
        &mut self,
        out: &mut Vec<u8>,
        dic_limit: usize,
        src: &[u8],
        finish: Finish,
    ) -> Step {
        let in_size = src.len();
        let mut consumed = 0usize;

        while self.state != State::Error {
            if self.state == State::Finished {
                return Step {
                    consumed,
                    status: Status::FinishedWithMark,
                    fault: None,
                };
            }
            let dic_pos = out.len();
            if dic_pos == dic_limit && finish == Finish::Any {
                return Step {
                    consumed,
                    status: Status::NotFinished,
                    fault: None,
                };
            }

            if self.state != State::Data && self.state != State::DataCont {
                let Some(&b) = src.get(consumed) else {
                    return Step {
                        consumed,
                        status: Status::NeedsMoreInput,
                        fault: None,
                    };
                };
                consumed += 1;
                self.state = self.update_state(b);
                if dic_pos == dic_limit && self.state != State::Finished {
                    break;
                }
                continue;
            }

            let mut in_cur = in_size - consumed;
            let mut out_cur = dic_limit.saturating_sub(dic_pos);
            let mut cur_finish = Finish::Any;
            if out_cur >= self.unpack_size as usize {
                out_cur = self.unpack_size as usize;
                cur_finish = Finish::End;
            }

            if self.is_uncompressed() {
                if in_cur == 0 {
                    return Step {
                        consumed,
                        status: Status::NeedsMoreInput,
                        fault: None,
                    };
                }
                if self.state == State::Data {
                    let init_dic = self.control == CONTROL_COPY_RESET_DIC;
                    self.decoder.init_dic_and_state(init_dic, false);
                }
                in_cur = in_cur.min(out_cur);
                if in_cur == 0 {
                    break;
                }
                let Some(bytes) = src.get(consumed..consumed + in_cur) else {
                    break;
                };
                self.decoder.update_with_uncompressed(out, bytes);
                consumed += in_cur;
                self.unpack_size -= u32::try_from(in_cur).unwrap_or(u32::MAX);
                self.state = if self.unpack_size == 0 {
                    State::Control
                } else {
                    State::DataCont
                };
            } else {
                if self.state == State::Data {
                    let init_dic = self.control >= 0xE0;
                    let init_state = self.control >= 0xA0;
                    self.decoder.init_dic_and_state(init_dic, init_state);
                    self.state = State::DataCont;
                }
                in_cur = in_cur.min(self.pack_size as usize);
                let Some(chunk) = src.get(consumed..consumed + in_cur) else {
                    break;
                };
                let step = self
                    .decoder
                    .decode_to_dic(out, dic_pos + out_cur, chunk, cur_finish);
                consumed += step.consumed;
                self.pack_size -= u32::try_from(step.consumed).unwrap_or(u32::MAX);
                let produced = out.len() - dic_pos;
                self.unpack_size -= u32::try_from(produced).unwrap_or(u32::MAX);
                if step.fault.is_some() {
                    break;
                }
                if step.status == Status::NeedsMoreInput {
                    if self.pack_size == 0 {
                        break;
                    }
                    return Step {
                        consumed,
                        status: Status::NeedsMoreInput,
                        fault: None,
                    };
                }
                if step.consumed == 0 && produced == 0 {
                    if step.status != Status::MaybeFinishedWithoutMark
                        || self.unpack_size != 0
                        || self.pack_size != 0
                    {
                        break;
                    }
                    self.state = State::Control;
                }
            }
        }

        self.state = State::Error;
        Step {
            consumed,
            status: Status::NotSpecified,
            fault: Some(Fault::Data),
        }
    }

    /// `Lzma2Dec_Parse`: walks the chunk headers of `src`, skipping their
    /// data, as far as `out_size` more bytes of output; with
    /// `check_finish_block`, on to the header after the last of them.
    /// Returns where it stopped and the input it took.
    pub(crate) fn parse(
        &mut self,
        mut out_size: usize,
        src: &[u8],
        check_finish_block: bool,
    ) -> (ParseStatus, usize) {
        let in_size = src.len();
        let mut consumed = 0usize;

        while self.state != State::Error {
            if self.state == State::Finished {
                return (ParseStatus::FinishedWithMark, consumed);
            }
            if out_size == 0 && !check_finish_block {
                return (ParseStatus::NotFinished, consumed);
            }

            if self.state != State::Data && self.state != State::DataCont {
                let Some(&b) = src.get(consumed) else {
                    return (ParseStatus::NeedsMoreInput, consumed);
                };
                consumed += 1;
                self.state = self.update_state(b);
                if self.state == State::Unpack0
                    && (self.control == CONTROL_COPY_RESET_DIC || self.control >= 0xE0)
                {
                    return (ParseStatus::NewBlock, consumed);
                }
                if out_size == 0 && self.state != State::Finished {
                    return (ParseStatus::NotFinished, consumed);
                }
                if self.state == State::Data {
                    return (ParseStatus::NewChunk, consumed);
                }
                continue;
            }

            if out_size == 0 {
                return (ParseStatus::NotFinished, consumed);
            }

            let mut in_cur = in_size - consumed;
            if self.is_uncompressed() {
                if in_cur == 0 {
                    return (ParseStatus::NeedsMoreInput, consumed);
                }
                in_cur = in_cur.min(self.unpack_size as usize).min(out_size);
                self.parse_pos += in_cur;
                consumed += in_cur;
                out_size -= in_cur;
                self.unpack_size -= u32::try_from(in_cur).unwrap_or(u32::MAX);
                self.state = if self.unpack_size == 0 {
                    State::Control
                } else {
                    State::DataCont
                };
            } else {
                self.is_extra_mode = true;
                if in_cur == 0 {
                    if self.pack_size != 0 {
                        return (ParseStatus::NeedsMoreInput, consumed);
                    }
                } else if self.state == State::Data {
                    self.state = State::DataCont;
                    if src.get(consumed).is_some_and(|&b| b != 0) {
                        // An LZMA chunk's first byte must be zero.
                        consumed += 1;
                        self.pack_size -= 1;
                        break;
                    }
                }
                in_cur = in_cur.min(self.pack_size as usize);
                consumed += in_cur;
                self.pack_size -= u32::try_from(in_cur).unwrap_or(u32::MAX);
                if self.pack_size == 0 {
                    let rem = out_size.min(self.unpack_size as usize);
                    self.parse_pos += rem;
                    self.unpack_size -= u32::try_from(rem).unwrap_or(u32::MAX);
                    out_size -= rem;
                    if self.unpack_size == 0 {
                        self.state = State::Control;
                    }
                }
            }
        }

        self.state = State::Error;
        (ParseStatus::NotSpecified, consumed)
    }
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

    fn sample(n: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(n);
        let mut x = 0x1234_5678u32;
        while v.len() < n {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if x >> 30 == 0 {
                v.extend_from_slice(b"seven zip ");
            } else {
                v.push((x >> 24) as u8);
            }
        }
        v.truncate(n);
        v
    }

    /// A raw LZMA2 stream of `data` and its dictionary property, by
    /// liblzma through the `xz` crate.
    fn stream(data: &[u8]) -> (u8, Vec<u8>) {
        let opts = xz::LzmaOptions::preset(xz::Preset::DEFAULT);
        (opts.lzma2_prop(), xz::lzma2_encode(data, &opts).unwrap())
    }

    #[test]
    fn the_dictionary_sizes_are_lzma2s() {
        assert_eq!(dic_size_from_prop(0), Some(4096));
        assert_eq!(dic_size_from_prop(1), Some(6144));
        assert_eq!(dic_size_from_prop(16), Some(1 << 20));
        assert_eq!(dic_size_from_prop(18), Some(2 << 20));
        assert_eq!(dic_size_from_prop(40), Some(0xFFFF_FFFF));
        assert_eq!(dic_size_from_prop(41), None);
    }

    #[test]
    fn a_sound_stream_decodes_to_its_end_marker() {
        let data = sample(300_000);
        let (prop, input) = stream(&data);
        let mut dec = Lzma2Dec::new(prop).unwrap();
        let mut out = Vec::new();
        let step = dec.decode_to_dic(&mut out, data.len(), &input, Finish::End);
        assert_eq!(
            step,
            Step {
                consumed: input.len(),
                status: Status::FinishedWithMark,
                fault: None
            }
        );
        assert_eq!(out, data);
    }

    #[test]
    fn parsing_finds_every_chunk_and_the_end() {
        let data = sample(300_000);
        let (prop, input) = stream(&data);
        let mut dec = Lzma2Dec::new(prop).unwrap();
        let mut used = 0;
        let mut chunks = 0;
        loop {
            let (status, n) = dec.parse(usize::MAX, &input[used..], true);
            used += n;
            match status {
                ParseStatus::NewChunk | ParseStatus::NewBlock => chunks += 1,
                ParseStatus::FinishedWithMark => break,
                other => panic!("{other:?} at {used}"),
            }
        }
        assert_eq!(used, input.len());
        assert_eq!(dec.parse_pos, data.len());
        assert!(chunks > 2, "{chunks} chunks");
    }

    #[test]
    fn a_chunk_gets_its_packed_size_and_no_more() {
        let data = sample(3000);
        let (prop, mut input) = stream(&data);
        assert_eq!(input[0], 0xE0);
        // One packed byte fewer than the chunk holds: its range coder runs
        // dry at the chunk's end, though the stream goes on.
        let pack = u16::from_be_bytes([input[3], input[4]]);
        input[3..5].copy_from_slice(&(pack - 1).to_be_bytes());
        let mut dec = Lzma2Dec::new(prop).unwrap();
        let mut out = Vec::new();
        let step = dec.decode_to_dic(&mut out, data.len(), &input, Finish::End);
        assert_eq!(step.fault, Some(Fault::Data));
        assert!(out.len() <= data.len());
        assert_eq!(out[..], data[..out.len()]);
    }

    #[test]
    fn the_first_chunk_must_reset_the_dictionary() {
        let data = sample(3000);
        let (prop, mut input) = stream(&data);
        input[0] = 0xC0;
        let mut dec = Lzma2Dec::new(prop).unwrap();
        let mut out = Vec::new();
        let step = dec.decode_to_dic(&mut out, data.len(), &input, Finish::End);
        assert_eq!(
            step,
            Step {
                consumed: 1,
                status: Status::NotSpecified,
                fault: Some(Fault::Data)
            }
        );
        assert!(out.is_empty());
        // A stored chunk without the reset is refused the same way.
        input[0] = 2;
        let mut dec = Lzma2Dec::new(prop).unwrap();
        let mut out = Vec::new();
        let step = dec.decode_to_dic(&mut out, data.len(), &input, Finish::End);
        assert_eq!(step.fault, Some(Fault::Data));
        assert_eq!(step.consumed, 1);
    }

    /// A chunk's new properties may not ask for more than four literal
    /// context and position bits between them.
    #[test]
    fn lc_and_lp_together_are_at_most_four() {
        let data = sample(3000);
        let (prop, mut input) = stream(&data);
        assert!(input[0] >= 0xC0, "the first chunk sets properties");
        // The byte is lc + 9 * (lp + 5 * pb).
        for (byte, ok) in [(4, true), (3 + 9, true), (4 + 9, false), (8, false)] {
            input[5] = byte;
            let mut dec = Lzma2Dec::new(prop).unwrap();
            let mut out = Vec::new();
            let step = dec.decode_to_dic(&mut out, data.len(), &input, Finish::End);
            if ok {
                // The data was not written for these properties, but they
                // are accepted: decoding gets past the header.
                assert!(step.consumed > 6, "{byte}");
            } else {
                assert_eq!(
                    step,
                    Step {
                        consumed: 6,
                        status: Status::NotSpecified,
                        fault: Some(Fault::Data)
                    },
                    "{byte}"
                );
                assert!(out.is_empty());
            }
        }
    }

    #[test]
    fn stopping_at_a_limit_inside_a_chunk_is_not_finished() {
        let data = sample(3000);
        let (prop, input) = stream(&data);
        let mut dec = Lzma2Dec::new(prop).unwrap();
        let mut out = Vec::new();
        let step = dec.decode_to_dic(&mut out, 1000, &input, Finish::Any);
        assert_eq!(step.status, Status::NotFinished);
        assert_eq!(out[..], data[..1000]);
        // With FINISH_END there, the chunk not being over is damage.
        let mut dec = Lzma2Dec::new(prop).unwrap();
        let mut out = Vec::new();
        let step = dec.decode_to_dic(&mut out, 1000, &input, Finish::End);
        assert_eq!(step.fault, Some(Fault::Data));
        assert_eq!(out[..], data[..1000]);
    }
}
