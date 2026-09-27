//! `drbdmanage.c`: the drbdmanage control volume.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_OTHER};

/// `persistence_magic`.
const PERSISTENCE_MAGIC: [u8; 4] = [0x1a, 0xdb, 0x98, 0xa2];

/// `probe_drbdmanage`: a hex UUID ended by a newline; a version if the
/// persistence area says so.
fn probe_drbdmanage(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let err_or_none = |pr: &Probe| pr.none_or_err();
    // struct drbdmanage_hdr: magic[11], uuid[32], lf.
    let Some(hdr) = pr.get_buffer(0, 44) else {
        return err_or_none(pr);
    };
    let uuid = hdr.span(11, 32);
    if !uuid.iter().all(u8::is_ascii_hexdigit) {
        return 1;
    }
    if hdr.u8_at(43) != b'\n' {
        return 1;
    }
    if pr.strncpy_uuid(uuid) != 0 {
        return err_or_none(pr);
    }
    // struct drbdmanage_pers: magic[4], version_le.
    let Some(prs) = pr.get_buffer(0x1000, 8) else {
        return err_or_none(pr);
    };
    if prs.span(0, 4) == PERSISTENCE_MAGIC {
        // `"%d"` of the big-endian value, as upstream reads it despite the
        // field's name.
        let v = i32::from_ne_bytes(prs.be32(4).to_ne_bytes());
        if pr.sprintf_version(v.to_string()) != 0 {
            return err_or_none(pr);
        }
    }
    0
}

/// `drbdmanage_idinfo`.
pub static DRBDMANAGE: IdInfo = IdInfo {
    name: "drbdmanage_control_volume",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 64 * 1024,
    probefunc: Some(probe_drbdmanage),
    magics: &[IdMag::new(b"$DRBDmgr=q", 0, 0)],
};
