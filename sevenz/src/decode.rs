//! Decoding a folder: 7-Zip's `7zDecode.cpp` and the graph check of
//! `CoderMixer2.cpp`, over the whole archive in memory.
//!
//! A folder is a small graph: each coder reads one or more input streams --
//! a packed stream from the archive, or another coder's output, by a bond --
//! and writes one output; one coder's output is the folder's. 7-Zip pipes
//! the streams between threads; with the whole archive at hand, each coder's
//! inputs are decoded first and handed to it whole, which is the same
//! computation in another order.
//!
//! Damage stops a coder part-way, and what it wrote before then still
//! counts: 7-Zip extracts the files of a damaged solid folder that lie
//! before the damage. So a coder's result is its output so far and, if it
//! stopped short, why ([`Decoded`]); the files are judged against that
//! (`Archive::read_folder`).

use alloc::vec::Vec;

use crate::header::{Folder, Folders, Unpacker};
use crate::lzma_coder::{self, Threads};
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

/// A coder's or a folder's output, and why it stopped short if it did.
#[derive(Debug, Default)]
pub(crate) struct Decoded {
    pub(crate) out: Vec<u8>,
    pub(crate) error: Option<Error>,
    /// Some coder finished before its input did (7-Zip's
    /// `dataAfterEnd_Error`: "There are some data after the end of the
    /// payload data").
    pub(crate) after_end: bool,
}

/// The archive as [`Unpacker`] needs it, to decode packed header streams.
pub(crate) struct ArchiveUnpacker<'a> {
    pub(crate) data: &'a [u8],
}

impl Unpacker for ArchiveUnpacker<'_> {
    fn unpack(&self, folders: &Folders, index: usize, base: u64) -> Result<(Vec<u8>, bool)> {
        let size = usize::try_from(folders.unpack_size(index)).map_err(|_| Error::Unsupported)?;
        // 7-Zip decodes a packed header with one thread (`7zIn.cpp` passes
        // `mtMode = false`).
        let d = decode_folder(self.data, folders, index, base, size, Threads::One)?;
        match d.error {
            Some(e) => Err(e),
            None => Ok((d.out, d.after_end)),
        }
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
/// `start` in `data`, refusing an output above `limit`, LZMA2 as 7-Zip with
/// `threads` decodes it. A folder no coder of which could start -- an
/// unsupported method or graph -- is an `Err`; damage found while decoding
/// is in the [`Decoded`].
pub(crate) fn decode_folder(
    data: &[u8],
    folders: &Folders,
    index: usize,
    start: u64,
    limit: usize,
    threads: Threads,
) -> Result<Decoded> {
    let folder = folders.folders.get(index).ok_or(Error::Header)?;
    if folder.coders.len() > DECODE_CODERS_MAX {
        return Err(Error::Unsupported);
    }
    let coder_to_stream = check_graph(folder)?;
    if folders.unpack_size(index) > limit as u64 {
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
        threads,
    };
    ctx.coder(folder.unpack_coder as usize)
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
    threads: Threads,
}

impl Ctx<'_> {
    /// A coder's input stream `s`: a packed stream, or another coder's
    /// output.
    fn input(&self, s: u32) -> Result<Decoded> {
        if let Some(j) = self.folder.pack_streams.iter().position(|&p| p == s) {
            let (offset, size) = self.folders.pack_stream(self.index, j);
            let begin = self.start.checked_add(offset).ok_or(Error::Header)?;
            let end = begin.checked_add(size).ok_or(Error::Header)?;
            let (begin, end) = (
                usize::try_from(begin).map_err(|_| Error::UnexpectedEnd)?,
                usize::try_from(end).map_err(|_| Error::UnexpectedEnd)?,
            );
            let bytes = self.data.get(begin..end).ok_or(Error::UnexpectedEnd)?;
            return Ok(Decoded {
                out: bytes.to_vec(),
                ..Decoded::default()
            });
        }
        let bond = self
            .folder
            .bonds
            .iter()
            .find(|b| b.pack_index == s)
            .ok_or(Error::Unsupported)?;
        self.coder(bond.unpack_index as usize)
    }

    /// Coder `c`'s output, its inputs decoded first. An input that stopped
    /// short is decoded as far as it goes, and its error is the coder's.
    fn coder(&self, c: usize) -> Result<Decoded> {
        let coder = self.folder.coders.get(c).ok_or(Error::Header)?;
        let first = self.coder_to_stream.get(c).copied().unwrap_or(0);
        let mut inputs = Vec::with_capacity(coder.num_streams as usize);
        let mut upstream = None;
        let mut after_end = false;
        for i in 0..coder.num_streams {
            let d = self.input(first.saturating_add(i))?;
            upstream = upstream.or(d.error);
            after_end |= d.after_end;
            inputs.push(d.out);
        }
        let size = self.folders.coder_unpack_size(self.index, c);
        if size > self.limit as u64 {
            return Err(Error::OutputTooLarge);
        }
        let size = size as usize;
        let mut d = run(coder.method, &coder.props, &mut inputs, size, self.threads)?;
        d.after_end |= after_end;
        if d.error.is_none() {
            d.error = upstream;
        }
        if d.error.is_none() && d.out.len() != size {
            d.error = Some(Error::Data);
        }
        Ok(d)
    }
}

/// Runs one coder over its inputs, to an output of `size` bytes. A method
/// or properties it cannot use is an `Err`; damage is in the [`Decoded`].
fn run(
    method: u64,
    props: &[u8],
    inputs: &mut [Vec<u8>],
    size: usize,
    threads: Threads,
) -> Result<Decoded> {
    let simple = |inputs: &mut [Vec<u8>]| -> Result<Vec<u8>> {
        match inputs {
            [one] => Ok(core::mem::take(one)),
            _ => Err(Error::Unsupported),
        }
    };
    let done = |out: Vec<u8>, error: Option<Error>| Decoded {
        out,
        error,
        after_end: false,
    };
    match method {
        method::COPY => {
            if !props.is_empty() {
                return Err(Error::Unsupported);
            }
            let mut data = simple(inputs)?;
            let mut d = done(Vec::new(), None);
            if data.len() > size {
                d.after_end = true;
                data.truncate(size);
            } else if data.len() < size {
                d.error = Some(Error::Data);
            }
            d.out = data;
            Ok(d)
        }
        // 7-Zip's own decoders, which part from liblzma's on damaged data
        // (`lzma_dec.rs`).
        method::LZMA => {
            let data = simple(inputs)?;
            let c = lzma_coder::lzma(props, &data, size).ok_or(Error::Unsupported)?;
            Ok(done(c.out, (!c.ok).then_some(Error::Data)))
        }
        method::LZMA2 => {
            let data = simple(inputs)?;
            let c = lzma_coder::lzma2(props, &data, size, threads).ok_or(Error::Unsupported)?;
            Ok(done(c.out, (!c.ok).then_some(Error::Data)))
        }
        method::BZIP2 => {
            let data = simple(inputs)?;
            Ok(match bzip2::decompress_limited(&data, size) {
                Ok(out) => done(out, None),
                Err(_) => done(Vec::new(), Some(Error::Data)),
            })
        }
        method::DEFLATE => {
            let data = simple(inputs)?;
            let mut stream = deflate::inflate_stream(&data, size);
            let mut out = alloc::vec![0u8; size];
            let mut n = 0usize;
            let mut error = None;
            while n < size {
                let Some(rest) = out.get_mut(n..) else {
                    break;
                };
                match stream.read(rest) {
                    Ok(0) => break,
                    Ok(k) => n = n.saturating_add(k),
                    Err(_) => {
                        error = Some(Error::Data);
                        break;
                    }
                }
            }
            out.truncate(n);
            Ok(done(out, error))
        }
        method::DELTA => {
            let &[p] = props else {
                return Err(Error::Unsupported);
            };
            let mut data = simple(inputs)?;
            xz::delta_decode(u32::from(p).saturating_add(1), &mut data);
            Ok(done(data, None))
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
            Ok(done(data, None))
        }
        // Ported next; until then, refused as 7-Zip refuses a method it
        // lacks.
        method::PPMD | method::BCJ2 | method::DEFLATE64 => Err(Error::Unsupported),
        method::AES => Err(Error::PasswordRequired),
        _ => Err(Error::Unsupported),
    }
}
