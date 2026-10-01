//! `drbdproxy_datalog.c`: DRBD Proxy data logs.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `probe_drbdproxy_datalog`.
fn probe_drbdproxy_datalog(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    // struct log_header_t: magic, version, uuid[16], flags.
    let Some(lh) = pr.get_buffer(0, 40) else {
        return pr.none_or_err();
    };
    pr.set_uuid(lh.span(16, 16));
    pr.sprintf_version(format!("v{}", lh.le64(8)));
    0
}

/// `drbdproxy_datalog_idinfo`.
pub static DRBDPROXY_DATALOG: IdInfo = IdInfo {
    name: "drbdproxy_datalog",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 16 * 1024,
    probefunc: Some(probe_drbdproxy_datalog),
    magics: &[IdMag::new(b"DRBDdlh*", 0, 0)],
};
