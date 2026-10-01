//! `isw_raid.c`: Intel Software RAID (IMSM) members, the metadata in the
//! second-to-last sector of a whole disk.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID, c_str};

/// `ISW_SIGNATURE`.
const ISW_SIGNATURE: &[u8] = b"Intel Raid ISM Cfg Sig. ";
/// `sizeof(struct isw_metadata)`.
const ISW_SIZE: u64 = 48;

/// `probe_iswraid`.
fn probe_iswraid(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    if pr.size < 0x10000 {
        return 1;
    }
    if !pr.is_reg() && !pr.is_wholedisk() {
        return 1;
    }
    let sector_size = u64::from(pr.sectorsize());
    // A sector size of 0 would be a division by zero upstream.
    let off = pr
        .size
        .checked_div(sector_size)
        .unwrap_or(0)
        .wrapping_sub(2)
        .wrapping_mul(sector_size);
    let Some(isw) = pr.get_buffer(off, ISW_SIZE) else {
        return pr.none_or_err();
    };
    if isw.span(0, ISW_SIGNATURE.len()) != ISW_SIGNATURE {
        return 1;
    }
    // `"%6s"` of the string after the signature: the version, padded on the
    // left to six. (Upstream's `%s` runs to the first NUL, past the 32-byte
    // field if it has to; here it stops at the end of the 48 bytes read.)
    let v = c_str(isw.span(ISW_SIGNATURE.len(), 48));
    let mut version = vec![b' '; 6usize.saturating_sub(v.len())];
    version.extend_from_slice(v);
    if pr.sprintf_version(&version) != 0 {
        return 1;
    }
    if pr.set_magic(off, isw.span(0, 32)) != 0 {
        return 1;
    }
    0
}

/// `iswraid_idinfo`.
pub static ISWRAID: IdInfo = IdInfo {
    name: "isw_raid_member",
    usage: USAGE_RAID,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_iswraid),
    magics: &[],
};
