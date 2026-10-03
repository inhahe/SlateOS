//! Decoding a folder: 7-Zip's `7zDecode.cpp` and the graph check of
//! `CoderMixer2.cpp`, over the whole archive in memory.
//!
//! A folder is a small graph: each coder reads one or more input streams --
//! a packed stream from the archive, or another coder's output, by a bond --
//! and writes one output; one coder's output is the folder's. 7-Zip pipes
//! the streams between threads; with the whole archive at hand, each coder's
//! inputs are decoded first and handed to it whole, which is the same
//! computation in another order.

use alloc::vec::Vec;

use crate::header::{Folder, Folders, Unpacker};
use crate::{Error, Result};

/// The method IDs (`7zHeader.h`).
pub(crate) mod method {
    pub(crate) const COPY: u64 = 0;
    pub(crate) const DELTA: u64 = 3;
    pub(crate) const LZMA2: u64 = 0x21;
    pub(crate) const LZMA: u64 = 0x03_0101;
    pub(crate) const PPMD: u64 = 0x03_0401;
    pub(crate) const DEFLATE: u64 = 0x04_0108;
    pub(crate) const DEFLATE64: u64 = 0x04_0109;
    pub(crate) const BZIP2: u64 = 0x04_0202;
    pub(crate) const BCJ: u64 = 0x0303_0103;
    pub(crate) const BCJ2: u64 = 0x0303_011b;
    pub(crate) const PPC: u64 = 0x0303_0205;
    pub(crate) const IA64: u64 = 0x0303_0401;
    pub(crate) const ARM: u64 = 0x0303_0501;
    pub(crate) const ARMT: u64 = 0x0303_0701;
    pub(crate) const SPARC: u64 = 0x0303_0805;
    pub(crate) const AES: u64 = 0x06f1_0701;
}

/// `IsDecodingSupported`: at most 32 coders.
const DECODE_CODERS_MAX: usize = 32;

/// The archive as [`Unpacker`] needs it, to decode packed header streams.
pub(crate) struct ArchiveUnpacker<'a> {
    pub(crate) data: &'a [u8],
}

impl Unpacker for ArchiveUnpacker<'_> {
    fn unpack(&self, folders: &Folders, index: usize, base: u64) -> Result<(Vec<u8>, bool)> {
        let size = usize::try_from(folders.unpack_size(index)).map_err(|_| Error::Unsupported)?;
        decode_folder(self.data, folders, index, base, size)
    }
}

/// `CBindInfo::CalcMapsAndCheck`: one bond fewer than coders, every input a
/// bond's or a packed stream, and every coder reached exactly once from the
/// unpack coder.
fn check_graph(folder: &Folder) -> Result<Vec<u32>> {
    let n = folder.coders.len();
    if n == 0 || folder.bonds.len() != n.saturating_sub(1) {
        return Err(Error::Unsupported);
    }
    let mut coder_to_stream = Vec::with_capacity(n);
    let mut streams = 0u32;
    for c in &folder.coders {
        coder_to_stream.push(streams);
        streams = streams.saturating_add(c.num_streams);
    }
    if streams as usize != folder.bonds.len().saturating_add(folder.pack_streams.len()) {
        return Err(Error::Unsupported);
    }
    let mut used = alloc::vec![false; n];
    if !reach(
        folder,
        &coder_to_stream,
        folder.unpack_coder as usize,
        &mut used,
    ) {
        return Err(Error::Unsupported);
    }
    if used.iter().any(|u| !u) {
        return Err(Error::Unsupported);
    }
    Ok(coder_to_stream)
}

/// `CBondsChecks::CheckCoder`
fn reach(folder: &Folder, coder_to_stream: &[u32], coder: usize, used: &mut [bool]) -> bool {
    match used.get_mut(coder) {
        Some(u) if !*u => *u = true,
        _ => return false,
    }
    let start = coder_to_stream.get(coder).copied().unwrap_or(0);
    let num = folder.coders.get(coder).map_or(0, |c| c.num_streams);
    for i in 0..num {
        let s = start.saturating_add(i);
        if folder.pack_streams.contains(&s) {
            continue;
        }
        let Some(bond) = folder.bonds.iter().find(|b| b.pack_index == s) else {
            return false;
        };
        if !reach(folder, coder_to_stream, bond.unpack_index as usize, used) {
            return false;
        }
    }
    true
}

/// Decodes folder `index` of `folders`, whose packed streams start at
/// `start` in `data`, refusing an output above `limit`. Returns the output
/// and whether some coder finished before its input did.
pub(crate) fn decode_folder(
    data: &[u8],
    folders: &Folders,
    index: usize,
    start: u64,
    limit: usize,
) -> Result<(Vec<u8>, bool)> {
    let folder = folders.folders.get(index).ok_or(Error::Header)?;
    if folder.coders.len() > DECODE_CODERS_MAX {
        return Err(Error::Unsupported);
    }
    let coder_to_stream = check_graph(folder)?;
    let size = folders.unpack_size(index);
    if size > limit as u64 {
        return Err(Error::OutputTooLarge);
    }
    let ctx = Ctx {
        data,
        folders,
        index,
        folder,
        coder_to_stream: &coder_to_stream,
        start,
        limit,
    };
    let mut after_end = false;
    let out = ctx.coder(folder.unpack_coder as usize, &mut after_end)?;
    Ok((out, after_end))
}

/// What decoding one folder needs.
struct Ctx<'a> {
    data: &'a [u8],
    folders: &'a Folders,
    index: usize,
    folder: &'a Folder,
    coder_to_stream: &'a [u32],
    start: u64,
    limit: usize,
}

impl Ctx<'_> {
    /// A coder's input stream `s`: a packed stream, or another coder's
    /// output.
    fn input(&self, s: u32, after_end: &mut bool) -> Result<Vec<u8>> {
        if let Some(j) = self.folder.pack_streams.iter().position(|&p| p == s) {
            let (offset, size) = self.folders.pack_stream(self.index, j);
            let begin = self.start.checked_add(offset).ok_or(Error::Header)?;
            let end = begin.checked_add(size).ok_or(Error::Header)?;
            let (begin, end) = (
                usize::try_from(begin).map_err(|_| Error::UnexpectedEnd)?,
                usize::try_from(end).map_err(|_| Error::UnexpectedEnd)?,
            );
            return Ok(self
                .data
                .get(begin..end)
                .ok_or(Error::UnexpectedEnd)?
                .to_vec());
        }
        let bond = self
            .folder
            .bonds
            .iter()
            .find(|b| b.pack_index == s)
            .ok_or(Error::Unsupported)?;
        self.coder(bond.unpack_index as usize, after_end)
    }

    /// Coder `c`'s output, its inputs decoded first.
    fn coder(&self, c: usize, after_end: &mut bool) -> Result<Vec<u8>> {
        let coder = self.folder.coders.get(c).ok_or(Error::Header)?;
        let first = self.coder_to_stream.get(c).copied().unwrap_or(0);
        let mut inputs = Vec::with_capacity(coder.num_streams as usize);
        for i in 0..coder.num_streams {
            inputs.push(self.input(first.saturating_add(i), after_end)?);
        }
        let size = self.folders.coder_unpack_size(self.index, c);
        if size > self.limit as u64 {
            return Err(Error::OutputTooLarge);
        }
        let size = size as usize;
        let out = run(coder.method, &coder.props, &mut inputs, size, after_end)?;
        if out.len() != size {
            return Err(Error::Data);
        }
        Ok(out)
    }
}

/// Runs one coder over its inputs, to an output of `size` bytes.
fn run(
    method: u64,
    props: &[u8],
    inputs: &mut [Vec<u8>],
    size: usize,
    after_end: &mut bool,
) -> Result<Vec<u8>> {
    let simple = |inputs: &mut [Vec<u8>]| -> Result<Vec<u8>> {
        match inputs {
            [one] => Ok(core::mem::take(one)),
            _ => Err(Error::Unsupported),
        }
    };
    match method {
        method::COPY => {
            if !props.is_empty() {
                return Err(Error::Unsupported);
            }
            let mut data = simple(inputs)?;
            if data.len() < size {
                return Err(Error::Data);
            }
            if data.len() > size {
                *after_end = true;
                data.truncate(size);
            }
            Ok(data)
        }
        method::LZMA => {
            let p: [u8; 5] = props.try_into().map_err(|_| Error::Unsupported)?;
            let data = simple(inputs)?;
            xz::lzma1(p, &data, Some(size as u64), size).map_err(codec)
        }
        method::LZMA2 => {
            let &[p] = props else {
                return Err(Error::Unsupported);
            };
            let data = simple(inputs)?;
            xz::lzma2(p, &data, size).map_err(codec)
        }
        method::BZIP2 => {
            let data = simple(inputs)?;
            bzip2::decompress_limited(&data, size).map_err(|_| Error::Data)
        }
        method::DEFLATE => {
            let data = simple(inputs)?;
            deflate::inflate_limited(&data, size).map_err(|_| Error::Data)
        }
        method::DELTA => {
            let &[p] = props else {
                return Err(Error::Unsupported);
            };
            let mut data = simple(inputs)?;
            xz::delta_decode(u32::from(p).saturating_add(1), &mut data);
            Ok(data)
        }
        method::BCJ | method::PPC | method::IA64 | method::ARM | method::ARMT | method::SPARC => {
            let arch = match method {
                method::BCJ => xz::Bcj::X86,
                method::PPC => xz::Bcj::PowerPc,
                method::IA64 => xz::Bcj::Ia64,
                method::ARM => xz::Bcj::Arm,
                method::ARMT => xz::Bcj::ArmThumb,
                _ => xz::Bcj::Sparc,
            };
            let start = match props {
                [] => 0,
                &[a, b, c, d] => u32::from_le_bytes([a, b, c, d]),
                _ => return Err(Error::Unsupported),
            };
            let mut data = simple(inputs)?;
            xz::bcj_decode(arch, start, &mut data);
            Ok(data)
        }
        // Ported next; until then, refused as 7-Zip refuses a method it
        // lacks.
        method::PPMD | method::BCJ2 | method::DEFLATE64 => Err(Error::Unsupported),
        method::AES => Err(Error::PasswordRequired),
        _ => Err(Error::Unsupported),
    }
}

/// An `xz` codec error as 7-Zip reports it.
fn codec(e: xz::Error) -> Error {
    match e {
        xz::Error::Unsupported => Error::Unsupported,
        xz::Error::OutputTooLarge => Error::OutputTooLarge,
        _ => Error::Data,
    }
}
