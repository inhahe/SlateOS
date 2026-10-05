//! bzip2: the decoder and the compressor, ported from libbzip2 1.0.8.
//!
//! [`decompress`] reads every stream `bzip2 -d` reads and refuses every one it
//! refuses; [`compress`] writes the bytes `bzip2` writes -- not merely a valid
//! stream, the same one, byte for byte, at every block size. This is the codec
//! behind every `.bz2` and `.tar.bz2` on the system.
//!
//! # Provenance
//!
//! A port of bzip2/libbzip2 1.0.8 of 13 July 2019, by Julian Seward: the
//! `bzip2-1.0.8` tree the `bzip2-sys` 0.1.13 crate vendors, read function by
//! function. Its licence is reproduced in `licenses/libbzip2-LICENSE`, which
//! its first condition requires of a redistribution of source, and named in
//! `licenses/notices.yaml` for the image's notices. This is an *altered* version, which its third condition requires
//! saying plainly: the C has been rewritten in Rust, its streaming state
//! machine replaced by whole-buffer calls, its memory-saving "small" decoder
//! left out, and its 16-by-16 move-to-front list replaced by a plain one (the
//! same answers, a different speed trade). Julian Seward wrote the algorithms
//! and their tuning; what is ours is the plumbing around them.
//!
//! | libbzip2 | here |
//! |---|---|
//! | `decompress.c`: `BZ2_decompress`, `makeMaps_d`, `GET_MTF_VAL` | `decompress::decode_stream`, `decompress::decode_block`, `decompress::Coder::symbol` |
//! | `bzlib.c`: `unRLE_obuf_to_output_FAST`, the CRC checks of `BZ2_bzDecompress` | the end of `decompress::decode_block` |
//! | `bzip2.c`: `uncompressStream`'s handling of what follows a stream | [`decompress_limited`] |
//! | 7-Zip's reading of a BZip2 coder in a 7z archive (its rules, not its code) | [`decompress_as_7zip`] |
//! | `bzlib.c`: `ADD_CHAR_TO_BLOCK`, `add_pair_to_block`, `flush_RL`, `handle_compress` | `compress::Encoder` |
//! | `compress.c`: `BZ2_compressBlock`, `generateMTFValues`, `sendMTFValues`, `bsW` | `compress::Encoder::compress_block`, `compress::generate_mtf_values`, `compress::send_mtf_values`, `compress::BitWriter` |
//! | `blocksort.c`: `mainSort`, `mainQSort3`, `mainSimpleSort`, `mainGtU`, `fallbackSort`, `fallbackQSort3`, `fallbackSimpleSort`, `BZ2_blockSort` | `blocksort::Main::{sort, qsort3, simple_sort, gt_u}`, `blocksort::{fallback_sort, fallback_qsort3, fallback_simple_sort}`, `blocksort::Sorter::sort` |
//! | `huffman.c`: `BZ2_hbMakeCodeLengths`, `BZ2_hbAssignCodes`, `BZ2_hbCreateDecodeTables` | `huffman::make_code_lengths`, `huffman::assign_codes`, `huffman::DecodeTable::new` |
//! | `randtable.c`, `crctable.c` | `tables` (transcribed by a script, not typed) |
//!
//! # Why a port, and why this crate
//!
//! It was `kernel/src/fs/bzip2.rs`, and a module of a *binary* crate cannot
//! be depended on, so the archive manager could not open a `.tar.bz2`
//! (`requests/e-a-bzip2-xz-and-7z-are-trapped-in-the-kernel-binary.md`; the
//! fourth time after `deflate`, `ziparchive` and the image codecs). The
//! kernel's copy had been written rather than ported, and moving it would have
//! moved these with it:
//!
//! - **Its compressor made streams nothing could read.** It cut the input into
//!   100 000 x level byte pieces *before* the run-length step, which can make
//!   a piece longer -- four equal bytes become five -- so any input rich in
//!   four-byte runs (`AAAAB` repeated) produced blocks over the size the
//!   header declares, which every decoder, its own included, refuses. libbzip2
//!   fills the block *after* the run-length step, as here.
//! - **It sorted a repetitive block in quadratic time**, comparing equal
//!   rotations byte by byte to the end; libbzip2's sort budgets that and falls
//!   back to a doubling sort that does not care.
//! - **It decoded the first of several concatenated streams and stopped**, so
//!   a file from `pbzip2` -- one stream per block -- came out silently short.
//! - **It had no output limit**: a few dozen bytes can describe tens of
//!   megabytes, and a few kilobytes gigabytes.
//! - **It could not read a randomised block** (bzip2 0.9.0 and earlier), and
//!   refused more than 18 002 selectors, which libbzip2 1.0.8 accepts and
//!   ignores; it accepted an empty block and a run with its count byte
//!   missing, which libbzip2 refuses.
//!
//! # Byte for byte
//!
//! The fixtures in `tests/data` were written by libbzip2 1.0.8 itself
//! (`generate.py` beside them, run against Python's `bz2`, which links that
//! release): streams of text, noise, runs, periodic data that exhausts the
//! main sort's budget, data that the run-length step grows, empty and
//! one-byte inputs, at block sizes 1 to 9. The tests require [`compress`] to
//! reproduce each one exactly and [`decompress`] to read it back; and a corpus
//! of every one-byte corruption of a stream, with libbzip2's verdict on each,
//! requires [`decompress`] to accept exactly the corruptions libbzip2 accepts.
//!
//! # The output limit
//!
//! A bzip2 block of 900 000 bytes can stand for 46 million, and the zero-run
//! codes let a few bytes of block stand for 900 000 of them, so the length a
//! stream decompresses to is not knowable from its size. [`decompress`] caps
//! it at [`MAX_OUTPUT`]; [`decompress_limited`] takes the cap from a caller
//! that knows better, and is checked before each run is appended, not after.
//!
//! # No index, anywhere
//!
//! As in `deflate`, nothing here is indexed with `[i]` outside the tests:
//! reads go through `get`, writes through `get_mut`, and where a miss is
//! impossible the comment says why. This is a parser of untrusted input that
//! a kernel links, where a panic has nothing above it to catch it.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

mod blocksort;
mod compress;
mod decompress;
mod huffman;
mod tables;

/// Everything that can go wrong reading a bzip2 stream.
///
/// libbzip2 answers all but two of these with one code, `BZ_DATA_ERROR`.
/// They are told apart here because they mean different things to the person
/// holding the file: [`Error::UnexpectedEnd`] is a download cut short, worth
/// fetching again; [`Error::BadHeader`] is a file that was never bzip2;
/// the two mismatches are damage, and carry both values so that a caller can
/// say how much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input ended in the middle of a stream. Truncation, usually.
    UnexpectedEnd,
    /// The input does not begin with `BZh` and a block size from `1` to `9`:
    /// it is not a bzip2 stream (libbzip2's `BZ_DATA_ERROR_MAGIC`).
    BadHeader,
    /// Where a block or the end-of-stream marker should begin, neither does.
    BadBlockMagic,
    /// A block's starting rotation lies outside the block.
    BadOrigPtr,
    /// A block's coding tables are malformed: no byte values in use, a table
    /// count outside 2 to 6, no selectors, a selector naming a table that does
    /// not exist, or a code length outside 1 to 20.
    InvalidTables,
    /// A bit pattern no code in the selected table matches, or coded data
    /// running past the last selector.
    InvalidHuffmanCode,
    /// A block decodes to more symbols than the size its stream header
    /// declares, or a run of zeros longer than any block could hold.
    BlockTooLarge,
    /// A block's run-length layer is malformed: four equal bytes with no
    /// count byte after them before the block ends.
    InvalidRun,
    /// A block's bytes do not match the CRC stored in its header.
    BlockCrcMismatch {
        /// The value the block header claims.
        expected: u32,
        /// The value the decompressed bytes give.
        actual: u32,
    },
    /// The blocks' CRCs, combined, do not match the CRC at the end of the
    /// stream: a block was lost, repeated or reordered.
    StreamCrcMismatch {
        /// The value the end-of-stream marker claims.
        expected: u32,
        /// The value the stream's blocks combine to.
        actual: u32,
    },
    /// Decompression reached the caller's output cap. Not necessarily a
    /// malformed stream: it is also what a decompression bomb looks like, and
    /// from inside the decoder the two cannot be told apart.
    OutputTooLarge,
}

/// The text a person sees when a `.bz2` file will not open.
///
/// Here rather than in each caller for the reason `deflate::Error`'s is:
/// [`Error`] is `#[non_exhaustive]`, so a caller's own table needs a wildcard
/// arm that would absorb a variant added later, while this `match` needs none
/// and fails to build until the new variant has words.
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::UnexpectedEnd => f.write_str("bzip2 stream ends unexpectedly"),
            Self::BadHeader => f.write_str("not a bzip2 stream"),
            Self::BadBlockMagic => f.write_str("bzip2 block marker missing"),
            Self::BadOrigPtr => f.write_str("bzip2 block's start position is outside the block"),
            Self::InvalidTables => f.write_str("bzip2 block's coding tables are malformed"),
            Self::InvalidHuffmanCode => f.write_str("undecodable bzip2 Huffman code"),
            Self::BlockTooLarge => f.write_str("bzip2 block is larger than its stream allows"),
            Self::InvalidRun => f.write_str("bzip2 block's run-length data is malformed"),
            Self::BlockCrcMismatch { expected, actual } => write!(
                f,
                "bzip2 block CRC mismatch: block declares {expected:#010x}, \
                 its bytes give {actual:#010x}"
            ),
            Self::StreamCrcMismatch { expected, actual } => write!(
                f,
                "bzip2 stream CRC mismatch: stream declares {expected:#010x}, \
                 its blocks give {actual:#010x}"
            ),
            Self::OutputTooLarge => f.write_str("decompressed size exceeds the caller's limit"),
        }
    }
}

/// Shorthand for this crate's fallible operations.
pub type Result<T> = core::result::Result<T, Error>;

/// The output cap [`decompress`] applies: 256 MiB.
///
/// The same as `xz`'s, and four times `deflate`'s: `.tar.bz2` is a format for
/// source trees and whole distributions, where a few hundred megabytes is an
/// ordinary archive, not an attack. A caller that knows the size it expects --
/// or holds a smaller budget -- passes its own to [`decompress_limited`].
pub const MAX_OUTPUT: usize = 256 * 1024 * 1024;

/// A bzip2 block size, in hundreds of kilobytes: `bzip2 -1` to `bzip2 -9`.
///
/// Bigger blocks compress better and cost more memory on both sides -- about
/// eight bytes per block byte to compress, four to decompress. The level is
/// written into the stream's header, so it is also what every decoder of the
/// stream will have to allocate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Level(u8);

impl Level {
    /// 100 000-byte blocks, `bzip2 -1`.
    pub const FASTEST: Self = Self(1);
    /// 900 000-byte blocks, `bzip2 -9` -- bzip2's own default.
    pub const BEST: Self = Self(9);

    /// The level `n`, if it is one: 1 to 9.
    #[must_use]
    pub const fn new(n: u8) -> Option<Self> {
        if n >= 1 && n <= 9 {
            Some(Self(n))
        } else {
            None
        }
    }

    /// The level as a number, 1 to 9.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Default for Level {
    /// [`Level::BEST`], as `bzip2` with no level given.
    fn default() -> Self {
        Self::BEST
    }
}

/// Compresses `data` into one bzip2 stream, exactly as
/// `bzip2 -<level>` -- libbzip2 1.0.8 with its default work factor -- would.
#[must_use]
pub fn compress(data: &[u8], level: Level) -> Vec<u8> {
    compress::compress(data, level)
}

/// Decompresses a bzip2 file -- one stream, or several concatenated -- up to
/// [`MAX_OUTPUT`] bytes.
///
/// # Errors
///
/// Any [`Error`]; see [`decompress_limited`].
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    decompress_limited(data, MAX_OUTPUT)
}

/// Decompresses a bzip2 file, refusing to produce more than `limit` bytes.
///
/// What follows a stream is read as `bzip2 -d` reads it (`uncompressStream`
/// in `bzip2.c`): another stream is decompressed and appended, so the
/// concatenations `pbzip2` and `cat a.bz2 b.bz2` make come out whole; bytes
/// that cannot begin a stream -- not `BZh` and a digit -- are ignored, as the
/// zeros a tape or a fixed-size download pads with are; and bytes that do
/// begin one must then be a whole, valid stream.
///
/// # Errors
///
/// [`Error::BadHeader`] if `data` does not begin with a bzip2 stream,
/// [`Error::UnexpectedEnd`] if a stream is cut short (including an empty
/// `data`), [`Error::OutputTooLarge`] at the cap, and the rest of [`Error`]
/// for a stream that is damaged.
pub fn decompress_limited(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    decompress::decompress(data, limit)
}

/// Decompresses the bzip2 stream at the start of `data` onto `out` as 7-Zip
/// reads a BZip2 coder of a 7z archive, up to `limit` bytes in `out`, and
/// returns how many bytes of `data` the stream took up.
///
/// 7-Zip's decoder is not libbzip2: it refuses a coding table that is not a
/// prefix code, accepts a block ending in four equal bytes with no count,
/// and reads one stream only -- the rules are listed in the `decompress`
/// module. What follows the stream is left to the caller, which 7-Zip calls
/// data after the end.
///
/// On an error `out` keeps what was decompressed before it -- every block
/// before the damaged one, and that one too when what is wrong is its CRC,
/// which is checked after its bytes are out -- as 7-Zip gives back the
/// files of a damaged solid block that lie before the damage.
///
/// # Errors
///
/// As [`decompress_limited`]; [`Error::OutputTooLarge`] with `out` filled to
/// `limit`.
pub fn decompress_as_7zip(data: &[u8], out: &mut Vec<u8>, limit: usize) -> Result<usize> {
    decompress::decompress_as_7zip(data, out, limit)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn levels_are_one_to_nine() {
        assert_eq!(Level::new(0), None);
        assert_eq!(Level::new(1), Some(Level::FASTEST));
        assert_eq!(Level::new(9), Some(Level::BEST));
        assert_eq!(Level::new(10), None);
        assert_eq!(Level::default(), Level::BEST);
        assert_eq!(Level::new(5).map(Level::get), Some(5));
    }

    #[test]
    fn an_empty_input_is_a_header_and_a_trailer() {
        // What `bzip2 < /dev/null` writes: no blocks, a combined CRC of zero.
        let empty = compress(&[], Level::BEST);
        assert_eq!(empty, b"BZh9\x17\x72\x45\x38\x50\x90\x00\x00\x00\x00");
        assert_eq!(decompress(&empty).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn round_trips_at_every_level() {
        let mut data = Vec::new();
        for i in 0u32..20_000 {
            data.push(b"the quick brown fox jumps over the lazy dog "[(i % 44) as usize]);
            if i % 997 == 0 {
                data.extend_from_slice(&[0x55; 300]);
            }
        }
        for n in 1..=9 {
            let level = Level::new(n).unwrap();
            let packed = compress(&data, level);
            assert_eq!(packed[3], b'0' + n);
            assert_eq!(decompress(&packed).unwrap(), data, "level {n}");
        }
    }

    #[test]
    fn the_limit_is_checked_before_the_output_grows_past_it() {
        let data = vec![7u8; 10_000];
        let packed = compress(&data, Level::BEST);
        assert_eq!(decompress_limited(&packed, 10_000).unwrap(), data);
        assert_eq!(
            decompress_limited(&packed, 9_999),
            Err(Error::OutputTooLarge)
        );
        assert_eq!(decompress_limited(&packed, 0), Err(Error::OutputTooLarge));
    }

    #[test]
    fn every_error_has_words() {
        let all = [
            Error::UnexpectedEnd,
            Error::BadHeader,
            Error::BadBlockMagic,
            Error::BadOrigPtr,
            Error::InvalidTables,
            Error::InvalidHuffmanCode,
            Error::BlockTooLarge,
            Error::InvalidRun,
            Error::BlockCrcMismatch {
                expected: 1,
                actual: 2,
            },
            Error::StreamCrcMismatch {
                expected: 3,
                actual: 4,
            },
            Error::OutputTooLarge,
        ];
        for e in all {
            let text = alloc::format!("{e}");
            assert!(!text.is_empty());
        }
        let crc = alloc::format!(
            "{}",
            Error::BlockCrcMismatch {
                expected: 0x10,
                actual: 0x20
            }
        );
        assert!(
            crc.contains("0x00000010") && crc.contains("0x00000020"),
            "{crc}"
        );
    }
}
