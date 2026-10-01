//! `drbd.c`: DRBD 8.4 and 9 metadata at the end of the device.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `DRBD_MAGIC`.
const DRBD_MAGIC: u32 = 0x8374_0267;
/// `DRBD_MD_MAGIC_08`.
const DRBD_MD_MAGIC_08: u32 = DRBD_MAGIC + 4;
/// `DRBD_MD_MAGIC_84_UNCLEAN`.
const DRBD_MD_MAGIC_84_UNCLEAN: u32 = DRBD_MAGIC + 5;
/// `DRBD_MD_MAGIC_09`.
const DRBD_MD_MAGIC_09: u32 = DRBD_MAGIC + 6;
/// `DRBD_MD_OFFSET`: the metadata's distance from the end.
const DRBD_MD_OFFSET: u64 = 4096;
/// `sizeof(struct md_on_disk_08)`.
const MD08_SIZE: u64 = 104;
/// `sizeof(struct meta_data_on_disk_9)`.
const MD09_SIZE: u64 = 1392;
/// `offsetof(..., magic)`, the same in both.
const MAGIC_OFF: usize = 60;

/// Both versions: the metadata of `size` bytes, its magic one of `magics`,
/// its 64-bit "UUID" at `uuid_off`.
fn probe_version(
    pr: &mut Probe,
    size: u64,
    magics: &[u32],
    uuid_off: usize,
    version: &[u8],
) -> i32 {
    let off = pr.size.wrapping_sub(DRBD_MD_OFFSET);
    // Small devices cannot be DRBD.
    if pr.size < 0x10000 {
        return 1;
    }
    let Some(md) = pr.get_buffer(off, size) else {
        return pr.none_or_err();
    };
    if !magics.contains(&md.be32(MAGIC_OFF)) {
        return 1;
    }
    // DRBD's UUIDs are 64 bits, printed in hex.
    let raw = md.span(uuid_off, 8);
    pr.sprintf_uuid(raw, format!("{:x}", md.be64(uuid_off)));
    pr.set_version(version);
    if pr.set_magic(off.wrapping_add(MAGIC_OFF as u64), md.span(MAGIC_OFF, 4)) != 0 {
        return 1;
    }
    0
}

/// `probe_drbd`: 8.4 first; then 9.
fn probe_drbd(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let ret = probe_version(
        pr,
        MD08_SIZE,
        &[DRBD_MD_MAGIC_08, DRBD_MD_MAGIC_84_UNCLEAN],
        40,
        b"v08",
    );
    if ret <= 0 {
        return ret;
    }
    probe_version(pr, MD09_SIZE, &[DRBD_MD_MAGIC_09], 48, b"v09")
}

/// `drbd_idinfo`.
pub static DRBD: IdInfo = IdInfo {
    name: "drbd",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_drbd),
    magics: &[],
};
