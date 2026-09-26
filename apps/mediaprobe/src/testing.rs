//! Real video files for other crates' tests to read.
//!
//! # Why this is in the library and not in a test module
//!
//! The video player's tests and the file manager's each need an MP4, a
//! Matroska file or an AVI to point themselves at, and a builder in a
//! `#[cfg(test)]` module is invisible outside its own crate. A fixture that is
//! subtly wrong makes a correct reader look broken or, worse, a broken one
//! look correct; one copy, read back by this crate's own tests, is the cure.
//! `imagecodec::testing` and `audiotags::testing` are the same argument.
//!
//! # What it deliberately is not
//!
//! Not an encoder. There are no pictures and no sound: the files are their
//! headers -- every box, element and chunk a reader looks at -- around empty
//! media data. A test of how the readers take a *particular* shape of file
//! lays its bytes out with the builders below in this crate's own tests.

/// A length, as the `u32` a box's or a chunk's header holds.
fn len32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

// ============================================================================
// ISO base media
// ============================================================================

/// A box: its length, its four letters, its body.
pub(crate) fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = len32(body.len().saturating_add(8)).to_be_bytes().to_vec();
    b.extend_from_slice(kind);
    b.extend_from_slice(body);
    b
}

/// A full box: a version, three bytes of flags, then `body`.
pub(crate) fn full_box(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut v = flags.to_be_bytes().to_vec();
    if let Some(first) = v.first_mut() {
        *first = version;
    }
    v.extend_from_slice(body);
    mp4_box(kind, &v)
}

/// A language as `mdhd` packs it: three lowercase letters, five bits each,
/// less 0x60.
pub(crate) fn packed(language: &str) -> u16 {
    language.bytes().take(3).fold(0_u16, |acc, c| {
        acc.checked_shl(5).unwrap_or(0) | u16::from(c.saturating_sub(0x60) & 0x1F)
    })
}

/// One MP4 track, as [`mp4_trak`] lays it out.
pub(crate) struct Mp4Track<'a> {
    pub handler: &'a [u8; 4],
    pub entry: &'a [u8; 4],
    pub width: u16,
    pub height: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub timescale: u32,
    pub duration: u32,
    pub samples: u32,
    pub language: &'a str,
    pub enabled: bool,
    /// For an `mp4a` entry: the `esds` object type (0x40 AAC, 0x6B MP3).
    pub object_type: Option<u8>,
}

/// A `trak`: `tkhd`, then `mdia` holding `mdhd`, `hdlr` and a sample table
/// of one description and a sample count.
pub(crate) fn mp4_trak(t: &Mp4Track) -> Vec<u8> {
    let mut tkhd = vec![0; 80];
    if let Some(d) = tkhd.get_mut(16..20) {
        d.copy_from_slice(&t.duration.to_be_bytes());
    }
    if let Some(w) = tkhd.get_mut(72..76) {
        w.copy_from_slice(&(u32::from(t.width) << 16).to_be_bytes());
    }
    if let Some(h) = tkhd.get_mut(76..80) {
        h.copy_from_slice(&(u32::from(t.height) << 16).to_be_bytes());
    }
    let tkhd = full_box(b"tkhd", 0, u32::from(t.enabled), &tkhd);

    let mut mdhd = vec![0; 8];
    mdhd.extend_from_slice(&t.timescale.to_be_bytes());
    mdhd.extend_from_slice(&t.duration.to_be_bytes());
    mdhd.extend_from_slice(&packed(t.language).to_be_bytes());
    mdhd.extend_from_slice(&[0, 0]);
    let mdhd = full_box(b"mdhd", 0, 0, &mdhd);

    let mut hdlr = vec![0; 4];
    hdlr.extend_from_slice(t.handler);
    hdlr.extend_from_slice(&[0; 12]);
    hdlr.extend_from_slice(b"Handler\0");
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr);

    let mut entry = vec![0, 0, 0, 0, 0, 0, 0, 1];
    if t.handler == b"vide" {
        entry.extend_from_slice(&[0; 16]);
        entry.extend_from_slice(&t.width.to_be_bytes());
        entry.extend_from_slice(&t.height.to_be_bytes());
        entry.extend_from_slice(&[0, 0x48, 0, 0, 0, 0x48, 0, 0]);
        entry.extend_from_slice(&[0; 4]);
        entry.extend_from_slice(&1_u16.to_be_bytes());
        entry.extend_from_slice(&[0; 32]);
        entry.extend_from_slice(&[0, 0x18, 0xFF, 0xFF]);
    } else if t.handler == b"soun" {
        entry.extend_from_slice(&[0; 8]);
        entry.extend_from_slice(&t.channels.to_be_bytes());
        entry.extend_from_slice(&16_u16.to_be_bytes());
        entry.extend_from_slice(&[0; 4]);
        entry.extend_from_slice(&(t.sample_rate << 16).to_be_bytes());
        if let Some(object_type) = t.object_type {
            // An elementary stream descriptor (3) holding a decoder
            // configuration (4) whose first byte is the object type.
            let esds = [
                0x03,
                0x19,
                0x00,
                0x01,
                0x00,
                0x04,
                0x11,
                object_type,
                0x15,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0x06,
                0x01,
                0x02,
            ];
            entry.extend(full_box(b"esds", 0, 0, &esds));
        }
    }
    let mut stsd = 1_u32.to_be_bytes().to_vec();
    stsd.extend(mp4_box(t.entry, &entry));
    let stsd = full_box(b"stsd", 0, 0, &stsd);
    let mut stsz = 0_u32.to_be_bytes().to_vec();
    stsz.extend_from_slice(&t.samples.to_be_bytes());
    let stsz = full_box(b"stsz", 0, 0, &stsz);
    let stbl = mp4_box(b"stbl", &[stsd, stsz].concat());
    let minf = mp4_box(b"minf", &stbl);
    let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
    mp4_box(b"trak", &[tkhd, mdia].concat())
}

/// An `mvhd` of `duration` at `timescale`.
pub(crate) fn mvhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut b = vec![0; 8];
    b.extend_from_slice(&timescale.to_be_bytes());
    b.extend_from_slice(&duration.to_be_bytes());
    b.extend_from_slice(&[0; 80]);
    full_box(b"mvhd", 0, 0, &b)
}

/// An MP4 of `secs` seconds: an H.264 picture of `width` by `height` at
/// `fps` frames a second, and AAC sound, stereo at 48 kHz, both in English.
#[must_use]
pub fn mp4(width: u16, height: u16, secs: u32, fps: u32) -> Vec<u8> {
    let video = mp4_trak(&Mp4Track {
        handler: b"vide",
        entry: b"avc1",
        width,
        height,
        channels: 0,
        sample_rate: 0,
        timescale: 90_000,
        duration: secs.saturating_mul(90_000),
        samples: secs.saturating_mul(fps),
        language: "eng",
        enabled: true,
        object_type: None,
    });
    let audio = mp4_trak(&Mp4Track {
        handler: b"soun",
        entry: b"mp4a",
        width: 0,
        height: 0,
        channels: 2,
        sample_rate: 48_000,
        timescale: 48_000,
        duration: secs.saturating_mul(48_000),
        samples: secs.saturating_mul(47),
        language: "eng",
        enabled: true,
        object_type: Some(0x40),
    });
    let moov = mp4_box(
        b"moov",
        &[mvhd(1000, secs.saturating_mul(1000)), video, audio].concat(),
    );
    let mut file = mp4_box(b"ftyp", b"isom\0\0\x02\0isomiso2avc1mp41");
    file.extend(moov);
    file.extend(mp4_box(b"mdat", &[]));
    file
}

// ============================================================================
// Matroska
// ============================================================================

/// An EBML id's bytes: the id with its marker, without leading zeros.
fn ebml_id(id: u64) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    bytes.get(first..).unwrap_or(&[]).to_vec()
}

/// An EBML length in eight bytes: the marker, then the value.
fn ebml_size(n: usize) -> [u8; 8] {
    let mut b = u64::try_from(n).unwrap_or(0).to_be_bytes();
    if let Some(first) = b.first_mut() {
        *first = 0x01;
    }
    b
}

/// An element: its id, its length, its body.
pub(crate) fn element(id: u64, body: &[u8]) -> Vec<u8> {
    let mut e = ebml_id(id);
    e.extend_from_slice(&ebml_size(body.len()));
    e.extend_from_slice(body);
    e
}

/// An unsigned integer element, in eight bytes.
pub(crate) fn uint_element(id: u64, value: u64) -> Vec<u8> {
    element(id, &value.to_be_bytes())
}

/// A float element, in eight bytes.
pub(crate) fn float_element(id: u64, value: f64) -> Vec<u8> {
    element(id, &value.to_be_bytes())
}

/// The EBML header naming `doc_type`.
pub(crate) fn ebml_header(doc_type: &str) -> Vec<u8> {
    element(0x1A45_DFA3, &element(0x4282, doc_type.as_bytes()))
}

/// A video `TrackEntry`: `codec` at `width` by `height`, `fps` frames a
/// second.
pub(crate) fn mkv_video(codec: &str, width: u64, height: u64, fps: u64) -> Vec<u8> {
    let video = [uint_element(0xB0, width), uint_element(0xBA, height)].concat();
    let body = [
        uint_element(0xD7, 1),
        uint_element(0x83, 1),
        element(0x86, codec.as_bytes()),
        uint_element(0x23_E383, 1_000_000_000_u64.checked_div(fps).unwrap_or(0)),
        element(0xE0, &video),
    ]
    .concat();
    element(0xAE, &body)
}

/// An audio `TrackEntry`: `codec`, `channels` at `rate`.
pub(crate) fn mkv_audio(codec: &str, channels: u64, rate: f64) -> Vec<u8> {
    let audio = [float_element(0xB5, rate), uint_element(0x9F, channels)].concat();
    let body = [
        uint_element(0xD7, 2),
        uint_element(0x83, 2),
        element(0x86, codec.as_bytes()),
        element(0xE1, &audio),
    ]
    .concat();
    element(0xAE, &body)
}

/// A Segment of `Info` -- `secs` at a millisecond a tick -- and `tracks`,
/// then an empty `Cluster`.
pub(crate) fn mkv_segment(secs: f64, title: Option<&str>, tracks: &[Vec<u8>]) -> Vec<u8> {
    let mut info = [
        uint_element(0x2A_D7B1, 1_000_000),
        float_element(0x4489, secs * 1000.0),
    ]
    .concat();
    if let Some(title) = title {
        info.extend(element(0x7BA9, title.as_bytes()));
    }
    let body = [
        element(0x1549_A966, &info),
        element(0x1654_AE6B, &tracks.concat()),
        element(0x1F43_B675, &uint_element(0xE7, 0)),
    ]
    .concat();
    element(0x1853_8067, &body)
}

fn matroska(doc_type: &str, video: &str, width: u16, height: u16, secs: u32, fps: u32) -> Vec<u8> {
    let tracks = [
        mkv_video(video, u64::from(width), u64::from(height), u64::from(fps)),
        mkv_audio("A_OPUS", 2, 48_000.0),
    ];
    let mut file = ebml_header(doc_type);
    file.extend(mkv_segment(f64::from(secs), None, &tracks));
    file
}

/// A Matroska file of `secs` seconds: an H.264 picture of `width` by
/// `height` at `fps` frames a second, and Opus sound, stereo at 48 kHz.
#[must_use]
pub fn mkv(width: u16, height: u16, secs: u32, fps: u32) -> Vec<u8> {
    matroska("matroska", "V_MPEG4/ISO/AVC", width, height, secs, fps)
}

/// A WebM file: [`mkv`]'s, with a VP9 picture and the WebM document type.
#[must_use]
pub fn webm(width: u16, height: u16, secs: u32, fps: u32) -> Vec<u8> {
    matroska("webm", "V_VP9", width, height, secs, fps)
}

// ============================================================================
// AVI
// ============================================================================

/// A RIFF chunk: its id, its length, its body, and a pad byte after an odd
/// length.
pub(crate) fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut c = id.to_vec();
    c.extend_from_slice(&len32(body.len()).to_le_bytes());
    c.extend_from_slice(body);
    if !body.len().is_multiple_of(2) {
        c.push(0);
    }
    c
}

/// A `LIST` chunk of `kind` holding `body`.
pub(crate) fn avi_list(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = kind.to_vec();
    b.extend_from_slice(body);
    chunk(b"LIST", &b)
}

/// A stream header: its type and handler, then a clock of `scale` ticks a
/// frame at `rate` ticks a second, `length` frames long.
pub(crate) fn strh(
    kind: &[u8; 4],
    handler: &[u8; 4],
    scale: u32,
    rate: u32,
    length: u32,
) -> Vec<u8> {
    let mut b = kind.to_vec();
    b.extend_from_slice(handler);
    b.extend_from_slice(&[0; 12]);
    b.extend_from_slice(&scale.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&0_u32.to_le_bytes());
    b.extend_from_slice(&length.to_le_bytes());
    b.extend_from_slice(&[0; 20]);
    chunk(b"strh", &b)
}

/// A bitmap header of `width` by `height`, compressed as `compression`.
pub(crate) fn bitmap(width: i32, height: i32, compression: &[u8; 4]) -> Vec<u8> {
    let mut b = 40_u32.to_le_bytes().to_vec();
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.extend_from_slice(&1_u16.to_le_bytes());
    b.extend_from_slice(&24_u16.to_le_bytes());
    b.extend_from_slice(compression);
    b.extend_from_slice(&[0; 20]);
    chunk(b"strf", &b)
}

/// A wave format: `tag`, `channels` at `rate`.
pub(crate) fn wave_format(tag: u16, channels: u16, rate: u32) -> Vec<u8> {
    let mut b = tag.to_le_bytes().to_vec();
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&[0; 8]);
    chunk(b"strf", &b)
}

/// A main header: `micros` a frame, `frames` frames, `width` by `height`.
pub(crate) fn avih(micros: u32, frames: u32, width: u32, height: u32) -> Vec<u8> {
    let mut b = micros.to_le_bytes().to_vec();
    b.extend_from_slice(&[0; 12]);
    b.extend_from_slice(&frames.to_le_bytes());
    b.extend_from_slice(&[0; 12]);
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.extend_from_slice(&[0; 16]);
    chunk(b"avih", &b)
}

/// A whole AVI around `header`: RIFF, the header list, and an empty `movi`.
pub(crate) fn avi_file(header: &[u8]) -> Vec<u8> {
    let mut body = b"AVI ".to_vec();
    body.extend(avi_list(b"hdrl", header));
    body.extend(avi_list(b"movi", &[]));
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&len32(body.len()).to_le_bytes());
    file.extend(body);
    file
}

/// An AVI of `secs` seconds: an H.264 picture of `width` by `height` at
/// `fps` frames a second, and MP3 sound, stereo at 44.1 kHz.
#[must_use]
pub fn avi(width: u16, height: u16, secs: u32, fps: u32) -> Vec<u8> {
    let frames = secs.saturating_mul(fps);
    let micros = 1_000_000_u32.checked_div(fps).unwrap_or(0);
    let video = [
        strh(b"vids", b"H264", 1, fps, frames),
        bitmap(i32::from(width), i32::from(height), b"H264"),
    ]
    .concat();
    let audio = [
        strh(b"auds", &[0; 4], 1152, 44_100, secs.saturating_mul(38)),
        wave_format(0x0055, 2, 44_100),
    ]
    .concat();
    let header = [
        avih(micros, frames, u32::from(width), u32::from(height)),
        avi_list(b"strl", &video),
        avi_list(b"strl", &audio),
    ]
    .concat();
    avi_file(&header)
}
