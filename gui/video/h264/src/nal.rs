//! A packet split into NAL units, as FFmpeg's `ff_h2645_packet_split`
//! splits it (`h2645_parse.c`, with `get_nalsize` from `h2645_parse.h`).
//!
//! A packet carries its NAL units either after start codes (`00 00 01`,
//! Annex B, as in a `.264` file or a transport stream) or each after its
//! length (MP4's and Matroska's "NAL format", FFmpeg's NALFF). Each unit's
//! bytes are unescaped -- an encoder writes `00 00 03` wherever two zero
//! bytes would otherwise be followed by a byte under 4, and the `03` goes --
//! into one buffer for the whole packet, unit after unit, as FFmpeg's
//! `H2645RBSP` holds them.
//!
//! # Why one buffer, laid out as FFmpeg's
//!
//! What a read finds past the end of a unit is part of what FFmpeg decodes
//! from a damaged one (`bits` says how far reads go). FFmpeg writes each
//! unit at the running sum of the bytes the units before it took from the
//! packet, and pads each with 64 zeros -- which the next unit, written
//! straight after, mostly overwrites. So past the end of a unit FFmpeg reads
//! the next unit's bytes, and past the packet's last unit, its padding and
//! then whatever an earlier, longer packet left in the buffer, which is
//! kept from packet to packet. [`Splitter`] keeps its buffer the same way,
//! so those reads find what FFmpeg's would.

#![allow(
    clippy::indexing_slicing,
    reason = "every index is checked against the buffer it reads: positions under its length \
              (`i + 2 < length`, `si < length`), or regions sized by `alloc` to hold the packet"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes are a packet's, under i32::MAX by the checks FFmpeg makes, plus 64 bytes \
              of padding and a margin of 256 KiB"
)]

use crate::Error;
use crate::bits::Bits;

/// `H264_NAL_SLICE`: a coded slice of a picture that is not an IDR.
pub(crate) const NAL_SLICE: u32 = 1;
/// `H264_NAL_DPA` to `H264_NAL_DPC`: data partitions, which FFmpeg does not
/// decode.
pub(crate) const NAL_DPA: u32 = 2;
pub(crate) const NAL_DPB: u32 = 3;
pub(crate) const NAL_DPC: u32 = 4;
/// `H264_NAL_IDR_SLICE`: a slice of an instantaneous decoding refresh.
pub(crate) const NAL_IDR_SLICE: u32 = 5;
/// `H264_NAL_SEI`: supplemental enhancement information.
pub(crate) const NAL_SEI: u32 = 6;
/// `H264_NAL_SPS`: a sequence parameter set.
pub(crate) const NAL_SPS: u32 = 7;
/// `H264_NAL_PPS`: a picture parameter set.
pub(crate) const NAL_PPS: u32 = 8;
/// `H264_NAL_AUD`: an access unit delimiter.
pub(crate) const NAL_AUD: u32 = 9;
/// `H264_NAL_END_SEQUENCE`.
pub(crate) const NAL_END_SEQUENCE: u32 = 10;
/// `H264_NAL_END_STREAM`.
pub(crate) const NAL_END_STREAM: u32 = 11;
/// `H264_NAL_FILLER_DATA`.
pub(crate) const NAL_FILLER_DATA: u32 = 12;
/// `H264_NAL_SPS_EXT`: a sequence parameter set extension.
pub(crate) const NAL_SPS_EXT: u32 = 13;
/// `H264_NAL_AUXILIARY_SLICE`: a slice of an auxiliary (alpha) picture.
pub(crate) const NAL_AUXILIARY_SLICE: u32 = 19;

/// The zeros after every buffer FFmpeg reads (`AV_INPUT_BUFFER_PADDING_SIZE`).
pub(crate) const PADDING: usize = 64;

/// Room FFmpeg keeps past a packet's units when not asked for small
/// padding (`MAX_MBPAIR_SIZE`).
const MAX_MBPAIR_SIZE: usize = 256 * 1024;

/// How a packet's units are laid out, and how they are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Flags {
    /// Each unit after its length (`H2645_FLAG_IS_NALFF`), else after a
    /// start code.
    pub(crate) nalff: bool,
    /// A unit with nothing to unescape is read where it is in the packet,
    /// and no room is kept past the units (`H2645_FLAG_SMALL_PADDING`); what
    /// a parameter set read from a container's configuration is split with.
    pub(crate) small_padding: bool,
}

/// Where a unit's unescaped bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// In the splitter's buffer, from this byte.
    Rbsp(usize),
    /// In the packet itself, from this byte: a unit with nothing to
    /// unescape, split with small padding.
    Packet(usize),
}

/// One NAL unit of a packet (`H2645NAL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Nal {
    /// Where its unescaped bytes start, its header byte first.
    pub(crate) data: Place,
    /// How many unescaped bytes it has (`size`).
    pub(crate) size: usize,
    /// How many bits of them are syntax: without trailing zero bytes, the
    /// stop bit and the zeros after it (`size_bits`).
    pub(crate) size_bits: usize,
    /// Where it starts in the packet, its header byte first (`raw_data`).
    pub(crate) raw_start: usize,
    /// How many of the packet's bytes it took (`raw_size`).
    pub(crate) raw_size: usize,
    /// `nal_ref_idc`: 0 for a unit no picture refers to.
    pub(crate) ref_idc: u32,
    /// `nal_unit_type`.
    pub(crate) kind: u32,
}

/// What splits packets into units, and holds the unescaped bytes of the
/// last one split (`H2645Packet`).
#[derive(Debug, Default)]
pub(crate) struct Splitter {
    /// The units of the last packet split.
    nals: Vec<Nal>,
    /// Their unescaped bytes, laid out as FFmpeg lays them out
    /// (`rbsp_buffer`), kept from packet to packet.
    rbsp: Vec<u8>,
}

impl Splitter {
    /// The units of the last packet split.
    pub(crate) fn nals(&self) -> &[Nal] {
        &self.nals
    }

    /// `nal`'s unescaped bytes, from its header byte to the end of the
    /// buffer they are in: the unit's `size` bytes, then whatever FFmpeg
    /// would read past them. `packet` is the packet `nal` was split from.
    pub(crate) fn data<'s>(&'s self, packet: &'s [u8], nal: &Nal) -> &'s [u8] {
        match nal.data {
            Place::Rbsp(at) => self.rbsp.get(at..).unwrap_or(&[]),
            Place::Packet(at) => packet.get(at..).unwrap_or(&[]),
        }
    }

    /// A reader of `nal`'s syntax, past its header byte, as `nal->gb` is
    /// left once the header is read.
    pub(crate) fn reader<'s>(&'s self, packet: &'s [u8], nal: &Nal) -> Option<Bits<'s>> {
        let mut gb = Bits::new(self.data(packet, nal), nal.size_bits)?;
        gb.skip(8);
        Some(gb)
    }

    /// Split `packet` into its units (`ff_h2645_packet_split` for H.264),
    /// each length `nal_length_size` bytes long when `flags.nalff`.
    ///
    /// A unit whose header is damaged, or which is nothing but zeros, is
    /// left out, as FFmpeg leaves it out. A length running past the packet
    /// is an error for the whole packet, as is a packet with no start code
    /// in it at all.
    pub(crate) fn split(
        &mut self,
        packet: &[u8],
        nal_length_size: usize,
        flags: Flags,
    ) -> Result<(), Error> {
        let length = packet.len();
        let mut next_avc = if flags.nalff { 0 } else { length };
        let padding = if flags.small_padding {
            0
        } else {
            MAX_MBPAIR_SIZE
        };
        self.alloc(length + padding)?;

        let mut rbsp_size = 0usize;
        let mut pos = 0usize;
        self.nals.clear();
        while length - pos >= 4 {
            let mut skip_trailing_zeros = true;
            let extract_length;
            if pos == next_avc {
                extract_length = nal_size(nal_length_size, &packet[pos..])?;
                pos += nal_length_size;
                next_avc = pos + extract_length;
            } else {
                // A start code: what follows it is the unit. Past where the
                // last length said the next unit starts, FFmpeg's search
                // answers a negative distance, which its unsigned skip takes
                // to the end of the packet.
                pos += if pos > next_avc {
                    length - pos
                } else {
                    find_next_start_code(&packet[pos..], next_avc - pos)
                };
                if pos >= length {
                    return if self.nals.is_empty() {
                        // No start code anywhere in the packet.
                        Err(Error::InvalidData)
                    } else {
                        // Bytes after the last unit that start nothing.
                        Ok(())
                    };
                }
                if pos >= next_avc {
                    // Past the unit the last length said: on to the next
                    // length (FFmpeg's unsigned skip of `next_avc - pos`
                    // goes to the end of the packet when that is negative).
                    pos = if pos > next_avc { length } else { next_avc };
                    continue;
                }
                extract_length = (length - pos).min(next_avc - pos);
            }

            let (data, size, consumed) = extract_rbsp(
                &packet[pos..pos + extract_length],
                &mut self.rbsp,
                rbsp_size,
                pos,
                flags.small_padding,
            );
            if let Place::Rbsp(_) = data {
                rbsp_size += consumed;
            }
            let raw_start = pos;
            pos += consumed;

            // A unit followed by a VC-1 style 00 00 01 E0 keeps its trailing
            // zero bytes (FFmpeg's commit 3566042a0).
            if length - pos >= 4 && packet[pos..pos + 4] == [0, 0, 1, 0xE0] {
                skip_trailing_zeros = false;
            }

            let mut nal = Nal {
                data,
                size,
                size_bits: 0,
                raw_start,
                raw_size: consumed,
                ref_idc: 0,
                kind: 0,
            };
            let Some(size_bits) =
                bit_length(self.data(packet, &nal), size, 1, skip_trailing_zeros)?
            else {
                continue;
            };
            if size == 0 || size_bits == 0 {
                continue;
            }
            nal.size_bits = size_bits;
            let Some(mut gb) = Bits::new(self.data(packet, &nal), size_bits) else {
                return Err(Error::InvalidData);
            };
            // forbidden_zero_bit, nal_ref_idc, nal_unit_type: a unit whose
            // first bit is set is skipped.
            if gb.bit() != 0 {
                continue;
            }
            nal.ref_idc = gb.bits(2);
            nal.kind = gb.bits(5);
            self.nals.push(nal);
        }
        Ok(())
    }

    /// Make the buffer hold a packet of `size` bytes and its padding
    /// (`alloc_rbsp_buffer`): kept, its padding zeroed, when it already
    /// does; otherwise replaced by a zeroed one a sixteenth larger.
    fn alloc(&mut self, size: usize) -> Result<(), Error> {
        let min_size = size;
        if size > (i32::MAX as usize) - PADDING {
            return Err(Error::TooLarge);
        }
        let size = size + PADDING;
        if self.rbsp.len() >= size {
            self.rbsp[min_size..min_size + PADDING].fill(0);
            return Ok(());
        }
        let size = (size + size / 16 + 32).min(i32::MAX as usize);
        self.rbsp = vec![0; size];
        Ok(())
    }
}

/// A unit's length before it, `nal_length_size` bytes big-endian, checked
/// against what is left of the packet (`get_nalsize`).
fn nal_size(nal_length_size: usize, buf: &[u8]) -> Result<usize, Error> {
    if nal_length_size >= buf.len() {
        return Err(Error::InvalidData);
    }
    // FFmpeg accumulates in an int: a length with its top bit set is
    // negative, and refused below with the rest.
    #[allow(
        clippy::cast_possible_wrap,
        reason = "FFmpeg's int, wrapped as it wraps"
    )]
    let size = buf[..nal_length_size]
        .iter()
        .fold(0u32, |n, &b| (n << 8) | u32::from(b)) as i32;
    let left = buf.len() - nal_length_size;
    match usize::try_from(size) {
        Ok(size) if size > 0 && size <= left => Ok(size),
        _ => Err(Error::InvalidData),
    }
}

/// How far into `buf` the next unit starts: past the next start code, or
/// `next_avc` (the end of the stretch searched) when there is none
/// (`find_next_start_code`).
fn find_next_start_code(buf: &[u8], next_avc: usize) -> usize {
    if 3 >= next_avc {
        return next_avc;
    }
    let mut i = 0;
    while i + 3 < next_avc {
        if buf.get(i..i + 3) == Some(&[0, 0, 1]) {
            break;
        }
        i += 1;
    }
    i + 3
}

/// Unescape the unit at the start of `src` into `rbsp` from `at`
/// (`ff_h2645_extract_rbsp`): `00 00 03` loses its `03`, and `00 00 01` or
/// `00 00 02` ends the unit. Answers where the unit's bytes are, how many
/// there are, and how many of `src`'s it took. `src_at` is where `src`
/// starts in the packet.
///
/// With `small_padding`, a unit with nothing to unescape stays where it is
/// in the packet. Otherwise it is copied, and followed in `rbsp` by 64
/// zeros.
fn extract_rbsp(
    src: &[u8],
    rbsp: &mut [u8],
    at: usize,
    src_at: usize,
    small_padding: bool,
) -> (Place, usize, usize) {
    let mut length = src.len();
    // The first 00 00 01 or 00 00 03; past the end when there is none, as
    // FFmpeg's scan leaves its index.
    let mut i = 0;
    loop {
        if i + 2 >= length {
            i = length;
            break;
        }
        if src[i] == 0 && src[i + 1] == 0 && (src[i + 2] == 1 || src[i + 2] == 3) {
            if src[i + 2] == 1 {
                // A start code: the unit ended before it.
                length = i;
            }
            break;
        }
        i += 1;
    }
    // Nothing to unescape before the end: with small padding, the unit is
    // read where it is.
    if i >= length.saturating_sub(1) && small_padding {
        return (Place::Packet(src_at), length, length);
    }
    i = i.min(length);

    let dst = &mut rbsp[at..];
    dst[..i].copy_from_slice(&src[..i]);
    let (mut si, mut di) = (i, i);
    let mut stopped = false;
    while si + 2 < length {
        if src[si + 2] > 3 {
            dst[di] = src[si];
            dst[di + 1] = src[si + 1];
            di += 2;
            si += 2;
        } else if src[si] == 0 && src[si + 1] == 0 && src[si + 2] != 0 {
            if src[si + 2] == 3 {
                // An escape.
                dst[di] = 0;
                dst[di + 1] = 0;
                di += 2;
                si += 3;
                continue;
            }
            // 00 00 01 or 00 00 02: the next unit starts here.
            stopped = true;
            break;
        } else {
            dst[di] = src[si];
            di += 1;
            si += 1;
        }
    }
    if !stopped {
        while si < length {
            dst[di] = src[si];
            di += 1;
            si += 1;
        }
    }
    dst[di..di + PADDING].fill(0);
    (Place::Rbsp(at), di, si)
}

/// How many bits of a unit's `size` bytes are syntax (`get_bit_length`):
/// trailing zero bytes off (when `skip_trailing_zeros`), then the stop bit
/// and the zeros after it in the last byte. `Ok(None)` stands for FFmpeg's
/// `AVERROR_INVALIDDATA` for a unit shorter than `min_size`, which the
/// splitter skips; a size too large for an `int` count of bits is an error.
fn bit_length(
    data: &[u8],
    nal_size: usize,
    min_size: usize,
    skip_trailing_zeros: bool,
) -> Result<Option<usize>, Error> {
    let byte = |k: usize| data.get(k).copied().unwrap_or(0);
    let mut size = nal_size;
    while skip_trailing_zeros && size > 0 && byte(size - 1) == 0 {
        size -= 1;
    }
    if size == 0 {
        return Ok(Some(0));
    }
    let mut trailing_padding = 0;
    if size <= min_size {
        if nal_size < min_size {
            return Ok(None);
        }
        size = min_size;
    } else {
        let v = byte(size - 1);
        if v != 0 {
            trailing_padding = v.trailing_zeros() as usize + 1;
        }
    }
    if size > (i32::MAX as usize) / 8 {
        return Err(Error::TooLarge);
    }
    Ok(Some(size * 8 - trailing_padding))
}

#[cfg(test)]
mod tests;
