//! What an audio file says about itself -- how long it plays, at what rate
//! and bitrate, and the title, artist and album its tags give -- read from its
//! headers and tags, without decoding a sample.
//!
//! The readers were `apps/musicplayer`'s own, inside its binary where nothing
//! else could reach them, and `apps/explorer`'s audio columns were invented
//! for want of them ("3:42, 320 kbps, Unknown Artist" for every song). They
//! were not finished readers, and the move finished them: an ISO-8859-1 tag
//! was decoded as UTF-8 (so a title with an "e acute" was dropped whole), a
//! text frame kept the NUL it may end in, a FLAC's bit depth was computed as
//! `hi | (lo + 1)` (a 32-bit FLAC read as 16), nothing read an MP3's length
//! or bitrate, an Ogg file at all, a FLAC's own tags, a WAV's, or an ID3v1
//! tag -- and the player never called any of them.
//!
//! # What is read, and from where
//!
//! | Format | Length, rate | Tags |
//! |---|---|---|
//! | MP3 | the first frame header; a Xing/Info or VBRI header for a VBR file | ID3v2.2/2.3/2.4, ID3v1 |
//! | FLAC | STREAMINFO | its Vorbis comment block |
//! | Ogg (Vorbis, Opus) | the first packet; the last page's granule position | the comment packet |
//! | WAV | `fmt ` and `data` | `LIST`/`INFO` |
//!
//! Everything is bounded: a tag is read up to [`MAX_TAG_BYTES`], an Ogg
//! file's first and last [`OGG_WINDOW`] bytes, and a value out of the file
//! that would run past what was read ends the read rather than a panic.
//!
//! Each field of [`Tags`] is one line of text, for a list's row or a column's
//! cell: a control character inside one -- a line break in a title -- is read
//! as a space.
//!
//! [`testing`] makes real files -- a WAV, an MP3 with its tag -- for other
//! crates' tests.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub mod testing;

/// The largest tag this reads. An ID3v2 tag can carry a cover picture of
/// several megabytes; the text frames are at its start, and a tag larger
/// than this is read as far as this.
pub const MAX_TAG_BYTES: usize = 16 * 1024 * 1024;

/// How much of an Ogg file's start and end is read: the first holds the
/// identification and comment packets, the last the final page.
pub const OGG_WINDOW: usize = 256 * 1024;

/// How far past an ID3v2 tag an MP3's first frame is looked for.
const FRAME_SEARCH: usize = 64 * 1024;

/// An audio file's container, by what its first bytes say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Flac,
    Ogg,
    #[default]
    Unknown,
}

impl AudioFormat {
    /// The format the first bytes of a file name. Not the file's name: that
    /// is a claim by whoever gave it, and the content is the fact.
    #[must_use]
    pub fn detect(head: &[u8]) -> Self {
        if head.get(..4) == Some(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
            Self::Wav
        } else if head.get(..4) == Some(b"fLaC") {
            Self::Flac
        } else if head.get(..4) == Some(b"OggS") {
            Self::Ogg
        } else if let Some(tag) = id3v2_len(head) {
            // An ID3v2 tag says nothing of what follows it: some taggers put
            // one before a FLAC stream. What is after the tag decides -- if it
            // is within `head`; `read` looks further when it is not.
            if head.get(tag..tag.saturating_add(4)) == Some(b"fLaC") {
                Self::Flac
            } else {
                Self::Mp3
            }
        } else if mp3_frame(head).is_some() {
            Self::Mp3
        } else {
            Self::Unknown
        }
    }

    /// The format's usual name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Mp3 => "MP3",
            Self::Flac => "FLAC",
            Self::Ogg => "Ogg",
            Self::Unknown => "Unknown",
        }
    }
}

/// How an audio file plays: each part `None` where the file does not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioInfo {
    pub format: AudioFormat,
    pub duration_secs: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub bits_per_sample: Option<u16>,
    /// The average, for a file whose bitrate varies.
    pub bitrate_kbps: Option<u32>,
}

/// What an audio file's tags say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub track: Option<u32>,
}

impl Tags {
    /// Fill every field this has not from `other`: an ID3v1 tag behind an
    /// ID3v2 one, which the v2 tag wins wherever both say.
    fn fill_from(&mut self, other: Self) {
        let fill = |mine: &mut Option<String>, theirs: Option<String>| {
            if mine.is_none() {
                *mine = theirs;
            }
        };
        fill(&mut self.title, other.title);
        fill(&mut self.artist, other.artist);
        fill(&mut self.album, other.album);
        fill(&mut self.year, other.year);
        fill(&mut self.genre, other.genre);
        if self.track.is_none() {
            self.track = other.track;
        }
    }
}

/// Read the audio file at `path`: how it plays and what its tags say.
///
/// # Errors
///
/// Only when the file cannot be opened or read; a file that is not audio,
/// or whose headers say nothing, is an [`AudioInfo`] of
/// [`AudioFormat::Unknown`] with nothing in it.
pub fn read_path(path: &Path) -> io::Result<(AudioInfo, Tags)> {
    let mut file = std::fs::File::open(path)?;
    read(&mut file)
}

/// [`read_path`], from anything that can be read and sought in.
///
/// # Errors
///
/// When a read or a seek fails.
pub fn read<R: Read + Seek>(r: &mut R) -> io::Result<(AudioInfo, Tags)> {
    let len = r.seek(SeekFrom::End(0))?;
    let head = read_at(r, 0, 64 * 1024, len)?;
    let mut format = AudioFormat::detect(&head);
    // A tag longer than the head: what follows it is read where it is.
    if format == AudioFormat::Mp3
        && let Some(tag) = id3v2_len(&head)
        && tag >= head.len()
        && read_at(r, u64::try_from(tag).unwrap_or(u64::MAX), 4, len)?.as_slice() == b"fLaC"
    {
        format = AudioFormat::Flac;
    }
    let (mut info, tags) = match format {
        AudioFormat::Wav => read_wav(r, len)?,
        AudioFormat::Flac => read_flac(r, len)?,
        AudioFormat::Ogg => read_ogg(r, len)?,
        AudioFormat::Mp3 => read_mp3(r, len)?,
        AudioFormat::Unknown => (AudioInfo::default(), Tags::default()),
    };
    info.format = format;
    Ok((info, tags))
}

/// Up to `want` bytes from `at`, fewer at the end of the file.
fn read_at<R: Read + Seek>(r: &mut R, at: u64, want: usize, len: u64) -> io::Result<Vec<u8>> {
    let room = usize::try_from(len.saturating_sub(at)).unwrap_or(usize::MAX);
    let mut buf = vec![0; want.min(room)];
    r.seek(SeekFrom::Start(at))?;
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// A little-endian `u32` at `at` in `b`.
fn le32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([
        *s.first()?,
        *s.get(1)?,
        *s.get(2)?,
        *s.get(3)?,
    ]))
}

/// A big-endian `u32` at `at` in `b`.
fn be32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([
        *s.first()?,
        *s.get(1)?,
        *s.get(2)?,
        *s.get(3)?,
    ]))
}

/// A little-endian `u16` at `at` in `b`.
fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *b.get(at)?,
        *b.get(at.checked_add(1)?)?,
    ]))
}

/// A little-endian `u64` at `at` in `b`.
fn le64(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at.checked_add(8)?)?;
    let mut bytes = [0; 8];
    bytes.copy_from_slice(s);
    Some(u64::from_le_bytes(bytes))
}

/// Seconds as a float, from a count at a rate.
#[expect(
    clippy::cast_precision_loss,
    reason = "a sample count and a rate are far inside f64's integer-exact range"
)]
fn seconds(count: u64, rate: u32) -> Option<f64> {
    (rate > 0).then(|| count as f64 / f64::from(rate))
}

/// Kilobits a second, from bytes over seconds.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a file's size in bits over its length is a small positive number"
)]
fn kbps(bytes: u64, secs: f64) -> Option<u32> {
    (secs > 0.0).then(|| (bytes as f64 * 8.0 / secs / 1000.0).round() as u32)
}

/// Kilobits a second from bits a second, to the nearest, as [`kbps`]
/// rounds: 352 800 is 353, not 352.
fn kbps_of_bits(bits: u32) -> u32 {
    bits.saturating_add(500) / 1000
}

// ============================================================================
// Text in tags
// ============================================================================

/// Text a tag holds, as one line: trimmed of the NULs and spaces a
/// fixed-width or NUL-terminated field is padded with, and each control
/// character inside a space -- every field is shown on a line of its own, and
/// a line break or an escape drawn into a row is not what the tag meant to
/// show. `None` if nothing is left.
fn clean(text: &str) -> Option<String> {
    let t = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!t.is_empty()).then(|| {
        t.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    })
}

/// ISO-8859-1: every byte is the character with that number.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// UTF-16 in the byte order given, or the one its byte order mark says.
fn utf16(bytes: &[u8], big_endian: bool) -> Option<String> {
    let (bytes, be) = match bytes.get(..2) {
        Some([0xFF, 0xFE]) => (bytes.get(2..)?, false),
        Some([0xFE, 0xFF]) => (bytes.get(2..)?, true),
        _ => (bytes, big_endian),
    };
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            let pair = [
                c.first().copied().unwrap_or(0),
                c.get(1).copied().unwrap_or(0),
            ];
            if be {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            }
        })
        .collect();
    String::from_utf16(&units).ok()
}

/// An ID3v2 text frame's text: its first byte says how it is encoded. A
/// frame of several values (ID3v2.4 separates them with a NUL) gives its first.
fn id3_text(frame: &[u8]) -> Option<String> {
    let (&encoding, body) = frame.split_first()?;
    let text = match encoding {
        0 => latin1(body),
        1 => utf16(body, false)?,
        2 => utf16(body, true)?,
        3 => String::from_utf8(body.to_vec()).ok()?,
        _ => return None,
    };
    let first = text
        .split('\0')
        .find(|v| !v.trim().is_empty())
        .unwrap_or("");
    clean(first)
}

/// The genres ID3v1 numbers, which a TCON frame may give as `(17)` or `17`.
const GENRES: [&str; 80] = [
    "Blues",
    "Classic Rock",
    "Country",
    "Dance",
    "Disco",
    "Funk",
    "Grunge",
    "Hip-Hop",
    "Jazz",
    "Metal",
    "New Age",
    "Oldies",
    "Other",
    "Pop",
    "R&B",
    "Rap",
    "Reggae",
    "Rock",
    "Techno",
    "Industrial",
    "Alternative",
    "Ska",
    "Death Metal",
    "Pranks",
    "Soundtrack",
    "Euro-Techno",
    "Ambient",
    "Trip-Hop",
    "Vocal",
    "Jazz+Funk",
    "Fusion",
    "Trance",
    "Classical",
    "Instrumental",
    "Acid",
    "House",
    "Game",
    "Sound Clip",
    "Gospel",
    "Noise",
    "AlternRock",
    "Bass",
    "Soul",
    "Punk",
    "Space",
    "Meditative",
    "Instrumental Pop",
    "Instrumental Rock",
    "Ethnic",
    "Gothic",
    "Darkwave",
    "Techno-Industrial",
    "Electronic",
    "Pop-Folk",
    "Eurodance",
    "Dream",
    "Southern Rock",
    "Comedy",
    "Cult",
    "Gangsta",
    "Top 40",
    "Christian Rap",
    "Pop/Funk",
    "Jungle",
    "Native American",
    "Cabaret",
    "New Wave",
    "Psychedelic",
    "Rave",
    "Showtunes",
    "Trailer",
    "Lo-Fi",
    "Tribal",
    "Acid Punk",
    "Acid Jazz",
    "Polka",
    "Retro",
    "Musical",
    "Rock & Roll",
    "Hard Rock",
];

/// A genre as a tag gives it: a number (`17`, `(17)`, `(17)Rock`) is the
/// genre ID3v1 numbers so; anything else is itself.
fn genre(text: String) -> String {
    let t = text.trim();
    let number = t
        .strip_prefix('(')
        .and_then(|rest| rest.split(')').next())
        .unwrap_or(t);
    match number.parse::<usize>() {
        Ok(n) => GENRES.get(n).map_or(text.clone(), |g| (*g).to_owned()),
        Err(_) => text,
    }
}

/// A track number as a tag gives it: `3`, or `3/12`.
fn track_number(text: &str) -> Option<u32> {
    text.split('/').next()?.trim().parse().ok()
}

// ============================================================================
// ID3
// ============================================================================

/// A synchsafe integer: four bytes of seven bits.
fn synchsafe(b: &[u8]) -> Option<u32> {
    let s = b.get(..4)?;
    if s.iter().any(|&x| x & 0x80 != 0) {
        return None;
    }
    Some(s.iter().fold(0_u32, |acc, &x| (acc << 7) | u32::from(x)))
}

/// Undo ID3 unsynchronisation: every `FF 00` was an `FF`.
fn unsynchronise(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut last = 0_u8;
    for &b in data {
        if !(last == 0xFF && b == 0) {
            out.push(b);
        }
        last = b;
    }
    out
}

/// The size of the ID3v2 tag at the start of `head`, header included, or
/// `None` if there is none.
fn id3v2_len(head: &[u8]) -> Option<usize> {
    if head.get(..3) != Some(b"ID3") {
        return None;
    }
    let size = usize::try_from(synchsafe(head.get(6..10)?)?).ok()?;
    let footer = if head.get(5)? & 0x10 != 0 { 10 } else { 0 };
    10_usize.checked_add(size)?.checked_add(footer)
}

/// The tags an ID3v2 tag holds. `tag` is the whole tag, header included.
fn parse_id3v2(tag: &[u8]) -> Tags {
    let mut tags = Tags::default();
    let (Some(&major), Some(&flags)) = (tag.get(3), tag.get(5)) else {
        return tags;
    };
    // The whole tag unsynchronised (v2.2 and v2.3), past the header.
    let body = tag.get(10..).unwrap_or(&[]);
    let body = if flags & 0x80 != 0 && major < 4 {
        unsynchronise(body)
    } else {
        body.to_vec()
    };
    let mut pos = 0_usize;
    // An extended header is skipped: its size is in its first four bytes.
    if flags & 0x40 != 0 && major >= 3 {
        let ext = if major >= 4 {
            synchsafe(body.get(..4).unwrap_or(&[]))
        } else {
            be32(&body, 0).map(|n| n.saturating_add(4))
        };
        pos = ext
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(usize::MAX);
    }
    let (id_len, header_len) = if major == 2 { (3, 6) } else { (4, 10) };
    while let Some(header) = body.get(pos..pos.saturating_add(header_len)) {
        let Some(id) = header.get(..id_len) else {
            break;
        };
        if id.iter().all(|&b| b == 0) || !id.iter().all(u8::is_ascii_alphanumeric) {
            break; // padding, or no more frames
        }
        let size = match major {
            2 => header
                .get(3..6)
                .map(|s| s.iter().fold(0_u32, |acc, &x| (acc << 8) | u32::from(x))),
            3 => be32(header, 4),
            _ => synchsafe(header.get(4..8).unwrap_or(&[])),
        };
        let Some(size) = size.and_then(|n| usize::try_from(n).ok()) else {
            break;
        };
        let start = pos.saturating_add(header_len);
        let Some(end) = start.checked_add(size) else {
            break;
        };
        let Some(mut frame) = body.get(start..end).map(<[u8]>::to_vec) else {
            break;
        };
        // A compressed or encrypted frame is not text this can read; a v2.4
        // frame may be unsynchronised, or carry its length first.
        if major >= 3 {
            let fflags = header.get(9).copied().unwrap_or(0);
            let (compressed, encrypted, unsync, length_first) = if major == 3 {
                (fflags & 0x80 != 0, fflags & 0x40 != 0, false, false)
            } else {
                (
                    fflags & 0x08 != 0,
                    fflags & 0x04 != 0,
                    fflags & 0x02 != 0,
                    fflags & 0x01 != 0,
                )
            };
            if compressed || encrypted {
                pos = end;
                continue;
            }
            if unsync {
                frame = unsynchronise(&frame);
            }
            if length_first {
                frame = frame.get(4..).unwrap_or(&[]).to_vec();
            }
        }
        let text = || id3_text(&frame);
        match id {
            b"TIT2" | b"TT2" => tags.title = text(),
            b"TPE1" | b"TP1" => tags.artist = text(),
            b"TALB" | b"TAL" => tags.album = text(),
            b"TDRC" | b"TYER" | b"TYE" => tags.year = text(),
            b"TCON" | b"TCO" => tags.genre = text().map(genre),
            b"TRCK" | b"TRK" => tags.track = text().as_deref().and_then(track_number),
            _ => {}
        }
        pos = end;
    }
    tags
}

/// The tags an ID3v1 tag holds: the last 128 bytes of a file, `TAG` first.
fn parse_id3v1(last: &[u8]) -> Option<Tags> {
    if last.len() != 128 || last.get(..3) != Some(b"TAG") {
        return None;
    }
    let field = |from: usize, len: usize| {
        let bytes = last.get(from..from.checked_add(len)?)?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        clean(&latin1(bytes.get(..end)?))
    };
    // ID3v1.1: a zero at 125 and a track number at 126.
    let track = (last.get(125) == Some(&0))
        .then(|| last.get(126).copied())
        .flatten()
        .filter(|&n| n > 0)
        .map(u32::from);
    Some(Tags {
        title: field(3, 30),
        artist: field(33, 30),
        album: field(63, 30),
        year: field(93, 4),
        genre: last
            .get(127)
            .and_then(|&g| GENRES.get(usize::from(g)))
            .map(|g| (*g).to_owned()),
        track,
    })
}

/// A file's ID3v1 tag, if it has one: 128 bytes at its end.
fn read_id3v1<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Option<Tags>> {
    if len < 128 {
        return Ok(None);
    }
    let last = read_at(r, len.saturating_sub(128), 128, len)?;
    Ok(parse_id3v1(&last))
}

// ============================================================================
// MP3
// ============================================================================

/// An MPEG audio frame header, as far as its length and rate go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mp3Frame {
    /// 1 for MPEG-1, 2 for MPEG-2, 25 for MPEG-2.5.
    version: u8,
    layer: u8,
    bitrate_kbps: u32,
    sample_rate: u32,
    padding: bool,
    mono: bool,
}

impl Mp3Frame {
    /// Samples in one frame.
    fn samples(self) -> u32 {
        match (self.layer, self.version) {
            (1, _) => 384,
            (3, 2 | 25) => 576,
            _ => 1152,
        }
    }

    /// The frame's length in bytes, header included.
    fn len(self) -> usize {
        let bits = u64::from(self.bitrate_kbps).saturating_mul(1000);
        let rate = u64::from(self.sample_rate).max(1);
        let pad = u64::from(self.padding);
        let bytes = match self.layer {
            1 => bits
                .saturating_mul(12)
                .checked_div(rate)
                .unwrap_or(0)
                .saturating_add(pad)
                .saturating_mul(4),
            3 if self.version != 1 => bits
                .saturating_mul(72)
                .checked_div(rate)
                .unwrap_or(0)
                .saturating_add(pad),
            _ => bits
                .saturating_mul(144)
                .checked_div(rate)
                .unwrap_or(0)
                .saturating_add(pad),
        };
        usize::try_from(bytes).unwrap_or(usize::MAX)
    }

    /// Where a Xing or Info header sits, from the frame's start: after the
    /// four-byte header and the side information.
    fn xing_offset(self) -> usize {
        match (self.version, self.mono) {
            (1, false) => 36,
            (1, true) | (_, false) => 21,
            (_, true) => 13,
        }
    }
}

/// MPEG-1 bitrates by layer (I, II, III) and index, in kilobits.
const MPEG1_KBPS: [[u32; 15]; 3] = [
    [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ],
];

/// MPEG-2 and 2.5 bitrates: layer I, then layers II and III.
const MPEG2_KBPS: [[u32; 15]; 2] = [
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ],
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];

/// The frame header at the start of `b`, if it is one.
fn mp3_frame(b: &[u8]) -> Option<Mp3Frame> {
    let h = be32(b, 0)?;
    if h >> 21 != 0x7FF {
        return None;
    }
    let version: u8 = match (h >> 19) & 3 {
        0 => 25,
        2 => 2,
        3 => 1,
        _ => return None,
    };
    let layer: u8 = match (h >> 17) & 3 {
        1 => 3,
        2 => 2,
        3 => 1,
        _ => return None,
    };
    let index = usize::try_from((h >> 12) & 0xF).ok()?;
    let row = usize::from(layer.saturating_sub(1));
    let bitrate_kbps = if version == 1 {
        *MPEG1_KBPS.get(row)?.get(index)?
    } else {
        *MPEG2_KBPS.get(usize::from(layer != 1))?.get(index)?
    };
    let base = match (h >> 10) & 3 {
        0 => 44_100,
        1 => 48_000,
        2 => 32_000,
        _ => return None,
    };
    let sample_rate = match version {
        1 => base,
        2 => base / 2,
        _ => base / 4,
    };
    if bitrate_kbps == 0 {
        return None; // "free format": no length to go by
    }
    Some(Mp3Frame {
        version,
        layer,
        bitrate_kbps,
        sample_rate,
        padding: (h >> 9) & 1 == 1,
        mono: (h >> 6) & 3 == 3,
    })
}

/// An MP3's length and bitrate: from a Xing/Info or VBRI header when the
/// first frame has one (a VBR file), else from its constant bitrate and the
/// bytes of audio.
fn read_mp3<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<(AudioInfo, Tags)> {
    let head = read_at(r, 0, 10, len)?;
    let (mut tags, audio_start) = match id3v2_len(&head) {
        Some(tag_len) => {
            let tag = read_at(r, 0, tag_len.min(MAX_TAG_BYTES), len)?;
            (parse_id3v2(&tag), tag_len)
        }
        None => (Tags::default(), 0),
    };
    let v1 = read_id3v1(r, len)?;
    let audio_end = if v1.is_some() {
        len.saturating_sub(128)
    } else {
        len
    };
    if let Some(v1) = v1 {
        tags.fill_from(v1);
    }
    let start = u64::try_from(audio_start).unwrap_or(u64::MAX);
    let window = read_at(r, start, FRAME_SEARCH, len)?;
    // The first frame: the first place a header is followed, at the length
    // it gives, by another -- or by the end of what was read. Eleven set bits
    // are one byte in 2048 of anything, and a lone match in a tag's padding or
    // a picture is not a frame.
    let mut info = AudioInfo::default();
    let found = (0..window.len()).find_map(|at| {
        let rest = window.get(at..)?;
        let frame = mp3_frame(rest)?;
        let next = frame.len();
        let confirmed = match rest.get(next..) {
            Some(after) if after.len() >= 4 => mp3_frame(after).is_some(),
            _ => true,
        };
        confirmed.then_some((at, frame))
    });
    let Some((at, frame)) = found else {
        return Ok((info, tags));
    };
    info.sample_rate = Some(frame.sample_rate);
    info.channels = Some(if frame.mono { 1 } else { 2 });
    let first = window.get(at..).unwrap_or(&[]);
    let xing = first.get(frame.xing_offset()..).unwrap_or(&[]);
    let vbr = if matches!(xing.get(..4), Some(b"Xing" | b"Info")) {
        let flags = be32(xing, 4).unwrap_or(0);
        let frames = (flags & 1 != 0).then(|| be32(xing, 8)).flatten();
        let bytes = (flags & 2 != 0)
            .then(|| be32(xing, if flags & 1 != 0 { 12 } else { 8 }))
            .flatten();
        frames.map(|f| (f, bytes))
    } else if first.get(36..40) == Some(b"VBRI") {
        be32(first, 50).map(|f| (f, be32(first, 46)))
    } else {
        None
    };
    let audio_bytes =
        audio_end.saturating_sub(start.saturating_add(u64::try_from(at).unwrap_or(0)));
    match vbr {
        Some((frames, bytes)) => {
            let secs = seconds(
                u64::from(frames).saturating_mul(u64::from(frame.samples())),
                frame.sample_rate,
            );
            info.duration_secs = secs;
            info.bitrate_kbps = secs.and_then(|s| kbps(bytes.map_or(audio_bytes, u64::from), s));
        }
        None => {
            info.bitrate_kbps = Some(frame.bitrate_kbps);
            #[expect(
                clippy::cast_precision_loss,
                reason = "a file's size in bits is far inside f64's integer-exact range"
            )]
            let secs = audio_bytes as f64 * 8.0 / (f64::from(frame.bitrate_kbps) * 1000.0);
            info.duration_secs = Some(secs);
        }
    }
    Ok((info, tags))
}

// ============================================================================
// Vorbis comments (FLAC, Ogg)
// ============================================================================

/// The tags a Vorbis comment block holds: a vendor string, then
/// `KEY=value` pairs, lengths little-endian.
fn parse_vorbis_comments(block: &[u8]) -> Tags {
    let mut tags = Tags::default();
    let Some(vendor) = le32(block, 0).and_then(|n| usize::try_from(n).ok()) else {
        return tags;
    };
    let mut pos = 4_usize.saturating_add(vendor);
    let Some(count) = le32(block, pos) else {
        return tags;
    };
    pos = pos.saturating_add(4);
    for _ in 0..count {
        let Some(n) = le32(block, pos).and_then(|n| usize::try_from(n).ok()) else {
            break;
        };
        let start = pos.saturating_add(4);
        let Some(bytes) = start.checked_add(n).and_then(|end| block.get(start..end)) else {
            break;
        };
        pos = start.saturating_add(n);
        let Ok(comment) = std::str::from_utf8(bytes) else {
            continue;
        };
        let Some((key, value)) = comment.split_once('=') else {
            continue;
        };
        // The first of a repeated key: "ARTIST" twice is two artists.
        let slot = match key.to_ascii_uppercase().as_str() {
            "TITLE" => &mut tags.title,
            "ARTIST" => &mut tags.artist,
            "ALBUM" => &mut tags.album,
            "DATE" | "YEAR" => &mut tags.year,
            "GENRE" => &mut tags.genre,
            "TRACKNUMBER" => {
                if tags.track.is_none() {
                    tags.track = track_number(value);
                }
                continue;
            }
            _ => continue,
        };
        if slot.is_none() {
            *slot = clean(value);
        }
    }
    tags
}

// ============================================================================
// FLAC
// ============================================================================

/// A FLAC file's STREAMINFO -- rate, channels, depth, samples -- and its
/// Vorbis comment block. A FLAC may start with an ID3v2 tag, which some
/// taggers put there; it is stepped over, and its tags used for what the
/// file's own do not say.
fn read_flac<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<(AudioInfo, Tags)> {
    let head = read_at(r, 0, 10, len)?;
    let (id3, mut pos) = match id3v2_len(&head) {
        Some(tag_len) => {
            let tag = read_at(r, 0, tag_len.min(MAX_TAG_BYTES), len)?;
            (
                Some(parse_id3v2(&tag)),
                u64::try_from(tag_len).unwrap_or(u64::MAX),
            )
        }
        None => (None, 0),
    };
    let mut info = AudioInfo::default();
    let mut tags = Tags::default();
    if read_at(r, pos, 4, len)?.as_slice() != b"fLaC" {
        return Ok((info, id3.unwrap_or_default()));
    }
    pos = pos.saturating_add(4);
    // The metadata blocks, until the last says it is.
    for _ in 0..256 {
        let header = read_at(r, pos, 4, len)?;
        let (Some(&kind), Some(size)) = (
            header.first(),
            header
                .get(1..4)
                .map(|s| s.iter().fold(0_u32, |acc, &x| (acc << 8) | u32::from(x))),
        ) else {
            break;
        };
        let body_at = pos.saturating_add(4);
        let size_usize = usize::try_from(size).unwrap_or(usize::MAX);
        match kind & 0x7F {
            0 => {
                let si = read_at(r, body_at, size_usize.min(34), len)?;
                if let (Some(&a), Some(&b), Some(&c), Some(&d)) =
                    (si.get(10), si.get(11), si.get(12), si.get(13))
                {
                    let rate = (u32::from(a) << 12) | (u32::from(b) << 4) | (u32::from(c) >> 4);
                    let channels = u16::from((c >> 1) & 7).saturating_add(1);
                    // Five bits across two bytes, plus one: `(hi | lo) + 1`.
                    // It was `hi | (lo + 1)`, which read a 32-bit FLAC as 16.
                    let depth = ((u16::from(c & 1) << 4) | u16::from(d >> 4)).saturating_add(1);
                    let samples =
                        (u64::from(d & 0x0F) << 32) | u64::from(be32(&si, 14).unwrap_or(0));
                    info.sample_rate = (rate > 0).then_some(rate);
                    info.channels = Some(channels);
                    info.bits_per_sample = Some(depth);
                    info.duration_secs = if samples > 0 {
                        seconds(samples, rate)
                    } else {
                        None
                    };
                }
            }
            4 => {
                let block = read_at(r, body_at, size_usize.min(MAX_TAG_BYTES), len)?;
                tags = parse_vorbis_comments(&block);
            }
            _ => {}
        }
        pos = body_at.saturating_add(u64::from(size));
        if kind & 0x80 != 0 {
            break;
        }
    }
    if let Some(secs) = info.duration_secs {
        info.bitrate_kbps = kbps(len.saturating_sub(pos), secs);
    }
    if let Some(id3) = id3 {
        tags.fill_from(id3);
    }
    Ok((info, tags))
}

// ============================================================================
// Ogg
// ============================================================================

/// The packets that begin in `data`, joined across pages, as far as `data`
/// goes: Ogg's pages carry a stream in segments of up to 255 bytes, and a
/// packet ends at the first segment shorter than that.
fn ogg_packets(data: &[u8], want: usize) -> Vec<Vec<u8>> {
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut pos = 0_usize;
    while packets.len() < want {
        let Some(page) = data.get(pos..) else {
            break;
        };
        if page.get(..4) != Some(b"OggS") {
            break;
        }
        let Some(&count) = page.get(26) else {
            break;
        };
        let table_end = 27_usize.saturating_add(usize::from(count));
        let Some(table) = page.get(27..table_end) else {
            break;
        };
        let mut body = table_end;
        for &segment in table {
            let end = body.saturating_add(usize::from(segment));
            let Some(bytes) = page.get(body..end) else {
                return packets;
            };
            current.extend_from_slice(bytes);
            body = end;
            if segment < 255 {
                packets.push(std::mem::take(&mut current));
                if packets.len() >= want {
                    return packets;
                }
            }
        }
        pos = pos.saturating_add(body);
    }
    packets
}

/// An Ogg Vorbis or Opus file: its rate and channels from the first packet,
/// its tags from the second, and its length from the last page's granule
/// position -- the sample the stream ends at.
fn read_ogg<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<(AudioInfo, Tags)> {
    let head = read_at(r, 0, OGG_WINDOW, len)?;
    let packets = ogg_packets(&head, 2);
    let mut info = AudioInfo::default();
    let mut tags = Tags::default();
    let (Some(first), second) = (packets.first(), packets.get(1)) else {
        return Ok((info, tags));
    };
    // Vorbis: 1 "vorbis" version channels rate. Opus: "OpusHead" version
    // channels pre-skip rate -- and its granule counts at 48 kHz whatever
    // rate the audio was made at.
    let (rate, granule_rate, pre_skip) = if first.get(..7) == Some(b"\x01vorbis") {
        let channels = first.get(11).copied();
        info.channels = channels.map(u16::from);
        let rate = le32(first, 12);
        let nominal = le32(first, 20).filter(|&b| b > 0 && b < u32::MAX / 2);
        info.bitrate_kbps = nominal.map(kbps_of_bits);
        if let Some(comment) = second.filter(|p| p.get(..7) == Some(b"\x03vorbis")) {
            tags = parse_vorbis_comments(comment.get(7..).unwrap_or(&[]));
        }
        (rate, rate, 0)
    } else if first.get(..8) == Some(b"OpusHead") {
        info.channels = first.get(9).copied().map(u16::from);
        let pre_skip = le16(first, 10).map_or(0, u64::from);
        if let Some(comment) = second.filter(|p| p.get(..8) == Some(b"OpusTags")) {
            tags = parse_vorbis_comments(comment.get(8..).unwrap_or(&[]));
        }
        (Some(48_000), Some(48_000), pre_skip)
    } else {
        return Ok((info, tags));
    };
    info.sample_rate = rate.filter(|&r| r > 0);
    // The last page: the last "OggS" in the file's end.
    let tail_at = len.saturating_sub(u64::try_from(OGG_WINDOW).unwrap_or(u64::MAX));
    let tail = read_at(r, tail_at, OGG_WINDOW, len)?;
    let last = tail
        .windows(4)
        .rposition(|w| w == b"OggS")
        .and_then(|at| tail.get(at..));
    if let (Some(page), Some(rate)) = (last, granule_rate)
        && let Some(granule) = le64(page, 6).filter(|&g| g != u64::MAX)
    {
        info.duration_secs = seconds(granule.saturating_sub(pre_skip), rate);
        if info.bitrate_kbps.is_none()
            && let Some(secs) = info.duration_secs
        {
            info.bitrate_kbps = kbps(len, secs);
        }
    }
    Ok((info, tags))
}

// ============================================================================
// WAV
// ============================================================================

/// A WAV file's `fmt ` chunk -- rate, channels, depth -- its `data` chunk's
/// size for its length, and its `LIST`/`INFO` chunk's tags.
fn read_wav<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<(AudioInfo, Tags)> {
    let mut info = AudioInfo::default();
    let mut tags = Tags::default();
    let mut pos = 12_u64;
    let mut byte_rate = 0_u32;
    let mut data_size: Option<u64> = None;
    for _ in 0..1024 {
        let header = read_at(r, pos, 8, len)?;
        let (Some(id), Some(size)) = (header.get(..4), le32(&header, 4)) else {
            break;
        };
        let body_at = pos.saturating_add(8);
        let size_usize = usize::try_from(size).unwrap_or(usize::MAX);
        match id {
            b"fmt " => {
                let fmt = read_at(r, body_at, size_usize.min(64), len)?;
                info.channels = le16(&fmt, 2).filter(|&c| c > 0);
                info.sample_rate = le32(&fmt, 4).filter(|&r| r > 0);
                byte_rate = le32(&fmt, 8).unwrap_or(0);
                info.bits_per_sample = le16(&fmt, 14).filter(|&b| b > 0);
            }
            b"data" => data_size = Some(u64::from(size)),
            b"LIST" => {
                let list = read_at(r, body_at, size_usize.min(MAX_TAG_BYTES), len)?;
                if list.get(..4) == Some(b"INFO") {
                    let mut at = 4_usize;
                    while let (Some(sub), Some(n)) = (
                        list.get(at..at.saturating_add(4)),
                        le32(&list, at.saturating_add(4)),
                    ) {
                        let start = at.saturating_add(8);
                        let n = usize::try_from(n).unwrap_or(usize::MAX);
                        let Some(text) = start.checked_add(n).and_then(|end| list.get(start..end))
                        else {
                            break;
                        };
                        let value = clean(
                            &String::from_utf8(text.to_vec()).unwrap_or_else(|_| latin1(text)),
                        );
                        match sub {
                            b"INAM" => tags.title = value,
                            b"IART" => tags.artist = value,
                            b"IPRD" => tags.album = value,
                            b"ICRD" => tags.year = value,
                            b"IGNR" => tags.genre = value,
                            b"ITRK" => tags.track = value.as_deref().and_then(track_number),
                            _ => {}
                        }
                        at = start.saturating_add(n).saturating_add(n & 1);
                    }
                }
            }
            _ => {}
        }
        // Chunks are word-aligned.
        pos = body_at
            .saturating_add(u64::from(size))
            .saturating_add(u64::from(size & 1));
        if pos >= len {
            break;
        }
    }
    if let Some(bytes) = data_size {
        let bytes = bytes.min(len);
        if byte_rate > 0 {
            info.duration_secs = seconds(bytes, byte_rate);
            info.bitrate_kbps = Some(kbps_of_bits(byte_rate.saturating_mul(8)));
        }
    }
    Ok((info, tags))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read_bytes(bytes: &[u8]) -> (AudioInfo, Tags) {
        read(&mut Cursor::new(bytes.to_vec())).expect("read")
    }

    /// An ID3v2 text frame: version 3 or 4 header, encoding byte, text.
    fn id3_frame(major: u8, id: &[u8; 4], encoding: u8, text: &[u8]) -> Vec<u8> {
        let mut body = vec![encoding];
        body.extend_from_slice(text);
        let n = body.len() as u32;
        let size = if major >= 4 {
            [
                ((n >> 21) & 0x7F) as u8,
                ((n >> 14) & 0x7F) as u8,
                ((n >> 7) & 0x7F) as u8,
                (n & 0x7F) as u8,
            ]
        } else {
            n.to_be_bytes()
        };
        let mut f = id.to_vec();
        f.extend_from_slice(&size);
        f.extend_from_slice(&[0, 0]);
        f.extend_from_slice(&body);
        f
    }

    /// A whole ID3v2 tag around `frames`, with `padding` zeros after them.
    fn id3_tag(major: u8, frames: &[Vec<u8>], padding: usize) -> Vec<u8> {
        let mut body: Vec<u8> = frames.concat();
        body.extend(std::iter::repeat_n(0, padding));
        let n = body.len() as u32;
        let mut t = b"ID3".to_vec();
        t.extend_from_slice(&[major, 0, 0]);
        t.extend_from_slice(&[
            ((n >> 21) & 0x7F) as u8,
            ((n >> 14) & 0x7F) as u8,
            ((n >> 7) & 0x7F) as u8,
            (n & 0x7F) as u8,
        ]);
        t.extend_from_slice(&body);
        t
    }

    use crate::testing::mp3_frames;

    #[test]
    fn a_constant_bitrate_mp3_is_timed_by_its_size() {
        // 100 frames of 1152 samples at 44.1 kHz: 2.612 seconds.
        let (info, _) = read_bytes(&mp3_frames(100));
        assert_eq!(info.format, AudioFormat::Mp3);
        assert_eq!(info.sample_rate, Some(44_100));
        assert_eq!(info.channels, Some(2));
        assert_eq!(info.bitrate_kbps, Some(128));
        let secs = info.duration_secs.unwrap();
        assert!((secs - 41_700.0 * 8.0 / 128_000.0).abs() < 1e-6, "{secs}");
    }

    #[test]
    fn a_variable_bitrate_mp3_is_timed_by_its_xing_header() {
        let mut file = mp3_frames(10);
        // The Xing header in the first frame, after 32 bytes of side info:
        // frames and bytes present, 3000 frames.
        let at = 4 + 32;
        file[at..at + 4].copy_from_slice(b"Xing");
        file[at + 4..at + 8].copy_from_slice(&3u32.to_be_bytes());
        file[at + 8..at + 12].copy_from_slice(&3000u32.to_be_bytes());
        file[at + 12..at + 16].copy_from_slice(&1_000_000u32.to_be_bytes());
        let (info, _) = read_bytes(&file);
        let secs = info.duration_secs.unwrap();
        assert!((secs - 3000.0 * 1152.0 / 44_100.0).abs() < 1e-6, "{secs}");
        assert_eq!(info.bitrate_kbps, Some(102), "1,000,000 bytes over 78.4 s");
    }

    #[test]
    fn id3v2_text_is_read_in_every_encoding_and_trimmed_of_its_nul() {
        let frames = vec![
            id3_frame(3, b"TIT2", 0, b"Caf\xe9\0"),
            id3_frame(3, b"TPE1", 1, &[0xFF, 0xFE, b'A', 0, b'b', 0, 0, 0]),
            id3_frame(3, b"TALB", 2, &[0, b'X', 0, b'Y']),
            id3_frame(3, b"TCON", 0, b"(17)"),
            id3_frame(3, b"TRCK", 0, b"3/12"),
        ];
        let mut file = id3_tag(3, &frames, 64);
        file.extend(mp3_frames(4));
        let (info, tags) = read_bytes(&file);
        assert_eq!(info.format, AudioFormat::Mp3);
        assert_eq!(
            tags.title.as_deref(),
            Some("Caf\u{e9}"),
            "Latin-1 was not read as Latin-1"
        );
        assert_eq!(tags.artist.as_deref(), Some("Ab"));
        assert_eq!(tags.album.as_deref(), Some("XY"));
        assert_eq!(tags.genre.as_deref(), Some("Rock"));
        assert_eq!(tags.track, Some(3));
        assert_eq!(
            info.sample_rate,
            Some(44_100),
            "the frame after the tag was not found"
        );
    }

    #[test]
    fn id3v2_4_sizes_are_synchsafe_and_a_second_value_is_left_out() {
        let frames = vec![id3_frame(4, b"TIT2", 3, "Na\u{ef}ve\0Other".as_bytes())];
        let mut file = id3_tag(4, &frames, 0);
        file.extend(mp3_frames(2));
        let (_, tags) = read_bytes(&file);
        assert_eq!(tags.title.as_deref(), Some("Na\u{ef}ve"));
    }

    #[test]
    fn an_id3v1_tag_fills_what_the_id3v2_one_does_not_say() {
        let mut file = id3_tag(3, &[id3_frame(3, b"TIT2", 0, b"From v2")], 0);
        file.extend(mp3_frames(3));
        let mut v1 = vec![0u8; 128];
        v1[..3].copy_from_slice(b"TAG");
        v1[3..10].copy_from_slice(b"From v1");
        v1[33..39].copy_from_slice(b"Artist");
        v1[63..68].copy_from_slice(b"Album");
        v1[93..97].copy_from_slice(b"1999");
        v1[125] = 0;
        v1[126] = 7;
        v1[127] = 8;
        file.extend(&v1);
        let (info, tags) = read_bytes(&file);
        assert_eq!(tags.title.as_deref(), Some("From v2"), "v1 overrode v2");
        assert_eq!(tags.artist.as_deref(), Some("Artist"));
        assert_eq!(tags.album.as_deref(), Some("Album"));
        assert_eq!(tags.year.as_deref(), Some("1999"));
        assert_eq!(tags.track, Some(7));
        assert_eq!(tags.genre.as_deref(), Some("Jazz"));
        // The v1 tag is not audio: three frames of 417 bytes, timed as such.
        let secs = info.duration_secs.unwrap();
        assert!(
            (secs - 3.0 * 417.0 * 8.0 / 128_000.0).abs() < 1e-6,
            "{secs}"
        );
    }

    /// A FLAC file: STREAMINFO for `rate` Hz, `channels`, `depth` bits,
    /// `samples`, then a Vorbis comment block with `comments`.
    fn flac(rate: u32, channels: u8, depth: u8, samples: u64, comments: &[&str]) -> Vec<u8> {
        let mut f = b"fLaC".to_vec();
        let mut si = vec![0u8; 34];
        si[10] = (rate >> 12) as u8;
        si[11] = (rate >> 4) as u8;
        si[12] = (((rate & 0xF) << 4) as u8) | ((channels - 1) << 1) | ((depth - 1) >> 4);
        si[13] = (((depth - 1) & 0xF) << 4) | ((samples >> 32) as u8 & 0x0F);
        si[14..18].copy_from_slice(&(samples as u32).to_be_bytes());
        f.extend_from_slice(&[0x00, 0, 0, 34]);
        f.extend(&si);
        let mut vc = Vec::new();
        vc.extend_from_slice(&4u32.to_le_bytes());
        vc.extend_from_slice(b"test");
        vc.extend_from_slice(&(comments.len() as u32).to_le_bytes());
        for c in comments {
            vc.extend_from_slice(&(c.len() as u32).to_le_bytes());
            vc.extend_from_slice(c.as_bytes());
        }
        let n = vc.len() as u32;
        f.extend_from_slice(&[0x84, (n >> 16) as u8, (n >> 8) as u8, n as u8]);
        f.extend(&vc);
        f.extend(std::iter::repeat_n(0, 1000));
        f
    }

    #[test]
    fn a_flac_file_reads_its_streaminfo_and_its_own_tags() {
        let file = flac(
            48_000,
            2,
            24,
            480_000,
            &[
                "TITLE=Song",
                "artist=Someone",
                "TRACKNUMBER=4/9",
                "ARTIST=Second",
            ],
        );
        let (info, tags) = read_bytes(&file);
        assert_eq!(info.format, AudioFormat::Flac);
        assert_eq!(info.sample_rate, Some(48_000));
        assert_eq!(info.channels, Some(2));
        assert_eq!(info.bits_per_sample, Some(24));
        assert_eq!(info.duration_secs, Some(10.0));
        assert_eq!(tags.title.as_deref(), Some("Song"));
        assert_eq!(
            tags.artist.as_deref(),
            Some("Someone"),
            "keys are matched without case, the first kept"
        );
        assert_eq!(tags.track, Some(4));
    }

    #[test]
    fn a_flac_behind_an_id3_tag_is_a_flac_and_both_tags_are_read() {
        let mut file = id3_tag(3, &[id3_frame(3, b"TALB", 0, b"From ID3")], 0);
        file.extend(flac(44_100, 2, 16, 441_000, &["TITLE=From FLAC"]));
        let (info, tags) = read_bytes(&file);
        assert_eq!(
            info.format,
            AudioFormat::Flac,
            "taken for an MP3 by its tag"
        );
        assert_eq!(info.duration_secs, Some(10.0));
        assert_eq!(tags.title.as_deref(), Some("From FLAC"));
        assert_eq!(
            tags.album.as_deref(),
            Some("From ID3"),
            "the ID3 tag's fields were not used"
        );
    }

    #[test]
    fn a_lone_sync_before_the_first_frame_is_not_a_frame() {
        // Junk with a header-like pattern at its start, then real frames: the
        // pattern's "next frame" is junk, so it is passed over.
        let mut file = vec![0xFF, 0xFB, 0x50, 0x00];
        file.extend(std::iter::repeat_n(0x11, 40));
        let start = file.len();
        file.extend(mp3_frames(5));
        let (info, _) = read_bytes(&file);
        assert_eq!(
            info.bitrate_kbps,
            Some(128),
            "the junk's header was taken for the first frame"
        );
        let secs = info.duration_secs.unwrap();
        let want = (file.len() - start) as f64 * 8.0 / 128_000.0;
        assert!((secs - want).abs() < 1e-6, "{secs} vs {want}");
    }

    #[test]
    fn a_32_bit_flac_is_32_bits() {
        let (info, _) = read_bytes(&flac(44_100, 1, 32, 44_100, &[]));
        assert_eq!(
            info.bits_per_sample,
            Some(32),
            "the depth's high bit was added to wrongly"
        );
        assert_eq!(info.channels, Some(1));
    }

    /// An Ogg page holding `packets` (each under 255 bytes), with `granule`.
    fn ogg_page(packets: &[Vec<u8>], granule: u64) -> Vec<u8> {
        let mut p = b"OggS".to_vec();
        p.extend_from_slice(&[0, 0]);
        p.extend_from_slice(&granule.to_le_bytes());
        p.extend_from_slice(&[0; 12]);
        p.push(packets.len() as u8);
        for packet in packets {
            p.push(packet.len() as u8);
        }
        for packet in packets {
            p.extend(packet);
        }
        p
    }

    #[test]
    fn an_ogg_vorbis_file_is_timed_by_its_last_page_and_read_for_its_tags() {
        let mut ident = b"\x01vorbis".to_vec();
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.push(2);
        ident.extend_from_slice(&44_100u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&160_000u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.push(0);
        let mut comment = b"\x03vorbis".to_vec();
        comment.extend_from_slice(&0u32.to_le_bytes());
        comment.extend_from_slice(&1u32.to_le_bytes());
        let c = "ALBUM=\u{c9}t\u{e9}";
        comment.extend_from_slice(&(c.len() as u32).to_le_bytes());
        comment.extend_from_slice(c.as_bytes());
        let mut file = ogg_page(&[ident], 0);
        file.extend(ogg_page(&[comment], 0));
        file.extend(ogg_page(&[vec![0; 100]], 441_000));
        let (info, tags) = read_bytes(&file);
        assert_eq!(info.format, AudioFormat::Ogg);
        assert_eq!(info.sample_rate, Some(44_100));
        assert_eq!(info.channels, Some(2));
        assert_eq!(info.bitrate_kbps, Some(160));
        assert_eq!(info.duration_secs, Some(10.0));
        assert_eq!(tags.album.as_deref(), Some("\u{c9}t\u{e9}"));
    }

    #[test]
    fn an_opus_file_counts_its_granule_at_48_khz_less_its_pre_skip() {
        let mut head = b"OpusHead".to_vec();
        head.push(1);
        head.push(2);
        head.extend_from_slice(&312u16.to_le_bytes());
        head.extend_from_slice(&44_100u32.to_le_bytes());
        head.extend_from_slice(&[0, 0, 0]);
        let mut tags_packet = b"OpusTags".to_vec();
        tags_packet.extend_from_slice(&0u32.to_le_bytes());
        tags_packet.extend_from_slice(&1u32.to_le_bytes());
        tags_packet.extend_from_slice(&7u32.to_le_bytes());
        tags_packet.extend_from_slice(b"TITLE=O");
        let mut file = ogg_page(&[head], 0);
        file.extend(ogg_page(&[tags_packet], 0));
        file.extend(ogg_page(&[vec![0; 10]], 96_312));
        let (info, tags) = read_bytes(&file);
        assert_eq!(info.duration_secs, Some(2.0));
        assert_eq!(info.sample_rate, Some(48_000));
        assert_eq!(tags.title.as_deref(), Some("O"));
    }

    #[test]
    fn a_wav_file_reads_its_format_its_length_and_its_info_tags() {
        let mut data = Vec::new();
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&2u16.to_le_bytes());
        fmt.extend_from_slice(&8000u32.to_le_bytes());
        fmt.extend_from_slice(&32_000u32.to_le_bytes());
        fmt.extend_from_slice(&4u16.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        data.extend(&fmt);
        let mut info_list = b"INFO".to_vec();
        for (id, text) in [(b"INAM", "Title"), (b"IART", "Band")] {
            info_list.extend_from_slice(id);
            let t = format!("{text}\0");
            info_list.extend_from_slice(&(t.len() as u32).to_le_bytes());
            info_list.extend_from_slice(t.as_bytes());
            if t.len() % 2 == 1 {
                info_list.push(0);
            }
        }
        data.extend_from_slice(b"LIST");
        data.extend_from_slice(&(info_list.len() as u32).to_le_bytes());
        data.extend(&info_list);
        data.extend_from_slice(b"data");
        data.extend_from_slice(&64_000u32.to_le_bytes());
        data.extend(std::iter::repeat_n(0, 64_000));
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&((data.len() + 4) as u32).to_le_bytes());
        file.extend_from_slice(b"WAVE");
        file.extend(&data);
        let (info, tags) = read_bytes(&file);
        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(info.sample_rate, Some(8000));
        assert_eq!(info.channels, Some(2));
        assert_eq!(info.bits_per_sample, Some(16));
        assert_eq!(info.duration_secs, Some(2.0));
        assert_eq!(info.bitrate_kbps, Some(256));
        assert_eq!(tags.title.as_deref(), Some("Title"));
        assert_eq!(tags.artist.as_deref(), Some("Band"));
    }

    #[test]
    fn what_is_not_audio_says_nothing_and_a_cut_file_does_not_panic() {
        let (info, tags) = read_bytes(b"not audio at all, just words");
        assert_eq!(info, AudioInfo::default());
        assert_eq!(tags, Tags::default());
        // Every prefix of every fixture: a file cut anywhere.
        let mut id3 = id3_tag(3, &[id3_frame(3, b"TIT2", 0, b"x")], 10);
        id3.extend(mp3_frames(2));
        for file in [
            id3,
            flac(44_100, 2, 16, 44_100, &["TITLE=t"]),
            mp3_frames(3),
        ] {
            for cut in 0..file.len().min(600) {
                let _ = read_bytes(&file[..cut]);
            }
        }
    }

    /// A chunk claiming the whole address space ends the read: the advance
    /// is saturating, and a read past the end is empty -- this test would
    /// never finish on a parser whose cursor wrapped back to the start.
    #[test]
    fn a_wav_with_an_absurd_chunk_size_does_not_loop() {
        let mut data = Vec::new();
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(b"WAVE");
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        data.resize(64, 0);
        let (info, _) = read_bytes(&data);
        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(info.duration_secs, None);
    }

    /// An ID3 extended header claiming a huge size ends the tag, and a frame
    /// claiming more than the tag holds ends it too.
    #[test]
    fn an_id3_tag_that_lies_about_its_sizes_ends_the_read() {
        let mut data = b"ID3".to_vec();
        data.extend_from_slice(&[4, 0, 0x40, 0, 0, 0x01, 0x00]);
        data.extend_from_slice(&[0x7F, 0x7F, 0x7F, 0x7F]);
        data.resize(200, 0);
        let (_, tags) = read_bytes(&data);
        assert_eq!(tags, Tags::default());
        let mut frame = id3_frame(3, b"TIT2", 0, b"short");
        frame[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        let (_, tags) = read_bytes(&id3_tag(3, &[frame], 0));
        assert_eq!(tags.title, None, "a frame longer than its tag was read");
    }

    #[test]
    fn a_control_character_inside_a_field_is_a_space() {
        let file = flac(
            44_100,
            2,
            16,
            44_100,
            &["TITLE=Line one\nline two", "ARTIST=\tTabbed\x1b[31m\r\n"],
        );
        let (_, tags) = read_bytes(&file);
        assert_eq!(tags.title.as_deref(), Some("Line one line two"));
        // Trimmed at the ends first, so the tab and line break are gone and
        // only the escape inside is a space.
        assert_eq!(tags.artist.as_deref(), Some("Tabbed [31m"));
    }

    /// The files [`testing`] makes are what they are made as: a fixture that
    /// is wrong makes a correct reader look broken elsewhere.
    #[test]
    fn the_test_files_read_as_what_they_were_made_as() {
        let wav = crate::testing::wav(
            22_050,
            1,
            16,
            3,
            &[
                (b"INAM", "A Title"),
                (b"IART", "An Artist"),
                (b"IPRD", "An Album"),
            ],
        );
        let (info, tags) = read_bytes(&wav);
        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(info.sample_rate, Some(22_050));
        assert_eq!(info.channels, Some(1));
        assert_eq!(info.bits_per_sample, Some(16));
        assert_eq!(info.duration_secs, Some(3.0));
        assert_eq!(info.bitrate_kbps, Some(353));
        assert_eq!(tags.title.as_deref(), Some("A Title"));
        assert_eq!(tags.artist.as_deref(), Some("An Artist"));
        assert_eq!(tags.album.as_deref(), Some("An Album"));

        // An odd-length INFO value takes a pad byte; the chunk after it is
        // still found.
        let wav = crate::testing::wav(8000, 2, 8, 1, &[(b"INAM", "Odd"), (b"IART", "Even")]);
        let (info, tags) = read_bytes(&wav);
        assert_eq!(info.duration_secs, Some(1.0));
        assert_eq!(tags.title.as_deref(), Some("Odd"));
        assert_eq!(tags.artist.as_deref(), Some("Even"));

        let mp3 = crate::testing::mp3(
            100,
            &[
                (b"TIT2", "Caf\u{e9}"),
                (b"TPE1", "The Band"),
                (b"TALB", "Songs"),
            ],
        );
        let (info, tags) = read_bytes(&mp3);
        assert_eq!(info.format, AudioFormat::Mp3);
        assert_eq!(info.sample_rate, Some(44_100));
        assert_eq!(info.bitrate_kbps, Some(128));
        // Timed by its size: 100 frames of 417 bytes at 128 kbps.
        let secs = info.duration_secs.unwrap();
        assert!(
            (secs - 100.0 * 417.0 * 8.0 / 128_000.0).abs() < 1e-6,
            "{secs}"
        );
        assert_eq!(tags.title.as_deref(), Some("Caf\u{e9}"));
        assert_eq!(tags.artist.as_deref(), Some("The Band"));
        assert_eq!(tags.album.as_deref(), Some("Songs"));

        // No tags: no ID3 tag at all, and the frames alone.
        let bare = crate::testing::mp3(10, &[]);
        assert_eq!(bare, mp3_frames(10));
    }

    #[test]
    fn a_synchsafe_integer_is_seven_bits_a_byte_and_a_high_bit_is_refused() {
        assert_eq!(synchsafe(&[0x00, 0x00, 0x02, 0x01]), Some(257));
        assert_eq!(synchsafe(&[0x00, 0x00, 0x00, 0x7F]), Some(127));
        assert_eq!(synchsafe(&[0x00, 0x00, 0x01, 0x00]), Some(128));
        assert_eq!(synchsafe(&[0x80, 0x00, 0x00, 0x00]), None);
        assert_eq!(synchsafe(&[0x00, 0x00]), None);
    }

    #[test]
    fn a_genre_number_is_its_name_and_a_name_is_itself() {
        assert_eq!(genre(String::from("17")), "Rock");
        assert_eq!(genre(String::from("(8)Jazz")), "Jazz");
        assert_eq!(genre(String::from("Shoegaze")), "Shoegaze");
        assert_eq!(genre(String::from("(250)")), "(250)");
    }
}
