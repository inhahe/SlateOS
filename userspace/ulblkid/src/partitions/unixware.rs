//! `unixware.c`: the UnixWare VTOC, in sector 29 of a DOS partition of type
//! 0x63. The first slice, the whole disk, is skipped; the others count only
//! when marked valid, and (nested) only inside their parent.
//!
//! Upstream's magic offset is miscomputed -- `UNIXWARE_OFFSET -
//! UNIXWARE_KBOFFSET + 4` subtracts the KiB count where it meant the KiB
//! offset -- so the magic search looks at byte 29174 rather than at the
//! label's `d_magic` (byte 14852). The port searches where upstream does.

use super::{Partition, is_nested_dimension, need_typeonly};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK};

/// `UNIXWARE_SECTOR`.
const UNIXWARE_SECTOR: u32 = 29;
/// `UNIXWARE_OFFSET`: the label's offset in bytes, `29 << 9`.
const UNIXWARE_OFFSET: u64 = 14848;
/// `UNIXWARE_KBOFFSET`: the same in whole KiB, `14848 >> 10`.
const UNIXWARE_KBOFFSET: i64 = 14;
/// `UNIXWARE_MAGICOFFSET`, as upstream computes it: `14848 - 14 + 4`.
const UNIXWARE_MAGICOFFSET: u32 = 14838;
/// `UNIXWARE_VTOCMAGIC`.
const UNIXWARE_VTOCMAGIC: u32 = 0x600D_DEEE;
/// `UNIXWARE_MAXPARTITIONS`.
const UNIXWARE_MAXPARTITIONS: usize = 16;
/// `UNIXWARE_TAG_UNUSED`.
const UNIXWARE_TAG_UNUSED: u16 = 0x0000;
/// `UNIXWARE_TAG_ENTIRE_DISK`.
const UNIXWARE_TAG_ENTIRE_DISK: u16 = 0x0005;
/// `UNIXWARE_FLAG_VALID`.
const UNIXWARE_FLAG_VALID: u16 = 0x0200;
/// `offsetof(struct unixware_disklabel, vtoc.v_magic)`.
const V_MAGIC: usize = 156;
/// `offsetof(struct unixware_disklabel, vtoc.v_slice)`.
const V_SLICE: usize = 216;
/// `sizeof(struct unixware_partition)`.
const SLICE_SIZE: usize = 12;

/// `probe_unixware_pt`.
fn probe_unixware_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(l) = pr.get_sector(UNIXWARE_SECTOR) else {
        return pr.none_or_err();
    };
    if l.le32(V_MAGIC) != UNIXWARE_VTOCMAGIC {
        return PROBE_NONE;
    }
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let parent: Option<Partition> = ls.parent().and_then(|id| ls.partition(id.0).cloned());
    let tab = ls.new_parttable("unixware", UNIXWARE_OFFSET);
    // Slice 0 describes the whole disk.
    for i in 1..UNIXWARE_MAXPARTITIONS {
        let p = V_SLICE.saturating_add(i.saturating_mul(SLICE_SIZE));
        let tag = l.le16(p);
        let flg = l.le16(p.saturating_add(2));
        if tag == UNIXWARE_TAG_UNUSED
            || tag == UNIXWARE_TAG_ENTIRE_DISK
            || flg != UNIXWARE_FLAG_VALID
        {
            continue;
        }
        let start = l.le32(p.saturating_add(4));
        let size = l.le32(p.saturating_add(8));
        if let Some(par) = &parent
            && !is_nested_dimension(par, u64::from(start), u64::from(size))
        {
            // Reaches outside its parent: ignored.
            continue;
        }
        let id = ls.add_partition(tab, u64::from(start), u64::from(size));
        if let Some(pp) = ls.part_mut(id) {
            pp.set_type(i32::from(tag));
            pp.set_flags(u64::from(flg));
        }
    }
    PROBE_OK
}

/// `unixware_pt_idinfo`: never on a floppy.
pub static UNIXWARE: IdInfo = IdInfo {
    name: "unixware",
    usage: 0,
    flags: 0,
    minsz: 1024 * 1440 + 1,
    probefunc: Some(probe_unixware_pt),
    magics: &[IdMag::new(
        b"\x0D\x60\xE5\xCA",
        UNIXWARE_KBOFFSET,
        UNIXWARE_MAGICOFFSET,
    )],
};
