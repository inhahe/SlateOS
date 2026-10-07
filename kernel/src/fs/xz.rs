//! XZ compression and decompression, through the `xz` crate.
//!
//! A shim. The work is lane E's `xz` crate: a port of liblzma 5.2.5,
//! `no_std` + `alloc`, whose decoder accepts exactly what `xz -d` accepts --
//! held by its tests to liblzma's verdicts on XZ Utils' own test files,
//! files xz made, and thousands of corruptions -- and whose encoder writes
//! what `xz` writes, byte for byte. This module keeps the kernel's names for
//! it, as `compress.rs` does for `deflate`
//! (requests/e-a-the-xz-crate-is-ready-and-xz-compress-loses-files.md).
//!
//! It replaced, on 2026-10-07, an implementation written here rather than
//! ported, which lost data: its compressor put the whole input in one LZMA2
//! chunk, whose header cannot describe more than 2 MiB in or 64 KiB out, so
//! every file past those sizes was written unreadable -- by any decoder, its
//! own included -- and `fcompress` stored them so. Its decoder stopped after a
//! file's first stream, never verified SHA-256 checks, the index or the
//! footer, returned damaged data as good, and capped each block's output but
//! not the file's.

use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use alloc::vec::Vec;

/// The kernel's error for an `xz` crate error: [`KernelError::NotSupported`]
/// for a feature the reader does not have, [`KernelError::FileTooLarge`] at
/// the output cap -- which is also what a decompression bomb looks like --
/// and [`KernelError::CorruptedData`] for the rest, which are all damage or
/// not xz at all.
fn kernel_error(e: ::xz::Error) -> KernelError {
    match e {
        ::xz::Error::Unsupported => KernelError::NotSupported,
        ::xz::Error::OutputTooLarge => KernelError::FileTooLarge,
        _ => KernelError::CorruptedData,
    }
}

/// Decompress an `.xz` file: every stream in it, as `xz -d` reads
/// concatenated files, with every check verified, up to the crate's 256 MiB
/// cap on the whole output.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] for damage or data that is not xz,
/// [`KernelError::NotSupported`] for a filter or setting the reader lacks,
/// [`KernelError::FileTooLarge`] past the cap.
pub fn unxz(data: &[u8]) -> KernelResult<Vec<u8>> {
    ::xz::decompress(data).map_err(kernel_error)
}

/// Compress `data` as an `.xz` file: `xz -6`'s settings -- CRC-64, LZMA2, one
/// block -- with the dictionary no larger than the data needs.
///
/// The cap on the dictionary is what keeps a small file's compression small:
/// the match finder's hash table is sized from the dictionary (16 MiB of it
/// at `-6`'s 8 MiB), and a dictionary larger than the input buys nothing, no
/// match being able to reach further back than the input's start. The
/// stream is an ordinary `.xz` file either way; only the dictionary size its
/// header records differs from `xz -6`'s for inputs under 8 MiB.
///
/// # Errors
///
/// None in practice: the options are `xz -6`'s with a dictionary between
/// 4 KiB and 8 MiB, which the crate always accepts; its refusal is passed on
/// as [`KernelError::NotSupported`] rather than assumed away.
pub fn xz_compress(data: &[u8]) -> KernelResult<Vec<u8>> {
    /// The smallest dictionary the format allows.
    const MIN_DICT: u32 = 4096;
    let mut options = ::xz::XzOptions::preset(::xz::Preset::DEFAULT);
    let needed = u32::try_from(data.len())
        .unwrap_or(u32::MAX)
        .checked_next_power_of_two()
        .unwrap_or(u32::MAX)
        .max(MIN_DICT);
    options.lzma2.dict_size = options.lzma2.dict_size.min(needed);
    ::xz::compress_with(data, &options).map_err(kernel_error)
}

/// A small, deterministic, incompressible test input: `len` bytes of an
/// xorshift sequence.
fn noise(len: usize, mut seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        out.extend_from_slice(&seed.to_le_bytes());
    }
    out.truncate(len);
    out
}

/// Boot self-test: the shim reaches the crate, and the cases the replaced
/// implementation got wrong come out right. The crate's own tests (run on
/// the host) are what hold it to liblzma; this checks the kernel's link to
/// it.
///
/// # Errors
///
/// [`KernelError::InternalError`] on any failure.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[xz] === self-test start ===");
    let fail = |what: &str| {
        serial_println!("[xz]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };

    // Text round trip.
    let text = b"The quick brown fox jumps over the lazy dog. ".repeat(200);
    match xz_compress(&text).and_then(|c| unxz(&c)) {
        Ok(back) if back == text => {}
        _ => return fail("a text round trip"),
    }
    // Empty input.
    match xz_compress(&[]).and_then(|c| unxz(&c)) {
        Ok(back) if back.is_empty() => {}
        _ => return fail("an empty round trip"),
    }
    // Past 64 KiB compressed: the old compressor's chunk header lied here, and
    // nothing could read the file back.
    let random = noise(70_000, 0x9E37_79B9_7F4A_7C15);
    let packed = match xz_compress(&random) {
        Ok(p) => p,
        Err(_) => return fail("compressing 70 000 random bytes"),
    };
    if packed.len() <= 65_536 {
        return fail("70 000 random bytes compressed under 64 KiB: the case is not exercised");
    }
    match unxz(&packed) {
        Ok(back) if back == random => {}
        _ => return fail("a round trip past 64 KiB compressed"),
    }
    // Two streams back to back decode as one file, as `xz -d` reads them; the
    // old decoder stopped after the first.
    let (Ok(a), Ok(b)) = (xz_compress(b"first "), xz_compress(b"second")) else {
        return fail("compressing two short streams");
    };
    let mut both = a.clone();
    both.extend_from_slice(&b);
    match unxz(&both) {
        Ok(back) if back == b"first second" => {}
        _ => return fail("two concatenated streams"),
    }
    // Damage is an error, not data.
    let mut damaged = a;
    if let Some(byte) = damaged.get_mut(14) {
        *byte ^= 0x40;
    }
    if unxz(&damaged).is_ok() {
        return fail("a damaged stream decoded");
    }
    if unxz(b"not xz at all") != Err(KernelError::CorruptedData) {
        return fail("non-xz data was not CorruptedData");
    }

    serial_println!("[xz] === self-test passed ===");
    Ok(())
}
