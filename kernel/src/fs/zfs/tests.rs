//! ZFS self-tests, run at boot against structures built in RAM.
//!
//! # Why this file exists rather than `#[cfg(test)] mod tests`
//!
//! `kernel/Cargo.toml` sets `test = false` on the kernel binary: the kernel
//! provides its own `panic_impl` and the rest of the `no_std` lang items, so a
//! host `cargo test` cannot link it. A `#[cfg(test)]` module inside this crate
//! is therefore never compiled and never run — it is a test that *looks* like
//! coverage and provides none. Every check in this file instead runs on the
//! real target, on every boot, through [`self_test`].
//!
//! # What is checked here
//!
//! The groups below are the *format* checks: bitfield extraction, the two
//! Fletcher variants and ZLE, the XDR nvlist walker, uberblock parsing, dnode
//! parsing including the spill pointer, ZAP micro and fat leaves, and System
//! Attribute layout resolution. Each builds the on-disk bytes it parses
//! directly from the format documentation — the constant is written out in the
//! test rather than referenced from the parser wherever doing so would make
//! the check circular.
//!
//! Where a builder is unavoidably shared with the parser (the ZAP leaf
//! builder calls [`super::zap::zap_hash`], since a leaf's bucket assignment is
//! *defined* by the hash), the check that remains meaningful is the structural
//! one: that a chunk chain is followed across chunk boundaries, that iteration
//! reaches entries in every bucket, that a name longer than one chunk round
//! trips. A wrong CRC-64 seed would still pass, and cannot be settled by any
//! self-consistent test — only by a real `zpool`-written image. That reasoning
//! is recorded at [`super::zap::zap_hash`] instead.
//!
//! # Never panics
//!
//! A self-test that panics takes the kernel down before it can print the rest
//! of its results, so one broken check would hide every check after it. All
//! assertions go through [`Checks`], which tallies and prints.

// Every group must have the [`TestGroup`] signature so they can share one
// array, even the groups whose checks all happen to be infallible today. A
// group that gains a `?` later should not also have to change its signature
// and the array's element type.
#![allow(clippy::unnecessary_wraps)]

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::MemorySource;
use crate::fs::path::Path;
use crate::fs::vfs::{EntryType, FileSystem};
use crate::serial_println;

use super::dmu::{Reader, parse_dnode};
use super::label::{
    POOL_STATE_ACTIVE, POOL_STATE_DESTROYED, POOL_STATE_EXPORTED, POOL_STATE_L2CACHE,
    POOL_STATE_SPARE, SPA_VERSION_FEATURES, front_label_offset, parse_uberblock,
};
use super::nvlist::{
    DATA_TYPE_NVLIST, DATA_TYPE_NVLIST_ARRAY, DATA_TYPE_STRING, DATA_TYPE_UINT64,
    NV_ENCODE_NATIVE, NV_ENCODE_XDR, NV_LITTLE_ENDIAN, NvList,
};
use super::raw::{
    BLKPTR_LEN, DMU_OST_META, DMU_OST_ZFS, DMU_OT_DIRECTORY_CONTENTS, DMU_OT_DNODE,
    DMU_OT_DSL_DATASET, DMU_OT_DSL_DIR, DMU_OT_MASTER_NODE, DMU_OT_NONE, DMU_OT_OBJECT_DIRECTORY,
    DMU_OT_OBJSET, DMU_OT_PLAIN_FILE_CONTENTS, DMU_OT_SA, DMU_OT_SA_ATTR_LAYOUTS,
    DMU_OT_SA_ATTR_REGISTRATION, DMU_OT_SA_MASTER_NODE, DNODE_CORE_LEN, DNODE_LEN, Dva,
    OBJSET_PHYS_SIZE, SPA_MINBLOCKSHIFT, UBERBLOCK_MAGIC, UBERBLOCK_MAGIC_SWAPPED,
    VDEV_LABEL_NVLIST_OFFSET, VDEV_LABEL_SIZE, VDEV_LABEL_START_SIZE, VDEV_LABEL_UBERBLOCK_OFFSET,
    VDEV_LABEL_UBERBLOCK_SIZE, ZIO_CHECKSUM_FLETCHER_4, ZIO_CHECKSUM_SHA256, ZIO_COMPRESS_OFF,
    ZIO_COMPRESS_ZSTD, bf64_get, bf64_get_sb, parse_blkptr,
};
use super::sa::{SA_MAGIC, SaAttr, SaMap, SaRegistry, decimal_key, parse_header};
use super::zap::{
    CHAIN_END, MZAP_ENT_LEN, ZAP_CHUNK_ARRAY, ZAP_CHUNK_ENTRY, ZAP_CHUNK_FREE, ZAP_HASHBITS,
    ZAP_LEAF_ARRAY_BYTES, ZAP_LEAF_MAGIC, ZAP_MAGIC, ZBT_HEADER, ZBT_LEAF, ZBT_MICRO,
    ZFS_CRC64_TABLE, leaf_chunk_off, leaf_entries, leaf_lookup, leaf_lookup_array, leaf_numchunks,
    log2_exact, micro_entries, micro_lookup, zap_hash,
};
use super::zio::{
    ZEC_LEN, ZEC_MAGIC, compression_supported, compute, decompress, fletcher_2, fletcher_4,
    sha256_cksum, zle_decompress,
};
use super::{DD_HEAD_DATASET_OBJ, DS_BP_OFFSET, OBJSET_TYPE_OFFSET, ZfsFs};

/// Running tally of the self-test.
struct Checks {
    passed: u32,
    failed: u32,
}

/// One group of the self-test.
///
/// Runs its checks against the shared tally, returning `Err` only for a hard
/// error that makes its *own* remaining steps meaningless — a parse that could
/// not proceed, so everything downstream of it in that group would be
/// asserting against nothing. A failed assertion is not an error and does not
/// stop the group.
type TestGroup = fn(&mut Checks) -> KernelResult<()>;

impl Checks {
    const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    /// Assert `cond`, reporting `what` on failure.
    ///
    /// Never panics, and returns nothing: the tally is what decides the
    /// suite's verdict, so there is no failure for a caller to propagate and
    /// no `?` to write at the call site.
    fn check(&mut self, cond: bool, what: &str) {
        if cond {
            self.passed = self.passed.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: {}", what);
        }
    }

    /// Assert `got == want` for two `u64`s, printing both on failure.
    ///
    /// Separate from [`Checks::check`] because "expected 16384, got 512" tells
    /// whoever reads the serial log which field was misread, whereas a bare
    /// failure name tells them only that something was.
    fn check_u64(&mut self, got: u64, want: u64, what: &str) {
        if got == want {
            self.passed = self.passed.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
            serial_println!(
                "[zfs] SELF-TEST FAILED: {} - got {}, expected {}",
                what,
                got,
                want
            );
        }
    }

    /// Assert that `got` failed with exactly `want`.
    ///
    /// A helper rather than `got == Err(want)` because most of the success
    /// types involved — `Uberblock`, `Dnode`, `SaMap` — do not implement
    /// `PartialEq`, and deriving it on production types purely so a test could
    /// spell an equality would be the test dictating the driver's API.
    /// Comparing the error alone is also the stricter check: it says *which*
    /// rejection happened, not merely that one did.
    fn check_err<T>(&mut self, got: KernelResult<T>, want: KernelError, what: &str) {
        match got {
            Err(e) if e == want => self.passed = self.passed.saturating_add(1),
            Err(e) => {
                self.failed = self.failed.saturating_add(1);
                serial_println!(
                    "[zfs] SELF-TEST FAILED: {} - got {:?}, expected {:?}",
                    what,
                    e,
                    want
                );
            }
            Ok(_) => {
                self.failed = self.failed.saturating_add(1);
                serial_println!(
                    "[zfs] SELF-TEST FAILED: {} - succeeded, expected {:?}",
                    what,
                    want
                );
            }
        }
    }

    /// Assert two byte slices are equal, reporting the first difference.
    fn check_bytes(&mut self, got: &[u8], want: &[u8], what: &str) {
        if got.len() != want.len() {
            self.failed = self.failed.saturating_add(1);
            serial_println!(
                "[zfs] SELF-TEST FAILED: {} - length {} != {}",
                what,
                got.len(),
                want.len()
            );
            return;
        }
        for (i, (a, b)) in got.iter().zip(want.iter()).enumerate() {
            if a != b {
                self.failed = self.failed.saturating_add(1);
                serial_println!(
                    "[zfs] SELF-TEST FAILED: {} - byte {} is {:#04x}, expected {:#04x}",
                    what,
                    i,
                    a,
                    b
                );
                return;
            }
        }
        self.passed = self.passed.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// Bitfields and DVAs
// ---------------------------------------------------------------------------

fn test_primitives(c: &mut Checks) -> KernelResult<()> {
    c.check_u64(bf64_get(0xFF00, 8, 8), 0xFF, "bf64_get extracts a byte");
    c.check_u64(
        bf64_get(0x8000_0000_0000_0000, 63, 1),
        1,
        "bf64_get reaches the top bit",
    );
    c.check_u64(bf64_get(0, 0, 64), 0, "bf64_get of a full-width zero");
    // Out-of-range requests must answer 0 rather than shifting by >= 64, which
    // in release builds is a wrapping shift and would return a live bit.
    c.check_u64(
        bf64_get(u64::MAX, 64, 1),
        0,
        "bf64_get past the top answers 0",
    );
    c.check_u64(
        bf64_get(u64::MAX, 0, 0),
        0,
        "bf64_get of zero width answers 0",
    );

    // Sized fields store `(bytes >> 9) - 1`, so a 16 KiB block stores 31 and a
    // 512-byte block stores 0.
    c.check_u64(
        bf64_get_sb(31, 0, 16, SPA_MINBLOCKSHIFT, 1),
        16384,
        "bf64_get_sb decodes a 16 KiB size",
    );
    c.check_u64(
        bf64_get_sb(0, 0, 16, SPA_MINBLOCKSHIFT, 1),
        512,
        "bf64_get_sb decodes the minimum size",
    );

    // Every DVA offset is relative to the 4 MiB label reserve at the front of
    // the vdev; forgetting the bias reads the labels as if they were data.
    let dva = Dva {
        vdev: 0,
        asize: 512,
        offset: 0,
        gang: false,
    };
    c.check_u64(
        dva.physical_offset()?,
        VDEV_LABEL_START_SIZE,
        "DVA offset 0 lands past the label reserve",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Checksums and decompression
// ---------------------------------------------------------------------------

fn test_zio(c: &mut Checks) -> KernelResult<()> {
    // Four 32-bit words of 1: a = 1,2,3,4; b = 1,3,6,10; c = 1,4,10,20;
    // d = 1,5,15,35 — the running sum of running sums, four deep.
    let mut data = Vec::new();
    for _ in 0..4 {
        data.extend_from_slice(&1u32.to_le_bytes());
    }
    let f4 = fletcher_4(&data);
    c.check(f4 == [4, 10, 20, 35], "fletcher-4 of four unit words");

    let mut data = Vec::new();
    for v in 1u64..=4 {
        data.extend_from_slice(&v.to_le_bytes());
    }
    // Fletcher-2 pairs the words: a0 = 1+3, a1 = 2+4, b0 = 1+4, b1 = 2+6.
    let f2 = fletcher_2(&data);
    c.check(f2 == [4, 6, 5, 8], "fletcher-2 pairs the words");

    // ZLE: one literal byte 0xAA, then a 100-byte zero run.
    let src = [0x00, 0xAA, 100 + 64 - 1];
    match zle_decompress(&src, 101) {
        Ok(out) => {
            c.check(out.len() == 101, "ZLE produces the declared length");
            c.check(out.first() == Some(&0xAA), "ZLE emits the literal byte");
            c.check(
                out.get(1..).is_some_and(|t| t.iter().all(|&b| b == 0)),
                "ZLE expands the zero run",
            );
        }
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: ZLE round trip errored: {:?}", e);
        }
    }

    // An "uncompressed" payload shorter than the logical size is corruption,
    // not a short read to be zero-filled.
    c.check_err(
        decompress(ZIO_COMPRESS_OFF, &[0u8; 4], 8),
        KernelError::CorruptedData,
        "uncompressed refuses a short payload",
    );

    // Zstd is refused by name rather than misread as something else. A wrong
    // guess here would hand back plausible-looking garbage from a real pool.
    c.check(
        !compression_supported(ZIO_COMPRESS_ZSTD),
        "zstd is reported unsupported",
    );
    c.check_err(
        decompress(ZIO_COMPRESS_ZSTD, &[0u8; 16], 16),
        KernelError::NotSupported,
        "zstd decompression is refused by name",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// XDR nvlists
// ---------------------------------------------------------------------------

/// Build one XDR nvpair.
///
/// Written from the XDR encoding rather than by calling the parser: name as a
/// big-endian length plus bytes padded to four, then the type, the element
/// count, and the already-encoded value — all big-endian, as XDR always is
/// regardless of the host.
fn nv_pair(name: &[u8], dtype: u32, nelem: u32, value: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    let Ok(nlen) = u32::try_from(name.len()) else {
        return Vec::new();
    };
    body.extend_from_slice(&nlen.to_be_bytes());
    body.extend_from_slice(name);
    while body.len() % 4 != 0 {
        body.push(0);
    }
    body.extend_from_slice(&dtype.to_be_bytes());
    body.extend_from_slice(&nelem.to_be_bytes());
    body.extend_from_slice(value);

    let mut out = Vec::new();
    // The pair is preceded by its encoded and decoded sizes, both counting the
    // two size fields themselves.
    let Ok(encoded) = u32::try_from(body.len().saturating_add(8)) else {
        return Vec::new();
    };
    out.extend_from_slice(&encoded.to_be_bytes());
    out.extend_from_slice(&encoded.to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// [`nv_pair`], for the one type that needs no separate value encoder.
fn nv_pair_u64(name: &[u8], value: u64) -> Vec<u8> {
    nv_pair(name, DATA_TYPE_UINT64, 1, &value.to_be_bytes())
}

/// An XDR variable-length opaque: a big-endian length, the bytes, then padding
/// up to a four-byte boundary.
fn xdr_string(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let Ok(len) = u32::try_from(s.len()) else {
        return out;
    };
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(s);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// The six-byte nvlist header a vdev label carries.
fn nv_header() -> Vec<u8> {
    // encoding, endianness, two reserved bytes, then version and nvflag.
    let mut v = vec![NV_ENCODE_XDR, NV_LITTLE_ENDIAN, 0, 0];
    v.extend_from_slice(&0u32.to_be_bytes()); // version
    v.extend_from_slice(&1u32.to_be_bytes()); // nvflag
    v
}

fn test_nvlist(c: &mut Checks) -> KernelResult<()> {
    let mut buf = nv_header();
    buf.extend_from_slice(&nv_pair_u64(b"txg", 42));
    buf.extend_from_slice(&nv_pair_u64(b"ashift", 12));
    buf.extend_from_slice(&0u32.to_be_bytes()); // terminator

    let nv = NvList::new(&buf)?;
    c.check(
        nv.get_u64(b"txg") == Some(42),
        "nvlist finds the first pair",
    );
    c.check(
        nv.get_u64(b"ashift") == Some(12),
        "nvlist finds a later pair",
    );
    c.check(
        nv.get_u64(b"absent").is_none(),
        "nvlist stops at the terminator",
    );

    // `NV_ENCODE_NATIVE` never appears on disk; accepting it would mean
    // decoding host-endian data with the big-endian XDR reader.
    let mut buf = nv_header();
    if let Some(b) = buf.first_mut() {
        *b = NV_ENCODE_NATIVE;
    }
    c.check_err(
        NvList::new(&buf),
        KernelError::NotSupported,
        "nvlist rejects the native encoding",
    );

    // An "encoded size" of 4 is smaller than the two size fields, so stepping
    // by it would never advance past this pair. The walk must stop, not spin.
    let mut buf = nv_header();
    buf.extend_from_slice(&4u32.to_be_bytes());
    buf.extend_from_slice(&4u32.to_be_bytes());
    let nv = NvList::new(&buf)?;
    c.check(
        nv.get_u64(b"anything").is_none(),
        "a zero-advance pair does not loop forever",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Vdev labels and uberblocks
// ---------------------------------------------------------------------------

fn test_label(c: &mut Checks) -> KernelResult<()> {
    c.check_u64(front_label_offset(0), 0, "label 0 is at the start");
    c.check_u64(
        front_label_offset(1),
        256 * 1024,
        "label 1 is one label further in",
    );

    // A zeroed slot parses, but with a txg of 0 and a hole for a rootbp —
    // which is exactly what `find_uberblock` uses to reject an unwritten slot.
    // If either were nonzero here, a zeroed uberblock array would beat a real
    // uberblock and the mount would follow a null root.
    let slot = [0u8; 1024];
    let ub = parse_uberblock(&slot, 0)?;
    c.check_u64(ub.txg, 0, "a zeroed uberblock has txg 0");
    c.check(ub.rootbp.is_hole(), "a zeroed uberblock has a hole rootbp");

    Ok(())
}

// ---------------------------------------------------------------------------
// Dnodes
// ---------------------------------------------------------------------------

fn test_dmu(c: &mut Checks) -> KernelResult<()> {
    let buf = [0u8; DNODE_LEN];
    let dn = parse_dnode(&buf, 0)?;
    c.check(!dn.is_allocated(), "an empty slot is unallocated");
    c.check(
        dn.blkptrs.is_empty(),
        "an unallocated dnode has no block pointers",
    );
    c.check(dn.spill.is_none(), "an unallocated dnode has no spill");

    // An allocated object always has at least one block pointer; zero would
    // make the level-0 lookup index an empty slice.
    let mut buf = [0u8; DNODE_LEN];
    buf[0] = DMU_OT_PLAIN_FILE_CONTENTS;
    buf[2] = 1; // nlevels
    buf[3] = 0; // nblkptr — impossible for an allocated object
    buf[8] = 1; // datablkszsec
    c.check(
        parse_dnode(&buf, 0).is_err(),
        "a dnode with no block pointers is rejected",
    );

    // An indirect block smaller than one block pointer gives `epbs` of zero or
    // less, so the level walk would never narrow and would loop or divide by
    // zero.
    let mut buf = [0u8; DNODE_LEN];
    buf[0] = DMU_OT_PLAIN_FILE_CONTENTS;
    buf[1] = 4; // indblkshift = 16 bytes, smaller than a 128-byte pointer
    buf[2] = 2; // nlevels > 1, so the fan-out matters
    buf[3] = 1;
    buf[8] = 1;
    c.check(
        parse_dnode(&buf, 0).is_err(),
        "an indirect block smaller than a pointer is rejected",
    );

    // `DN_SPILL_BLKPTR` lives in the *last* block pointer's worth of the
    // dnode's slots, not after `nblkptr` — which is why it is found from the
    // end. A two-slot dnode puts it at 1024 - 128 = 896.
    let mut buf = [0u8; DNODE_LEN * 2];
    buf[0] = DMU_OT_PLAIN_FILE_CONTENTS;
    buf[1] = 14; // indblkshift
    buf[2] = 1; // nlevels
    buf[3] = 1; // nblkptr
    buf[7] = 0x04; // dn_flags: DNODE_FLAG_SPILL_BLKPTR
    buf[8] = 1; // datablkszsec
    buf[12] = 1; // extra_slots -> two slots in all
    // A spill pointer whose first DVA is non-zero, so it is not a hole. A
    // block pointer's DVA word 0 is `asize` in the low 24 bits and the vdev
    // above bit 32; word 1 is the offset. One 512-byte block at offset 512.
    let spill_off = DNODE_LEN * 2 - 128;
    if let Some(w) = buf.get_mut(spill_off..spill_off + 8) {
        w.copy_from_slice(&1u64.to_le_bytes()); // asize = 1 sector
    }
    if let Some(w) = buf.get_mut(spill_off + 8..spill_off + 16) {
        w.copy_from_slice(&1u64.to_le_bytes()); // offset = 1 sector
    }
    match parse_dnode(&buf, 0) {
        Ok(dn) => {
            c.check(dn.is_allocated(), "the spill dnode is allocated");
            c.check(dn.flags & 0x04 != 0, "the spill flag survives parsing");
            match dn.spill {
                Some(bp) => {
                    c.check(!bp.is_hole(), "the spill pointer is not a hole");
                    match bp.dvas.first() {
                        Some(dva) => c.check_u64(
                            dva.offset,
                            512,
                            "the spill DVA offset is read from the last 128 bytes",
                        ),
                        None => c.check(false, "the spill pointer has a DVA"),
                    }
                }
                None => c.check(false, "a flagged dnode yields a spill pointer"),
            }
        }
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: spill dnode did not parse: {:?}", e);
        }
    }

    // Without the flag the same bytes must *not* be read as a spill pointer:
    // they are bonus buffer, and misreading them would invent a block pointer
    // out of a file's system attributes.
    if let Some(b) = buf.get_mut(7) {
        *b = 0;
    }
    match parse_dnode(&buf, 0) {
        Ok(dn) => c.check(
            dn.spill.is_none(),
            "no spill is read without the spill flag",
        ),
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!(
                "[zfs] SELF-TEST FAILED: unflagged dnode did not parse: {:?}",
                e
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// ZAP
// ---------------------------------------------------------------------------

/// Build a one-block microzap holding the given name/value pairs.
///
/// A microzap is a 64-byte header followed by 64-byte slots, each a value at
/// 0, a collision differentiator at 8, and a NUL-padded name at 14.
fn build_micro(size: usize, pairs: &[(&[u8], u64)]) -> Vec<u8> {
    let mut block = vec![0u8; size];
    if let Some(w) = block.get_mut(..8) {
        w.copy_from_slice(&ZBT_MICRO.to_le_bytes());
    }
    for (i, (name, value)) in pairs.iter().enumerate() {
        let off = MZAP_ENT_LEN.saturating_mul(i.saturating_add(1));
        if let Some(w) = block.get_mut(off..off.saturating_add(8)) {
            w.copy_from_slice(&value.to_le_bytes());
        }
        let n_off = off.saturating_add(14);
        if let Some(w) = block.get_mut(n_off..n_off.saturating_add(name.len())) {
            w.copy_from_slice(name);
        }
    }
    block
}

/// Write `data` as a chunk chain starting at the next free chunk, and return
/// the index of its first chunk.
fn write_array(block: &mut [u8], blkshift: u32, next: &mut usize, data: &[u8]) -> u16 {
    let first = *next;
    let mut written = 0usize;
    while written < data.len() {
        let idx = *next;
        *next = next.saturating_add(1);
        let Some(off) = leaf_chunk_off(blkshift, idx) else {
            break;
        };
        if let Some(b) = block.get_mut(off) {
            *b = ZAP_CHUNK_ARRAY;
        }
        let n = data.len().saturating_sub(written).min(ZAP_LEAF_ARRAY_BYTES);
        let (Some(dst), Some(src)) = (
            block.get_mut(off.saturating_add(1)..off.saturating_add(1).saturating_add(n)),
            data.get(written..written.saturating_add(n)),
        ) else {
            break;
        };
        dst.copy_from_slice(src);
        written = written.saturating_add(n);
        let link = if written < data.len() {
            u16::try_from(*next).unwrap_or(CHAIN_END)
        } else {
            CHAIN_END
        };
        if let Some(w) = block.get_mut(off.saturating_add(22)..off.saturating_add(24)) {
            w.copy_from_slice(&link.to_le_bytes());
        }
    }
    u16::try_from(first).unwrap_or(CHAIN_END)
}

/// Build a fatzap leaf block holding `ents`, each a name, an integer width and
/// the raw big-endian value bytes.
///
/// `lh_prefix_len` is left at zero, so the whole hash table is one leaf's
/// worth and the bucket is the top `blkshift - 5` bits of the hash.
fn build_leaf(blkshift: u32, salt: u64, ents: &[(&[u8], u8, &[u8])]) -> Vec<u8> {
    let size = 1usize << blkshift;
    let hash_entries = 1usize << blkshift.saturating_sub(5);
    let mut block = vec![0u8; size];
    if let Some(w) = block.get_mut(..8) {
        w.copy_from_slice(&ZBT_LEAF.to_le_bytes());
    }
    if let Some(w) = block.get_mut(24..28) {
        w.copy_from_slice(&ZAP_LEAF_MAGIC.to_le_bytes());
    }
    for i in 0..leaf_numchunks(blkshift).unwrap_or(0) {
        if let Some(off) = leaf_chunk_off(blkshift, i)
            && let Some(b) = block.get_mut(off)
        {
            *b = ZAP_CHUNK_FREE;
        }
    }
    for i in 0..hash_entries {
        let off = 48usize.saturating_add(i.saturating_mul(2));
        if let Some(w) = block.get_mut(off..off.saturating_add(2)) {
            w.copy_from_slice(&CHAIN_END.to_le_bytes());
        }
    }

    let mut next = 0usize;
    for (name, intlen, value) in ents {
        if *intlen == 0 {
            continue;
        }
        let hash = zap_hash(salt, name);
        let shift = 64u32.saturating_sub(blkshift.saturating_sub(5));
        let bucket = ((hash >> shift) as usize) & hash_entries.saturating_sub(1);
        let entry_idx = next;
        next = next.saturating_add(1);
        // ZAP names carry their terminator in `le_name_numints`.
        let mut nb = name.to_vec();
        nb.push(0);
        let name_chunk = write_array(&mut block, blkshift, &mut next, &nb);
        let value_chunk = write_array(&mut block, blkshift, &mut next, value);

        let Some(off) = leaf_chunk_off(blkshift, entry_idx) else {
            continue;
        };
        let hb = 48usize.saturating_add(bucket.saturating_mul(2));
        let head = block
            .get(hb..hb.saturating_add(2))
            .and_then(|s| s.try_into().ok())
            .map_or(CHAIN_END, u16::from_le_bytes);
        if let Some(b) = block.get_mut(off) {
            *b = ZAP_CHUNK_ENTRY;
        }
        if let Some(b) = block.get_mut(off.saturating_add(1)) {
            *b = *intlen;
        }
        let numints = value
            .len()
            .checked_div(usize::from(*intlen))
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(0);
        let name_numints = u16::try_from(nb.len()).unwrap_or(0);
        for (field_off, bytes) in [
            (2usize, head.to_le_bytes()),
            (4, name_chunk.to_le_bytes()),
            (6, name_numints.to_le_bytes()),
            (8, value_chunk.to_le_bytes()),
            (10, numints.to_le_bytes()),
        ] {
            let at = off.saturating_add(field_off);
            if let Some(w) = block.get_mut(at..at.saturating_add(2)) {
                w.copy_from_slice(&bytes);
            }
        }
        let at = off.saturating_add(16);
        if let Some(w) = block.get_mut(at..at.saturating_add(8)) {
            w.copy_from_slice(&hash.to_le_bytes());
        }
        if let Some(w) = block.get_mut(hb..hb.saturating_add(2)) {
            w.copy_from_slice(&u16::try_from(entry_idx).unwrap_or(CHAIN_END).to_le_bytes());
        }
    }
    block
}

fn test_zap_micro(c: &mut Checks) -> KernelResult<()> {
    let block = build_micro(512, &[(b"ROOT", 34), (b"VERSION", 5)]);
    c.check(
        micro_lookup(&block, b"ROOT") == Ok(34),
        "microzap finds ROOT",
    );
    c.check(
        micro_lookup(&block, b"VERSION") == Ok(5),
        "microzap finds VERSION",
    );
    // A prefix must miss: microzap names are fixed-width and NUL-padded, so a
    // length-blind compare would match "ROO" against "ROOT".
    c.check_err(
        micro_lookup(&block, b"ROO"),
        KernelError::NotFound,
        "microzap rejects a prefix of a name",
    );

    let block = build_micro(512, &[(b"a", 1), (b"b", 2)]);
    let ents = micro_entries(&block);
    c.check(ents.len() == 2, "microzap iteration skips empty slots");
    if let Some(e) = ents.first() {
        c.check_bytes(&e.name, b"a", "microzap iteration reads the first name");
    }
    if let Some(e) = ents.get(1) {
        c.check_u64(e.value, 2, "microzap iteration reads the second value");
    }

    Ok(())
}

fn test_zap_hash(c: &mut Checks) -> KernelResult<()> {
    // Spot values of `zfs_crc64_table`, taken from the published reflected
    // polynomial rather than from this file's generator — a test that ran the
    // same code twice would prove nothing.
    c.check_u64(
        ZFS_CRC64_TABLE.first().copied().unwrap_or(1),
        0,
        "CRC-64 table entry 0",
    );
    c.check_u64(
        ZFS_CRC64_TABLE.get(1).copied().unwrap_or(0),
        0x01b0_0000_0000_0000,
        "CRC-64 table entry 1",
    );
    c.check_u64(
        ZFS_CRC64_TABLE.get(2).copied().unwrap_or(0),
        0x0360_0000_0000_0000,
        "CRC-64 table entry 2",
    );
    c.check_u64(
        ZFS_CRC64_TABLE.get(255).copied().unwrap_or(0),
        0x9090_0000_0000_0000,
        "CRC-64 table entry 255",
    );

    // Only the top `ZAP_HASHBITS` bits are kept; the rest must be cleared, or
    // the bucket index computed from the hash would be shifted by the
    // leftovers.
    let h = zap_hash(0x1234_5678_9abc_def0, b"somename");
    let low_mask = (1u64 << (64u32.saturating_sub(ZAP_HASHBITS))).saturating_sub(1);
    c.check_u64(h & low_mask, 0, "zap_hash keeps only the high bits");

    c.check(
        log2_exact(16384) == Some(14),
        "log2_exact of a power of two",
    );
    c.check(log2_exact(0).is_none(), "log2_exact rejects zero");
    c.check(
        log2_exact(1536).is_none(),
        "log2_exact rejects a non-power of two",
    );

    Ok(())
}

fn test_zap_leaf(c: &mut Checks) -> KernelResult<()> {
    // A 16 KiB leaf: 512 hash-table entries (1 KiB) after a 48-byte header.
    c.check(
        leaf_chunk_off(14, 0) == Some(48 + 512 * 2),
        "leaf chunks start after the header and hash table",
    );
    c.check(
        leaf_numchunks(14) == Some(638),
        "a 16 KiB leaf holds 638 chunks",
    );
    // The chunk array must end exactly at the end of the block — the check
    // that the geometry is right rather than merely plausible.
    c.check(
        leaf_chunk_off(14, 638) == Some(16384),
        "the chunk array ends exactly at the block end",
    );

    let leaf = build_leaf(9, 0, &[(b"ROOT", 8, &34u64.to_be_bytes())]);
    c.check(
        leaf_lookup(&leaf, 9, zap_hash(0, b"ROOT"), b"ROOT") == Ok(34),
        "fat leaf finds a single-object value",
    );
    c.check_err(
        leaf_lookup(&leaf, 9, zap_hash(0, b"ROO"), b"ROO"),
        KernelError::NotFound,
        "fat leaf misses an absent name",
    );

    // What the SA `LAYOUTS` ZAP stores: a layout number as the name, and the
    // attribute numbers it contains as a *big-endian* u16 array. 24 bytes
    // spans two chunks, so this also proves the chain is followed.
    let mut value = Vec::new();
    for a in 0u16..12 {
        value.extend_from_slice(&a.to_be_bytes());
    }
    c.check(
        value.len() > ZAP_LEAF_ARRAY_BYTES,
        "the layout value spans more than one chunk",
    );
    let layout_leaf = build_leaf(9, 0x99, &[(b"2", 2, &value)]);
    match leaf_lookup_array(&layout_leaf, 9, zap_hash(0x99, b"2"), b"2") {
        Ok(got) => {
            c.check(got.intlen == 2, "the layout array is 2 bytes wide");
            c.check(got.numints == 12, "the layout array holds 12 integers");
            let want: Vec<u16> = (0u16..12).collect();
            c.check(
                got.as_u16s() == want,
                "the layout array decodes big-endian across chunks",
            );
        }
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: layout array lookup: {:?}", e);
        }
    }

    // Iteration must reach entries in every bucket, not just the first chain.
    let leaf = build_leaf(
        9,
        7,
        &[
            (b"alpha", 8, &1u64.to_be_bytes()),
            (b"beta", 8, &2u64.to_be_bytes()),
            (b"gamma", 8, &3u64.to_be_bytes()),
        ],
    );
    let mut out = Vec::new();
    leaf_entries(&leaf, 9, &mut out);
    c.check(out.len() == 3, "fat leaf iteration sees every entry");
    let mut names: Vec<&[u8]> = out.iter().map(|e| e.name.as_slice()).collect();
    names.sort_unstable();
    c.check(
        names == vec![&b"alpha"[..], &b"beta"[..], &b"gamma"[..]],
        "fat leaf iteration reads every name",
    );

    // A name longer than one 21-byte chunk exercises the chain on the *name*
    // side, where a truncated compare would match the wrong entry.
    let long: &[u8] = b"a-name-that-does-not-fit-in-twenty-one-bytes";
    let leaf = build_leaf(9, 3, &[(long, 8, &77u64.to_be_bytes())]);
    c.check(
        leaf_lookup(&leaf, 9, zap_hash(3, long), long) == Ok(77),
        "a name longer than one chunk round trips",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// System Attributes
// ---------------------------------------------------------------------------

/// A registry with four known attributes, one of them variable-length.
fn sa_reg() -> SaRegistry {
    SaRegistry::from_attrs(vec![
        SaAttr {
            name: b"ZPL_MODE".to_vec(),
            num: 0,
            length: 8,
            bswap: 0,
        },
        SaAttr {
            name: b"ZPL_SIZE".to_vec(),
            num: 1,
            length: 8,
            bswap: 0,
        },
        SaAttr {
            name: b"ZPL_SYMLINK".to_vec(),
            num: 2,
            length: 0,
            bswap: 0,
        },
        SaAttr {
            name: b"ZPL_UID".to_vec(),
            num: 3,
            length: 8,
            bswap: 0,
        },
    ])
}

fn test_sa(c: &mut Checks) -> KernelResult<()> {
    c.check_bytes(&decimal_key(0), b"0", "layout key for 0");
    c.check_bytes(&decimal_key(2), b"2", "layout key for 2");
    c.check_bytes(&decimal_key(1234), b"1234", "layout key for 1234");

    // `sa_layout_info` packs the header size in units of 8 into the low 10
    // bits and the layout number above them.
    let mut buf = vec![0u8; 32];
    if let Some(w) = buf.get_mut(..4) {
        w.copy_from_slice(&SA_MAGIC.to_le_bytes());
    }
    let info: u16 = 2 | (7 << 10);
    if let Some(w) = buf.get_mut(4..6) {
        w.copy_from_slice(&info.to_le_bytes());
    }
    let hdr = parse_header(&buf)?;
    c.check(hdr.hdrsize == 16, "SA header size is decoded in units of 8");
    c.check(hdr.layout_num == 7, "SA layout number is decoded");

    // A legacy `znode_phys_t` bonus has no SA magic. It must be reported as
    // "not an SA set" so the caller falls back to the fixed offsets, rather
    // than being read as an SA set with a garbage layout number.
    let buf = vec![0u8; 264];
    c.check_err(
        parse_header(&buf),
        KernelError::InvalidArgument,
        "a legacy znode bonus is not an SA set",
    );

    // Variable-length attributes take their size from `sa_lengths[]`, which
    // follows the header at offset 6. Getting this wrong shifts every
    // attribute after the variable one — the exact silent-wrong-answer this
    // driver reads the registry to avoid.
    let mut buf = vec![0u8; 16];
    if let Some(w) = buf.get_mut(..4) {
        w.copy_from_slice(&SA_MAGIC.to_le_bytes());
    }
    let info: u16 = 2 | (3 << 10);
    if let Some(w) = buf.get_mut(4..6) {
        w.copy_from_slice(&info.to_le_bytes());
    }
    if let Some(w) = buf.get_mut(6..8) {
        w.copy_from_slice(&5u16.to_le_bytes());
    }
    let hdr = parse_header(&buf)?;
    // Layout MODE(8) SYMLINK(var = 5) SIZE(8): 0, 8 and 13.
    let map = SaMap::resolve(&hdr, &buf, &[0, 2, 1], &sa_reg())?;
    let mut values = vec![0u8; 21];
    if let Some(w) = values.get_mut(..8) {
        w.copy_from_slice(&0o100_644u64.to_le_bytes());
    }
    if let Some(w) = values.get_mut(8..13) {
        w.copy_from_slice(b"a/b/c");
    }
    if let Some(w) = values.get_mut(13..21) {
        w.copy_from_slice(&4096u64.to_le_bytes());
    }
    c.check(
        map.get_uint(&values, 0) == Some(0o100_644),
        "the attribute before the variable one is at 0",
    );
    c.check(
        map.get(&values, 2) == Some(&b"a/b/c"[..]),
        "the variable-length attribute takes its length from sa_lengths",
    );
    c.check(
        map.get_uint(&values, 1) == Some(4096),
        "the attribute after the variable one is shifted by its length",
    );

    // An attribute the registry does not know has no width, so the offset of
    // everything after it is unknowable. Guessing is the failure mode; the
    // whole map must be refused instead.
    let mut buf = vec![0u8; 8];
    if let Some(w) = buf.get_mut(..4) {
        w.copy_from_slice(&SA_MAGIC.to_le_bytes());
    }
    if let Some(w) = buf.get_mut(4..6) {
        w.copy_from_slice(&1u16.to_le_bytes());
    }
    let hdr = parse_header(&buf)?;
    c.check_err(
        SaMap::resolve(&hdr, &buf, &[0, 99], &sa_reg()),
        KernelError::CorruptedData,
        "an unregistered attribute is corruption, not a guess",
    );

    let r = sa_reg();
    c.check(r.num_of(b"ZPL_SIZE") == Some(1), "registry finds a name");
    c.check(
        r.num_of(b"ZPL_NOSUCH").is_none(),
        "registry misses an unknown name",
    );
    c.check(
        r.length_of(2) == Some(0),
        "a variable-length attribute declares width 0",
    );
    c.check(!r.is_empty(), "a populated registry is not empty");

    Ok(())
}

// ---------------------------------------------------------------------------
// A whole pool, built and mounted
// ---------------------------------------------------------------------------
//
// The groups above check one structure at a time against bytes written by
// hand. That is necessary but not sufficient: every one of them could pass
// while the *chain* between them is wrong, because the chain is where ZFS
// keeps its difficulty. An uberblock names a block pointer, which names an
// objset, whose meta-dnode names a dnode array, an index into which names a
// DSL directory, whose bonus names a dataset, whose bonus names another
// objset, whose object 1 names a ZAP, one of whose entries names the root
// directory. Nine dereferences, each with its own encoding, before a single
// file name has been read.
//
// So this section builds a complete pool in RAM — real checksums, real block
// pointers, a real fatzap, a real spill block — mounts it through
// `MemorySource`, and reads files out of it. Three things it does on purpose,
// because each is a place a plausible-but-wrong driver passes anyway:
//
//  * **The winning uberblock is not the first one.** Slot 3 holds an older
//    txg with a valid checksum; the live pool is in slot 11. A reader that
//    took the first valid slot rather than the highest txg mounts the decoy.
//  * **The attribute numbers are not upstream's.** `ZPL_MODE` is 7 here, not
//    5. A driver carrying the usual hardcoded table finds nothing.
//  * **One file's layout is in a different order.** `/hello` uses a layout
//    that puts `ZPL_SIZE` at byte 56 rather than byte 8, so a driver that
//    assumed the default layout reports its creation time as its size — a
//    wrong answer, not an error. That is exactly the failure
//    `design-decisions.md` §245 chose to spend several hundred lines
//    avoiding, and this is the check that proves the spending worked.

/// Bytes of the image: the format's own 4 MiB label-and-boot reserve, which
/// every DVA offset is measured from, plus room for the pool's blocks. Nothing
/// smaller is possible — a DVA offset of 0 *means* byte 4 MiB.
const IMG_LEN: usize = 4 * 1024 * 1024 + 128 * 1024;

/// `log2` of the vdev's sector size. 9 gives 1024-byte uberblock slots and 128
/// of them per label, which is what a pool on a 512-byte-sector disk looks
/// like.
const IMG_ASHIFT: u64 = 9;

/// The transaction group the live pool is at.
const IMG_TXG: u64 = 77;
/// The decoy uberblock's transaction group. Lower, so it must lose.
const IMG_OLD_TXG: u64 = 5;
/// Slot the live uberblock is written to, deliberately not slot 0.
const IMG_UB_SLOT: usize = 11;
/// Slot the decoy is written to, deliberately *before* the live one.
const IMG_OLD_UB_SLOT: usize = 3;

const IMG_GUID: u64 = 0x5111_7E57_0000_0001;
const IMG_TIMESTAMP: u64 = 1_700_000_000;
const IMG_POOL_NAME: &[u8] = b"selftest";

/// Size of the dnode array block in both objsets: 32 objects per block.
const IMG_DN_BLK: usize = 16 * 1024;

const HELLO_TEXT: &[u8] = b"Hello from a synthetic ZFS pool.\n";
const NESTED_TEXT: &[u8] = b"one level down\n";

/// Object numbers in the filesystem objset. Object 1 is fixed by the format;
/// the rest are this image's choice.
const OBJ_MASTER: u64 = 1;
const OBJ_SA_MASTER: u64 = 2;
const OBJ_REGISTRY: u64 = 3;
const OBJ_LAYOUTS: u64 = 4;
const OBJ_ROOT: u64 = 5;
const OBJ_HELLO: u64 = 6;
const OBJ_SUB: u64 = 7;
const OBJ_LINK: u64 = 8;
const OBJ_BIG: u64 = 9;
const OBJ_NESTED: u64 = 10;
const OBJ_DIRLINK: u64 = 11;

/// `/big` spans this many blocks of `IMG_BIG_BLKSZ`, one of which is a hole.
const IMG_BIG_BLOCKS: u64 = 6;
const IMG_BIG_BLKSZ: usize = 512;
/// The block id `/big` leaves unwritten, so a sparse read has to be right.
const IMG_BIG_HOLE: u64 = 2;

// The attribute numbers this pool registers. Upstream ZFS assigns ZPL_ATIME 0,
// ZPL_MTIME 1, ... ZPL_MODE 5, ZPL_SIZE 6 and so on; these are deliberately
// different, because the numbers are per-dataset and a driver that treats them
// as constants is reading a table it was never given.
const A_SYMLINK: u16 = 0;
const A_LINKS: u16 = 1;
const A_UID: u16 = 2;
const A_SIZE: u16 = 3;
const A_MTIME: u16 = 4;
const A_PARENT: u16 = 5;
const A_CRTIME: u16 = 6;
const A_MODE: u16 = 7;
const A_CTIME: u16 = 8;
const A_GID: u16 = 9;
const A_FLAGS: u16 = 10;
const A_GEN: u16 = 11;
const A_ATIME: u16 = 12;
const A_RDEV: u16 = 13;

/// `IFTODT(S_IFREG)`, which the ZPL stores in a directory entry's top bits.
const IMG_DT_REG: u64 = 8;

/// The registry rows the image writes: name, number, and on-disk width in
/// bytes. A width of 0 marks a variable-length attribute whose real width is
/// carried per-object in `sa_lengths[]`.
fn img_registry() -> [(&'static [u8], u16, u16); 14] {
    [
        (b"ZPL_SYMLINK", A_SYMLINK, 0),
        (b"ZPL_LINKS", A_LINKS, 8),
        (b"ZPL_UID", A_UID, 8),
        (b"ZPL_SIZE", A_SIZE, 8),
        (b"ZPL_MTIME", A_MTIME, 16),
        (b"ZPL_PARENT", A_PARENT, 8),
        (b"ZPL_CRTIME", A_CRTIME, 16),
        (b"ZPL_MODE", A_MODE, 8),
        (b"ZPL_CTIME", A_CTIME, 16),
        (b"ZPL_GID", A_GID, 8),
        (b"ZPL_FLAGS", A_FLAGS, 8),
        (b"ZPL_GEN", A_GEN, 8),
        (b"ZPL_ATIME", A_ATIME, 16),
        (b"ZPL_RDEV", A_RDEV, 8),
    ]
}

/// Layout 1: the attribute order upstream uses for a freshly created object.
const IMG_LAYOUT_1: [u16; 12] = [
    A_MODE, A_SIZE, A_GEN, A_UID, A_GID, A_PARENT, A_FLAGS, A_ATIME, A_MTIME, A_CTIME, A_CRTIME,
    A_LINKS,
];

/// Layout 2: the same twelve attributes in a different order, so that every
/// one of them lands at a different byte offset than layout 1 puts it.
const IMG_LAYOUT_2: [u16; 12] = [
    A_LINKS, A_CRTIME, A_MODE, A_CTIME, A_UID, A_SIZE, A_MTIME, A_GID, A_FLAGS, A_GEN, A_ATIME,
    A_PARENT,
];

/// Layout 3: the bonus half of an object whose attributes did not all fit.
const IMG_LAYOUT_3: [u16; 5] = [A_MODE, A_SIZE, A_LINKS, A_UID, A_GID];

/// Layout 4: the spill half — one variable-length attribute and nothing else.
const IMG_LAYOUT_4: [u16; 1] = [A_SYMLINK];

/// Layout 5: layout 3 plus an inline symlink target, for the link that does
/// *not* spill.
const IMG_LAYOUT_5: [u16; 6] = [A_MODE, A_SIZE, A_LINKS, A_UID, A_GID, A_SYMLINK];

// ---------------------------------------------------------------------------
// Little-endian scalar writers. Each ignores a write that would not fit rather
// than panicking, which keeps the builders free of indexing and keeps a
// mistake in one of them from taking the kernel down before the other groups
// have printed.

fn put_u8(buf: &mut [u8], off: usize, v: u8) {
    if let Some(b) = buf.get_mut(off) {
        *b = v;
    }
}

fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    if let Some(w) = buf.get_mut(off..off.saturating_add(2)) {
        w.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    if let Some(w) = buf.get_mut(off..off.saturating_add(4)) {
        w.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_u64(buf: &mut [u8], off: usize, v: u64) {
    if let Some(w) = buf.get_mut(off..off.saturating_add(8)) {
        w.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_bytes(buf: &mut [u8], off: usize, v: &[u8]) {
    if let Some(w) = buf.get_mut(off..off.saturating_add(v.len())) {
        w.copy_from_slice(v);
    }
}

/// Round `data` up to a whole number of 512-byte sectors, which is the only
/// granularity a DVA can express.
fn pad512(data: &[u8]) -> Vec<u8> {
    let len = data.len().max(1).saturating_add(511) & !511usize;
    let mut out = vec![0u8; len];
    put_bytes(&mut out, 0, data);
    out
}

/// Encode a 128-byte `blkptr_t` for an uncompressed block of `data` written at
/// `dva_off`.
///
/// Written from the `blkptr_t` diagram rather than by inverting
/// [`super::raw::parse_blkptr`]: if the two shared a bias-and-shift helper, an
/// off-by-one in it would cancel out and every check below would pass against
/// an image no real pool resembles.
///
/// The checksum is computed here because [`Reader::read_block`] verifies every
/// block it reads. An image with placeholder checksums would fail at the first
/// dereference with `IoError` and tell us nothing about anything else.
fn make_blkptr(dva_off: u64, data: &[u8], obj_type: u8, level: u8, alg: u8) -> Vec<u8> {
    let mut bp = vec![0u8; BLKPTR_LEN];
    let sectors = u64::try_from(data.len()).unwrap_or(0) >> SPA_MINBLOCKSHIFT;

    // DVA word 0: ASIZE in 512-byte units at bits 0..24, vdev at 32..64 (0).
    put_u64(&mut bp, 0, sectors);
    // DVA word 1: offset in 512-byte units at bits 0..63, the gang bit at 63.
    put_u64(&mut bp, 8, dva_off >> SPA_MINBLOCKSHIFT);

    // LSIZE and PSIZE are stored as `(bytes >> 9) - 1`, so one sector is 0.
    let sz = sectors.saturating_sub(1);
    let props = sz
        | (sz << 16)
        | (u64::from(ZIO_COMPRESS_OFF) << 32)
        | (u64::from(alg) << 40)
        | (u64::from(obj_type) << 48)
        | (u64::from(level) << 56)
        // The `B` bit: this pool was written little-endian. Without it
        // `read_block` refuses the pool as foreign-endian, which is the
        // correct behaviour and would fail every check below.
        | (1u64 << 63);
    put_u64(&mut bp, 48, props);

    put_u64(&mut bp, 72, IMG_TXG); // blk_phys_birth
    put_u64(&mut bp, 80, IMG_TXG); // blk_birth
    put_u64(&mut bp, 88, 1); // blk_fill

    let cksum = compute(alg, data).unwrap_or([0u64; 4]);
    for (i, word) in cksum.iter().enumerate() {
        put_u64(&mut bp, 96usize.saturating_add(i.saturating_mul(8)), *word);
    }
    bp
}

/// The image under construction: its bytes, and the next free DVA offset.
struct PoolImage {
    bytes: Vec<u8>,
    next: u64,
}

impl PoolImage {
    fn new() -> Self {
        Self {
            bytes: vec![0u8; IMG_LEN],
            next: 0,
        }
    }

    /// Place `data` in the allocatable area and return a block pointer to it.
    ///
    /// Allocation is a bump from the end of the 4 MiB reserve — a real pool
    /// allocates from space maps, but nothing on the read path can tell the
    /// difference, and a space-map allocator in a test would be testing
    /// itself.
    fn put(&mut self, data: &[u8], obj_type: u8, level: u8, alg: u8) -> Vec<u8> {
        let dva_off = self.next;
        self.next = self
            .next
            .saturating_add(u64::try_from(data.len()).unwrap_or(0));
        let at = usize::try_from(VDEV_LABEL_START_SIZE.saturating_add(dva_off)).unwrap_or(0);
        put_bytes(&mut self.bytes, at, data);
        make_blkptr(dva_off, data, obj_type, level, alg)
    }

    /// Absolute byte offset of the block a [`Self::put`] at `dva_off` wrote.
    fn abs(dva_off: u64) -> usize {
        usize::try_from(VDEV_LABEL_START_SIZE.saturating_add(dva_off)).unwrap_or(0)
    }
}

/// One object's dnode, in the terms the on-disk slot stores it.
struct DnodeSpec<'a> {
    obj_type: u8,
    bonustype: u8,
    /// `log2` of the indirect block size; unused when `nlevels` is 1.
    indblkshift: u8,
    nlevels: u8,
    datablksz: usize,
    maxblkid: u64,
    /// Bytes the object occupies, which is what `stat` reports as `blocks`.
    used: u64,
    blkptrs: &'a [Vec<u8>],
    bonus: &'a [u8],
    spill: Option<&'a [u8]>,
}

impl DnodeSpec<'_> {
    /// A dnode with nothing set: every builder below starts here and overrides
    /// only the fields it cares about, so a new field added to the struct does
    /// not silently default to garbage in a dozen places.
    const fn blank() -> Self {
        Self {
            obj_type: DMU_OT_NONE,
            bonustype: DMU_OT_NONE,
            indblkshift: 14,
            nlevels: 1,
            datablksz: 512,
            maxblkid: 0,
            used: 512,
            blkptrs: &[],
            bonus: &[],
            spill: None,
        }
    }
}

/// Write `spec` into the dnode array at object number `obj`.
fn put_dnode(arr: &mut [u8], obj: u64, spec: &DnodeSpec<'_>) {
    let base = usize::try_from(obj).unwrap_or(0).saturating_mul(DNODE_LEN);
    put_u8(arr, base, spec.obj_type);
    put_u8(arr, base.saturating_add(1), spec.indblkshift);
    put_u8(arr, base.saturating_add(2), spec.nlevels);
    put_u8(
        arr,
        base.saturating_add(3),
        u8::try_from(spec.blkptrs.len()).unwrap_or(1).max(1),
    );
    put_u8(arr, base.saturating_add(4), spec.bonustype);
    // `dn_flags`; `DNODE_FLAG_SPILL_BLKPTR` is bit 2.
    put_u8(
        arr,
        base.saturating_add(7),
        if spec.spill.is_some() { 0x04 } else { 0 },
    );
    put_u16(
        arr,
        base.saturating_add(8),
        u16::try_from(spec.datablksz >> 9).unwrap_or(1),
    );
    put_u16(
        arr,
        base.saturating_add(10),
        u16::try_from(spec.bonus.len()).unwrap_or(0),
    );
    put_u64(arr, base.saturating_add(16), spec.maxblkid);
    put_u64(arr, base.saturating_add(24), spec.used);

    for (i, bp) in spec.blkptrs.iter().enumerate() {
        let at = base
            .saturating_add(DNODE_CORE_LEN)
            .saturating_add(i.saturating_mul(BLKPTR_LEN));
        put_bytes(arr, at, bp);
    }

    // The bonus buffer begins after the inline block pointers — after *at
    // least one*, because a dnode with none is unallocated.
    let nblkptr = spec.blkptrs.len().max(1);
    let bonus_at = base
        .saturating_add(DNODE_CORE_LEN)
        .saturating_add(nblkptr.saturating_mul(BLKPTR_LEN));
    put_bytes(arr, bonus_at, spec.bonus);

    if let Some(sp) = spec.spill {
        // The spill pointer is the last block pointer's worth of the dnode's
        // slots, which is why it is placed from the end and not from
        // `nblkptr`. A single-slot dnode puts it at byte 384.
        let at = base.saturating_add(DNODE_LEN).saturating_sub(BLKPTR_LEN);
        put_bytes(arr, at, sp);
    }
}

/// The attribute values one object carries, before a layout fixes where each
/// one lands.
struct Attrs {
    mode: u64,
    size: u64,
    links: u64,
    uid: u64,
    gid: u64,
    parent: u64,
    /// `gen` is a reserved word in Rust 2024, so the field cannot use the
    /// attribute's own name.
    generation: u64,
    flags: u64,
    atime: u64,
    mtime: u64,
    ctime: u64,
    crtime: u64,
}

impl Attrs {
    /// A plausible set for object `obj`, differing per object so that reading
    /// the wrong object's attributes shows up as a wrong value rather than as
    /// the same value twice.
    fn for_obj(obj: u64, mode: u64, size: u64, parent: u64) -> Self {
        Self {
            mode,
            size,
            links: 1,
            uid: 1000,
            gid: 1000,
            parent,
            generation: obj,
            flags: 0,
            atime: IMG_TIMESTAMP.saturating_add(obj),
            mtime: IMG_TIMESTAMP.saturating_add(obj).saturating_add(100),
            ctime: IMG_TIMESTAMP.saturating_add(obj).saturating_add(200),
            crtime: IMG_TIMESTAMP.saturating_add(obj).saturating_add(300),
        }
    }
}

/// Serialise `at` in `layout` order, which is what fixes every attribute's
/// byte offset within the value area.
///
/// A timestamp is two 64-bit values — seconds then nanoseconds — which is why
/// its width is 16 and not 8, and why a reader that assumed 8 would find the
/// next attribute inside the nanoseconds field.
fn sa_values(layout: &[u16], at: &Attrs, symlink: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let time = |secs: u64, out: &mut Vec<u8>| {
        out.extend_from_slice(&secs.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    };
    for &num in layout {
        match num {
            A_MODE => out.extend_from_slice(&at.mode.to_le_bytes()),
            A_SIZE => out.extend_from_slice(&at.size.to_le_bytes()),
            A_LINKS => out.extend_from_slice(&at.links.to_le_bytes()),
            A_UID => out.extend_from_slice(&at.uid.to_le_bytes()),
            A_GID => out.extend_from_slice(&at.gid.to_le_bytes()),
            A_PARENT => out.extend_from_slice(&at.parent.to_le_bytes()),
            A_GEN => out.extend_from_slice(&at.generation.to_le_bytes()),
            A_FLAGS => out.extend_from_slice(&at.flags.to_le_bytes()),
            A_RDEV => out.extend_from_slice(&0u64.to_le_bytes()),
            A_ATIME => time(at.atime, &mut out),
            A_MTIME => time(at.mtime, &mut out),
            A_CTIME => time(at.ctime, &mut out),
            A_CRTIME => time(at.crtime, &mut out),
            A_SYMLINK => out.extend_from_slice(symlink),
            _ => {}
        }
    }
    out
}

/// Build one System Attribute buffer: an `sa_hdr_phys_t`, then the values.
///
/// `var_lens` are the widths of the variable-length attributes in the order
/// the layout lists them. That ordering is the entire contract between
/// `sa_lengths[]` and the layout, and it is what `SaMap::resolve` walks.
fn sa_buffer(layout_num: u16, var_lens: &[u16], values: &[u8]) -> Vec<u8> {
    // `sa_lengths[]` starts at byte 6, and the header is a whole number of
    // eight-byte units because that is how `SA_HDR_SIZE` is encoded.
    let hdrsize = 6usize
        .saturating_add(var_lens.len().saturating_mul(2))
        .saturating_add(7)
        & !7usize;
    let mut buf = vec![0u8; hdrsize];
    put_u32(&mut buf, 0, SA_MAGIC);
    // `sa_layout_info`: the header size in eight-byte units at bits 0..10, the
    // layout number above it.
    let info = u16::try_from(hdrsize >> 3).unwrap_or(1) | (layout_num << 10);
    put_u16(&mut buf, 4, info);
    for (i, len) in var_lens.iter().enumerate() {
        put_u16(&mut buf, 6usize.saturating_add(i.saturating_mul(2)), *len);
    }
    buf.extend_from_slice(values);
    buf
}

/// Build the two blocks of a fatzap that has never split: the header, whose
/// second half is the embedded pointer table, and the single leaf.
///
/// `zt_shift` is 0, so the table has one entry and every hash lands in that
/// leaf. That is what a four-key ZAP actually looks like — and it is the only
/// shape this driver supports, since an *external* pointer table is refused by
/// name.
fn build_fatzap(blkshift: u32, salt: u64, ents: &[(&[u8], u8, &[u8])]) -> (Vec<u8>, Vec<u8>) {
    let size = 1usize << blkshift;
    let mut header = vec![0u8; size];
    put_u64(&mut header, 0, ZBT_HEADER);
    put_u64(&mut header, 8, ZAP_MAGIC);
    // `zap_ptrtbl.zt_blk` and `zt_numblks` stay 0: the table is embedded.
    put_u64(&mut header, 32, 0); // zt_shift
    put_u64(&mut header, 72, salt);
    // The embedded table occupies the second half of the block; its one entry
    // is the block id of the leaf.
    put_u64(&mut header, size >> 1, 1);
    (header, build_leaf(blkshift, salt, ents))
}

/// A directory ZAP value: the object number in the low 48 bits, the `IFTODT`
/// type in bits 60..64.
const fn dirent(ty: u64, obj: u64) -> u64 {
    (ty << 60) | obj
}

/// The parts of a label configuration the tests vary.
///
/// Every field here is one the mounting path screens on, so each rejection
/// case can override exactly one of them against [`ConfigSpec::disk`] and the
/// name of the case says what is being tested. Positional arguments would not:
/// `build_config(b"disk", 0, 1, 0, 5000)` is unreadable, and adding a sixth
/// screen would silently renumber every existing call.
struct ConfigSpec<'a> {
    /// `vdev_tree.type`: `disk`, `file`, `mirror`, `raidz`…
    vdev_type: &'a [u8],
    /// `vdev_tree.id` — this top-level vdev's index within the pool.
    id: u64,
    /// `vdev_tree.is_log` — non-zero marks a separate intent-log device.
    is_log: u64,
    /// `state` — `POOL_STATE_*`.
    state: u64,
    /// `version` — the SPA version.
    version: u64,
    /// `vdev_tree.children` — `0` emits no `children` key at all, which is what
    /// a leaf vdev's label looks like. `n > 0` emits an `nvlist_array` of `n`
    /// leaf children, which is what a mirror's does.
    children: u32,
    /// Whether `ashift` is written on the *top-level* vdev. A label that
    /// records it only on the legs is the case the child fallback exists for,
    /// and it is reachable only by turning this off.
    ashift_on_top: bool,
}

impl ConfigSpec<'_> {
    /// An ordinary, mountable single-disk pool. Every image builds from this,
    /// and every case below overrides one field of it.
    const fn disk() -> Self {
        Self {
            vdev_type: b"disk",
            id: 0,
            is_log: 0,
            state: POOL_STATE_ACTIVE,
            version: SPA_VERSION_FEATURES,
            children: 0,
            ashift_on_top: true,
        }
    }

    /// A two-way mirror. The image bytes are unchanged — which is the point:
    /// a mirror leg *is* a complete copy of the pool's address space, so the
    /// same device that mounts as a single disk must mount as a mirror leg and
    /// read exactly the same files.
    const fn mirror() -> Self {
        Self {
            vdev_type: b"mirror",
            children: 2,
            ..Self::disk()
        }
    }
}

/// One leg of a mirror, as it appears in the top-level vdev's `children`
/// array: a leaf `disk` vdev with its own index within the mirror.
///
/// Each leg carries `ashift` as well. Real labels record it on the top-level
/// vdev, but recording it here too is what lets a single flag in
/// [`ConfigSpec`] produce the leaves-only label the fallback is for.
fn build_leaf_vdev(id: u64) -> Vec<u8> {
    let mut out = Vec::new();
    // A nested list carries its own version and nvflag, but no encoding byte.
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&nv_pair(
        b"type",
        DATA_TYPE_STRING,
        1,
        &xdr_string(b"disk"),
    ));
    out.extend_from_slice(&nv_pair_u64(b"id", id));
    out.extend_from_slice(&nv_pair_u64(b"ashift", IMG_ASHIFT));
    out.extend_from_slice(&0u32.to_be_bytes()); // terminator
    out
}

/// The pool configuration nvlist a vdev label carries.
fn build_config(spec: &ConfigSpec<'_>) -> Vec<u8> {
    let mut vdev = Vec::new();
    // A nested list carries its own version and nvflag, but no encoding byte —
    // it inherits the outer list's.
    vdev.extend_from_slice(&0u32.to_be_bytes());
    vdev.extend_from_slice(&1u32.to_be_bytes());
    vdev.extend_from_slice(&nv_pair(
        b"type",
        DATA_TYPE_STRING,
        1,
        &xdr_string(spec.vdev_type),
    ));
    // Written unconditionally, including in the ordinary `id: 0, is_log: 0`
    // case: a real label always carries both, and a builder that emitted them
    // only when interesting would leave the mounting path never once reading
    // the shape it will actually meet on disk.
    vdev.extend_from_slice(&nv_pair_u64(b"id", spec.id));
    vdev.extend_from_slice(&nv_pair_u64(b"is_log", spec.is_log));
    if spec.ashift_on_top {
        vdev.extend_from_slice(&nv_pair_u64(b"ashift", IMG_ASHIFT));
    }
    vdev.extend_from_slice(&nv_pair_u64(b"asize", u64::try_from(IMG_LEN).unwrap_or(0)));
    if spec.children > 0 {
        // An XDR nvlist array's value is its elements' encodings back to back,
        // each with its own version/nvflag header and terminator — there is no
        // pointer table, which is what the NV_ENCODE_NATIVE form would use.
        let mut legs = Vec::new();
        for leg in 0..spec.children {
            legs.extend_from_slice(&build_leaf_vdev(u64::from(leg)));
        }
        vdev.extend_from_slice(&nv_pair(
            b"children",
            DATA_TYPE_NVLIST_ARRAY,
            spec.children,
            &legs,
        ));
    }
    vdev.extend_from_slice(&0u32.to_be_bytes()); // terminator

    let mut out = nv_header();
    out.extend_from_slice(&nv_pair(
        b"name",
        DATA_TYPE_STRING,
        1,
        &xdr_string(IMG_POOL_NAME),
    ));
    out.extend_from_slice(&nv_pair_u64(b"pool_guid", IMG_GUID));
    out.extend_from_slice(&nv_pair_u64(b"txg", IMG_TXG));
    out.extend_from_slice(&nv_pair_u64(b"version", spec.version));
    // Like `id` and `is_log`: always written, so the ordinary mount exercises
    // the same key the rejection cases below vary.
    out.extend_from_slice(&nv_pair_u64(b"state", spec.state));
    out.extend_from_slice(&nv_pair(b"vdev_tree", DATA_TYPE_NVLIST, 1, &vdev));
    out.extend_from_slice(&0u32.to_be_bytes()); // terminator
    out
}

/// Write one uberblock slot at absolute offset `at`.
///
/// An uberblock is self-checksummed: the SHA-256 is taken over the slot with
/// its own checksum field replaced by the *byte offset it was written at*.
/// That is what binds a copy to its slot — an uberblock memcpy'd elsewhere
/// fails to verify, which is exactly what stops a stale label being mistaken
/// for a live one.
fn put_uberblock(bytes: &mut [u8], at: usize, txg: u64, rootbp: &[u8]) {
    let mut slot = vec![0u8; 1024];
    put_u64(&mut slot, 0, UBERBLOCK_MAGIC);
    put_u64(&mut slot, 8, SPA_VERSION_FEATURES);
    put_u64(&mut slot, 16, txg);
    put_u64(&mut slot, 24, IMG_GUID);
    put_u64(&mut slot, 32, IMG_TIMESTAMP);
    put_bytes(&mut slot, 40, rootbp);

    let eck = slot.len().saturating_sub(ZEC_LEN);
    put_u64(&mut slot, eck, ZEC_MAGIC);
    // The verifier: the block's own offset, then three zero words (already
    // zero here, since the slot was allocated zeroed).
    put_u64(
        &mut slot,
        eck.saturating_add(8),
        u64::try_from(at).unwrap_or(0),
    );
    let cksum = sha256_cksum(&slot);
    for (i, word) in cksum.iter().enumerate() {
        let off = eck.saturating_add(8).saturating_add(i.saturating_mul(8));
        put_u64(&mut slot, off, *word);
    }
    put_bytes(bytes, at, &slot);
}

/// A built image, plus the few absolute offsets a damage test needs.
struct Pool {
    bytes: Vec<u8>,
    /// Where `/hello`'s only data block landed, for the corruption check.
    hello_at: usize,
    /// Where the vdev label nvlist starts in label 0.
    nvlist_at: usize,
    /// Where the uberblock array starts in label 0.
    ub_array_at: usize,
}

/// Build a complete, mountable single-vdev pool.
///
/// Blocks are written leaves-first throughout, because a block pointer carries
/// its target's checksum: nothing can be encoded before the bytes it addresses
/// exist.
#[allow(clippy::too_many_lines)]
fn build_pool() -> Pool {
    let mut img = PoolImage::new();

    // --- File data -------------------------------------------------------

    let hello_dva = img.next;
    let hello_bp = img.put(
        &pad512(HELLO_TEXT),
        DMU_OT_PLAIN_FILE_CONTENTS,
        0,
        ZIO_CHECKSUM_FLETCHER_4,
    );
    let nested_bp = img.put(
        &pad512(NESTED_TEXT),
        DMU_OT_PLAIN_FILE_CONTENTS,
        0,
        ZIO_CHECKSUM_FLETCHER_4,
    );

    // `/big` is two levels deep with a hole in the middle: `indblkshift` 9
    // means four pointers per indirect block, so six blocks need two indirect
    // blocks and two inline pointers in the dnode.
    let mut big_bps: Vec<Vec<u8>> = Vec::new();
    for blkid in 0..IMG_BIG_BLOCKS {
        if blkid == IMG_BIG_HOLE {
            // A hole is an all-zero block pointer, not a pointer to zeroes.
            big_bps.push(vec![0u8; BLKPTR_LEN]);
            continue;
        }
        let fill = u8::try_from(0xA0u64.saturating_add(blkid)).unwrap_or(0xA0);
        big_bps.push(img.put(
            &vec![fill; IMG_BIG_BLKSZ],
            DMU_OT_PLAIN_FILE_CONTENTS,
            0,
            ZIO_CHECKSUM_FLETCHER_4,
        ));
    }
    let mut big_ind: Vec<Vec<u8>> = Vec::new();
    for chunk in big_bps.chunks(4) {
        let mut block = vec![0u8; 512];
        for (i, bp) in chunk.iter().enumerate() {
            put_bytes(&mut block, i.saturating_mul(BLKPTR_LEN), bp);
        }
        big_ind.push(img.put(
            &block,
            DMU_OT_PLAIN_FILE_CONTENTS,
            1,
            ZIO_CHECKSUM_FLETCHER_4,
        ));
    }
    let big_size = u64::try_from(IMG_BIG_BLKSZ)
        .unwrap_or(512)
        .saturating_mul(IMG_BIG_BLOCKS);

    // --- The spilled symlink target --------------------------------------
    //
    // `/dirlink`'s bonus holds its mode and size under layout 3; its target
    // lives in a spill block under layout 4. The two are *independent*
    // attribute sets, each with its own header — which is the thing a reader
    // gets wrong by concatenating them.
    let spill_len = u16::try_from(b"sub".len()).unwrap_or(0);
    let spill_bp = img.put(
        &pad512(&sa_buffer(4, &[spill_len], b"sub")),
        DMU_OT_SA,
        0,
        ZIO_CHECKSUM_SHA256,
    );

    // --- ZAPs ------------------------------------------------------------

    let root_zap = build_micro(
        1024,
        &[
            (b"hello", dirent(IMG_DT_REG, OBJ_HELLO)),
            (b"sub", dirent(4, OBJ_SUB)),
            (b"link", dirent(10, OBJ_LINK)),
            (b"big", dirent(IMG_DT_REG, OBJ_BIG)),
            (b"dirlink", dirent(10, OBJ_DIRLINK)),
        ],
    );
    let root_bp = img.put(&root_zap, DMU_OT_DIRECTORY_CONTENTS, 0, ZIO_CHECKSUM_SHA256);

    let sub_zap = build_micro(512, &[(b"nested", dirent(IMG_DT_REG, OBJ_NESTED))]);
    let sub_bp = img.put(&sub_zap, DMU_OT_DIRECTORY_CONTENTS, 0, ZIO_CHECKSUM_SHA256);

    let master_zap = build_micro(
        512,
        &[
            (b"ROOT", OBJ_ROOT),
            (b"VERSION", 5),
            (b"SA_ATTRS", OBJ_SA_MASTER),
        ],
    );
    let master_bp = img.put(&master_zap, DMU_OT_MASTER_NODE, 0, ZIO_CHECKSUM_SHA256);

    let sa_master_zap = build_micro(
        512,
        &[(b"REGISTRY", OBJ_REGISTRY), (b"LAYOUTS", OBJ_LAYOUTS)],
    );
    let sa_master_bp = img.put(
        &sa_master_zap,
        DMU_OT_SA_MASTER_NODE,
        0,
        ZIO_CHECKSUM_SHA256,
    );

    // The registry's value packs the byteswap class into bits 0..8, the width
    // into 8..24 and the number into 24..40.
    let reg_pairs: Vec<(&[u8], u64)> = img_registry()
        .iter()
        .map(|&(name, num, len)| (name, (u64::from(len) << 8) | (u64::from(num) << 24)))
        .collect();
    let registry_zap = build_micro(2048, &reg_pairs);
    let registry_bp = img.put(
        &registry_zap,
        DMU_OT_SA_ATTR_REGISTRATION,
        0,
        ZIO_CHECKSUM_SHA256,
    );

    // The layout table must be a fatzap: its values are arrays of `u16`, and a
    // microzap slot holds exactly one `u64`. This is the one place in the
    // driver that needs an array-valued ZAP entry, so it is the one place a
    // microzap cannot stand in.
    let l1 = layout_bytes(&IMG_LAYOUT_1);
    let l2 = layout_bytes(&IMG_LAYOUT_2);
    let l3 = layout_bytes(&IMG_LAYOUT_3);
    let l4 = layout_bytes(&IMG_LAYOUT_4);
    let l5 = layout_bytes(&IMG_LAYOUT_5);
    // 1024-byte blocks, not 512: five layouts need seventeen leaf chunks and a
    // 512-byte leaf has eighteen. Overflowing would not fail — `build_leaf`
    // would drop the entries it could not place — so the margin is deliberate
    // rather than incidental.
    let (lay_hdr, lay_leaf) = build_fatzap(
        10,
        0x1234_5678_9abc_def0,
        &[
            (b"1", 2, &l1),
            (b"2", 2, &l2),
            (b"3", 2, &l3),
            (b"4", 2, &l4),
            (b"5", 2, &l5),
        ],
    );
    let lay_hdr_bp = img.put(&lay_hdr, DMU_OT_SA_ATTR_LAYOUTS, 0, ZIO_CHECKSUM_SHA256);
    let lay_leaf_bp = img.put(&lay_leaf, DMU_OT_SA_ATTR_LAYOUTS, 0, ZIO_CHECKSUM_SHA256);

    // --- The filesystem objset's dnode array -----------------------------

    let mut fs_dn = vec![0u8; IMG_DN_BLK];

    put_dnode(
        &mut fs_dn,
        OBJ_MASTER,
        &DnodeSpec {
            obj_type: DMU_OT_MASTER_NODE,
            blkptrs: &[master_bp],
            ..DnodeSpec::blank()
        },
    );
    put_dnode(
        &mut fs_dn,
        OBJ_SA_MASTER,
        &DnodeSpec {
            obj_type: DMU_OT_SA_MASTER_NODE,
            blkptrs: &[sa_master_bp],
            ..DnodeSpec::blank()
        },
    );
    put_dnode(
        &mut fs_dn,
        OBJ_REGISTRY,
        &DnodeSpec {
            obj_type: DMU_OT_SA_ATTR_REGISTRATION,
            datablksz: 2048,
            used: 2048,
            blkptrs: &[registry_bp],
            ..DnodeSpec::blank()
        },
    );
    put_dnode(
        &mut fs_dn,
        OBJ_LAYOUTS,
        &DnodeSpec {
            obj_type: DMU_OT_SA_ATTR_LAYOUTS,
            // Must match the shift `build_fatzap` was given: the leaf's chunk
            // and hash-table geometry is derived from the object's block size,
            // not stored in the block.
            datablksz: 1024,
            maxblkid: 1,
            used: 2048,
            blkptrs: &[lay_hdr_bp, lay_leaf_bp],
            ..DnodeSpec::blank()
        },
    );

    let root_attrs = Attrs::for_obj(OBJ_ROOT, 0o040_755, 0, OBJ_ROOT);
    let root_bonus = sa_buffer(1, &[], &sa_values(&IMG_LAYOUT_1, &root_attrs, &[]));
    put_dnode(
        &mut fs_dn,
        OBJ_ROOT,
        &DnodeSpec {
            obj_type: DMU_OT_DIRECTORY_CONTENTS,
            bonustype: DMU_OT_SA,
            datablksz: 1024,
            used: 1024,
            blkptrs: &[root_bp],
            bonus: &root_bonus,
            ..DnodeSpec::blank()
        },
    );

    // `/hello` uses layout 2: the same attributes as everything else, in a
    // different order. Its size therefore does *not* sit at byte 8.
    let hello_attrs = Attrs::for_obj(
        OBJ_HELLO,
        0o100_644,
        u64::try_from(HELLO_TEXT.len()).unwrap_or(0),
        OBJ_ROOT,
    );
    let hello_bonus = sa_buffer(2, &[], &sa_values(&IMG_LAYOUT_2, &hello_attrs, &[]));
    put_dnode(
        &mut fs_dn,
        OBJ_HELLO,
        &DnodeSpec {
            obj_type: DMU_OT_PLAIN_FILE_CONTENTS,
            bonustype: DMU_OT_SA,
            blkptrs: &[hello_bp],
            bonus: &hello_bonus,
            ..DnodeSpec::blank()
        },
    );

    let sub_attrs = Attrs::for_obj(OBJ_SUB, 0o040_755, 0, OBJ_ROOT);
    let sub_bonus = sa_buffer(1, &[], &sa_values(&IMG_LAYOUT_1, &sub_attrs, &[]));
    put_dnode(
        &mut fs_dn,
        OBJ_SUB,
        &DnodeSpec {
            obj_type: DMU_OT_DIRECTORY_CONTENTS,
            bonustype: DMU_OT_SA,
            blkptrs: &[sub_bp],
            bonus: &sub_bonus,
            ..DnodeSpec::blank()
        },
    );

    // `/link` keeps its target inline in the bonus (layout 5), the common case
    // for a short target.
    let link_attrs = Attrs::for_obj(
        OBJ_LINK,
        0o120_777,
        u64::try_from(b"hello".len()).unwrap_or(0),
        OBJ_ROOT,
    );
    let link_bonus = sa_buffer(
        5,
        &[u16::try_from(b"hello".len()).unwrap_or(0)],
        &sa_values(&IMG_LAYOUT_5, &link_attrs, b"hello"),
    );
    put_dnode(
        &mut fs_dn,
        OBJ_LINK,
        &DnodeSpec {
            obj_type: DMU_OT_PLAIN_FILE_CONTENTS,
            bonustype: DMU_OT_SA,
            bonus: &link_bonus,
            blkptrs: &[vec![0u8; BLKPTR_LEN]],
            ..DnodeSpec::blank()
        },
    );

    // `/dirlink` keeps its target in a spill block instead.
    let dirlink_attrs = Attrs::for_obj(
        OBJ_DIRLINK,
        0o120_777,
        u64::try_from(b"sub".len()).unwrap_or(0),
        OBJ_ROOT,
    );
    let dirlink_bonus = sa_buffer(3, &[], &sa_values(&IMG_LAYOUT_3, &dirlink_attrs, &[]));
    put_dnode(
        &mut fs_dn,
        OBJ_DIRLINK,
        &DnodeSpec {
            obj_type: DMU_OT_PLAIN_FILE_CONTENTS,
            bonustype: DMU_OT_SA,
            bonus: &dirlink_bonus,
            blkptrs: &[vec![0u8; BLKPTR_LEN]],
            spill: Some(&spill_bp),
            ..DnodeSpec::blank()
        },
    );

    let big_attrs = Attrs::for_obj(OBJ_BIG, 0o100_644, big_size, OBJ_ROOT);
    let big_bonus = sa_buffer(1, &[], &sa_values(&IMG_LAYOUT_1, &big_attrs, &[]));
    put_dnode(
        &mut fs_dn,
        OBJ_BIG,
        &DnodeSpec {
            obj_type: DMU_OT_PLAIN_FILE_CONTENTS,
            bonustype: DMU_OT_SA,
            indblkshift: 9,
            nlevels: 2,
            maxblkid: IMG_BIG_BLOCKS.saturating_sub(1),
            used: big_size,
            blkptrs: &big_ind,
            bonus: &big_bonus,
            ..DnodeSpec::blank()
        },
    );

    let nested_attrs = Attrs::for_obj(
        OBJ_NESTED,
        0o100_644,
        u64::try_from(NESTED_TEXT.len()).unwrap_or(0),
        OBJ_SUB,
    );
    let nested_bonus = sa_buffer(1, &[], &sa_values(&IMG_LAYOUT_1, &nested_attrs, &[]));
    put_dnode(
        &mut fs_dn,
        OBJ_NESTED,
        &DnodeSpec {
            obj_type: DMU_OT_PLAIN_FILE_CONTENTS,
            bonustype: DMU_OT_SA,
            blkptrs: &[nested_bp],
            bonus: &nested_bonus,
            ..DnodeSpec::blank()
        },
    );

    let fs_dn_bp = img.put(&fs_dn, DMU_OT_DNODE, 0, ZIO_CHECKSUM_SHA256);

    // --- The filesystem objset -------------------------------------------

    let mut fs_objset = vec![0u8; OBJSET_PHYS_SIZE];
    put_dnode(
        &mut fs_objset,
        0,
        &DnodeSpec {
            obj_type: DMU_OT_DNODE,
            datablksz: IMG_DN_BLK,
            used: u64::try_from(IMG_DN_BLK).unwrap_or(0),
            blkptrs: &[fs_dn_bp],
            ..DnodeSpec::blank()
        },
    );
    // `os_type` sits after the meta dnode and the 192-byte ZIL header. A pool
    // whose root dataset is a zvol has `DMU_OST_ZVOL` here, and mounting it as
    // a directory tree would list garbage.
    put_u64(&mut fs_objset, OBJSET_TYPE_OFFSET, DMU_OST_ZFS);
    let ds_bp = img.put(&fs_objset, DMU_OT_OBJSET, 0, ZIO_CHECKSUM_SHA256);

    // --- The MOS ---------------------------------------------------------

    let objdir_zap = build_micro(512, &[(b"root_dataset", 2)]);
    let objdir_bp = img.put(&objdir_zap, DMU_OT_OBJECT_DIRECTORY, 0, ZIO_CHECKSUM_SHA256);

    let mut mos_dn = vec![0u8; IMG_DN_BLK];
    put_dnode(
        &mut mos_dn,
        1,
        &DnodeSpec {
            obj_type: DMU_OT_OBJECT_DIRECTORY,
            blkptrs: &[objdir_bp],
            ..DnodeSpec::blank()
        },
    );

    // A DSL directory's bonus is a `dsl_dir_phys_t`; the only field this
    // driver reads is `dd_head_dataset_obj` at byte 8, the dataset holding the
    // live filesystem as opposed to a snapshot.
    let mut dsl_dir_bonus = vec![0u8; 256];
    put_u64(&mut dsl_dir_bonus, DD_HEAD_DATASET_OBJ, 3);
    put_dnode(
        &mut mos_dn,
        2,
        &DnodeSpec {
            obj_type: DMU_OT_DSL_DIR,
            bonustype: DMU_OT_DSL_DIR,
            bonus: &dsl_dir_bonus,
            blkptrs: &[vec![0u8; BLKPTR_LEN]],
            ..DnodeSpec::blank()
        },
    );

    // A DSL dataset's bonus is a `dsl_dataset_phys_t`, whose `ds_bp` at byte
    // 128 is the block pointer to the dataset's own objset. 320 bytes is the
    // whole bonus a one-slot dnode with one block pointer has room for, which
    // is exactly `sizeof(dsl_dataset_phys_t)` — not a coincidence.
    let mut dsl_ds_bonus = vec![0u8; 320];
    put_bytes(&mut dsl_ds_bonus, DS_BP_OFFSET, &ds_bp);
    put_dnode(
        &mut mos_dn,
        3,
        &DnodeSpec {
            obj_type: DMU_OT_DSL_DATASET,
            bonustype: DMU_OT_DSL_DATASET,
            bonus: &dsl_ds_bonus,
            blkptrs: &[vec![0u8; BLKPTR_LEN]],
            ..DnodeSpec::blank()
        },
    );

    let mos_dn_bp = img.put(&mos_dn, DMU_OT_DNODE, 0, ZIO_CHECKSUM_SHA256);

    let mut mos_objset = vec![0u8; OBJSET_PHYS_SIZE];
    put_dnode(
        &mut mos_objset,
        0,
        &DnodeSpec {
            obj_type: DMU_OT_DNODE,
            datablksz: IMG_DN_BLK,
            used: u64::try_from(IMG_DN_BLK).unwrap_or(0),
            blkptrs: &[mos_dn_bp],
            ..DnodeSpec::blank()
        },
    );
    put_u64(&mut mos_objset, OBJSET_TYPE_OFFSET, DMU_OST_META);
    let root_mos_bp = img.put(&mos_objset, DMU_OT_OBJSET, 0, ZIO_CHECKSUM_SHA256);

    // --- Labels ----------------------------------------------------------

    let config = build_config(&ConfigSpec::disk());
    let nvlist_at = usize::try_from(VDEV_LABEL_NVLIST_OFFSET).unwrap_or(0);
    let ub_array_at = usize::try_from(VDEV_LABEL_UBERBLOCK_OFFSET).unwrap_or(0);
    let label_stride = usize::try_from(VDEV_LABEL_SIZE).unwrap_or(0);

    // The decoy: an older, perfectly valid uberblock in an *earlier* slot,
    // whose rootbp addresses a block that is not an objset. Selecting it
    // would fail the mount rather than silently misread, but it would fail
    // for the wrong reason and hide the real bug.
    let decoy_bp = make_blkptr(img.next, &[0u8; 512], DMU_OT_OBJSET, 0, ZIO_CHECKSUM_SHA256);

    for label in 0..2usize {
        let base = label.saturating_mul(label_stride);
        put_bytes(&mut img.bytes, base.saturating_add(nvlist_at), &config);
        let array = base.saturating_add(ub_array_at);
        put_uberblock(
            &mut img.bytes,
            array.saturating_add(IMG_OLD_UB_SLOT.saturating_mul(1024)),
            IMG_OLD_TXG,
            &decoy_bp,
        );
        put_uberblock(
            &mut img.bytes,
            array.saturating_add(IMG_UB_SLOT.saturating_mul(1024)),
            IMG_TXG,
            &root_mos_bp,
        );
    }

    Pool {
        bytes: img.bytes,
        hello_at: PoolImage::abs(hello_dva),
        nvlist_at,
        ub_array_at,
    }
}

/// A layout's attribute numbers as the big-endian `u16` array a ZAP stores.
///
/// Big-endian is not a typo: ZAP array values are stored in network order
/// whatever the pool's own byte order is, which is the one place in ZFS where
/// the endianness does not follow the `B` bit.
fn layout_bytes(layout: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(layout.len().saturating_mul(2));
    for n in layout {
        out.extend_from_slice(&n.to_be_bytes());
    }
    out
}

/// Mount an image through [`MemorySource`], with no device involved.
fn mount(bytes: Vec<u8>) -> KernelResult<ZfsFs> {
    ZfsFs::open_source(Box::new(MemorySource::new(bytes)))
}

fn test_pool_mount(c: &mut Checks) -> KernelResult<()> {
    let pool = build_pool();
    let mut fs = match mount(pool.bytes) {
        Ok(fs) => fs,
        Err(e) => {
            c.check(false, "the synthetic pool mounts");
            serial_println!("[zfs]   mount failed: {:?}", e);
            return Ok(());
        }
    };
    c.check(true, "the synthetic pool mounts");

    // --- The pool, as the label described it -----------------------------
    let cfg = fs.pool_config();
    c.check_bytes(&cfg.name, IMG_POOL_NAME, "pool name from the label nvlist");
    c.check_u64(cfg.guid, IMG_GUID, "pool guid");
    c.check_u64(u64::from(cfg.ashift), IMG_ASHIFT, "ashift");
    c.check_bytes(&cfg.vdev_type, b"disk", "vdev type");
    c.check_u64(cfg.version, SPA_VERSION_FEATURES, "spa version");

    // The decisive one: slot 3 holds a valid uberblock at txg 5 and slot 11
    // holds the live one at txg 77. Reading 5 here would mean the driver took
    // the first valid slot rather than the newest.
    c.check_u64(
        fs.uberblock().txg,
        IMG_TXG,
        "the highest-txg uberblock wins, not the first",
    );

    // --- The root directory ----------------------------------------------
    let mut ents = fs.readdir(Path::new(b"/"))?;
    ents.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    c.check(ents.len() == 5, "root has five entries");
    let names: Vec<&[u8]> = ents.iter().map(|e| e.name.as_bytes()).collect();
    c.check(
        names == [b"big".as_slice(), b"dirlink", b"hello", b"link", b"sub"],
        "root entry names",
    );
    let types: Vec<EntryType> = ents.iter().map(|e| e.entry_type).collect();
    c.check(
        types
            == [
                EntryType::File,
                EntryType::Symlink,
                EntryType::File,
                EntryType::Symlink,
                EntryType::Directory,
            ],
        "root entry types come from the dirent's top bits",
    );

    // --- A file, read through the whole chain ----------------------------
    let hello = fs.read_file(Path::new(b"/hello"))?;
    c.check_bytes(&hello, HELLO_TEXT, "/hello contents");

    // This is the §245 check. `/hello`'s attributes are stored under a layout
    // that puts `ZPL_SIZE` at byte 56; the default layout puts it at byte 8,
    // where this object keeps the seconds half of its creation time. A driver
    // reading the hardcoded offset would report 1700000606 here — a number
    // that looks like nothing in particular, from a mount that succeeded.
    let st = fs.stat(Path::new(b"/hello"))?;
    c.check_u64(
        st.size,
        u64::try_from(HELLO_TEXT.len()).unwrap_or(0),
        "/hello size, resolved through the dataset's own layout",
    );
    c.check(st.entry_type == EntryType::File, "/hello is a file");

    let meta = fs.metadata(Path::new(b"/hello"))?;
    c.check_u64(meta.ino, OBJ_HELLO, "/hello inode is its object number");
    c.check(meta.permissions == 0o444, "/hello mode, write bits masked");
    c.check(meta.nlinks == 1, "/hello link count");
    c.check_u64(
        meta.modified_ns,
        IMG_TIMESTAMP
            .saturating_add(OBJ_HELLO)
            .saturating_add(100)
            .saturating_mul(1_000_000_000),
        "/hello mtime, from a 16-byte timestamp attribute",
    );

    // --- A subdirectory --------------------------------------------------
    let nested = fs.read_file(Path::new(b"/sub/nested"))?;
    c.check_bytes(&nested, NESTED_TEXT, "/sub/nested contents");

    // --- Symlinks, both storage forms ------------------------------------
    let inline = fs.readlink(Path::new(b"/link"))?;
    c.check_bytes(inline.as_bytes(), b"hello", "inline symlink target");
    let spilled = fs.readlink(Path::new(b"/dirlink"))?;
    c.check_bytes(
        spilled.as_bytes(),
        b"sub",
        "spilled symlink target, from a second SA buffer",
    );
    // Following the spilling link proves the spill block was not merely
    // present but usable as a path component.
    let through = fs.read_file(Path::new(b"/dirlink/nested"))?;
    c.check_bytes(&through, NESTED_TEXT, "resolving through a spilled symlink");

    // --- The sparse, two-level file --------------------------------------
    let big = fs.read_file(Path::new(b"/big"))?;
    c.check_u64(
        u64::try_from(big.len()).unwrap_or(0),
        u64::try_from(IMG_BIG_BLKSZ)
            .unwrap_or(0)
            .saturating_mul(IMG_BIG_BLOCKS),
        "/big length",
    );
    let mut big_ok = true;
    for blkid in 0..IMG_BIG_BLOCKS {
        let want = if blkid == IMG_BIG_HOLE {
            0u8
        } else {
            u8::try_from(0xA0u64.saturating_add(blkid)).unwrap_or(0)
        };
        let start = usize::try_from(blkid)
            .unwrap_or(0)
            .saturating_mul(IMG_BIG_BLKSZ);
        let seg = big
            .get(start..start.saturating_add(IMG_BIG_BLKSZ))
            .unwrap_or(&[]);
        if seg.len() != IMG_BIG_BLKSZ || seg.iter().any(|&b| b != want) {
            big_ok = false;
        }
    }
    c.check(
        big_ok,
        "/big reads back through two levels of indirection, hole included",
    );

    // A range that straddles the hole and both of its neighbours, starting and
    // ending mid-block: the stitch across block boundaries is separate code
    // from the whole-file read, and a partial first block is separate again
    // from a whole one.
    //
    // Bytes 1000..1600 of a 512-byte-blocked file are the last 24 bytes of
    // block 1, the whole of block 2 — the hole — and the first 64 bytes of
    // block 3. Each of the three is checked, because a reader that dropped the
    // hole entirely would still return the right *first* byte and a plausible
    // length.
    let straddle = fs.read_at(Path::new(b"/big"), 1000, 600)?;
    c.check(straddle.len() == 600, "read_at length");
    let tail_of_1 = straddle.get(..24).unwrap_or(&[]);
    let hole = straddle.get(24..536).unwrap_or(&[]);
    let head_of_3 = straddle.get(536..).unwrap_or(&[]);
    c.check(
        tail_of_1.len() == 24 && tail_of_1.iter().all(|&b| b == 0xA1),
        "read_at starts mid-block, in the block the offset lands in",
    );
    c.check(
        hole.len() == 512 && hole.iter().all(|&b| b == 0x00),
        "read_at spans the hole without shifting the bytes after it",
    );
    c.check(
        head_of_3.len() == 64 && head_of_3.iter().all(|&b| b == 0xA3),
        "read_at ends mid-block, past the hole",
    );

    // --- Errors that must stay distinguishable ---------------------------
    c.check_err(
        fs.read_file(Path::new(b"/sub")),
        KernelError::IsADirectory,
        "reading a directory as a file",
    );
    c.check_err(
        fs.readdir(Path::new(b"/hello")),
        KernelError::NotADirectory,
        "listing a file as a directory",
    );
    c.check_err(
        fs.stat(Path::new(b"/absent")),
        KernelError::NotFound,
        "a name that is not in the directory ZAP",
    );

    let info = fs.statvfs()?;
    c.check(info.read_only, "the mount is read-only");
    c.check_u64(info.block_size, 1 << IMG_ASHIFT, "statvfs block size");

    Ok(())
}

fn test_pool_damage(c: &mut Checks) -> KernelResult<()> {
    // A flipped bit in a file's data block. The block pointer's checksum still
    // describes the original bytes, so the read must fail rather than hand
    // back corrupted data — this is the check that proves `read_block` really
    // verifies rather than merely parsing a checksum field.
    let pool = build_pool();
    let mut bytes = pool.bytes.clone();
    if let Some(b) = bytes.get_mut(pool.hello_at) {
        *b ^= 0xFF;
    }
    match mount(bytes) {
        Ok(mut fs) => c.check_err(
            fs.read_file(Path::new(b"/hello")),
            KernelError::IoError,
            "a corrupt data block fails its checksum",
        ),
        Err(_) => c.check(false, "an image with one corrupt data block still mounts"),
    }

    // No uberblock at all: not a ZFS device, as opposed to a damaged one.
    let mut bytes = pool.bytes.clone();
    let ub_len = usize::try_from(VDEV_LABEL_UBERBLOCK_SIZE).unwrap_or(0);
    let stride = usize::try_from(VDEV_LABEL_SIZE).unwrap_or(0);
    for label in 0..2usize {
        let at = label
            .saturating_mul(stride)
            .saturating_add(pool.ub_array_at);
        if let Some(w) = bytes.get_mut(at..at.saturating_add(ub_len)) {
            w.fill(0);
        }
    }
    c.check_err(
        mount(bytes).map(|_| ()),
        KernelError::InvalidArgument,
        "a zeroed uberblock array is not a pool",
    );

    // Every uberblock byte-swapped: a pool created on a big-endian host. This
    // must be named as unsupported, because "not a ZFS pool" would send
    // whoever reads it looking for an entirely different problem.
    let mut bytes = pool.bytes.clone();
    for label in 0..2usize {
        let at = label
            .saturating_mul(stride)
            .saturating_add(pool.ub_array_at);
        if let Some(w) = bytes.get_mut(at..at.saturating_add(ub_len)) {
            w.fill(0);
        }
        put_u64(&mut bytes, at, UBERBLOCK_MAGIC_SWAPPED);
        // A swapped uberblock still needs a plausible txg, or it would be
        // rejected as an unwritten slot before the magic was ever considered.
        put_u64(&mut bytes, at.saturating_add(16), IMG_TXG);
    }
    c.check_err(
        mount(bytes).map(|_| ()),
        KernelError::NotSupported,
        "a big-endian pool is refused by name",
    );

    // The config cases below each overwrite the *same* bytes — the nvlist
    // region of both front labels — so each starts from a fresh clone of the
    // good image. Sharing one buffer would make every case's verdict depend on
    // the one before it, and a case that stopped rejecting would be masked by
    // its predecessor's damage rather than reported.
    let reconfigured = |spec: &ConfigSpec<'_>| {
        let mut bytes = pool.bytes.clone();
        let cfg = build_config(spec);
        for label in 0..2usize {
            let at = label.saturating_mul(stride).saturating_add(pool.nvlist_at);
            put_bytes(&mut bytes, at, &cfg);
        }
        bytes
    };

    // A raidz top-level vdev. One `SectorSource` is one device, and a raidz
    // leg holds interleaved data and parity columns, so the bytes at a DVA's
    // offset are a stripe fragment rather than the record. Reconstruction needs
    // every column and we have one.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            vdev_type: b"raidz",
            children: 3,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "a raidz vdev is refused rather than half-read",
    );

    // A `replacing` vdev — a mirror mid-resilver. This is the case that makes
    // the vdev-type test a whitelist and not a raidz blacklist: its shape is
    // identical to a mirror's, a parent with replica children, so anything that
    // reasoned "has children, children are replicas, therefore readable" would
    // accept it. One of those children is the disk being resilvered *onto* and
    // is not yet a complete copy of anything. We cannot tell from one label
    // which child the device in hand is, so accepting this would be a coin flip
    // on whether the reads are real.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            vdev_type: b"replacing",
            children: 2,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "a resilvering `replacing` vdev is refused, mirror-shaped though it is",
    );

    // A `spare` vdev — the same shape again, a pool running on a hot spare
    // while the original is being brought back. Same reasoning, and named
    // separately because a whitelist that admitted it by accident would look
    // just as correct as one that does not.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            vdev_type: b"spare",
            children: 2,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "an in-use `spare` vdev is refused, mirror-shaped though it is",
    );

    // A mirror claiming no children. Not a shape ZFS produces: the config
    // contradicts itself, which is a parse problem rather than an unsupported
    // layout, so it is `InvalidArgument` and not `NotSupported`. Without this
    // the ashift fallback would descend into a `children` list that is not
    // there.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            vdev_type: b"mirror",
            children: 0,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::InvalidArgument,
        "a mirror with no children is a contradiction, not a layout",
    );

    // The same contradiction from the other side: a leaf vdev that claims
    // children. ZFS gives `children` only to parents, so this label disagrees
    // with itself just as the childless mirror does, and it gets the same
    // `InvalidArgument` for the same reason.
    //
    // This case is worth pinning because its meaning changed. Before the vdev
    // type whitelist, "has children" *was* how a mirror or raidz was detected,
    // and `NotSupported` was the correct answer. Now type is decided first and
    // nothing but a malformed `disk`/`file` can reach that branch — so a future
    // reader who sees the check and assumes it still guards raidz is wrong, and
    // this test is what says so.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            vdev_type: b"disk",
            children: 2,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::InvalidArgument,
        "a leaf vdev that claims children is a contradiction too",
    );

    // A separate intent-log (SLOG) device. Everything else about this label is
    // a valid, mountable pool — same name, same GUID, same uberblocks, type
    // `disk`, no children — so `is_log` is the *only* thing standing between
    // "the pool's data disk" and "a device that holds no part of the object
    // tree". Without the check the mount gets as far as reading the meta object
    // set off a log device and reports `IoError`, which says "this pool is
    // corrupt" about a pool that is perfectly intact.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            is_log: 1,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "an intent-log device is named, not misread as the pool's data disk",
    );

    // The second disk of a striped pool. Its label says `type: disk` with no
    // children, exactly like a single-disk pool's — `id` is the only field that
    // tells them apart. Every DVA this driver can follow names vdev 0, so on a
    // device holding vdev 1 there is nothing readable at all, and saying so
    // here beats failing on the first block with a checksum error.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            id: 1,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "a stripe's second disk is refused before it can fail a block read",
    );

    // A hot spare. It is a genuine member of the pool, labelled and idle,
    // holding no part of any object tree — so there is nothing here to mount
    // however healthy the label looks.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            state: POOL_STATE_SPARE,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "a hot spare is named, not mistaken for the pool it stands by",
    );

    // An L2ARC cache device. Same shape as the spare: a real pool member whose
    // contents are an evictable copy of blocks that live somewhere else.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            state: POOL_STATE_L2CACHE,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "an L2ARC cache device is refused rather than read as a pool",
    );

    // A destroyed pool. This is the case the state check exists for: `zpool
    // destroy` rewrites the state field and little else, so the uberblocks are
    // still valid and the mount would *succeed* — silently reviving a pool the
    // administrator deleted. Refusing it is what `zpool import -D` being a
    // separate, explicit gesture means.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            state: POOL_STATE_DESTROYED,
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "a destroyed pool is refused, not silently revived",
    );

    // A cleanly exported pool, by contrast, mounts: an export leaves the
    // on-disk state consistent by definition, and read-only is all this driver
    // does. Asserting the *accepted* half matters as much as the refused ones —
    // a state check that rejected everything but `ACTIVE` would pass all five
    // cases above while breaking the commonest way a disk is legitimately
    // handed to another machine.
    c.check(
        mount(reconfigured(&ConfigSpec {
            state: POOL_STATE_EXPORTED,
            ..ConfigSpec::disk()
        }))
        .is_ok(),
        "a cleanly exported pool still mounts read-only",
    );

    // An SPA version this driver has never seen. Nothing above 5000 exists
    // today, which is the point: the constant documents a ceiling, and a
    // ceiling nothing compares against is a comment, not a check.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            version: SPA_VERSION_FEATURES.saturating_add(1),
            ..ConfigSpec::disk()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "an SPA version past the documented ceiling is refused by name",
    );

    // --- A mirror leg reads as the complete pool it is ---------------------
    //
    // The decisive test for mirror support, and the reason it is done against
    // the *unmodified* image bytes: only the config changes, from a lone disk
    // to one leg of a two-way mirror. If a mirror leg really is a byte-
    // identical copy of the top-level vdev's address space — the claim the
    // whole feature rests on — then every DVA resolves to the same physical
    // offset it did a moment ago, and this device must still read as the same
    // pool. Asserting only that it *mounts* would be far weaker: the mount
    // reads one uberblock, and it is the object tree underneath that would
    // expose a wrong offset.
    match mount(reconfigured(&ConfigSpec::mirror())) {
        Ok(mut fs) => {
            c.check(true, "a two-way mirror's leg mounts");
            c.check_bytes(
                &fs.pool_config().vdev_type,
                b"mirror",
                "the config reports the mirror it read",
            );
            c.check_u64(
                u64::from(fs.pool_config().ashift),
                IMG_ASHIFT,
                "a mirror's ashift comes off the top-level vdev",
            );
            c.check_u64(
                fs.uberblock().txg,
                IMG_TXG,
                "a mirror leg's uberblock array is the pool's",
            );
            match fs.read_file(Path::new(b"/hello")) {
                Ok(text) => c.check_bytes(
                    &text,
                    HELLO_TEXT,
                    "a file read through a mirror leg is byte-identical",
                ),
                Err(e) => {
                    c.check(false, "a file read through a mirror leg is byte-identical");
                    serial_println!("[zfs]   mirror read failed: {e:?}");
                }
            }
        }
        Err(e) => {
            c.check(false, "a two-way mirror's leg mounts");
            c.check(false, "the config reports the mirror it read");
            c.check(false, "a mirror's ashift comes off the top-level vdev");
            c.check(false, "a mirror leg's uberblock array is the pool's");
            c.check(false, "a file read through a mirror leg is byte-identical");
            serial_println!("[zfs]   mirror mount failed: {e:?}");
        }
    }

    // The same mirror with `ashift` recorded only on its legs. OpenZFS writes
    // it on the top-level vdev, so this label is not one we expect to meet —
    // but the fallback that handles it is unreachable otherwise, and an
    // unreachable fallback is an untested one. A wrong ashift is not silent:
    // it changes the uberblock stride, and an uberblock is checksummed against
    // the byte offset it was written at, so the failure would be a mount that
    // finds no uberblock rather than a pool read at the wrong granularity.
    match mount(reconfigured(&ConfigSpec {
        ashift_on_top: false,
        ..ConfigSpec::mirror()
    })) {
        Ok(fs) => {
            c.check(
                true,
                "a mirror recording ashift only on its legs still mounts",
            );
            c.check_u64(
                u64::from(fs.pool_config().ashift),
                IMG_ASHIFT,
                "ashift falls back to the first leg",
            );
        }
        Err(e) => {
            c.check(
                false,
                "a mirror recording ashift only on its legs still mounts",
            );
            c.check(false, "ashift falls back to the first leg");
            serial_println!("[zfs]   leaves-only ashift mount failed: {e:?}");
        }
    }

    // A mirrored separate intent log, and a mirror that is not the pool's first
    // top-level vdev. Both are now reachable in a way they were not when every
    // mirror was refused outright, and both must stay refused: `is_log` and
    // `id` live on the top-level vdev, which for these *is* the mirror.
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            is_log: 1,
            ..ConfigSpec::mirror()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "a mirrored intent log is still an intent log",
    );
    c.check_err(
        mount(reconfigured(&ConfigSpec {
            id: 1,
            ..ConfigSpec::mirror()
        }))
        .map(|_| ()),
        KernelError::NotSupported,
        "a mirror that is the stripe's second vdev is still unreachable",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Gang blocks
// ---------------------------------------------------------------------------
//
// A gang block is what ZFS writes when it wanted one contiguous run and the
// pool could not give it one: the DVA's gang bit is set, the offset leads to a
// small header of block pointers, and the pieces those name concatenate into
// the block. Everything here is built by hand from the on-disk layout rather
// than by inverting the reader, for the same reason the rest of this file is:
// a builder that shares the reader's idea of the format cannot disagree with
// it, and disagreement is the only thing a test can detect.
//
// Three properties are not guessable from the layout and are each pinned by a
// named check below, because getting any of them wrong yields a driver that
// works on the pools you tested and silently misreads the ones you did not:
//
//  * the header is checksummed against its own **address**, not against the
//    parent's `blk_cksum`;
//  * that address is always **DVA 0's**, even when copy 1 is the one being
//    read, which is what makes ditto copies of a header interchangeable;
//  * the txg in it is the *physical* birth, which is stored as zero whenever
//    it equals the logical one — so zero means "use the logical txg", not
//    "txg zero".

/// Reserve `len` bytes of the image's allocatable area.
fn gang_alloc(img: &mut PoolImage, len: usize) -> u64 {
    let off = img.next;
    img.next = img.next.saturating_add(u64::try_from(len).unwrap_or(0));
    off
}

/// Write a `hdr_len`-byte gang header at DVA offset `place`, holding
/// `children`, self-checksummed as though it lived at DVA offset `identity`
/// in transaction group `txg`.
///
/// `place` and `identity` are the same for every header a real pool writes.
/// They are separate parameters only so that the ditto-copy check below can
/// exist: it needs a header that is valid *at DVA 0's address* while actually
/// being read from DVA 1's, which is precisely the case a driver that
/// verifies against "the DVA I am reading" gets wrong.
fn write_gang_header(
    img: &mut PoolImage,
    place: u64,
    identity: u64,
    children: &[Vec<u8>],
    hdr_len: usize,
    txg: u64,
) {
    let mut gbh = vec![0u8; hdr_len];
    for (i, bp) in children.iter().enumerate() {
        put_bytes(&mut gbh, i.saturating_mul(BLKPTR_LEN), bp);
    }

    // The self-checksum: hash the header with its own address sitting where
    // the checksum will go, then overwrite that field with the hash. This is
    // `zio_checksum_compute`'s embedded path, written out longhand.
    let eck = hdr_len.saturating_sub(ZEC_LEN);
    put_u64(&mut gbh, eck, ZEC_MAGIC);
    let verifier: [u64; 4] = [0, identity, txg, 0];
    for (i, w) in verifier.iter().enumerate() {
        put_u64(
            &mut gbh,
            eck.saturating_add(8).saturating_add(i.saturating_mul(8)),
            *w,
        );
    }
    let cksum = sha256_cksum(&gbh);
    for (i, w) in cksum.iter().enumerate() {
        put_u64(
            &mut gbh,
            eck.saturating_add(8).saturating_add(i.saturating_mul(8)),
            *w,
        );
    }

    put_bytes(&mut img.bytes, PoolImage::abs(place), &gbh);
}

/// A block pointer with the gang bit set on each of `dvas` — `(offset,
/// allocated size)` pairs — standing in for `total` bytes of data.
///
/// `phys_birth` and `birth` are set separately so the check that the verifier
/// falls back from a zero physical birth to the logical one can be written.
///
/// The checksum field is filled with nonsense **on purpose**: ZFS does not
/// verify a gang leader's `blk_cksum` against the assembled bytes — each piece
/// is covered by its own pointer's checksum and the header by its own trailer
/// — so a driver that checked it here would reject every real gang block. The
/// nonsense is what makes the happy-path checks below prove that.
fn make_gang_blkptr(dvas: &[(u64, u64)], total: usize, phys_birth: u64, birth: u64) -> Vec<u8> {
    let mut bp = vec![0u8; BLKPTR_LEN];
    for (i, &(off, asize)) in dvas.iter().enumerate() {
        let at = i.saturating_mul(16);
        put_u64(&mut bp, at, asize >> SPA_MINBLOCKSHIFT);
        put_u64(
            &mut bp,
            at.saturating_add(8),
            (off >> SPA_MINBLOCKSHIFT) | (1u64 << 63),
        );
    }
    let sectors = u64::try_from(total).unwrap_or(0) >> SPA_MINBLOCKSHIFT;
    let sz = sectors.saturating_sub(1);
    let props = sz
        | (sz << 16)
        | (u64::from(ZIO_COMPRESS_OFF) << 32)
        | (u64::from(ZIO_CHECKSUM_FLETCHER_4) << 40)
        | (u64::from(DMU_OT_PLAIN_FILE_CONTENTS) << 48)
        | (1u64 << 63);
    put_u64(&mut bp, 48, props);
    put_u64(&mut bp, 72, phys_birth);
    put_u64(&mut bp, 80, birth);
    put_u64(&mut bp, 88, 1);
    put_u64(&mut bp, 96, 0xDEAD_BEEF_DEAD_BEEF);
    bp
}

/// `n` sectors of `fill`.
fn gang_piece(fill: u8, sectors: usize) -> Vec<u8> {
    vec![fill; sectors.saturating_mul(512)]
}

/// The gang transaction group. Different from [`IMG_TXG`] so that a driver
/// which reached for the wrong birth field would not accidentally agree.
const GANG_TXG: u64 = 1234;

/// A dynamic gang header's size on an `ashift=12` pool: one minimum
/// allocation, which is 4096 bytes, holding 31 pointers rather than three.
const GANG_BIG: usize = 4096;

#[expect(
    clippy::too_many_lines,
    reason = "one image shared by every case; splitting it would mean rebuilding \
              a 4 MiB image per check or threading a dozen offsets through helpers"
)]
fn test_gang(c: &mut Checks) -> KernelResult<()> {
    let mut img = PoolImage::new();
    let alg = ZIO_CHECKSUM_FLETCHER_4;
    let ot = DMU_OT_PLAIN_FILE_CONTENTS;

    // --- The pieces every case draws on ----------------------------------
    let (a, b, d) = (
        gang_piece(0x11, 1),
        gang_piece(0x22, 2),
        gang_piece(0x33, 1),
    );
    let a_bp = img.put(&a, ot, 0, alg);
    let b_bp = img.put(&b, ot, 0, alg);
    let d_bp = img.put(&d, ot, 0, alg);
    let abd: Vec<u8> = [a.as_slice(), b.as_slice(), d.as_slice()].concat();

    let read = |img: &PoolImage, bp: &[u8]| -> KernelResult<Vec<u8>> {
        let src = MemorySource::new(img.bytes.clone());
        let parsed = parse_blkptr(bp, 0)?;
        Reader::new(&src).read_block(&parsed)
    };

    // --- The ordinary case: three pieces, an old-style 512-byte header ----
    let h1 = gang_alloc(&mut img, 512);
    write_gang_header(
        &mut img,
        h1,
        h1,
        &[a_bp.clone(), b_bp.clone(), d_bp.clone()],
        512,
        GANG_TXG,
    );
    let bp1 = make_gang_blkptr(&[(h1, 512)], abd.len(), GANG_TXG, GANG_TXG);
    match read(&img, &bp1) {
        Ok(got) => c.check_bytes(&got, &abd, "a gang block reassembles in header order"),
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: gang read: {:?}", e);
        }
    }

    // A zero physical birth means "same as the logical birth", which is how
    // every ordinary write stores it. A driver that fed the raw field to the
    // verifier would hash against txg 0 and reject this.
    let h2 = gang_alloc(&mut img, 512);
    write_gang_header(&mut img, h2, h2, &[a_bp.clone(), d_bp.clone()], 512, GANG_TXG);
    let ad: Vec<u8> = [a.as_slice(), d.as_slice()].concat();
    let bp2 = make_gang_blkptr(&[(h2, 512)], ad.len(), 0, GANG_TXG);
    match read(&img, &bp2) {
        Ok(got) => c.check_bytes(
            &got,
            &ad,
            "a zero physical birth falls back to the logical one",
        ),
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: gang phys_birth: {:?}", e);
        }
    }

    // --- A dynamic gang header: 4096 bytes, 31 slots ---------------------
    let h3 = gang_alloc(&mut img, GANG_BIG);
    write_gang_header(
        &mut img,
        h3,
        h3,
        &[a_bp.clone(), b_bp.clone(), d_bp.clone()],
        GANG_BIG,
        GANG_TXG,
    );
    let bp3 = make_gang_blkptr(&[(h3, GANG_BIG as u64)], abd.len(), GANG_TXG, GANG_TXG);
    match read(&img, &bp3) {
        Ok(got) => c.check_bytes(&got, &abd, "a dynamic gang header reassembles too"),
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: dynamic gang header: {:?}", e);
        }
    }

    // The same allocation size holding an *old* 512-byte header, which is what
    // an `ashift=12` pool without the feature looks like: 4096 bytes reserved,
    // 512 of them meaningful. Reading it as 4096 finds no trailer magic at
    // 4056, so the reader must retry at 512 rather than call the block bad.
    let h4 = gang_alloc(&mut img, GANG_BIG);
    write_gang_header(&mut img, h4, h4, &[a_bp.clone(), d_bp.clone()], 512, GANG_TXG);
    let bp4 = make_gang_blkptr(&[(h4, GANG_BIG as u64)], ad.len(), GANG_TXG, GANG_TXG);
    match read(&img, &bp4) {
        Ok(got) => c.check_bytes(
            &got,
            &ad,
            "an old header in a large allocation falls back to 512",
        ),
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: gang size fallback: {:?}", e);
        }
    }

    // --- A gang piece that is itself a gang block ------------------------
    let h5 = gang_alloc(&mut img, 512);
    write_gang_header(&mut img, h5, h5, &[a_bp.clone(), b_bp.clone()], 512, GANG_TXG);
    let ab_len = a.len().saturating_add(b.len());
    let inner_bp = make_gang_blkptr(&[(h5, 512)], ab_len, GANG_TXG, GANG_TXG);
    let h6 = gang_alloc(&mut img, 512);
    write_gang_header(&mut img, h6, h6, &[inner_bp, d_bp.clone()], 512, GANG_TXG);
    let bp6 = make_gang_blkptr(&[(h6, 512)], abd.len(), GANG_TXG, GANG_TXG);
    match read(&img, &bp6) {
        Ok(got) => c.check_bytes(&got, &abd, "a gang tree nests"),
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: nested gang: {:?}", e);
        }
    }

    // --- Ditto copies: the verifier is DVA 0's address, always -----------
    //
    // Copy 0 is garbage, so the read falls through to copy 1 — which sits at a
    // different offset but is checksummed against copy 0's. That is what ZFS
    // does (`BP_IDENTITY`), and it is the only reason two copies of a header
    // can be byte-identical. A driver that verified against the offset it was
    // reading would fail here and report a healthy pool as corrupt.
    let h7_bad = gang_alloc(&mut img, 512);
    let h7_good = gang_alloc(&mut img, 512);
    put_bytes(
        &mut img.bytes,
        PoolImage::abs(h7_bad),
        &vec![0x5A; 512],
    );
    write_gang_header(
        &mut img,
        h7_good,
        h7_bad,
        &[a_bp.clone(), d_bp.clone()],
        512,
        GANG_TXG,
    );
    let bp7 = make_gang_blkptr(&[(h7_bad, 512), (h7_good, 512)], ad.len(), GANG_TXG, GANG_TXG);
    match read(&img, &bp7) {
        Ok(got) => c.check_bytes(
            &got,
            &ad,
            "a gang header's verifier is DVA 0's address on every copy",
        ),
        Err(e) => {
            c.failed = c.failed.saturating_add(1);
            serial_println!("[zfs] SELF-TEST FAILED: gang ditto copy: {:?}", e);
        }
    }

    // The same header read through a pointer that names the *wrong* offset:
    // the bytes are intact and the trailer magic is there, but the address it
    // was hashed against is not the one it is being read from. This is the
    // check that says the address is genuinely part of the checksum.
    let bp7_misaddressed =
        make_gang_blkptr(&[(h7_good, 512)], ad.len(), GANG_TXG, GANG_TXG);
    c.check_err(
        read(&img, &bp7_misaddressed).map(|_| ()),
        KernelError::IoError,
        "a gang header at the wrong address does not verify",
    );

    // ...and read at the right address but claiming the wrong txg.
    let bp7_mistxg = make_gang_blkptr(
        &[(h7_bad, 512), (h7_good, 512)],
        ad.len(),
        GANG_TXG.saturating_add(1),
        GANG_TXG.saturating_add(1),
    );
    c.check_err(
        read(&img, &bp7_mistxg).map(|_| ()),
        KernelError::IoError,
        "a gang header from the wrong txg does not verify",
    );

    // --- Trees that do not add up ----------------------------------------
    //
    // Both directions are `InvalidArgument`, not `IoError`: the bytes read and
    // verified fine, and it is the header and the pointer that disagree.
    let bp_short = make_gang_blkptr(&[(h2, 512)], ad.len().saturating_add(512), 0, GANG_TXG);
    c.check_err(
        read(&img, &bp_short).map(|_| ()),
        KernelError::InvalidArgument,
        "a gang tree short of the block it stands for is refused",
    );
    let bp_long = make_gang_blkptr(&[(h1, 512)], a.len(), GANG_TXG, GANG_TXG);
    c.check_err(
        read(&img, &bp_long).map(|_| ()),
        KernelError::InvalidArgument,
        "a gang tree longer than the block it stands for is refused",
    );

    // --- A cycle ---------------------------------------------------------
    //
    // A header whose only child is a gang pointer back to itself verifies at
    // every level — it really is the header it claims to be — and appends no
    // bytes, so nothing but the depth cap ever stops the recursion. Without
    // the cap this check overflows the kernel stack instead of failing.
    let h8 = gang_alloc(&mut img, 512);
    let self_bp = make_gang_blkptr(&[(h8, 512)], a.len(), GANG_TXG, GANG_TXG);
    write_gang_header(
        &mut img,
        h8,
        h8,
        core::slice::from_ref(&self_bp),
        512,
        GANG_TXG,
    );
    c.check_err(
        read(&img, &self_bp).map(|_| ()),
        KernelError::NotSupported,
        "a self-referential gang header is stopped by the depth cap",
    );

    // --- Damage ----------------------------------------------------------
    let mut damaged = PoolImage {
        bytes: img.bytes.clone(),
        next: img.next,
    };
    // One flipped bit inside the header's pointer array: magic intact, hash
    // wrong.
    if let Some(byte) = damaged.bytes.get_mut(PoolImage::abs(h1).saturating_add(9)) {
        *byte ^= 0x01;
    }
    c.check_err(
        read(&damaged, &bp1).map(|_| ()),
        KernelError::IoError,
        "a corrupt gang header fails its self-checksum",
    );
    // The magic itself destroyed: not a gang header at all, so the complaint
    // is about the shape rather than about the contents.
    put_u64(&mut damaged.bytes, PoolImage::abs(h1).saturating_add(472), 0);
    c.check_err(
        read(&damaged, &bp1).map(|_| ()),
        KernelError::InvalidArgument,
        "a gang header without its trailer magic is refused",
    );

    Ok(())
}

// ---------------------------------------------------------------------------

/// Run the ZFS self-tests.
///
/// # Errors
///
/// [`KernelError::InternalError`] if any check failed or any group errored.
/// The individual failures are on the serial log; this only reports that there
/// were some, because a boot-time test has no other channel.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[zfs] Running self-test...");

    let mut c = Checks::new();
    let mut aborted = 0u32;

    // Named so a hard error says *which* group stopped; without the name the
    // serial log shows an error with no indication of where it came from.
    let groups: [(&str, TestGroup); 12] = [
        ("primitives", test_primitives),
        ("zio", test_zio),
        ("nvlist", test_nvlist),
        ("label", test_label),
        ("dmu", test_dmu),
        ("zap micro", test_zap_micro),
        ("zap hash", test_zap_hash),
        ("zap leaf", test_zap_leaf),
        ("sa", test_sa),
        // Builds an image and reads through `Reader`, so it wants the format
        // groups above to have passed first — but it is not a mount, so it
        // stays ahead of the two pool groups.
        ("gang", test_gang),
        // Last, and in this order: the two pool groups exercise everything
        // above through the real mount path, so a failure here after the
        // others passed localises the fault to the chain between structures
        // rather than to any one of them.
        ("pool mount", test_pool_mount),
        ("pool damage", test_pool_damage),
    ];

    for (name, run) in groups {
        if let Err(e) = run(&mut c) {
            aborted = aborted.saturating_add(1);
            serial_println!("[zfs] SELF-TEST ERROR in {}: {:?}", name, e);
        }
    }

    if c.failed == 0 && aborted == 0 {
        serial_println!("[zfs] Self-test passed ({} checks).", c.passed);
        return Ok(());
    }

    serial_println!(
        "[zfs] Self-test FAILED: {} passed, {} failed, {} group(s) errored.",
        c.passed,
        c.failed,
        aborted
    );
    Err(KernelError::InternalError)
}
