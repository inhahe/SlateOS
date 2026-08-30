//! DEFLATE/gzip/zlib for the kernel — a thin shim over the `deflate` crate.
//!
//! The codec itself used to live here, all two thousand lines of it. It was
//! promoted to a root crate because a module of a *binary* crate cannot be
//! depended on, and `gui/imagecodec` needed an inflater to decode PNGs: not
//! being able to name `crate::fs::compress`, lane C wrote a second,
//! independent DEFLATE implementation (`requests/c-a-two-inflates.md`). Two
//! implementations of a decompressor is not two of an ordinary function —
//! each is its own parser of untrusted input, each has to get the same thirty
//! rows of length/distance tables right, and a bug fixed in one is not fixed
//! in the other.
//!
//! What remains here is the kernel's *view* of that crate:
//!
//! - the same nine public names the ten in-kernel call sites already use, so
//!   `zip.rs`, `sevenz.rs`, `fcompress.rs`, `swap.rs`, `httpd.rs`, `oci.rs`,
//!   `logpersist.rs`, `bench.rs` and `kshell.rs` are unchanged;
//! - [`to_kernel_error`], which maps [`deflate::Error`] onto [`KernelError`];
//! - [`self_test`], which stays in the kernel because
//!   `kernel/Cargo.toml` sets `test = false` — the boot battery is the only
//!   thing that runs an assertion in kernel context, and it is worth knowing
//!   that the codec works *there*, linked against the kernel's allocator,
//!   and not only on the build host.
//!
//! The crate's own `#[cfg(test)]` suite is the finer-grained one: it can
//! reach the private encoder internals, and it runs a single-byte-corruption
//! sweep that would take far too long to do at boot. This file's self-test is
//! deliberately the coarse round-trip battery.

use crate::error::{KernelError, KernelResult};
use alloc::vec::Vec;

/// Map a [`deflate::Error`] onto the kernel's error type.
///
/// The crate distinguishes eleven failures where the kernel copy returned
/// `CorruptedData` for all of them; the kernel's error enum does not have
/// room for that distinction, so most of it collapses here. That is not a
/// loss — the detail is still in the `deflate::Error` a non-kernel caller
/// receives, and the one distinction the kernel *can* express is preserved:
/// [`deflate::Error::OutputTooLarge`] is a resource limit, not corruption,
/// and a caller retrying with a bigger buffer is behaving correctly whereas a
/// caller retrying a bad checksum is not.
#[must_use]
pub fn to_kernel_error(err: deflate::Error) -> KernelError {
    match err {
        deflate::Error::OutputTooLarge => KernelError::OutOfMemory,
        _ => KernelError::CorruptedData,
    }
}

/// Decompress a raw DEFLATE stream (no gzip/zlib header).
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the stream is malformed, or
/// [`KernelError::OutOfMemory`] if it expands past [`deflate::MAX_OUTPUT`].
pub fn inflate(data: &[u8]) -> KernelResult<Vec<u8>> {
    deflate::inflate(data).map_err(to_kernel_error)
}

/// Compress `data` into a raw DEFLATE stream.
#[must_use]
pub fn deflate(data: &[u8]) -> Vec<u8> {
    deflate::deflate(data)
}

/// Compress `data` into a gzip stream (RFC 1952).
#[must_use]
pub fn gzip(data: &[u8]) -> Vec<u8> {
    deflate::gzip(data)
}

/// Decompress a gzip stream, verifying its CRC-32 and length trailer.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the header, the payload or either
/// trailer field is wrong, or [`KernelError::OutOfMemory`] on overrun.
pub fn gunzip(data: &[u8]) -> KernelResult<Vec<u8>> {
    deflate::gunzip(data).map_err(to_kernel_error)
}

/// CRC-32 (ISO 3309) — the gzip/ZIP/PNG polynomial, not CRC32C.
///
/// Kept under this name because `zip.rs` and `sevenz.rs` call it; new code
/// should use [`crate::crypto::crc32`] (or the `crc32` crate) directly.
#[must_use]
pub fn crc32_iso_pub(data: &[u8]) -> u32 {
    crate::crypto::crc32(data)
}

/// Adler-32 (RFC 1950 §8) — zlib's integrity check.
#[must_use]
pub fn adler32(data: &[u8]) -> u32 {
    deflate::adler32(data)
}

/// Decompress a zlib stream (RFC 1950), verifying its Adler-32.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the two-byte header is not a zlib
/// header, the stream needs a preset dictionary, the payload is malformed or
/// the Adler-32 disagrees; [`KernelError::OutOfMemory`] on overrun.
pub fn zlib_inflate(data: &[u8]) -> KernelResult<Vec<u8>> {
    deflate::zlib_inflate(data).map_err(to_kernel_error)
}

/// Compress `data` into a zlib stream (RFC 1950).
#[must_use]
pub fn zlib_deflate(data: &[u8]) -> Vec<u8> {
    deflate::zlib_deflate(data)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-battery self-test for the compression codec.
///
/// Coarser than the crate's own suite on purpose. The crate's tests run on
/// the build host and can afford an exhaustive corruption sweep; what this
/// one adds is that the same code works *in kernel context* — linked against
/// the kernel's allocator, on the bare-metal target, with the kernel's
/// codegen flags. A codec that passes on the host and faults here would
/// otherwise not be caught until a `.tar.gz` was opened.
///
/// # Errors
///
/// [`KernelError::InternalError`] if any vector mismatches.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[compress] Running self-test...");

    // CRC-32 ISO check value. Guards the *link*: this is now the `crc32`
    // crate's table reached through two re-exports, and a mis-wiring that
    // picked up CRC32C instead would produce a plausible-looking 32-bit
    // number rather than a compile error.
    let crc = crc32_iso_pub(b"123456789");
    if crc != 0xCBF4_3926 {
        crate::serial_println!(
            "[compress]   FAIL: CRC32 ISO expected 0xCBF43926, got {:#010x}",
            crc
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   CRC-32 ISO verified ✓");

    // A stored block, written by hand: BFINAL=1, BTYPE=00, LEN=5, NLEN=!5.
    let stored = [0x01, 0x05, 0x00, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o'];
    let result = inflate(&stored)?;
    if result.as_slice() != b"hello" {
        crate::serial_println!(
            "[compress]   FAIL: stored block produced {} bytes",
            result.len()
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Stored block inflate verified ✓");

    // Adler-32 check value (matches Python's `zlib.adler32(b"Wikipedia")`).
    let adler = adler32(b"Wikipedia");
    if adler != 0x11E6_0398 {
        crate::serial_println!(
            "[compress]   FAIL: Adler-32 expected 0x11E60398, got {:#010x}",
            adler
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Adler-32 verified ✓");

    // Round-trips through all three framings, on the four input shapes that
    // drive the encoder down different paths: mixed text, empty, entirely
    // repetitive, and incompressible.
    let text: &[u8] = b"Hello, world! Hello, world! This is a test of DEFLATE compression. \
                        AAAAAAAAAAAAAAAAAAAAAA BBBBBBBBBBBBBBBB repetition helps compression.";
    let repeated = [0xAA_u8; 1024];

    let mut noise = Vec::with_capacity(1024);
    let mut state: u32 = 0x1234_5678;
    for _ in 0..1024 {
        // xorshift32, so the bytes are reproducible but have no structure the
        // LZ77 matcher can exploit — this is the case where the encoder must
        // fall back to a stored block rather than expand the input.
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        noise.push(state as u8);
    }

    let mut mixed = Vec::with_capacity(8192);
    for i in 0..8192usize {
        mixed.push(if i % 100 < 60 {
            // Spelled out rather than indexed into `b"ABCD"`: the index would
            // be provably in range, but `indexing_slicing` cannot see that and
            // a self-test is the last place to spend an `#[allow]`.
            match i % 4 {
                0 => b'A',
                1 => b'B',
                2 => b'C',
                _ => b'D',
            }
        } else {
            (i.wrapping_mul(7).wrapping_add(13) % 256) as u8
        });
    }

    let cases: [(&str, &[u8]); 5] = [
        ("text", text),
        ("empty", b""),
        ("all-same", &repeated),
        ("noise", &noise),
        ("8K mixed", &mixed),
    ];

    for (name, input) in cases {
        let raw = deflate(input);
        if inflate(&raw)?.as_slice() != input {
            crate::serial_println!("[compress]   FAIL: deflate round-trip on {}", name);
            return Err(KernelError::InternalError);
        }

        let gz = gzip(input);
        if gunzip(&gz)?.as_slice() != input {
            crate::serial_println!("[compress]   FAIL: gzip round-trip on {}", name);
            return Err(KernelError::InternalError);
        }

        let z = zlib_deflate(input);
        if zlib_inflate(&z)?.as_slice() != input {
            crate::serial_println!("[compress]   FAIL: zlib round-trip on {}", name);
            return Err(KernelError::InternalError);
        }

        crate::serial_println!(
            "[compress]   {}: {} -> raw {}, gzip {}, zlib {} ✓",
            name,
            input.len(),
            raw.len(),
            gz.len(),
            z.len()
        );
    }

    // The failure paths, checked here and not only on the host, because the
    // mapping from `deflate::Error` to `KernelError` is kernel-side code that
    // the crate's own tests cannot see. A truncated gzip must be an error
    // rather than a short read, and an over-large expansion must be
    // `OutOfMemory` rather than `CorruptedData`.
    let gz = gzip(text);
    let Some(truncated) = gz.get(..gz.len().saturating_sub(4)) else {
        crate::serial_println!("[compress]   FAIL: gzip output too short to truncate");
        return Err(KernelError::InternalError);
    };
    if gunzip(truncated).is_ok() {
        crate::serial_println!("[compress]   FAIL: truncated gzip accepted");
        return Err(KernelError::InternalError);
    }
    let mut bad_crc = gz.clone();
    let Some(last) = bad_crc.last_mut() else {
        crate::serial_println!("[compress]   FAIL: gzip output empty");
        return Err(KernelError::InternalError);
    };
    *last ^= 0xFF;
    if gunzip(&bad_crc) != Err(KernelError::CorruptedData) {
        crate::serial_println!("[compress]   FAIL: bad gzip trailer not CorruptedData");
        return Err(KernelError::InternalError);
    }
    if deflate::inflate_limited(&deflate(&repeated), 1023).map_err(to_kernel_error)
        != Err(KernelError::OutOfMemory)
    {
        crate::serial_println!("[compress]   FAIL: output cap did not map to OutOfMemory");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Error mapping verified ✓");

    crate::serial_println!("[compress] Self-test passed.");
    Ok(())
}
