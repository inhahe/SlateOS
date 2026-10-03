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
use core::cell::RefCell;

use crate::header::{Folder, Folders, Unpacker};
use crate::lzma_coder::{self, Threads};
use crate::{Error, Result, aes7z, bcj2, branch, inflate, ppmd7};

/// The method IDs (`7zHeader.h`).
pub(crate) mod method {
    pub(crate) const COPY: u64 = 0;
    pub(crate) const DELTA: u64 = 3;
    pub(crate) const ARM64: u64 = 0xa;
    pub(crate) const RISCV: u64 = 0xb;
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

/// The password, and the keys derived from it -- once for each salt and
/// number of rounds, as 7-Zip caches them, since deriving one takes 2^19
/// rounds of SHA-256 by default.
#[derive(Debug, Default)]
pub(crate) struct Keys {
    /// The password's UTF-16LE bytes, if there is one.
    password: Option<Vec<u8>>,
    derived: RefCell<Vec<Derived>>,
}

/// A key derived, and what from besides the password.
#[derive(Debug)]
struct Derived {
    cycles_power: u32,
    salt: Vec<u8>,
    key: [u8; 32],
}

impl Keys {
    pub(crate) fn new(password: Option<&str>) -> Self {
        Self {
            password: password.map(aes7z::password_bytes),
            derived: RefCell::new(Vec::new()),
        }
    }

    /// The key for `props`, or `None` without a password.
    fn key(&self, props: &aes7z::Props) -> Option<[u8; 32]> {
        let password = self.password.as_deref()?;
        let mut derived = self.derived.borrow_mut();
        if let Some(d) = derived
            .iter()
            .find(|d| d.cycles_power == props.cycles_power && d.salt == props.salt)
        {
            return Some(d.key);
        }
        let key = aes7z::derive_key(props, password);
        derived.push(Derived {
            cycles_power: props.cycles_power,
            salt: props.salt.clone(),
            key,
        });
        Some(key)
    }
}

/// The archive as [`Unpacker`] needs it, to decode packed header streams.
pub(crate) struct ArchiveUnpacker<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) keys: &'a Keys,
}

impl Unpacker for ArchiveUnpacker<'_> {
    fn unpack(&self, folders: &Folders, index: usize, base: u64) -> Result<(Vec<u8>, bool)> {
        let size = usize::try_from(folders.unpack_size(index)).map_err(|_| Error::Unsupported)?;
        // 7-Zip decodes a packed header with one thread (`7zIn.cpp` passes
        // `mtMode = false`).
        let d = decode_folder(
            self.data,
            folders,
            index,
            base,
            size,
            Threads::One,
            self.keys,
        )?;
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
    keys: &Keys,
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
        keys,
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
    keys: &'a Keys,
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
        let mut d = run(
            coder.method,
            &coder.props,
            &mut inputs,
            size,
            self.threads,
            self.limit,
            self.keys,
        )?;
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

/// Runs one coder over its inputs, to an output of `size` bytes, with at
/// most `limit` bytes of model memory. A method or properties it cannot use
/// is an `Err`; damage is in the [`Decoded`].
fn run(
    method: u64,
    props: &[u8],
    inputs: &mut [Vec<u8>],
    size: usize,
    threads: Threads,
    limit: usize,
    keys: &Keys,
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
        method::PPMD => {
            let data = simple(inputs)?;
            match ppmd7::decode(props, &data, size, limit) {
                Ok(c) => Ok(done(c.out, (!c.ok).then_some(Error::Data))),
                Err(ppmd7::Refused::Unsupported) => Err(Error::Unsupported),
                Err(ppmd7::Refused::TooLarge) => Err(Error::OutputTooLarge),
            }
        }
        method::BCJ2 => {
            if !props.is_empty() {
                return Err(Error::Unsupported);
            }
            let [main, call, jump, rc] = inputs else {
                return Err(Error::Unsupported);
            };
            let c = bcj2::decode([main, call, jump, rc], size);
            Ok(done(c.out, (!c.ok).then_some(Error::Data)))
        }
        method::BZIP2 => {
            // 7-Zip's BZip2 and Deflate coders take no properties, and since
            // 23.00 a coder given some it cannot take is refused.
            if !props.is_empty() {
                return Err(Error::Unsupported);
            }
            let data = simple(inputs)?;
            // As 7-Zip's decoder reads it: one stream, by rules of its own.
            // What decoded before damage is kept, a block's bytes before its
            // CRC is checked: 7-Zip writes them, and the files they complete
            // are judged by their own CRCs.
            let mut out = Vec::new();
            let mut d = match bzip2::decompress_as_7zip(&data, &mut out, size) {
                Ok(used) => {
                    let mut d = done(Vec::new(), None);
                    // 7-Zip's mixer: input left after a coder that succeeded.
                    d.after_end = used < data.len();
                    d
                }
                Err(_) => done(Vec::new(), Some(Error::Data)),
            };
            d.out = out;
            Ok(d)
        }
        method::DEFLATE | method::DEFLATE64 => {
            if !props.is_empty() {
                return Err(Error::Unsupported);
            }
            let data = simple(inputs)?;
            // By 7-Zip's rules for the coder (`inflate.rs`), which part from
            // zlib's at the end of the stream and past the end of the input.
            let r = inflate::decode(&data, size, method == method::DEFLATE64);
            let mut d = done(r.out, (!r.ok).then_some(Error::Data));
            // 7-Zip's mixer: input left after a coder that succeeded.
            d.after_end = r.ok && r.used < data.len();
            Ok(d)
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
            // 7-Zip's coder for these takes no properties, and since 23.00
            // refuses a coder that is given some (`7zDecode.cpp`).
            if !props.is_empty() {
                return Err(Error::Unsupported);
            }
            let mut data = simple(inputs)?;
            xz::bcj_decode(arch, 0, &mut data);
            Ok(done(data, None))
        }
        method::ARM64 | method::RISCV => {
            // `NCompress::NBranch::CDecoder`: an optional start address,
            // aligned to the instruction (4 bytes, or 2 for RISC-V).
            let alignment = if method == method::ARM64 { 3 } else { 1 };
            let pc = match props {
                [] => 0,
                &[a, b, c, d] => u32::from_le_bytes([a, b, c, d]),
                _ => return Err(Error::Unsupported),
            };
            if pc & alignment != 0 {
                return Err(Error::Unsupported);
            }
            let mut data = simple(inputs)?;
            // What is not converted -- a last partial instruction -- is
            // kept as it is, as `FilterCoder` writes it at the stream's end.
            if method == method::ARM64 {
                branch::arm64_decode(&mut data, pc);
            } else {
                branch::riscv_decode(&mut data, pc);
            }
            Ok(done(data, None))
        }
        method::AES => {
            // The properties first, as 7-Zip sets them before it asks for
            // a password.
            let p = aes7z::parse_props(props).ok_or(Error::Unsupported)?;
            let key = keys.key(&p).ok_or(Error::PasswordRequired)?;
            let data = simple(inputs)?;
            let c = aes7z::decrypt(&p, &key, &data, size);
            Ok(done(c.out, (!c.ok).then_some(Error::Data)))
        }
        _ => Err(Error::Unsupported),
    }
}
