//! `gpt.c`: the UEFI GPT label driver (UEFI 2.3.1, chapter 5).
//!
//! A protective MBR in sector 0; the primary header in sector 1 and its
//! entry array after it; a backup array and header at the end of the disk.
//! Headers and the entry array are kept as the bytes read (or made), as
//! upstream keeps them, so that a header larger than 92 bytes, an entry
//! size other than 128 and the reserved areas all survive a rewrite; fields
//! are read and written in place, little-endian.
//!
//! GUIDs are stored as UEFI writes them -- the first three fields
//! little-endian -- and turned to and from libuuid's text order by
//! [`swap_efi_guid`].

use crate::context::Context;
use crate::errno::{EINVAL, ENOMEM, ENOSPC, ENOSYS, ERANGE};
use crate::field::{FIELDFL_DETAIL, FIELDFL_EYECANDY, FIELDFL_NUMBER, Field};
use crate::item::{ItemData, LabelItem};
use crate::label::{Label, LabelData, LabelOps, Located};
use crate::partition::Partition;
use crate::parttype::{PartType, Shortcut};
use crate::utils::{div, le_get, le_put, strtol, trunc_i32};
use crate::{
    GPT_FLAG_GUIDSPECIFIC, GPT_FLAG_LEGACYBOOT, GPT_FLAG_NOBLOCK, GPT_FLAG_REQUIRED,
    GPT_NPARTITIONS_DEFAULT, Geometry, LabelType, fieldtype, is_undef,
};

/// "EFI PART".
const GPT_HEADER_SIGNATURE: u64 = 0x5452_4150_2049_4645;
const GPT_HEADER_REVISION_V1_02: u32 = 0x0001_0200;
const GPT_HEADER_REVISION_V1_00: u32 = 0x0001_0000;
const GPT_HEADER_REVISION_V0_99: u32 = 0x0000_9900;
/// The smallest header.
const GPT_HEADER_MINSZ: u32 = 92;
/// `sizeof (struct gpt_header)`: 92 bytes and the reserved rest of 512.
const GPT_HEADER_SIZE: usize = 512;
/// A protective MBR.
const GPT_MBR_PROTECTIVE: i32 = 1;
/// A hybrid one.
const GPT_MBR_HYBRID: i32 = 2;
/// `GPT_PRIMARY_PARTITION_TABLE_LBA`.
const PRIMARY_LBA: u64 = 1;
/// `EFI_PMBR_OSTYPE`.
const EFI_PMBR_OSTYPE: u8 = 0xee;
/// `sizeof (struct gpt_entry)`.
const GPT_ENTRY_SIZE: usize = 128;
/// `GPT_PART_NAME_LEN`: UTF-16 units in a name.
const GPT_PART_NAME_LEN: usize = 36;
/// `GPT_NPARTITIONS`: `FDISK_GPT_NPARTITIONS_DEFAULT`.
const GPT_NPARTITIONS: u64 = 128;
const _: () = assert!(GPT_NPARTITIONS_DEFAULT == 128);
/// The Linux filesystem type, the default.
const GPT_DEFAULT_ENTRY_TYPE: &[u8] = b"0FC63DAF-8483-4772-8E79-3D69D8477DE4";

/// `GPT_ATTRBIT_*`.
const ATTRBIT_REQ: u32 = 0;
const ATTRBIT_NOBLOCK: u32 = 1;
const ATTRBIT_LEGACY: u32 = 2;
const ATTRBIT_GUID_FIRST: u32 = 48;
const ATTRBIT_GUID_COUNT: u32 = 16;
/// `GPT_ATTRSTR_*`.
const ATTRSTR_REQ: &str = "RequiredPartition";
const ATTRSTR_REQ_TYPO: &str = "RequiredPartiton";
const ATTRSTR_NOBLOCK: &str = "NoBlockIOProtocol";
const ATTRSTR_LEGACY: &str = "LegacyBIOSBootable";

/// 16 bytes at `off`.
fn get16(b: &[u8], off: usize) -> [u8; 16] {
    let mut g = [0u8; 16];
    if let Some(src) = b.get(off..off.wrapping_add(16)) {
        g.copy_from_slice(src);
    }
    g
}

/// Writes 16 bytes at `off`.
fn put16(b: &mut [u8], off: usize, g: &[u8; 16]) {
    if let Some(dst) = b.get_mut(off..off.wrapping_add(16)) {
        dst.copy_from_slice(g);
    }
}

/// `struct gpt_header`, as the sector it lives in.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Hdr(Vec<u8>);

impl Hdr {
    fn zeroed(sector_size: u64) -> Self {
        Self(vec![
            0u8;
            usize::try_from(sector_size)
                .unwrap_or(GPT_HEADER_SIZE)
                .max(GPT_HEADER_SIZE)
        ])
    }
    fn signature(&self) -> u64 {
        le_get(&self.0, 0, 8)
    }
    fn revision(&self) -> u32 {
        crate::utils::u64_as_u32(le_get(&self.0, 8, 4))
    }
    fn size(&self) -> u32 {
        crate::utils::u64_as_u32(le_get(&self.0, 12, 4))
    }
    fn crc32(&self) -> u32 {
        crate::utils::u64_as_u32(le_get(&self.0, 16, 4))
    }
    fn my_lba(&self) -> u64 {
        le_get(&self.0, 24, 8)
    }
    fn alternative_lba(&self) -> u64 {
        le_get(&self.0, 32, 8)
    }
    fn first_usable_lba(&self) -> u64 {
        le_get(&self.0, 40, 8)
    }
    fn last_usable_lba(&self) -> u64 {
        le_get(&self.0, 48, 8)
    }
    fn disk_guid(&self) -> [u8; 16] {
        get16(&self.0, 56)
    }
    fn partition_entry_lba(&self) -> u64 {
        le_get(&self.0, 72, 8)
    }
    fn npartition_entries(&self) -> u32 {
        crate::utils::u64_as_u32(le_get(&self.0, 80, 4))
    }
    fn sizeof_partition_entry(&self) -> u32 {
        crate::utils::u64_as_u32(le_get(&self.0, 84, 4))
    }
    fn partition_entry_array_crc32(&self) -> u32 {
        crate::utils::u64_as_u32(le_get(&self.0, 88, 4))
    }
    fn set_signature(&mut self, v: u64) {
        le_put(&mut self.0, 0, 8, v);
    }
    fn set_revision(&mut self, v: u32) {
        le_put(&mut self.0, 8, 4, u64::from(v));
    }
    fn set_size(&mut self, v: u32) {
        le_put(&mut self.0, 12, 4, u64::from(v));
    }
    fn set_crc32(&mut self, v: u32) {
        le_put(&mut self.0, 16, 4, u64::from(v));
    }
    fn set_my_lba(&mut self, v: u64) {
        le_put(&mut self.0, 24, 8, v);
    }
    fn set_alternative_lba(&mut self, v: u64) {
        le_put(&mut self.0, 32, 8, v);
    }
    fn set_first_usable_lba(&mut self, v: u64) {
        le_put(&mut self.0, 40, 8, v);
    }
    fn set_last_usable_lba(&mut self, v: u64) {
        le_put(&mut self.0, 48, 8, v);
    }
    fn set_disk_guid(&mut self, g: &[u8; 16]) {
        put16(&mut self.0, 56, g);
    }
    fn set_partition_entry_lba(&mut self, v: u64) {
        le_put(&mut self.0, 72, 8, v);
    }
    fn set_npartition_entries(&mut self, v: u32) {
        le_put(&mut self.0, 80, 4, u64::from(v));
    }
    fn set_sizeof_partition_entry(&mut self, v: u32) {
        le_put(&mut self.0, 84, 4, u64::from(v));
    }
    fn set_partition_entry_array_crc32(&mut self, v: u32) {
        le_put(&mut self.0, 88, 4, u64::from(v));
    }
}

/// `struct gpt_entry` field offsets.
mod ent {
    pub const TYPE: usize = 0;
    pub const PARTITION_GUID: usize = 16;
    pub const LBA_START: usize = 32;
    pub const LBA_END: usize = 40;
    pub const ATTRS: usize = 48;
    pub const NAME: usize = 56;
}

/// `struct fdisk_gpt_label`'s own part.
#[derive(Clone, Debug, Default)]
pub struct GptLabel {
    /// The primary header.
    pheader: Option<Hdr>,
    /// The backup header.
    bheader: Option<Hdr>,
    /// The entry array.
    ents: Option<Vec<u8>>,
    /// Do not move the backup to the end of the device.
    no_relocate: bool,
    /// Put the backup right after the last partition.
    minimize: bool,
}

impl GptLabel {
    /// `gpt_get_nentries`.
    fn nentries(&self) -> usize {
        self.pheader
            .as_ref()
            .map_or(0, |h| usize::try_from(h.npartition_entries()).unwrap_or(0))
    }

    /// Where entry `i` starts in the array.
    fn entry_off(&self, i: usize) -> usize {
        let esz = self.pheader.as_ref().map_or(0, |h| {
            usize::try_from(h.sizeof_partition_entry()).unwrap_or(0)
        });
        esz.wrapping_mul(i)
    }

    fn ents(&self) -> &[u8] {
        self.ents.as_deref().unwrap_or(&[])
    }

    fn e_type(&self, i: usize) -> [u8; 16] {
        get16(self.ents(), self.entry_off(i).wrapping_add(ent::TYPE))
    }
    fn e_guid(&self, i: usize) -> [u8; 16] {
        get16(
            self.ents(),
            self.entry_off(i).wrapping_add(ent::PARTITION_GUID),
        )
    }
    fn e_start(&self, i: usize) -> u64 {
        le_get(
            self.ents(),
            self.entry_off(i).wrapping_add(ent::LBA_START),
            8,
        )
    }
    fn e_end(&self, i: usize) -> u64 {
        le_get(self.ents(), self.entry_off(i).wrapping_add(ent::LBA_END), 8)
    }
    fn e_attrs(&self, i: usize) -> u64 {
        le_get(self.ents(), self.entry_off(i).wrapping_add(ent::ATTRS), 8)
    }
    fn e_name(&self, i: usize) -> Vec<u8> {
        let off = self.entry_off(i).wrapping_add(ent::NAME);
        self.ents()
            .get(off..off.wrapping_add(72))
            .map(<[u8]>::to_vec)
            .unwrap_or_default()
    }

    /// `f` on entry `i`, as upstream's `struct gpt_entry` sees it: the
    /// 128 bytes at the entry's place in the array, whatever the entry size
    /// the header gives. Of a smaller entry at the end of the array, what
    /// lies in the array is written back, and what lies past it -- which
    /// upstream writes past the array's end -- is dropped.
    fn with_entry(&mut self, i: usize, f: impl FnOnce(&mut [u8])) {
        let off = self.entry_off(i);
        let Some(ents) = self.ents.as_mut() else {
            return;
        };
        let avail = ents
            .get(off..)
            .map_or(0, |rest| rest.len().min(GPT_ENTRY_SIZE));
        let end = off.wrapping_add(avail);
        let mut e = [0u8; GPT_ENTRY_SIZE];
        if let (Some(dst), Some(src)) = (e.get_mut(..avail), ents.get(off..end)) {
            dst.copy_from_slice(src);
        }
        f(&mut e);
        if let (Some(dst), Some(src)) = (ents.get_mut(off..end), e.get(..avail)) {
            dst.copy_from_slice(src);
        }
    }

    /// `gpt_entry_is_used (e)`: a type.
    fn is_used(&self, i: usize) -> bool {
        self.e_type(i) != [0u8; 16]
    }

    /// `gpt_partition_size (e)`: 0 when the end is before the start.
    fn e_size(&self, i: usize) -> u64 {
        let (s, e) = (self.e_start(i), self.e_end(i));
        if s > e {
            0
        } else {
            e.wrapping_sub(s).wrapping_add(1)
        }
    }
}

/// `swap_efi_guid (uid)`: between UEFI's order and libuuid's.
fn swap_efi_guid(g: &[u8; 16]) -> [u8; 16] {
    let mut o = *g;
    o.swap(0, 3);
    o.swap(1, 2);
    o.swap(4, 5);
    o.swap(6, 7);
    o
}

/// `string_to_guid (in, guid)`: -EINVAL for a malformed GUID.
fn string_to_guid(s: &[u8]) -> Result<[u8; 16], i32> {
    let be = crate::uuid::parse(s).ok_or(-EINVAL)?;
    Ok(swap_efi_guid(&be))
}

/// `guid_to_string (guid, out)`: upper case.
fn guid_to_string(g: &[u8; 16]) -> Vec<u8> {
    crate::uuid::unparse_upper(&swap_efi_guid(g))
}

/// `gpt_entry_set_name (e, str)`: UTF-8 to UTF-16LE, 36 units at most. The
/// bytes of `s` used, or -EILSEQ.
fn entry_set_name(gpt: &mut GptLabel, i: usize, s: &[u8]) -> Result<usize, i32> {
    /// `EILSEQ`.
    const EILSEQ: i32 = 84;
    let mut name = [0u16; GPT_PART_NAME_LEN];
    let mut k = 0usize;
    let mut mblen: u32 = 0;
    let mut used = 0usize;
    for &c in s {
        if c == 0 || k >= GPT_PART_NAME_LEN {
            break;
        }
        used = used.wrapping_add(1);
        let Some(slot) = name.get_mut(k) else { break };
        if mblen == 0 {
            if c & 0x80 == 0 {
                *slot = u16::from(c);
                k = k.wrapping_add(1);
            } else if c & 0xe0 == 0xc0 {
                mblen = 1;
                *slot = u16::from(c & 0x1f) << 6;
            } else if c & 0xf0 == 0xe0 {
                mblen = 2;
                *slot = u16::from(c & 0x0f) << 12;
            } else {
                // Broken UTF-8, or beyond U+FFFF.
                return Err(-EILSEQ);
            }
        } else {
            if c & 0xc0 != 0x80 {
                return Err(-EILSEQ);
            }
            mblen = mblen.wrapping_sub(1);
            *slot |= u16::from(c & 0x3f) << (mblen.wrapping_mul(6));
            if mblen == 0 {
                // A code point reserved for surrogate pairs.
                if *slot & 0xf800 == 0xd800 {
                    return Err(-EILSEQ);
                }
                k = k.wrapping_add(1);
            }
        }
    }
    gpt.with_entry(i, |e| {
        for (j, u) in name.iter().enumerate() {
            le_put(
                e,
                ent::NAME.wrapping_add(j.wrapping_mul(2)),
                2,
                u64::from(*u),
            );
        }
    });
    Ok(used)
}

/// `encode_to_utf8 (src, count)`: a name from UTF-16LE.
fn encode_to_utf8(src: &[u8]) -> Vec<u8> {
    let len = (src.len().wrapping_mul(3) >> 1).wrapping_add(1);
    ulblkid::encode::encode_to_utf8(ulblkid::encode::ENCODE_UTF16LE, len, src)
}

/// The current label's GPT data.
fn gpt(cxt: &Context) -> Option<&GptLabel> {
    match &cxt.label_ref()?.data {
        LabelData::Gpt(g) => Some(g),
        _ => None,
    }
}

/// The current label's GPT data, for changing.
fn gptm(cxt: &mut Context) -> Option<&mut GptLabel> {
    match &mut cxt.label_mut()?.data {
        LabelData::Gpt(g) => Some(g),
        _ => None,
    }
}

/// `gpt_calculate_sizeof_entries (hdr, nents, &sz)`: the array's bytes.
fn calculate_sizeof_entries(hdr: Option<&Hdr>, nents: u32) -> Result<usize, i32> {
    let esz = hdr.map_or(
        u32::try_from(GPT_ENTRY_SIZE).unwrap_or(128),
        Hdr::sizeof_partition_entry,
    );
    let max = usize::MAX
        .checked_div(usize::try_from(esz).unwrap_or(1))
        .unwrap_or(0);
    if nents == 0 || esz == 0 || max < usize::try_from(nents).unwrap_or(usize::MAX) {
        return Err(-ERANGE);
    }
    Ok(usize::try_from(nents)
        .unwrap_or(0)
        .wrapping_mul(usize::try_from(esz).unwrap_or(0)))
}

/// `gpt_calculate_sectorsof_entries`: the array's sectors.
fn calculate_sectorsof_entries(hdr: Option<&Hdr>, nents: u32, ss: u64) -> Result<u64, i32> {
    let esz = u64::try_from(calculate_sizeof_entries(hdr, nents)?).unwrap_or(0);
    Ok(div(esz.wrapping_add(ss).wrapping_sub(1), ss))
}

/// `gpt_calculate_alternative_entries_lba`: where the backup array goes.
fn calculate_alternative_entries_lba(
    hdr: Option<&Hdr>,
    nents: u32,
    total: u64,
    ss: u64,
) -> Result<u64, i32> {
    let esects = calculate_sectorsof_entries(hdr, nents, ss)?;
    if total < 1u64.wrapping_add(esects) {
        return Err(-ENOSPC);
    }
    Ok(total.wrapping_sub(1).wrapping_sub(esects))
}

/// `gpt_calculate_last_lba`: the last usable LBA.
fn calculate_last_lba(hdr: Option<&Hdr>, nents: u32, total: u64, ss: u64) -> Result<u64, i32> {
    let esects = calculate_sectorsof_entries(hdr, nents, ss)?;
    if total < 2u64.wrapping_add(esects) {
        return Err(-ENOSPC);
    }
    Ok(total.wrapping_sub(2).wrapping_sub(esects))
}

/// `gpt_calculate_first_lba`: the first usable LBA.
fn calculate_first_lba(hdr: Option<&Hdr>, nents: u32, ss: u64) -> Result<u64, i32> {
    Ok(calculate_sectorsof_entries(hdr, nents, ss)?.wrapping_add(2))
}

/// `gpt_sizeof_entries (hdr, &sz)`: the current array's bytes.
fn sizeof_entries(hdr: &Hdr) -> Result<usize, i32> {
    calculate_sizeof_entries(Some(hdr), hdr.npartition_entries())
}

/// `gpt_get_header_id (header)`.
fn get_header_id(h: &Hdr) -> Vec<u8> {
    guid_to_string(&h.disk_guid())
}

/// The four protective-MBR records zeroed and the first set to cover the
/// disk (what `gpt_mknew_pmbr` and `gpt_write_pmbr` both build).
fn build_pmbr(b: &mut [u8], total_sectors: u64) {
    if let Some(r) = b.get_mut(0x1be..0x1fe) {
        r.fill(0);
    }
    le_put(b, 510, 2, 0xaa55);
    let rec: usize = 0x1be;
    le_put(b, rec.wrapping_add(4), 1, u64::from(EFI_PMBR_OSTYPE));
    le_put(b, rec.wrapping_add(2), 1, 2);
    le_put(b, rec.wrapping_add(5), 1, 0xff);
    le_put(b, rec.wrapping_add(6), 1, 0xff);
    le_put(b, rec.wrapping_add(7), 1, 0xff);
    le_put(b, rec.wrapping_add(8), 4, 1);
    le_put(
        b,
        rec.wrapping_add(12),
        4,
        total_sectors.wrapping_sub(1).min(0xffff_ffff),
    );
}

/// `gpt_mknew_pmbr (cxt)`: a new protective MBR in the first sector.
fn mknew_pmbr(cxt: &mut Context) -> i32 {
    if cxt.firstsector.is_none() {
        return -ENOSYS;
    }
    let rc = if cxt.has_protected_bootbits() {
        cxt.init_firstsector_buffer(0, crate::dos::MBR_PT_BOOTBITS_SIZE)
    } else {
        cxt.init_firstsector_buffer(0, 0)
    };
    if rc != 0 {
        return rc;
    }
    let total = cxt.total_sectors;
    cxt.with_firstsector(|b| build_pmbr(b, total));
    0
}

/// `gpt_fix_alternative_lba (cxt, gpt)`: the backup header moved to the
/// end of the device.
fn fix_alternative_lba(gpt: &mut GptLabel, total: u64, ss: u64) -> Result<(), i32> {
    let (Some(p), Some(b)) = (gpt.pheader.as_mut(), gpt.bheader.as_mut()) else {
        return Err(-EINVAL);
    };
    let nents = p.npartition_entries();
    // From primary to backup...
    p.set_alternative_lba(total.wrapping_sub(1));
    // ...and back.
    b.set_alternative_lba(p.my_lba());
    b.set_my_lba(p.alternative_lba());
    let x = calculate_alternative_entries_lba(Some(p), nents, total, ss)?;
    b.set_partition_entry_lba(x);
    let x = calculate_last_lba(Some(p), nents, total, ss)?;
    p.set_last_usable_lba(x);
    b.set_last_usable_lba(x);
    Ok(())
}

/// `gpt_calculate_minimal_size (cxt, gpt)`: the smallest device that holds
/// the partitions and the backup.
fn calculate_minimal_size(gpt: &GptLabel, total: u64, ss: u64) -> u64 {
    let x = (0..gpt.nentries())
        .filter(|&i| gpt.is_used(i))
        .map(|i| gpt.e_end(i))
        .max()
        .unwrap_or(0);
    let mut sum = x.wrapping_add(1);
    let last = gpt
        .pheader
        .as_ref()
        .and_then(|h| calculate_last_lba(Some(h), h.npartition_entries(), total, ss).ok())
        .unwrap_or(0);
    sum = sum.wrapping_add(total.wrapping_sub(last));
    sum
}

/// `gpt_possible_minimize`.
fn possible_minimize(gpt: &GptLabel, total: u64, ss: u64) -> bool {
    let min = calculate_minimal_size(gpt, total, ss);
    gpt.pheader
        .as_ref()
        .is_some_and(|h| h.alternative_lba() > min.wrapping_sub(1))
}

/// `gpt_minimize_alternative_lba`: the backup right after the partitions.
fn minimize_alternative_lba(cxt: &mut Context) -> i32 {
    let (total, ss) = (cxt.total_sectors, cxt.sector_size);
    let Some(g) = gptm(cxt) else { return -EINVAL };
    let min = calculate_minimal_size(g, total, ss);
    if let Err(rc) = fix_alternative_lba(g, min, ss) {
        return rc;
    }
    cxt.set_label_changed();
    0
}

/// `gpt_mknew_header_common (cxt, header, lba)`.
fn mknew_header_common(header: &mut Hdr, lba: u64, total: u64, ss: u64) {
    header.set_my_lba(lba);
    if lba == PRIMARY_LBA {
        header.set_alternative_lba(total.wrapping_sub(1));
        header.set_partition_entry_lba(2);
    } else {
        let x =
            calculate_alternative_entries_lba(Some(header), header.npartition_entries(), total, ss)
                .unwrap_or(0);
        header.set_alternative_lba(PRIMARY_LBA);
        header.set_partition_entry_lba(x);
    }
}

/// `gpt_mknew_header_from_bkp (cxt, header, lba, header2)`.
fn mknew_header_from_bkp(header: &mut Hdr, lba: u64, h2: &Hdr, total: u64, ss: u64) {
    header.set_signature(h2.signature());
    header.set_revision(h2.revision());
    header.set_size(h2.size());
    header.set_npartition_entries(h2.npartition_entries());
    header.set_sizeof_partition_entry(h2.sizeof_partition_entry());
    header.set_first_usable_lba(h2.first_usable_lba());
    header.set_last_usable_lba(h2.last_usable_lba());
    header.set_disk_guid(&h2.disk_guid());
    mknew_header_common(header, lba, total, ss);
}

/// `gpt_copy_header (cxt, src)`: the other header, made from this one.
fn copy_header(src: &Hdr, total: u64, ss: u64) -> Hdr {
    let mut res = Hdr::zeroed(ss);
    res.set_my_lba(src.alternative_lba());
    res.set_alternative_lba(src.my_lba());
    res.set_signature(src.signature());
    res.set_revision(src.revision());
    res.set_size(src.size());
    res.set_npartition_entries(src.npartition_entries());
    res.set_sizeof_partition_entry(src.sizeof_partition_entry());
    res.set_first_usable_lba(src.first_usable_lba());
    res.set_last_usable_lba(src.last_usable_lba());
    res.set_disk_guid(&src.disk_guid());
    if res.my_lba() == PRIMARY_LBA {
        res.set_partition_entry_lba(2);
    } else {
        let esz = u64::from(src.npartition_entries()).wrapping_mul(GPT_ENTRY_SIZE as u64);
        let esects = div(esz.wrapping_add(ss).wrapping_sub(1), ss);
        res.set_partition_entry_lba(total.wrapping_sub(1).wrapping_sub(esects));
    }
    res
}

/// `get_script_u64 (cxt, &num, name)`: a size header of the script, in
/// sectors when it has a suffix. 1 when there is none.
fn get_script_u64(cxt: &Context, name: &[u8]) -> Result<u64, i32> {
    let Some(script) = cxt.script.as_ref() else {
        return Err(1);
    };
    let script = script.borrow();
    let Some(s) = script.get_header(name) else {
        return Err(1);
    };
    let (r, mut num, pwr) = ulstrutils::parse_size_power(s);
    match r {
        Err(ulstrutils::NumErr::Range) => return Err(-ERANGE),
        Err(_) => return Err(-EINVAL),
        Ok(_) => {}
    }
    if pwr != 0 {
        num = div(num, cxt.sector_size);
    }
    Ok(num)
}

/// `count_first_last_lba (cxt, &first, &last, &maxents)`: the usable range
/// for a new table, and how many entries fit.
fn count_first_last_lba(cxt: &mut Context) -> Result<(u64, u64, u32), i32> {
    let (total, ss) = (cxt.total_sectors, cxt.sector_size);
    let mut nents: u64 = GPT_NPARTITIONS;
    if cxt.script.is_some() {
        match get_script_u64(cxt, b"table-length") {
            Ok(n) => nents = n,
            Err(1) => nents = GPT_NPARTITIONS,
            Err(rc) => return Err(rc),
        }
    }
    let mut flba = 0;
    let mut llba = 0;
    let mut rc = 0;
    // A device too small for the entries: fewer, until something fits.
    while nents > 0 {
        let n32 = crate::utils::u64_as_u32(nents);
        rc = match calculate_last_lba(None, n32, total, ss) {
            Ok(l) => {
                llba = l;
                match calculate_first_lba(None, n32, ss) {
                    Ok(f) => {
                        flba = f;
                        0
                    }
                    Err(e) => e,
                }
            }
            Err(e) => e,
        };
        if llba < flba {
            rc = -ENOSPC;
        } else if rc == 0 {
            break;
        }
        nents = nents.wrapping_sub(1);
    }
    if rc != 0 {
        return Err(rc);
    }
    let mut first = 0;
    let mut last = 0;
    if cxt.script.is_some() {
        match get_script_u64(cxt, b"first-lba") {
            Ok(f) => {
                first = f;
                if first < flba || first > llba {
                    cxt.warnx(b"First LBA specified by script is out of range.");
                    return Err(-ERANGE);
                }
            }
            Err(1) => {}
            Err(e) => return Err(e),
        }
        match get_script_u64(cxt, b"last-lba") {
            Ok(l) => {
                last = l;
                if last > llba || last < flba {
                    cxt.warnx(b"Last LBA specified by script is out of range.");
                    return Err(-ERANGE);
                }
            }
            Err(1) => {}
            Err(e) => return Err(e),
        }
    }
    if last == 0 {
        last = llba;
    }
    // The default by topology.
    if first == 0 {
        first = if flba < cxt.first_lba && cxt.first_lba < last {
            cxt.first_lba
        } else {
            flba
        };
    }
    Ok((first, last, crate::utils::u64_as_u32(nents)))
}

/// `gpt_mknew_header (cxt, header, lba)`: a new header (revision 1.0).
fn mknew_header(cxt: &mut Context, header: &mut Hdr, lba: u64) -> i32 {
    header.set_signature(GPT_HEADER_SIGNATURE);
    header.set_revision(GPT_HEADER_REVISION_V1_00);
    // The header's own size, without the zeroed reserved area -- valid
    // either way, but some tools reject the whole sector.
    header.set_size(GPT_HEADER_MINSZ);
    let (first, last, nents) = match count_first_last_lba(cxt) {
        Ok(r) => r,
        Err(rc) => return rc,
    };
    header.set_npartition_entries(nents);
    header.set_sizeof_partition_entry(u32::try_from(GPT_ENTRY_SIZE).unwrap_or(128));
    header.set_first_usable_lba(first);
    header.set_last_usable_lba(last);
    mknew_header_common(header, lba, cxt.total_sectors, cxt.sector_size);

    let mut has_id = false;
    if let Some(script) = cxt.script.clone() {
        if let Some(id) = script.borrow().get_header(b"label-id") {
            if let Ok(g) = string_to_guid(id) {
                header.set_disk_guid(&g);
                has_id = true;
            }
        }
    }
    if !has_id {
        let g = swap_efi_guid(&crate::uuid::generate_random());
        header.set_disk_guid(&g);
    }
    0
}

/// `valid_pmbr (cxt)`: `GPT_MBR_PROTECTIVE`, `GPT_MBR_HYBRID`, or 0. A
/// protective MBR whose size is wrong is corrected in memory, with a
/// warning.
fn valid_pmbr(cxt: &mut Context) -> i32 {
    let fs = cxt.firstsector();
    if fs.is_empty() || le_get(&fs, 510, 2) != 0xaa55 {
        return 0;
    }
    let rec_off = |i: usize| 0x1be_usize.wrapping_add(i.wrapping_mul(16));
    let os_type = |i: usize| fs.get(rec_off(i).wrapping_add(4)).copied().unwrap_or(0);
    let Some(part) = (0..4).find(|&i| os_type(i) == EFI_PMBR_OSTYPE) else {
        return 0;
    };
    if (0..4).any(|i| os_type(i) != EFI_PMBR_OSTYPE && os_type(i) != 0) {
        return GPT_MBR_HYBRID;
    }
    let rec = rec_off(part);
    if le_get(&fs, rec.wrapping_add(8), 4) != PRIMARY_LBA {
        return GPT_MBR_PROTECTIVE;
    }
    // A protective MBR covers the disk or 2 TiB, whichever is less; some
    // tools write the 32-bit maximum regardless. A wrong size is only
    // warned about, so that an image of a smaller disk can be copied.
    let sz_lba = le_get(&fs, rec.wrapping_add(12), 4);
    let total = cxt.total_sectors;
    if sz_lba != total.wrapping_sub(1) && sz_lba != 0xffff_ffff {
        cxt.warnx(
            format!(
                "GPT PMBR size mismatch ({sz_lba} != {}) will be corrected by write.",
                total.wrapping_sub(1)
            )
            .as_bytes(),
        );
        // Corrected in memory too, so that the warning is not given again.
        cxt.with_firstsector(|b| {
            le_put(
                b,
                rec.wrapping_add(12),
                4,
                total.wrapping_sub(1).min(0xffff_ffff),
            );
        });
        cxt.set_label_changed();
    }
    GPT_MBR_PROTECTIVE
}

/// `last_lba (cxt)`: the last sector, by `fstat`.
fn last_lba(cxt: &mut Context) -> u64 {
    let meta = match cxt.dev.as_ref().map(|d| d.metadata()) {
        Some(Ok(m)) => m,
        Some(Err(e)) => {
            cxt.warn(crate::utils::errno_of(&e), b"gpt: stat() failed");
            return 0;
        }
        None => return 0,
    };
    if ulblkid::blkdev::is_block(&meta) {
        cxt.total_sectors.wrapping_sub(1)
    } else if meta.file_type().is_file() {
        div(meta.len(), cxt.sector_size).wrapping_sub(1)
    } else {
        cxt.warnx(format!("gpt: cannot handle files with mode {:o}", file_mode(&meta)).as_bytes());
        0
    }
}

/// `st_mode`, for the message.
fn file_mode(m: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.mode()
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        0
    }
}

/// `read_lba (cxt, lba, buffer, bytes)`: 0 when it all reads.
fn read_lba(cxt: &Context, lba: u64, buf: &mut [u8]) -> bool {
    cxt.read_from_device(buf, lba.wrapping_mul(cxt.sector_size)) == 0
}

/// `gpt_read_entries (cxt, header)`.
fn read_entries(cxt: &Context, header: &Hdr) -> Option<Vec<u8>> {
    let sz = sizeof_entries(header).ok()?;
    let mut v = Vec::new();
    v.try_reserve_exact(sz).ok()?;
    v.resize(sz, 0);
    (cxt.read_from_device(
        &mut v,
        header.partition_entry_lba().wrapping_mul(cxt.sector_size),
    ) == 0)
        .then_some(v)
}

/// `count_crc32 (buf, len, ex_off, ex_len)`: CRC-32 with a range taken as
/// zeros.
fn count_crc32(buf: &[u8], len: usize, ex_off: usize, ex_len: usize) -> u32 {
    let mut data = buf
        .get(..len.min(buf.len()))
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    // A length past the buffer reads what follows it in memory upstream;
    // here, zeros.
    data.resize(len, 0);
    if let Some(ex) = data.get_mut(ex_off..ex_off.wrapping_add(ex_len)) {
        ex.fill(0);
    }
    crc32::crc32_raw(!0, &data) ^ !0
}

/// `gpt_header_count_crc32 (header)`.
fn header_count_crc32(h: &Hdr) -> u32 {
    count_crc32(&h.0, usize::try_from(h.size()).unwrap_or(0), 16, 4)
}

/// `gpt_entryarr_count_crc32 (header, ents)`.
fn entryarr_count_crc32(h: &Hdr, ents: &[u8]) -> u32 {
    match sizeof_entries(h) {
        Ok(sz) => count_crc32(ents, sz, 0, 0),
        Err(_) => 0,
    }
}

/// `gpt_recompute_crc (header, ents)`.
fn recompute_crc(h: Option<&mut Hdr>, ents: &[u8]) {
    if let Some(h) = h {
        let c = entryarr_count_crc32(h, ents);
        h.set_partition_entry_array_crc32(c);
        let c = header_count_crc32(h);
        h.set_crc32(c);
    }
}

/// Both headers' CRCs recomputed.
fn recompute_both(g: &mut GptLabel) {
    let ents = g.ents.clone().unwrap_or_default();
    recompute_crc(g.pheader.as_mut(), &ents);
    recompute_crc(g.bheader.as_mut(), &ents);
}

/// `gpt_check_header_crc (header, ents)`: 1 when valid -- after
/// recomputing the array's CRC, when `ents` is given, in case the array
/// changed.
fn check_header_crc(h: &mut Hdr, ents: Option<&[u8]>) -> bool {
    let orgcrc = h.crc32();
    if header_count_crc32(h) == orgcrc {
        return true;
    }
    if let Some(ents) = ents {
        recompute_crc(Some(h), ents);
        return header_count_crc32(h) == orgcrc;
    }
    false
}

/// `gpt_check_entryarr_crc (header, ents)`.
fn check_entryarr_crc(h: &Hdr, ents: &[u8]) -> bool {
    entryarr_count_crc32(h, ents) == h.partition_entry_array_crc32()
}

/// `gpt_check_lba_sanity (cxt, header)`.
fn check_lba_sanity(cxt: &mut Context, h: &Hdr) -> bool {
    let lastlba = last_lba(cxt);
    let fu = h.first_usable_lba();
    let lu = h.last_usable_lba();
    if lu < fu {
        return false;
    }
    if fu > lastlba || lu > lastlba {
        return false;
    }
    // The header has to be outside the usable range.
    !(fu < PRIMARY_LBA && PRIMARY_LBA < lu)
}

/// `gpt_read_header (cxt, lba, &ents)`: a header that passes every check,
/// with its entries.
fn read_header(cxt: &mut Context, lba: u64) -> Option<(Hdr, Vec<u8>)> {
    let mut h = Hdr::zeroed(cxt.sector_size);
    if !read_lba(cxt, lba, &mut h.0) {
        return None;
    }
    if h.signature() != GPT_HEADER_SIGNATURE {
        return None;
    }
    let hsz = h.size();
    if hsz < GPT_HEADER_MINSZ || u64::from(hsz) > cxt.sector_size {
        return None;
    }
    if !check_header_crc(&mut h, None) {
        return None;
    }
    let ents = read_entries(cxt, &h)?;
    if !check_entryarr_crc(&h, &ents) {
        return None;
    }
    if !check_lba_sanity(cxt, &h) {
        return None;
    }
    if h.my_lba() != lba {
        return None;
    }
    Some((h, ents))
}

/// `gpt_locate_disklabel (cxt, n, ...)`.
fn gpt_locate_disklabel(cxt: &mut Context, n: i32) -> Result<Located, i32> {
    let ss = cxt.sector_size;
    let g = gpt(cxt);
    match n {
        0 => Ok(("PMBR", 0, 512)),
        1 => Ok(("GPT Header", PRIMARY_LBA.wrapping_mul(ss), GPT_HEADER_SIZE)),
        2 => {
            let h = g.and_then(|g| g.pheader.as_ref()).ok_or(-EINVAL)?;
            Ok((
                "GPT Entries",
                h.partition_entry_lba().wrapping_mul(ss),
                sizeof_entries(h)?,
            ))
        }
        3 => {
            let h = g.and_then(|g| g.bheader.as_ref()).ok_or(-EINVAL)?;
            Ok((
                "GPT Backup Entries",
                h.partition_entry_lba().wrapping_mul(ss),
                sizeof_entries(h)?,
            ))
        }
        4 => {
            let h = g.and_then(|g| g.pheader.as_ref()).ok_or(-EINVAL)?;
            Ok((
                "GPT Backup Header",
                h.alternative_lba().wrapping_mul(ss),
                GPT_HEADER_SIZE,
            ))
        }
        _ => Err(1),
    }
}

/// `GPT_LABELITEM_*`.
pub mod item {
    pub const FIRSTLBA: i32 = crate::NLABELITEMS;
    pub const LASTLBA: i32 = crate::NLABELITEMS + 1;
    pub const ALTLBA: i32 = crate::NLABELITEMS + 2;
    pub const ENTRIESLBA: i32 = crate::NLABELITEMS + 3;
    pub const ENTRIESALLOC: i32 = crate::NLABELITEMS + 4;
    pub const ENTRIESLASTLBA: i32 = crate::NLABELITEMS + 5;
}

/// `gpt_get_disklabel_item (cxt, item)`.
fn gpt_get_disklabel_item(cxt: &mut Context, it: &mut LabelItem) -> i32 {
    let ss = cxt.sector_size;
    let Some(h) = gpt(cxt).and_then(|g| g.pheader.clone()) else {
        return -EINVAL;
    };
    let (name, data) = match it.id {
        crate::LABELITEM_ID => ("Disk identifier", ItemData::Str(get_header_id(&h))),
        item::FIRSTLBA => ("First usable LBA", ItemData::U64(h.first_usable_lba())),
        item::LASTLBA => ("Last usable LBA", ItemData::U64(h.last_usable_lba())),
        item::ALTLBA => ("Alternative LBA", ItemData::U64(h.alternative_lba())),
        item::ENTRIESLBA => (
            "Partition entries starting LBA",
            ItemData::U64(h.partition_entry_lba()),
        ),
        item::ENTRIESLASTLBA => {
            let x = calculate_sectorsof_entries(Some(&h), h.npartition_entries(), ss).unwrap_or(0);
            (
                "Partition entries ending LBA",
                ItemData::U64(h.partition_entry_lba().wrapping_add(x).wrapping_sub(1)),
            )
        }
        item::ENTRIESALLOC => (
            "Allocated partition entries",
            ItemData::U64(u64::from(h.npartition_entries())),
        ),
        id => return if id < crate::NLABELITEMS { 1 } else { 2 },
    };
    it.name = Some(name);
    it.data = data;
    0
}

/// `partitions_in_use (gpt)`.
fn partitions_in_use(g: &GptLabel) -> usize {
    (0..g.nentries()).filter(|&i| g.is_used(i)).count()
}

/// `check_too_big_partitions (gpt, sectors)`: the first (from 1) that ends
/// past the disk, or 0.
fn check_too_big_partitions(g: &GptLabel, sectors: u64) -> usize {
    (0..g.nentries())
        .find(|&i| g.is_used(i) && g.e_end(i) >= sectors)
        .map_or(0, |i| i.wrapping_add(1))
}

/// `check_start_after_end_partitions (gpt)`.
fn check_start_after_end_partitions(g: &GptLabel) -> usize {
    (0..g.nentries())
        .find(|&i| g.is_used(i) && g.e_start(i) > g.e_end(i))
        .map_or(0, |i| i.wrapping_add(1))
}

/// `partition_overlap (e1, e2)`.
fn partition_overlap(g: &GptLabel, a: usize, b: usize) -> bool {
    let (s1, e1, s2, e2) = (g.e_start(a), g.e_end(a), g.e_start(b), g.e_end(b));
    s1 != 0 && s2 != 0 && (s1 <= e2) != (e1 < s2)
}

/// `check_overlap_partitions (gpt)`: the first (from 1) that overlaps an
/// earlier one, or 0.
fn check_overlap_partitions(g: &GptLabel) -> usize {
    for i in 0..g.nentries() {
        for j in 0..i {
            if g.is_used(i) && g.is_used(j) && partition_overlap(g, i, j) {
                return i.wrapping_add(1);
            }
        }
    }
    0
}

/// `find_first_available (gpt, start)`: the first free sector from
/// `start`, or 0.
fn find_first_available(g: &GptLabel, start: u64) -> u64 {
    let Some(h) = g.pheader.as_ref() else {
        return 0;
    };
    let (fu, lu) = (h.first_usable_lba(), h.last_usable_lba());
    let mut first = if start < fu { fu } else { start };
    loop {
        let mut moved = false;
        for i in 0..g.nentries() {
            if !g.is_used(i) || first < g.e_start(i) {
                continue;
            }
            if first <= g.e_end(i) {
                first = g.e_end(i).wrapping_add(1);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    if first > lu { 0 } else { first }
}

/// `find_last_free (gpt, start)`: the end of the free space at `start`.
fn find_last_free(g: &GptLabel, start: u64) -> u64 {
    let mut nearest = g.pheader.as_ref().map_or(0, Hdr::last_usable_lba);
    for i in 0..g.nentries() {
        let ps = g.e_start(i);
        if nearest > ps && ps > start {
            nearest = ps.wrapping_sub(1);
        }
    }
    nearest
}

/// `find_last_free_sector (gpt)`: the last free sector on the disk.
fn find_last_free_sector(g: &GptLabel) -> u64 {
    let mut last = g.pheader.as_ref().map_or(0, Hdr::last_usable_lba);
    loop {
        let mut moved = false;
        for i in 0..g.nentries() {
            if last >= g.e_start(i) && last <= g.e_end(i) {
                last = g.e_start(i).wrapping_sub(1);
                moved = true;
            }
        }
        if !moved {
            return last;
        }
    }
}

/// `find_first_in_largest (gpt)`: the start of the largest free space.
fn find_first_in_largest(g: &GptLabel) -> u64 {
    let mut start = 0;
    let mut selected_size = 0;
    let mut selected = 0;
    loop {
        let first = find_first_available(g, start);
        if first == 0 {
            return selected;
        }
        let last = find_last_free(g, first);
        let size = last.wrapping_sub(first).wrapping_add(1);
        if size > selected_size {
            selected_size = size;
            selected = first;
        }
        start = last.wrapping_add(1);
    }
}

/// `get_free_sectors (cxt, gpt, &nsegments, &largest)`: the free sectors,
/// the number of free spaces, the largest.
fn get_free_sectors(g: &GptLabel, total: u64) -> (u64, u32, u64) {
    let (mut num, mut largest, mut tot, mut start) = (0u32, 0u64, 0u64, 0u64);
    if total == 0 {
        return (0, 0, 0);
    }
    loop {
        let first = find_first_available(g, start);
        if first == 0 {
            return (tot, num, largest);
        }
        let last = find_last_free(g, first);
        let sz = last.wrapping_sub(first).wrapping_add(1);
        if sz > largest {
            largest = sz;
        }
        tot = tot.wrapping_add(sz);
        num = num.wrapping_add(1);
        start = last.wrapping_add(1);
    }
}

/// `gpt_probe_label (cxt)`.
fn gpt_probe_label(cxt: &mut Context) -> i32 {
    if valid_pmbr(cxt) == 0 {
        gpt_deinit_cxt(cxt);
        return 0;
    }
    let (total, ss) = (cxt.total_sectors, cxt.sector_size);
    let mut ents = None;
    let primary = read_header(cxt, PRIMARY_LBA);
    let mut pheader = None;
    let mut bheader = None;
    if let Some((h, e)) = primary {
        let alt = h.alternative_lba();
        pheader = Some(h);
        ents = Some(e);
        // The primary is good: the backup from its alternative LBA.
        bheader = read_header(cxt, alt).map(|(b, _)| b);
    } else {
        // The primary is corrupt: the backup from the last LBA.
        let last = last_lba(cxt);
        if let Some((b, e)) = read_header(cxt, last) {
            bheader = Some(b);
            ents = Some(e);
        }
    }
    if pheader.is_none() && bheader.is_none() {
        gpt_deinit_cxt(cxt);
        return 0;
    }
    let ents_v = ents.clone().unwrap_or_default();
    let mut changed = false;
    match (&pheader, &bheader) {
        (Some(p), None) => {
            cxt.warnx(b"The backup GPT table is corrupt, but the primary appears OK, so that will be used.");
            let mut b = copy_header(p, total, ss);
            recompute_crc(Some(&mut b), &ents_v);
            bheader = Some(b);
            changed = true;
        }
        (None, Some(b)) => {
            cxt.warnx(b"The primary GPT table is corrupt, but the backup appears OK, so that will be used.");
            let mut p = copy_header(b, total, ss);
            recompute_crc(Some(&mut p), &ents_v);
            pheader = Some(p);
            changed = true;
        }
        _ => {}
    }
    if let Some(g) = gptm(cxt) {
        g.pheader = pheader;
        g.bheader = bheader;
        g.ents = ents;
    }
    if changed {
        cxt.set_label_changed();
    }

    // The headers may be right and the backup still not at the end of the
    // device (it was resized, say).
    let (minimize, no_relocate, alt) = match gpt(cxt) {
        Some(g) => (
            g.minimize,
            g.no_relocate,
            g.pheader.as_ref().map_or(0, Hdr::alternative_lba),
        ),
        None => return 0,
    };
    if !minimize && (alt > total || alt < total.wrapping_sub(1)) {
        if no_relocate || cxt.is_readonly() {
            cxt.warnx(b"The backup GPT table is not on the end of the device.");
        } else {
            cxt.warnx(b"The backup GPT table is not on the end of the device. This problem will be corrected by write.");
            let failed = gptm(cxt).is_none_or(|g| fix_alternative_lba(g, total, ss).is_err());
            if failed {
                cxt.warnx(b"Failed to recalculate backup GPT table location");
            }
            if let Some(g) = gptm(cxt) {
                recompute_both(g);
            }
            cxt.set_label_changed();
        }
    }
    if minimize && gpt(cxt).is_some_and(|g| possible_minimize(g, total, ss)) {
        cxt.set_label_changed();
    }
    let (nmax, ncur) = gpt(cxt).map_or((0, 0), |g| (g.nentries(), partitions_in_use(g)));
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = nmax;
        lb.nparts_cur = ncur;
    }
    1
}

/// `gpt_entry_attrs_to_string (e, &res)`.
fn attrs_to_string(attrs: u64) -> Option<Vec<u8>> {
    if attrs == 0 {
        return None;
    }
    let isset = |bit: u32| attrs & (1u64 << bit) != 0;
    let mut out = String::new();
    if isset(ATTRBIT_REQ) {
        out.push_str(ATTRSTR_REQ);
    }
    if isset(ATTRBIT_NOBLOCK) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(ATTRSTR_NOBLOCK);
    }
    if isset(ATTRBIT_LEGACY) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(ATTRSTR_LEGACY);
    }
    let mut count: u32 = 0;
    for n in ATTRBIT_GUID_FIRST..ATTRBIT_GUID_FIRST.wrapping_add(ATTRBIT_GUID_COUNT) {
        if !isset(n) {
            continue;
        }
        if count == 0 {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("GUID:{n}"));
        } else {
            out.push_str(&format!(",{n}"));
        }
        count = count.wrapping_add(1);
    }
    Some(out.into_bytes())
}

/// `gpt_entry_attrs_from_string (cxt, e, str)`.
fn attrs_from_string(cxt: &mut Context, s: &[u8]) -> Result<u64, i32> {
    let mut attrs: u64 = 0;
    let isblank = |c: u8| c == b' ' || c == b'\t';
    let mut p = 0usize;
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    while at(p) != 0 {
        let mut bit: i64 = -1;
        while isblank(at(p)) {
            p = p.wrapping_add(1);
        }
        if at(p) == 0 {
            break;
        }
        let rest = s.get(p..).unwrap_or(&[]);
        if rest.starts_with(ATTRSTR_REQ.as_bytes()) {
            bit = i64::from(ATTRBIT_REQ);
            p = p.wrapping_add(ATTRSTR_REQ.len());
        } else if rest.starts_with(ATTRSTR_REQ_TYPO.as_bytes()) {
            bit = i64::from(ATTRBIT_REQ);
            p = p.wrapping_add(ATTRSTR_REQ_TYPO.len());
        } else if rest.starts_with(ATTRSTR_LEGACY.as_bytes()) {
            bit = i64::from(ATTRBIT_LEGACY);
            p = p.wrapping_add(ATTRSTR_LEGACY.len());
        } else if rest.starts_with(ATTRSTR_NOBLOCK.as_bytes()) {
            bit = i64::from(ATTRBIT_NOBLOCK);
            p = p.wrapping_add(ATTRSTR_NOBLOCK.len());
        } else if at(p).is_ascii_digit()
            || (rest.starts_with(b"GUID:") && at(p.wrapping_add(5)).is_ascii_digit())
        {
            // GUID:<bit>, or <bit>.
            if at(p) == b'G' {
                p = p.wrapping_add(5);
            }
            let (v, end, err) = strtol(s.get(p..).unwrap_or(&[]), 0);
            let v = i64::from(trunc_i32(v));
            if err != 0
                || end == 0 && p == 0
                || v < i64::from(ATTRBIT_GUID_FIRST)
                || v >= i64::from(ATTRBIT_GUID_FIRST.wrapping_add(ATTRBIT_GUID_COUNT))
            {
                bit = -1;
            } else {
                bit = v;
                p = p.wrapping_add(end);
            }
        }
        if bit < 0 {
            let mut m = b"unsupported GPT attribute bit '".to_vec();
            m.extend_from_slice(crate::utils::c_str(s.get(p..).unwrap_or(&[])));
            m.push(b'\'');
            cxt.warnx(&m);
            return Err(-EINVAL);
        }
        if at(p) != 0 && at(p) != b',' && !isblank(at(p)) {
            let mut m = b"failed to parse GPT attribute string '".to_vec();
            m.extend_from_slice(crate::utils::c_str(s));
            m.push(b'\'');
            cxt.warnx(&m);
            return Err(-EINVAL);
        }
        attrs |= 1u64 << u32::try_from(bit).unwrap_or(0);
        while isblank(at(p)) {
            p = p.wrapping_add(1);
        }
        if at(p) == b',' {
            p = p.wrapping_add(1);
        }
    }
    Ok(attrs)
}

/// `gpt_partition_parttype (cxt, e)`.
fn partition_parttype(cxt: &Context, ty: &[u8; 16]) -> PartType {
    let s = guid_to_string(ty);
    cxt.label_ref()
        .and_then(|lb| lb.get_parttype_from_string(&s))
        .cloned()
        .unwrap_or_else(|| PartType::new_unknown(0, Some(&s)))
}

/// `gpt_get_partition (cxt, n, pa)`.
fn gpt_get_partition(cxt: &mut Context, n: usize, pa: &mut Partition) -> i32 {
    let Some(g) = gpt(cxt) else { return -EINVAL };
    if n >= g.nentries() {
        return -EINVAL;
    }
    pa.used = g.is_used(n) || g.e_start(n) != 0;
    if !pa.used {
        return 0;
    }
    pa.start = g.e_start(n);
    pa.size = g.e_size(n);
    let ty = g.e_type(n);
    let guid = g.e_guid(n);
    let attrs = g.e_attrs(n);
    let name = g.e_name(n);
    pa.ty = Some(partition_parttype(cxt, &ty));
    pa.uuid = Some(guid_to_string(&guid));
    pa.attrs = attrs_to_string(attrs);
    pa.name = Some(encode_to_utf8(&name));
    0
}

/// `gpt_set_partition (cxt, n, pa)`.
fn gpt_set_partition(cxt: &mut Context, n: usize, pa: &Partition) -> i32 {
    let Some(g) = gpt(cxt) else { return -EINVAL };
    if n >= g.nentries() {
        return -EINVAL;
    }
    if let Some(u) = pa.uuid.as_deref() {
        let old = guid_to_string(&g.e_guid(n));
        let uuid = match string_to_guid(u) {
            Ok(u) => u,
            Err(rc) => return rc,
        };
        if let Some(g) = gptm(cxt) {
            g.with_entry(n, |e| put16(e, ent::PARTITION_GUID, &uuid));
        }
        let new = guid_to_string(&uuid);
        let mut m = b"Partition UUID changed from ".to_vec();
        m.extend_from_slice(&old);
        m.extend_from_slice(b" to ");
        m.extend_from_slice(&new);
        m.push(b'.');
        cxt.info(&m);
    }
    if let Some(name) = pa.name.as_deref() {
        let old = gpt(cxt)
            .map(|g| encode_to_utf8(&g.e_name(n)))
            .unwrap_or_default();
        let r = gptm(cxt).map_or(Err(-EINVAL), |g| entry_set_name(g, n, name));
        match r {
            Err(rc) => {
                cxt.warn(
                    rc.wrapping_neg(),
                    b"Failed to translate partition name, name not changed.",
                );
            }
            Ok(len) => {
                let mut m = b"Partition name changed from '".to_vec();
                m.extend_from_slice(&old);
                m.extend_from_slice(b"' to '");
                m.extend_from_slice(name.get(..len).unwrap_or(name));
                m.extend_from_slice(b"'.");
                cxt.info(&m);
            }
        }
    }
    if let Some(ts) = pa.ty.as_ref().and_then(|t| t.typestr.as_deref()) {
        let typeid = match string_to_guid(ts) {
            Ok(t) => t,
            Err(rc) => return rc,
        };
        if let Some(g) = gptm(cxt) {
            g.with_entry(n, |e| put16(e, ent::TYPE, &typeid));
        }
    }
    if let Some(a) = pa.attrs.as_deref() {
        match attrs_from_string(cxt, a) {
            Ok(v) => {
                if let Some(g) = gptm(cxt) {
                    g.with_entry(n, |e| le_put(e, ent::ATTRS, 8, v));
                }
            }
            Err(rc) => return rc,
        }
    }
    let Some(g) = gpt(cxt) else { return -EINVAL };
    let start = if pa.has_start() {
        pa.start
    } else {
        crate::UNDEF
    };
    let mut end = crate::UNDEF;
    if pa.has_size() || pa.has_start() {
        let xstart = if pa.has_start() {
            pa.start
        } else {
            g.e_start(n)
        };
        let xsize = if pa.has_size() { pa.size } else { g.e_size(n) };
        end = xstart.wrapping_add(xsize).wrapping_sub(1);
    }
    let (fu, lu) = g
        .pheader
        .as_ref()
        .map_or((0, 0), |h| (h.first_usable_lba(), h.last_usable_lba()));
    if !is_undef(start) {
        if start < fu {
            cxt.warnx(b"The start of the partition understeps FirstUsableLBA.");
            return -EINVAL;
        }
        if let Some(g) = gptm(cxt) {
            g.with_entry(n, |e| le_put(e, ent::LBA_START, 8, start));
        }
    }
    if !is_undef(end) {
        if end > lu {
            cxt.warnx(b"The end of the partition oversteps LastUsableLBA.");
            return -EINVAL;
        }
        if let Some(g) = gptm(cxt) {
            g.with_entry(n, |e| le_put(e, ent::LBA_END, 8, end));
        }
    }
    if let Some(g) = gptm(cxt) {
        recompute_both(g);
    }
    cxt.set_label_changed();
    0
}

/// `gpt_write (cxt, offset, buf, count)`: written and synced.
fn gpt_write(cxt: &Context, offset: u64, buf: &[u8]) -> i32 {
    let rc = cxt.write_to_device(buf, offset);
    if rc != 0 {
        return rc;
    }
    match cxt.dev.as_ref().map(|d| d.sync_all()) {
        Some(Err(e)) => crate::utils::neg_errno(&e),
        _ => 0,
    }
}

/// `gpt_write_pmbr (cxt)`: the protective MBR, unless the same one is on
/// the disk.
fn write_pmbr(cxt: &mut Context) -> i32 {
    let total = cxt.total_sectors;
    cxt.with_firstsector(|b| build_pmbr(b, total));
    let fs = cxt.firstsector();
    // `sizeof (struct gpt_legacy_mbr)`.
    let mut current = vec![0u8; 512];
    if cxt.read_from_device(&mut current, 0) == 0 && fs.get(..512) == Some(current.as_slice()) {
        return 0;
    }
    gpt_write(cxt, 0, &fs)
}

/// `gpt_write_disklabel (cxt)`: backup array, backup header, primary
/// array, primary header, protective MBR -- in that order, which UEFI
/// requires; stopping at the first failure.
fn gpt_write_disklabel(cxt: &mut Context) -> i32 {
    let mbr_type = valid_pmbr(cxt);
    let total = cxt.total_sectors;
    let Some(g) = gpt(cxt) else { return -EINVAL };
    let alt = g.pheader.as_ref().map_or(0, Hdr::alternative_lba);
    // The disk has to hold the backup, and the backup has to be at its end.
    if alt > total || alt < total.wrapping_sub(1) || check_overlap_partitions(g) != 0 {
        return -EINVAL;
    }
    if g.minimize {
        let _ = minimize_alternative_lba(cxt);
    }
    let Some(g) = gptm(cxt) else { return -EINVAL };
    recompute_both(g);
    let g = g.clone();
    let (Some(p), Some(b)) = (g.pheader.as_ref(), g.bheader.as_ref()) else {
        return -EINVAL;
    };
    let ents = g.ents.clone().unwrap_or_default();
    let ss = cxt.sector_size;

    let write_parts = |cxt: &Context, h: &Hdr| -> i32 {
        match sizeof_entries(h) {
            Ok(sz) => gpt_write(
                cxt,
                h.partition_entry_lba().wrapping_mul(ss),
                ents.get(..sz).unwrap_or(&ents),
            ),
            Err(rc) => rc,
        }
    };
    let ssz = usize::try_from(ss).unwrap_or(GPT_HEADER_SIZE);
    let rc = write_parts(cxt, b);
    if rc != 0 {
        return rc;
    }
    let rc = gpt_write(
        cxt,
        p.alternative_lba().wrapping_mul(ss),
        b.0.get(..ssz).unwrap_or(&b.0),
    );
    if rc != 0 {
        return rc;
    }
    let rc = write_parts(cxt, p);
    if rc != 0 {
        return rc;
    }
    let rc = gpt_write(
        cxt,
        PRIMARY_LBA.wrapping_mul(ss),
        p.0.get(..ssz).unwrap_or(&p.0),
    );
    if rc != 0 {
        return rc;
    }
    if mbr_type == GPT_MBR_HYBRID {
        cxt.warnx(b"The device contains hybrid MBR -- writing GPT only.");
        return 0;
    }
    write_pmbr(cxt)
}

/// `gpt_verify_disklabel (cxt)`.
fn gpt_verify_disklabel(cxt: &mut Context) -> i32 {
    let Some(mut g) = gpt(cxt).cloned() else {
        return -EINVAL;
    };
    let mut nerror: i32 = 0;
    let ents = g.ents.clone().unwrap_or_default();
    if g.bheader.is_none() {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(b"Disk does not contain a valid backup header.");
    }
    if let Some(p) = g.pheader.as_mut() {
        if !check_header_crc(p, Some(&ents)) {
            nerror = nerror.wrapping_add(1);
            cxt.warnx(b"Invalid primary header CRC checksum.");
        }
    }
    if let Some(b) = g.bheader.as_mut() {
        if !check_header_crc(b, Some(&ents)) {
            nerror = nerror.wrapping_add(1);
            cxt.warnx(b"Invalid backup header CRC checksum.");
        }
    }
    // The header CRC checks recompute stale CRCs, as upstream's do, in the
    // label itself.
    if let Some(l) = gptm(cxt) {
        l.pheader.clone_from(&g.pheader);
        l.bheader.clone_from(&g.bheader);
    }
    let Some(p) = g.pheader.clone() else {
        return -EINVAL;
    };
    if !check_entryarr_crc(&p, &ents) {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(b"Invalid partition entry checksum.");
    }
    if !check_lba_sanity(cxt, &p) {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(b"Invalid primary header LBA sanity checks.");
    }
    if let Some(b) = g.bheader.clone() {
        if !check_lba_sanity(cxt, &b) {
            nerror = nerror.wrapping_add(1);
            cxt.warnx(b"Invalid backup header LBA sanity checks.");
        }
    }
    if p.my_lba() != PRIMARY_LBA {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(b"MyLBA mismatch with real position at primary header.");
    }
    if let Some(b) = g.bheader.clone() {
        if b.my_lba() != last_lba(cxt) {
            nerror = nerror.wrapping_add(1);
            cxt.warnx(b"MyLBA mismatch with real position at backup header.");
        }
    }
    if p.alternative_lba() >= cxt.total_sectors {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(b"Disk is too small to hold all data.");
    }
    // The primary's alternate LBA has to be the backup.
    if g.bheader
        .as_ref()
        .is_some_and(|b| p.my_lba() != b.alternative_lba())
    {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(b"Primary and backup header mismatch.");
    }
    let ptnum = check_overlap_partitions(&g);
    if ptnum != 0 {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(
            format!(
                "Partition {ptnum} overlaps with partition {}.",
                ptnum.wrapping_add(1)
            )
            .as_bytes(),
        );
    }
    let ptnum = check_too_big_partitions(&g, cxt.total_sectors);
    if ptnum != 0 {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(format!("Partition {ptnum} is too big for the disk.").as_bytes());
    }
    let ptnum = check_start_after_end_partitions(&g);
    if ptnum != 0 {
        nerror = nerror.wrapping_add(1);
        cxt.warnx(format!("Partition {ptnum} ends before it starts.").as_bytes());
    }

    if nerror == 0 {
        cxt.info(b"No errors detected.");
        cxt.info(format!("Header version: {}", get_header_revstr(Some(&p))).as_bytes());
        cxt.info(
            format!(
                "Using {} out of {} partitions.",
                partitions_in_use(&g),
                g.nentries()
            )
            .as_bytes(),
        );
        let (free, nsegments, largest) = get_free_sectors(&g, cxt.total_sectors);
        let strsz = (largest != 0).then(|| {
            ulstrutils::size_to_human_string(
                ulstrutils::SIZE_SUFFIX_SPACE | ulstrutils::SIZE_SUFFIX_3LETTER,
                largest.wrapping_mul(cxt.sector_size),
            )
        });
        let m = if nsegments == 1 {
            format!("A total of {free} free sectors is available in {nsegments} segment.")
        } else {
            format!(
                "A total of {free} free sectors is available in {nsegments} segments (the largest is {}).",
                strsz.as_deref().unwrap_or("0 B")
            )
        };
        cxt.info(m.as_bytes());
    } else {
        cxt.warnx(
            if nerror == 1 {
                format!("{nerror} error detected.")
            } else {
                format!("{nerror} errors detected.")
            }
            .as_bytes(),
        );
    }
    nerror
}

/// `gpt_get_header_revstr (header)`.
fn get_header_revstr(h: Option<&Hdr>) -> &'static str {
    match h.map(Hdr::revision) {
        Some(GPT_HEADER_REVISION_V1_02) => "1.2",
        Some(GPT_HEADER_REVISION_V1_00) => "1.0",
        Some(GPT_HEADER_REVISION_V0_99) => "0.99",
        _ => "unknown",
    }
}

/// `gpt_delete_partition (cxt, partnum)`.
fn gpt_delete_partition(cxt: &mut Context, partnum: usize) -> i32 {
    let nmax = cxt.label_ref().map_or(0, |l| l.nparts_max);
    if partnum >= nmax {
        return -EINVAL;
    }
    let Some(g) = gptm(cxt) else { return -EINVAL };
    if !g.is_used(partnum) {
        return -EINVAL;
    }
    // Zeroed: `sizeof_partition_entry` bytes.
    let off = g.entry_off(partnum);
    let esz = g.pheader.as_ref().map_or(0, |h| {
        usize::try_from(h.sizeof_partition_entry()).unwrap_or(0)
    });
    if let Some(e) = g
        .ents
        .as_mut()
        .and_then(|e| e.get_mut(off..off.wrapping_add(esz)))
    {
        e.fill(0);
    }
    recompute_both(g);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = lb.nparts_cur.wrapping_sub(1);
        lb.changed = true;
    }
    0
}

/// `alignment_required (cxt)`.
fn alignment_required(cxt: &Context) -> bool {
    cxt.grain != cxt.sector_size
}

/// `gpt_add_partition (cxt, pa, &partno)`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's one function, kept whole to compare"
)]
fn gpt_add_partition(
    cxt: &mut Context,
    pa: Option<&mut Partition>,
    partno: Option<&mut usize>,
) -> i32 {
    let pa = pa.map(|p| &*p);
    let partnum = match cxt.partition_next_partno(pa) {
        Ok(n) => n,
        Err(rc) => return rc,
    };
    let Some(g) = gpt(cxt).cloned() else {
        return -EINVAL;
    };
    if g.is_used(partnum) {
        cxt.warnx(
            format!(
                "Partition {} is already defined.  Delete it before re-adding it.",
                partnum.wrapping_add(1)
            )
            .as_bytes(),
        );
        return -ERANGE;
    }
    if g.nentries() == partitions_in_use(&g) {
        cxt.warnx(b"All partitions are already in use.");
        return -ENOSPC;
    }
    if get_free_sectors(&g, cxt.total_sectors).0 == 0 {
        cxt.warnx(b"No free sectors available.");
        return -ENOSPC;
    }
    let typestr = pa
        .and_then(|p| p.ty.as_ref())
        .and_then(|t| t.typestr.as_deref())
        .unwrap_or(GPT_DEFAULT_ENTRY_TYPE);
    let typeid = match string_to_guid(typestr) {
        Ok(t) => t,
        Err(rc) => return rc,
    };
    let Some(ph) = g.pheader.clone() else {
        return -EINVAL;
    };
    let grain_s = div(cxt.grain, cxt.sector_size);

    let mut disk_f = find_first_available(&g, ph.first_usable_lba());
    // Without an explicit start, small gaps before the first partition are
    // ignored.
    if pa.is_none_or(|p| !p.has_start()) && g.is_used(0) && disk_f < g.e_start(0) {
        loop {
            disk_f = find_first_available(&g, disk_f);
            if disk_f == 0 {
                break;
            }
            let x = find_last_free(&g, disk_f);
            if x.wrapping_sub(disk_f) >= grain_s {
                break;
            }
            disk_f = x.wrapping_add(1);
        }
        if disk_f == 0 {
            disk_f = find_first_available(&g, ph.first_usable_lba());
        }
    }
    let disk_l = find_last_free_sector(&g);

    // The default is the largest free space...
    let mut dflt_f = find_first_in_largest(&g);
    let mut dflt_l = find_last_free(&g, dflt_f);
    // ...unless it is too small (a script may still ask for it).
    if pa.is_none_or(|p| !p.has_start()) && dflt_l.wrapping_sub(dflt_f).wrapping_add(1) < grain_s {
        cxt.warnx(b"No enough free sectors available.");
        return -ENOSPC;
    }
    dflt_f = cxt.align_lba_in_range(dflt_f, dflt_f, dflt_l);

    // The first sector.
    let user_f = if pa.is_some_and(|p| p.start_follow_default) {
        dflt_f
    } else if let Some(p) = pa.filter(|p| p.has_start()) {
        if p.start != find_first_available(&g, p.start) {
            cxt.warnx(format!("Sector {} already used.", p.start).as_bytes());
            return -ERANGE;
        }
        p.start
    } else {
        loop {
            let mut ask = crate::Ask::new(crate::AskType::Number);
            ask.set_query(b"First sector");
            ask.number_set_low(disk_f);
            ask.number_set_default(dflt_f);
            ask.number_set_high(disk_l);
            let rc = cxt.do_ask(&mut ask);
            if rc != 0 {
                return rc;
            }
            let f = ask.number_get_result();
            if f != find_first_available(&g, f) {
                cxt.warnx(format!("Sector {f} already used.").as_bytes());
                continue;
            }
            break f;
        }
    };

    // The last sector.
    let max_l = find_last_free(&g, user_f);
    dflt_l = max_l;
    // A partition at the end of the disk is given an aligned size by
    // default: LastUsableLBA may be unaligned where logical and physical
    // sectors differ.
    if max_l == ph.last_usable_lba() {
        dflt_l = cxt.align_lba_in_range(max_l, user_f, max_l).wrapping_sub(1);
    }
    let user_l = if pa.is_some_and(|p| p.end_follow_default) {
        dflt_l
    } else if let Some(p) = pa.filter(|p| p.has_size()) {
        let mut l = user_f.wrapping_add(p.size).wrapping_sub(1);
        if l != dflt_l
            && !p.size_explicit
            && alignment_required(cxt)
            && l.wrapping_sub(user_f) > grain_s
        {
            l = cxt.align_lba_in_range(l, user_f, dflt_l);
            if l > user_f {
                l = l.wrapping_sub(1);
            }
        }
        l
    } else {
        loop {
            let mut ask = crate::Ask::new(crate::AskType::Offset);
            ask.set_query(b"Last sector, +/-sectors or +/-size{K,M,G,T,P}");
            ask.number_set_low(user_f);
            ask.number_set_default(dflt_l);
            ask.number_set_high(max_l);
            ask.number_set_base(user_f);
            ask.number_set_unit(cxt.sector_size);
            ask.number_set_wrap_negative(true);
            let rc = cxt.do_ask(&mut ask);
            if rc != 0 {
                return rc;
            }
            let mut l = ask.number_get_result();
            if ask.number_is_relative() {
                l = cxt.align_lba_in_range(l, user_f, dflt_l);
                if l > user_f {
                    l = l.wrapping_sub(1);
                }
            }
            if l >= user_f && l <= disk_l {
                break l;
            }
            cxt.warnx(b"Value out of range.");
        }
    };

    let nmax = cxt.label_ref().map_or(0, |l| l.nparts_max);
    if user_f > user_l || partnum >= nmax {
        cxt.warnx(format!("Could not create partition {}", partnum.wrapping_add(1)).as_bytes());
        return -EINVAL;
    }
    // Checked against the header rather than the context, to be safe.
    if user_l > ph.last_usable_lba() {
        cxt.warnx(
            format!(
                "The last usable GPT sector is {}, but {user_l} is requested.",
                ph.last_usable_lba()
            )
            .as_bytes(),
        );
        return -EINVAL;
    }
    if user_f < ph.first_usable_lba() {
        cxt.warnx(
            format!(
                "The first usable GPT sector is {}, but {user_f} is requested.",
                ph.first_usable_lba()
            )
            .as_bytes(),
        );
        return -EINVAL;
    }

    let uuid = match pa.and_then(|p| p.uuid.as_deref()) {
        // Reusing a UUID, to copy a table.
        Some(u) => match string_to_guid(u) {
            Ok(u) => u,
            Err(rc) => return rc,
        },
        // A new one, which every new partition gets.
        None => swap_efi_guid(&crate::uuid::generate_random()),
    };
    if let Some(gm) = gptm(cxt) {
        gm.with_entry(partnum, |e| {
            le_put(e, ent::LBA_END, 8, user_l);
            le_put(e, ent::LBA_START, 8, user_f);
            put16(e, ent::TYPE, &typeid);
            put16(e, ent::PARTITION_GUID, &uuid);
        });
        if let Some(name) = pa.and_then(|p| p.name.as_deref()).filter(|n| !n.is_empty()) {
            // A name that is no UTF-8 is left off, as upstream leaves it
            // when gpt_entry_set_name fails: the partition is made all the
            // same, and the name was the user's to see in the listing.
            let _ = entry_set_name(gm, partnum, name);
        }
    }
    if let Some(a) = pa.and_then(|p| p.attrs.clone()) {
        if let Ok(v) = attrs_from_string(cxt, &a) {
            if let Some(gm) = gptm(cxt) {
                gm.with_entry(partnum, |e| le_put(e, ent::ATTRS, 8, v));
            }
        }
    }
    if let Some(gm) = gptm(cxt) {
        recompute_both(gm);
    }
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_cur = lb.nparts_cur.wrapping_add(1);
        lb.changed = true;
    }
    let t = partition_parttype(cxt, &typeid);
    let num = i64::try_from(partnum.wrapping_add(1)).unwrap_or(i64::MAX);
    cxt.info_new_partition(num, user_f, user_l, Some(&t));
    if let Some(partno) = partno {
        *partno = partnum;
    }
    0
}

/// `gpt_create_disklabel (cxt)`.
fn gpt_create_disklabel(cxt: &mut Context) -> i32 {
    let rc = mknew_pmbr(cxt);
    if rc < 0 {
        return rc;
    }
    let (total, ss) = (cxt.total_sectors, cxt.sector_size);
    let mut p = Hdr::zeroed(ss);
    let rc = mknew_header(cxt, &mut p, PRIMARY_LBA);
    if rc < 0 {
        return rc;
    }
    let mut b = Hdr::zeroed(ss);
    let last = last_lba(cxt);
    mknew_header_from_bkp(&mut b, last, &p, total, ss);
    let esz = match sizeof_entries(&p) {
        Ok(s) => s,
        Err(rc) => return rc,
    };
    let mut ents = Vec::new();
    if ents.try_reserve_exact(esz).is_err() {
        return -ENOMEM;
    }
    ents.resize(esz, 0);
    recompute_crc(Some(&mut p), &ents);
    recompute_crc(Some(&mut b), &ents);
    let nents = usize::try_from(p.npartition_entries()).unwrap_or(0);
    let guid = p.disk_guid();
    if let Some(g) = gptm(cxt) {
        g.pheader = Some(p);
        g.bheader = Some(b);
        g.ents = Some(ents);
    }
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = nents;
        lb.nparts_cur = 0;
        lb.changed = true;
    }
    let mut m = b"Created a new GPT disklabel (GUID: ".to_vec();
    m.extend_from_slice(&guid_to_string(&guid));
    m.extend_from_slice(b").");
    cxt.info(&m);
    if u64::try_from(nents).unwrap_or(u64::MAX) < GPT_NPARTITIONS {
        cxt.info(
            format!("The maximal number of partitions is {nents} (default is {GPT_NPARTITIONS}).")
                .as_bytes(),
        );
    }
    0
}

/// `gpt_set_disklabel_id (cxt, str)`.
fn gpt_set_disklabel_id(cxt: &mut Context, s: Option<&[u8]>) -> i32 {
    let r = match s {
        Some(s) => string_to_guid(s),
        None => match cxt.ask_string(b"Enter new disk UUID (in 8-4-4-4-12 format)") {
            Ok(buf) => string_to_guid(&buf.unwrap_or_default()),
            Err(_) => return -EINVAL,
        },
    };
    let uuid = match r {
        Ok(u) => u,
        Err(rc) => {
            cxt.warnx(b"Failed to parse your UUID.");
            return rc;
        }
    };
    let Some(g) = gptm(cxt) else { return -EINVAL };
    let old = g.pheader.as_ref().map(get_header_id).unwrap_or_default();
    if let Some(p) = g.pheader.as_mut() {
        p.set_disk_guid(&uuid);
    }
    if let Some(b) = g.bheader.as_mut() {
        b.set_disk_guid(&uuid);
    }
    recompute_both(g);
    let new = g.pheader.as_ref().map(get_header_id).unwrap_or_default();
    let mut m = b"Disk identifier changed from ".to_vec();
    m.extend_from_slice(&old);
    m.extend_from_slice(b" to ");
    m.extend_from_slice(&new);
    m.push(b'.');
    cxt.info(&m);
    cxt.set_label_changed();
    0
}

/// `gpt_check_table_overlap (cxt, first_usable, last_usable)`.
fn check_table_overlap(cxt: &mut Context, first_usable: u64, last_usable: u64) -> i32 {
    let total = cxt.total_sectors;
    if first_usable > total || last_usable > total || first_usable > last_usable {
        cxt.warnx(b"Not enough space for new partition table!");
        return -ENOSPC;
    }
    let Some(g) = gpt(cxt).cloned() else {
        return -EINVAL;
    };
    let mut rc = 0;
    for i in 0..g.nentries() {
        if !g.is_used(i) {
            continue;
        }
        if g.e_start(i) < first_usable {
            cxt.warnx(
                format!(
                    "Partition #{} out of range (minimal start is {first_usable} sectors)",
                    i.wrapping_add(1)
                )
                .as_bytes(),
            );
            rc = -EINVAL;
        }
        if g.e_end(i) > last_usable {
            cxt.warnx(
                format!(
                    "Partition #{} out of range (maximal end is {} sectors)",
                    i.wrapping_add(1),
                    last_usable.wrapping_sub(1)
                )
                .as_bytes(),
            );
            rc = -EINVAL;
        }
    }
    rc
}

/// `fdisk_gpt_set_npartitions (cxt, nents)`: the entry array resized, if
/// the partitions allow.
pub fn set_npartitions(cxt: &mut Context, nents: u32) -> i32 {
    if !cxt.is_labeltype(LabelType::Gpt) {
        return -EINVAL;
    }
    let (total, ss) = (cxt.total_sectors, cxt.sector_size);
    let Some(p) = gpt(cxt).and_then(|g| g.pheader.clone()) else {
        return -EINVAL;
    };
    let old_nents = p.npartition_entries();
    if old_nents == nents {
        return 0;
    }
    let new_size = match calculate_sizeof_entries(Some(&p), nents) {
        Ok(s) => s,
        Err(rc) => {
            let entry_size = p.sizeof_partition_entry();
            if entry_size == 0 {
                cxt.warnx(b"The partition entry size is zero.");
            } else {
                cxt.warnx(
                    format!(
                        "The number of the partition has to be smaller than {}.",
                        u32::MAX.checked_div(entry_size).unwrap_or(0)
                    )
                    .as_bytes(),
                );
            }
            return rc;
        }
    };
    let first_usable = match calculate_first_lba(Some(&p), nents, ss) {
        Ok(f) => f,
        Err(rc) => return rc,
    };
    let last_usable = match calculate_last_lba(Some(&p), nents, total, ss) {
        Ok(l) => l,
        Err(rc) => return rc,
    };
    if nents > old_nents {
        let (rc, old_size) = match calculate_sizeof_entries(Some(&p), old_nents) {
            Ok(old_size) => (
                check_table_overlap(cxt, first_usable, last_usable),
                old_size,
            ),
            Err(rc) => (rc, 0),
        };
        if rc != 0 {
            return rc;
        }
        if let Some(g) = gptm(cxt) {
            let mut ents = g.ents.take().unwrap_or_default();
            if ents
                .try_reserve(new_size.saturating_sub(ents.len()))
                .is_err()
            {
                g.ents = Some(ents);
                cxt.warnx(b"Cannot allocate memory!");
                return -ENOMEM;
            }
            // `memset (ents + old_size, 0, new_size - old_size)`: entries a
            // shrink hid, still in the array, come back empty.
            ents.resize(new_size.max(ents.len()), 0);
            if let Some(grown) = ents.get_mut(old_size..new_size) {
                grown.fill(0);
            }
            g.ents = Some(ents);
        }
    }
    cxt.set_first_lba(first_usable);
    let _ = cxt.set_last_lba(last_usable);
    if let Some(g) = gptm(cxt) {
        for h in [g.pheader.as_mut(), g.bheader.as_mut()]
            .into_iter()
            .flatten()
        {
            h.set_npartition_entries(nents);
            h.set_first_usable_lba(first_usable);
            h.set_last_usable_lba(last_usable);
        }
        // The backup header recalculated.
        let alt = g.pheader.as_ref().map_or(0, Hdr::alternative_lba);
        if let Some(b) = g.bheader.as_mut() {
            mknew_header_common(b, alt, total, ss);
        }
        recompute_both(g);
    }
    let nmax = gpt(cxt).map_or(0, GptLabel::nentries);
    if let Some(lb) = cxt.label_mut() {
        lb.nparts_max = nmax;
    }
    cxt.info(format!("Partition table length changed from {old_nents} to {nents}.").as_bytes());
    cxt.set_label_changed();
    0
}

/// `gpt_part_is_used (cxt, i)`.
fn gpt_part_is_used(cxt: &mut Context, i: usize) -> i32 {
    let Some(g) = gpt(cxt) else { return 0 };
    if i >= g.nentries() {
        return 0;
    }
    i32::from(g.is_used(i) || g.e_start(i) != 0)
}

/// `fdisk_gpt_is_hybrid (cxt)`.
pub fn is_hybrid(cxt: &mut Context) -> bool {
    valid_pmbr(cxt) == GPT_MBR_HYBRID
}

/// `fdisk_gpt_get_partition_attrs (cxt, partnum, &attrs)`.
pub fn get_partition_attrs(cxt: &Context, partnum: usize) -> Result<u64, i32> {
    if !cxt.is_labeltype(LabelType::Gpt) {
        return Err(-EINVAL);
    }
    let g = gpt(cxt).ok_or(-EINVAL)?;
    if partnum >= g.nentries() {
        return Err(-EINVAL);
    }
    Ok(g.e_attrs(partnum))
}

/// `fdisk_gpt_set_partition_attrs (cxt, partnum, attrs)`.
pub fn set_partition_attrs(cxt: &mut Context, partnum: usize, attrs: u64) -> i32 {
    if !cxt.is_labeltype(LabelType::Gpt) {
        return -EINVAL;
    }
    let Some(g) = gptm(cxt) else { return -EINVAL };
    if partnum >= g.nentries() {
        return -EINVAL;
    }
    g.with_entry(partnum, |e| le_put(e, ent::ATTRS, 8, attrs));
    cxt.info(
        format!(
            "The attributes on partition {} changed to 0x{attrs:016x}.",
            partnum.wrapping_add(1)
        )
        .as_bytes(),
    );
    if let Some(g) = gptm(cxt) {
        recompute_both(g);
    }
    cxt.set_label_changed();
    0
}

/// `gpt_toggle_partition_flag (cxt, i, flag)`.
fn gpt_toggle_partition_flag(cxt: &mut Context, i: usize, flag: u64) -> i32 {
    let Some(g) = gpt(cxt) else { return -EINVAL };
    if i >= g.nentries() {
        return -EINVAL;
    }
    let mut attrs = g.e_attrs(i);
    let mut flag = flag;
    let mut name = "";
    let bit: i64 = match flag {
        GPT_FLAG_REQUIRED => {
            name = ATTRSTR_REQ;
            i64::from(ATTRBIT_REQ)
        }
        GPT_FLAG_NOBLOCK => {
            name = ATTRSTR_NOBLOCK;
            i64::from(ATTRBIT_NOBLOCK)
        }
        GPT_FLAG_LEGACYBOOT => {
            name = ATTRSTR_LEGACY;
            i64::from(ATTRBIT_LEGACY)
        }
        GPT_FLAG_GUIDSPECIFIC => match cxt.ask_number(48, 48, 63, b"Enter GUID specific bit") {
            Ok(t) => i64::from(trunc_i32(i64::from_ne_bytes(t.to_ne_bytes()))),
            Err(rc) => return rc,
        },
        // A GUID-specific bit given by number.
        48..=63 => {
            let b = i64::try_from(flag).unwrap_or(-1);
            flag = GPT_FLAG_GUIDSPECIFIC;
            b
        }
        _ => -1,
    };
    let Ok(bitu) = u32::try_from(bit) else {
        cxt.warnx(format!("failed to toggle unsupported bit {flag}").as_bytes());
        return -EINVAL;
    };
    let mask = 1u64.checked_shl(bitu).unwrap_or(0);
    attrs ^= mask;
    if let Some(g) = gptm(cxt) {
        g.with_entry(i, |e| le_put(e, ent::ATTRS, 8, attrs));
    }
    let on = attrs & mask != 0;
    let pn = i.wrapping_add(1);
    let m = if flag == GPT_FLAG_GUIDSPECIFIC {
        format!(
            "The GUID specific bit {bit} on partition {pn} is {} now.",
            if on { "enabled" } else { "disabled" }
        )
    } else {
        format!(
            "The {name} flag on partition {pn} is {} now.",
            if on { "enabled" } else { "disabled" }
        )
    };
    cxt.info(m.as_bytes());
    if let Some(g) = gptm(cxt) {
        recompute_both(g);
    }
    cxt.set_label_changed();
    0
}

/// `gpt_reorder (cxt)`: entries sorted by start, unused last. 1 when
/// already in order.
fn gpt_reorder(cxt: &mut Context) -> i32 {
    let Some(g) = gptm(cxt) else { return -EINVAL };
    let nparts = g.nentries();
    // Upstream compares and sorts `sizeof (struct gpt_entry)` chunks,
    // whatever the header's entry size.
    let chunk = |e: &[u8], i: usize| -> Vec<u8> {
        let off = i.wrapping_mul(GPT_ENTRY_SIZE);
        let mut v = e
            .get(off..off.wrapping_add(GPT_ENTRY_SIZE))
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        v.resize(GPT_ENTRY_SIZE, 0);
        v
    };
    let cmp = |a: &[u8], b: &[u8]| -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let used = |e: &[u8]| e.get(..16).is_some_and(|t| t.iter().any(|&x| x != 0));
        match (used(a), used(b)) {
            (false, false) => Ordering::Equal,
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (true, true) => le_get(a, ent::LBA_START, 8).cmp(&le_get(b, ent::LBA_START, 8)),
        }
    };
    let ents = g.ents.clone().unwrap_or_default();
    let mut list: Vec<Vec<u8>> = (0..nparts).map(|i| chunk(&ents, i)).collect();
    let mess = list
        .windows(2)
        .any(|w| matches!(w, [a, b] if cmp(a, b) == std::cmp::Ordering::Greater));
    if !mess {
        return 1;
    }
    list.sort_by(|a, b| cmp(a, b));
    let mut new = ents;
    for (i, e) in list.iter().enumerate() {
        let off = i.wrapping_mul(GPT_ENTRY_SIZE);
        if let Some(dst) = new.get_mut(off..off.wrapping_add(GPT_ENTRY_SIZE)) {
            dst.copy_from_slice(e);
        }
    }
    g.ents = Some(new);
    recompute_both(g);
    cxt.set_label_changed();
    0
}

/// `gpt_reset_alignment (cxt)`: the table's usable range, or an estimate
/// of a new table's.
fn gpt_reset_alignment(cxt: &mut Context) -> i32 {
    if let Some(h) = gpt(cxt).and_then(|g| g.pheader.clone()) {
        // Always follow the existing table.
        cxt.first_lba = h.first_usable_lba();
        cxt.last_lba = h.last_usable_lba();
    } else if let Ok((first, last, _)) = count_first_last_lba(cxt) {
        if cxt.first_lba < first {
            cxt.first_lba = first;
        }
        if cxt.last_lba > last {
            cxt.last_lba = last;
        }
    }
    0
}

/// `gpt_deinit (lb)`.
fn gpt_deinit(lb: &mut Label) {
    if let LabelData::Gpt(g) = &mut lb.data {
        g.ents = None;
        g.pheader = None;
        g.bheader = None;
    }
}

/// `gpt_deinit (cxt->label)`.
fn gpt_deinit_cxt(cxt: &mut Context) {
    if let Some(lb) = cxt.label_mut() {
        gpt_deinit(lb);
    }
}

/// `gpt_operations`.
static GPT_OPS: LabelOps = LabelOps {
    probe: Some(gpt_probe_label),
    write: Some(gpt_write_disklabel),
    verify: Some(gpt_verify_disklabel),
    create: Some(gpt_create_disklabel),
    locate: Some(gpt_locate_disklabel),
    reorder: Some(gpt_reorder),
    get_item: Some(gpt_get_disklabel_item),
    set_id: Some(gpt_set_disklabel_id),
    add_part: Some(gpt_add_partition),
    del_part: Some(gpt_delete_partition),
    get_part: Some(gpt_get_partition),
    set_part: Some(gpt_set_partition),
    part_is_used: Some(gpt_part_is_used),
    part_toggle_flag: Some(gpt_toggle_partition_flag),
    reset_alignment: Some(gpt_reset_alignment),
    deinit: Some(gpt_deinit),
};

/// `gpt_fields`.
static GPT_FIELDS: [Field; 10] = [
    Field::new(fieldtype::DEVICE, "Device", 10.0, 0),
    Field::new(fieldtype::START, "Start", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::END, "End", 5.0, FIELDFL_NUMBER),
    Field::new(fieldtype::SECTORS, "Sectors", 5.0, FIELDFL_NUMBER),
    Field::new(
        fieldtype::SIZE,
        "Size",
        5.0,
        FIELDFL_NUMBER | FIELDFL_EYECANDY,
    ),
    Field::new(fieldtype::TYPE, "Type", 0.1, FIELDFL_EYECANDY),
    Field::new(fieldtype::TYPEID, "Type-UUID", 36.0, FIELDFL_DETAIL),
    Field::new(fieldtype::UUID, "UUID", 36.0, FIELDFL_DETAIL),
    Field::new(fieldtype::NAME, "Name", 0.2, FIELDFL_DETAIL),
    Field::new(fieldtype::ATTR, "Attrs", 0.0, FIELDFL_DETAIL),
];

/// `gpt_parttype_cuts`.
static GPT_PARTTYPE_CUTS: [Shortcut; 6] = [
    Shortcut {
        shortcut: Some("L"),
        alias: Some("linux"),
        data: "0FC63DAF-8483-4772-8E79-3D69D8477DE4",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("S"),
        alias: Some("swap"),
        data: "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("H"),
        alias: Some("home"),
        data: "933AC7E1-2EB4-4F13-B844-0E14E2AEF915",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("U"),
        alias: Some("uefi"),
        data: "C12A7328-F81F-11D2-BA4B-00A0C93EC93B",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("R"),
        alias: Some("raid"),
        data: "A19D880F-05FC-4D3B-A006-743F0F84911E",
        deprecated: false,
    },
    Shortcut {
        shortcut: Some("V"),
        alias: Some("lvm"),
        data: "E6D6D379-F507-44C2-A23C-238F2A3DF928",
        deprecated: false,
    },
];

/// `fdisk_new_gpt_label (cxt)`.
#[must_use]
pub fn new_label() -> Label {
    Label {
        name: "gpt",
        id: LabelType::Gpt,
        parttypes: &crate::gpt_parttypes::GPT_PARTTYPES,
        parttype_cuts: &GPT_PARTTYPE_CUTS,
        nparts_max: 0,
        nparts_cur: 0,
        flags: 0,
        geom_min: Geometry::default(),
        geom_max: Geometry::default(),
        changed: false,
        disabled: false,
        fields: &GPT_FIELDS,
        op: &GPT_OPS,
        data: LabelData::Gpt(Box::default()),
    }
}

/// `fdisk_gpt_disable_relocation (lb, disable)`.
pub fn disable_relocation(lb: &mut Label, disable: bool) {
    if let LabelData::Gpt(g) = &mut lb.data {
        g.no_relocate = disable;
    }
}

/// `fdisk_gpt_enable_minimize (lb, enable)`.
pub fn enable_minimize(lb: &mut Label, enable: bool) {
    if let LabelData::Gpt(g) = &mut lb.data {
        g.minimize = enable;
    }
}
