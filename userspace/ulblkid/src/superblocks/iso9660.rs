//! `iso9660.c`: ISO 9660, with Joliet's Unicode names merged back into the
//! primary descriptor's where one is a trimmed copy of the other, and High
//! Sierra.

use crate::encode::{ENCODE_UTF16BE, c_isspace};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, IDINFO_TOLERANT, USAGE_FILESYSTEM};

/// `ISO_SUPERBLOCK_OFFSET`.
const ISO_SUPERBLOCK_OFFSET: u64 = 0x8000;
/// `ISO_SECTOR_SIZE`.
const ISO_SECTOR_SIZE: u64 = 0x800;
/// `ISO_VD_MAX`.
const ISO_VD_MAX: usize = 16;
/// `max(sizeof(struct boot_record), sizeof(struct iso_volume_descriptor))`.
const DESC_SIZE: u64 = 847;
/// `sizeof buf`: `ISO_MAX_FIELDSIZ * 5 / 2`.
const MERGE_BUF: usize = 128 * 5 / 2;

/// `probe_iso9660_hsfs`: High Sierra, the format before ISO 9660.
fn probe_hsfs(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    // struct high_sierra_volume_descriptor: 80 bytes.
    let Some(iso) = pr.get_sb_buffer(mag, 80) else {
        return pr.none_or_err();
    };
    pr.set_fsblocksize(0x800);
    pr.set_block_size(0x800);
    pr.set_version(b"High Sierra");
    pr.set_label(iso.span(48, 32));
    0
}

/// `probe_iso9660_set_uuid(pr, date)`: a UUID from a date's sixteen
/// digits; `false` when the date is unset (all '0' and no offset).
fn set_uuid_from_date(pr: &mut Probe, date: &[u8]) -> bool {
    let digits = date.span(0, 16).to_vec();
    if digits.iter().all(|&b| b == b'0') && date.u8_at(16) == 0 {
        return false;
    }
    // "%c%c%c%c-%c%c-%c%c-%c%c-%c%c-%c%c-%c%c": the bytes as they are,
    // a NUL among them included.
    let mut text = Vec::with_capacity(22);
    for (k, &b) in digits.iter().enumerate() {
        if matches!(k, 4 | 6 | 8 | 10 | 12 | 14) {
            text.push(b'-');
        }
        text.push(b);
    }
    pr.sprintf_uuid(&digits, text);
    true
}

/// `is_str_empty`: nothing, or only white space.
fn is_str_empty(s: &[u8]) -> bool {
    s.first().is_none_or(|&b| b == 0) || s.iter().all(|&b| c_isspace(b))
}

/// `is_utf16be_str_empty`: only white space, in UTF-16BE.
fn is_utf16be_str_empty(s: &[u8]) -> bool {
    (0..s.len())
        .step_by(2)
        .all(|i| s.u8_at(i) == 0 && c_isspace(s.u8_at(i.saturating_add(1))))
}

/// `merge_utf16be_ascii(out, out_len, utf16, ascii, len)`: Joliet's name,
/// in UTF-16BE, where it and the primary descriptor's ASCII name agree --
/// '_' marking a character ASCII cannot hold -- and the ASCII name's tail
/// after Joliet's shorter one ends. `None` where they disagree.
fn merge_utf16be_ascii(utf16: &[u8], ascii: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; MERGE_BUF];
    let (mut o, mut a, mut u) = (0usize, 0usize, 0usize);
    let put = |out: &mut Vec<u8>, at: usize, b: u8| {
        if let Some(x) = out.get_mut(at) {
            *x = b;
        }
    };
    // Every index stays below `len` or MERGE_BUF (a few hundred), so the
    // saturating additions never saturate.
    let next = |i: usize, n: usize| i.saturating_add(n);
    while next(u, 1) < len && a < len && next(o, 1) < MERGE_BUF {
        // A surrogate pair: the high half copied through.
        if (0xD8..=0xDB).contains(&utf16.u8_at(u))
            && next(u, 3) < len
            && (0xDC..=0xDF).contains(&utf16.u8_at(next(u, 2)))
        {
            put(&mut out, o, utf16.u8_at(u));
            put(&mut out, next(o, 1), utf16.u8_at(next(u, 1)));
            o = next(o, 2);
            u = next(u, 2);
        }
        let (c, hi, lo) = (ascii.u8_at(a), utf16.u8_at(u), utf16.u8_at(next(u, 1)));
        if c == b'_' {
            put(&mut out, o, hi);
            put(&mut out, next(o, 1), lo);
        } else if hi == 0 && lo == b'_' {
            put(&mut out, o, 0);
            put(&mut out, next(o, 1), c);
        } else if hi == 0 && c.eq_ignore_ascii_case(&lo) {
            put(&mut out, o, 0);
            put(
                &mut out,
                next(o, 1),
                if c.is_ascii_uppercase() { lo } else { c },
            );
        } else {
            return None;
        }
        o = next(o, 2);
        a = next(a, 1);
        u = next(u, 2);
    }
    while a < len && next(o, 1) < MERGE_BUF {
        put(&mut out, o, 0);
        put(&mut out, next(o, 1), ascii.u8_at(a));
        o = next(o, 2);
        a = next(a, 1);
    }
    if o == 0 {
        return None;
    }
    out.truncate(o);
    Some(out)
}

/// One of the identifier fields, Joliet's merged in where possible:
/// `SYSTEM_ID` and `VOLUME_SET_ID` are set whatever they hold.
fn set_merged_always(
    pr: &mut Probe,
    name: &'static str,
    pvd: &[u8],
    joliet: Option<&[u8]>,
    at: usize,
    len: usize,
) {
    match joliet {
        Some(j) => match merge_utf16be_ascii(j.span(at, len), pvd.span(at, len), len) {
            Some(buf) => pr.set_utf8_id_label(name, &buf, ENCODE_UTF16BE),
            None => pr.set_utf8_id_label(name, j.span(at, len), ENCODE_UTF16BE),
        },
        None => pr.set_id_label(name, pvd.span(at, len)),
    };
}

/// The publisher, data preparer and application identifiers: skipped where
/// empty, or marked unset by a leading '_'.
fn set_merged_if_set(
    pr: &mut Probe,
    name: &'static str,
    pvd: &[u8],
    joliet: Option<&[u8]>,
    at: usize,
) {
    let ascii = pvd.span(at, 128);
    let is_ascii_empty = is_str_empty(ascii) || ascii.u8_at(0) == b'_';
    let uni = joliet.map(|j| j.span(at, 128));
    let is_unicode_empty =
        uni.is_none_or(|u| is_utf16be_str_empty(u) || (u.u8_at(0) == 0 && u.u8_at(1) == b'_'));
    if !is_unicode_empty
        && !is_ascii_empty
        && let Some(u) = uni
        && let Some(buf) = merge_utf16be_ascii(u, ascii, 128)
    {
        pr.set_utf8_id_label(name, &buf, ENCODE_UTF16BE);
    } else if !is_unicode_empty && let Some(u) = uni {
        pr.set_utf8_id_label(name, u, ENCODE_UTF16BE);
    } else if !is_ascii_empty {
        pr.set_id_label(name, ascii);
    }
}

/// `probe_iso9660`.
fn probe_iso9660(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(mag) = mag else {
        return 1;
    };
    let mut off = mag
        .hoff
        .and_then(|h| pr.get_hint(h.as_bytes()))
        .unwrap_or(0);
    if off % ISO_SECTOR_SIZE != 0 {
        return 1;
    }
    if mag.magic == b"CDROM" {
        return probe_hsfs(pr, Some(mag));
    }
    let mut boot = None;
    let mut pvd = None;
    let mut joliet = None;
    off = off.wrapping_add(ISO_SUPERBLOCK_OFFSET);
    for _ in 0..ISO_VD_MAX {
        if boot.is_some() && pvd.is_some() && joliet.is_some() {
            break;
        }
        let Some(desc) = pr.get_buffer(off, DESC_SIZE) else {
            break;
        };
        let ty = desc.u8_at(0);
        if ty == 0xff {
            break;
        } else if boot.is_none() && ty == 0 {
            boot = Some(desc);
        } else if pvd.is_none() && ty == 1 {
            pvd = Some(desc);
        } else if joliet.is_none() && ty == 2 {
            let esc = desc.span(88, 3);
            if esc == b"%/@" || esc == b"%/C" || esc == b"%/E" {
                joliet = Some(desc);
            }
        }
        off = off.wrapping_add(ISO_SECTOR_SIZE);
    }
    let Some(pvd) = pvd else {
        return pr.none_or_err();
    };
    let joliet = joliet.as_deref();
    let logical_block_size = u32::from(pvd.le16(128));
    let space_size = pvd.le32(80);
    pr.set_fsblocksize(logical_block_size);
    pr.set_block_size(logical_block_size);
    pr.set_fssize(u64::from(space_size).wrapping_mul(u64::from(logical_block_size)));
    set_merged_always(pr, "SYSTEM_ID", &pvd, joliet, 8, 32);
    set_merged_always(pr, "VOLUME_SET_ID", &pvd, joliet, 190, 128);
    set_merged_if_set(pr, "PUBLISHER_ID", &pvd, joliet, 318);
    set_merged_if_set(pr, "DATA_PREPARER_ID", &pvd, joliet, 446);
    set_merged_if_set(pr, "APPLICATION_ID", &pvd, joliet, 574);
    // A UUID from the modification date, else the creation date.
    if !set_uuid_from_date(pr, pvd.span(830, 17)) {
        set_uuid_from_date(pr, pvd.span(813, 17));
    }
    if let Some(b) = &boot {
        pr.set_id_label("BOOT_SYSTEM_ID", b.span(7, 32));
    }
    if joliet.is_some() {
        pr.set_version(b"Joliet Extension");
    }
    // Joliet's label holds 16 characters of Unicode, the primary one 32 of
    // ASCII with '_' for what ASCII lacks: where Joliet's is a prefix of the
    // primary one, the whole name is rebuilt from both.
    match joliet {
        Some(j) => match merge_utf16be_ascii(j.span(40, 32), pvd.span(40, 32), 32) {
            Some(buf) => pr.set_utf8label(&buf, ENCODE_UTF16BE),
            None => pr.set_utf8label(j.span(40, 32), ENCODE_UTF16BE),
        },
        None => pr.set_label(pvd.span(40, 32)),
    };
    0
}

/// `iso9660_idinfo`.
pub static ISO9660: IdInfo = IdInfo {
    name: "iso9660",
    usage: USAGE_FILESYSTEM,
    flags: IDINFO_TOLERANT,
    minsz: 0,
    probefunc: Some(probe_iso9660),
    magics: &[
        IdMag::hinted(b"CD001", "session_offset", 32, 1),
        IdMag::hinted(b"CDROM", "session_offset", 32, 9),
    ],
};
