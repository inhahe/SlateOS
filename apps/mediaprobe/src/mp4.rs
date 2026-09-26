//! ISO base media files -- MP4, M4V, MOV, 3GP: a tree of boxes, each a length
//! and a four-letter type, the facts in `moov`.
//!
//! Only `moov` and its descendants are read, and of those only the small
//! boxes that hold a fact; `mdat` -- the pictures and sound, nearly all of the
//! file -- is stepped over by its size.

use std::io::{self, Read, Seek};

use crate::{
    Codec, Container, Kind, MAX_CHILDREN, Probe, Track, be16, be32, be64, bytes, language, read_at,
    seconds, text,
};

/// The most of a box that holds a fact -- a header's worth of fields, or a
/// sample description -- that is read.
const SMALL_BOX: usize = 4096;

/// A box: its type, and where its body starts and where it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bx {
    kind: [u8; 4],
    body: u64,
    end: u64,
}

/// The container a file's first box names, or `Unknown` if it is not one.
pub(crate) fn container(head: &[u8]) -> Container {
    // A real box is at least its own eight-byte header long (1 is a 64-bit
    // size to follow, 0 "to the end of the file").
    let sized = be32(head, 0).is_some_and(|n| n == 0 || n == 1 || n >= 8);
    match head.get(4..8) {
        Some(b"ftyp") if sized => {
            if head.get(8..12) == Some(b"qt  ") {
                Container::QuickTime
            } else {
                Container::Mp4
            }
        }
        // A QuickTime file older than `ftyp` starts with one of its own boxes.
        Some(b"moov" | b"mdat" | b"wide") if sized => Container::QuickTime,
        _ => Container::Unknown,
    }
}

/// The box that starts at `at`, if one does and it starts inside `limit`. A
/// box running past `limit` -- its parent's end, or a file cut short -- ends
/// there.
fn box_at<R: Read + Seek>(r: &mut R, at: u64, limit: u64, len: u64) -> io::Result<Option<Bx>> {
    let head = read_at(r, at, 16, len)?;
    let (Some(size), Some(kind)) = (be32(&head, 0), bytes::<4>(&head, 4)) else {
        return Ok(None);
    };
    let (header, size) = match size {
        0 => (8_u64, limit.saturating_sub(at)),
        1 => match be64(&head, 8) {
            Some(large) => (16, large),
            None => return Ok(None),
        },
        n => (8, u64::from(n)),
    };
    if size < header {
        return Ok(None);
    }
    let body = at.saturating_add(header);
    let end = at.saturating_add(size).min(limit);
    if body > end {
        return Ok(None);
    }
    Ok(Some(Bx { kind, body, end }))
}

/// The boxes from `from` to `to`, in order, as far as they are boxes.
fn children<R: Read + Seek>(r: &mut R, from: u64, to: u64, len: u64) -> io::Result<Vec<Bx>> {
    let mut out = Vec::new();
    let mut at = from;
    // Every box is at least eight bytes, so `at` moves each time; the cap is
    // for a file of a million empty ones.
    while at < to && out.len() < MAX_CHILDREN {
        let Some(b) = box_at(r, at, to, len)? else {
            break;
        };
        at = b.end;
        out.push(b);
    }
    Ok(out)
}

/// The children of `parent`.
fn inside<R: Read + Seek>(r: &mut R, parent: Bx, len: u64) -> io::Result<Vec<Bx>> {
    children(r, parent.body, parent.end, len)
}

/// The first of `boxes` of `kind`.
fn child(boxes: &[Bx], kind: &[u8; 4]) -> Option<Bx> {
    boxes.iter().find(|b| &b.kind == kind).copied()
}

/// `b`'s body, as far as [`SMALL_BOX`].
fn body<R: Read + Seek>(r: &mut R, b: Bx, len: u64) -> io::Result<Vec<u8>> {
    let size = usize::try_from(b.end.saturating_sub(b.body)).unwrap_or(usize::MAX);
    read_at(r, b.body, size.min(SMALL_BOX), len)
}

/// A full box's timescale, and its duration unless that is "unknown" (every
/// bit set): version 1 has 64-bit times, version 0 32-bit.
fn scale_and_duration(b: &[u8]) -> Option<(u64, Option<u64>)> {
    if b.first() == Some(&1) {
        let scale = u64::from(be32(b, 20)?);
        Some((scale, be64(b, 24).filter(|&d| d != u64::MAX)))
    } else {
        let scale = u64::from(be32(b, 12)?);
        Some((scale, be32(b, 16).filter(|&d| d != u32::MAX).map(u64::from)))
    }
}

/// A duration in `scale` units, in seconds; `None` for none, or a zero one --
/// a fragmented file's header before its fragments say.
fn secs(scale: u64, duration: Option<u64>) -> Option<f64> {
    duration.filter(|&d| d > 0).and_then(|d| seconds(d, scale))
}

pub(crate) fn probe<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Probe> {
    let mut probe = Probe::default();
    let top = children(r, 0, len, len)?;
    let Some(moov) = child(&top, b"moov") else {
        return Ok(probe);
    };
    let kids = inside(r, moov, len)?;
    let mut movie_scale = 0;
    if let Some(mvhd) = child(&kids, b"mvhd") {
        let b = body(r, mvhd, len)?;
        if let Some((scale, duration)) = scale_and_duration(&b) {
            movie_scale = scale;
            probe.duration_secs = secs(scale, duration);
        }
    }
    // A fragmented file's movie header may give no length; its `mehd` does.
    if probe.duration_secs.is_none()
        && let Some(mvex) = child(&kids, b"mvex")
        && let Some(mehd) = child(&inside(r, mvex, len)?, b"mehd")
    {
        let b = body(r, mehd, len)?;
        let duration = if b.first() == Some(&1) {
            be64(&b, 4)
        } else {
            be32(&b, 4).map(u64::from)
        };
        probe.duration_secs = secs(movie_scale, duration);
    }
    let mut longest_track: Option<f64> = None;
    for trak in kids.iter().filter(|b| &b.kind == b"trak") {
        let (track, track_secs) = read_trak(r, *trak, len)?;
        if let Some(s) = track_secs {
            longest_track = Some(longest_track.map_or(s, |l| l.max(s)));
        }
        probe.tracks.push(track);
    }
    // No movie length at all: the longest track's is the movie's.
    if probe.duration_secs.is_none() {
        probe.duration_secs = longest_track;
    }
    if let Some(udta) = child(&kids, b"udta") {
        probe.title = read_title(r, udta, len)?;
    }
    Ok(probe)
}

/// One `trak`, and how long it plays.
fn read_trak<R: Read + Seek>(r: &mut R, trak: Bx, len: u64) -> io::Result<(Track, Option<f64>)> {
    let mut track = Track::default();
    let kids = inside(r, trak, len)?;
    // The presentation size, taken when the sample description gives none.
    let mut presented = (None, None);
    if let Some(tkhd) = child(&kids, b"tkhd") {
        let b = body(r, tkhd, len)?;
        // Flag 1 is "enabled": the track a player shows unless asked otherwise.
        track.default = be32(&b, 0).is_some_and(|v| v & 1 != 0);
        // Width and height close the box, 16.16 fixed point.
        let at = if b.first() == Some(&1) { 88 } else { 76 };
        let fixed = |at: usize| be32(&b, at).map(|v| v >> 16).filter(|&v| v > 0);
        presented = (fixed(at), fixed(at.saturating_add(4)));
    }
    let Some(mdia) = child(&kids, b"mdia") else {
        return Ok((track, None));
    };
    let media = inside(r, mdia, len)?;
    let mut time = None;
    if let Some(mdhd) = child(&media, b"mdhd") {
        let b = body(r, mdhd, len)?;
        time = scale_and_duration(&b);
        let at = if b.first() == Some(&1) { 32 } else { 20 };
        track.language = be16(&b, at).and_then(packed_language);
    }
    if let Some(hdlr) = child(&media, b"hdlr") {
        let b = body(r, hdlr, len)?;
        track.kind = match b.get(8..12) {
            Some(b"vide") => Kind::Video,
            Some(b"soun") => Kind::Audio,
            Some(b"sbtl" | b"text" | b"subt" | b"clcp") => Kind::Subtitle,
            _ => Kind::Other,
        };
    }
    let track_secs = time.and_then(|(scale, duration)| secs(scale, duration));
    let stbl = match child(&media, b"minf") {
        Some(minf) => child(&inside(r, minf, len)?, b"stbl"),
        None => None,
    };
    if let Some(stbl) = stbl {
        let table = inside(r, stbl, len)?;
        if let Some(stsd) = child(&table, b"stsd") {
            read_sample_description(r, stsd, len, &mut track)?;
        }
        // A video track's frames are its samples: their count over its
        // length is its rate -- the average, if the rate varies.
        if track.kind == Kind::Video
            && let Some(sizes) = child(&table, b"stsz").or_else(|| child(&table, b"stz2"))
            && let Some(secs) = track_secs
        {
            let b = body(r, sizes, len)?;
            track.frame_rate = be32(&b, 8).map(|count| f64::from(count) / secs);
        }
    }
    if track.kind == Kind::Video {
        track.width = track.width.or(presented.0);
        track.height = track.height.or(presented.1);
    }
    Ok((track, track_secs))
}

/// The first sample description: what the track is encoded as, and for a
/// picture its size, for sound its channels and rate.
fn read_sample_description<R: Read + Seek>(
    r: &mut R,
    stsd: Bx,
    len: u64,
    track: &mut Track,
) -> io::Result<()> {
    // A full box -- version and flags -- then a count, then the entries.
    let Some(entry) = box_at(r, stsd.body.saturating_add(8), stsd.end, len)? else {
        return Ok(());
    };
    let b = body(r, entry, len)?;
    track.codec = codec_of(&entry.kind);
    match track.kind {
        Kind::Video => {
            // Six reserved bytes and a reference index, sixteen bytes of
            // nothing, then width and height.
            track.width = be16(&b, 24).map(u32::from).filter(|&w| w > 0);
            track.height = be16(&b, 26).map(u32::from).filter(|&h| h > 0);
        }
        Kind::Audio => {
            // QuickTime's sound description version 2 moved both to a later,
            // wider place; versions 0 and 1 share ISO's layout.
            let children_at = if be16(&b, 8) == Some(2) {
                track.channels = be32(&b, 40).and_then(|c| u16::try_from(c).ok());
                track.sample_rate = bytes::<8>(&b, 32)
                    .map(f64::from_be_bytes)
                    .and_then(whole_rate);
                64
            } else {
                track.channels = be16(&b, 16);
                // 16.16 fixed point.
                track.sample_rate = be32(&b, 24).map(|v| v >> 16);
                if be16(&b, 8) == Some(1) { 44 } else { 28 }
            };
            track.channels = track.channels.filter(|&c| c > 0);
            track.sample_rate = track.sample_rate.filter(|&s| s > 0);
            // `mp4a` is MPEG-4's audio entry for AAC and MP3 alike: the
            // elementary stream descriptor says which.
            if &entry.kind == b"mp4a" {
                let from = entry.body.saturating_add(children_at);
                if let Some(esds) = child(&children(r, from, entry.end, len)?, b"esds") {
                    let d = body(r, esds, len)?;
                    if let Some(codec) = esds_object_type(&d).and_then(codec_of_object_type) {
                        track.codec = codec;
                    }
                }
            }
        }
        Kind::Subtitle | Kind::Other => {}
    }
    Ok(())
}

/// A sample rate given as a float, as a whole number of hertz.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "checked finite and in u32's range first"
)]
fn whole_rate(rate: f64) -> Option<u32> {
    (rate.is_finite() && rate >= 1.0 && rate <= f64::from(u32::MAX)).then(|| rate.round() as u32)
}

/// An MPEG-4 descriptor at `at`: its tag, and where its payload starts. The
/// length is up to four bytes of seven bits, the high bit "more follows".
fn descriptor(b: &[u8], at: usize) -> Option<(u8, usize)> {
    let tag = *b.get(at)?;
    let mut i = at.checked_add(1)?;
    for _ in 0..4 {
        let byte = *b.get(i)?;
        i = i.checked_add(1)?;
        if byte & 0x80 == 0 {
            break;
        }
    }
    Some((tag, i))
}

/// An `esds` box's object type: what the elementary stream is encoded as.
fn esds_object_type(b: &[u8]) -> Option<u8> {
    // Version and flags, then the elementary stream descriptor (tag 3).
    let (tag, at) = descriptor(b, 4)?;
    if tag != 3 {
        return None;
    }
    // Its id, then flags saying which optional fields follow.
    let flags = *b.get(at.checked_add(2)?)?;
    let mut at = at.checked_add(3)?;
    if flags & 0x80 != 0 {
        at = at.checked_add(2)?;
    }
    if flags & 0x40 != 0 {
        let url = usize::from(*b.get(at)?);
        at = at.checked_add(1)?.checked_add(url)?;
    }
    if flags & 0x20 != 0 {
        at = at.checked_add(2)?;
    }
    // The decoder configuration (tag 4) opens with the object type.
    let (tag, at) = descriptor(b, at)?;
    (tag == 4).then(|| b.get(at).copied()).flatten()
}

/// The codec an MPEG-4 object type names, for the ones `mp4a` carries.
fn codec_of_object_type(object_type: u8) -> Option<Codec> {
    match object_type {
        0x40 | 0x66..=0x68 => Some(Codec::Aac),
        0x69 | 0x6B => Some(Codec::Mp3),
        0xA5 => Some(Codec::Ac3),
        0xA6 => Some(Codec::Eac3),
        0xA9 | 0xAC => Some(Codec::Dts),
        0xAD => Some(Codec::Opus),
        _ => None,
    }
}

/// The codec a sample entry's four letters name.
fn codec_of(kind: &[u8; 4]) -> Codec {
    match kind {
        b"avc1" | b"avc2" | b"avc3" | b"avc4" => Codec::H264,
        b"hvc1" | b"hev1" => Codec::H265,
        b"av01" => Codec::Av1,
        b"vp08" => Codec::Vp8,
        b"vp09" => Codec::Vp9,
        b"mp4v" => Codec::Mpeg4Visual,
        b"mp2v" | b"m2v1" => Codec::Mpeg2Video,
        b"mjpa" | b"mjpb" | b"jpeg" => Codec::MotionJpeg,
        b"vc-1" => Codec::Vc1,
        b"mp4a" => Codec::Aac,
        b".mp3" => Codec::Mp3,
        b"Opus" => Codec::Opus,
        b"fLaC" => Codec::Flac,
        b"alac" => Codec::Alac,
        b"ac-3" => Codec::Ac3,
        b"ec-3" => Codec::Eac3,
        b"dtsc" | b"dtsh" | b"dtsl" | b"dtse" => Codec::Dts,
        b"mlpa" => Codec::TrueHd,
        b"lpcm" | b"sowt" | b"twos" | b"in24" | b"in32" | b"fl32" | b"fl64" | b"raw " | b"ipcm"
        | b"fpcm" => Codec::Pcm,
        b"tx3g" | b"text" => Codec::Text,
        b"wvtt" => Codec::WebVtt,
        b"stpp" => Codec::Ttml,
        b"apcn" | b"apch" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => {
            Codec::Other(String::from("ProRes"))
        }
        other => fourcc(other),
    }
}

/// Four letters this does not know, as themselves -- or nothing, if they are
/// not letters.
pub(crate) fn fourcc(code: &[u8; 4]) -> Codec {
    if code.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        text(code).map_or(Codec::Unknown, Codec::Other)
    } else {
        Codec::Unknown
    }
}

/// ISO 639-2 packed in fifteen bits, five to a letter, each less 0x60. A
/// value under 0x400 is a Macintosh language number, which QuickTime files
/// use and which this does not map: nothing is said rather than a guess.
pub(crate) fn packed_language(code: u16) -> Option<String> {
    if code < 0x400 {
        return None;
    }
    let letter = |shift: u32| {
        let bits = code.checked_shr(shift).unwrap_or(0) & 0x1F;
        u8::try_from(bits)
            .ok()
            .and_then(|b| b.checked_add(0x60))
            .filter(u8::is_ascii_lowercase)
            .map(char::from)
    };
    let code: String = [letter(10)?, letter(5)?, letter(0)?].iter().collect();
    language(&code)
}

/// The movie's title: iTunes-style (`meta`/`ilst`/`\u{a9}nam`/`data`), or
/// QuickTime's own text atom (`\u{a9}nam` in `udta`).
fn read_title<R: Read + Seek>(r: &mut R, udta: Bx, len: u64) -> io::Result<Option<String>> {
    const NAME: [u8; 4] = [0xA9, b'n', b'a', b'm'];
    let kids = inside(r, udta, len)?;
    if let Some(meta) = child(&kids, b"meta") {
        // ISO's `meta` is a full box -- version and flags before its first
        // child -- and QuickTime's is not: whether `hdlr` is where a first
        // child's type would be says which.
        let peek = read_at(r, meta.body.saturating_add(4), 4, len)?;
        let first = if peek.as_slice() == b"hdlr" {
            meta.body
        } else {
            meta.body.saturating_add(4)
        };
        let items = children(r, first, meta.end, len)?;
        if let Some(ilst) = child(&items, b"ilst")
            && let Some(name) = child(&inside(r, ilst, len)?, &NAME)
            && let Some(data) = child(&inside(r, name, len)?, b"data")
        {
            // A type and a locale, then the text.
            let b = body(r, data, len)?;
            if let Some(title) = b.get(8..).and_then(text) {
                return Ok(Some(title));
            }
        }
    }
    if let Some(name) = child(&kids, &NAME) {
        // A length and a language, then the text.
        let b = body(r, name, len)?;
        let n = be16(&b, 0).map_or(0, usize::from);
        return Ok(b.get(4..n.saturating_add(4)).and_then(text));
    }
    Ok(None)
}
