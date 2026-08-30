//! NTFS self-tests, driven by a synthetic volume built in RAM.
//!
//! ## Why this file builds a whole filesystem
//!
//! The hard parts of an NTFS driver are all on-disk: update-sequence fixups,
//! sign-extended runlist deltas, the resident/non-resident split, the B+ tree
//! that a directory really is. None of them can be exercised by unit-testing
//! a parser against a hand-written byte array — a byte array proves the
//! parser accepts what the *test author* believed the format to be, which is
//! exactly the belief under test.
//!
//! So [`build_image`] writes a complete, structurally valid NTFS volume:
//! a boot sector, an `$MFT` located through its own runlist, a root directory
//! whose index has overflowed into an `INDX` block, a subdirectory whose
//! index has not, a resident file, a fragmented non-resident file, a sparse
//! file, and a file whose `$DATA` lives in an extension record reached
//! through an `$ATTRIBUTE_LIST`. The driver then mounts it through
//! [`MemorySource`] and reads it back with no device involved, on every boot.
//!
//! The builder is deliberately written *independently* of the parser — it
//! lays out bytes from the format documentation rather than calling any
//! serialisation helper the parser shares. A builder that reused the parser's
//! notion of the layout would agree with it by construction, including where
//! both are wrong.
//!
//! ## Volume layout
//!
//! ```text
//! LCN  0      boot sector (in the first 512 bytes of the cluster)
//! LCN  4..9   $MFT (6 clusters = 24 records of 1024 bytes)
//! LCN 10      big.bin, first fragment
//! LCN 12      root directory's INDX block (VCN 0)
//! LCN 20      big.bin, second fragment  (a deliberate forward jump)
//! LCN 21      sparse.bin's non-sparse tail
//! LCN 22      split.bin's data (owned by extension record 21)
//! ```
//!
//! ```text
//! MFT  0   $MFT          5   . (root)      16  hello.txt
//!      3   $Volume       18  sub/          17  big.bin
//!                        19  sub/inner.txt 20  split.bin (base)
//!                                          21  split.bin (extension)
//!                                          22  sparse.bin
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::Path;
use crate::fs::vfs::{EntryType, FileSystem};
use crate::serial_println;

use super::NtfsFs;
use super::attr::{AttributeType, DataRun, NameSpace, decode_runlist};
use super::boot::BootSector;
use super::raw::{filetime_to_unix_ns, utf16le_at};
use super::record::{FILE_MAGIC, apply_fixups, mft_ref_record, mft_ref_sequence};
use crate::fs::blocksrc::MemorySource;

// ---------------------------------------------------------------------------
// Volume geometry
// ---------------------------------------------------------------------------

const BYTES_PER_SECTOR: usize = 512;
const SECTORS_PER_CLUSTER: usize = 8;
const CLUSTER: usize = BYTES_PER_SECTOR * SECTORS_PER_CLUSTER; // 4096
const MFT_RECORD: usize = 1024;
const INDEX_BLOCK: usize = 4096;

const TOTAL_CLUSTERS: usize = 24;
const MFT_LCN: u64 = 4;
const MFT_CLUSTERS: u64 = 6;
const MFT_RECORDS: u64 = (MFT_CLUSTERS * CLUSTER as u64) / MFT_RECORD as u64; // 24

const LCN_BIG_1: u64 = 10;
const LCN_ROOT_INDX: u64 = 12;
const LCN_BIG_2: u64 = 20;
const LCN_SPARSE: u64 = 21;
const LCN_SPLIT: u64 = 22;

// MFT record numbers.
const REC_MFT: u64 = 0;
const REC_VOLUME: u64 = 3;
const REC_ROOT: u64 = 5;
const REC_HELLO: u64 = 16;
const REC_BIG: u64 = 17;
const REC_SUB: u64 = 18;
const REC_INNER: u64 = 19;
const REC_SPLIT: u64 = 20;
const REC_SPLIT_EXT: u64 = 21;
const REC_SPARSE: u64 = 22;

/// Every record in the synthetic volume uses sequence 1, so a reference is
/// `(1 << 48) | record`. Using a non-zero sequence is deliberate: a driver
/// that forgets to mask the sequence off reads record `281474976710661`
/// instead of `5`, and a test volume with sequence 0 would never catch it.
const SEQ: u64 = 1;

/// 2024-01-01T00:00:00Z as a Windows `FILETIME`.
const TEST_FILETIME: u64 = 133_485_408_000_000_000;
/// The same instant in nanoseconds since the Unix epoch.
const TEST_UNIX_NS: u64 = 1_704_067_200_000_000_000;

const HELLO_CONTENT: &[u8] = b"Hello from NTFS!\n";
const INNER_CONTENT: &[u8] = b"nested\n";
/// Spans two clusters, so a read must cross a fragment boundary.
const BIG_SIZE: usize = 5000;
/// One sparse cluster followed by real data.
const SPARSE_SIZE: usize = 6000;
/// Small, but reached only through an `$ATTRIBUTE_LIST`.
const SPLIT_SIZE: usize = 300;

/// Build an MFT reference for a record.
const fn mref(record: u64) -> u64 {
    (SEQ << 48) | record
}

/// The expected contents of `big.bin`.
fn big_content() -> Vec<u8> {
    (0..BIG_SIZE)
        .map(|i| u8::try_from(i % 251).unwrap_or(0))
        .collect()
}

/// The expected contents of `sparse.bin`: a hole, then data.
fn sparse_content() -> Vec<u8> {
    let mut v = vec![0u8; SPARSE_SIZE];
    for i in CLUSTER..SPARSE_SIZE {
        if let Some(b) = v.get_mut(i) {
            *b = u8::try_from(i % 97).unwrap_or(0);
        }
    }
    v
}

/// The expected contents of `split.bin`.
fn split_content() -> Vec<u8> {
    (0..SPLIT_SIZE)
        .map(|i| u8::try_from(i % 13).unwrap_or(0))
        .collect()
}

// ---------------------------------------------------------------------------
// Byte-level builders
// ---------------------------------------------------------------------------

/// Copy `bytes` into `image` at `offset`, growing nothing (the image is
/// pre-sized).
fn put(image: &mut [u8], offset: usize, bytes: &[u8]) {
    if let Some(dst) = image.get_mut(offset..offset.saturating_add(bytes.len())) {
        dst.copy_from_slice(bytes);
    }
}

/// Write one byte at `offset`, ignoring an out-of-range offset.
///
/// The single-byte counterpart of [`put`]. Indexing (`buf[off] = v`) would be
/// shorter, but it panics on a builder bug — and a builder that panics takes
/// the kernel down at boot instead of reporting a failed self-test.
fn put_u8(image: &mut [u8], offset: usize, value: u8) {
    if let Some(dst) = image.get_mut(offset) {
        *dst = value;
    }
}

/// Round `v` up to a multiple of 8 (every NTFS record is 8-byte aligned).
const fn align8(v: usize) -> usize {
    v.saturating_add(7) & !7
}

/// Encode a `&str` as UTF-16LE bytes.
fn utf16(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len().saturating_mul(2));
    for unit in s.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// Smallest number of bytes that can hold `v` unsigned.
fn min_unsigned_bytes(v: u64) -> usize {
    let mut n = 1usize;
    let mut limit = 1u64 << 8;
    while n < 8 && v >= limit {
        n = n.saturating_add(1);
        limit = limit.checked_shl(8).unwrap_or(u64::MAX);
    }
    n
}

/// Smallest number of bytes that can hold `v` two's-complement signed.
fn min_signed_bytes(v: i64) -> usize {
    for n in 1..8usize {
        let bits = (n as u32).saturating_mul(8);
        let max = 1i64.checked_shl(bits.saturating_sub(1)).unwrap_or(i64::MAX);
        let min = max.checked_neg().unwrap_or(i64::MIN);
        if v >= min && v < max {
            return n;
        }
    }
    8
}

/// Encode a runlist from (length, delta) pairs; `None` delta means sparse.
///
/// Widths are the *minimum* that fit, exactly as Windows writes them, so the
/// decoder is tested against variable-width fields rather than a convenient
/// fixed width.
fn encode_runs(runs: &[(u64, Option<i64>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (length, delta) in runs {
        let len_size = min_unsigned_bytes(*length);
        let off_size = delta.map_or(0, min_signed_bytes);

        let header = u8::try_from(len_size | (off_size << 4)).unwrap_or(0);
        out.push(header);

        let len_bytes = length.to_le_bytes();
        out.extend_from_slice(len_bytes.get(0..len_size).unwrap_or(&[]));

        if let Some(d) = delta {
            let off_bytes = d.to_le_bytes();
            out.extend_from_slice(off_bytes.get(0..off_size).unwrap_or(&[]));
        }
    }
    out.push(0); // terminator
    out
}

/// Build a resident attribute record.
fn resident_attr(ty: u32, name: &str, flags: u16, id: u16, value: &[u8], indexed: bool) -> Vec<u8> {
    let name_units = name.encode_utf16().count();
    let name_off = 0x18usize;
    let value_off = align8(name_off.saturating_add(name_units.saturating_mul(2)));
    let length = align8(value_off.saturating_add(value.len()));

    let mut a = vec![0u8; length];
    put(&mut a, 0x00, &ty.to_le_bytes());
    put(
        &mut a,
        0x04,
        &u32::try_from(length).unwrap_or(0).to_le_bytes(),
    );
    put_u8(&mut a, 0x08, 0); // resident
    put_u8(&mut a, 0x09, u8::try_from(name_units).unwrap_or(0));
    put(
        &mut a,
        0x0A,
        &u16::try_from(name_off).unwrap_or(0).to_le_bytes(),
    );
    put(&mut a, 0x0C, &flags.to_le_bytes());
    put(&mut a, 0x0E, &id.to_le_bytes());
    put(
        &mut a,
        0x10,
        &u32::try_from(value.len()).unwrap_or(0).to_le_bytes(),
    );
    put(
        &mut a,
        0x14,
        &u16::try_from(value_off).unwrap_or(0).to_le_bytes(),
    );
    put_u8(&mut a, 0x16, u8::from(indexed));
    put(&mut a, name_off, &utf16(name));
    put(&mut a, value_off, value);
    a
}

/// Build a non-resident attribute record.
#[allow(clippy::too_many_arguments)] // Mirrors the on-disk field list.
fn non_resident_attr(
    ty: u32,
    name: &str,
    flags: u16,
    id: u16,
    start_vcn: u64,
    last_vcn: u64,
    allocated: u64,
    data_size: u64,
    initialized: u64,
    runs: &[u8],
) -> Vec<u8> {
    let name_units = name.encode_utf16().count();
    let name_off = 0x40usize;
    let runs_off = align8(name_off.saturating_add(name_units.saturating_mul(2)));
    let length = align8(runs_off.saturating_add(runs.len()));

    let mut a = vec![0u8; length];
    put(&mut a, 0x00, &ty.to_le_bytes());
    put(
        &mut a,
        0x04,
        &u32::try_from(length).unwrap_or(0).to_le_bytes(),
    );
    put_u8(&mut a, 0x08, 1); // non-resident
    put_u8(&mut a, 0x09, u8::try_from(name_units).unwrap_or(0));
    put(
        &mut a,
        0x0A,
        &u16::try_from(name_off).unwrap_or(0).to_le_bytes(),
    );
    put(&mut a, 0x0C, &flags.to_le_bytes());
    put(&mut a, 0x0E, &id.to_le_bytes());
    put(&mut a, 0x10, &start_vcn.to_le_bytes());
    put(&mut a, 0x18, &last_vcn.to_le_bytes());
    put(
        &mut a,
        0x20,
        &u16::try_from(runs_off).unwrap_or(0).to_le_bytes(),
    );
    put(&mut a, 0x22, &0u16.to_le_bytes()); // compression unit: none
    put(&mut a, 0x28, &allocated.to_le_bytes());
    put(&mut a, 0x30, &data_size.to_le_bytes());
    put(&mut a, 0x38, &initialized.to_le_bytes());
    put(&mut a, name_off, &utf16(name));
    put(&mut a, runs_off, runs);
    a
}

/// Build a `$STANDARD_INFORMATION` value.
fn standard_information(dos_flags: u32) -> Vec<u8> {
    let mut v = vec![0u8; 48];
    for off in [0x00usize, 0x08, 0x10, 0x18] {
        put(&mut v, off, &TEST_FILETIME.to_le_bytes());
    }
    put(&mut v, 0x20, &dos_flags.to_le_bytes());
    v
}

/// Build a `$FILE_NAME` value.
fn file_name_value(
    parent: u64,
    name: &str,
    namespace: u8,
    dos_flags: u32,
    allocated: u64,
    data_size: u64,
) -> Vec<u8> {
    let name_units = name.encode_utf16().count();
    let mut v = vec![0u8; 0x42usize.saturating_add(name_units.saturating_mul(2))];
    put(&mut v, 0x00, &parent.to_le_bytes());
    for off in [0x08usize, 0x10, 0x18, 0x20] {
        put(&mut v, off, &TEST_FILETIME.to_le_bytes());
    }
    put(&mut v, 0x28, &allocated.to_le_bytes());
    put(&mut v, 0x30, &data_size.to_le_bytes());
    put(&mut v, 0x38, &dos_flags.to_le_bytes());
    put_u8(&mut v, 0x40, u8::try_from(name_units).unwrap_or(0));
    put_u8(&mut v, 0x41, namespace);
    put(&mut v, 0x42, &utf16(name));
    v
}

/// Build one index entry.
fn index_entry(reference: u64, key: &[u8], child_vcn: Option<u64>, is_last: bool) -> Vec<u8> {
    let key_len = key.len();
    let mut length = align8(0x10usize.saturating_add(key_len));
    if child_vcn.is_some() {
        length = length.saturating_add(8);
    }

    let mut e = vec![0u8; length];
    put(&mut e, 0x00, &reference.to_le_bytes());
    put(
        &mut e,
        0x08,
        &u16::try_from(length).unwrap_or(0).to_le_bytes(),
    );
    put(
        &mut e,
        0x0A,
        &u16::try_from(key_len).unwrap_or(0).to_le_bytes(),
    );
    let mut flags = 0u16;
    if child_vcn.is_some() {
        flags |= super::index::ENTRY_HAS_SUBNODE;
    }
    if is_last {
        flags |= super::index::ENTRY_IS_LAST;
    }
    put(&mut e, 0x0C, &flags.to_le_bytes());
    put(&mut e, 0x10, key);
    if let Some(vcn) = child_vcn {
        put(&mut e, length.saturating_sub(8), &vcn.to_le_bytes());
    }
    e
}

/// Build an `$INDEX_ROOT` value from a list of already-built entries.
fn index_root_value(
    block_size: u32,
    clusters_per_block: u8,
    entries: &[Vec<u8>],
    large: bool,
) -> Vec<u8> {
    let body: Vec<u8> = entries.iter().flat_map(|e| e.iter().copied()).collect();
    let mut v = vec![0u8; 0x20usize.saturating_add(body.len())];

    put(&mut v, 0x00, &AttributeType::FILE_NAME.0.to_le_bytes());
    put(&mut v, 0x04, &1u32.to_le_bytes()); // COLLATION_FILE_NAME
    put(&mut v, 0x08, &block_size.to_le_bytes());
    put_u8(&mut v, 0x0C, clusters_per_block);

    // Node header at 0x10; its offsets are relative to itself.
    let entries_offset = 0x10u32;
    let entries_size = 0x10u32.saturating_add(u32::try_from(body.len()).unwrap_or(0));
    put(&mut v, 0x10, &entries_offset.to_le_bytes());
    put(&mut v, 0x14, &entries_size.to_le_bytes());
    put(&mut v, 0x18, &entries_size.to_le_bytes());
    put(
        &mut v,
        0x1C,
        &(if large {
            super::index::NODE_HAS_CHILDREN
        } else {
            0
        })
        .to_le_bytes(),
    );
    put(&mut v, 0x20, &body);
    v
}

/// Build one `INDX` block, fixups included.
fn indx_block(vcn: u64, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0u8; INDEX_BLOCK];
    let usa_count = 1usize.saturating_add(INDEX_BLOCK / BYTES_PER_SECTOR);
    let usa_offset = 0x28usize;
    let first_entry = align8(usa_offset.saturating_add(usa_count.saturating_mul(2)));

    put(&mut b, 0x00, super::index::INDX_MAGIC);
    put(
        &mut b,
        0x04,
        &u16::try_from(usa_offset).unwrap_or(0).to_le_bytes(),
    );
    put(
        &mut b,
        0x06,
        &u16::try_from(usa_count).unwrap_or(0).to_le_bytes(),
    );
    put(&mut b, 0x10, &vcn.to_le_bytes());

    // Node header at 0x18, offsets relative to it.
    let body: Vec<u8> = entries.iter().flat_map(|e| e.iter().copied()).collect();
    let entries_offset = first_entry.saturating_sub(INDX_NODE);
    let entries_size = entries_offset.saturating_add(body.len());
    put(
        &mut b,
        INDX_NODE,
        &u32::try_from(entries_offset).unwrap_or(0).to_le_bytes(),
    );
    put(
        &mut b,
        INDX_NODE + 4,
        &u32::try_from(entries_size).unwrap_or(0).to_le_bytes(),
    );
    put(
        &mut b,
        INDX_NODE + 8,
        &u32::try_from(INDEX_BLOCK.saturating_sub(INDX_NODE))
            .unwrap_or(0)
            .to_le_bytes(),
    );
    put(&mut b, INDX_NODE + 12, &0u32.to_le_bytes()); // leaf
    put(&mut b, first_entry, &body);

    write_fixups(&mut b, usa_offset, usa_count, 0x0202);
    b
}

/// Offset of the node header inside an `INDX` block.
const INDX_NODE: usize = 0x18;

/// Apply update-sequence fixups to a freshly built structure.
///
/// This is the *write* side of what `record::apply_fixups` undoes: stash each
/// sector's last two bytes in the USA and stamp the USN in their place.
fn write_fixups(buf: &mut [u8], usa_offset: usize, usa_count: usize, usn: u16) {
    put(buf, usa_offset, &usn.to_le_bytes());
    let sectors = usa_count.saturating_sub(1);
    for i in 0..sectors {
        let tail = i
            .saturating_add(1)
            .saturating_mul(BYTES_PER_SECTOR)
            .saturating_sub(2);
        let Some(original) = buf.get(tail..tail.saturating_add(2)) else {
            continue;
        };
        let original = [
            *original.first().unwrap_or(&0),
            *original.get(1).unwrap_or(&0),
        ];
        let slot = usa_offset.saturating_add(i.saturating_add(1).saturating_mul(2));
        put(buf, slot, &original);
        put(buf, tail, &usn.to_le_bytes());
    }
}

/// Build one MFT record from a list of already-built attributes.
fn mft_record(number: u64, flags: u16, base_reference: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
    let mut r = vec![0u8; MFT_RECORD];
    let usa_count = 1usize.saturating_add(MFT_RECORD / BYTES_PER_SECTOR);
    let usa_offset = 0x30usize;
    let attrs_offset = align8(usa_offset.saturating_add(usa_count.saturating_mul(2)));

    put(&mut r, 0x00, FILE_MAGIC);
    put(
        &mut r,
        0x04,
        &u16::try_from(usa_offset).unwrap_or(0).to_le_bytes(),
    );
    put(
        &mut r,
        0x06,
        &u16::try_from(usa_count).unwrap_or(0).to_le_bytes(),
    );
    put(&mut r, 0x10, &u16::try_from(SEQ).unwrap_or(1).to_le_bytes());
    put(&mut r, 0x12, &1u16.to_le_bytes()); // hard links
    put(
        &mut r,
        0x14,
        &u16::try_from(attrs_offset).unwrap_or(0).to_le_bytes(),
    );
    put(&mut r, 0x16, &flags.to_le_bytes());
    put(
        &mut r,
        0x1C,
        &u32::try_from(MFT_RECORD).unwrap_or(0).to_le_bytes(),
    );
    put(&mut r, 0x20, &base_reference.to_le_bytes());
    put(&mut r, 0x28, &8u16.to_le_bytes()); // next attribute id
    put(
        &mut r,
        0x2C,
        &u32::try_from(number).unwrap_or(0).to_le_bytes(),
    );

    let mut offset = attrs_offset;
    for attr in attrs {
        put(&mut r, offset, attr);
        offset = offset.saturating_add(attr.len());
    }
    put(&mut r, offset, &AttributeType::END.0.to_le_bytes());
    let bytes_used = align8(offset.saturating_add(4));
    put(
        &mut r,
        0x18,
        &u32::try_from(bytes_used).unwrap_or(0).to_le_bytes(),
    );

    write_fixups(&mut r, usa_offset, usa_count, 0x0101);
    r
}

/// Build an `$ATTRIBUTE_LIST` entry.
fn attribute_list_entry(ty: u32, start_vcn: u64, reference: u64, id: u16) -> Vec<u8> {
    let length = 0x20usize;
    let mut e = vec![0u8; length];
    put(&mut e, 0x00, &ty.to_le_bytes());
    put(
        &mut e,
        0x04,
        &u16::try_from(length).unwrap_or(0).to_le_bytes(),
    );
    put_u8(&mut e, 0x06, 0); // name length
    put_u8(&mut e, 0x07, 0x1A); // name offset
    put(&mut e, 0x08, &start_vcn.to_le_bytes());
    put(&mut e, 0x10, &reference.to_le_bytes());
    put(&mut e, 0x18, &id.to_le_bytes());
    e
}

// ---------------------------------------------------------------------------
// The volume
// ---------------------------------------------------------------------------

/// DOS attribute value used for the directories in the image.
const DOS_DIR: u32 = super::attr::DOS_DIRECTORY;

/// Build the complete synthetic NTFS volume.
fn build_image() -> Vec<u8> {
    let mut image = vec![0u8; TOTAL_CLUSTERS.saturating_mul(CLUSTER)];

    // ---- Boot sector ----
    {
        let mut b = vec![0u8; BYTES_PER_SECTOR];
        put(&mut b, 0x00, &[0xEB, 0x52, 0x90]); // jump
        put(&mut b, 0x03, super::boot::NTFS_OEM_ID);
        put(
            &mut b,
            0x0B,
            &u16::try_from(BYTES_PER_SECTOR).unwrap_or(512).to_le_bytes(),
        );
        put_u8(&mut b, 0x0D, u8::try_from(SECTORS_PER_CLUSTER).unwrap_or(8));
        put_u8(&mut b, 0x15, 0xF8); // media descriptor
        let total_sectors = (TOTAL_CLUSTERS.saturating_mul(SECTORS_PER_CLUSTER)) as u64;
        put(&mut b, 0x28, &total_sectors.to_le_bytes());
        put(&mut b, 0x30, &MFT_LCN.to_le_bytes());
        put(&mut b, 0x38, &MFT_LCN.to_le_bytes()); // mirror: same, unused here
        // -10 => 2^10 = 1024-byte MFT records. The negative encoding is the
        // common real-world case and the one a naive parser gets wrong.
        put_u8(&mut b, 0x40, 0xF6); // -10 as i8
        put_u8(&mut b, 0x44, 1); // one cluster per index buffer => 4096
        put(&mut b, 0x48, &0x1234_5678_9ABC_DEF0u64.to_le_bytes());
        put(&mut b, 0x1FE, &[0x55, 0xAA]);
        put(&mut image, 0, &b);
    }

    // ---- File data ----
    let big = big_content();
    put(
        &mut image,
        (LCN_BIG_1 as usize).saturating_mul(CLUSTER),
        big.get(0..CLUSTER).unwrap_or(&[]),
    );
    put(
        &mut image,
        (LCN_BIG_2 as usize).saturating_mul(CLUSTER),
        big.get(CLUSTER..).unwrap_or(&[]),
    );

    let sparse = sparse_content();
    put(
        &mut image,
        (LCN_SPARSE as usize).saturating_mul(CLUSTER),
        sparse.get(CLUSTER..).unwrap_or(&[]),
    );

    put(
        &mut image,
        (LCN_SPLIT as usize).saturating_mul(CLUSTER),
        &split_content(),
    );

    // ---- MFT records ----
    let mut mft = vec![0u8; (MFT_CLUSTERS as usize).saturating_mul(CLUSTER)];

    // Record 0: $MFT itself. Its $DATA runlist is what the driver bootstraps
    // through, so a wrong runlist decoder fails at mount rather than later.
    let mft_bytes = MFT_RECORDS.saturating_mul(MFT_RECORD as u64);
    let rec_mft = mft_record(
        REC_MFT,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(super::attr::DOS_HIDDEN | super::attr::DOS_SYSTEM),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(
                    mref(REC_ROOT),
                    "$MFT",
                    1,
                    super::attr::DOS_HIDDEN,
                    mft_bytes,
                    mft_bytes,
                ),
                true,
            ),
            non_resident_attr(
                AttributeType::DATA.0,
                "",
                0,
                2,
                0,
                MFT_CLUSTERS.saturating_sub(1),
                MFT_CLUSTERS.saturating_mul(CLUSTER as u64),
                mft_bytes,
                mft_bytes,
                #[allow(clippy::cast_possible_wrap)]
                &encode_runs(&[(MFT_CLUSTERS, Some(MFT_LCN as i64))]),
            ),
        ],
    );
    put(&mut mft, 0, &rec_mft);

    // Record 3: $Volume — label and version.
    let mut volume_info = vec![0u8; 12];
    put_u8(&mut volume_info, 0x08, 3);
    put_u8(&mut volume_info, 0x09, 1);
    let rec_volume = mft_record(
        REC_VOLUME,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(super::attr::DOS_HIDDEN | super::attr::DOS_SYSTEM),
                false,
            ),
            resident_attr(
                AttributeType::VOLUME_NAME.0,
                "",
                0,
                1,
                &utf16(VOLUME_LABEL),
                false,
            ),
            resident_attr(
                AttributeType::VOLUME_INFORMATION.0,
                "",
                0,
                2,
                &volume_info,
                false,
            ),
        ],
    );
    put(
        &mut mft,
        (REC_VOLUME as usize).saturating_mul(MFT_RECORD),
        &rec_volume,
    );

    // Record 5: the root directory. Its index is deliberately *large*: the
    // $INDEX_ROOT holds only a terminator pointing at VCN 0, and every real
    // name lives in the INDX block. A driver that reads only $INDEX_ROOT
    // sees an empty root.
    let root_root = index_root_value(
        u32::try_from(INDEX_BLOCK).unwrap_or(4096),
        1,
        &[index_entry(0, &[], Some(0), true)],
        true,
    );
    let rec_root = mft_record(
        REC_ROOT,
        super::record::FLAG_IN_USE | super::record::FLAG_DIRECTORY,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(DOS_DIR),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(mref(REC_ROOT), ".", 1, DOS_DIR, 0, 0),
                true,
            ),
            resident_attr(AttributeType::INDEX_ROOT.0, "$I30", 0, 2, &root_root, false),
            non_resident_attr(
                AttributeType::INDEX_ALLOCATION.0,
                "$I30",
                0,
                3,
                0,
                0,
                INDEX_BLOCK as u64,
                INDEX_BLOCK as u64,
                INDEX_BLOCK as u64,
                #[allow(clippy::cast_possible_wrap)]
                &encode_runs(&[(1, Some(LCN_ROOT_INDX as i64))]),
            ),
            resident_attr(
                AttributeType::BITMAP.0,
                "$I30",
                0,
                4,
                &[1, 0, 0, 0, 0, 0, 0, 0],
                false,
            ),
        ],
    );
    put(
        &mut mft,
        (REC_ROOT as usize).saturating_mul(MFT_RECORD),
        &rec_root,
    );

    // Record 16: hello.txt — resident $DATA.
    let rec_hello = mft_record(
        REC_HELLO,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(0),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(
                    mref(REC_ROOT),
                    "hello.txt",
                    1,
                    0,
                    0,
                    HELLO_CONTENT.len() as u64,
                ),
                true,
            ),
            resident_attr(AttributeType::DATA.0, "", 0, 2, HELLO_CONTENT, false),
        ],
    );
    put(
        &mut mft,
        (REC_HELLO as usize).saturating_mul(MFT_RECORD),
        &rec_hello,
    );

    // Record 17: big.bin — non-resident, two fragments ten clusters apart.
    let rec_big = mft_record(
        REC_BIG,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(0),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(
                    mref(REC_ROOT),
                    "big.bin",
                    1,
                    0,
                    (2 * CLUSTER) as u64,
                    BIG_SIZE as u64,
                ),
                true,
            ),
            non_resident_attr(
                AttributeType::DATA.0,
                "",
                0,
                2,
                0,
                1,
                (2 * CLUSTER) as u64,
                BIG_SIZE as u64,
                BIG_SIZE as u64,
                #[allow(clippy::cast_possible_wrap)]
                &encode_runs(&[
                    (1, Some(LCN_BIG_1 as i64)),
                    (1, Some((LCN_BIG_2 as i64).saturating_sub(LCN_BIG_1 as i64))),
                ]),
            ),
        ],
    );
    put(
        &mut mft,
        (REC_BIG as usize).saturating_mul(MFT_RECORD),
        &rec_big,
    );

    // Record 18: sub/ — a *small* directory, index entirely resident. The
    // opposite shape from the root, on purpose.
    let sub_entries = vec![
        index_entry(
            mref(REC_INNER),
            &file_name_value(
                mref(REC_SUB),
                "inner.txt",
                1,
                0,
                0,
                INNER_CONTENT.len() as u64,
            ),
            None,
            false,
        ),
        index_entry(0, &[], None, true),
    ];
    let rec_sub = mft_record(
        REC_SUB,
        super::record::FLAG_IN_USE | super::record::FLAG_DIRECTORY,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(DOS_DIR),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(mref(REC_ROOT), "sub", 1, DOS_DIR, 0, 0),
                true,
            ),
            resident_attr(
                AttributeType::INDEX_ROOT.0,
                "$I30",
                0,
                2,
                &index_root_value(
                    u32::try_from(INDEX_BLOCK).unwrap_or(4096),
                    1,
                    &sub_entries,
                    false,
                ),
                false,
            ),
        ],
    );
    put(
        &mut mft,
        (REC_SUB as usize).saturating_mul(MFT_RECORD),
        &rec_sub,
    );

    // Record 19: sub/inner.txt.
    let rec_inner = mft_record(
        REC_INNER,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(0),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(
                    mref(REC_SUB),
                    "inner.txt",
                    1,
                    0,
                    0,
                    INNER_CONTENT.len() as u64,
                ),
                true,
            ),
            resident_attr(AttributeType::DATA.0, "", 0, 2, INNER_CONTENT, false),
        ],
    );
    put(
        &mut mft,
        (REC_INNER as usize).saturating_mul(MFT_RECORD),
        &rec_inner,
    );

    // Record 20: split.bin's base record — no $DATA here at all. Its $DATA
    // is in record 21, reachable only through the $ATTRIBUTE_LIST.
    let attr_list = {
        let mut v = Vec::new();
        v.extend_from_slice(&attribute_list_entry(
            AttributeType::STANDARD_INFORMATION.0,
            0,
            mref(REC_SPLIT),
            0,
        ));
        v.extend_from_slice(&attribute_list_entry(
            AttributeType::FILE_NAME.0,
            0,
            mref(REC_SPLIT),
            1,
        ));
        v.extend_from_slice(&attribute_list_entry(
            AttributeType::DATA.0,
            0,
            mref(REC_SPLIT_EXT),
            0,
        ));
        v
    };
    let rec_split = mft_record(
        REC_SPLIT,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(0),
                false,
            ),
            resident_attr(AttributeType::ATTRIBUTE_LIST.0, "", 0, 1, &attr_list, false),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                2,
                &file_name_value(
                    mref(REC_ROOT),
                    "split.bin",
                    1,
                    0,
                    CLUSTER as u64,
                    SPLIT_SIZE as u64,
                ),
                true,
            ),
        ],
    );
    put(
        &mut mft,
        (REC_SPLIT as usize).saturating_mul(MFT_RECORD),
        &rec_split,
    );

    // Record 21: split.bin's extension record.
    let rec_split_ext = mft_record(
        REC_SPLIT_EXT,
        super::record::FLAG_IN_USE,
        mref(REC_SPLIT),
        &[non_resident_attr(
            AttributeType::DATA.0,
            "",
            0,
            0,
            0,
            0,
            CLUSTER as u64,
            SPLIT_SIZE as u64,
            SPLIT_SIZE as u64,
            #[allow(clippy::cast_possible_wrap)]
            &encode_runs(&[(1, Some(LCN_SPLIT as i64))]),
        )],
    );
    put(
        &mut mft,
        (REC_SPLIT_EXT as usize).saturating_mul(MFT_RECORD),
        &rec_split_ext,
    );

    // Record 22: sparse.bin — a hole followed by real data.
    let rec_sparse = mft_record(
        REC_SPARSE,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(0),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(
                    mref(REC_ROOT),
                    "sparse.bin",
                    1,
                    0,
                    CLUSTER as u64,
                    SPARSE_SIZE as u64,
                ),
                true,
            ),
            non_resident_attr(
                AttributeType::DATA.0,
                "",
                super::attr::ATTR_FLAG_SPARSE,
                2,
                0,
                1,
                CLUSTER as u64,
                SPARSE_SIZE as u64,
                SPARSE_SIZE as u64,
                #[allow(clippy::cast_possible_wrap)]
                &encode_runs(&[(1, None), (1, Some(LCN_SPARSE as i64))]),
            ),
        ],
    );
    put(
        &mut mft,
        (REC_SPARSE as usize).saturating_mul(MFT_RECORD),
        &rec_sparse,
    );

    put(&mut image, (MFT_LCN as usize).saturating_mul(CLUSTER), &mft);

    // ---- The root directory's INDX block ----
    //
    // Includes a DOS-namespace alias for big.bin, which must NOT appear in a
    // listing: showing both `big.bin` and `BIG~1.BIN` is the classic NTFS
    // double-listing bug.
    let root_entries = vec![
        index_entry(
            mref(REC_BIG),
            &file_name_value(mref(REC_ROOT), "BIG~1.BIN", 2, 0, 0, BIG_SIZE as u64),
            None,
            false,
        ),
        index_entry(
            mref(REC_BIG),
            &file_name_value(mref(REC_ROOT), "big.bin", 1, 0, 0, BIG_SIZE as u64),
            None,
            false,
        ),
        index_entry(
            mref(REC_HELLO),
            &file_name_value(
                mref(REC_ROOT),
                "hello.txt",
                1,
                0,
                0,
                HELLO_CONTENT.len() as u64,
            ),
            None,
            false,
        ),
        index_entry(
            mref(REC_SPARSE),
            &file_name_value(mref(REC_ROOT), "sparse.bin", 1, 0, 0, SPARSE_SIZE as u64),
            None,
            false,
        ),
        index_entry(
            mref(REC_SPLIT),
            &file_name_value(mref(REC_ROOT), "split.bin", 1, 0, 0, SPLIT_SIZE as u64),
            None,
            false,
        ),
        index_entry(
            mref(REC_SUB),
            &file_name_value(mref(REC_ROOT), "sub", 1, DOS_DIR, 0, 0),
            None,
            false,
        ),
        index_entry(0, &[], None, true),
    ];
    put(
        &mut image,
        (LCN_ROOT_INDX as usize).saturating_mul(CLUSTER),
        &indx_block(0, &root_entries),
    );

    image
}

/// The label written into the synthetic volume's `$Volume` record.
const VOLUME_LABEL: &str = "SLATE_NTFS";

// ---------------------------------------------------------------------------
// Check harness
// ---------------------------------------------------------------------------

/// Running count of checks, so the summary line is a real number rather than
/// a hand-maintained one that drifts as tests are added.
struct Checks {
    passed: u32,
}

impl Checks {
    fn new() -> Self {
        Self { passed: 0 }
    }

    /// Assert `cond`, reporting `what` on failure.
    ///
    /// Returns `Err` rather than panicking: a self-test that panics takes the
    /// kernel down before it can print the rest of its results, so one broken
    /// check hides every check after it.
    fn check(&mut self, cond: bool, what: &str) -> KernelResult<()> {
        if cond {
            self.passed = self.passed.saturating_add(1);
            Ok(())
        } else {
            serial_println!("[ntfs] SELF-TEST FAILED: {}", what);
            Err(KernelError::InternalError)
        }
    }

    /// Assert two byte slices are equal, reporting the first difference.
    fn check_bytes(&mut self, got: &[u8], want: &[u8], what: &str) -> KernelResult<()> {
        if got.len() != want.len() {
            serial_println!(
                "[ntfs] SELF-TEST FAILED: {} — length {} != {}",
                what,
                got.len(),
                want.len()
            );
            return Err(KernelError::InternalError);
        }
        for (i, (a, b)) in got.iter().zip(want.iter()).enumerate() {
            if a != b {
                serial_println!(
                    "[ntfs] SELF-TEST FAILED: {} — byte {} is {:#04x}, expected {:#04x}",
                    what,
                    i,
                    a,
                    b
                );
                return Err(KernelError::InternalError);
            }
        }
        self.passed = self.passed.saturating_add(1);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests over the primitives
// ---------------------------------------------------------------------------

fn test_runlists(c: &mut Checks) -> KernelResult<()> {
    // Two fragments, the second ten clusters further on.
    let encoded = encode_runs(&[(1, Some(10)), (1, Some(10))]);
    let runs = decode_runlist(&encoded, 0)?;
    c.check(runs.len() == 2, "two runs decode")?;
    c.check(
        runs.first()
            == Some(&DataRun {
                vcn: 0,
                lcn: Some(10),
                length: 1,
            }),
        "first run",
    )?;
    c.check(
        runs.get(1)
            == Some(&DataRun {
                vcn: 1,
                lcn: Some(20),
                length: 1,
            }),
        "second run is a delta, not an absolute",
    )?;

    // A *backward* delta. This is the case a parser that reads the offset as
    // unsigned gets spectacularly wrong: -1 would decode as 255.
    let encoded = encode_runs(&[(4, Some(100)), (4, Some(-1))]);
    let runs = decode_runlist(&encoded, 0)?;
    c.check(
        runs.get(1).and_then(|r| r.lcn) == Some(99),
        "negative delta sign-extends",
    )?;

    // A sparse run occupies VCNs but no LCNs, and does not move the cursor.
    let encoded = encode_runs(&[(2, Some(50)), (3, None), (1, Some(1))]);
    let runs = decode_runlist(&encoded, 0)?;
    c.check(runs.len() == 3, "sparse run is still a run")?;
    c.check(runs.get(1).map(|r| r.lcn) == Some(None), "hole has no LCN")?;
    c.check(
        runs.get(2).and_then(|r| r.lcn) == Some(51),
        "delta after a hole is relative to the last real LCN",
    )?;

    // start_vcn offsets the whole list (a mid-file attribute record).
    let encoded = encode_runs(&[(3, Some(7))]);
    let runs = decode_runlist(&encoded, 100)?;
    c.check(
        runs.first().map(|r| r.vcn) == Some(100),
        "runlist honours start_vcn",
    )?;

    // A zero-length run would make the decoder spin.
    let bad = [0x11u8, 0x00, 0x05, 0x00];
    c.check(
        decode_runlist(&bad, 0).is_err(),
        "zero-length run is rejected",
    )?;

    Ok(())
}

fn test_boot_sector(c: &mut Checks) -> KernelResult<()> {
    let image = build_image();
    let boot = BootSector::parse(image.get(0..512).unwrap_or(&[]))?;

    c.check(boot.bytes_per_sector == 512, "bytes per sector")?;
    c.check(boot.sectors_per_cluster == 8, "sectors per cluster")?;
    c.check(boot.cluster_size == 4096, "cluster size")?;
    c.check(
        boot.mft_record_size == 1024,
        "negative clusters_per_mft_record decodes to 2^10",
    )?;
    c.check(boot.index_record_size == 4096, "index record size")?;
    c.check(boot.mft_lcn == MFT_LCN, "MFT LCN")?;

    // Not NTFS at all.
    let mut foreign = image.clone();
    put(&mut foreign, 0x03, b"MSDOS5.0");
    c.check(
        matches!(
            BootSector::parse(foreign.get(0..512).unwrap_or(&[])),
            Err(KernelError::NotSupported)
        ),
        "a non-NTFS OEM id is NotSupported, not CorruptedData",
    )?;
    c.check(
        !super::boot::looks_like_ntfs(foreign.get(0..512).unwrap_or(&[])),
        "probe rejects a foreign volume",
    )?;

    // A sector size that is not a power of two would break every offset.
    let mut bad = image.clone();
    put(&mut bad, 0x0B, &700u16.to_le_bytes());
    c.check(
        BootSector::parse(bad.get(0..512).unwrap_or(&[])).is_err(),
        "a non-power-of-two sector size is rejected",
    )?;

    // An MFT outside the volume.
    let mut bad = image.clone();
    put(&mut bad, 0x30, &9_999_999u64.to_le_bytes());
    c.check(
        BootSector::parse(bad.get(0..512).unwrap_or(&[])).is_err(),
        "an MFT outside the volume is rejected",
    )?;

    Ok(())
}

fn test_fixups(c: &mut Checks) -> KernelResult<()> {
    // Build a record, then verify the fixups round-trip: the bytes the
    // builder displaced must come back.
    let mut buf = vec![0u8; MFT_RECORD];
    put(&mut buf, 0, FILE_MAGIC);
    // Put a recognisable value in each sector's last two bytes.
    put(&mut buf, BYTES_PER_SECTOR - 2, &0xBEEFu16.to_le_bytes());
    put(&mut buf, 2 * BYTES_PER_SECTOR - 2, &0xF00Du16.to_le_bytes());
    put(&mut buf, 0x04, &0x30u16.to_le_bytes());
    put(&mut buf, 0x06, &3u16.to_le_bytes());
    write_fixups(&mut buf, 0x30, 3, 0x0101);

    // While fixed up, the tails carry the USN, not the data.
    c.check(
        buf.get(BYTES_PER_SECTOR - 2..BYTES_PER_SECTOR) == Some(&[0x01, 0x01][..]),
        "fixup stamps the USN into the sector tail",
    )?;

    apply_fixups(&mut buf, FILE_MAGIC, 512)?;
    c.check(
        buf.get(BYTES_PER_SECTOR - 2..BYTES_PER_SECTOR) == Some(&0xBEEFu16.to_le_bytes()[..]),
        "fixups restore the first sector's tail",
    )?;
    c.check(
        buf.get(2 * BYTES_PER_SECTOR - 2..2 * BYTES_PER_SECTOR)
            == Some(&0xF00Du16.to_le_bytes()[..]),
        "fixups restore the second sector's tail",
    )?;

    // A torn write: the second sector was not part of the same update, so
    // its tail does not carry the USN.
    let mut torn = vec![0u8; MFT_RECORD];
    put(&mut torn, 0, FILE_MAGIC);
    put(&mut torn, 0x04, &0x30u16.to_le_bytes());
    put(&mut torn, 0x06, &3u16.to_le_bytes());
    write_fixups(&mut torn, 0x30, 3, 0x0101);
    put(
        &mut torn,
        2 * BYTES_PER_SECTOR - 2,
        &0x9999u16.to_le_bytes(),
    );
    c.check(
        apply_fixups(&mut torn, FILE_MAGIC, 512) == Err(KernelError::CorruptedData),
        "a torn write is detected, not silently accepted",
    )?;

    // Wrong magic.
    let mut wrong = vec![0u8; MFT_RECORD];
    put(&mut wrong, 0, b"BAAD");
    c.check(
        apply_fixups(&mut wrong, FILE_MAGIC, 512).is_err(),
        "wrong magic is rejected",
    )?;

    Ok(())
}

fn test_primitives(c: &mut Checks) -> KernelResult<()> {
    c.check(
        filetime_to_unix_ns(TEST_FILETIME) == TEST_UNIX_NS,
        "FILETIME converts to Unix nanoseconds",
    )?;
    c.check(
        filetime_to_unix_ns(0) == 0,
        "a pre-1970 FILETIME saturates to 0 rather than wrapping",
    )?;

    let name = utf16("héllo");
    c.check(
        utf16le_at(&name, 0, 5).as_deref() == Some("héllo"),
        "UTF-16LE names decode",
    )?;
    c.check(
        utf16le_at(&name, 0, 100).is_none(),
        "an out-of-bounds name read fails rather than reading past the end",
    )?;

    // An unpaired surrogate must not make the whole name unreadable.
    let lone = [0x00u8, 0xD8, 0x41, 0x00];
    c.check(
        utf16le_at(&lone, 0, 2).as_deref() == Some("\u{FFFD}A"),
        "an unpaired surrogate becomes U+FFFD",
    )?;

    c.check(
        mft_ref_record(mref(REC_ROOT)) == REC_ROOT,
        "MFT reference masks off the sequence number",
    )?;
    c.check(
        u64::from(mft_ref_sequence(mref(REC_ROOT))) == SEQ,
        "MFT reference exposes the sequence number",
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// End-to-end tests over the synthetic volume
// ---------------------------------------------------------------------------

fn mount_image() -> KernelResult<NtfsFs> {
    NtfsFs::open_source(Box::new(MemorySource::new(build_image())))
}

fn names_of(fs: &mut NtfsFs, path: &str) -> KernelResult<Vec<String>> {
    let entries = fs.readdir(Path::new(path))?;
    Ok(entries
        .iter()
        .map(|e| String::from_utf8_lossy(e.name.as_bytes()).into_owned())
        .collect())
}

fn test_volume(c: &mut Checks) -> KernelResult<()> {
    let mut fs = mount_image()?;

    // -- mount ----------------------------------------------------------
    let info = fs.statvfs()?;
    c.check(info.fs_type == "ntfs", "fs_type")?;
    c.check(
        info.volume_label == VOLUME_LABEL,
        "volume label comes from $Volume's $VOLUME_NAME",
    )?;
    c.check(info.read_only, "an NTFS mount is read-only")?;
    c.check(
        info.block_size == CLUSTER as u64,
        "block size is the cluster",
    )?;

    // -- root listing ---------------------------------------------------
    let names = names_of(&mut fs, "/")?;
    c.check(
        names.len() == 5,
        "root lists exactly its five user entries (the INDX block was read)",
    )?;
    for want in ["hello.txt", "big.bin", "sub", "split.bin", "sparse.bin"] {
        c.check(
            names.iter().any(|n| n == want),
            "root listing contains its entry",
        )?;
    }
    c.check(
        !names.iter().any(|n| n == "BIG~1.BIN"),
        "the 8.3 alias is hidden, so big.bin is not listed twice",
    )?;
    c.check(
        !names.iter().any(|n| n == "$MFT"),
        "volume metadata files are not listed",
    )?;

    // -- resident data --------------------------------------------------
    let data = fs.read_file(Path::new("/hello.txt"))?;
    c.check_bytes(&data, HELLO_CONTENT, "resident $DATA reads back")?;

    // -- non-resident, fragmented data ----------------------------------
    let big = big_content();
    let data = fs.read_file(Path::new("/big.bin"))?;
    c.check_bytes(&data, &big, "fragmented non-resident $DATA reads back")?;

    // A read that straddles the fragment boundary is where a runlist bug
    // shows up: the first cluster is right and the rest is garbage.
    let across = fs.read_at(Path::new("/big.bin"), (CLUSTER - 8) as u64, 32)?;
    c.check_bytes(
        &across,
        big.get(CLUSTER - 8..CLUSTER + 24).unwrap_or(&[]),
        "a read across the fragment boundary is contiguous",
    )?;

    // A read past the end returns what exists, not an error.
    let tail = fs.read_at(Path::new("/big.bin"), (BIG_SIZE - 10) as u64, 500)?;
    c.check(tail.len() == 10, "a read past EOF is clamped to the file")?;

    // -- sparse ---------------------------------------------------------
    let sparse = fs.read_file(Path::new("/sparse.bin"))?;
    c.check_bytes(
        &sparse,
        &sparse_content(),
        "a sparse run reads as zeroes followed by real data",
    )?;

    // -- $ATTRIBUTE_LIST ------------------------------------------------
    let split = fs.read_file(Path::new("/split.bin"))?;
    c.check_bytes(
        &split,
        &split_content(),
        "$DATA in an extension record is found through the $ATTRIBUTE_LIST",
    )?;

    // -- subdirectory (resident index, no $INDEX_ALLOCATION) ------------
    let names = names_of(&mut fs, "/sub")?;
    c.check(names.len() == 1, "a small directory's index is resident")?;
    c.check(
        names.first().map(String::as_str) == Some("inner.txt"),
        "subdirectory entry",
    )?;
    let data = fs.read_file(Path::new("/sub/inner.txt"))?;
    c.check_bytes(&data, INNER_CONTENT, "nested file reads back")?;

    // -- stat / metadata ------------------------------------------------
    let st = fs.stat(Path::new("/big.bin"))?;
    c.check(st.entry_type == EntryType::File, "stat: type")?;
    c.check(st.size == BIG_SIZE as u64, "stat: size")?;

    let st = fs.stat(Path::new("/sub"))?;
    c.check(
        st.entry_type == EntryType::Directory,
        "stat: directory type",
    )?;

    let meta = fs.metadata(Path::new("/hello.txt"))?;
    c.check(
        meta.ino == REC_HELLO,
        "metadata: ino is the MFT record number",
    )?;
    c.check(
        meta.modified_ns == TEST_UNIX_NS,
        "metadata: $STANDARD_INFORMATION timestamps convert",
    )?;
    c.check(
        meta.size == HELLO_CONTENT.len() as u64,
        "metadata: resident size",
    )?;
    c.check(meta.permissions == 0o444, "metadata: read-only permissions")?;

    // -- error paths ----------------------------------------------------
    c.check(
        fs.read_file(Path::new("/nope.txt")) == Err(KernelError::NotFound),
        "a missing file is NotFound",
    )?;
    c.check(
        fs.read_file(Path::new("/sub")) == Err(KernelError::IsADirectory),
        "reading a directory is IsADirectory",
    )?;
    c.check(
        matches!(
            fs.readdir(Path::new("/hello.txt")),
            Err(KernelError::NotADirectory)
        ),
        "listing a file is NotADirectory",
    )?;
    c.check(
        fs.read_file(Path::new("/sub/nope/deeper.txt")) == Err(KernelError::NotFound),
        "a path through a missing directory is NotFound",
    )?;

    // -- case handling --------------------------------------------------
    // Exact match wins; a unique case-insensitive match is accepted, because
    // NTFS's own collation forbids two Win32 names differing only in case.
    let data = fs.read_file(Path::new("/HELLO.TXT"))?;
    c.check_bytes(
        &data,
        HELLO_CONTENT,
        "a Windows-cased path resolves through the case-insensitive fallback",
    )?;

    Ok(())
}

/// A volume whose bytes have been damaged must fail, not read garbage.
fn test_corruption(c: &mut Checks) -> KernelResult<()> {
    // Damage a byte inside the root's INDX block *after* its fixups were
    // computed, in a sector tail — exactly what a torn write looks like.
    let mut image = build_image();
    let indx = (LCN_ROOT_INDX as usize).saturating_mul(CLUSTER);
    put(
        &mut image,
        indx.saturating_add(BYTES_PER_SECTOR).saturating_sub(2),
        &0xDEADu16.to_le_bytes(),
    );

    let mut fs = NtfsFs::open_source(Box::new(MemorySource::new(image)))?;
    c.check(
        matches!(fs.readdir(Path::new("/")), Err(KernelError::CorruptedData)),
        "a torn INDX block is refused rather than listed",
    )?;

    // Damage the $MFT's own record so the mount itself must fail.
    let mut image = build_image();
    put(
        &mut image,
        (MFT_LCN as usize).saturating_mul(CLUSTER),
        b"BAAD",
    );
    c.check(
        NtfsFs::open_source(Box::new(MemorySource::new(image))).is_err(),
        "a corrupt $MFT record fails the mount",
    )?;

    // Damage the *root directory's* record, number 5. Nothing in the bootstrap
    // reaches it — record 0 is read at the LCN the boot sector names, and the
    // $Volume read at record 3 is deliberately best-effort — so before the
    // mount was made to read record 5, this volume mounted cleanly and then
    // failed every path lookup, with the error surfacing far from the thing
    // that caused it.
    let mut image = build_image();
    let root_rec = (MFT_LCN as usize)
        .saturating_mul(CLUSTER)
        .saturating_add((REC_ROOT as usize).saturating_mul(MFT_RECORD));
    put(&mut image, root_rec, b"BAAD");
    c.check(
        NtfsFs::open_source(Box::new(MemorySource::new(image))).is_err(),
        "an unreadable root record fails the mount, not the first lookup",
    )?;

    // Record 5 intact, in use, and correctly fixed up — but without the
    // directory flag. Every structural check passes; only a check on the
    // root's type catches it. The flags live at 0x16 in the record header,
    // which is not a sector tail, so patching it does not disturb the update
    // sequence array the fixups depend on.
    let mut image = build_image();
    put(
        &mut image,
        root_rec.saturating_add(0x16),
        &super::record::FLAG_IN_USE.to_le_bytes(),
    );
    c.check(
        matches!(
            NtfsFs::open_source(Box::new(MemorySource::new(image))),
            Err(KernelError::CorruptedData)
        ),
        "a root record that is not a directory fails the mount",
    )?;

    Ok(())
}

/// Compressed data must be refused, not returned as compressed bytes.
fn test_compression_is_refused(c: &mut Checks) -> KernelResult<()> {
    let mut image = build_image();

    // Set the compressed flag on big.bin's $DATA. Locating it by scanning
    // for the attribute is fragile; instead rebuild the record with the flag
    // set, which is what the builder is for.
    let rec = mft_record(
        REC_BIG,
        super::record::FLAG_IN_USE,
        0,
        &[
            resident_attr(
                AttributeType::STANDARD_INFORMATION.0,
                "",
                0,
                0,
                &standard_information(0),
                false,
            ),
            resident_attr(
                AttributeType::FILE_NAME.0,
                "",
                0,
                1,
                &file_name_value(
                    mref(REC_ROOT),
                    "big.bin",
                    1,
                    0,
                    (2 * CLUSTER) as u64,
                    BIG_SIZE as u64,
                ),
                true,
            ),
            non_resident_attr(
                AttributeType::DATA.0,
                "",
                super::attr::ATTR_FLAG_COMPRESSED,
                2,
                0,
                1,
                (2 * CLUSTER) as u64,
                BIG_SIZE as u64,
                BIG_SIZE as u64,
                #[allow(clippy::cast_possible_wrap)]
                &encode_runs(&[
                    (1, Some(LCN_BIG_1 as i64)),
                    (1, Some((LCN_BIG_2 as i64).saturating_sub(LCN_BIG_1 as i64))),
                ]),
            ),
        ],
    );
    let offset = (MFT_LCN as usize)
        .saturating_mul(CLUSTER)
        .saturating_add((REC_BIG as usize).saturating_mul(MFT_RECORD));
    put(&mut image, offset, &rec);

    let mut fs = NtfsFs::open_source(Box::new(MemorySource::new(image)))?;
    c.check(
        fs.read_file(Path::new("/big.bin")) == Err(KernelError::NotSupported),
        "compressed $DATA is refused, not returned as compressed bytes",
    )?;
    // The directory it lives in must still be listable.
    c.check(
        fs.readdir(Path::new("/")).map(|e| e.len()) == Ok(5),
        "one unreadable file does not make its directory unreadable",
    )?;

    Ok(())
}

/// The namespace filter, tested directly as well as through a listing.
fn test_namespaces(c: &mut Checks) -> KernelResult<()> {
    c.check(NameSpace::from_raw(0) == NameSpace::Posix, "namespace 0")?;
    c.check(NameSpace::from_raw(1) == NameSpace::Win32, "namespace 1")?;
    c.check(NameSpace::from_raw(2) == NameSpace::Dos, "namespace 2")?;
    c.check(
        NameSpace::from_raw(3) == NameSpace::Win32AndDos,
        "namespace 3",
    )?;
    c.check(
        NameSpace::from_raw(9) == NameSpace::Unknown(9),
        "an unknown namespace is preserved, not coerced",
    )?;
    c.check(!NameSpace::Dos.is_visible(), "DOS aliases are hidden")?;
    c.check(
        NameSpace::Win32AndDos.is_visible(),
        "a name that is both long and short is still shown",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run every NTFS self-test.
///
/// # Errors
///
/// Returns the first failure; the failing check has already been printed.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[ntfs] Running self-test...");

    let mut c = Checks::new();

    test_primitives(&mut c)?;
    test_runlists(&mut c)?;
    test_boot_sector(&mut c)?;
    test_fixups(&mut c)?;
    test_namespaces(&mut c)?;
    test_volume(&mut c)?;
    test_corruption(&mut c)?;
    test_compression_is_refused(&mut c)?;

    serial_println!(
        "[ntfs] Self-test passed ({} checks over a synthetic volume).",
        c.passed
    );
    Ok(())
}
