//! Bzip2 compression and decompression, through the `bzip2` crate.
//!
//! A shim. The work is lane E's `bzip2` crate: a port of libbzip2 1.0.8,
//! `no_std` + `alloc`, whose compressor writes libbzip2's exact bytes and
//! whose decoder accepts exactly the streams libbzip2 accepts, both held to
//! libbzip2's own output and verdicts by its tests. This module keeps the
//! kernel's names for it, as `compress.rs` does for `deflate`
//! (requests/e-a-the-bzip2-crate-is-ready-for-the-kernel-shim.md).
//!
//! It replaced, on 2026-10-07, an implementation written here rather than
//! ported, with five faults the crate does not have: its compressor cut the
//! input into blocks *before* the first run-length step, so input rich in
//! four-byte runs made blocks larger than the header promised, which no
//! decoder reads (data lost when `fcompress` stored them); its decoder
//! stopped after the first of several concatenated streams, had no output
//! cap, sorted repetitive blocks in quadratic time, and differed from
//! libbzip2 on randomised blocks, excess selectors, empty blocks and a run
//! missing its count.

use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use alloc::vec::Vec;

/// The kernel's error for a `bzip2` crate error: [`KernelError::FileTooLarge`]
/// at the output cap -- also what a decompression bomb looks like -- and
/// [`KernelError::CorruptedData`] for the rest, which are damage or not
/// bzip2 at all.
fn kernel_error(e: ::bzip2::Error) -> KernelError {
    match e {
        ::bzip2::Error::OutputTooLarge => KernelError::FileTooLarge,
        _ => KernelError::CorruptedData,
    }
}

/// Decompress bzip2 data: every stream in it, as `bzip2 -d` reads
/// concatenated files (`pbzip2` writes one per block), each block's and each
/// stream's CRC verified, up to the crate's 256 MiB cap on the whole output.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] for damage or data that is not bzip2,
/// [`KernelError::FileTooLarge`] past the cap.
pub fn bunzip2(data: &[u8]) -> KernelResult<Vec<u8>> {
    ::bzip2::decompress(data).map_err(kernel_error)
}

/// Compress `data` as one bzip2 stream at `level` (1 to 9: 100 000 to 900 000
/// byte blocks), exactly as `bzip2 -<level>` would. A level outside 1 to 9 is
/// clamped into it, as this function always has.
#[must_use]
pub fn bzip2_compress(data: &[u8], level: u8) -> Vec<u8> {
    let level = ::bzip2::Level::new(level.clamp(1, 9)).unwrap_or(::bzip2::Level::BEST);
    ::bzip2::compress(data, level)
}

/// Boot self-test: the shim reaches the crate, and the cases the replaced
/// implementation got wrong come out right. The crate's own tests (run on
/// the host) are what hold it to libbzip2; this checks the kernel's link to
/// it.
///
/// # Errors
///
/// [`KernelError::InternalError`] on any failure.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[bzip2] === self-test start ===");
    let fail = |what: &str| {
        serial_println!("[bzip2]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };

    let text = b"The quick brown fox jumps over the lazy dog. ".repeat(200);
    if bunzip2(&bzip2_compress(&text, 9)).as_deref() != Ok(text.as_slice()) {
        return fail("a text round trip");
    }
    if bunzip2(&bzip2_compress(&[], 1)).as_deref() != Ok(&[][..]) {
        return fail("an empty round trip");
    }
    // Four-byte runs fill a level-1 block (100 000 bytes) past its size once
    // the run-length step has expanded them: the old compressor cut the input
    // first and wrote a block bigger than its header allowed, which nothing
    // could read back.
    let runs = b"AAAAB".repeat(20_000);
    if bunzip2(&bzip2_compress(&runs, 1)).as_deref() != Ok(runs.as_slice()) {
        return fail("a level-1 round trip of four-byte runs");
    }
    // Two streams back to back are one file, as `bzip2 -d` reads them; the old
    // decoder stopped after the first.
    let mut both = bzip2_compress(b"first ", 1);
    both.extend_from_slice(&bzip2_compress(b"second", 1));
    if bunzip2(&both).as_deref() != Ok(&b"first second"[..]) {
        return fail("two concatenated streams");
    }
    // Damage is an error, not data.
    let mut damaged = bzip2_compress(&text, 9);
    if let Some(byte) = damaged.get_mut(40) {
        *byte ^= 0x10;
    }
    if bunzip2(&damaged).is_ok() {
        return fail("a damaged stream decoded");
    }
    if bunzip2(b"not bzip2") != Err(KernelError::CorruptedData) {
        return fail("non-bzip2 data was not CorruptedData");
    }

    serial_println!("[bzip2] === self-test passed ===");
    Ok(())
}
