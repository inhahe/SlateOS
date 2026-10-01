//! `bsd.c`: the BSD disklabel -- on its own, or nested in a DOS partition
//! of type FreeBSD, NetBSD or OpenBSD (the table is then named after the
//! parent's type, and its partitions must lie inside the parent).
//!
//! Only the binary interface reads it: for values the prober reports
//! nothing, as upstream's does.

use super::{Partition, is_nested_dimension, need_typeonly};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK};

/// `BSD_MAXPARTITIONS`.
const BSD_MAXPARTITIONS: u16 = 16;
/// `BSD_FS_UNUSED`.
const BSD_FS_UNUSED: u8 = 0;
/// `sizeof(struct bsd_disklabel)`, with its sixteen partitions.
const BSD_DISKLABEL_SIZE: usize = 404;
/// `offsetof(struct bsd_disklabel, d_checksum)`.
const D_CHECKSUM: usize = 136;
/// `offsetof(struct bsd_disklabel, d_npartitions)`.
const D_NPARTITIONS: usize = 138;
/// `offsetof(struct bsd_disklabel, d_partitions)`.
const D_PARTITIONS: usize = 148;
/// `sizeof(struct bsd_partition)`.
const BSD_PARTITION_SIZE: usize = 16;

/// `MBR_FREEBSD_PARTITION`.
const MBR_FREEBSD_PARTITION: i32 = 0xa5;
/// `MBR_OPENBSD_PARTITION`.
const MBR_OPENBSD_PARTITION: i32 = 0xa6;
/// `MBR_NETBSD_PARTITION`.
const MBR_NETBSD_PARTITION: i32 = 0xa9;

/// `BLKID_MAG_SECTOR(mag)`: the 512-byte sector the magic is in.
fn mag_sector(mag: &IdMag) -> u32 {
    let kb_sectors = u32::try_from(mag.kboff.checked_div(2).unwrap_or(0)).unwrap_or(0);
    kb_sectors.wrapping_add(mag.sboff >> 9)
}

/// `BLKID_MAG_OFFSET(mag)`: the magic's offset in bytes.
fn mag_offset(mag: &IdMag) -> u64 {
    u64::try_from(mag.kboff)
        .unwrap_or(0)
        .wrapping_shl(10)
        .wrapping_add(u64::from(mag.sboff))
}

/// `bsd_checksum(l)`: the XOR of the label's 16-bit words, its checksum
/// field XORed back out.
fn bsd_checksum(l: &[u8]) -> u16 {
    let mut csum = 0u16;
    for k in (0..BSD_DISKLABEL_SIZE).step_by(2) {
        csum ^= l.le16(k);
    }
    csum ^ l.le16(D_CHECKSUM)
}

/// `probe_bsd_pt`.
fn probe_bsd_pt(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    if need_typeonly(pr) {
        return PROBE_NONE;
    }
    let Some(mag) = mag else {
        return PROBE_NONE;
    };
    let sector = mag_sector(mag);
    let Some(data) = pr.get_sector(sector) else {
        return pr.none_or_err();
    };
    // The label is where the magic was, within its sector -- and at byte
    // 128 it runs past the sector's end, into the rest of the buffer.
    let lastoff = mag_offset(mag).wrapping_sub(u64::from(sector) << 9);
    let lastoff = usize::try_from(lastoff).unwrap_or(usize::MAX);
    let label = data
        .through_end()
        .span(lastoff, BSD_DISKLABEL_SIZE)
        .to_vec();
    if !pr.verify_csum(
        u64::from(bsd_checksum(&label)),
        u64::from(label.le16(D_CHECKSUM)),
    ) {
        return PROBE_NONE;
    }
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    // The BSD flavour, from the DOS partition the label is nested in.
    let parent: Option<Partition> = ls.parent().and_then(|id| ls.partition(id.0).cloned());
    let mut name = "bsd";
    let mut abs_offset = 0u32;
    if let Some(par) = &parent {
        match par.ty {
            MBR_FREEBSD_PARTITION => {
                name = "freebsd";
                // A 64-bit start in a 32-bit variable, truncated as C does.
                abs_offset = u32::try_from(par.start & u64::from(u32::MAX)).unwrap_or(0);
            }
            MBR_NETBSD_PARTITION => name = "netbsd",
            MBR_OPENBSD_PARTITION => name = "openbsd",
            // A BSD label in a partition of another type: upstream only
            // warns.
            _ => {}
        }
    }
    let tab = ls.new_parttable(name, mag_offset(mag));
    let npartitions = label.le16(D_NPARTITIONS);
    // More than sixteen are ignored.
    let nparts = usize::from(npartitions.min(BSD_MAXPARTITIONS));
    // FreeBSD 10 and later use offsets relative to the slice, which shows as
    // the whole-disk partition 'c' starting at 0.
    let relative = abs_offset != 0
        && nparts >= 3
        && label.le32(
            D_PARTITIONS
                .saturating_add(2 * BSD_PARTITION_SIZE)
                .saturating_add(4),
        ) == 0;
    for i in 0..nparts {
        let p = label.span(
            D_PARTITIONS.saturating_add(i.saturating_mul(BSD_PARTITION_SIZE)),
            BSD_PARTITION_SIZE,
        );
        let fstype = p.u8_at(12);
        if fstype == BSD_FS_UNUSED {
            continue;
        }
        let mut start = p.le32(4);
        let size = p.le32(0);
        if relative {
            start = start.wrapping_add(abs_offset);
        }
        if let Some(par) = &parent {
            // The same as its parent, or reaching outside it: ignored.
            if par.start == u64::from(start) && par.size == u64::from(size) {
                continue;
            }
            if !is_nested_dimension(par, u64::from(start), u64::from(size)) {
                continue;
            }
        }
        let id = ls.add_partition(tab, u64::from(start), u64::from(size));
        if let Some(pp) = ls.part_mut(id) {
            pp.set_type(i32::from(fstype));
        }
    }
    PROBE_OK
}

/// `bsd_pt_idinfo`: the label is in sector 1 on most machines; at byte 64
/// on Alpha, PowerPC, IA-64 and HPPA; at 128 on some others.
pub static BSD: IdInfo = IdInfo {
    name: "bsd",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_bsd_pt),
    magics: &[
        IdMag::new(b"\x57\x45\x56\x82", 0, 512),
        IdMag::new(b"\x57\x45\x56\x82", 0, 64),
        IdMag::new(b"\x57\x45\x56\x82", 0, 128),
    ],
};
