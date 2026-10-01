//! `btrfs.c`: btrfs -- the superblock at 64 KiB (or, on a zoned device, in
//! the superblock log zones), its CRC-32C, XXH64 or SHA-256 verified.

use crate::blkdev;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `BTRFS_SUPER_INFO_SIZE`, and `sizeof(struct btrfs_super_block)`.
const BTRFS_SUPER_INFO_SIZE: u64 = 4096;
/// `SECTOR_SHIFT`.
const SECTOR_SHIFT: u32 = 9;
/// `BLK_ZONE_TYPE_CONVENTIONAL`.
const ZONE_TYPE_CONVENTIONAL: u8 = 1;
/// `BLK_ZONE_COND_EMPTY`.
const ZONE_COND_EMPTY: u8 = 0x1;
/// `BLK_ZONE_COND_FULL`.
const ZONE_COND_FULL: u8 = 0xE;
/// `ENOENT`.
const ENOENT: i32 = 2;
/// `EIO`.
const EIO: i32 = 5;
/// `EUCLEAN`.
const EUCLEAN: i32 = 117;

/// `struct blk_zone`, the parts used.
#[derive(Clone, Copy, Default)]
struct Zone {
    start: u64,
    len: u64,
    wp: u64,
    ty: u8,
    cond: u8,
}

/// `blkdev_get_zonereport(fd, sector, 2)`: the two zones from `sector`.
fn zone_report(pr: &Probe, sector: u64) -> Result<[Zone; 2], i32> {
    let Some(file) = pr.file() else {
        return Err(EIO);
    };
    // struct blk_zone_report (16 bytes) and two struct blk_zone (64 each).
    let mut rep = [0u8; 16 + 128];
    rep[..8].copy_from_slice(&sector.to_ne_bytes());
    rep[8..12].copy_from_slice(&2u32.to_ne_bytes());
    const BLKREPORTZONE: u64 = 0xc010_1282;
    // SAFETY: BLKREPORTZONE reads the 16-byte header and writes at most
    // `nr_zones` (2) zones of 64 bytes after it; the buffer holds both.
    unsafe { blkdev::ioctl_ptr(file, BLKREPORTZONE, &mut rep) }?;
    if u32::from_ne_bytes([rep[8], rep[9], rep[10], rep[11]]) != 2 {
        return Err(EIO);
    }
    let zone = |k: usize| {
        let z = rep.span(k.saturating_mul(64).saturating_add(16), 64);
        Zone {
            start: u64::from_ne_bytes(z.span(0, 8).try_into().unwrap_or([0; 8])),
            len: u64::from_ne_bytes(z.span(8, 8).try_into().unwrap_or([0; 8])),
            wp: u64::from_ne_bytes(z.span(16, 8).try_into().unwrap_or([0; 8])),
            ty: z.u8_at(24),
            cond: z.u8_at(25),
        }
    };
    Ok([zone(0), zone(1)])
}

/// `sb_write_pointer(pr, zones, &wp)`: where the latest superblock of the
/// two log zones is. `Err(-ENOENT)` with the position when neither zone was
/// written.
fn sb_write_pointer(pr: &mut Probe, zones: &[Zone; 2]) -> Result<u64, (i32, u64)> {
    let empty = [
        zones[0].cond == ZONE_COND_EMPTY,
        zones[1].cond == ZONE_COND_EMPTY,
    ];
    let full = [
        zones[0].cond == ZONE_COND_FULL,
        zones[1].cond == ZONE_COND_FULL,
    ];
    let sector = if empty[0] && empty[1] {
        return Err((-ENOENT, zones[0].start << SECTOR_SHIFT));
    } else if full[0] && full[1] {
        // Both full: the newer superblock, by generation.
        let mut generation = [0u64; 2];
        for (g, z) in generation.iter_mut().zip(zones.iter()) {
            let bytenr =
                (z.start.wrapping_add(z.len) << SECTOR_SHIFT).wrapping_sub(BTRFS_SUPER_INFO_SIZE);
            let Some(sb) = pr.get_buffer(bytenr, BTRFS_SUPER_INFO_SIZE) else {
                return Err((-EIO, 0));
            };
            *g = sb.le64(72);
        }
        if generation[0] > generation[1] {
            zones[1].start
        } else {
            zones[0].start
        }
    } else if !full[0] && (empty[1] || full[1]) {
        zones[0].wp
    } else if full[0] {
        zones[1].wp
    } else {
        return Err((-EUCLEAN, 0));
    };
    Ok(sector << SECTOR_SHIFT)
}

/// `sb_log_offset(pr, &bytenr)`: the superblock's place on a zoned device.
fn sb_log_offset(pr: &mut Probe) -> Result<u64, i32> {
    let zones = zone_report(pr, 0).map_err(i32::wrapping_neg)?;
    // The head of a conventional zone, if either is one.
    if let Some(z) = zones.iter().find(|z| z.ty == ZONE_TYPE_CONVENTIONAL) {
        return Ok(z.start << SECTOR_SHIFT);
    }
    let wp = match sb_write_pointer(pr, &zones) {
        Ok(mut wp) => {
            if wp == zones[0].start << SECTOR_SHIFT {
                wp = zones[1].start.wrapping_add(zones[1].len) << SECTOR_SHIFT;
            }
            wp.wrapping_sub(BTRFS_SUPER_INFO_SIZE)
        }
        Err((rc, wp)) if rc == -ENOENT => wp,
        Err(_) => return Err(1),
    };
    Ok(wp)
}

/// `btrfs_verify_csum`.
fn verify_csum(pr: &mut Probe, bfs: &[u8]) -> bool {
    let data = bfs.span(32, 4096 - 32);
    match bfs.le16(196) {
        0 => {
            let crc = !crc32c::crc32c_raw(!0, data);
            pr.verify_csum(u64::from(crc), u64::from(bfs.le32(0)))
        }
        1 => {
            let h = xxhash64::xxh64(data, 0);
            pr.verify_csum(h, bfs.le64(0))
        }
        2 => {
            let h = sha2::sha256(data);
            pr.verify_csum_buf(&h, bfs.span(0, 32))
        }
        // An unknown type is not checked.
        _ => true,
    }
}

/// `probe_btrfs`.
fn probe_btrfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let bfs = if pr.zone_size != 0 {
        let offset = match sb_log_offset(pr) {
            Ok(o) => o,
            Err(rc) => return rc,
        };
        pr.get_buffer(offset, BTRFS_SUPER_INFO_SIZE)
    } else {
        pr.get_sb_buffer(mag, BTRFS_SUPER_INFO_SIZE)
    };
    let Some(bfs) = bfs else {
        return pr.none_or_err();
    };
    if !verify_csum(pr, &bfs) {
        return 1;
    }
    let sectorsize = bfs.le32(144);
    // Without a sector size, total_bytes means nothing.
    if sectorsize == 0 {
        return 1;
    }
    if bfs.u8_at(299) != 0 {
        pr.set_label(bfs.span(299, 256));
    }
    pr.set_uuid(bfs.span(32, 16));
    pr.set_uuid_as(bfs.span(267, 16), Some("UUID_SUB"));
    pr.set_fsblocksize(sectorsize);
    pr.set_block_size(sectorsize);
    // Not 0: checked above.
    let sectorsize_log = sectorsize.checked_ilog2().unwrap_or(0);
    pr.set_fslastblock(bfs.le64(112) >> sectorsize_log);
    // Without the RAID factor, which only the device tree knows.
    pr.set_fssize(bfs.le64(112));
    0
}

/// `btrfs_idinfo`: at 64 KiB, or at the start of either of the first two
/// zones of a zoned device.
pub static BTRFS: IdInfo = IdInfo {
    name: "btrfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 1024 * 1024,
    probefunc: Some(probe_btrfs),
    magics: &[
        IdMag::new(b"_BHRfS_M", 64, 0x40),
        IdMag::zoned(b"_BHRfS_M", 0, 0, 0x40),
        IdMag::zoned(b"_BHRfS_M", 1, 0, 0x40),
    ],
};
