//! `gpt.c`: the EFI GUID partition table, and the protective MBR that
//! stands in front of one (`PMBR`, reported on its own when the GPT behind
//! it is gone).
//!
//! A header is believed only when its checksum, its own LBA, its usable
//! range and its entry array's checksum all agree; the primary at LBA 1 is
//! tried first, then the backup at the last LBA. A GPT is looked for only
//! behind a protective MBR, unless the caller asks with
//! [`PARTS_FORCE_GPT`](crate::PARTS_FORCE_GPT).

use super::mbr::{self, MBR_GPT_PARTITION};
use super::{get_flags, need_typeonly, parttable_set_uuid, set_ptuuid};
use crate::encode::ENCODE_UTF16LE;
use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Buf, Bytes, ENOMEM, PARTS_FORCE_GPT, PROBE_NONE, PROBE_OK};

/// `GPT_PRIMARY_LBA`.
const GPT_PRIMARY_LBA: u64 = 1;
/// `GPT_HEADER_SIGNATURE`: "EFI PART", read little-endian.
const GPT_HEADER_SIGNATURE: u64 = 0x5452_4150_2049_4645;
/// `GPT_HEADER_SIGNATURE_STR`.
const GPT_HEADER_SIGNATURE_STR: &[u8] = b"EFI PART";
/// `sizeof(struct gpt_header)`: the fields, without the reserved rest of
/// the sector.
const GPT_HEADER_SIZE: u32 = 92;
/// `sizeof(struct gpt_entry)`.
const GPT_ENTRY_SIZE: u32 = 128;
/// `offsetof(struct gpt_header, header_crc32)`.
const HEADER_CRC32_OFF: usize = 16;

/// `struct gpt_header`'s fields, read out of a verified header.
#[derive(Clone, Copy, Debug)]
struct GptHeader {
    /// `first_usable_lba`.
    first_usable_lba: u64,
    /// `last_usable_lba`.
    last_usable_lba: u64,
    /// `disk_guid`, as the sixteen bytes on disk.
    disk_guid: [u8; 16],
    /// `num_partition_entries`.
    num_partition_entries: u32,
}

/// `count_crc32(buf, len, exclude_off, exclude_len)`: EFI's CRC-32 -- seed
/// ~0, inverted at the end -- with `exclude_len` bytes at `exclude_off`
/// read as zeros (`ul_crc32_exclude_offset`).
fn count_crc32(buf: &[u8], exclude_off: usize, exclude_len: usize) -> u32 {
    let head = buf.get(..exclude_off.min(buf.len())).unwrap_or_default();
    let skip_end = exclude_off.saturating_add(exclude_len).min(buf.len());
    let tail = buf.get(skip_end..).unwrap_or_default();
    let zeros = vec![0u8; skip_end.saturating_sub(head.len())];
    let mut crc = crc32::crc32_raw(!0, head);
    crc = crc32::crc32_raw(crc, &zeros);
    crc32::crc32_raw(crc, tail) ^ !0
}

/// `get_lba_buffer(pr, lba, bytes)`: `bytes` bytes at logical block `lba`.
fn get_lba_buffer(pr: &mut Probe, lba: u64, bytes: u64) -> Option<Buf> {
    let off = u64::from(pr.sectorsize()).wrapping_mul(lba);
    pr.get_buffer(off, bytes)
}

/// Where each byte of a UUID comes from in an EFI GUID.
const EFI_GUID_ORDER: [usize; 16] = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];

/// `swap_efi_guid`: EFI stores a GUID's first three fields little-endian;
/// as a UUID they are big-endian.
fn swap_efi_guid(g: &[u8]) -> [u8; 16] {
    let mut u = [0u8; 16];
    for (b, &from) in u.iter_mut().zip(EFI_GUID_ORDER.iter()) {
        *b = g.u8_at(from);
    }
    u
}

/// `last_lba(pr, &lba)`: the device's last logical block, if it has one.
fn last_lba(pr: &mut Probe) -> Option<u64> {
    let sz = pr.size();
    let ssz = u64::from(pr.sectorsize());
    if sz < ssz {
        return None;
    }
    // A sector size of 0 would be a division by zero upstream.
    sz.checked_div(ssz).map(|n| n.wrapping_sub(1))
}

/// `is_pmbr_valid(pr, has)`: a valid MBR with a GPT entry. `has` is `None`
/// when the caller only wants to know whether to go on -- and then the
/// check is skipped under [`PARTS_FORCE_GPT`]. 1 yes, 0 no, `-errno` for a
/// read error.
fn is_pmbr_valid(pr: &mut Probe, mut has: Option<&mut bool>) -> i32 {
    let flags = get_flags(pr);
    match has.as_deref_mut() {
        Some(h) => *h = false,
        None => {
            if flags & PARTS_FORCE_GPT != 0 {
                return 1;
            }
        }
    }
    let Some(data) = pr.get_sector(0) else {
        if pr.errno != 0 {
            return pr.errno.wrapping_neg();
        }
        return 0;
    };
    if !mbr::is_valid_magic(&data) {
        return 0;
    }
    if !mbr::partitions(&data)
        .iter()
        .any(|p| p.sys_ind == MBR_GPT_PARTITION)
    {
        return 0;
    }
    if let Some(h) = has {
        *h = true;
    }
    1
}

/// `get_gpt_header(pr, &hdr, &ents, lba, lastlba)`: the header at `lba`
/// and its entry array, if both are valid. `None` otherwise, with
/// [`Probe::errno`] as the last read left it.
fn get_gpt_header(pr: &mut Probe, lba: u64, lastlba: u64) -> Option<(GptHeader, Buf)> {
    let ssz = pr.sectorsize();
    // The whole sector is the header's.
    let h = get_lba_buffer(pr, lba, u64::from(ssz))?;
    if h.le64(0) != GPT_HEADER_SIGNATURE {
        return None;
    }
    let hsz = h.le32(12);
    // UEFI: at least the 92 bytes of fields, at most a block.
    if hsz > ssz || hsz < GPT_HEADER_SIZE {
        return None;
    }
    let crc = count_crc32(
        h.span(0, usize::try_from(hsz).unwrap_or(usize::MAX)),
        HEADER_CRC32_OFF,
        4,
    );
    if !pr.verify_csum(u64::from(crc), u64::from(h.le32(HEADER_CRC32_OFF))) {
        return None;
    }
    // A valid header is where it says it is.
    if h.le64(24) != lba {
        return None;
    }
    let fu = h.le64(40);
    let lu = h.le64(48);
    if lu < fu || fu > lastlba || lu > lastlba {
        return None;
    }
    // ... and outside the usable range.
    if fu < lba && lba < lu {
        return None;
    }
    let num = h.le32(80);
    let entsz = h.le32(84);
    let esz = u64::from(num).wrapping_mul(u64::from(entsz));
    if esz == 0 || esz >= u64::from(u32::MAX) || entsz != GPT_ENTRY_SIZE {
        return None;
    }
    let mut disk_guid = [0u8; 16];
    for (d, &s) in disk_guid.iter_mut().zip(h.span(56, 16)) {
        *d = s;
    }
    let hdr = GptHeader {
        first_usable_lba: fu,
        last_usable_lba: lu,
        disk_guid,
        num_partition_entries: num,
    };
    let entries_lba = h.le64(72);
    let ents = get_lba_buffer(pr, entries_lba, esz)?;
    // A checksum mismatch here is fatal whatever the flags say: upstream
    // compares directly rather than through blkid_probe_verify_csum.
    if count_crc32(&ents, 0, 0) != h.le32(88) {
        return None;
    }
    Some((hdr, ents))
}

/// `probe_gpt_pt`.
fn probe_gpt_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(lastlba) = last_lba(pr) else {
        return PROBE_NONE;
    };
    let ret = is_pmbr_valid(pr, None);
    if ret < 0 {
        return ret;
    }
    if ret == 0 {
        return PROBE_NONE;
    }
    pr.errno = 0;
    let mut lba = GPT_PRIMARY_LBA;
    let mut found = get_gpt_header(pr, lba, lastlba);
    if found.is_none() && pr.errno == 0 {
        lba = lastlba;
        found = get_gpt_header(pr, lba, lastlba);
    }
    let Some((h, e)) = found else {
        return pr.none_or_err();
    };
    // Upstream multiplies by the device's size, not its sector size, so this
    // lands past the end of the device and matches no wiped area -- unless
    // the product wraps. Kept as upstream computes it, wrapping included.
    let wipe_off = lba.wrapping_mul(pr.size());
    pr.use_wiper(wipe_off, 8);
    let magic_off = u64::from(pr.sectorsize()).wrapping_mul(lba);
    if pr.set_magic(magic_off, GPT_HEADER_SIGNATURE_STR) != 0 {
        return -ENOMEM;
    }
    let guid = swap_efi_guid(&h.disk_guid);
    if need_typeonly(pr) {
        // Values only: the type, and the disk GUID as PTUUID.
        set_ptuuid(pr, &guid);
        return PROBE_OK;
    }
    let ss = pr.sectorsize();
    let tab_off = u64::from(ss).wrapping_mul(lba);
    let ssf = u64::from(ss / 512);
    let Some(ls) = pr.partlist_mut() else {
        return PROBE_NONE;
    };
    let tab = ls.new_parttable("gpt", tab_off);
    if let Some(t) = ls.tab_mut(tab) {
        parttable_set_uuid(t, &guid);
    }
    let fu = h.first_usable_lba;
    let lu = h.last_usable_lba;
    let esize = usize::try_from(GPT_ENTRY_SIZE).unwrap_or(128);
    for i in 0..h.num_partition_entries {
        let base = usize::try_from(i)
            .unwrap_or(usize::MAX)
            .saturating_mul(esize);
        let ent = e.span(base, esize);
        let start = ent.le64(32);
        let size = ent.le64(40).wrapping_sub(start).wrapping_add(1);
        // An unused entry (type 00000000-0000-...) still uses up a number.
        if uuid_is_zero(ent.span(0, 16)) {
            ls.increment_partno();
            continue;
        }
        // The partition has to be inside the usable range.
        if start < fu || start.wrapping_add(size).wrapping_sub(1) > lu {
            ls.increment_partno();
            continue;
        }
        let par = ls.add_partition(tab, start.wrapping_mul(ssf), size.wrapping_mul(ssf));
        if let Some(p) = ls.part_mut(par) {
            p.set_utf8name(ent.span(56, 72), ENCODE_UTF16LE);
            p.set_uuid(&swap_efi_guid(ent.span(16, 16)));
            p.set_type_uuid(&swap_efi_guid(ent.span(0, 16)));
            p.set_flags(ent.le64(48));
        }
    }
    PROBE_OK
}

/// `!guidcmp(guid, GPT_UNUSED_ENTRY_GUID)`.
fn uuid_is_zero(g: &[u8]) -> bool {
    g.iter().all(|&b| b == 0)
}

/// `gpt_pt_idinfo`. No magic: most EFI implementations let the protective
/// MBR be skipped, so the prober always runs.
pub static GPT: IdInfo = IdInfo {
    name: "gpt",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_gpt_pt),
    magics: &[],
};

/// `probe_pmbr_pt`: a protective MBR alone, with no valid GPT header at
/// either end.
fn probe_pmbr_pt(pr: &mut Probe, _mag: Option<&'static IdMag>) -> i32 {
    let Some(lastlba) = last_lba(pr) else {
        return PROBE_NONE;
    };
    let mut has = false;
    // Upstream ignores the return value: only `has` matters.
    is_pmbr_valid(pr, Some(&mut has));
    if !has {
        return PROBE_NONE;
    }
    if get_gpt_header(pr, GPT_PRIMARY_LBA, lastlba).is_none()
        && get_gpt_header(pr, lastlba, lastlba).is_none()
    {
        return PROBE_OK;
    }
    PROBE_NONE
}

/// `pmbr_pt_idinfo`.
pub static PMBR: IdInfo = IdInfo {
    name: "PMBR",
    usage: 0,
    flags: 0,
    minsz: 0,
    probefunc: Some(probe_pmbr_pt),
    magics: &[IdMag::new(b"\x55\xAA", 0, 510)],
};
