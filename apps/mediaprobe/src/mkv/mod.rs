//! Matroska and WebM, read by `gui/video/matroska` -- the tree's one Matroska
//! reader, the one the video player plays a file through -- so that the
//! tracks a program lists for a file are the tracks that play from it.
//!
//! Until 2026-10-04 this crate walked a file's EBML itself, by its own rules,
//! and could disagree with the player about what a file held
//! (`requests/f-e-two-matroska-demuxers-which-stays.md`). The facts are
//! `matroska::Demuxer`'s now, read as FFmpeg reads them: a track FFmpeg
//! passes over -- of a kind it does not play, with no codec, or with a codec
//! of another kind than the track's -- is not listed, and a file it cannot
//! open says only what it is.
//!
//! Opening a demuxer reads the Segment's description and nothing else: its
//! `Info`, its `Tracks`, and wherever its `SeekHead` says they are when a
//! `Cluster` of unknown length hides them -- not a frame, and not the `Cues`.
//! Each track's codec setup is read whole, as the player needs it: a crafted
//! file can make that as large as itself (to `matroska`'s 256 MiB an element),
//! but never larger, since nothing is allocated for bytes the file does not
//! hold.
//!
//! What stays here is the first look -- whether the EBML header calls a file
//! WebM, from its first bytes alone ([`container`]), as
//! [`crate::Container::detect`] promises -- and the naming: a codec ID as a
//! [`Codec`], a language as a header gives it.

use std::io::{self, Read, Seek};

use matroska::{Demuxer, TrackKind};

use crate::{Codec, Container, Kind, MAX_CHILDREN, Probe, Track, language, text};

const EBML: u64 = 0x1A45_DFA3;
const DOC_TYPE: u64 = 0x4282;

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

/// What a Matroska or WebM file says about itself, as the player's demuxer
/// reads it. The container is the caller's to fill, from the first bytes.
///
/// # Errors
///
/// When a read or a seek fails. A file the demuxer refuses -- cut short in
/// its description, damaged, or of an EBML version it does not read -- is a
/// probe with nothing in it: the player could not open it either, and a
/// listing that showed its tracks would promise what does not play.
pub(crate) fn probe<R: Read + Seek>(r: &mut R) -> io::Result<Probe> {
    let demuxer = match Demuxer::open(r) {
        Ok(demuxer) => demuxer,
        Err(matroska::Error::Io(kind)) => return Err(io::Error::from(kind)),
        Err(
            matroska::Error::Truncated
            | matroska::Error::Invalid(_)
            | matroska::Error::Unsupported(_),
        ) => return Ok(Probe::default()),
    };
    let info = demuxer.info();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a timestamp scale in nanoseconds is far inside f64's integer-exact range"
    )]
    let tick = info.timestamp_scale as f64 / 1e9;
    Ok(Probe {
        duration_secs: info.duration.map(|ticks| ticks * tick),
        tracks: demuxer.tracks().iter().map(track).collect(),
        title: info.title.as_deref().and_then(text),
        ..Probe::default()
    })
}

/// One of the demuxer's tracks, in this crate's terms. Its defaults are
/// already the demuxer's -- a track is default unless it says not, in English
/// unless it says another language, and its sound 8 kHz in one channel
/// unless it says otherwise -- so what is left is naming.
fn track(t: &matroska::Track) -> Track {
    let kind = match t.kind {
        TrackKind::Video => Kind::Video,
        TrackKind::Audio => Kind::Audio,
        TrackKind::Subtitle => Kind::Subtitle,
        // Timed metadata: nothing to watch or hear.
        TrackKind::Metadata => Kind::Other,
    };
    let pixels = |n: u64| u32::try_from(n).ok().filter(|&n| n > 0);
    // A frame's length says the rate of pictures; of sound or subtitles, a
    // frame is a packet, and its rate means nothing to show.
    #[expect(
        clippy::cast_precision_loss,
        reason = "nanoseconds a frame are far inside f64's integer-exact range"
    )]
    let frame_rate = t
        .default_duration
        .filter(|_| kind == Kind::Video)
        .map(|ns| 1e9 / ns as f64);
    // RFC 9559: a reader that has a track's BCP 47 tag ignores its ISO 639-2
    // code -- an undetermined tag included.
    let language_given = t
        .language_bcp47
        .as_deref()
        .and_then(text)
        .or_else(|| text(&t.language));
    Track {
        kind,
        codec: text(&t.codec_id).map_or(Codec::Unknown, |id| codec_of(&id)),
        width: t.video.and_then(|v| pixels(v.pixel_width)),
        height: t.video.and_then(|v| pixels(v.pixel_height)),
        frame_rate,
        // The rate the sound plays at: twice the coded one for HE-AAC, whose
        // spectral band replication doubles it; the coded rate where the file
        // gives no other, which the demuxer has already put in its place.
        sample_rate: t
            .audio
            .and_then(|a| whole_hertz(a.output_sampling_frequency)),
        channels: t
            .audio
            .and_then(|a| u16::try_from(a.channels).ok().filter(|&c| c > 0)),
        language: language_given.as_deref().and_then(language),
        name: t.name.as_deref().and_then(text),
        default: t.default,
        forced: t.forced,
    }
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
        // WebM's own timed text -- subtitles, captions, descriptions and
        // metadata -- all WebVTT's cues, one codec ID for each use of them.
        id if id.starts_with("D_WEBVTT/") => Codec::WebVtt,
        other => Codec::Other(other.to_owned()),
    }
}
