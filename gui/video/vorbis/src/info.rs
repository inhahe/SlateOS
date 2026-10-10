//! The three Vorbis headers (Vorbis I §4.2): identification (channels,
//! rate, block sizes), comments (the tags), and setup (every codebook,
//! floor, residue, mapping and mode the audio packets refer to).
//!
//! Translated into Rust from Tremor's `info.c`, copyright Xiph.Org, used
//! under its BSD licence (`licenses/tremor-COPYING`). Tremor checks the
//! headers' order and Ogg's first-packet flag as it reads them; here each
//! header is read on its own, and the decoder is made from the first and
//! the third (as FFmpeg does, which reads no tags).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "counts from 6- and 8-bit fields, sizes checked against the packet"
)]

use crate::Error;
use crate::bitpack::BitReader;
use crate::codebook::{self, Book};
use crate::floor0::Floor0;
use crate::floor1::Floor1;
use crate::mapping::Mapping;
use crate::residue::Residue;

/// What the identification header says about a stream (`vorbis_info`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    /// 1 to 255.
    pub channels: usize,
    /// Samples a second, a channel.
    pub rate: u32,
    /// The encoder's hints, in bits a second; 0 (or anything at all) where
    /// it gave none.
    pub bitrate_upper: i32,
    pub bitrate_nominal: i32,
    pub bitrate_lower: i32,
    /// The short and the long block's sizes, powers of two from 64 to
    /// 8192, the short no longer than the long.
    pub blocksizes: [usize; 2],
}

/// A comment header's text (`vorbis_comment`): the encoder's name and the
/// tags, `NAME=value` each, as the bytes they are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comments {
    pub vendor: Vec<u8>,
    pub comments: Vec<Vec<u8>>,
}

/// A floor of either type.
#[derive(Clone, Debug)]
pub(crate) enum Floor {
    Zero(Floor0),
    /// Boxed: its class tables are some hundreds of bytes, a floor 0's a few.
    One(Box<Floor1>),
}

/// A mode (`vorbis_info_mode`): a block size and the mapping that decodes
/// it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mode {
    pub blockflag: bool,
    pub mapping: usize,
}

/// Everything the setup header sets up.
#[derive(Clone, Debug)]
pub(crate) struct Setup {
    pub books: Vec<Book>,
    pub floors: Vec<Floor>,
    pub residues: Vec<Residue>,
    pub maps: Vec<Mapping>,
    pub modes: Vec<Mode>,
}

/// A header's common start: its type, then "vorbis".
fn header(packet: &[u8], want: i64) -> Result<BitReader<'_>, Error> {
    let mut opb = BitReader::new(packet);
    let packtype = opb.read(8);
    let mut magic = [0u8; 6];
    for b in &mut magic {
        *b = opb.read(8) as u8;
    }
    if &magic != b"vorbis" {
        return Err(Error::NotVorbis);
    }
    if packtype != want {
        return Err(Error::BadHeader);
    }
    Ok(opb)
}

impl Info {
    /// The identification header (`_vorbis_unpack_info`).
    pub fn parse(packet: &[u8]) -> Result<Self, Error> {
        let mut opb = header(packet, 1)?;
        if opb.read(32) != 0 {
            return Err(Error::Version);
        }
        let channels = opb.read(8);
        let rate = opb.read(32);
        let bitrate_upper = opb.read(32) as i32;
        let bitrate_nominal = opb.read(32) as i32;
        let bitrate_lower = opb.read(32) as i32;
        let bs0 = opb.read(4);
        let bs1 = opb.read(4);
        if rate < 1 || channels < 1 || !(6..=13).contains(&bs0) || !(bs0..=13).contains(&bs1) {
            return Err(Error::BadHeader);
        }
        // The framing bit.
        if opb.read(1) != 1 {
            return Err(Error::BadHeader);
        }
        Ok(Self {
            channels: channels as usize,
            rate: rate as u32,
            bitrate_upper,
            bitrate_nominal,
            bitrate_lower,
            blocksizes: [1 << bs0, 1 << bs1],
        })
    }
}

impl Comments {
    /// The comment header (`_vorbis_unpack_comment`).
    pub fn parse(packet: &[u8]) -> Result<Self, Error> {
        let mut opb = header(packet, 3)?;
        let string = |opb: &mut BitReader<'_>| -> Result<Vec<u8>, Error> {
            // C reads the length as an int: 2^31 and up is negative.
            let len = opb.read(32) as i32;
            if len < 0 || i64::from(len) > opb.storage_left() {
                return Err(Error::BadHeader);
            }
            Ok((0..len).map(|_| opb.read(8) as u8).collect())
        };
        let vendor = string(&mut opb)?;
        let count = opb.read(32) as i32;
        if count < 0 || count == i32::MAX || i64::from(count) > opb.storage_left() >> 2 {
            return Err(Error::BadHeader);
        }
        let mut comments = Vec::with_capacity(count as usize);
        for _ in 0..count {
            comments.push(string(&mut opb)?);
        }
        if opb.read(1) != 1 {
            return Err(Error::BadHeader);
        }
        Ok(Self { vendor, comments })
    }
}

impl Setup {
    /// The setup header (`_vorbis_unpack_books`), for a stream of
    /// `channels`; each book readied for decoding as it is read, which
    /// Tremor does when the decoder is made (refusing the stream either
    /// way).
    pub(crate) fn parse(packet: &[u8], channels: usize) -> Result<Self, Error> {
        let mut opb = header(packet, 5)?;
        let bad = Error::BadHeader;
        let count = opb.read(8) + 1;
        if count <= 0 {
            return Err(bad);
        }
        let mut books = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let s = codebook::unpack(&mut opb).ok_or(bad)?;
            books.push(codebook::init_decode(&s).ok_or(bad)?);
        }
        // Vorbis I has no time backend: each must be type 0.
        let times = opb.read(6) + 1;
        if times <= 0 {
            return Err(bad);
        }
        for _ in 0..times {
            if opb.read(16) != 0 {
                return Err(bad);
            }
        }
        let count = opb.read(6) + 1;
        if count <= 0 {
            return Err(bad);
        }
        let mut floors = Vec::with_capacity(count as usize);
        for _ in 0..count {
            floors.push(match opb.read(16) {
                0 => Floor::Zero(Floor0::unpack(&mut opb, &books).ok_or(bad)?),
                1 => Floor::One(Box::new(Floor1::unpack(&mut opb, books.len()).ok_or(bad)?)),
                _ => return Err(bad),
            });
        }
        let count = opb.read(6) + 1;
        if count <= 0 {
            return Err(bad);
        }
        let mut residues = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let kind = opb.read(16);
            if !(0..3).contains(&kind) {
                return Err(bad);
            }
            residues.push(Residue::unpack(kind as u8, &mut opb, &books).ok_or(bad)?);
        }
        let count = opb.read(6) + 1;
        if count <= 0 {
            return Err(bad);
        }
        let mut maps = Vec::with_capacity(count as usize);
        for _ in 0..count {
            if opb.read(16) != 0 {
                return Err(bad);
            }
            maps.push(
                Mapping::unpack(&mut opb, channels, times, floors.len(), residues.len())
                    .ok_or(bad)?,
            );
        }
        let count = opb.read(6) + 1;
        if count <= 0 {
            return Err(bad);
        }
        let mut modes = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let blockflag = opb.read(1);
            let windowtype = opb.read(16);
            let transformtype = opb.read(16);
            let mapping = opb.read(8);
            // Tremor tests the window and transform types only from above,
            // so an end of the packet is caught by the mapping's test.
            if windowtype >= 1 || transformtype >= 1 || mapping < 0 || mapping >= maps.len() as i64
            {
                return Err(bad);
            }
            modes.push(Mode {
                blockflag: blockflag != 0,
                mapping: mapping as usize,
            });
        }
        if opb.read(1) != 1 {
            return Err(bad);
        }
        Ok(Self {
            books,
            floors,
            residues,
            maps,
            modes,
        })
    }
}
