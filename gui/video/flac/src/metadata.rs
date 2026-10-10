//! A FLAC stream's metadata blocks (RFC 9639 §8): its `STREAMINFO`, seek
//! table, Vorbis comments (the tags: title, artist, ...), pictures (cover
//! art), cue sheet, and whatever else, as libFLAC 1.5.0's `stream_decoder.c`
//! reads them (`read_metadata_*`), translated into Rust -- copyright Josh
//! Coalson and Xiph.Org, BSD (`licenses/flac-COPYING`).
//!
//! A block whose contents do not fit its stated length, or break its own
//! rules, ends the metadata as libFLAC ends it (`BAD_METADATA`): what was read
//! before it stands, and the search for the first frame starts where its
//! reading stopped.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "offsets within a block of at most 16 MiB, each checked against its length before it is added"
)]

use crate::frame::Defaults;

/// The stream's `STREAMINFO`: what every frame shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    /// The fewest and most samples a channel in a block; equal for a
    /// stream of fixed block size.
    pub min_block_size: u32,
    pub max_block_size: u32,
    /// The smallest and largest frame in bytes; 0 where unknown.
    pub min_frame_size: u32,
    pub max_frame_size: u32,
    pub sample_rate: u32,
    /// 1 to 8.
    pub channels: u32,
    /// 4 to 32.
    pub bits_per_sample: u32,
    /// Samples a channel in the stream; 0 where unknown.
    pub total_samples: u64,
    /// The MD5 of the decoded samples (each little-endian, as many bytes as
    /// its bit depth needs, channels interleaved); all zeros where unknown.
    pub md5: [u8; 16],
}

impl StreamInfo {
    /// A `STREAMINFO` block's body: its first 34 bytes (more are allowed and
    /// passed over, as libFLAC passes them over); `None` for fewer.
    #[allow(clippy::indexing_slicing, reason = "ranges within the 34 bytes taken")]
    pub fn parse(body: &[u8]) -> Option<Self> {
        let b = body.get(..34)?;
        let be = |r: std::ops::Range<usize>| b[r].iter().fold(0u64, |v, &x| v << 8 | u64::from(x));
        let packed = be(10..18);
        let mut md5 = [0u8; 16];
        md5.copy_from_slice(b.get(18..34)?);
        Some(Self {
            min_block_size: be(0..2) as u32,
            max_block_size: be(2..4) as u32,
            min_frame_size: be(4..7) as u32,
            max_frame_size: be(7..10) as u32,
            sample_rate: (packed >> 44) as u32,
            channels: ((packed >> 41) & 7) as u32 + 1,
            bits_per_sample: ((packed >> 36) & 31) as u32 + 1,
            total_samples: packed & 0xf_ffff_ffff,
            md5,
        })
    }

    pub(crate) fn defaults(&self) -> Defaults {
        Defaults {
            min_block_size: self.min_block_size,
            max_block_size: self.max_block_size,
            sample_rate: self.sample_rate,
            bits_per_sample: self.bits_per_sample,
        }
    }
}

/// A seek table's point: where a frame starts, and its first sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeekPoint {
    /// The frame's first sample; `u64::MAX` for a placeholder.
    pub sample_number: u64,
    /// Where the frame starts, from the first frame's start.
    pub stream_offset: u64,
    /// Samples a channel in the frame.
    pub frame_samples: u32,
}

/// A Vorbis comment block: the encoder's name and the tags, `NAME=value`
/// each, as the bytes they are (UTF-8 by the format's rules, which a file
/// may break).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comments {
    pub vendor: Vec<u8>,
    pub comments: Vec<Vec<u8>>,
}

impl Comments {
    /// The values of the tags named `name` (compared without regard to
    /// ASCII case, as the format names them), in order.
    pub fn get<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.comments.iter().filter_map(move |c| {
            let eq = c.iter().position(|&b| b == b'=')?;
            let (key, value) = c.split_at(eq);
            key.eq_ignore_ascii_case(name.as_bytes())
                .then(|| value.get(1..).unwrap_or_default())
        })
    }
}

/// A picture block: cover art, or another of ID3's picture kinds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picture {
    /// ID3v2's picture type: 3 is the front cover. Past 20, 0 ("other"), as
    /// libFLAC reads it.
    pub kind: u32,
    /// The MIME type, ASCII: `image/jpeg`, `image/png`, or `-->` where the
    /// data is a URL.
    pub mime: Vec<u8>,
    pub description: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub colors: u32,
    /// The picture's file, as stored.
    pub data: Vec<u8>,
}

/// A metadata block, as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    StreamInfo(StreamInfo),
    /// Padding, this many bytes.
    Padding(u32),
    Application {
        id: [u8; 4],
        data: Vec<u8>,
    },
    SeekTable(Vec<SeekPoint>),
    Comments(Comments),
    /// A cue sheet's body, unparsed (checked as libFLAC checks it).
    CueSheet(Vec<u8>),
    Picture(Picture),
    /// A block of a type the format reserves, its body as it is.
    Other {
        kind: u8,
        data: Vec<u8>,
    },
}

/// Everything a stream's metadata says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub stream_info: Option<StreamInfo>,
    /// The last seek table's points.
    pub seek_table: Vec<SeekPoint>,
    /// The tags: the last Vorbis comment block's.
    pub comments: Option<Comments>,
    pub pictures: Vec<Picture>,
    /// Every block read, in order.
    pub blocks: Vec<Block>,
}

/// A block body read to its end, or how far its reading got before it broke
/// its rules (libFLAC's `BAD_METADATA`).
pub(crate) type Parsed = Result<Block, usize>;

/// A block body of type `kind` (`read_metadata_` past its header).
pub(crate) fn parse_block(kind: u8, body: &[u8]) -> Parsed {
    match kind {
        0 => StreamInfo::parse(body).map(Block::StreamInfo).ok_or(0),
        1 => Ok(Block::Padding(
            u32::try_from(body.len()).unwrap_or(u32::MAX),
        )),
        2 => {
            let id: [u8; 4] = body
                .get(..4)
                .and_then(|b| b.try_into().ok())
                .ok_or(0usize)?;
            Ok(Block::Application {
                id,
                data: body.get(4..).unwrap_or_default().to_vec(),
            })
        }
        3 => seek_table(body),
        4 => comments(body),
        5 => cue_sheet(body),
        6 => picture(body),
        _ => Ok(Block::Other {
            kind,
            data: body.to_vec(),
        }),
    }
}

/// `read_metadata_seektable_`: whole 18-byte points, or none.
#[allow(clippy::indexing_slicing, reason = "ranges within an 18-byte point")]
fn seek_table(body: &[u8]) -> Parsed {
    if !body.len().is_multiple_of(18) {
        return Err(0);
    }
    let points = body
        .chunks_exact(18)
        .map(|p| {
            let be =
                |r: std::ops::Range<usize>| p[r].iter().fold(0u64, |v, &x| v << 8 | u64::from(x));
            SeekPoint {
                sample_number: be(0..8),
                stream_offset: be(8..16),
                frame_samples: be(16..18) as u32,
            }
        })
        .collect();
    Ok(Block::SeekTable(points))
}

/// `read_metadata_vorbiscomment_`: little-endian lengths, each bounded by
/// what is left of the block.
fn comments(body: &[u8]) -> Parsed {
    let le32 = |at: usize| -> Option<usize> {
        let b: [u8; 4] = body.get(at..at + 4)?.try_into().ok()?;
        usize::try_from(u32::from_le_bytes(b)).ok()
    };
    if body.len() < 8 {
        return Err(0);
    }
    // What is left, the vendor length and the comment count set aside.
    let mut left = body.len() - 8;
    let vendor_len = le32(0).ok_or(0usize)?;
    if left < vendor_len {
        // libFLAC keeps no vendor, and stops: the rest is not as stated.
        return Err(4);
    }
    left -= vendor_len;
    let vendor = body.get(4..4 + vendor_len).ok_or(4usize)?.to_vec();
    let mut at = 4 + vendor_len;
    let count = le32(at).ok_or(at)?;
    at += 4;
    if count > 100_000 {
        return Err(at);
    }
    let mut out = Comments {
        vendor,
        comments: Vec::new(),
    };
    for _ in 0..count {
        if left < 4 {
            break;
        }
        left -= 4;
        let len = le32(at).ok_or(at)?;
        at += 4;
        if left < len {
            return Err(at);
        }
        left -= len;
        out.comments
            .push(body.get(at..at + len).ok_or(at)?.to_vec());
        at += len;
    }
    if left > 0 {
        return Err(at);
    }
    Ok(Block::Comments(out))
}

/// `read_metadata_cuesheet_`, checked for fitting its block: 396 bytes,
/// then 36 a track and 12 an index point; at least one track.
fn cue_sheet(body: &[u8]) -> Parsed {
    let tracks = usize::from(*body.get(395).ok_or(body.len())?);
    if tracks == 0 {
        return Err(396);
    }
    let mut at = 396;
    for _ in 0..tracks {
        let indices = usize::from(*body.get(at + 35).ok_or(body.len())?);
        at += 36 + 12 * indices;
        if at > body.len() {
            return Err(body.len());
        }
    }
    if at != body.len() {
        return Err(at);
    }
    Ok(Block::CueSheet(body.to_vec()))
}

/// `read_metadata_picture_`: big-endian lengths, each bounded by what is
/// left of the block.
fn picture(body: &[u8]) -> Parsed {
    let mut at = 0usize;
    let word = |at: &mut usize| -> Result<u32, usize> {
        let b: [u8; 4] = body
            .get(*at..*at + 4)
            .and_then(|b| b.try_into().ok())
            .ok_or(body.len())?;
        *at += 4;
        Ok(u32::from_be_bytes(b))
    };
    let bytes = |at: &mut usize| -> Result<Vec<u8>, usize> {
        let len = word(at)? as usize;
        let v = body
            .get(*at..)
            .and_then(|rest| rest.get(..len))
            .ok_or(*at)?
            .to_vec();
        *at += len;
        Ok(v)
    };
    let kind = word(&mut at)?;
    let mime = bytes(&mut at)?;
    let description = bytes(&mut at)?;
    let width = word(&mut at)?;
    let height = word(&mut at)?;
    let depth = word(&mut at)?;
    let colors = word(&mut at)?;
    let data = bytes(&mut at)?;
    if at != body.len() {
        return Err(at);
    }
    Ok(Block::Picture(Picture {
        kind: if kind <= 20 { kind } else { 0 },
        mime,
        description,
        width,
        height,
        depth,
        colors,
        data,
    }))
}

impl Metadata {
    /// Adds a block read: kept in order, and the facts it carries noted.
    pub(crate) fn add(&mut self, block: Block) {
        match &block {
            Block::StreamInfo(s) => self.stream_info = Some(*s),
            Block::SeekTable(points) => self.seek_table.clone_from(points),
            Block::Comments(c) => self.comments = Some(c.clone()),
            Block::Picture(p) => self.pictures.push(p.clone()),
            _ => {}
        }
        self.blocks.push(block);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    fn streaminfo_body() -> Vec<u8> {
        let mut b = vec![0x10, 0x00, 0x10, 0x00, 0, 0, 14, 0, 0x30, 0x00];
        // 44100 Hz, 2 channels, 16 bits, 1000 samples.
        let packed: u64 = (44_100 << 44) | (1 << 41) | (15 << 36) | 1000;
        b.extend_from_slice(&packed.to_be_bytes());
        b.extend_from_slice(&[7; 16]);
        b
    }

    #[test]
    fn streaminfo_reads_back() {
        let s = StreamInfo::parse(&streaminfo_body()).unwrap();
        assert_eq!((s.min_block_size, s.max_block_size), (4096, 4096));
        assert_eq!((s.min_frame_size, s.max_frame_size), (14, 0x3000));
        assert_eq!(
            (
                s.sample_rate,
                s.channels,
                s.bits_per_sample,
                s.total_samples
            ),
            (44_100, 2, 16, 1000)
        );
        assert_eq!(s.md5, [7; 16]);
        assert!(StreamInfo::parse(&streaminfo_body()[..33]).is_none());
    }

    #[test]
    fn comments_and_their_damage() {
        let mut b = Vec::new();
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(b"enc");
        b.extend_from_slice(&2u32.to_le_bytes());
        for c in [&b"TITLE=One"[..], b"artist=Two"] {
            b.extend_from_slice(&(c.len() as u32).to_le_bytes());
            b.extend_from_slice(c);
        }
        let Ok(Block::Comments(c)) = comments(&b) else {
            panic!()
        };
        assert_eq!(c.vendor, b"enc");
        assert_eq!(c.get("title").collect::<Vec<_>>(), vec![&b"One"[..]]);
        assert_eq!(c.get("ARTIST").collect::<Vec<_>>(), vec![&b"Two"[..]]);
        // A comment longer than the block: refused where it is.
        let mut bad = b.clone();
        bad.truncate(b.len() - 2);
        assert!(comments(&bad).is_err());
        // Bytes past the comments: refused.
        let mut extra = b.clone();
        extra.push(0);
        assert!(comments(&extra).is_err());
    }

    #[test]
    fn a_picture_reads_back_and_must_fill_its_block() {
        let mut b = Vec::new();
        b.extend_from_slice(&3u32.to_be_bytes());
        b.extend_from_slice(&10u32.to_be_bytes());
        b.extend_from_slice(b"image/jpeg");
        b.extend_from_slice(&0u32.to_be_bytes());
        for v in [600u32, 600, 24, 0] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b.extend_from_slice(&3u32.to_be_bytes());
        b.extend_from_slice(&[1, 2, 3]);
        let Ok(Block::Picture(p)) = picture(&b) else {
            panic!()
        };
        assert_eq!(
            (p.kind, p.mime.as_slice(), p.width, p.data.as_slice()),
            (3, &b"image/jpeg"[..], 600, &[1u8, 2, 3][..])
        );
        assert!(picture(&b[..b.len() - 1]).is_err());
        let mut extra = b.clone();
        extra.push(0);
        assert!(picture(&extra).is_err());
    }

    #[test]
    fn a_seek_table_is_whole_points() {
        let mut b = Vec::new();
        b.extend_from_slice(&4096u64.to_be_bytes());
        b.extend_from_slice(&1234u64.to_be_bytes());
        b.extend_from_slice(&4096u16.to_be_bytes());
        let Ok(Block::SeekTable(points)) = seek_table(&b) else {
            panic!()
        };
        assert_eq!(
            points,
            vec![SeekPoint {
                sample_number: 4096,
                stream_offset: 1234,
                frame_samples: 4096
            }]
        );
        assert!(seek_table(&b[..17]).is_err());
    }
}
