//! `bitlocker.c`: BitLocker volumes -- Vista, Windows 7, and BitLocker To Go.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_CRYPTO};

/// `BDE_MAGIC_VISTA`.
const BDE_MAGIC_VISTA: &[u8] = b"\xeb\x52\x90-FVE-FS-";
/// `BDE_MAGIC_WIN7`.
const BDE_MAGIC_WIN7: &[u8] = b"\xeb\x58\x90-FVE-FS-";
/// `BDE_MAGIC_TOGO`.
const BDE_MAGIC_TOGO: &[u8] = b"\xeb\x58\x90MSWIN4.1";
/// `BDE_MAGIC_FVE`.
const BDE_MAGIC_FVE: &[u8] = b"-FVE-FS-";
/// `BDE_HDR_SIZE`.
const BDE_HDR_SIZE: u64 = 512;

/// The three kinds of header, in `get_bitlocker_type`'s order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Vista,
    Win7,
    Togo,
}

/// What `get_bitlocker_headers` found: the kind, and the header and FVE
/// metadata where it read them.
struct Headers {
    kind: Kind,
    hdr: Option<crate::Buf>,
    fve: Option<crate::Buf>,
}

/// `get_bitlocker_headers`: `Ok(Some(..))` for BitLocker, `Ok(None)` for
/// nothing, `Err(rc)` for a read error.
fn get_headers(pr: &mut Probe) -> Result<Option<Headers>, i32> {
    let err = |pr: &Probe| pr.none_or_err();
    let Some(buf) = pr.get_buffer(0, BDE_HDR_SIZE) else {
        return Err(err(pr));
    };
    let head = buf.span(0, 11);
    let kind = if head == BDE_MAGIC_VISTA {
        Kind::Vista
    } else if head == BDE_MAGIC_WIN7 {
        Kind::Win7
    } else if head == BDE_MAGIC_TOGO {
        Kind::Togo
    } else {
        return Ok(None);
    };
    let off = match kind {
        Kind::Win7 => buf.le64(176),
        Kind::Togo => buf.le64(440),
        Kind::Vista => {
            return Ok(Some(Headers {
                kind,
                hdr: None,
                fve: None,
            }));
        }
    };
    if off == 0 || off % 64 != 0 {
        return Ok(None);
    }
    // struct bde_fve_metadata: signature[8], size, version -- 12 bytes.
    let Some(fve) = pr.get_buffer(off, 12) else {
        return Err(err(pr));
    };
    if fve.span(0, 8) != BDE_MAGIC_FVE {
        return Ok(None);
    }
    Ok(Some(Headers {
        kind,
        hdr: Some(buf),
        fve: Some(fve),
    }))
}

/// `blkid_probe_is_bitlocker`.
pub(crate) fn is_bitlocker(pr: &mut Probe) -> bool {
    matches!(get_headers(pr), Ok(Some(_)))
}

/// `probe_bitlocker`.
fn probe_bitlocker(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let h = match get_headers(pr) {
        Ok(Some(h)) => h,
        Ok(None) => return 1,
        Err(rc) => return rc,
    };
    if h.kind == Kind::Win7
        && let Some(hdr) = &h.hdr
    {
        // "%016d" of the serial as an int -- always zero, it seems.
        let serial = i32::from_ne_bytes(hdr.le32(67).to_ne_bytes());
        pr.sprintf_uuid(hdr.span(67, 4), format!("{serial:016}"));
    }
    if let Some(fve) = &h.fve {
        pr.sprintf_version(fve.le16(10).to_string());
    }
    0
}

/// `bitlocker_idinfo`.
pub static BITLOCKER: IdInfo = IdInfo {
    name: "BitLocker",
    usage: USAGE_CRYPTO,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_bitlocker),
    magics: &[
        IdMag::new(BDE_MAGIC_VISTA, 0, 0),
        IdMag::new(BDE_MAGIC_WIN7, 0, 0),
        IdMag::new(BDE_MAGIC_TOGO, 0, 0),
    ],
};
