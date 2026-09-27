//! `hpfs.c`: OS/2 HPFS -- the superblock at 8 KiB, the spare superblock
//! after it, and the label and serial in the boot block.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_FILESYSTEM};

/// `HPFS_SBSPARE_OFFSET`.
const HPFS_SBSPARE_OFFSET: u64 = 0x2200;

/// `probe_hpfs`.
fn probe_hpfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let err = |pr: &Probe| pr.none_or_err();
    // struct hpfs_super_block: magic[4], magic1[4], version.
    let Some(hs) = pr.get_sb_buffer(mag, 9) else {
        return err(pr);
    };
    let version = hs.u8_at(8);
    let Some(hss) = pr.get_buffer(HPFS_SBSPARE_OFFSET, 8) else {
        return err(pr);
    };
    if hss.span(0, 4) != b"\x49\x18\x91\xf9" {
        return 1;
    }
    // The boot block, with the label and serial number.
    let Some(hbb) = pr.get_buffer(0, 512) else {
        return err(pr);
    };
    if hbb.span(510, 2) == b"\x55\xaa" && hbb.span(54, 4) == b"HPFS" && hbb.u8_at(38) == 0x28 {
        pr.set_label(hbb.span(43, 11));
        let s = hbb.span(39, 4);
        let text = format!(
            "{:02X}{:02X}-{:02X}{:02X}",
            s.u8_at(3),
            s.u8_at(2),
            s.u8_at(1),
            s.u8_at(0)
        );
        pr.sprintf_uuid(s, text);
    }
    pr.sprintf_version(version.to_string());
    pr.set_fsblocksize(512);
    pr.set_block_size(512);
    0
}

/// `hpfs_idinfo`.
pub static HPFS: IdInfo = IdInfo {
    name: "hpfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_hpfs),
    magics: &[IdMag::new(b"\x49\xe8\x95\xf9", 0x2000 >> 10, 0)],
};
