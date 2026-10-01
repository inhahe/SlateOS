//! `vfat.c`: FAT12, FAT16 and FAT32 -- the boot sector's BPB checked for
//! sanity, the volume label found in the root directory (walking FAT32's
//! cluster chain for it), and the boot sector's own label kept separately.
//!
//! The DOS-era and FAT32 layouts of the boot sector (`struct
//! msdos_super_block` and `struct vfat_super_block`) are read from the same
//! 512 bytes, at the offsets each struct gives them.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `FAT12_MAX`.
const FAT12_MAX: u32 = 0xFF4;
/// `FAT16_MAX`.
const FAT16_MAX: u32 = 0xFFF4;
/// `FAT32_MAX`.
const FAT32_MAX: u32 = 0x0FFF_FFF6;
/// `FAT_ATTR_VOLUME_ID`.
const FAT_ATTR_VOLUME_ID: u8 = 0x08;
/// `FAT_ATTR_DIR`.
const FAT_ATTR_DIR: u8 = 0x10;
/// `FAT_ATTR_LONG_NAME`.
const FAT_ATTR_LONG_NAME: u8 = 0x0f;
/// `FAT_ATTR_MASK`.
const FAT_ATTR_MASK: u8 = 0x3f;
/// `FAT_ENTRY_FREE`.
const FAT_ENTRY_FREE: u8 = 0xe5;
/// `no_name`.
const NO_NAME: &[u8] = b"NO NAME    ";
/// `sizeof(struct vfat_dir_entry)`.
const DIR_ENTRY: u64 = 32;

/// `is_power_of_2`.
fn is_power_of_2(n: u64) -> bool {
    n.is_power_of_two()
}

/// `search_fat_label(pr, offset, entries, out)`: the volume-label entry of
/// a directory -- read whole, or on a tiny device an entry at a time.
fn search_fat_label(pr: &mut Probe, offset: u64, entries: u32) -> Option<[u8; 11]> {
    let dir = if pr.is_tiny() {
        None
    } else {
        Some(pr.get_buffer(offset, u64::from(entries) * DIR_ENTRY)?)
    };
    for i in 0..entries {
        let ent: Vec<u8> = match &dir {
            Some(d) => d
                .span(
                    usize::try_from(u64::from(i) * DIR_ENTRY).unwrap_or(usize::MAX),
                    32,
                )
                .to_vec(),
            None => match pr.get_buffer(offset.wrapping_add(u64::from(i) * DIR_ENTRY), DIR_ENTRY) {
                Some(b) => b.to_vec(),
                None => break,
            },
        };
        let first = ent.u8_at(0);
        if first == 0 {
            break;
        }
        let attr = ent.u8_at(11);
        if first == FAT_ENTRY_FREE
            || ent.le16(20) != 0
            || ent.le16(26) != 0
            || attr & FAT_ATTR_MASK == FAT_ATTR_LONG_NAME
        {
            continue;
        }
        if attr & (FAT_ATTR_VOLUME_ID | FAT_ATTR_DIR) == FAT_ATTR_VOLUME_ID {
            let mut out = [0u8; 11];
            out.copy_from_slice(ent.span(0, 11));
            if out[0] == 0x05 {
                out[0] = 0xE5;
            }
            return Some(out);
        }
    }
    None
}

/// What `fat_valid_superblock` computes along the way.
struct Geometry {
    cluster_count: u32,
    fat_size: u32,
    sect_count: u32,
}

/// `fat_valid_superblock`: the BPB is plausible, the cluster count fits the
/// FAT type, and it is not BitLocker.
fn fat_valid_superblock(pr: &mut Probe, mag: &IdMag, sb: &[u8]) -> Option<Geometry> {
    // FATs found by the boot jump or the MBR signature alone need more.
    if mag.magic.len() <= 2 {
        // Old floppies carry a valid MBR signature.
        if sb.u8_at(0x1fe) != 0x55 || sb.u8_at(0x1ff) != 0xAA {
            return None;
        }
        // OS/2 and DFSee put a FAT-like header on JFS and HPFS.
        let magic = sb.span(0x36, 8);
        if magic == b"JFS     " || magic == b"HPFS    " {
            return None;
        }
    }
    let fats = u32::from(sb.u8_at(0x10));
    if fats == 0 {
        return None;
    }
    if sb.le16(0x0e) == 0 {
        return None;
    }
    let media = sb.u8_at(0x15);
    if !(media >= 0xf8 || media == 0xf0) {
        return None;
    }
    let cluster_size = u32::from(sb.u8_at(0x0d));
    if !is_power_of_2(u64::from(cluster_size)) {
        return None;
    }
    let sector_size = u32::from(sb.le16(0x0b));
    if !is_power_of_2(u64::from(sector_size)) || !(512..=4096).contains(&sector_size) {
        return None;
    }
    let dir_entries = u64::from(sb.le16(0x11));
    let reserved = u32::from(sb.le16(0x0e));
    let mut sect_count = u32::from(sb.le16(0x13));
    if sect_count == 0 {
        sect_count = sb.le32(0x20);
    }
    let mut fat_length = u32::from(sb.le16(0x16));
    if fat_length == 0 {
        fat_length = sb.le32(0x24);
    }
    let fat_size = fat_length.wrapping_mul(fats);
    // sector_size and cluster_size are powers of two: checked above.
    let dir_size = u32::try_from(
        dir_entries
            .wrapping_mul(DIR_ENTRY)
            .div_ceil(u64::from(sector_size)),
    )
    .unwrap_or(u32::MAX);
    let cluster_count = sect_count
        .wrapping_sub(reserved.wrapping_add(fat_size).wrapping_add(dir_size))
        .checked_div(cluster_size)
        .unwrap_or(0);
    let max_count = if sb.le16(0x16) == 0 && sb.le32(0x24) != 0 {
        FAT32_MAX
    } else if cluster_count > FAT12_MAX {
        FAT16_MAX
    } else {
        FAT12_MAX
    };
    if cluster_count > max_count {
        return None;
    }
    if super::bitlocker::is_bitlocker(pr) {
        return None;
    }
    Some(Geometry {
        cluster_count,
        fat_size,
        sect_count,
    })
}

/// `blkid_probe_is_vfat`: 1 for a valid FAT, 0 for anything else, `-errno`
/// for a read error.
pub(crate) fn is_vfat(pr: &mut Probe) -> i32 {
    let (rc, _, mag) = pr.get_idmag(&VFAT);
    if rc < 0 {
        return rc;
    }
    let Some(mag) = mag else {
        return 0;
    };
    if rc != crate::PROBE_OK {
        return 0;
    }
    let Some(sb) = pr.get_sb_buffer(Some(mag), 512) else {
        return if pr.errno != 0 {
            pr.errno.wrapping_neg()
        } else {
            0
        };
    };
    i32::from(fat_valid_superblock(pr, mag, &sb).is_some())
}

/// `probe_vfat`.
fn probe_vfat(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(mag) = mag else {
        return 1;
    };
    let Some(sb) = pr.get_sb_buffer(Some(mag), 512) else {
        return pr.none_or_err();
    };
    let Some(geo) = fat_valid_superblock(pr, mag, &sb) else {
        return 1;
    };
    let sector_size = u32::from(sb.le16(0x0b));
    let reserved = u32::from(sb.le16(0x0e));
    let cluster_size = u32::from(sb.u8_at(0x0d));
    let mut vol_label: Option<[u8; 11]> = None;
    let mut boot_label: Option<Vec<u8>> = None;
    let mut vol_serno: Option<Vec<u8>> = None;
    let mut version: Option<&[u8]> = None;
    if sb.le16(0x16) != 0 {
        // FAT12/16: the label may be an attribute in the root directory.
        let root_start = reserved
            .wrapping_add(geo.fat_size)
            .wrapping_mul(sector_size);
        let root_dir_entries = u32::from(sb.le16(0x11));
        vol_label = search_fat_label(pr, u64::from(root_start), root_dir_entries);
        let ext = sb.u8_at(0x26);
        if ext == 0x29 {
            boot_label = Some(sb.span(0x2b, 11).to_vec());
        }
        if ext == 0x28 || ext == 0x29 {
            vol_serno = Some(sb.span(0x27, 4).to_vec());
        }
        // Whatever the flags say.
        pr.set_value("SEC_TYPE", b"msdos\0");
        if geo.cluster_count < FAT12_MAX {
            version = Some(b"FAT12");
        } else if geo.cluster_count < FAT16_MAX {
            version = Some(b"FAT16");
        }
    } else if sb.le32(0x24) != 0 {
        // FAT32: follow the root directory's cluster chain for the label.
        let buf_size = cluster_size.wrapping_mul(sector_size);
        let start_data_sect = reserved.wrapping_add(geo.fat_size);
        // A uint64_t quotient in a uint32_t: truncated, as C truncates it.
        let entries = (u64::from(sb.le32(0x24)).wrapping_mul(u64::from(sector_size)) / 4) as u32;
        let mut next = sb.le32(0x2c);
        let mut maxloop = 100u32;
        while next != 0 && next < entries && {
            maxloop = maxloop.wrapping_sub(1);
            maxloop != 0
        } {
            let next_sect_off = next.wrapping_sub(2).wrapping_mul(cluster_size);
            let next_off = u64::from(start_data_sect.wrapping_add(next_sect_off))
                .wrapping_mul(u64::from(sector_size));
            let count = buf_size / 32;
            if let Some(l) = search_fat_label(pr, next_off, count) {
                vol_label = Some(l);
                break;
            }
            let fat_entry_off = u64::from(reserved)
                .wrapping_mul(u64::from(sector_size))
                .wrapping_add(u64::from(next).wrapping_mul(4));
            let Some(buf) = pr.get_buffer(fat_entry_off, u64::from(buf_size)) else {
                break;
            };
            next = buf.le32(0) & 0x0fff_ffff;
        }
        version = Some(b"FAT32");
        if sb.u8_at(0x42) == 0x29 {
            boot_label = Some(sb.span(0x47, 11).to_vec());
        }
        vol_serno = Some(sb.span(0x43, 4).to_vec());
        // The fsinfo block's signatures, or zeros where a volume never set
        // them.
        let fsinfo_sect = u64::from(sb.le16(0x30));
        if fsinfo_sect != 0 {
            let Some(fsinfo) = pr.get_buffer(fsinfo_sect.wrapping_mul(u64::from(sector_size)), 512)
            else {
                return pr.none_or_err();
            };
            let sig1 = fsinfo.span(0, 4);
            if sig1 != b"\x52\x52\x61\x41" && sig1 != b"\x52\x52\x64\x41" && sig1 != [0, 0, 0, 0] {
                return 1;
            }
            let sig2 = fsinfo.span(484, 4);
            if sig2 != b"\x72\x72\x41\x61" && sig2 != [0, 0, 0, 0] {
                return 1;
            }
        }
    }
    if let Some(b) = &boot_label
        && b.as_slice() != NO_NAME
    {
        pr.set_id_label("LABEL_FATBOOT", b);
    }
    if let Some(l) = vol_label {
        pr.set_label(&l);
    }
    if let Some(s) = &vol_serno {
        let text = format!(
            "{:02X}{:02X}-{:02X}{:02X}",
            s.u8_at(3),
            s.u8_at(2),
            s.u8_at(1),
            s.u8_at(0)
        );
        pr.sprintf_uuid(s, text);
    }
    if let Some(v) = version {
        pr.set_version(v);
    }
    pr.set_fsblocksize(cluster_size.wrapping_mul(sector_size));
    pr.set_block_size(sector_size);
    pr.set_fssize(u64::from(sector_size).wrapping_mul(u64::from(geo.sect_count)));
    0
}

/// `vfat_idinfo`.
pub static VFAT: IdInfo = IdInfo {
    name: "vfat",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_vfat),
    magics: &[
        IdMag::new(b"MSWIN", 0, 0x52),
        IdMag::new(b"FAT32   ", 0, 0x52),
        IdMag::new(b"MSDOS", 0, 0x36),
        IdMag::new(b"FAT16   ", 0, 0x36),
        IdMag::new(b"FAT12   ", 0, 0x36),
        IdMag::new(b"FAT     ", 0, 0x36),
        IdMag::new(b"\xeb", 0, 0),
        IdMag::new(b"\xe9", 0, 0),
        IdMag::new(b"\x55\xaa", 0, 0x1fe),
    ],
};
