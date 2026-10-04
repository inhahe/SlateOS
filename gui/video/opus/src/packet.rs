//! An Opus packet's framing (RFC 6716 §3): the TOC byte -- mode, bandwidth,
//! frame size, channels and frame count -- and the frames' lengths, in each
//! of the four codes, with padding and, for a multistream packet's inner
//! streams, self-delimiting lengths.
//!
//! Translated into Rust from libopus 1.5.2's `src/opus.c`
//! (`opus_packet_parse_impl`, `opus_packet_get_samples_per_frame`) and
//! `src/opus_decoder.c` (the `opus_packet_get_*` helpers), copyright
//! Xiph.Org, Skype and the contributors named in its `COPYING`, used under
//! libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is checked against the packet's length first, as libopus's parser checks it"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "lengths below the packet's, which a `&[u8]` bounds; frame counts below 64 and sizes below 1276"
)]

use crate::error::Error;

/// Which codec a frame uses (`MODE_SILK_ONLY`, `MODE_HYBRID`,
/// `MODE_CELT_ONLY`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Linear prediction (speech), up to wideband.
    Silk,
    /// SILK below 8 kHz, CELT above.
    Hybrid,
    /// The transform codec (music, low delay).
    Celt,
}

/// The audio bandwidth a packet codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bandwidth {
    /// 4 kHz (8 kHz sampling).
    Narrowband,
    /// 6 kHz (12 kHz sampling).
    Mediumband,
    /// 8 kHz (16 kHz sampling).
    Wideband,
    /// 12 kHz (24 kHz sampling).
    Superwideband,
    /// 20 kHz (48 kHz sampling).
    Fullband,
}

/// `opus_packet_get_mode`.
pub(crate) const fn mode(toc: u8) -> Mode {
    if toc & 0x80 != 0 {
        Mode::Celt
    } else if toc & 0x60 == 0x60 {
        Mode::Hybrid
    } else {
        Mode::Silk
    }
}

/// `opus_packet_get_bandwidth`.
pub(crate) const fn bandwidth(toc: u8) -> Bandwidth {
    const BY_INDEX: [Bandwidth; 5] = [
        Bandwidth::Narrowband,
        Bandwidth::Mediumband,
        Bandwidth::Wideband,
        Bandwidth::Superwideband,
        Bandwidth::Fullband,
    ];
    let bits = ((toc >> 5) & 0x3) as usize;
    if toc & 0x80 != 0 {
        // CELT: narrowband, then wide-, superwide- and fullband (no
        // mediumband).
        if bits == 0 {
            Bandwidth::Narrowband
        } else {
            BY_INDEX[bits + 1]
        }
    } else if toc & 0x60 == 0x60 {
        if toc & 0x10 != 0 {
            Bandwidth::Fullband
        } else {
            Bandwidth::Superwideband
        }
    } else {
        BY_INDEX[bits]
    }
}

/// `opus_packet_get_nb_channels`: the channels a packet codes.
pub(crate) const fn channels(toc: u8) -> usize {
    if toc & 0x4 != 0 { 2 } else { 1 }
}

/// `opus_packet_get_samples_per_frame`: samples a frame of a packet with
/// this TOC holds, a channel, at `fs`.
pub(crate) const fn samples_per_frame(toc: u8, fs: usize) -> usize {
    if toc & 0x80 != 0 {
        // CELT: 2.5, 5, 10 or 20 ms.
        (fs << ((toc >> 3) & 0x3)) / 400
    } else if toc & 0x60 == 0x60 {
        // Hybrid: 10 or 20 ms.
        if toc & 0x08 != 0 { fs / 50 } else { fs / 100 }
    } else {
        // SILK: 10, 20, 40 or 60 ms.
        let size = (toc >> 3) & 0x3;
        if size == 3 {
            fs * 60 / 1000
        } else {
            (fs << size) / 100
        }
    }
}

/// The frames of a packet.
#[derive(Clone, Debug)]
pub(crate) struct Parsed {
    /// How many frames: 1 to 48.
    pub count: usize,
    /// Each frame's length.
    pub sizes: [usize; 48],
    /// Where the first frame begins.
    pub payload_offset: usize,
    /// The packet's length with its padding: where a self-delimited
    /// packet's successor begins.
    pub packet_offset: usize,
}

/// `parse_size`: a frame length (one byte below 252, else two); the bytes
/// it took.
fn parse_size(data: &[u8]) -> Option<(usize, usize)> {
    match data {
        [] => None,
        [b, ..] if *b < 252 => Some((usize::from(*b), 1)),
        [_] => None,
        [b0, b1, ..] => Some((4 * usize::from(*b1) + usize::from(*b0), 2)),
    }
}

/// `opus_packet_parse_impl`: a packet's frames, or why it is not a valid
/// packet.
pub(crate) fn parse(packet: &[u8], self_delimited: bool) -> Result<Parsed, Error> {
    let Some((&toc, rest)) = packet.split_first() else {
        return Err(Error::InvalidPacket);
    };
    let framesize = samples_per_frame(toc, 48000);
    let mut data = rest;
    let mut len = rest.len();
    let mut sizes = [0usize; 48];
    let mut last_size = len;
    let mut cbr = false;
    let mut pad = 0usize;
    let count;
    match toc & 0x3 {
        // One frame.
        0 => count = 1,
        // Two frames of one length.
        1 => {
            count = 2;
            cbr = true;
            if !self_delimited {
                if len & 1 != 0 {
                    return Err(Error::InvalidPacket);
                }
                last_size = len / 2;
                sizes[0] = last_size;
            }
        }
        // Two frames, the first's length given.
        2 => {
            count = 2;
            let (size, bytes) = parse_size(data).ok_or(Error::InvalidPacket)?;
            len -= bytes;
            if size > len {
                return Err(Error::InvalidPacket);
            }
            sizes[0] = size;
            data = &data[bytes..];
            last_size = len - size;
        }
        // 1 to 48 frames, of one length or each given, with padding.
        _ => {
            let Some((&ch, after)) = data.split_first() else {
                return Err(Error::InvalidPacket);
            };
            data = after;
            count = usize::from(ch & 0x3F);
            if count == 0 || framesize * count > 5760 {
                return Err(Error::InvalidPacket);
            }
            len -= 1;
            // Padding: bytes of 255 add 254 and continue.
            let mut len_signed = len as isize;
            if ch & 0x40 != 0 {
                loop {
                    if len_signed <= 0 {
                        return Err(Error::InvalidPacket);
                    }
                    let Some((&p, after)) = data.split_first() else {
                        return Err(Error::InvalidPacket);
                    };
                    data = after;
                    len_signed -= 1;
                    let tmp = if p == 255 { 254 } else { usize::from(p) };
                    len_signed -= tmp as isize;
                    pad += tmp;
                    if p != 255 {
                        break;
                    }
                }
            }
            if len_signed < 0 {
                return Err(Error::InvalidPacket);
            }
            len = len_signed as usize;
            cbr = ch & 0x80 == 0;
            if !cbr {
                // Each frame's length but the last given.
                let mut last = len as isize;
                for size in sizes.iter_mut().take(count - 1) {
                    let (s, bytes) =
                        parse_size(&data[..len.min(data.len())]).ok_or(Error::InvalidPacket)?;
                    len -= bytes;
                    if s > len {
                        return Err(Error::InvalidPacket);
                    }
                    *size = s;
                    data = &data[bytes..];
                    last -= (bytes + s) as isize;
                }
                if last < 0 {
                    return Err(Error::InvalidPacket);
                }
                last_size = last as usize;
            } else if !self_delimited {
                last_size = len / count;
                if last_size * count != len {
                    return Err(Error::InvalidPacket);
                }
                for size in sizes.iter_mut().take(count - 1) {
                    *size = last_size;
                }
            }
        }
    }
    if self_delimited {
        // The last frame's length given too.
        let (s, bytes) = parse_size(&data[..len.min(data.len())]).ok_or(Error::InvalidPacket)?;
        len -= bytes;
        if s > len {
            return Err(Error::InvalidPacket);
        }
        sizes[count - 1] = s;
        data = &data[bytes..];
        if cbr {
            // One length for every frame.
            if s * count > len {
                return Err(Error::InvalidPacket);
            }
            for size in sizes.iter_mut().take(count - 1) {
                *size = s;
            }
        } else if bytes + s > last_size {
            return Err(Error::InvalidPacket);
        }
    } else {
        // An implicit length could be larger than a frame can be.
        if last_size > 1275 {
            return Err(Error::InvalidPacket);
        }
        sizes[count - 1] = last_size;
    }
    let payload_offset = packet.len() - data.len();
    let frames: usize = sizes[..count].iter().sum();
    Ok(Parsed {
        count,
        sizes,
        payload_offset,
        packet_offset: pad + payload_offset + frames,
    })
}

/// `opus_packet_get_nb_frames`: the frames in a packet.
pub(crate) fn frame_count(packet: &[u8]) -> Result<usize, Error> {
    match packet {
        [] => Err(Error::BadArgument),
        [toc, ..] if toc & 0x3 == 0 => Ok(1),
        [toc, ..] if toc & 0x3 != 3 => Ok(2),
        [_] => Err(Error::InvalidPacket),
        [_, count, ..] => Ok(usize::from(count & 0x3F)),
    }
}

/// The samples a packet decodes to, a channel, at `fs`
/// (`opus_packet_get_nb_samples`): an error for a packet longer than 120 ms
/// or without a frame count.
pub fn packet_samples(packet: &[u8], fs: u32) -> Result<usize, Error> {
    let count = frame_count(packet)?;
    let fs = fs as usize;
    let samples = count * samples_per_frame(packet[0], fs);
    if samples * 25 > fs * 3 {
        Err(Error::InvalidPacket)
    } else {
        Ok(samples)
    }
}

/// `opus_packet_has_lbrr`: whether a packet's first SILK frame carries the
/// low-bitrate redundancy for the packet before -- which decoding it with
/// FEC recovers.
pub fn packet_has_lbrr(packet: &[u8]) -> Result<bool, Error> {
    let Some(&toc) = packet.first() else {
        return Err(Error::InvalidPacket);
    };
    if mode(toc) == Mode::Celt {
        return Ok(false);
    }
    let frame_size = samples_per_frame(toc, 48000);
    let nb_frames = if frame_size > 960 {
        frame_size / 960
    } else {
        1
    };
    let parsed = parse(packet, false)?;
    // libopus reads the byte where the first frame begins whatever the
    // frame's length -- the next frame's, if the first is empty. Past the
    // packet's end it reads beyond its buffer; here there is no byte, and
    // so no redundancy.
    let Some(&first) = packet.get(parsed.payload_offset) else {
        return Ok(false);
    };
    let mut lbrr = (first >> (7 - nb_frames)) & 1 != 0;
    if channels(toc) == 2 {
        lbrr = lbrr || (first >> (6 - 2 * nb_frames)) & 1 != 0;
    }
    Ok(lbrr)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;
    use crate::testutil::{Digest, Xorshift};

    /// The packet functions held to libopus's over 200 000 packets of every
    /// length, a quarter of their bytes 255 (so that padding, two-byte
    /// lengths and code 3's counts come up), each parsed both ways --
    /// self-delimited (a multistream packet's) and not -- and asked its frames,
    /// samples and in-band FEC: every result and every error the same as
    /// `tools/functions.c` digested libopus's to.
    #[test]
    fn packet_functions_agree_with_libopus() {
        let mut rng = Xorshift(0x2545_F491_4F6C_DD1D);
        let mut d = Digest::new();
        let mut data = vec![0u8; 1700];
        for _ in 0..200_000 {
            let len = match rng.below(4) {
                0 => rng.below(4),
                1 => rng.below(16),
                2 => rng.below(300),
                _ => rng.below(1700),
            } as usize;
            for b in &mut data[..len] {
                let r = rng.next();
                *b = if r & 3 == 0 { 0xff } else { (r >> 2) as u8 };
            }
            let p = &data[..len];
            for self_delimited in [false, true] {
                match parse(p, self_delimited) {
                    Ok(parsed) => {
                        d.add(parsed.count as i32);
                        d.add(i32::from(p[0]));
                        for &size in &parsed.sizes[..parsed.count] {
                            d.add(size as i32);
                        }
                        d.add(parsed.payload_offset as i32);
                        d.add(parsed.packet_offset as i32);
                    }
                    Err(e) => d.add(e.code()),
                }
            }
            d.add(frame_count(p).map_or_else(Error::code, |n| n as i32));
            d.add(packet_samples(p, 48000).map_or_else(Error::code, |n| n as i32));
            d.add(packet_samples(p, 8000).map_or_else(Error::code, |n| n as i32));
            if len > 0 {
                d.add(packet_has_lbrr(p).map_or_else(Error::code, i32::from));
            }
        }
        d.check("packets", 1_601_782, 0x1373_c4a2_5bfd_1239)
            .unwrap();
    }

    #[test]
    fn a_toc_tells_mode_bandwidth_and_size() {
        // Config 31: CELT fullband 20 ms; stereo; one frame.
        let toc = (31 << 3) | 0x4;
        assert_eq!(mode(toc), Mode::Celt);
        assert_eq!(bandwidth(toc), Bandwidth::Fullband);
        assert_eq!(samples_per_frame(toc, 48000), 960);
        assert_eq!(channels(toc), 2);
        // Config 16: CELT narrowband 2.5 ms.
        assert_eq!(bandwidth(16 << 3), Bandwidth::Narrowband);
        assert_eq!(samples_per_frame(16 << 3, 48000), 120);
        // Configs 12 and 13: hybrid superwideband 10 and 20 ms; 14 and 15,
        // fullband; config 3: SILK NB 60 ms.
        assert_eq!(mode(13 << 3), Mode::Hybrid);
        assert_eq!(bandwidth(13 << 3), Bandwidth::Superwideband);
        assert_eq!(samples_per_frame(13 << 3, 48000), 960);
        assert_eq!(bandwidth(14 << 3), Bandwidth::Fullband);
        assert_eq!(samples_per_frame(14 << 3, 48000), 480);
        assert_eq!(samples_per_frame(3 << 3, 48000), 2880);
        assert_eq!(bandwidth(5 << 3), Bandwidth::Mediumband);
    }

    #[test]
    fn the_four_codes_parse() {
        // Code 0.
        let p = parse(&[0, 1, 2, 3], false).expect("code 0");
        assert_eq!((p.count, p.sizes[0], p.payload_offset), (1, 3, 1));
        // Code 1: an odd payload cannot be halved.
        assert!(parse(&[1, 1, 2, 3], false).is_err());
        let p = parse(&[1, 1, 2, 3, 4], false).expect("code 1");
        assert_eq!((p.count, p.sizes[0], p.sizes[1]), (2, 2, 2));
        // Code 2: the first frame's length given.
        let p = parse(&[2, 1, 9, 8, 7], false).expect("code 2");
        assert_eq!(
            (p.count, p.sizes[0], p.sizes[1], p.payload_offset),
            (2, 1, 2, 2)
        );
        // Code 3, CBR, three frames of two bytes, with two bytes of padding.
        let p = parse(&[3, 0x43, 2, 1, 1, 2, 2, 3, 3, 0, 0], false).expect("code 3");
        assert_eq!(
            (p.count, p.sizes[0], p.sizes[2], p.payload_offset),
            (3, 2, 2, 3)
        );
        assert_eq!(p.packet_offset, 11);
        // Code 3 with no frames, or more than 120 ms of them.
        assert!(parse(&[3, 0], false).is_err());
        assert!(parse(&[(31 << 3) | 3, 7], false).is_err());
    }

    #[test]
    fn a_long_frame_length_takes_two_bytes() {
        let mut p = vec![2u8, 252, 1];
        p.extend(std::iter::repeat_n(0u8, 256 + 3));
        let parsed = parse(&p, false).expect("parses");
        assert_eq!(parsed.sizes[0], 256);
        assert_eq!(parsed.sizes[1], 3);
    }
}
