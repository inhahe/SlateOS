//! A frame: its header, its subframes -- one a channel -- and the samples
//! they decode to (RFC 9639 §9), translated into Rust from libFLAC 1.5.0's
//! `stream_decoder.c` (`read_frame_`, `read_frame_header_`, the
//! `read_subframe_*` functions, `read_residual_partitioned_rice_`,
//! `undo_channel_coding`), `fixed.c` and `lpc.c` (the restore functions),
//! copyright Josh Coalson and Xiph.Org, used under libFLAC's BSD licence
//! (`licenses/flac-COPYING`).
//!
//! A frame that does not decode says why, as libFLAC's error callback does
//! ([`Status`]), and where the search for the next frame goes on, as
//! libFLAC goes on: from where the header's reading stopped for a damaged
//! header, else from the frame's fourth byte (libFLAC rewinds to the byte
//! after the one it saved when it found the frame's sync code).
//!
//! The arithmetic is libFLAC's, overflow included: where its C would
//! overflow a 32-bit sum (only in a damaged stream; an encoder's never
//! does), it wraps here as it wraps on every machine libFLAC runs on, and the
//! frame is then refused by the bounds check libFLAC makes of its samples.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "sample arithmetic wraps as libFLAC's C does (explicitly, where it can overflow); sizes are bounded by the 65 535-sample block and 8 channels"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "channel numbers below the header's count of at most 8, and sample indices below the block size the buffers were sized to"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "libFLAC's own conversions between its 32- and 64-bit sample types, reproduced"
)]

use crate::bits::{BitReader, Eof, RiceError};
use crate::crc;

/// What libFLAC's error callback says of a frame that did not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Bytes that are not a frame where one was expected, or a frame whose
    /// contents break the format's rules (`LOST_SYNC`).
    LostSync,
    /// A frame header that is damaged (`BAD_HEADER`).
    BadHeader,
    /// A frame whose CRC-16 does not match (`FRAME_CRC_MISMATCH`).
    CrcMismatch,
    /// A frame using what the format reserves (`UNPARSEABLE_STREAM`).
    Unparseable,
    /// A metadata block that breaks its own rules (`BAD_METADATA`).
    BadMetadata,
    /// A frame whose samples do not fit its bit depth (`OUT_OF_BOUNDS`).
    OutOfBounds,
    /// Frames missing between two that decoded (`MISSING_FRAME`).
    MissingFrame,
}

/// How a frame's two channels are coded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelAssignment {
    Independent,
    LeftSide,
    RightSide,
    MidSide,
}

/// What a frame's header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Samples a channel.
    pub block_size: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_assignment: ChannelAssignment,
    pub bits_per_sample: u32,
    /// The frame's first sample's number in the stream.
    pub sample_number: u64,
}

/// What the stream's `STREAMINFO` says that a frame header may leave to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Defaults {
    pub min_block_size: u32,
    pub max_block_size: u32,
    pub sample_rate: u32,
    pub bits_per_sample: u32,
}

/// What became of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decoded {
    /// It decoded; it is `len` bytes long, and its samples are in the
    /// decoder's output.
    Frame { header: Header, len: usize },
    /// It did not; the search for the next frame resumes `resume` bytes
    /// after its start. `past_header`: the header was sound, and the frame
    /// broke after it.
    Error {
        status: Status,
        resume: usize,
        past_header: bool,
    },
    /// The bytes ended first: in its header, or after it.
    NeedMore { in_header: bool },
}

/// Decodes frames, keeping libFLAC's state between them: the fixed block
/// size its frame numbers count in, and the buffers.
#[derive(Clone, Debug, Default)]
pub(crate) struct FrameDecoder {
    /// `STREAMINFO`'s facts, where the stream has one.
    pub defaults: Option<Defaults>,
    /// The block size a fixed-size stream's frame numbers count in, once
    /// known (`fixed_block_size`).
    pub fixed_block_size: u32,
    /// Each channel's samples, after the last frame that decoded.
    pub output: Vec<Vec<i32>>,
    /// A channel decoded wider than 32 bits: a 32-bit stream's side
    /// channel (`side_subframe`).
    side: Vec<i64>,
    side_in_use: bool,
    residual: Vec<i32>,
}

/// A frame stopped short: why, and whether the bytes ran out first.
enum Stop {
    NeedMore,
    /// A broken rule, found after the header: the frame is given up and the
    /// search resumes from its fourth byte.
    Broken(Status),
}

impl From<Eof> for Stop {
    fn from(_: Eof) -> Self {
        Self::NeedMore
    }
}

impl From<RiceError> for Stop {
    fn from(e: RiceError) -> Self {
        match e {
            RiceError::Eof => Self::NeedMore,
            RiceError::Invalid => Self::Broken(Status::LostSync),
        }
    }
}

/// Where libFLAC's search resumes after a frame given up past its header:
/// one byte after the position it saved on finding the sync code, which is
/// after the code's two bytes.
const AFTER_SYNC: usize = 3;

impl FrameDecoder {
    /// Decodes the frame `bytes` begin with -- at its sync code, 0xFFF8 or
    /// 0xFFF9 -- into [`Self::output`].
    pub(crate) fn decode(&mut self, bytes: &[u8]) -> Decoded {
        let mut r = BitReader::new(bytes);
        let header = match self.header(&mut r) {
            Ok(Ok(h)) => h,
            Ok(Err((status, resume))) => {
                return Decoded::Error {
                    status,
                    resume,
                    past_header: false,
                };
            }
            Err(Eof) => return Decoded::NeedMore { in_header: true },
        };
        match self.body(&mut r, &header.0, bytes) {
            Ok(()) => {}
            Err(Stop::NeedMore) => return Decoded::NeedMore { in_header: false },
            Err(Stop::Broken(status)) => {
                return Decoded::Error {
                    status,
                    resume: AFTER_SYNC,
                    past_header: true,
                };
            }
        }
        // A sound frame: the fixed block size is known from here on.
        if header.1 != 0 {
            self.fixed_block_size = header.1;
        }
        Decoded::Frame {
            header: header.0,
            len: r.bytes_consumed(),
        }
    }

    /// The header (`read_frame_header_`), and the fixed block size it makes
    /// known (0 where it makes none); or why it is not one, and where the
    /// search resumes.
    #[allow(
        clippy::type_complexity,
        reason = "header, or status and where to resume"
    )]
    fn header(&self, r: &mut BitReader<'_>) -> Result<Result<(Header, u32), (Status, usize)>, Eof> {
        let b0 = r.read_u32(8)? as u8;
        let b1 = r.read_u32(8)? as u8;
        let mut raw = vec![b0, b1];
        let mut unparseable = b1 & 0x02 != 0;
        // The header's next two bytes; a sync code's first byte among them
        // means the sync code found was none: the search resumes at it.
        for _ in 0..2 {
            let x = r.read_u32(8)? as u8;
            if x == 0xff {
                return Ok(Err((Status::BadHeader, raw.len())));
            }
            raw.push(x);
        }
        let (b2, b3) = (raw[2], raw[3]);
        let defaults = self.defaults;
        let mut block_size = 0u32;
        let mut block_hint = 0;
        match b2 >> 4 {
            0 => unparseable = true,
            1 => block_size = 192,
            x @ 2..=5 => block_size = 576 << (x - 2),
            x @ 6..=7 => block_hint = x,
            x => block_size = 256 << (x - 8),
        }
        let mut sample_rate = 0u32;
        let mut rate_hint = 0;
        match b2 & 0x0f {
            0 => match defaults {
                Some(d) => sample_rate = d.sample_rate,
                None => unparseable = true,
            },
            x @ 1..=11 => {
                sample_rate = [
                    88_200, 176_400, 192_000, 8_000, 16_000, 22_050, 24_000, 32_000, 44_100,
                    48_000, 96_000,
                ][usize::from(x - 1)];
            }
            x @ 12..=14 => rate_hint = x,
            _ => return Ok(Err((Status::BadHeader, 4))),
        }
        let x = b3 >> 4;
        let (channels, channel_assignment) = if x & 8 != 0 {
            let assignment = match x & 7 {
                0 => ChannelAssignment::LeftSide,
                1 => ChannelAssignment::RightSide,
                2 => ChannelAssignment::MidSide,
                _ => {
                    unparseable = true;
                    ChannelAssignment::Independent
                }
            };
            (2, assignment)
        } else {
            (u32::from(x) + 1, ChannelAssignment::Independent)
        };
        let bits_per_sample = match (b3 & 0x0e) >> 1 {
            0 => match defaults {
                Some(d) => d.bits_per_sample,
                None => {
                    unparseable = true;
                    0
                }
            },
            1 => 8,
            2 => 12,
            3 => {
                unparseable = true;
                0
            }
            4 => 16,
            5 => 20,
            6 => 24,
            _ => 32,
        };
        if b3 & 0x01 != 0 {
            unparseable = true;
        }
        // The frame's number: its first sample's, in a stream of variable
        // block size (or, as libFLAC concedes to old encoders, in one whose
        // STREAMINFO says its blocks vary); else its frame number.
        let variable =
            b1 & 0x01 != 0 || defaults.is_some_and(|d| d.min_block_size != d.max_block_size);
        let Some(number) = r.read_utf8(&mut raw, variable)? else {
            // The search resumes at the byte that broke the code.
            return Ok(Err((Status::BadHeader, raw.len() - 1)));
        };
        if block_hint != 0 {
            let mut x = r.read_u32(8)?;
            raw.push(x as u8);
            if block_hint == 7 {
                let low = r.read_u32(8)?;
                raw.push(low as u8);
                x = (x << 8) | low;
            }
            block_size = x + 1;
            if block_size > 65_535 {
                return Ok(Err((Status::BadHeader, raw.len() - 1)));
            }
        }
        if rate_hint != 0 {
            let mut x = r.read_u32(8)?;
            raw.push(x as u8);
            if rate_hint != 12 {
                let low = r.read_u32(8)?;
                raw.push(low as u8);
                x = (x << 8) | low;
            }
            sample_rate = match rate_hint {
                12 => x * 1000,
                13 => x,
                _ => x * 10,
            };
        }
        let crc8 = r.read_u32(8)? as u8;
        let consumed = raw.len() + 1;
        if crc::crc8(&raw) != crc8 {
            return Ok(Err((Status::BadHeader, consumed)));
        }
        let mut next_fixed = 0;
        let sample_number = if variable {
            number
        } else {
            // A frame number, counted in the fixed block size.
            let frame = number;
            if self.fixed_block_size != 0 {
                u64::from(self.fixed_block_size) * frame
            } else if let Some(d) = defaults {
                if d.min_block_size == d.max_block_size {
                    next_fixed = d.max_block_size;
                    u64::from(d.min_block_size) * frame
                } else {
                    unparseable = true;
                    0
                }
            } else if frame == 0 {
                next_fixed = block_size;
                0
            } else {
                // No STREAMINFO and a frame past the first: taken as not
                // the last, short frame.
                u64::from(block_size) * frame
            }
        };
        if unparseable {
            return Ok(Err((Status::Unparseable, consumed)));
        }
        Ok(Ok((
            Header {
                block_size,
                sample_rate,
                channels,
                channel_assignment,
                bits_per_sample,
                sample_number,
            },
            next_fixed,
        )))
    }

    /// The subframes, the padding, the CRC-16 and the samples' check
    /// (the rest of `read_frame_`).
    fn body(&mut self, r: &mut BitReader<'_>, h: &Header, bytes: &[u8]) -> Result<(), Stop> {
        let block = h.block_size as usize;
        let channels = h.channels as usize;
        self.output
            .resize_with(channels.max(self.output.len()), Vec::new);
        for out in self.output.iter_mut().take(channels) {
            out.resize(block, 0);
        }
        if self.residual.len() < block {
            self.residual.resize(block, 0);
        }
        self.side_in_use = false;
        for channel in 0..channels {
            let mut bps = h.bits_per_sample;
            match h.channel_assignment {
                ChannelAssignment::Independent => {}
                ChannelAssignment::LeftSide | ChannelAssignment::MidSide if channel == 1 => {
                    bps += 1
                }
                ChannelAssignment::RightSide if channel == 0 => bps += 1,
                _ => {}
            }
            self.subframe(r, channel, bps, block)?;
        }
        // Zero bits to the byte's end.
        if !r.is_byte_aligned() {
            let pad = r.bits_to_alignment();
            if r.read_u32(pad)? != 0 {
                return Err(Stop::Broken(Status::LostSync));
            }
        }
        // The CRC-16 of every byte before it, sync code to padding.
        let end = r.bytes_consumed();
        let crc = r.read_u32(16)? as u16;
        if crc::crc16(bytes.get(..end).unwrap_or_default()) != crc {
            return Err(Stop::Broken(Status::CrcMismatch));
        }
        self.undo_channel_coding(h);
        // Every sample must fit the frame's bit depth.
        let shift = 32 - h.bits_per_sample.min(32);
        let (lower, upper) = (i32::MIN >> shift, i32::MAX >> shift);
        for out in self.output.iter().take(channels) {
            if out[..block].iter().any(|&s| s < lower || s > upper) {
                return Err(Stop::Broken(Status::OutOfBounds));
            }
        }
        Ok(())
    }

    /// One channel's subframe (`read_subframe_`) into its output, or, 33
    /// bits wide, into `side`.
    fn subframe(
        &mut self,
        r: &mut BitReader<'_>,
        channel: usize,
        mut bps: u32,
        block: usize,
    ) -> Result<(), Stop> {
        let x = r.read_u32(8)?;
        let has_wasted = x & 1 != 0;
        let kind = x & 0xfe;
        let mut wasted = 0;
        if has_wasted {
            wasted = r.read_unary()?.wrapping_add(1);
            if wasted >= bps {
                return Err(Stop::Broken(Status::LostSync));
            }
            bps -= wasted;
        }
        if kind & 0x80 != 0 {
            return Err(Stop::Broken(Status::LostSync));
        }
        match kind {
            0 => self.constant(r, channel, bps, block)?,
            2 => self.verbatim(r, channel, bps, block)?,
            k if k < 16 => return Err(Stop::Broken(Status::Unparseable)),
            k if k <= 24 => {
                let order = ((k >> 1) & 7) as usize;
                if block <= order {
                    return Err(Stop::Broken(Status::LostSync));
                }
                self.fixed(r, channel, bps, order, block)?;
            }
            k if k < 64 => return Err(Stop::Broken(Status::Unparseable)),
            k => {
                let order = (((k >> 1) & 31) + 1) as usize;
                if block <= order {
                    return Err(Stop::Broken(Status::LostSync));
                }
                self.lpc(r, channel, bps, order, block)?;
            }
        }
        if has_wasted {
            if bps + wasted < 33 {
                for v in &mut self.output[channel][..block] {
                    *v = (*v as u32).wrapping_shl(wasted) as i32;
                }
            } else {
                // A 33-bit side channel with wasted bits was decoded at 32
                // bits or fewer: widened as it is shifted.
                self.side_in_use = true;
                self.side.resize(block, 0);
                for (s, &v) in self.side.iter_mut().zip(&self.output[channel][..block]) {
                    *s = ((i64::from(v) as u64).wrapping_shl(wasted)) as i64;
                }
            }
        }
        Ok(())
    }

    fn constant(
        &mut self,
        r: &mut BitReader<'_>,
        channel: usize,
        bps: u32,
        block: usize,
    ) -> Result<(), Stop> {
        let value = r.read_i64(bps)?;
        if bps <= 32 {
            self.output[channel][..block].fill(value as i32);
        } else {
            self.side_in_use = true;
            self.side.clear();
            self.side.resize(block, value);
        }
        Ok(())
    }

    fn verbatim(
        &mut self,
        r: &mut BitReader<'_>,
        channel: usize,
        bps: u32,
        block: usize,
    ) -> Result<(), Stop> {
        if bps < 33 {
            for v in &mut self.output[channel][..block] {
                *v = r.read_i32(bps)?;
            }
        } else {
            self.side_in_use = true;
            self.side.resize(block, 0);
            for v in &mut self.side[..block] {
                *v = r.read_i64(bps)?;
            }
        }
        Ok(())
    }

    /// Warm-up samples, `order` of them, `bps` bits each.
    fn warmup(r: &mut BitReader<'_>, bps: u32, order: usize) -> Result<[i64; 32], Eof> {
        let mut warmup = [0i64; 32];
        for w in warmup.iter_mut().take(order) {
            *w = r.read_i64(bps)?;
        }
        Ok(warmup)
    }

    fn fixed(
        &mut self,
        r: &mut BitReader<'_>,
        channel: usize,
        bps: u32,
        order: usize,
        block: usize,
    ) -> Result<(), Stop> {
        let warmup = Self::warmup(r, bps, order)?;
        self.residual_block(r, order, block)?;
        let n = block - order;
        if bps < 33 {
            let out = &mut self.output[channel];
            for (o, &w) in out.iter_mut().zip(&warmup[..order]) {
                *o = w as i32;
            }
            restore_fixed(
                &self.residual[..n],
                order,
                &mut out[..block],
                bps as usize + order <= 32,
            );
        } else {
            self.side_in_use = true;
            self.side.resize(block, 0);
            self.side[..order].copy_from_slice(&warmup[..order]);
            restore_fixed_33(&self.residual[..n], order, &mut self.side[..block]);
        }
        Ok(())
    }

    fn lpc(
        &mut self,
        r: &mut BitReader<'_>,
        channel: usize,
        bps: u32,
        order: usize,
        block: usize,
    ) -> Result<(), Stop> {
        let warmup = Self::warmup(r, bps, order)?;
        let precision = r.read_u32(4)?;
        if precision == 15 {
            return Err(Stop::Broken(Status::LostSync));
        }
        let precision = precision + 1;
        let shift = r.read_i32(5)?;
        if shift < 0 {
            return Err(Stop::Broken(Status::LostSync));
        }
        let mut coeffs = [0i32; 32];
        for c in coeffs.iter_mut().take(order) {
            *c = r.read_i32(precision)?;
        }
        let coeffs = &coeffs[..order];
        self.residual_block(r, order, block)?;
        let n = block - order;
        if bps <= 32 {
            let out = &mut self.output[channel];
            for (o, &w) in out.iter_mut().zip(&warmup[..order]) {
                *o = w as i32;
            }
            let narrow = max_residual_bps(bps, coeffs, shift) <= 32
                && max_prediction_before_shift_bps(bps, coeffs) <= 32;
            restore_lpc(
                &self.residual[..n],
                coeffs,
                shift as u32,
                &mut out[..block],
                narrow,
            );
        } else {
            self.side_in_use = true;
            self.side.resize(block, 0);
            self.side[..order].copy_from_slice(&warmup[..order]);
            restore_lpc_33(
                &self.residual[..n],
                coeffs,
                shift as u32,
                &mut self.side[..block],
            );
        }
        Ok(())
    }

    /// The residual's entropy coding and partitions
    /// (`read_residual_partitioned_rice_`), into `self.residual`.
    fn residual_block(
        &mut self,
        r: &mut BitReader<'_>,
        order: usize,
        block: usize,
    ) -> Result<(), Stop> {
        let method = r.read_u32(2)?;
        if method > 1 {
            return Err(Stop::Broken(Status::Unparseable));
        }
        let extended = method == 1;
        let partition_order = r.read_u32(4)?;
        if (block >> partition_order) < order || !block.is_multiple_of(1usize << partition_order) {
            return Err(Stop::Broken(Status::LostSync));
        }
        let partitions = 1usize << partition_order;
        let per = block >> partition_order;
        let (plen, escape) = if extended { (5, 31) } else { (4, 15) };
        let mut at = 0usize;
        for partition in 0..partitions {
            let k = r.read_u32(plen)?;
            let count = if partition == 0 { per - order } else { per };
            let out = &mut self.residual[at..at + count];
            if k < escape {
                r.read_rice_block(out, k)?;
            } else {
                let raw_bits = r.read_u32(5)?;
                if raw_bits == 0 {
                    out.fill(0);
                } else {
                    for v in out.iter_mut() {
                        *v = r.read_i32(raw_bits)?;
                    }
                }
            }
            at += count;
        }
        Ok(())
    }

    /// Left/right from the coded pair (`undo_channel_coding`).
    fn undo_channel_coding(&mut self, h: &Header) {
        if h.channel_assignment == ChannelAssignment::Independent {
            return;
        }
        let block = h.block_size as usize;
        let side = self.side_in_use;
        // A coded pair is always two channels.
        let (first, rest) = self.output.split_at_mut(1);
        let (left, right) = (&mut first[0], &mut rest[0]);
        match h.channel_assignment {
            ChannelAssignment::Independent => {}
            ChannelAssignment::LeftSide => {
                for i in 0..block {
                    right[i] = if side {
                        i64::from(left[i]).wrapping_sub(self.side[i]) as i32
                    } else {
                        left[i].wrapping_sub(right[i])
                    };
                }
            }
            ChannelAssignment::RightSide => {
                for i in 0..block {
                    left[i] = if side {
                        i64::from(right[i]).wrapping_add(self.side[i]) as i32
                    } else {
                        left[i].wrapping_add(right[i])
                    };
                }
            }
            ChannelAssignment::MidSide => {
                for i in 0..block {
                    if side {
                        let s = self.side[i];
                        let mid = ((i64::from(left[i]) as u64) << 1) as i64 | (s & 1);
                        left[i] = (mid.wrapping_add(s) >> 1) as i32;
                        right[i] = (mid.wrapping_sub(s) >> 1) as i32;
                    } else {
                        let s = right[i];
                        let mid = ((left[i] as u32) << 1) as i32 | (s & 1);
                        left[i] = mid.wrapping_add(s) >> 1;
                        right[i] = mid.wrapping_sub(s) >> 1;
                    }
                }
            }
        }
    }
}

/// A fixed predictor's samples (`FLAC__fixed_restore_signal`, and its
/// `_wide` form where 32 bits might not hold the sums -- the same samples,
/// summed in 64 bits and cut to 32). `data` holds the warm-up samples
/// first.
fn restore_fixed(residual: &[i32], order: usize, data: &mut [i32], narrow: bool) {
    for (i, &e) in residual.iter().enumerate() {
        let n = i + order;
        let d = |k: usize| data[n - k];
        data[n] = if narrow {
            match order {
                0 => e,
                1 => e.wrapping_add(d(1)),
                2 => e.wrapping_add(d(1).wrapping_mul(2)).wrapping_sub(d(2)),
                3 => e
                    .wrapping_add(d(1).wrapping_mul(3))
                    .wrapping_sub(d(2).wrapping_mul(3))
                    .wrapping_add(d(3)),
                _ => e
                    .wrapping_add(d(1).wrapping_mul(4))
                    .wrapping_sub(d(2).wrapping_mul(6))
                    .wrapping_add(d(3).wrapping_mul(4))
                    .wrapping_sub(d(4)),
            }
        } else {
            let d = |k: usize| i64::from(data[n - k]);
            let e = i64::from(e);
            (match order {
                0 => e,
                1 => e + d(1),
                2 => e + 2 * d(1) - d(2),
                3 => e + 3 * d(1) - 3 * d(2) + d(3),
                _ => e + 4 * d(1) - 6 * d(2) + 4 * d(3) - d(4),
            }) as i32
        };
    }
}

/// [`restore_fixed`] for a 33-bit side channel
/// (`FLAC__fixed_restore_signal_wide_33bit`).
fn restore_fixed_33(residual: &[i32], order: usize, data: &mut [i64]) {
    for (i, &e) in residual.iter().enumerate() {
        let n = i + order;
        let d = |k: usize| data[n - k];
        let e = i64::from(e);
        data[n] = match order {
            0 => e,
            1 => e.wrapping_add(d(1)),
            2 => e.wrapping_add(d(1).wrapping_mul(2)).wrapping_sub(d(2)),
            3 => e
                .wrapping_add(d(1).wrapping_mul(3))
                .wrapping_sub(d(2).wrapping_mul(3))
                .wrapping_add(d(3)),
            _ => e
                .wrapping_add(d(1).wrapping_mul(4))
                .wrapping_sub(d(2).wrapping_mul(6))
                .wrapping_add(d(3).wrapping_mul(4))
                .wrapping_sub(d(4)),
        };
    }
}

/// A linear predictor's samples (`FLAC__lpc_restore_signal`, summing in 32
/// bits, where `narrow`; else `_wide`, in 64). `data` holds the warm-up
/// samples first.
fn restore_lpc(residual: &[i32], coeffs: &[i32], shift: u32, data: &mut [i32], narrow: bool) {
    let order = coeffs.len();
    for (i, &e) in residual.iter().enumerate() {
        let n = i + order;
        let history = &data[i..n];
        data[n] = if narrow {
            let mut sum = 0i32;
            for (c, &h) in coeffs.iter().zip(history.iter().rev()) {
                sum = sum.wrapping_add(c.wrapping_mul(h));
            }
            e.wrapping_add(sum >> shift)
        } else {
            let mut sum = 0i64;
            for (&c, &h) in coeffs.iter().zip(history.iter().rev()) {
                sum = sum.wrapping_add(i64::from(c) * i64::from(h));
            }
            (i64::from(e).wrapping_add(sum >> shift)) as i32
        };
    }
}

/// [`restore_lpc`] for a 33-bit side channel
/// (`FLAC__lpc_restore_signal_wide_33bit`).
fn restore_lpc_33(residual: &[i32], coeffs: &[i32], shift: u32, data: &mut [i64]) {
    let order = coeffs.len();
    for (i, &e) in residual.iter().enumerate() {
        let n = i + order;
        let mut sum = 0i64;
        for (&c, &h) in coeffs.iter().zip(data[i..n].iter().rev()) {
            sum = sum.wrapping_add(i64::from(c).wrapping_mul(h));
        }
        data[n] = i64::from(e).wrapping_add(sum >> shift);
    }
}

/// libFLAC's `FLAC__bitmath_silog2`: the bits a signed number needs.
fn silog2(v: i64) -> u32 {
    match v {
        0 => 0,
        -1 => 2,
        _ => {
            let v = if v < 0 { -(v + 1) } else { v };
            63 - v.leading_zeros() + 2
        }
    }
}

/// The largest prediction before its shift, in absolute value: a sample's
/// largest times the coefficients' absolute sum
/// (`FLAC__lpc_max_prediction_value_before_shift`).
fn max_prediction_before_shift(bps: u32, coeffs: &[i32]) -> u64 {
    let max_sample = 1u64 << (bps - 1);
    let sum = coeffs
        .iter()
        .fold(0u32, |s, &c| s.wrapping_add(c.unsigned_abs()));
    max_sample.wrapping_mul(u64::from(sum))
}

/// `FLAC__lpc_max_prediction_before_shift_bps`.
fn max_prediction_before_shift_bps(bps: u32, coeffs: &[i32]) -> u32 {
    silog2(max_prediction_before_shift(bps, coeffs) as i64)
}

/// `FLAC__lpc_max_residual_bps`.
fn max_residual_bps(bps: u32, coeffs: &[i32], shift: i32) -> u32 {
    let max_sample = 1u64 << (bps - 1);
    let before = max_prediction_before_shift(bps, coeffs) as i64;
    let after = (-((-before) >> shift)) as u64;
    silog2(max_sample.wrapping_add(after) as i64)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]
mod tests {
    use super::*;

    #[test]
    fn silog2_is_libflacs() {
        for (v, bits) in [
            (-10, 5),
            (-9, 5),
            (-8, 4),
            (-5, 4),
            (-4, 3),
            (-2, 2),
            (-1, 2),
            (0, 0),
            (1, 2),
            (2, 3),
            (4, 4),
            (7, 4),
            (8, 5),
            (10, 5),
        ] {
            assert_eq!(silog2(v), bits, "{v}");
        }
    }

    #[test]
    fn fixed_predictors_restore_their_signal() {
        // x = 1, 4, 9, 16, 25: second differences are 2.
        let mut data = [1, 4, 0, 0, 0];
        restore_fixed(&[2, 2, 2], 2, &mut data, true);
        assert_eq!(data, [1, 4, 9, 16, 25]);
        let mut wide = [1, 4, 0, 0, 0];
        restore_fixed(&[2, 2, 2], 2, &mut wide, false);
        assert_eq!(wide, data);
    }

    #[test]
    fn mid_side_is_undone_with_the_side_channels_low_bit() {
        // Left 5, right 2: mid (5 + 2) >> 1 = 3, side 3.
        let mut d = FrameDecoder {
            output: vec![vec![3], vec![3]],
            ..FrameDecoder::default()
        };
        let h = Header {
            block_size: 1,
            sample_rate: 44_100,
            channels: 2,
            channel_assignment: ChannelAssignment::MidSide,
            bits_per_sample: 16,
            sample_number: 0,
        };
        d.undo_channel_coding(&h);
        assert_eq!(d.output, vec![vec![5], vec![2]]);
    }
}
