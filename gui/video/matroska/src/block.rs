//! A block: the bytes of a `SimpleBlock` or a `BlockGroup`'s `Block` --
//! which track, when relative to its Cluster, some flags, and one frame or
//! several *laced* together (sound codecs' short frames, packed to save each
//! one's header). Read as FFmpeg reads them (`matroska_parse_block`,
//! `matroska_parse_laces`), its checks included.

use std::ops::Range;

use crate::Error;
use crate::ebml::{svint_in, vint_in};

/// A block's header and where its frames lie in its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub track: u64,
    /// The timestamp relative to the Cluster's, in the segment's ticks.
    pub relative: i16,
    /// `SimpleBlock`'s flags: 0x80 a key frame, 0x08 invisible, 0x01
    /// discardable (a `Block`'s carry only the middle one).
    pub flags: u8,
    /// Each frame's bytes, in order.
    pub frames: Vec<Range<usize>>,
}

/// The most bytes a block's header takes: a track number of up to 8 bytes,
/// the timestamp and the flags.
pub(crate) const HEADER_MAX: u64 = 11;

/// A block's header alone: its track, timestamp and flags.
///
/// # Errors
///
/// When the bytes are too few for it, or the track number is damaged.
pub(crate) fn header(data: &[u8]) -> Result<(u64, i16, u8), Error> {
    let (n, track) = vint_in(data).ok_or(Error::Invalid("a block's track number"))?;
    let [t0, t1, flags, ..] = *data.get(n..).unwrap_or_default() else {
        return Err(Error::Invalid("a block shorter than its header"));
    };
    Ok((track, i16::from_be_bytes([t0, t1]), flags))
}

/// Parse a block's bytes.
///
/// # Errors
///
/// When the header is cut short or damaged, or the lacing does not fit the
/// bytes -- each where FFmpeg refuses the block.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "every offset and size is checked against the bytes left before it is used, so each is at most the block's length (256 MiB at most) and no sum overflows; `laces` is at least 1"
)]
pub(crate) fn parse(data: &[u8]) -> Result<Block, Error> {
    let bad = |why| Error::Invalid(why);
    let (track, relative, flags) = header(data)?;
    let start = vint_in(data).map_or(0, |(n, _)| n) + 3;
    let size = data.len() - start;
    let lacing = (flags >> 1) & 3;
    if lacing == 0 {
        return Ok(Block {
            track,
            relative,
            flags,
            frames: core::iter::once(start..data.len()).collect(),
        });
    }
    let Some(&count) = data.get(start) else {
        return Err(bad("a laced block with no frame count"));
    };
    let laces = usize::from(count) + 1;
    // The bytes after the count, and the sizes of all but the last frame.
    let mut at = start + 1;
    let mut left = size - 1;
    let mut sizes = Vec::with_capacity(laces);
    match lacing {
        // Xiph: each size a run of 255s and a last byte under 255.
        1 => {
            let mut total = 0usize;
            for _ in 0..laces - 1 {
                let mut lace = 0usize;
                loop {
                    if left <= total {
                        return Err(bad("a Xiph-laced block's sizes"));
                    }
                    let b = data
                        .get(at)
                        .copied()
                        .ok_or(bad("a Xiph-laced block's sizes"))?;
                    total += usize::from(b);
                    lace += usize::from(b);
                    at += 1;
                    left -= 1;
                    if b != 0xff {
                        break;
                    }
                }
                sizes.push(lace);
            }
            if left < total {
                return Err(bad("a Xiph-laced block's sizes"));
            }
            sizes.push(left - total);
        }
        // Fixed: the bytes shared out evenly.
        2 => {
            if !left.is_multiple_of(laces) {
                return Err(bad("a fixed-laced block not a multiple of its count"));
            }
            sizes.resize(laces, left / laces);
        }
        // EBML: the first size, then each next as a signed difference from
        // the one before, the last what is left. With a single frame the
        // first size is read and then replaced by what is left, as FFmpeg
        // does.
        _ => {
            let why = "an EBML-laced block's sizes";
            let int_max = usize::try_from(i32::MAX.unsigned_abs()).unwrap_or(usize::MAX);
            let tail = data.get(at..).unwrap_or_default();
            let (n0, first) = vint_in(tail).ok_or(bad(why))?;
            let first = usize::try_from(first)
                .ok()
                .filter(|&s| s <= int_max)
                .ok_or(bad(why))?;
            sizes.resize(laces, 0);
            let mut offset = n0;
            let mut total = first;
            let mut previous = first;
            if let Some(s) = sizes.first_mut() {
                *s = first;
            }
            for s in sizes.iter_mut().take(laces - 1).skip(1) {
                let (n, delta) = tail.get(offset..).and_then(svint_in).ok_or(bad(why))?;
                let next = i64::try_from(previous)
                    .ok()
                    .and_then(|p| usize::try_from(p + delta).ok())
                    .filter(|&s| s <= int_max)
                    .ok_or(bad(why))?;
                *s = next;
                previous = next;
                total += next;
                offset += n;
            }
            at += offset;
            left = left.checked_sub(offset).ok_or(bad(why))?;
            if left < total {
                return Err(bad(why));
            }
            if let Some(s) = sizes.last_mut() {
                *s = left - total;
            }
        }
    }
    let mut frames = Vec::with_capacity(laces);
    for s in sizes {
        frames.push(at..at + s);
        at += s;
    }
    Ok(Block {
        track,
        relative,
        flags,
        frames,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    /// A block of track 1 at +5, with `flags` and `body` after the header.
    fn block(flags: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![0x81, 0x00, 0x05, flags];
        v.extend_from_slice(body);
        v
    }

    fn frames(data: &[u8]) -> Vec<Vec<u8>> {
        parse(data)
            .unwrap()
            .frames
            .iter()
            .map(|r| data[r.clone()].to_vec())
            .collect()
    }

    #[test]
    fn an_unlaced_block_is_one_frame() {
        let b = parse(&block(0x80, b"abc")).unwrap();
        assert_eq!((b.track, b.relative, b.flags), (1, 5, 0x80));
        assert_eq!(frames(&block(0x80, b"abc")), [b"abc".to_vec()]);
        // Empty is still a frame here; whether it is a packet is the
        // demuxer's call.
        assert_eq!(frames(&block(0, b"")), [Vec::<u8>::new()]);
    }

    #[test]
    fn xiph_lacing_reads_runs_of_255() {
        // Three frames: 2 bytes, 256 bytes (255 + 1), and the rest (3).
        let mut body = vec![2, 2, 0xff, 1];
        body.extend_from_slice(&[7; 2]);
        body.extend_from_slice(&[8; 256]);
        body.extend_from_slice(&[9; 3]);
        let f = frames(&block(0x02, &body));
        assert_eq!(f.iter().map(Vec::len).collect::<Vec<_>>(), [2, 256, 3]);
        assert!(f[2].iter().all(|&b| b == 9));
        // Sizes claiming more than the block holds.
        assert!(parse(&block(0x02, &[1, 9, 1, 2])).is_err());
    }

    #[test]
    fn fixed_lacing_shares_the_bytes_out() {
        assert_eq!(
            frames(&block(0x04, &[2, 1, 2, 3, 4, 5, 6])),
            [vec![1, 2], vec![3, 4], vec![5, 6]]
        );
        assert!(
            parse(&block(0x04, &[2, 1, 2, 3, 4])).is_err(),
            "4 bytes in 3"
        );
    }

    #[test]
    fn ebml_lacing_reads_differences() {
        // Three frames: 3 bytes, 3 + 1 = 4 (0xc0 is +1), and the rest (2).
        let mut body = vec![2, 0x83, 0xc0];
        body.extend_from_slice(&[1; 3]);
        body.extend_from_slice(&[2; 4]);
        body.extend_from_slice(&[3; 2]);
        let f = frames(&block(0x06, &body));
        assert_eq!(f.iter().map(Vec::len).collect::<Vec<_>>(), [3, 4, 2]);
        // A difference taking a size below zero.
        assert!(parse(&block(0x06, &[2, 0x81, 0x80, 0, 0, 0])).is_err());
        // One frame: FFmpeg reads its size (1), then makes it what is left
        // less that size (3 - 1), and the last byte belongs to nothing.
        assert_eq!(frames(&block(0x06, &[0, 0x81, 5, 6, 7])), [vec![5, 6]]);
    }

    #[test]
    fn a_damaged_block_is_an_error() {
        assert!(parse(&[]).is_err());
        assert!(parse(&[0x81, 0x00]).is_err(), "no flags");
        assert!(parse(&[0x00, 0, 0, 0]).is_err(), "no track number");
        assert!(parse(&block(0x02, &[])).is_err(), "laced with no count");
    }
}
