//! `atari.c`: the Atari ST partition table (AHDI) -- four primary entries
//! in the root sector, extended ("XGM") partitions chained through root
//! sectors of their own, and the eight extra entries of the ICD format.
//!
//! It has no magic string, so it runs last and believes a root sector only
//! when the disk size it records is plausible, its bad-sector list fits
//! and at least one entry is an active partition with an alphanumeric ID
//! inside the disk.

use super::{Partlist, TabId, need_typeonly};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, ENOMEM, PROBE_NONE, PROBE_OK};

/// `offsetof(struct atari_rootsector, icd_part)`.
const ICD_PART: usize = 0x156;
/// `offsetof(struct atari_rootsector, hd_size)`.
const HD_SIZE: usize = 0x1c2;
/// `offsetof(struct atari_rootsector, part)`.
const PART: usize = 0x1c6;
/// `offsetof(struct atari_rootsector, bsl_start)`.
const BSL_START: usize = 0x1f6;
/// `offsetof(struct atari_rootsector, bsl_len)`.
const BSL_LEN: usize = 0x1fa;
/// `sizeof(struct atari_part_def)`.
const PART_DEF_SIZE: usize = 12;

/// `struct atari_part_def`.
#[derive(Clone, Copy, Debug)]
struct PartDef {
    /// `flags`: bit 0 active, bit 7 bootable.
    flags: u8,
    /// `id`: three characters.
    id: [u8; 3],
    /// `start`, big-endian on disk.
    start: u32,
    /// `size`.
    size: u32,
}

impl PartDef {
    /// The entry at `off` in a root sector.
    fn read(rs: &[u8], off: usize) -> PartDef {
        PartDef {
            flags: rs.u8_at(off),
            id: [
                rs.u8_at(off.saturating_add(1)),
                rs.u8_at(off.saturating_add(2)),
                rs.u8_at(off.saturating_add(3)),
            ],
            start: rs.be32(off.saturating_add(4)),
            size: rs.be32(off.saturating_add(8)),
        }
    }

    /// `IS_ACTIVE(partdef)`.
    fn is_active(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// Primary entry `i` of a root sector.
fn part(rs: &[u8], i: usize) -> PartDef {
    PartDef::read(rs, PART.saturating_add(i.saturating_mul(PART_DEF_SIZE)))
}

/// `linux_isalnum(c)`: Linux's `isalnum`, which is Latin-1's -- ASCII
/// letters and digits, and the Latin-1 letters from 0xC0 (but not the
/// multiplication and division signs).
fn linux_isalnum(c: u8) -> bool {
    c.is_ascii_alphanumeric() || (c >= 0xc0 && c != 0xd7 && c != 0xf7)
}

/// `is_valid_dimension(start, size, maxoff)`: `start..start+size` is a
/// non-empty range within `maxoff` sectors. The end is summed in 32 bits,
/// and a wrapped sum fails `end >= start`.
fn is_valid_dimension(start: u32, size: u32, maxoff: u32) -> bool {
    let end = start.wrapping_add(size);
    end >= start
        && 0 < start
        && start <= maxoff
        && 0 < size
        && size <= maxoff
        && 0 < end
        && end <= maxoff
}

/// `is_valid_partition(part, maxoff)`: active, alphanumeric ID, inside the
/// disk.
fn is_valid_partition(p: &PartDef, maxoff: u32) -> bool {
    p.is_active()
        && p.id.iter().all(|&c| linux_isalnum(c))
        && is_valid_dimension(p.start, p.size, maxoff)
}

/// `is_id_common(id)`: one of the IDs the ICD format's entries have.
fn is_id_common(id: &[u8; 3]) -> bool {
    [b"GEM", b"BGM", b"LNX", b"SWP", b"RAW"].contains(&id)
}

/// `parse_partition(ls, tab, part, offset)`: add the entry, its start
/// `offset` sectors on. One already found at that start is skipped -- and
/// for a primary (`offset` 0) its number used up. 1 added, 0 skipped.
fn parse_partition(ls: &mut Partlist, tab: TabId, p: &PartDef, offset: u32) -> i32 {
    let start = p.start.wrapping_add(offset);
    if ls.partition_by_start(u64::from(start)).is_some() {
        if offset == 0 {
            ls.increment_partno();
        }
        return 0;
    }
    let par = ls.add_partition(tab, u64::from(start), u64::from(p.size));
    if let Some(pp) = ls.part_mut(par) {
        pp.set_type_string(&p.id);
    }
    1
}

/// `parse_extended(pr, ls, tab, part)`: follow an XGM chain. Each root
/// sector holds a data partition (the first active entry of the first
/// three) and, after it, either an inactive entry (the end) or the next
/// XGM link, relative to the chain's first sector. 1 when the chain ends
/// properly, 0 when it is malformed, `-errno` for a read error.
fn parse_extended(pr: &mut Probe, tab: TabId, xpart: &PartDef) -> i32 {
    let x0start = xpart.start;
    let mut xstart = x0start;
    let mut ct = 0u32;
    loop {
        ct = ct.saturating_add(1);
        if ct > 100 {
            break;
        }
        let Some(xrs) = pr.get_sector(xstart) else {
            if pr.errno != 0 {
                return pr.errno.wrapping_neg();
            }
            return 0;
        };
        // A data partition, then the link to the next XGM or an inactive
        // entry: so the data partition is one of the first three.
        let Some(i) = (0..3).find(|&i| part(&xrs, i).is_active()) else {
            return 0;
        };
        let data = part(&xrs, i);
        if &data.id == b"XGM" {
            return 0;
        }
        let Some(ls) = pr.partlist_mut() else {
            return 0;
        };
        let rc = parse_partition(ls, tab, &data, xstart);
        if rc <= 0 {
            return rc;
        }
        let link = part(&xrs, i.saturating_add(1));
        if !link.is_active() {
            break;
        }
        if &link.id != b"XGM" {
            return 0;
        }
        xstart = x0start.wrapping_add(link.start);
    }
    1
}

/// `probe_atari_pt`.
fn probe_atari_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    // Not defined for other sector sizes.
    if pr.sectorsize() != 512 {
        return PROBE_NONE;
    }
    let size = pr.size() / 512;
    // Nor for large disks.
    if size > u64::from(i32::MAX.unsigned_abs()) {
        return PROBE_NONE;
    }
    let Some(rs) = pr.get_sector(0) else {
        return pr.none_or_err();
    };
    // The disk size recorded in the root sector.
    let rssize = rs.be32(HD_SIZE);
    if rssize < 2 || u64::from(rssize) > size {
        return PROBE_NONE;
    }
    // The bad-sector list, if there is one.
    let bsl_start = rs.be32(BSL_START);
    let bsl_len = rs.be32(BSL_LEN);
    if (bsl_start != 0 || bsl_len != 0) && !is_valid_dimension(bsl_start, bsl_len, rssize) {
        return PROBE_NONE;
    }
    // At least one valid partition; the first is the magic.
    let Some(first) = (0..4).find(|&i| is_valid_partition(&part(&rs, i), rssize)) else {
        return PROBE_NONE;
    };
    let off = PART.saturating_add(first.saturating_mul(PART_DEF_SIZE));
    // flags and id: four bytes.
    let magic = rs.span(off, 4).to_vec();
    if pr.set_magic(u64::try_from(off).unwrap_or(0), &magic) != 0 {
        return -ENOMEM;
    }
    if need_typeonly(pr) {
        return PROBE_OK;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let tab = ls.new_parttable("atari", 0);
    let mut has_xgm = false;
    for i in 0..4 {
        let p = part(&rs, i);
        if !p.is_active() {
            if let Some(ls) = pr.partlist_mut() {
                ls.increment_partno();
            }
            continue;
        }
        let rc = if &p.id == b"XGM" {
            has_xgm = true;
            parse_extended(pr, tab, &p)
        } else {
            match pr.partlist_mut() {
                Some(ls) => parse_partition(ls, tab, &p, 0),
                None => 0,
            }
        };
        if rc < 0 {
            return rc;
        }
    }
    // Without XGM partitions it may be the ICD format -- if the first ICD
    // entry has one of the usual IDs.
    let icd = |i: usize| {
        PartDef::read(
            &rs,
            ICD_PART.saturating_add(i.saturating_mul(PART_DEF_SIZE)),
        )
    };
    if !has_xgm && is_id_common(&icd(0).id) {
        let Some(ls) = pr.partlist_mut() else {
            return PROBE_OK;
        };
        for i in 0..8 {
            let p = icd(i);
            if !p.is_active() || !is_id_common(&p.id) {
                ls.increment_partno();
                continue;
            }
            let rc = parse_partition(ls, tab, &p, 0);
            if rc < 0 {
                return rc;
            }
        }
    }
    PROBE_OK
}

/// `atari_pt_idinfo`: no magic string.
pub static ATARI: IdInfo = IdInfo {
    name: "atari",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_atari_pt),
    magics: &[],
};
