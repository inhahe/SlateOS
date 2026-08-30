//! The read half of the ZIO pipeline: checksum verification and
//! decompression.
//!
//! ZFS never hands a block to a consumer without first proving it is the block
//! that was written. That guarantee is the reason the filesystem exists, and a
//! reader that skipped it would be strictly worse than one that did not exist:
//! it would return corrupt data confidently. So this module verifies every
//! checksum it knows how to compute, and *refuses* a block whose checksum
//! algorithm it does not implement rather than passing it through unchecked.
//!
//! # Which checksums matter in practice
//!
//! | Algorithm | Where it appears |
//! |---|---|
//! | `fletcher_4` | default for file data |
//! | `sha256` | default for all metadata |
//! | `fletcher_2` | data on pools created before ~2008 |
//! | `label` | the uberblock and the label nvlist (SHA-256, self-checksumming) |
//!
//! Skein, Edon-R and BLAKE3 are selectable but only by explicit
//! `zfs set checksum=`, and none of them is a default for anything. They are
//! named and rejected.
//!
//! # Self-checksumming ("embedded") blocks
//!
//! Most blocks store their checksum in the *parent* block pointer, so
//! verifying a block requires the pointer that led to it. A label or uberblock
//! has no parent — it is found by looking at a fixed offset — so it stores its
//! own checksum in a `zio_eck_t` trailer at the end of the block. The
//! computation replaces that trailer's checksum field with a *verifier* (for a
//! label, the block's byte offset in the vdev) before hashing, which is what
//! stops a label being a valid label at some other offset: a torn or
//! misdirected write cannot produce a block that verifies where it landed.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::lz4;

use super::raw::{
    ZIO_CHECKSUM_FLETCHER_2, ZIO_CHECKSUM_FLETCHER_4, ZIO_CHECKSUM_GANG_HEADER, ZIO_CHECKSUM_LABEL,
    ZIO_CHECKSUM_OFF, ZIO_CHECKSUM_SHA256, ZIO_COMPRESS_EMPTY, ZIO_COMPRESS_LZ4, ZIO_COMPRESS_LZJB,
    ZIO_COMPRESS_OFF, ZIO_COMPRESS_ZLE, read_u32, read_u64,
};

/// Size of the `zio_eck_t` trailer on a self-checksumming block: an 8-byte
/// magic followed by the 32-byte checksum.
pub const ZEC_LEN: usize = 40;

/// `zec_magic`. Its asymmetry under byte-swapping is deliberate upstream: a
/// reader that sees the swapped value knows the block is foreign-endian
/// without having to guess.
pub const ZEC_MAGIC: u64 = 0x0210_da7a_b10c_7a11;

/// A 256-bit ZFS checksum, as four 64-bit words.
pub type ZioCksum = [u64; 4];

// ---------------------------------------------------------------------------
// Checksum functions
// ---------------------------------------------------------------------------

/// Fletcher-2 over 64-bit words, taken two at a time.
///
/// Weak — it is a pair of interleaved Fletcher sums, not a hash — but it is
/// what pools from before the fletcher-4 default contain, and reading them is
/// the point of a read-only driver.
#[must_use]
pub fn fletcher_2(data: &[u8]) -> ZioCksum {
    let (mut a0, mut a1, mut b0, mut b1) = (0u64, 0u64, 0u64, 0u64);
    let mut off = 0usize;
    while let (Ok(w0), Ok(w1)) = (read_u64(data, off), read_u64(data, off.wrapping_add(8))) {
        a0 = a0.wrapping_add(w0);
        a1 = a1.wrapping_add(w1);
        b0 = b0.wrapping_add(a0);
        b1 = b1.wrapping_add(a1);
        off = off.wrapping_add(16);
    }
    [a0, a1, b0, b1]
}

/// Fletcher-4 over 32-bit words.
///
/// The accumulators are 64-bit while the input words are 32-bit, which is what
/// gives it far better error detection than Fletcher-2 at a similar cost.
#[must_use]
pub fn fletcher_4(data: &[u8]) -> ZioCksum {
    let (mut a, mut b, mut c, mut d) = (0u64, 0u64, 0u64, 0u64);
    let mut off = 0usize;
    while let Ok(w) = read_u32(data, off) {
        a = a.wrapping_add(u64::from(w));
        b = b.wrapping_add(a);
        c = c.wrapping_add(b);
        d = d.wrapping_add(c);
        off = off.wrapping_add(4);
    }
    [a, b, c, d]
}

/// SHA-256, packed the way ZFS stores it.
///
/// Upstream's comment on this is worth repeating: an early private SHA-256
/// implementation wrote its words big-endian and had no byte-swapping variant,
/// so the modern code byte-swaps to match and preserve on-disk compatibility.
/// The net effect is that word *i* of the stored checksum is the big-endian
/// reading of digest bytes `8i..8i+8`.
#[must_use]
pub fn sha256_cksum(data: &[u8]) -> ZioCksum {
    let digest = sha2::sha256(data);
    let mut out = [0u64; 4];
    for (i, word) in out.iter_mut().enumerate() {
        let mut b = [0u8; 8];
        // The digest is a fixed 32 bytes and `i < 4`, so this slice always
        // exists; the fallback keeps the indexing lint satisfied without an
        // unwrap.
        if let Some(src) = digest.get(i.wrapping_mul(8)..i.wrapping_mul(8).wrapping_add(8)) {
            b.copy_from_slice(src);
        }
        *word = u64::from_be_bytes(b);
    }
    out
}

/// Compute the checksum named by `alg` over `data`.
///
/// # Errors
///
/// [`KernelError::NotSupported`] for an algorithm this driver does not
/// implement, so that an unreadable pool says so instead of failing a
/// comparison against a checksum nobody computed.
pub fn compute(alg: u8, data: &[u8]) -> KernelResult<ZioCksum> {
    match alg {
        ZIO_CHECKSUM_FLETCHER_2 => Ok(fletcher_2(data)),
        ZIO_CHECKSUM_FLETCHER_4 => Ok(fletcher_4(data)),
        ZIO_CHECKSUM_SHA256 | ZIO_CHECKSUM_LABEL | ZIO_CHECKSUM_GANG_HEADER => {
            Ok(sha256_cksum(data))
        }
        _ => Err(KernelError::NotSupported),
    }
}

/// Verify `data` against the checksum stored in a parent block pointer.
///
/// # Errors
///
/// - [`KernelError::NotSupported`] if the algorithm is unimplemented.
/// - [`KernelError::IoError`] on a mismatch — which is what a caller with a
///   second DVA copy should treat as "try the mirror".
pub fn verify(alg: u8, data: &[u8], expect: &ZioCksum) -> KernelResult<()> {
    if alg == ZIO_CHECKSUM_OFF {
        return Ok(());
    }
    let got = compute(alg, data)?;
    if got == *expect {
        Ok(())
    } else {
        Err(KernelError::IoError)
    }
}

/// Verify a self-checksumming block whose `zio_eck_t` trailer holds its own
/// checksum, computed with the trailer's checksum field replaced by
/// `verifier`.
///
/// For a label or uberblock the verifier is `[byte_offset, 0, 0, 0]`, which
/// binds the block to the place it was written.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the block is shorter than the
///   trailer, or the trailer's magic is absent (including the byte-swapped
///   magic, which means a big-endian pool).
/// - [`KernelError::IoError`] on a checksum mismatch.
pub fn verify_embedded(alg: u8, block: &[u8], verifier: &ZioCksum) -> KernelResult<()> {
    let eck_off = block
        .len()
        .checked_sub(ZEC_LEN)
        .ok_or(KernelError::InvalidArgument)?;
    let magic = read_u64(block, eck_off)?;
    if magic != ZEC_MAGIC {
        return Err(KernelError::InvalidArgument);
    }

    let mut stored = [0u64; 4];
    for (i, word) in stored.iter_mut().enumerate() {
        *word = read_u64(
            block,
            eck_off.wrapping_add(8).wrapping_add(i.wrapping_mul(8)),
        )?;
    }

    // Rebuild the block as it was when hashed: everything the same, but the
    // trailer's checksum field holding the verifier. A copy is unavoidable
    // because `block` is borrowed, and it is the honest cost of not mutating
    // a caller's buffer behind its back.
    let mut scratch = block.to_vec();
    for (i, word) in verifier.iter().enumerate() {
        let off = eck_off.wrapping_add(8).wrapping_add(i.wrapping_mul(8));
        if let Some(dst) = scratch.get_mut(off..off.wrapping_add(8)) {
            dst.copy_from_slice(&word.to_le_bytes());
        }
    }

    let got = compute(alg, &scratch)?;
    if got == stored {
        Ok(())
    } else {
        Err(KernelError::IoError)
    }
}

// ---------------------------------------------------------------------------
// Decompression
// ---------------------------------------------------------------------------

/// Decompress `src` (`psize` bytes as stored) into exactly `lsize` bytes.
///
/// # Errors
///
/// - [`KernelError::NotSupported`] for an algorithm this driver lacks (Zstd,
///   and the gzip levels).
/// - [`KernelError::CorruptedData`] if the algorithm ran but did not produce
///   `lsize` bytes, which means the block pointer and the payload disagree.
pub fn decompress(alg: u8, src: &[u8], lsize: usize) -> KernelResult<Vec<u8>> {
    match alg {
        ZIO_COMPRESS_OFF => {
            // `psize == lsize` here, but a short read or a corrupt pointer can
            // make that untrue, and silently padding would hand the caller
            // invented bytes.
            if src.len() < lsize {
                return Err(KernelError::CorruptedData);
            }
            src.get(..lsize)
                .map(<[u8]>::to_vec)
                .ok_or(KernelError::CorruptedData)
        }
        ZIO_COMPRESS_EMPTY => Ok(vec![0u8; lsize]),
        ZIO_COMPRESS_LZJB => lzjb_decompress(src, lsize),
        ZIO_COMPRESS_ZLE => zle_decompress(src, lsize),
        ZIO_COMPRESS_LZ4 => lz4_zfs_decompress(src, lsize),
        _ => Err(KernelError::NotSupported),
    }
}

/// Whether this driver can decompress `alg`, for a mount-time capability
/// check that fails before the first read rather than during it.
#[must_use]
pub const fn compression_supported(alg: u8) -> bool {
    matches!(
        alg,
        ZIO_COMPRESS_OFF
            | ZIO_COMPRESS_EMPTY
            | ZIO_COMPRESS_LZJB
            | ZIO_COMPRESS_ZLE
            | ZIO_COMPRESS_LZ4
    )
}

/// ZFS's LZ4: a 4-byte big-endian compressed length, then a raw LZ4 block.
///
/// The length prefix exists because `psize` is rounded up to the pool's sector
/// size, so the payload's true end is not derivable from the block pointer.
/// Handing the padding to the LZ4 decoder would make it read a token out of
/// zero padding.
fn lz4_zfs_decompress(src: &[u8], lsize: usize) -> KernelResult<Vec<u8>> {
    let prefix = src.get(..4).ok_or(KernelError::CorruptedData)?;
    let mut b = [0u8; 4];
    b.copy_from_slice(prefix);
    let clen = usize::try_from(u32::from_be_bytes(b)).map_err(|_| KernelError::CorruptedData)?;
    let end = clen.checked_add(4).ok_or(KernelError::CorruptedData)?;
    let body = src.get(4..end).ok_or(KernelError::CorruptedData)?;

    let out = lz4::decompress_block(body, lsize)?;
    if out.len() != lsize {
        return Err(KernelError::CorruptedData);
    }
    Ok(out)
}

/// LZJB, transliterated from `module/zcommon/lzjb.c`.
///
/// The format is a bitmap-driven byte/match stream: one control byte per eight
/// output items says, bit by bit, whether the next item is a literal byte or a
/// two-byte (length, offset) match. `MATCH_BITS = 6` splits those two bytes
/// into a 6-bit length (biased by `MATCH_MIN = 3`) and a 10-bit back-offset.
fn lzjb_decompress(src: &[u8], lsize: usize) -> KernelResult<Vec<u8>> {
    const MATCH_BITS: u32 = 6;
    const MATCH_MIN: usize = 3;
    const OFFSET_MASK: usize = (1 << (16 - MATCH_BITS)) - 1;

    let mut dst: Vec<u8> = Vec::with_capacity(lsize);
    let mut sp = 0usize;
    let mut copymap = 0u8;
    let mut copymask = 1u16 << 7;

    while dst.len() < lsize {
        copymask = copymask.wrapping_shl(1);
        if copymask == (1 << 8) {
            copymask = 1;
            copymap = *src.get(sp).ok_or(KernelError::CorruptedData)?;
            sp = sp.wrapping_add(1);
        }
        if u16::from(copymap) & copymask != 0 {
            let b0 = usize::from(*src.get(sp).ok_or(KernelError::CorruptedData)?);
            let b1 = usize::from(
                *src.get(sp.wrapping_add(1))
                    .ok_or(KernelError::CorruptedData)?,
            );
            sp = sp.wrapping_add(2);

            let mlen = (b0 >> (8 - MATCH_BITS)).wrapping_add(MATCH_MIN);
            let offset = ((b0 << 8) | b1) & OFFSET_MASK;
            // A zero or too-large offset would reach before the start of the
            // output, which upstream returns -1 for.
            let mut cpy = dst
                .len()
                .checked_sub(offset)
                .ok_or(KernelError::CorruptedData)?;
            if offset == 0 {
                return Err(KernelError::CorruptedData);
            }
            // The match may overlap the not-yet-written tail — that is how
            // LZJB encodes runs — so it is copied one byte at a time rather
            // than by slice.
            let take = mlen.min(lsize.wrapping_sub(dst.len()));
            for _ in 0..take {
                let byte = *dst.get(cpy).ok_or(KernelError::CorruptedData)?;
                dst.push(byte);
                cpy = cpy.wrapping_add(1);
            }
        } else {
            dst.push(*src.get(sp).ok_or(KernelError::CorruptedData)?);
            sp = sp.wrapping_add(1);
        }
    }

    Ok(dst)
}

/// Zero-length encoding, transliterated from `module/zcommon/zle.c` with
/// `n = 64`, the only level ZFS uses.
///
/// Each control byte encodes either a literal run of `1..=64` bytes or a run
/// of `1..=192` zeroes. It exists for metadata that is mostly zero — space
/// maps, sparse indirect blocks — where LZ4's framing overhead would dominate.
pub(super) fn zle_decompress(src: &[u8], lsize: usize) -> KernelResult<Vec<u8>> {
    const N: usize = 64;

    let mut dst: Vec<u8> = Vec::with_capacity(lsize);
    let mut sp = 0usize;

    while sp < src.len() && dst.len() < lsize {
        let len = usize::from(*src.get(sp).ok_or(KernelError::CorruptedData)?).wrapping_add(1);
        sp = sp.wrapping_add(1);
        if len <= N {
            for _ in 0..len {
                dst.push(*src.get(sp).ok_or(KernelError::CorruptedData)?);
                sp = sp.wrapping_add(1);
                if dst.len() >= lsize {
                    break;
                }
            }
        } else {
            let zeroes = len.wrapping_sub(N);
            for _ in 0..zeroes {
                if dst.len() >= lsize {
                    break;
                }
                dst.push(0);
            }
        }
    }

    if dst.len() != lsize {
        return Err(KernelError::CorruptedData);
    }
    Ok(dst)
}
