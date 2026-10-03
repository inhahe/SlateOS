//! xz: the `.xz` and `.lzma` formats and raw LZMA/LZMA2, ported from liblzma
//! 5.2.5.
//!
//! [`decompress`] reads every `.xz` file `xz -d` reads and refuses every one
//! it refuses: concatenated streams and stream padding, every check type
//! (CRC-32, CRC-64 and SHA-256 verified; the format's other check IDs
//! skipped, as `xz` skips them), the delta filter and the six branch
//! converters. [`decompress_lzma`] reads `.lzma` files; [`lzma1`] and
//! [`lzma2`] decode the raw streams 7-Zip archives hold.
//!
//! # Provenance
//!
//! A port of liblzma from XZ Utils 5.2.5 (the `xz-5.2` tree the `lzma-sys`
//! 0.1.20 crate vendors), by Lasse Collin and Igor Pavlov, who put it in the
//! public domain; this port, as a courtesy their `COPYING` asks for and does
//! not require, says so. 5.2.5 is from 2020, before the 5.6.0 and 5.6.1
//! releases that carried a backdoor, and nothing here came from those.
//!
//! | liblzma | here |
//! |---|---|
//! | `rangecoder/range_decoder.h`, `lzma/lzma_decoder.c` | `lzma` |
//! | `lzma/lzma2_decoder.c` | `lzma2` |
//! | `lz/lz_decoder.c`, `lz_decoder.h` | `lzma::Dict` (the dictionary is the output) |
//! | `common/stream_decoder.c`, `block_decoder.c`, `block_header_decoder.c`, `index_hash.c`, `stream_flags_decoder.c`, `filter_flags_decoder.c` | `stream` |
//! | `common/alone_decoder.c` (and `xz`'s trailing-garbage check) | `alone` |
//! | `common/vli_decoder.c`, `vli_encoder.c`, `vli_size.c` | `vli` |
//! | `common/filter_common.c`, `delta/`, `simple/` | `filters` |
//! | `check/check.c`, `crc64_*.c` | `check` (CRC-32 from the `crc32` crate, SHA-256 from `sha2`) |
//!
//! # Why a port, and why this crate
//!
//! It was `kernel/src/fs/xz.rs`, which no other crate could depend on
//! (`requests/e-a-bzip2-xz-and-7z-are-trapped-in-the-kernel-binary.md`). That
//! copy had been written rather than ported, and read less: no branch
//! converters or delta filter (so no `.xz` of an executable made with
//! `xz --x86`), no SHA-256 check, no `.lzma`.
//!
//! # Refused as liblzma refuses
//!
//! liblzma is a state machine fed a buffer at a time. With the whole input in
//! hand there is nothing to resume, so this is straight-line code -- but every
//! check liblzma makes is made, at the same point: the range coder's first
//! byte must be zero and its last state clean; an LZMA2 chunk must use
//! exactly the bytes it declares; a block's sizes must match its header and
//! the index; a header's CRC is checked before its flags. The tests hold
//! this to liblzma's own test files (`tests/files` of XZ Utils) and to its
//! verdict on every one-byte corruption of several streams.
//!
//! # The output limit
//!
//! LZMA can stand for a gigabyte of zeros in a few hundred bytes. Every entry
//! point has a cap: [`MAX_OUTPUT`], or the caller's, checked before each
//! chunk or symbol is written, not after.
//!
//! # No index, anywhere
//!
//! As in `deflate` and `bzip2`, nothing is indexed with `[i]` outside the
//! tests and two constant tables whose index is a masked byte.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

mod alone;
mod check;
mod filters;
mod lzma;
mod lzma2;
mod stream;
mod vli;

pub use check::Check;
pub use filters::Bcj;
pub use stream::Info;

/// Everything that can go wrong reading `.xz`, `.lzma` or raw LZMA data.
///
/// liblzma reports nearly all of these as `LZMA_DATA_ERROR`; they are told
/// apart here by what they say to the person holding the file. Cut short is
/// [`Error::UnexpectedEnd`], worth downloading again; not this format at all
/// is [`Error::NotXz`] or [`Error::NotLzma`]; made by something newer or
/// stranger than this reader is [`Error::Unsupported`]; the rest are damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input ended in the middle of a stream.
    UnexpectedEnd,
    /// The input does not begin with the `.xz` magic bytes.
    NotXz,
    /// The input does not begin with a valid `.lzma` header.
    NotLzma,
    /// A feature this reader does not have: a filter it does not know,
    /// properties it does not accept, or flag bits the format reserves
    /// (liblzma's `LZMA_OPTIONS_ERROR`).
    Unsupported,
    /// A stream header, stream footer or block header fails its CRC-32.
    HeaderCrcMismatch,
    /// A block's data fails the integrity check stored after it.
    CheckMismatch,
    /// The index, or the footer's record of it, disagrees with the blocks.
    IndexMismatch,
    /// Padding that is not zeros, or not a multiple of four bytes.
    InvalidPadding,
    /// The compressed data is corrupt: an invalid LZMA or LZMA2 stream, or
    /// one that does not decode to the size its block header declares.
    InvalidData,
    /// Bytes after the end of a `.lzma` stream.
    TrailingData,
    /// Decompression reached the caller's output cap -- also what a
    /// decompression bomb looks like; the two cannot be told apart.
    OutputTooLarge,
}

/// The text a person sees when an `.xz` or `.lzma` file will not open. Here
/// for `deflate::Error`'s reason: a `match` in this crate needs no wildcard,
/// so a new variant without words does not build.
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::UnexpectedEnd => "the compressed data ends unexpectedly",
            Self::NotXz => "not an .xz file",
            Self::NotLzma => "not an .lzma file",
            Self::Unsupported => "the file uses a feature this reader does not support",
            Self::HeaderCrcMismatch => "a header of the file is damaged (its CRC does not match)",
            Self::CheckMismatch => "the decompressed data fails its integrity check",
            Self::IndexMismatch => "the file's index does not match its contents",
            Self::InvalidPadding => "the file's padding is damaged",
            Self::InvalidData => "the compressed data is corrupt",
            Self::TrailingData => "there is data after the end of the compressed stream",
            Self::OutputTooLarge => "decompressed size exceeds the caller's limit",
        })
    }
}

/// Shorthand for this crate's fallible operations.
pub type Result<T> = core::result::Result<T, Error>;

/// The output cap of the entry points that take none: 256 MiB, as `bzip2`'s.
pub const MAX_OUTPUT: usize = 256 * 1024 * 1024;

/// Decompresses an `.xz` file, up to [`MAX_OUTPUT`] bytes.
///
/// # Errors
///
/// See [`decompress_limited`].
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    decompress_limited(data, MAX_OUTPUT)
}

/// Decompresses an `.xz` file -- every stream in it, as `xz -d` does --
/// refusing to produce more than `limit` bytes.
///
/// # Errors
///
/// [`Error::NotXz`] if `data` does not begin with an `.xz` stream,
/// [`Error::UnexpectedEnd`] if it is cut short, [`Error::OutputTooLarge`] at
/// the cap, and the rest of [`Error`] for a damaged or unsupported file.
pub fn decompress_limited(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    decompress_with_info(data, limit).map(|(out, _)| out)
}

/// As [`decompress_limited`], also returning what the file held: how many
/// streams and blocks, and whether a check went unverified.
///
/// # Errors
///
/// As [`decompress_limited`].
pub fn decompress_with_info(data: &[u8], limit: usize) -> Result<(Vec<u8>, Info)> {
    let mut out = Vec::new();
    let info = stream::decode(data, &mut out, limit)?;
    Ok((out, info))
}

/// Decompresses an `.lzma` file, up to [`MAX_OUTPUT`] bytes.
///
/// # Errors
///
/// See [`decompress_lzma_limited`].
pub fn decompress_lzma(data: &[u8]) -> Result<Vec<u8>> {
    decompress_lzma_limited(data, MAX_OUTPUT)
}

/// Decompresses an `.lzma` file, refusing to produce more than `limit`
/// bytes.
///
/// # Errors
///
/// [`Error::NotLzma`] for a header that cannot be one, [`Error::TrailingData`]
/// for bytes after the stream, and the rest of [`Error`] as for `.xz`.
pub fn decompress_lzma_limited(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    alone::decode(data, &mut out, limit)?;
    Ok(out)
}

/// Whether `data` begins like an `.xz` file.
#[must_use]
pub fn looks_like_xz(data: &[u8]) -> bool {
    data.starts_with(&stream::HEADER_MAGIC)
}

/// Whether `data` begins with an `.lzma` header liblzma's format detection
/// (`lzma_auto_decoder`) would accept: valid properties, a dictionary size
/// of 2^n or 2^n + 2^(n-1), and a size under 256 GiB if known. `.lzma` has
/// no magic number, so this is a plausibility test, not a proof.
#[must_use]
pub fn looks_like_lzma(data: &[u8]) -> bool {
    alone::header(data).is_ok_and(|h| alone::plausible(&h))
}

/// Decodes a raw LZMA stream as 7-Zip stores one: `props` is the coder's
/// five property bytes (the `lc`/`lp`/`pb` byte and the dictionary size,
/// little-endian), `size` the uncompressed size if the container records it
/// (`None` requires the end-of-payload marker), `limit` the output cap.
/// `data` must be the stream and nothing more, as `xz -d --format=raw`
/// requires.
///
/// # Errors
///
/// [`Error::Unsupported`] for properties liblzma does not accept (`lc + lp`
/// above 4), [`Error::TrailingData`] for bytes after the stream, and the rest
/// of [`Error`] for corrupt data.
pub fn lzma1(props: [u8; 5], data: &[u8], size: Option<u64>, limit: usize) -> Result<Vec<u8>> {
    let [p, d0, d1, d2, d3] = props;
    let props = lzma::Props::from_byte(p).ok_or(Error::Unsupported)?;
    let size = size.map(|s| usize::try_from(s).unwrap_or(usize::MAX));
    let mut out = Vec::new();
    let mut rc = lzma::Rc::new(data, 0)?;
    let mut decoder = lzma::Decoder::new(props);
    let dict = lzma::Dict {
        start: 0,
        window: lzma::Dict::window_for(u32::from_le_bytes([d0, d1, d2, d3])),
    };
    decoder.decode(&mut rc, &mut out, dict, size, limit)?;
    if rc.pos != data.len() {
        return Err(Error::TrailingData);
    }
    Ok(out)
}

/// Decodes a raw LZMA2 stream as 7-Zip stores one: `prop` is the coder's one
/// property byte (the dictionary size), `limit` the output cap. `data` must be
/// the stream and nothing more, as `xz -d --format=raw` requires.
///
/// # Errors
///
/// [`Error::Unsupported`] for a property byte liblzma does not accept,
/// [`Error::TrailingData`] for bytes after the end marker, and the rest of
/// [`Error`] for corrupt data.
pub fn lzma2(prop: u8, data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let dict_size = lzma2::dict_size_from_prop(prop).ok_or(Error::Unsupported)?;
    let mut out = Vec::new();
    let end = lzma2::decode(data, 0, &mut out, dict_size, limit)?;
    if end != data.len() {
        return Err(Error::TrailingData);
    }
    Ok(out)
}

/// Undoes a branch converter over `data`, whose first byte is at position
/// `start` of the stream (7-Zip's `BCJ`, `PPC`, `IA64`, `ARM`, `ARMT` and
/// `SPARC` coders are these, with `start` 0).
pub fn bcj_decode(arch: Bcj, start: u32, data: &mut [u8]) {
    filters::bcj(arch, start, false, data);
}

/// Undoes the delta filter over `data` (7-Zip's `Delta` coder), `distance`
/// from 1 to 256.
pub fn delta_decode(distance: u32, data: &mut [u8]) {
    filters::undo(
        &[filters::Filter::Delta {
            distance: distance.clamp(1, 256),
        }],
        data,
    );
}
