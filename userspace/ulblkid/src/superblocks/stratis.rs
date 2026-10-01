//! `stratis.c`: Stratis pool block devices, either of two superblock copies.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, USAGE_RAID};

/// `STRATIS_MAGIC`.
const STRATIS_MAGIC: &[u8] = b"!Stra0tis\x86\xff\x02^Arh";
/// `BS`.
const BS: usize = 512;
/// `FIRST_COPY_OFFSET`.
const FIRST_COPY_OFFSET: usize = BS;
/// `SECOND_COPY_OFFSET`.
const SECOND_COPY_OFFSET: usize = BS * 9;
/// `SB_AREA_SIZE`.
const SB_AREA_SIZE: u64 = 512 * 16;

/// `stratis_valid_sb(p)`: CRC-32C of the sector after its first four bytes.
fn valid_sb(sb: &[u8]) -> bool {
    let crc = crc32c::crc32c_raw(!0, sb.span(4, BS - 4)) ^ !0;
    crc == sb.le32(0)
}

/// `stratis_format_uuid`: the 32 characters with dashes after the 8th,
/// 12th, 16th and 20th, and a NUL -- 37 bytes.
fn format_uuid(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(37);
    for i in 0..32 {
        out.push(src.u8_at(i));
        if matches!(i, 7 | 11 | 15 | 19) {
            out.push(b'-');
        }
    }
    out.push(0);
    out
}

/// `probe_stratis`.
fn probe_stratis(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(buf) = pr.get_buffer(0, SB_AREA_SIZE) else {
        return pr.none_or_err();
    };
    let at = if valid_sb(buf.span(FIRST_COPY_OFFSET, BS)) {
        FIRST_COPY_OFFSET
    } else if valid_sb(buf.span(SECOND_COPY_OFFSET, BS)) {
        SECOND_COPY_OFFSET
    } else {
        return 1;
    };
    let sb = buf.span(at, 128);
    let uuid = format_uuid(sb.span(64, 32));
    pr.strncpy_uuid(&uuid);
    let pool = format_uuid(sb.span(32, 32));
    pr.set_value("POOL_UUID", &pool);
    pr.set_value_str("BLOCKDEV_SECTORS", sb.le64(20).to_string().as_bytes());
    pr.set_value_str("BLOCKDEV_INITTIME", sb.le64(120).to_string().as_bytes());
    0
}

/// `stratis_idinfo`.
pub static STRATIS: IdInfo = IdInfo {
    name: "stratis",
    usage: USAGE_RAID,
    flags: 0,
    minsz: SB_AREA_SIZE,
    probefunc: Some(probe_stratis),
    magics: &[
        IdMag::new(STRATIS_MAGIC, 0, 516),
        IdMag::new(STRATIS_MAGIC, 0, 4612),
    ],
};
