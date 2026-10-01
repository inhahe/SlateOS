//! `zfs.c`: ZFS pool members -- four uberblocks among the four vdev labels,
//! then the pool's name and GUIDs from the first label's nvlist.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Buf, Bytes, Endianness, USAGE_FILESYSTEM};

/// `VDEV_LABEL_UBERBLOCK`.
const VDEV_LABEL_UBERBLOCK: u64 = 128 * 1024;
/// `VDEV_LABEL_NVPAIR`.
const VDEV_LABEL_NVPAIR: u64 = 16 * 1024;
/// `VDEV_LABEL_SIZE`.
const VDEV_LABEL_SIZE: u64 = 256 * 1024;
/// `UBERBLOCK_SIZE`.
const UBERBLOCK_SIZE: u64 = 1024;
/// `UBERBLOCKS_COUNT`.
const UBERBLOCKS_COUNT: u64 = 128;
/// `UBERBLOCK_MAGIC`.
const UBERBLOCK_MAGIC: u64 = 0x00ba_b10c;
/// `ZFS_WANT`: uberblocks needed for a match.
const ZFS_WANT: u32 = 4;
/// `DATA_TYPE_UINT64`.
const DATA_TYPE_UINT64: u32 = 8;
/// `DATA_TYPE_STRING`.
const DATA_TYPE_STRING: u32 = 9;
/// `DATA_TYPE_DIRECTORY`.
const DATA_TYPE_DIRECTORY: u32 = 19;
/// `sizeof(struct nvpair)`.
const NVPAIR: u64 = 12;

/// `strncmp(name, literal, n) == 0`, the name read from the nvlist.
fn strncmp_eq(name: &[u8], literal: &[u8], n: usize) -> bool {
    for i in 0..n {
        let a = name.u8_at(i);
        let b = literal.get(i).copied().unwrap_or(0);
        if a != b {
            return false;
        }
        if a == 0 {
            return true;
        }
    }
    true
}

/// `zfs_process_value`: the pair's value, if it is one wanted.
fn process_value(
    pr: &mut Probe,
    name: &[u8],
    namelen: usize,
    value: &[u8],
    max_value_size: u64,
    level: u32,
) {
    if strncmp_eq(name, b"name", namelen) && 12 <= max_value_size && level == 0 {
        let ty = value.be32(0);
        let strlen = value.be32(8);
        if ty != DATA_TYPE_STRING || u64::from(strlen) + 12 > max_value_size {
            return;
        }
        pr.set_label(value.span(12, strlen as usize));
    } else if strncmp_eq(name, b"guid", namelen) && 16 <= max_value_size && level == 0 {
        if value.be32(0) != DATA_TYPE_UINT64 {
            return;
        }
        pr.set_value_str("UUID_SUB", value.be64(8).to_string().as_bytes());
    } else if strncmp_eq(name, b"pool_guid", namelen) && 16 <= max_value_size && level == 0 {
        if value.be32(0) != DATA_TYPE_UINT64 {
            return;
        }
        let v = value.be64(8);
        pr.sprintf_uuid(&v.to_ne_bytes(), v.to_string());
    } else if strncmp_eq(name, b"ashift", namelen) && 16 <= max_value_size {
        if value.be32(0) != DATA_TYPE_UINT64 {
            return;
        }
        let v = value.be64(8);
        if v < 32 {
            pr.set_fsblocksize(1u32 << v);
            pr.set_block_size(1u32 << v);
        }
    }
}

/// `zfs_extract_guid_name(pr, offset)`: walk the label's nvlist -- the
/// first 4 KiB of it, which is where the wanted pairs have always been.
fn extract_guid_name(pr: &mut Probe, offset: u64) {
    let offset = (offset & !(VDEV_LABEL_SIZE - 1)).wrapping_add(VDEV_LABEL_NVPAIR);
    let Some(p) = pr.get_buffer(offset, 4096) else {
        return;
    };
    let mut at: usize = 12;
    let mut left: u64 = 4096 - 12;
    let mut level: u32 = 0;
    while left > NVPAIR {
        let nvp = p.get(at..).unwrap_or_default();
        let mut nvp_size = u64::from(nvp.be32(0));
        let namelen = nvp.be32(8);
        let namesize = (u64::from(namelen) + 3) & !3;
        if nvp.span(0, 4) == [0, 0, 0, 0] {
            if level == 0 {
                break;
            }
            level = level.wrapping_sub(1);
            nvp_size = 8;
        } else {
            if nvp_size > left || namesize.wrapping_add(NVPAIR) > nvp_size {
                break;
            }
            let max_value_size = nvp_size.wrapping_sub(namesize.wrapping_add(NVPAIR));
            let value = nvp
                .get(usize::try_from(namesize.wrapping_add(NVPAIR)).unwrap_or(usize::MAX)..)
                .unwrap_or_default();
            if 16 <= max_value_size && value.be32(0) == DATA_TYPE_DIRECTORY {
                nvp_size = namesize.wrapping_add(NVPAIR).wrapping_add(16);
                level = level.wrapping_add(1);
            } else {
                let name = nvp.get(12..).unwrap_or_default();
                process_value(pr, name, namelen as usize, value, max_value_size, level);
            }
        }
        if nvp_size > left {
            break;
        }
        left = left.wrapping_sub(nvp_size);
        at = at.saturating_add(usize::try_from(nvp_size).unwrap_or(usize::MAX));
    }
}

/// `find_uberblocks(label, &ub_offset, &swap_endian)`: how many of the
/// label's 128 slots hold an uberblock; the last one's offset and
/// byte order.
fn find_uberblocks(label: &[u8]) -> (u32, u64, bool) {
    let swab_magic = UBERBLOCK_MAGIC.swap_bytes();
    let mut found = 0u32;
    let (mut ub_offset, mut swap) = (0u64, false);
    for i in 0..UBERBLOCKS_COUNT {
        let off = i
            .wrapping_mul(UBERBLOCK_SIZE)
            .wrapping_add(VDEV_LABEL_UBERBLOCK);
        let magic = label.le64(usize::try_from(off).unwrap_or(usize::MAX));
        if magic == UBERBLOCK_MAGIC {
            ub_offset = off;
            swap = false;
            found = found.saturating_add(1);
        }
        if magic == swab_magic {
            ub_offset = off;
            swap = true;
            found = found.saturating_add(1);
        }
    }
    (found, ub_offset, swap)
}

/// `probe_zfs`.
fn probe_zfs(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let blk_align = pr.size % VDEV_LABEL_SIZE;
    let mut found = 0u32;
    let mut offset = 0u64;
    let mut ub: Option<(Buf, u64)> = None;
    let mut ub_offset = 0u64;
    let mut swab = false;
    for label_no in 0..4 {
        offset = match label_no {
            0 => 0,
            1 => VDEV_LABEL_SIZE,
            2 => pr
                .size
                .wrapping_sub(2 * VDEV_LABEL_SIZE)
                .wrapping_sub(blk_align),
            _ => pr
                .size
                .wrapping_sub(VDEV_LABEL_SIZE)
                .wrapping_sub(blk_align),
        };
        // On a whole disk, a label inside a partition is the partition's.
        if (pr.is_reg() || pr.is_wholedisk())
            && crate::partitions::is_covered_by_pt(pr, offset, VDEV_LABEL_SIZE)
        {
            continue;
        }
        let Some(label) = pr.get_buffer(offset, VDEV_LABEL_SIZE) else {
            return pr.none_or_err();
        };
        let (n, off, s) = find_uberblocks(&label);
        if n > 0 {
            found = found.saturating_add(n);
            swab = s;
            ub = Some((label, off));
            ub_offset = off.wrapping_add(offset);
            if found >= ZFS_WANT {
                break;
            }
        }
    }
    if found < ZFS_WANT {
        return 1;
    }
    let Some((label, at)) = ub else {
        return 1;
    };
    let at = usize::try_from(at).unwrap_or(usize::MAX);
    let version = label.le64(at.saturating_add(8));
    pr.sprintf_version(if swab { version.swap_bytes() } else { version }.to_string());
    extract_guid_name(pr, offset);
    if pr.set_magic(ub_offset, label.span(at, 8)) != 0 {
        return 1;
    }
    pr.set_fsendianness(if swab {
        Endianness::Big
    } else {
        Endianness::Little
    });
    0
}

/// `zfs_idinfo`.
pub static ZFS: IdInfo = IdInfo {
    name: "zfs_member",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 64 * 1024 * 1024,
    probefunc: Some(probe_zfs),
    magics: &[],
};
