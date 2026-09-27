//! `cs_fvault2.c`: Apple Core Storage physical volumes used for FileVault 2.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_CRYPTO};

/// `sizeof(struct cs_fvault2_sb)`.
const SB_SIZE: u64 = 512;

/// `probe_cs_fvault2`.
fn probe_cs_fvault2(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(sb) = pr.get_sb_buffer(mag, SB_SIZE) else {
        return pr.none_or_err();
    };
    // Only the type-1 checksum is supported.
    if sb.le16(8) != 1 || sb.le32(90) != 1 {
        return PROBE_NONE;
    }
    // `cs_fvault2_verify_csum`: CRC-32C from the stored seed over all but the
    // checksum itself, with no inversion.
    let crc = crc32c::crc32c_raw(sb.le32(4), sb.span(8, 504));
    if !pr.verify_csum(u64::from(crc), u64::from(sb.le32(0))) {
        return PROBE_NONE;
    }
    // Block type 0x10, a 16-byte key, AES-XTS.
    if sb.le16(10) != 0x10 || sb.le32(168) != 16 || sb.le32(172) != 2 {
        return PROBE_NONE;
    }
    pr.sprintf_version(sb.le16(8).to_string());
    pr.set_uuid(sb.span(304, 16));
    PROBE_OK
}

/// `cs_fvault2_idinfo`.
pub static CS_FVAULT2: IdInfo = IdInfo {
    name: "cs_fvault2",
    usage: USAGE_CRYPTO,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_cs_fvault2),
    magics: &[IdMag::new(b"CS", 0, 88)],
};
