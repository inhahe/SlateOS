//! `solaris_x86.c`: the Solaris x86 VTOC, in sector 1 of a DOS partition of
//! type 0x82 (Linux swap's byte). Slices are relative to that partition.
//!
//! Upstream's loop starts its counter at 1 and its slice pointer at 0, so
//! it reads slices 0 to `v_nparts - 2` and never the last. The port reads
//! the same ones.

use super::{Partition, is_nested_dimension, need_typeonly};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK};

/// `SOLARIS_MAXPARTITIONS`.
const SOLARIS_MAXPARTITIONS: u16 = 16;
/// `SOLARIS_SECTOR`.
const SOLARIS_SECTOR: u32 = 1;
/// `SOLARIS_OFFSET`: the label's offset in bytes.
const SOLARIS_OFFSET: u64 = 512;
/// `SOLARIS_MAGICOFFSET`: `v_sanity`'s, `512 + 12`.
const SOLARIS_MAGICOFFSET: u32 = 524;
/// `SOLARIS_TAG_WHOLEDISK`.
const SOLARIS_TAG_WHOLEDISK: u16 = 5;
/// `offsetof(struct solaris_vtoc, v_version)`.
const V_VERSION: usize = 16;
/// `offsetof(struct solaris_vtoc, v_nparts)`.
const V_NPARTS: usize = 30;
/// `offsetof(struct solaris_vtoc, v_slice)`.
const V_SLICE: usize = 72;
/// `sizeof(struct solaris_slice)`.
const SLICE_SIZE: usize = 12;

/// `probe_solaris_pt`.
fn probe_solaris_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(l) = pr.get_sector(SOLARIS_SECTOR) else {
        return pr.none_or_err();
    };
    if l.le32(V_VERSION) != 1 {
        // An unsupported version.
        return PROBE_NONE;
    }
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let parent: Option<Partition> = ls.parent().and_then(|id| ls.partition(id.0).cloned());
    let tab = ls.new_parttable("solaris", SOLARIS_OFFSET);
    let nparts = usize::from(l.le16(V_NPARTS).min(SOLARIS_MAXPARTITIONS));
    for i in 1..nparts {
        // Slice i - 1: see the module comment.
        let p = V_SLICE.saturating_add(i.saturating_sub(1).saturating_mul(SLICE_SIZE));
        let tag = l.le16(p);
        let flag = l.le16(p.saturating_add(2));
        let mut start = l.le32(p.saturating_add(4));
        let size = l.le32(p.saturating_add(8));
        if size == 0 || tag == SOLARIS_TAG_WHOLEDISK {
            continue;
        }
        if let Some(par) = &parent {
            // Slices are relative to the parent. Its 64-bit start is added
            // into a 32-bit variable, truncating as C does.
            let low = u32::try_from(par.start & u64::from(u32::MAX)).unwrap_or(0);
            start = start.wrapping_add(low);
            if !is_nested_dimension(par, u64::from(start), u64::from(size)) {
                // Reaches outside its parent: ignored.
                continue;
            }
        }
        let id = ls.add_partition(tab, u64::from(start), u64::from(size));
        if let Some(pp) = ls.part_mut(id) {
            pp.set_type(i32::from(tag));
            pp.set_flags(u64::from(flag));
        }
    }
    PROBE_OK
}

/// `solaris_x86_pt_idinfo`: the little-endian VTOC sanity 0x600DDEEE.
pub static SOLARIS_X86: IdInfo = IdInfo {
    name: "solaris",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_solaris_pt),
    magics: &[IdMag::new(b"\xEE\xDE\x0D\x60", 0, SOLARIS_MAGICOFFSET)],
};
