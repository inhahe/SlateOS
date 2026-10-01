//! `luks.c`: LUKS1 and LUKS2 headers, primary or (LUKS2) secondary.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, PROBE_NONE, PROBE_OK, USAGE_CRYPTO};

/// `LUKS_MAGIC`.
const LUKS_MAGIC: &[u8] = b"LUKS\xba\xbe";
/// `LUKS_MAGIC_2`: the secondary header's.
const LUKS_MAGIC_2: &[u8] = b"SKUL\xba\xbe";
/// `LUKS2_HDR2_OFFSETS`: where a LUKS2 secondary header may be.
const SECONDARY_OFFSETS: [u64; 9] = [
    0x04000, 0x008000, 0x010000, 0x020000, 0x40000, 0x080000, 0x100000, 0x200000, 0x400000,
];
/// `sizeof(struct luks2_phdr)`.
const PHDR_SIZE: u64 = 512;
/// `UUID_STRING_L`.
const UUID_STRING_L: usize = 40;
/// `LUKS2_LABEL_L`.
const LUKS2_LABEL_L: usize = 48;

/// `luks_attributes`: VERSION, UUID, and (LUKS2) LABEL and SUBSYSTEM.
fn luks_attributes(pr: &mut Probe, header: &[u8], offset: u64) -> i32 {
    if pr.set_magic(offset, header.span(0, 6)) != 0 {
        return PROBE_NONE;
    }
    let version = header.be16(6);
    pr.sprintf_version(version.to_string());
    // Both versions keep the UUID at 168.
    if version == 1 {
        pr.strncpy_uuid(header.span(168, UUID_STRING_L));
    } else if version == 2 {
        pr.strncpy_uuid(header.span(168, UUID_STRING_L));
        pr.set_label(header.span(24, LUKS2_LABEL_L));
        pr.set_id_label("SUBSYSTEM", header.span(208, LUKS2_LABEL_L));
    }
    PROBE_OK
}

/// `probe_luks`.
fn probe_luks(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(header) = pr.get_buffer(0, PHDR_SIZE) else {
        return pr.none_or_err();
    };
    if header.span(0, 6) == LUKS_MAGIC {
        return luks_attributes(pr, &header, 0);
    }
    // No primary header: look for a LUKS2 secondary one.
    for off in SECONDARY_OFFSETS {
        let Some(header) = pr.get_buffer(off, PHDR_SIZE) else {
            return pr.none_or_err();
        };
        if header.span(0, 6) == LUKS_MAGIC_2 {
            return luks_attributes(pr, &header, off);
        }
    }
    PROBE_NONE
}

/// `luks_idinfo`.
pub static LUKS: IdInfo = IdInfo {
    name: "crypto_LUKS",
    usage: USAGE_CRYPTO,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_luks),
    magics: &[],
};
