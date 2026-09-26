//! AVI: RIFF chunks, each a four-letter id, a little-endian length and a
//! body padded to an even length; `LIST` chunks hold more chunks.
//!
//! The facts are in the `hdrl` list at the start: the main header (`avih`),
//! then a `strl` list per stream, each a stream header (`strh`), a format
//! (`strf`: a bitmap header for a picture, a wave format for sound) and a
//! name (`strn`). A file past 1 GiB (OpenDML) counts its frames again in
//! `odml`/`dmlh`, because the main header's count covers only the first
//! gigabyte.

use std::io::{self, Read, Seek};

use crate::{Codec, Kind, MAX_CHILDREN, Probe, Track, bytes, le16, le32, read_at, seconds, text};

/// The most of `hdrl` that is read: a few hundred bytes a stream, and the
/// index of an OpenDML file's first gigabyte, which some writers put there.
const MAX_HEADER_LIST: usize = 1024 * 1024;

/// The chunks in `b`, in order, each its id and body, as far as they are
/// chunks.
fn chunks(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0_usize;
    // Every chunk is at least its eight-byte header, so `at` moves each time.
    while out.len() < MAX_CHILDREN {
        let (Some(id), Some(size)) = (bytes::<4>(b, at), le32(b, at.saturating_add(4))) else {
            break;
        };
        let Some(body) = at.checked_add(8) else {
            break;
        };
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        let end = body.saturating_add(size).min(b.len());
        let Some(value) = b.get(body..end) else {
            break;
        };
        out.push((id, value));
        // Word-aligned: an odd length is followed by a pad byte.
        at = end.saturating_add(size & 1);
    }
    out
}

/// The list inside a `LIST` chunk's body, if it is one of `kind`.
fn list<'a>(body: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    (body.get(..4) == Some(kind.as_slice()))
        .then(|| body.get(4..))
        .flatten()
}

pub(crate) fn probe<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Probe> {
    let mut probe = Probe::default();
    // The header list: the first `LIST hdrl` among the top-level chunks.
    let mut at = 12_u64;
    let mut header = None;
    for _ in 0..MAX_CHILDREN {
        let head = read_at(r, at, 12, len)?;
        let (Some(id), Some(size)) = (bytes::<4>(&head, 0), le32(&head, 4)) else {
            break;
        };
        let body = at.saturating_add(8);
        if &id == b"LIST" && head.get(8..12) == Some(b"hdrl") {
            let size = usize::try_from(size).unwrap_or(usize::MAX);
            header = Some(read_at(r, body, size.min(MAX_HEADER_LIST), len)?);
            break;
        }
        at = body
            .saturating_add(u64::from(size))
            .saturating_add(u64::from(size & 1));
        if at >= len {
            break;
        }
    }
    let Some(header) = header else {
        return Ok(probe);
    };
    let Some(header) = list(&header, b"hdrl") else {
        return Ok(probe);
    };
    let (mut main_frames, mut micros_per_frame) = (None, None);
    let mut opendml_frames = None;
    // The first video stream's clock: its ticks a second and its length.
    let mut video_clock: Option<(u32, u32, u32)> = None;
    for (id, body) in chunks(header) {
        match &id {
            b"avih" => {
                micros_per_frame = le32(body, 0).filter(|&m| m > 0);
                main_frames = le32(body, 16);
            }
            b"LIST" => {
                if let Some(stream) = list(body, b"strl") {
                    // Only a picture's stream has a clock (`read_stream`),
                    // so the first one there is the first picture's.
                    let (track, clock) = read_stream(stream);
                    video_clock = video_clock.or(clock);
                    probe.tracks.push(track);
                } else if let Some(odml) = list(body, b"odml") {
                    opendml_frames = chunks(odml)
                        .into_iter()
                        .find(|(id, _)| id == b"dmlh")
                        .and_then(|(_, b)| le32(b, 0))
                        .filter(|&n| n > 0);
                }
            }
            _ => {}
        }
    }
    // The video stream's own length at its own rate, the frame count past a
    // gigabyte; the main header's count at its frame time otherwise.
    probe.duration_secs = match video_clock {
        Some((scale, rate, length)) => {
            let frames = opendml_frames.unwrap_or(length);
            seconds(
                u64::from(frames).saturating_mul(u64::from(scale)),
                u64::from(rate),
            )
        }
        None => opendml_frames.or(main_frames).and_then(|frames| {
            micros_per_frame
                .and_then(|m| seconds(u64::from(frames).saturating_mul(u64::from(m)), 1_000_000))
        }),
    };
    Ok(probe)
}

/// One `strl`: the track, and for a picture its clock -- ticks a frame,
/// ticks a second, and length in frames.
fn read_stream(stream: &[u8]) -> (Track, Option<(u32, u32, u32)>) {
    let mut track = Track::default();
    let mut clock = None;
    let mut handler = None;
    let mut format: &[u8] = &[];
    for (id, body) in chunks(stream) {
        match &id {
            b"strh" => {
                track.kind = match body.get(..4) {
                    Some(b"vids") => Kind::Video,
                    Some(b"auds") => Kind::Audio,
                    Some(b"txts") => Kind::Subtitle,
                    _ => Kind::Other,
                };
                handler = bytes::<4>(body, 4);
                // Flag 1 is "disabled": a stream a player leaves off.
                track.default = le32(body, 8).is_some_and(|f| f & 1 == 0);
                let (scale, rate, length) = (le32(body, 20), le32(body, 24), le32(body, 32));
                if let (Some(scale), Some(rate), Some(length)) = (scale, rate, length)
                    && scale > 0
                    && rate > 0
                {
                    clock = Some((scale, rate, length));
                }
            }
            b"strf" => format = body,
            b"strn" => track.name = text(body),
            _ => {}
        }
    }
    match track.kind {
        Kind::Video => {
            // A bitmap header: its size, then a signed width and height (a
            // negative height is a picture stored top-down), then planes,
            // bits a pixel and the compression's four letters.
            let signed = |at: usize| {
                le32(format, at)
                    .map(|v| i32::from_le_bytes(v.to_le_bytes()).unsigned_abs())
                    .filter(|&v| v > 0)
            };
            track.width = signed(4);
            track.height = signed(8);
            // The compression names the codec; the stream header's handler
            // stands in where the compression is left blank.
            track.codec = bytes::<4>(format, 16)
                .filter(|c| c != &[0; 4])
                .or(handler)
                .map_or(Codec::Unknown, |c| video_codec(&c));
            if let Some((scale, rate, _)) = clock {
                track.frame_rate = Some(f64::from(rate) / f64::from(scale));
            }
        }
        Kind::Audio => {
            // A wave format: its tag, channels and rate.
            let mut tag = le16(format, 0);
            // WAVE_FORMAT_EXTENSIBLE: the real tag opens the sub-format's GUID.
            if tag == Some(0xFFFE) {
                tag = le16(format, 24).or(tag);
            }
            track.codec = tag.map_or(Codec::Unknown, audio_codec);
            track.channels = le16(format, 2).filter(|&c| c > 0);
            track.sample_rate = le32(format, 4).filter(|&r| r > 0);
        }
        Kind::Subtitle | Kind::Other => {}
    }
    let clock = clock.filter(|_| track.kind == Kind::Video);
    (track, clock)
}

/// The codec a video compression's four letters name, whatever their case.
fn video_codec(code: &[u8; 4]) -> Codec {
    let mut upper = *code;
    upper.make_ascii_uppercase();
    match &upper {
        b"H264" | b"X264" | b"AVC1" | b"DAVC" | b"VSSH" => Codec::H264,
        b"HEVC" | b"H265" | b"HVC1" | b"X265" => Codec::H265,
        b"XVID" | b"DIVX" | b"DX50" | b"FMP4" | b"MP4V" => Codec::Mpeg4Visual,
        b"MJPG" => Codec::MotionJpeg,
        b"MPG1" => Codec::Mpeg1Video,
        b"MPG2" | b"MPEG" => Codec::Mpeg2Video,
        b"VP80" => Codec::Vp8,
        b"VP90" => Codec::Vp9,
        b"AV01" => Codec::Av1,
        b"WVC1" => Codec::Vc1,
        _ => crate::mp4::fourcc(code),
    }
}

/// The codec a wave format tag names.
fn audio_codec(tag: u16) -> Codec {
    match tag {
        0x0001 | 0x0003 => Codec::Pcm,
        0x0050 => Codec::Mp2,
        0x0055 => Codec::Mp3,
        0x00FF | 0x1600 | 0x1610 | 0x4143 | 0x706D => Codec::Aac,
        0x0160..=0x0163 => Codec::Wma,
        0x2000 => Codec::Ac3,
        0x2001 => Codec::Dts,
        0xF1AC => Codec::Flac,
        0x566F => Codec::Vorbis,
        0x704F => Codec::Opus,
        other => Codec::Other(format!("0x{other:04X}")),
    }
}
