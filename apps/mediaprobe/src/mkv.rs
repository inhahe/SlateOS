//! Matroska and WebM: EBML, a tree of elements each an id, a length and a
//! body, the id and length in variable-length integers.
//!
//! The facts are in the Segment's `Info` and `Tracks`, which a muxer puts
//! before the first `Cluster` of pictures and sound. The walk steps over what
//! it does not read by length. An element whose length was not known when it
//! was written -- a live recording's `Cluster` -- runs to its parent's end,
//! so the walk ends there, and the `SeekHead` says where `Info` and `Tracks`
//! are past it.

use std::io::{self, Read, Seek};

use crate::{Codec, Container, Kind, MAX_CHILDREN, Probe, Track, language, read_at, text};

const EBML: u64 = 0x1A45_DFA3;
const DOC_TYPE: u64 = 0x4282;
const SEGMENT: u64 = 0x1853_8067;
const SEEK_HEAD: u64 = 0x114D_9B74;
const SEEK: u64 = 0x4DBB;
const SEEK_ID: u64 = 0x53AB;
const SEEK_POSITION: u64 = 0x53AC;
const INFO: u64 = 0x1549_A966;
const TIMESTAMP_SCALE: u64 = 0x2A_D7B1;
const DURATION: u64 = 0x4489;
const TITLE: u64 = 0x7BA9;
const TRACKS: u64 = 0x1654_AE6B;
const TRACK_ENTRY: u64 = 0xAE;
const TRACK_TYPE: u64 = 0x83;
const FLAG_DEFAULT: u64 = 0x88;
const FLAG_FORCED: u64 = 0x55AA;
const NAME: u64 = 0x536E;
const LANGUAGE: u64 = 0x22_B59C;
const LANGUAGE_BCP47: u64 = 0x22_B59D;
const CODEC_ID: u64 = 0x86;
const DEFAULT_DURATION: u64 = 0x23_E383;
const VIDEO: u64 = 0xE0;
const PIXEL_WIDTH: u64 = 0xB0;
const PIXEL_HEIGHT: u64 = 0xBA;
const AUDIO: u64 = 0xE1;
const SAMPLING_FREQUENCY: u64 = 0xB5;
const OUTPUT_SAMPLING_FREQUENCY: u64 = 0x78B5;
const CHANNELS: u64 = 0x9F;

/// The most of `Info` that is read: a few numbers and a title.
const MAX_INFO: usize = 64 * 1024;
/// The most of `Tracks` that is read: a few hundred bytes a track, and codec
/// setup data that can be a few kilobytes more.
const MAX_TRACKS: usize = 1024 * 1024;

/// An element: its id, and where its body starts and where it ends.
#[derive(Clone, Copy, Debug)]
struct El {
    id: u64,
    body: u64,
    end: u64,
}

/// An EBML variable-length integer at `at` in `b`, and its length in bytes:
/// as many as its first byte has leading zeros, plus one. An id keeps the
/// marker bit that says how long it is; a length does not.
pub(crate) fn vint(b: &[u8], at: usize, keep_marker: bool) -> Option<(u64, usize)> {
    let first = *b.get(at)?;
    let n = usize::try_from(first.leading_zeros())
        .ok()?
        .checked_add(1)?;
    if n > 8 {
        return None;
    }
    let mask = if keep_marker {
        0xFF
    } else {
        0xFF_u8.checked_shr(u32::try_from(n).ok()?).unwrap_or(0)
    };
    let mut value = u64::from(first & mask);
    for i in 1..n {
        value = value.checked_shl(8)? | u64::from(*b.get(at.checked_add(i)?)?);
    }
    Some((value, n))
}

/// Whether a length of `n` bytes is every value bit set: "not known".
fn unknown_length(value: u64, n: usize) -> bool {
    let bits = u32::try_from(n.saturating_mul(7)).unwrap_or(u32::MAX);
    1_u64
        .checked_shl(bits)
        .is_some_and(|top| value == top.wrapping_sub(1))
}

/// The element that starts at `at`, inside `limit`. One of unknown length,
/// or running past `limit`, ends at `limit`.
fn element_at<R: Read + Seek>(r: &mut R, at: u64, limit: u64, len: u64) -> io::Result<Option<El>> {
    let head = read_at(r, at, 12, len)?;
    let Some((id, id_len)) = vint(&head, 0, true) else {
        return Ok(None);
    };
    if id_len > 4 {
        return Ok(None);
    }
    let Some((size, size_len)) = vint(&head, id_len, false) else {
        return Ok(None);
    };
    let header = u64::try_from(id_len.saturating_add(size_len)).unwrap_or(u64::MAX);
    let body = at.saturating_add(header);
    let end = if unknown_length(size, size_len) {
        limit
    } else {
        body.saturating_add(size).min(limit)
    };
    if body > end {
        return Ok(None);
    }
    Ok(Some(El { id, body, end }))
}

/// The elements in `b`, in order, each its id and body, as far as they are
/// elements. One of unknown length runs to the end of `b`.
fn elements(b: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0_usize;
    // An element is at least two bytes -- an id and a length -- so `at`
    // moves each time.
    while at < b.len() && out.len() < MAX_CHILDREN {
        let Some((id, id_len)) = vint(b, at, true) else {
            break;
        };
        let Some(size_at) = at.checked_add(id_len) else {
            break;
        };
        let Some((size, size_len)) = vint(b, size_at, false) else {
            break;
        };
        let Some(body) = size_at.checked_add(size_len) else {
            break;
        };
        let end = if unknown_length(size, size_len) {
            b.len()
        } else {
            usize::try_from(size)
                .ok()
                .and_then(|s| body.checked_add(s))
                .map_or(b.len(), |e| e.min(b.len()))
        };
        let Some(value) = b.get(body..end) else {
            break;
        };
        out.push((id, value));
        at = end;
    }
    out
}

/// An unsigned integer element: up to eight bytes, big-endian; empty is 0.
fn uint(b: &[u8]) -> Option<u64> {
    if b.len() > 8 {
        return None;
    }
    Some(b.iter().fold(0_u64, |acc, &x| (acc << 8) | u64::from(x)))
}

/// A float element: four bytes or eight, big-endian; empty is 0.
fn float(b: &[u8]) -> Option<f64> {
    match b.len() {
        0 => Some(0.0),
        4 => Some(f64::from(f32::from_be_bytes(b.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(b.try_into().ok()?)),
        _ => None,
    }
}

/// Matroska, or WebM if the EBML header's document type says so.
pub(crate) fn container(head: &[u8]) -> Container {
    let header = elements(head);
    let doc_type = header
        .first()
        .filter(|(id, _)| *id == EBML)
        .and_then(|(_, body)| elements(body).into_iter().find(|(id, _)| *id == DOC_TYPE))
        .and_then(|(_, v)| text(v));
    if doc_type.as_deref() == Some("webm") {
        Container::WebM
    } else {
        Container::Matroska
    }
}

pub(crate) fn probe<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Probe> {
    let mut probe = Probe::default();
    let Some(header) = element_at(r, 0, len, len)?.filter(|e| e.id == EBML) else {
        return Ok(probe);
    };
    let Some(segment) = element_at(r, header.end, len, len)?.filter(|e| e.id == SEGMENT) else {
        return Ok(probe);
    };
    let (mut info, mut tracks) = (None, None);
    let mut seeks: Vec<(u64, u64)> = Vec::new();
    let mut at = segment.body;
    let mut walked = 0_usize;
    while at < segment.end && walked < MAX_CHILDREN && (info.is_none() || tracks.is_none()) {
        let Some(el) = element_at(r, at, segment.end, len)? else {
            break;
        };
        walked = walked.saturating_add(1);
        match el.id {
            INFO => info = Some(el),
            TRACKS => tracks = Some(el),
            SEEK_HEAD => seeks.extend(read_seek_head(r, el, len)?),
            _ => {}
        }
        at = el.end;
    }
    // Where the seek head says, for what the walk did not reach.
    for (id, position) in seeks {
        let wanted = (id == INFO && info.is_none()) || (id == TRACKS && tracks.is_none());
        if !wanted {
            continue;
        }
        let at = segment.body.saturating_add(position);
        if let Some(el) = element_at(r, at, segment.end, len)?.filter(|e| e.id == id) {
            if id == INFO {
                info = Some(el);
            } else {
                tracks = Some(el);
            }
        }
    }
    if let Some(info) = info {
        let b = read_body(r, info, MAX_INFO, len)?;
        read_info(&b, &mut probe);
    }
    if let Some(tracks) = tracks {
        let b = read_body(r, tracks, MAX_TRACKS, len)?;
        probe.tracks = elements(&b)
            .into_iter()
            .filter(|(id, _)| *id == TRACK_ENTRY)
            .map(|(_, entry)| read_track(entry))
            .collect();
    }
    Ok(probe)
}

/// `el`'s body, as far as `max`.
fn read_body<R: Read + Seek>(r: &mut R, el: El, max: usize, len: u64) -> io::Result<Vec<u8>> {
    let size = usize::try_from(el.end.saturating_sub(el.body)).unwrap_or(usize::MAX);
    read_at(r, el.body, size.min(max), len)
}

/// A seek head's entries: an element's id, and its position from the start
/// of the Segment's body.
fn read_seek_head<R: Read + Seek>(r: &mut R, el: El, len: u64) -> io::Result<Vec<(u64, u64)>> {
    let b = read_body(r, el, MAX_INFO, len)?;
    Ok(elements(&b)
        .into_iter()
        .filter(|(id, _)| *id == SEEK)
        .filter_map(|(_, seek)| {
            let fields = elements(seek);
            let id = fields
                .iter()
                .find(|(i, _)| *i == SEEK_ID)
                .and_then(|(_, v)| uint(v))?;
            let position = fields
                .iter()
                .find(|(i, _)| *i == SEEK_POSITION)
                .and_then(|(_, v)| uint(v))?;
            Some((id, position))
        })
        .collect())
}

/// The Segment's length and title.
fn read_info(b: &[u8], probe: &mut Probe) {
    // Nanoseconds a tick, a millisecond unless the file says otherwise.
    let mut scale = 1_000_000_u64;
    let mut duration = None;
    for (id, value) in elements(b) {
        match id {
            TIMESTAMP_SCALE => {
                if let Some(s) = uint(value).filter(|&s| s > 0) {
                    scale = s;
                }
            }
            DURATION => duration = float(value),
            TITLE => probe.title = text(value),
            _ => {}
        }
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a timestamp scale in nanoseconds is far inside f64's integer-exact range"
    )]
    let tick = scale as f64 / 1e9;
    probe.duration_secs = duration.map(|d| d * tick);
}

/// One `TrackEntry`, with the defaults Matroska gives what it leaves out: a
/// track is default unless it says not, and in English unless it says
/// another language.
fn read_track(entry: &[u8]) -> Track {
    let mut track = Track {
        default: true,
        ..Track::default()
    };
    let (mut iso, mut bcp47) = (Some(String::from("eng")), None);
    let mut frame_ns = None;
    for (id, value) in elements(entry) {
        match id {
            TRACK_TYPE => {
                track.kind = match uint(value) {
                    Some(1) => Kind::Video,
                    Some(2) => Kind::Audio,
                    Some(17) => Kind::Subtitle,
                    _ => Kind::Other,
                };
            }
            FLAG_DEFAULT => track.default = uint(value) != Some(0),
            FLAG_FORCED => track.forced = uint(value) == Some(1),
            NAME => track.name = text(value),
            LANGUAGE => iso = text(value),
            LANGUAGE_BCP47 => bcp47 = text(value),
            CODEC_ID => track.codec = text(value).map_or(Codec::Unknown, |id| codec_of(&id)),
            DEFAULT_DURATION => frame_ns = uint(value).filter(|&n| n > 0),
            VIDEO => {
                for (id, v) in elements(value) {
                    let pixels = || {
                        uint(v)
                            .and_then(|n| u32::try_from(n).ok())
                            .filter(|&n| n > 0)
                    };
                    match id {
                        PIXEL_WIDTH => track.width = pixels(),
                        PIXEL_HEIGHT => track.height = pixels(),
                        _ => {}
                    }
                }
            }
            AUDIO => read_audio(value, &mut track),
            _ => {}
        }
    }
    track.language = bcp47.or(iso).as_deref().and_then(language);
    if track.kind == Kind::Video {
        #[expect(
            clippy::cast_precision_loss,
            reason = "nanoseconds a frame is far inside f64's integer-exact range"
        )]
        let fps = frame_ns.map(|ns| 1e9 / ns as f64);
        track.frame_rate = fps;
    }
    track
}

/// An `Audio` element's rate and channels, with Matroska's defaults: 8 kHz
/// and one channel when it gives none, and the output rate -- twice the
/// coded one for HE-AAC -- over the coded rate when it gives both.
fn read_audio(value: &[u8], track: &mut Track) {
    let (mut coded, mut output, mut channels) = (8000.0, None, 1_u64);
    for (id, v) in elements(value) {
        match id {
            SAMPLING_FREQUENCY => coded = float(v).unwrap_or(coded),
            OUTPUT_SAMPLING_FREQUENCY => output = float(v),
            CHANNELS => channels = uint(v).unwrap_or(channels),
            _ => {}
        }
    }
    track.sample_rate = whole_hertz(output.unwrap_or(coded));
    track.channels = u16::try_from(channels).ok().filter(|&c| c > 0);
}

/// A rate given as a float, in whole hertz.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "checked finite and in u32's range first"
)]
fn whole_hertz(rate: f64) -> Option<u32> {
    (rate.is_finite() && rate >= 1.0 && rate <= f64::from(u32::MAX)).then(|| rate.round() as u32)
}

/// The codec a Matroska codec id names.
fn codec_of(id: &str) -> Codec {
    match id {
        "V_MPEG4/ISO/AVC" => Codec::H264,
        "V_MPEGH/ISO/HEVC" => Codec::H265,
        "V_AV1" => Codec::Av1,
        "V_VP8" => Codec::Vp8,
        "V_VP9" => Codec::Vp9,
        "V_THEORA" => Codec::Theora,
        "V_MPEG2" => Codec::Mpeg2Video,
        "V_MPEG1" => Codec::Mpeg1Video,
        "V_MJPEG" => Codec::MotionJpeg,
        "A_MPEG/L3" => Codec::Mp3,
        "A_MPEG/L2" => Codec::Mp2,
        "A_OPUS" => Codec::Opus,
        "A_VORBIS" => Codec::Vorbis,
        "A_FLAC" => Codec::Flac,
        "A_ALAC" => Codec::Alac,
        "A_AC3" | "A_AC3/BSID9" | "A_AC3/BSID10" => Codec::Ac3,
        "A_EAC3" => Codec::Eac3,
        "A_TRUEHD" => Codec::TrueHd,
        "S_TEXT/UTF8" | "S_TEXT/ASCII" => Codec::Text,
        "S_TEXT/ASS" | "S_ASS" => Codec::Ass,
        "S_TEXT/SSA" | "S_SSA" => Codec::Ssa,
        "S_TEXT/WEBVTT" => Codec::WebVtt,
        "S_HDMV/PGS" => Codec::Pgs,
        "S_VOBSUB" => Codec::VobSub,
        "S_DVBSUB" => Codec::DvbSub,
        id if id.starts_with("V_MPEG4/ISO/") => Codec::Mpeg4Visual,
        id if id.starts_with("A_AAC") => Codec::Aac,
        id if id.starts_with("A_PCM/") => Codec::Pcm,
        id if id.starts_with("A_DTS") => Codec::Dts,
        other => Codec::Other(other.to_owned()),
    }
}
