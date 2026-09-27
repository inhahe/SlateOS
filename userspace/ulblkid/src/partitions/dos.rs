//! `dos.c`: the MS-DOS (MBR) partition table -- four primary entries, the
//! chain of extended boot records that holds the logical partitions, and
//! the BSD, UnixWare, Solaris and Minix tables that can nest inside a
//! primary partition.
//!
//! 55AA at the end of the first sector is also how a FAT, exFAT or NTFS
//! boot sector ends, so an MBR is believed only once those are ruled out.

use super::mbr::{self, DosPartition, MBR_GPT_PARTITION, MBR_PT_OFFSET};
use super::{
    MBR_DOS_EXTENDED_PARTITION, MBR_LINUX_EXTENDED_PARTITION, MBR_W95_EXTENDED_PARTITION, PartId,
    TabId, do_subprobe, need_typeonly, parttable_set_id, strcpy_ptuuid,
};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::superblocks::{is_exfat, is_ntfs, is_vfat};
use crate::{PROBE_NONE, PROBE_OK};

/// `MBR_FREEBSD_PARTITION`.
const MBR_FREEBSD_PARTITION: i32 = 0xa5;
/// `MBR_OPENBSD_PARTITION`.
const MBR_OPENBSD_PARTITION: i32 = 0xa6;
/// `MBR_NETBSD_PARTITION`.
const MBR_NETBSD_PARTITION: i32 = 0xa9;
/// `MBR_UNIXWARE_PARTITION` (the same byte as GNU Hurd's).
const MBR_UNIXWARE_PARTITION: i32 = 0x63;
/// `MBR_SOLARIS_X86_PARTITION` (the same byte as Linux swap's).
const MBR_SOLARIS_X86_PARTITION: i32 = 0x82;
/// `MBR_MINIX_PARTITION`.
const MBR_MINIX_PARTITION: i32 = 0x81;

/// `dos_nested[]`: the partition types that hold a table of their own, and
/// the prober that reads it.
static DOS_NESTED: [(i32, &IdInfo); 6] = [
    (MBR_FREEBSD_PARTITION, &super::bsd::BSD),
    (MBR_NETBSD_PARTITION, &super::bsd::BSD),
    (MBR_OPENBSD_PARTITION, &super::bsd::BSD),
    (MBR_UNIXWARE_PARTITION, &super::unixware::UNIXWARE),
    (MBR_SOLARIS_X86_PARTITION, &super::solaris_x86::SOLARIS_X86),
    (MBR_MINIX_PARTITION, &super::minix::MINIX),
];

/// `is_extended(p)`: an extended partition, by type.
fn is_extended(p: &DosPartition) -> bool {
    matches!(
        i32::from(p.sys_ind),
        MBR_DOS_EXTENDED_PARTITION | MBR_W95_EXTENDED_PARTITION | MBR_LINUX_EXTENDED_PARTITION
    )
}

/// `parse_dos_extended(pr, tab, ex_start, ex_size, ssf)`: follow the chain
/// of extended boot records from the extended partition at `ex_start`,
/// adding each record's data partition. All arithmetic is upstream's
/// 32-bit sector arithmetic, wrapping as it wraps. A chain that loops, or
/// runs past 100 records without a data partition, ends quietly.
fn parse_dos_extended(pr: &mut Probe, tab: TabId, ex_start: u32, ex_size: u32, ssf: u32) -> i32 {
    if ex_start == 0 {
        // A bad offset in the primary extended partition.
        return 0;
    }
    let mut cur_start = ex_start;
    let mut cur_size = ex_size;
    let mut ct_nodata = 0u32;
    loop {
        ct_nodata = ct_nodata.saturating_add(1);
        if ct_nodata > 100 {
            return PROBE_OK;
        }
        let Some(data) = pr.get_sector(cur_start) else {
            if pr.errno != 0 {
                return pr.errno.wrapping_neg();
            }
            // A malformed partition.
            return PROBE_OK;
        };
        if !mbr::is_valid_magic(&data) {
            return PROBE_OK;
        }
        let entries = mbr::partitions(&data);
        // Usually the first entry is the data partition and the second the
        // link to the next record; DR-DOS sometimes puts the link first, and
        // OS/2 uses all four.
        for (i, p) in entries.iter().enumerate() {
            // The start is relative to the record's own extended partition.
            let start = p.start.wrapping_mul(ssf);
            let size = p.size.wrapping_mul(ssf);
            let abs_start = cur_start.wrapping_add(start);
            if size == 0 || is_extended(p) {
                continue;
            }
            if i >= 2 {
                // The third and fourth entries hold data only if it fits.
                if start.wrapping_add(size) > cur_size
                    || abs_start < ex_start
                    || abs_start.wrapping_add(size) > ex_start.wrapping_add(ex_size)
                {
                    continue;
                }
            }
            let Some(ls) = pr.partlist_mut() else {
                return PROBE_OK;
            };
            // A link back to a record already read: skip its partition, and
            // let `ct_nodata` end the loop.
            if ls.partition_by_start(u64::from(abs_start)).is_some() {
                continue;
            }
            let par = ls.add_partition(tab, u64::from(abs_start), u64::from(size));
            if let Some(pp) = ls.part_mut(par) {
                pp.set_type(i32::from(p.sys_ind));
                pp.set_flags(u64::from(p.boot_ind));
            }
            ls.gen_uuid(par);
            ct_nodata = 0;
        }
        // The first extended entry with a non-zero start links to the next
        // record; anything more is junk.
        let Some((start, size)) = entries.iter().find_map(|p| {
            let start = p.start.wrapping_mul(ssf);
            let size = p.size.wrapping_mul(ssf);
            (size != 0 && is_extended(p) && start != 0).then_some((start, size))
        }) else {
            return PROBE_OK;
        };
        cur_start = ex_start.wrapping_add(start);
        cur_size = size;
    }
}

/// `is_lvm(pr)`: an LVM physical volume was found on the device.
fn is_lvm(pr: &Probe) -> bool {
    pr.lookup_value("TYPE")
        .is_some_and(|v| v.as_c_str() == b"LVM2_member")
}

/// `is_empty_mbr(mbr)`: no entry has a size.
fn is_empty_mbr(entries: &[DosPartition; 4]) -> bool {
    entries.iter().all(|p| p.size == 0)
}

/// `probe_dos_pt`.
fn probe_dos_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(data) = pr.get_sector(0) else {
        return pr.none_or_err();
    };
    // An AIX disk (see aix.rs).
    if data.starts_with(super::aix::AIX_MAGIC) {
        return PROBE_NONE;
    }
    let entries = mbr::partitions(&data);
    // A boot indicator other than 0 or 0x80 is not a partition table.
    if entries
        .iter()
        .any(|p| p.boot_ind != 0 && p.boot_ind != 0x80)
    {
        return PROBE_NONE;
    }
    // GPT keeps a valid MBR in front of it: that is GPT's to report.
    if entries.iter().any(|p| p.sys_ind == MBR_GPT_PARTITION) {
        return PROBE_NONE;
    }
    // With 55AA present this is either a FAT boot sector or a partition
    // table -- and NTFS is another false positive.
    if is_vfat(pr) == 1 || is_exfat(pr) == 1 {
        return PROBE_NONE;
    }
    if is_ntfs(pr) == 1 {
        return PROBE_NONE;
    }
    // An LVM physical volume with an empty MBR in front of it is LVM (people
    // boot from those).
    if is_lvm(pr) && is_empty_mbr(&entries) {
        return PROBE_NONE;
    }
    pr.use_wiper(MBR_PT_OFFSET, 512 - MBR_PT_OFFSET);
    let id = mbr::get_id(&data);
    let idstr = format!("{id:08x}");
    if need_typeonly(pr) {
        // Values only: the type, and the disk ID as PTUUID.
        if id != 0 {
            strcpy_ptuuid(pr, idstr.as_bytes());
        }
        return PROBE_OK;
    }
    // Starts and sizes are in the device's sectors; the list keeps 512-byte
    // ones.
    let ssf = pr.sectorsize() / 512;
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let tab = ls.new_parttable("dos", MBR_PT_OFFSET);
    if id != 0
        && let Some(t) = ls.tab_mut(tab)
    {
        parttable_set_id(t, idstr.as_bytes());
    }
    // The primaries. An empty one still uses up its number, as Linux's do.
    for p in &entries {
        let start = p.start.wrapping_mul(ssf);
        let size = p.size.wrapping_mul(ssf);
        if size == 0 {
            ls.increment_partno();
            continue;
        }
        let par = ls.add_partition(tab, u64::from(start), u64::from(size));
        if let Some(pp) = ls.part_mut(par) {
            pp.set_type(i32::from(p.sys_ind));
            pp.set_flags(u64::from(p.boot_ind));
        }
        ls.gen_uuid(par);
    }
    // Linux numbers every logical partition and nested table from 5.
    ls.set_partno(5);
    // The logicals.
    for p in &entries {
        let start = p.start.wrapping_mul(ssf);
        let size = p.size.wrapping_mul(ssf);
        if size == 0 {
            continue;
        }
        // Upstream compares with -1, so only EPERM from a read counts: any
        // other read error leaves the table as far as it was read.
        if is_extended(p) && parse_dos_extended(pr, tab, start, size, ssf) == -1 {
            return PROBE_NONE;
        }
    }
    // Tables nested in primaries, on a disk larger than a floppy. Only the
    // partitions found so far are looked in.
    if !pr.is_tiny() {
        let nparts = pr.partlist().map_or(0, super::Partlist::numof_partitions);
        for i in 0..nparts {
            let Some(ty) = pr.partlist().and_then(|ls| {
                let pa = ls.partition(i)?;
                (pa.size != 0 && !ls.is_extended(pa) && !ls.is_logical(pa)).then_some(pa.ty)
            }) else {
                continue;
            };
            if let Some(&(_, id)) = DOS_NESTED.iter().find(|(t, _)| *t == ty) {
                let rc = do_subprobe(pr, PartId(i), id);
                if rc < 0 {
                    return rc;
                }
            }
        }
    }
    PROBE_OK
}

/// `dos_pt_idinfo`: the master boot sector -- code, the optional disk
/// signature at 440, the table at 446, 55AA at 510.
pub static DOS: IdInfo = IdInfo {
    name: "dos",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_dos_pt),
    magics: &[IdMag::new(b"\x55\xAA", 0, 510)],
};
