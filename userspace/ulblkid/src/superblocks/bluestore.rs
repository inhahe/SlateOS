//! `bluestore.c`: Ceph BlueStore block devices.

use crate::USAGE_OTHER;
use crate::probe::{IdInfo, IdMag, Probe};

/// `probe_bluestore`: the magic is the test; the header must be readable.
fn probe_bluestore(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    if pr.get_sb_buffer(mag, 22).is_none() {
        return pr.none_or_err();
    }
    0
}

/// `bluestore_idinfo`.
pub static BLUESTORE: IdInfo = IdInfo {
    name: "ceph_bluestore",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_bluestore),
    magics: &[IdMag::new(b"bluestore block device", 0, 0)],
};
