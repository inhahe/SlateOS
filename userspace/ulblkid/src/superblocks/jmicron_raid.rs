//! `jmicron_raid.c`: JMicron RAID members, metadata in the last sector.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `sizeof(struct jm_metadata)`: the unpacked 12-byte `segment` included.
const JM_SIZE: u64 = 128;

/// `probe_jmraid`.
fn probe_jmraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let off = (pr.size / 0x200).wrapping_sub(1).wrapping_mul(0x200);
    let Some(jm) = pr.get_buffer(off, JM_SIZE) else {
        return pr.none_or_err();
    };
    if jm.span(0, 2) != b"JM" {
        return 1;
    }
    // `jm_checksum`: the 16-bit words sum to 0 or 1.
    let sum = (0..64usize).fold(0u16, |s, k| s.wrapping_add(jm.le16(k.saturating_mul(2))));
    if !pr.verify_csum(u64::from(sum == 0 || sum == 1), 1) {
        return 1;
    }
    if jm.u8_at(0x30) > 5 {
        return 1;
    }
    let version = jm.le16(2);
    if pr.sprintf_version(format!("{}.{}", version >> 8, version & 0xFF)) != 0 {
        return 1;
    }
    if pr.set_magic(off, jm.span(0, 2)) != 0 {
        return 1;
    }
    0
}

/// `jmraid_idinfo`.
pub static JMRAID: IdInfo = IdInfo {
    name: "jmicron_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_jmraid),
    magics: &[],
};
