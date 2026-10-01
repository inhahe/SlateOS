//! `netware.c`: Novell NSS pools.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `probe_netware`.
fn probe_netware(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct netware_super_block: 636 bytes, SBH_PoolID at 348.
    let Some(nw) = pr.get_sb_buffer(mag, 636) else {
        return pr.none_or_err();
    };
    pr.set_uuid(nw.span(348, 16));
    pr.sprintf_version(format!("{}.{:02}", nw.le16(8), nw.le16(10)));
    0
}

/// `netware_idinfo`.
pub static NETWARE: IdInfo = IdInfo {
    name: "nss",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_netware),
    magics: &[IdMag::new(b"SPB5", 4, 0)],
};
