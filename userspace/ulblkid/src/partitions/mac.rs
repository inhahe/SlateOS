//! `mac.c`: the Apple partition map -- a driver descriptor in block 0 and
//! one map entry per block from block 1, every entry carrying the map's
//! length. As the Linux kernel does (and libparted does not), every entry
//! is a partition, free-space entries included.

use super::need_typeonly;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Buf, Bytes, PROBE_NONE, PROBE_OK};

/// `MAC_PARTITION_MAGIC`.
const MAC_PARTITION_MAGIC: u16 = 0x504d;
/// `MAC_PARTITION_MAGIC_OLD`.
const MAC_PARTITION_MAGIC_OLD: u16 = 0x5453;
/// `sizeof(struct mac_partition)`.
const MAC_PARTITION_SIZE: u16 = 136;

/// `get_mac_block(pr, block_size, num)`: block `num` of the map.
fn get_mac_block(pr: &mut Probe, block_size: u16, num: u32) -> Option<Buf> {
    pr.get_buffer(
        u64::from(num).wrapping_mul(u64::from(block_size)),
        u64::from(block_size),
    )
}

/// `has_part_signature(p)`: a map entry, by either signature.
fn has_part_signature(p: &[u8]) -> bool {
    matches!(p.be16(0), MAC_PARTITION_MAGIC | MAC_PARTITION_MAGIC_OLD)
}

/// `probe_mac_pt`.
fn probe_mac_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    // The driver descriptor record is always block 0.
    let Some(md) = pr.get_sector(0) else {
        return pr.none_or_err();
    };
    let block_size = md.be16(2);
    if block_size < MAC_PARTITION_SIZE {
        return PROBE_NONE;
    }
    // The map always starts at block 1.
    let Some(p) = get_mac_block(pr, block_size, 1) else {
        return pr.none_or_err();
    };
    if !has_part_signature(&p) {
        return PROBE_NONE;
    }
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    if pr.partlist().is_none() {
        return PROBE_NONE;
    }
    let tab = match pr.partlist_mut() {
        Some(ls) => ls.new_parttable("mac", 0),
        None => return PROBE_NONE,
    };
    let ssf = u32::from(block_size / 512);
    let nblks = p.be32(4);
    // The first entry's count, capped: a corrupt map is not read for ever.
    let nprts = nblks.min(256);
    for i in 0..nprts {
        let Some(p) = get_mac_block(pr, block_size, i.saturating_add(1)) else {
            return pr.none_or_err();
        };
        if !has_part_signature(&p) {
            return PROBE_NONE;
        }
        // An entry disagreeing with the first about the map's length is
        // only a debugging message upstream.
        let start = p.be32(8).wrapping_mul(ssf);
        let size = p.be32(12).wrapping_mul(ssf);
        let Some(ls) = pr.partlist_mut() else {
            return PROBE_NONE;
        };
        let par = ls.add_partition(tab, u64::from(start), u64::from(size));
        if let Some(pp) = ls.part_mut(par) {
            pp.set_name(p.span(16, 32));
            pp.set_type_string(p.span(48, 32));
        }
    }
    PROBE_OK
}

/// `mac_pt_idinfo`: every Apple disk starts with the driver descriptor
/// record, "ER".
pub static MAC: IdInfo = IdInfo {
    name: "mac",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_mac_pt),
    magics: &[IdMag::new(b"\x45\x52", 0, 0)],
};
