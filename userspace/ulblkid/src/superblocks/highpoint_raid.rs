//! `highpoint_raid.c`: HighPoint RocketRAID 45x members (metadata eleven
//! sectors from the end) and 37x members (a magic at 4640 bytes).

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `HPT45X_MAGIC_OK`.
const HPT45X_MAGIC_OK: u32 = 0x5a78_16f3;
/// `HPT45X_MAGIC_BAD`.
const HPT45X_MAGIC_BAD: u32 = 0x5a78_16fd;

/// `probe_highpoint45x`.
fn probe_highpoint45x(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let off = (pr.size / 0x200).wrapping_sub(11).wrapping_mul(0x200);
    let Some(hpt) = pr.get_buffer(off, 4) else {
        return pr.none_or_err();
    };
    let magic = hpt.le32(0);
    if magic != HPT45X_MAGIC_OK && magic != HPT45X_MAGIC_BAD {
        return 1;
    }
    if pr.set_magic(off, hpt.span(0, 4)) != 0 {
        return 1;
    }
    0
}

/// `probe_highpoint37x`: the magic is the whole test, on a whole disk.
fn probe_highpoint37x(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    0
}

/// `highpoint45x_idinfo`.
pub static HIGHPOINT45X: IdInfo = IdInfo {
    name: "hpt45x_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_highpoint45x),
    magics: &[],
};

/// `highpoint37x_idinfo`: the superblock at 4608 bytes, its magic 32 bytes
/// in.
pub static HIGHPOINT37X: IdInfo = IdInfo {
    name: "hpt37x_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_highpoint37x),
    magics: &[
        IdMag::new(b"\xf0\x16\x78\x5a", 4, 544),
        IdMag::new(b"\xfd\x16\x78\x5a", 4, 544),
    ],
};
