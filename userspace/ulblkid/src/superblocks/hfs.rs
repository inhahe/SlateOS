//! `hfs.c`: HFS and HFS+ (HFSX, and HFS+ embedded in an HFS wrapper). The
//! UUID is derived from the Finder info's volume ID as Apple derives it;
//! HFS+'s label is the root folder's name in the catalog B-tree.

use crate::encode::ENCODE_UTF16BE;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, IDINFO_TOLERANT, USAGE_FILESYSTEM};

/// `sizeof(struct hfs_mdb)`.
const HFS_MDB_SIZE: u64 = 130;
/// `sizeof(struct hfsplus_vol_header)`.
const HFSPLUS_VH_SIZE: u64 = 512;
/// `sizeof(struct hfsplus_bnode_descriptor)`.
const BNODE_DESC: usize = 14;
/// `sizeof(struct hfsplus_catalog_key)`.
const CATALOG_KEY: u32 = 518;
/// `HFS_NODE_LEAF`.
const HFS_NODE_LEAF: u8 = 0xff;
/// `HFSPLUS_POR_CNID`: the root folder's parent.
const HFSPLUS_POR_CNID: u32 = 1;

/// `hfs_set_uuid(pr, hfs_info, len)`: MD5 of a fixed namespace and the
/// volume ID, made a version-3 UUID. Nothing for an all-zero ID.
fn set_uuid(pr: &mut Probe, id: &[u8]) {
    const HASH_INIT: [u8; 16] = [
        0xb3, 0xe2, 0x0f, 0x39, 0xf2, 0x92, 0x11, 0xd6, 0x97, 0xa4, 0x00, 0x30, 0x65, 0x43, 0xec,
        0xac,
    ];
    if id.iter().all(|&b| b == 0) {
        return;
    }
    let mut ctx = md5::Md5::new();
    ctx.update(&HASH_INIT);
    ctx.update(id);
    let mut uuid = ctx.finalize();
    uuid[6] = 0x30 | (uuid[6] & 0x0f);
    uuid[8] = 0x80 | (uuid[8] & 0x3f);
    pr.set_uuid(&uuid);
}

/// `probe_hfs`.
fn probe_hfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(hfs) = pr.get_sb_buffer(mag, HFS_MDB_SIZE) else {
        return pr.none_or_err();
    };
    // An HFS wrapper around HFS+ is the HFS+ prober's.
    let embed = hfs.span(124, 2);
    if embed == b"H+" || embed == b"HX" {
        return 1;
    }
    let size = hfs.be32(20);
    if size == 0 || size & 511 != 0 {
        return 1;
    }
    set_uuid(pr, hfs.span(116, 8));
    let len = usize::from(hfs.u8_at(36)).min(27);
    pr.set_label(hfs.span(37, len));
    0
}

/// `probe_hfsplus`.
fn probe_hfsplus(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sbd) = pr.get_sb_buffer(mag, HFS_MDB_SIZE) else {
        return pr.none_or_err();
    };
    let mut off: u32 = 0;
    let hfsplus = if sbd.span(0, 2) == b"BD" {
        // HFS+ embedded in an HFS volume.
        let embed = sbd.span(124, 2);
        if embed != b"H+" && embed != b"HX" {
            return 1;
        }
        let alloc_block_size = sbd.be32(20);
        let alloc_first_block = u32::from(sbd.be16(28));
        let embed_first_block = u32::from(sbd.be16(126));
        off = alloc_first_block
            .wrapping_mul(512)
            .wrapping_add(embed_first_block.wrapping_mul(alloc_block_size));
        let kboff = mag.map_or(0, |m| m.kboff);
        // C adds the long to the unsigned offset, and passes the sum as a
        // uint64_t.
        let at = i64::from(off)
            .wrapping_add(kboff.wrapping_mul(1024))
            .cast_unsigned();
        pr.get_buffer(at, HFSPLUS_VH_SIZE)
    } else {
        pr.get_sb_buffer(mag, HFSPLUS_VH_SIZE)
    };
    let Some(hfsplus) = hfsplus else {
        return pr.none_or_err();
    };
    let sig = hfsplus.span(0, 2);
    if sig != b"H+" && sig != b"HX" {
        return 1;
    }
    set_uuid(pr, hfsplus.span(104, 8));
    let blocksize = hfsplus.be32(40);
    if blocksize < 512 {
        return 1;
    }
    pr.set_fsblocksize(blocksize);
    pr.set_block_size(blocksize);
    // The catalog file's extents.
    let extents = hfsplus.span(288, 64).to_vec();
    let cat_block = extents.be32(0);
    let err_or_ok = |pr: &Probe| {
        if pr.errno != 0 {
            pr.errno.wrapping_neg()
        } else {
            0
        }
    };
    let cat_off = u64::from(cat_block).wrapping_mul(u64::from(blocksize));
    let Some(buf) = pr.get_buffer(u64::from(off).wrapping_add(cat_off), 0x2000) else {
        return err_or_ok(pr);
    };
    // The B-tree header record, after the node descriptor.
    let leaf_node_head = buf.be32(BNODE_DESC + 10);
    let leaf_node_size = u32::from(buf.be16(BNODE_DESC + 18));
    let leaf_node_count = buf.be32(BNODE_DESC + 6);
    let min_leaf_node_size = u32::try_from(BNODE_DESC)
        .unwrap_or(0)
        .wrapping_add(CATALOG_KEY);
    if leaf_node_size < min_leaf_node_size || leaf_node_count == 0 {
        return 0;
    }
    // blocksize is at least 512: checked above.
    let mut leaf_block = leaf_node_head
        .wrapping_mul(leaf_node_size)
        .checked_div(blocksize)
        .unwrap_or(0);
    // The first leaf's physical block, through the extents.
    let mut ext_block_start = 0u32;
    let mut found = false;
    for ext in 0..8usize {
        ext_block_start = extents.be32(ext.saturating_mul(8));
        let ext_block_count = extents.be32(ext.saturating_mul(8).saturating_add(4));
        if ext_block_count == 0 {
            return 0;
        }
        if leaf_block < ext_block_count {
            found = true;
            break;
        }
        leaf_block = leaf_block.wrapping_sub(ext_block_count);
    }
    if !found {
        return 0;
    }
    let leaf_off = u64::from(ext_block_start)
        .wrapping_add(u64::from(leaf_block))
        .wrapping_mul(u64::from(blocksize));
    let Some(buf) = pr.get_buffer(
        u64::from(off).wrapping_add(leaf_off),
        u64::from(leaf_node_size),
    ) else {
        return err_or_ok(pr);
    };
    if buf.be16(10) == 0 {
        return 0;
    }
    if buf.u8_at(8) != HFS_NODE_LEAF {
        return 0;
    }
    // The first key: the root folder's thread record, named for the volume.
    let key = buf.get(BNODE_DESC..).unwrap_or_default();
    let unicode_len = usize::from(key.be16(6));
    if key.be32(2) != HFSPLUS_POR_CNID || unicode_len > 255 {
        return 0;
    }
    pr.set_utf8label(key.span(8, unicode_len * 2), ENCODE_UTF16BE);
    0
}

/// `hfs_idinfo`.
pub static HFS: IdInfo = IdInfo {
    name: "hfs",
    usage: USAGE_FILESYSTEM,
    flags: IDINFO_TOLERANT,
    minsz: 0,
    probefunc: Some(probe_hfs),
    magics: &[IdMag::new(b"BD", 1, 0)],
};

/// `hfsplus_idinfo`.
pub static HFSPLUS: IdInfo = IdInfo {
    name: "hfsplus",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_hfsplus),
    magics: &[
        IdMag::new(b"BD", 1, 0),
        IdMag::new(b"H+", 1, 0),
        IdMag::new(b"HX", 1, 0),
    ],
};
