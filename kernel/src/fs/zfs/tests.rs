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

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::dmu::parse_dnode;
use super::label::{front_label_offset, parse_uberblock};
use super::nvlist::{DATA_TYPE_UINT64, NV_ENCODE_NATIVE, NV_ENCODE_XDR, NV_LITTLE_ENDIAN, NvList};
use super::raw::{
    DMU_OT_PLAIN_FILE_CONTENTS, DNODE_LEN, Dva, SPA_MINBLOCKSHIFT, VDEV_LABEL_START_SIZE,
    ZIO_COMPRESS_OFF, ZIO_COMPRESS_ZSTD, bf64_get, bf64_get_sb,
};
use super::sa::{SA_MAGIC, SaAttr, SaMap, SaRegistry, decimal_key, parse_header};
use super::zap::{
    CHAIN_END, MZAP_ENT_LEN, ZAP_CHUNK_ARRAY, ZAP_CHUNK_ENTRY, ZAP_CHUNK_FREE, ZAP_HASHBITS,
    ZAP_LEAF_ARRAY_BYTES, ZAP_LEAF_MAGIC, ZBT_LEAF, ZBT_MICRO, ZFS_CRC64_TABLE, leaf_chunk_off,
    leaf_entries, leaf_lookup, leaf_lookup_array, leaf_numchunks, log2_exact, micro_entries,
    micro_lookup, zap_hash,
};
use super::zio::{compression_supported, decompress, fletcher_2, fletcher_4, zle_decompress};

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

/// Build one XDR nvpair holding a `u64`.
///
/// Written from the XDR encoding rather than by calling the parser: name as a
/// big-endian length plus bytes padded to four, then the type, the element
/// count, and the value — all big-endian, as XDR always is regardless of the
/// host.
fn nv_pair_u64(name: &[u8], value: u64) -> Vec<u8> {
    let mut body = Vec::new();
    let Ok(nlen) = u32::try_from(name.len()) else {
        return Vec::new();
    };
    body.extend_from_slice(&nlen.to_be_bytes());
    body.extend_from_slice(name);
    while body.len() % 4 != 0 {
        body.push(0);
    }
    body.extend_from_slice(&DATA_TYPE_UINT64.to_be_bytes());
    body.extend_from_slice(&1u32.to_be_bytes());
    body.extend_from_slice(&value.to_be_bytes());

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
    let groups: [(&str, TestGroup); 9] = [
        ("primitives", test_primitives),
        ("zio", test_zio),
        ("nvlist", test_nvlist),
        ("label", test_label),
        ("dmu", test_dmu),
        ("zap micro", test_zap_micro),
        ("zap hash", test_zap_hash),
        ("zap leaf", test_zap_leaf),
        ("sa", test_sa),
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
