//! `udf.c`: UDF (ECMA-167) -- the volume recognition sequence, the anchor
//! that fixes the block size, and the volume descriptors the label, the
//! UUID and the other identifiers come from.

use crate::encode::{ENCODE_LATIN1, ENCODE_UTF16BE, encode_to_utf8};
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Buf, Bytes, IDINFO_TOLERANT, USAGE_FILESYSTEM};

/// `TAG_ID_PVD`.
const TAG_ID_PVD: u16 = 1;
/// `TAG_ID_AVDP`.
const TAG_ID_AVDP: u16 = 2;
/// `TAG_ID_IUVD`.
const TAG_ID_IUVD: u16 = 4;
/// `TAG_ID_LVD`.
const TAG_ID_LVD: u16 = 6;
/// `TAG_ID_TD`.
const TAG_ID_TD: u16 = 8;
/// `TAG_ID_LVID`.
const TAG_ID_LVID: u16 = 9;
/// `UDF_VSD_OFFSET`.
const UDF_VSD_OFFSET: u64 = 0x8000;
/// `sizeof(struct volume_descriptor)`: the tag and the largest member of
/// the union, the logical volume descriptor.
const VD_SIZE: u64 = 440;
/// `sizeof(struct logical_vol_integ_descriptor_imp_use)`.
const LVIDIU_SIZE: u64 = 46;

/// `udf_cid_to_enc`.
fn cid_to_enc(cid: u8) -> Option<i32> {
    match cid {
        8 => Some(ENCODE_LATIN1),
        16 => Some(ENCODE_UTF16BE),
        _ => None,
    }
}

/// `is_charset_udf(charspec)`: type 0 and "OSTA Compressed Unicode".
fn is_charset_udf(cs: &[u8]) -> bool {
    cs.u8_at(0) == 0 && cs.span(1, 24) == b"OSTA Compressed Unicode\0"
}

/// A dstring's characters, its length taken from its last byte (less one
/// for the compression ID it counts), and its encoding.
fn dstring(d: &[u8], size: usize) -> (Option<i32>, &[u8]) {
    // Every caller's field is at least two bytes.
    let clen = usize::from(d.u8_at(size.saturating_sub(1)))
        .saturating_sub(1)
        .min(size.saturating_sub(2));
    (cid_to_enc(d.u8_at(0)), d.span(1, clen))
}

/// `strncmp(field, literal, sizeof(field)) == 0` for a literal shorter
/// than the field: its text and then its NUL.
fn field_is(field: &[u8], literal: &[u8]) -> bool {
    let mut l = literal.to_vec();
    l.push(0);
    field.span(0, l.len()) == l.as_slice()
}

/// `gen_uuid_from_volset_id`: UDF's UUID from the first 16 characters of
/// the volume set identifier (UDF 2.01 2.2.2.5 wants them unique, the first
/// eight a hex time stamp) -- as they are where they are hex digits,
/// hex-encoded where not. `None` when the identifier is unusable.
fn gen_uuid_from_volset_id(volset: &[u8]) -> Option<Vec<u8>> {
    let (enc, c) = dstring(volset, 128);
    let enc = enc?;
    let buf = encode_to_utf8(enc, 17, c);
    if buf.len() < 8 {
        return None;
    }
    let b = |i: usize| buf.get(i).copied().unwrap_or(0);
    let nonhexpos = (0..16).find(|&i| !b(i).is_ascii_hexdigit()).unwrap_or(16);
    let mut uuid = Vec::with_capacity(17);
    if nonhexpos < 8 {
        for i in 0..8 {
            uuid.extend_from_slice(format!("{:02x}", b(i)).as_bytes());
        }
    } else if nonhexpos < 16 {
        uuid.extend((0..8).map(|i| b(i).to_ascii_lowercase()));
        for i in 8..12 {
            uuid.extend_from_slice(format!("{:02x}", b(i)).as_bytes());
        }
    } else {
        uuid.extend((0..16).map(|i| b(i).to_ascii_lowercase()));
    }
    uuid.push(0);
    Some(uuid)
}

/// The block size and the anchor descriptor, once found.
struct Anchor {
    bs: u32,
    vd: Buf,
}

/// Read the AVDP for block size `pbs` at sector 256, then 512.
fn read_anchor(pr: &mut Probe, s_off: u64, pbs: u32) -> Result<Option<Buf>, i32> {
    for sector in [256u32, 512] {
        let Some(vd) = pr.get_buffer(
            s_off.wrapping_add(u64::from(sector.wrapping_mul(pbs))),
            VD_SIZE,
        ) else {
            return Err(pr.none_or_err());
        };
        // pbs is not 0: find_anchor skips that.
        let expected = s_off
            .checked_div(u64::from(pbs))
            .unwrap_or(0)
            .wrapping_add(u64::from(sector));
        if u64::from(vd.le32(12)) == expected && vd.le16(0) == TAG_ID_AVDP {
            return Ok(Some(vd));
        }
    }
    Ok(None)
}

/// The volume recognition sequence at `vsd_len`-byte strides: whether it
/// holds an NSR descriptor before it ends.
fn has_nsr(pr: &mut Probe, s_off: u64, vsd_len: u32) -> Result<bool, i32> {
    for b in 0..64u32 {
        let off = s_off
            .wrapping_add(UDF_VSD_OFFSET)
            .wrapping_add(u64::from(b.wrapping_mul(vsd_len)));
        let Some(vsd) = pr.get_buffer(off, 7) else {
            return Err(pr.none_or_err());
        };
        let id = vsd.span(1, 5);
        if id.u8_at(0) == 0 {
            break;
        }
        if id == b"NSR02" || id == b"NSR03" {
            return Ok(true);
        }
        // The sequence ends at the first sector that is not a descriptor;
        // it does not stop at TEA01 (UDF 2.00 and older may put NSR after).
        if !matches!(id, b"BEA01" | b"BOOT2" | b"CD001" | b"CDW02" | b"TEA01") {
            break;
        }
    }
    Ok(false)
}

/// Find the block size: the device's sector size first, then 512 to 4096.
fn find_anchor(pr: &mut Probe, s_off: u64) -> Result<Option<Anchor>, i32> {
    let sector_size = pr.sectorsize();
    let pbs = [sector_size, 512, 1024, 2048, 4096];
    // The 2048-byte VSD of the first session is read once, whatever the
    // block size: -1 not yet, 0 no NSR in it, 1 an NSR.
    let mut vsd_2048_valid: i32 = -1;
    for (i, &p) in pbs.iter().enumerate() {
        if i != 0 && sector_size == p {
            continue;
        }
        // Upstream divides by a sector size of 0 too.
        if p == 0 || !s_off.is_multiple_of(u64::from(p)) {
            continue;
        }
        let vsd_len = p.max(2048);
        let first_2048 = s_off == 0 && vsd_len == 2048;
        if first_2048 && vsd_2048_valid == 0 {
            continue;
        }
        if !((first_2048 && vsd_2048_valid == 1) || has_nsr(pr, s_off, vsd_len)?) {
            if first_2048 {
                vsd_2048_valid = 0;
            }
            continue;
        }
        if first_2048 {
            vsd_2048_valid = 1;
        }
        if let Some(vd) = read_anchor(pr, s_off, p)? {
            return Ok(Some(Anchor { bs: p, vd }));
        }
    }
    Ok(None)
}

/// `probe_udf`.
fn probe_udf(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let s_off = pr.get_hint(b"session_offset").unwrap_or(0);
    let Anchor { bs, vd } = match find_anchor(pr, s_off) {
        Ok(Some(a)) => a,
        Ok(None) => return 1,
        Err(rc) => return rc,
    };
    // bs is not 0: the anchor search skips that.
    let count = vd.le32(16).checked_div(bs).unwrap_or(0);
    let loc = vd.le32(20);
    let mut lvid_len: u32 = 0;
    let mut lvid_loc: u32 = 0;
    let mut udf_rev: u16 = 0;
    let (mut have_label, mut have_uuid, mut have_logvolid, mut have_volid) =
        (false, false, false, false);
    let (mut have_volsetid, mut have_applicationid, mut have_publisherid) = (false, false, false);
    for b in 0..count {
        let at = u64::from(loc.wrapping_add(b)).wrapping_mul(u64::from(bs));
        let Some(vd) = pr.get_buffer(at, VD_SIZE) else {
            return pr.none_or_err();
        };
        let ty = vd.le16(0);
        if ty == 0 || vd.le32(12) != loc.wrapping_add(b) || ty == TAG_ID_TD {
            break;
        }
        if ty == TAG_ID_PVD {
            let udf_cs = is_charset_udf(vd.span(200, 64));
            if !have_volid && udf_cs {
                let (enc, c) = dstring(vd.span(24, 32), 32);
                if let Some(enc) = enc {
                    have_volid = pr.set_utf8_id_label("VOLUME_ID", c, enc) == 0;
                }
            }
            if !have_uuid
                && udf_cs
                && let Some(uuid) = gen_uuid_from_volset_id(vd.span(72, 128))
            {
                have_uuid = pr.strncpy_uuid(&uuid) == 0;
            }
            if !have_volsetid && udf_cs {
                let (enc, c) = dstring(vd.span(72, 128), 128);
                if let Some(enc) = enc {
                    have_volsetid = pr.set_utf8_id_label("VOLUME_SET_ID", c, enc) == 0;
                }
            }
            if !have_applicationid {
                // The application identifier, or failing that the
                // implementation's developer ID, less a leading '*'.
                let strip = |f: &[u8]| -> Vec<u8> {
                    let s = crate::c_str(f);
                    s.strip_prefix(b"*").unwrap_or(s).to_vec()
                };
                let mut app_id = strip(vd.span(345, 23));
                if app_id.is_empty() {
                    app_id = strip(vd.span(389, 23));
                }
                if !app_id.is_empty() {
                    have_applicationid = pr.set_id_label("APPLICATION_ID", &app_id) == 0;
                }
            }
        } else if ty == TAG_ID_LVD {
            if (lvid_len == 0 || lvid_loc == 0) && vd.le32(268) != 0 {
                lvid_len = vd.le32(432);
                lvid_loc = vd.le32(436);
            }
            if udf_rev == 0 && field_is(vd.span(217, 23), b"*OSTA UDF Compliant") {
                udf_rev = vd.le16(240);
            }
            if (!have_logvolid || !have_label) && is_charset_udf(vd.span(20, 64)) {
                let (enc, c) = dstring(vd.span(84, 128), 128);
                if let Some(enc) = enc {
                    if !have_label {
                        have_label = pr.set_utf8label(c, enc) == 0;
                    }
                    if !have_logvolid {
                        have_logvolid = pr.set_utf8_id_label("LOGICAL_VOLUME_ID", c, enc) == 0;
                    }
                }
            }
        } else if ty == TAG_ID_IUVD
            && !have_publisherid
            && field_is(vd.span(21, 23), b"*UDF LV Info")
            && is_charset_udf(vd.span(52, 64))
        {
            // LVInfo1 is often the owner's name: PUBLISHER_ID, as ISO 9660
            // has it.
            let (enc, c) = dstring(vd.span(244, 36), 36);
            if let Some(enc) = enc {
                have_publisherid = pr.set_utf8_id_label("PUBLISHER_ID", c, enc) == 0;
            }
        }
        if have_volid
            && have_uuid
            && have_volsetid
            && have_logvolid
            && have_label
            && lvid_len != 0
            && lvid_loc != 0
            && have_applicationid
            && have_publisherid
        {
            break;
        }
    }
    // The first logical volume integrity descriptor, for the revision.
    if lvid_loc != 0 && u64::from(lvid_len) >= VD_SIZE {
        let at = u64::from(lvid_loc).wrapping_mul(u64::from(bs));
        let Some(vd) = pr.get_buffer(at, VD_SIZE) else {
            return pr.none_or_err();
        };
        if vd.le16(0) == TAG_ID_LVID
            && vd.le32(12) == lvid_loc
            && u64::from(vd.le32(76)) >= LVIDIU_SIZE
        {
            let iu_off = 16 + 64 + u64::from(8u32.wrapping_mul(vd.le32(72)));
            let Some(iu) = pr.get_buffer(at.wrapping_add(iu_off), LVIDIU_SIZE) else {
                return pr.none_or_err();
            };
            for rev in [iu.le16(40), iu.le16(42)] {
                if rev != 0 && udf_rev < rev {
                    udf_rev = rev;
                }
            }
        }
    }
    if udf_rev != 0 {
        // BCD in hex: 0x0201 is 2.01.
        pr.sprintf_version(format!("{:x}.{:02x}", udf_rev >> 8, udf_rev & 0xFF));
    }
    pr.set_fsblocksize(bs);
    pr.set_block_size(bs);
    0
}

/// `udf_idinfo`: any descriptor of the recognition sequence, at 32 KiB into
/// the session.
pub static UDF: IdInfo = IdInfo {
    name: "udf",
    usage: USAGE_FILESYSTEM,
    flags: IDINFO_TOLERANT,
    minsz: 0,
    probefunc: Some(probe_udf),
    magics: &[
        IdMag::hinted(b"BEA01", "session_offset", 32, 1),
        IdMag::hinted(b"BOOT2", "session_offset", 32, 1),
        IdMag::hinted(b"CD001", "session_offset", 32, 1),
        IdMag::hinted(b"CDW02", "session_offset", 32, 1),
        IdMag::hinted(b"NSR02", "session_offset", 32, 1),
        IdMag::hinted(b"NSR03", "session_offset", 32, 1),
        IdMag::hinted(b"TEA01", "session_offset", 32, 1),
    ],
};
