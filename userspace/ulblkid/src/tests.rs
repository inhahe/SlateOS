//! Unit tests: synthetic images, one per partition-table format and for a
//! superblock, probed through the public interface -- values (`PTTYPE`,
//! `TYPE`, `LABEL`...) and the binary partition list. Byte-for-byte
//! agreement with the real libblkid is `scripts/blkid-diff.sh`'s job; these
//! pin the behaviour each format's reader is built around.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests index, unwrap and do arithmetic on the images they built"
)]

use scratchdir::ScratchDir;

use crate::partitions::{Partition, Partlist};
use crate::{
    FLTR_NOTIN, PARTS_MAGIC, PROBE_NONE, PROBE_OK, Probe, SUBLKS_DEFAULT, SUBLKS_FSINFO,
    SUBLKS_MAGIC, SUBLKS_VERSION,
};

/// An image being built in memory.
struct Image(Vec<u8>);

impl Image {
    fn new(size: usize) -> Self {
        Image(vec![0; size])
    }
    fn put(&mut self, off: usize, data: &[u8]) -> &mut Self {
        self.0[off..off + data.len()].copy_from_slice(data);
        self
    }
    fn le16(&mut self, off: usize, v: u16) -> &mut Self {
        self.put(off, &v.to_le_bytes())
    }
    fn le32(&mut self, off: usize, v: u32) -> &mut Self {
        self.put(off, &v.to_le_bytes())
    }
    fn le64(&mut self, off: usize, v: u64) -> &mut Self {
        self.put(off, &v.to_le_bytes())
    }
    fn be16(&mut self, off: usize, v: u16) -> &mut Self {
        self.put(off, &v.to_be_bytes())
    }
    fn be32(&mut self, off: usize, v: u32) -> &mut Self {
        self.put(off, &v.to_be_bytes())
    }
    /// A DOS partition entry `i` of the MBR (or EBR) at `sector`.
    fn mbr_entry(
        &mut self,
        sector: usize,
        i: usize,
        boot: u8,
        ty: u8,
        start: u32,
        size: u32,
    ) -> &mut Self {
        let at = sector * 512 + 0x1be + 16 * i;
        self.put(at, &[boot]);
        self.put(at + 4, &[ty]);
        self.le32(at + 8, start);
        self.le32(at + 12, size)
    }
    /// 55AA at the end of `sector`.
    fn mbr_magic(&mut self, sector: usize) -> &mut Self {
        self.put(sector * 512 + 510, &[0x55, 0xAA])
    }
}

/// Write `img` into `dir` and open a probe on it.
fn probe_of(dir: &ScratchDir, name: &str, img: &Image) -> Probe {
    let p = dir.path(name);
    std::fs::write(&p, &img.0).unwrap();
    Probe::from_filename(p.to_str().unwrap().as_bytes()).unwrap()
}

/// A value, as `%s` prints it.
fn value(pr: &Probe, name: &str) -> Option<String> {
    pr.lookup_value(name)
        .map(|v| String::from_utf8(v.as_c_str().to_vec()).unwrap())
}

/// Probe for values: superblocks off, partitions on.
fn pt_values(pr: &mut Probe, flags: u32) -> i32 {
    pr.enable_superblocks(false);
    pr.enable_partitions(true);
    pr.set_partitions_flags(flags);
    pr.do_safeprobe()
}

/// The binary partition list, as `(partno, start, size, type)`.
fn parts(ls: &Partlist) -> Vec<(i32, u64, u64, i32)> {
    ls.partitions()
        .iter()
        .map(|p| (p.partno, p.start, p.size, p.ty))
        .collect()
}

/// A partition's text fields.
fn text(b: Option<&[u8]>) -> Option<String> {
    b.map(|b| String::from_utf8(b.to_vec()).unwrap())
}

/// An EFI GUID in its on-disk order, from its text form.
fn efi_guid(s: &str) -> [u8; 16] {
    let hex: Vec<u8> = s
        .split('-')
        .flat_map(|g| {
            (0..g.len())
                .step_by(2)
                .map(|k| u8::from_str_radix(&g[k..k + 2], 16).unwrap())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut g = [0u8; 16];
    g.copy_from_slice(&hex);
    g[0..4].reverse();
    g[4..6].reverse();
    g[6..8].reverse();
    g
}

// ---------------------------------------------------------------- DOS

/// An MBR with a primary, an extended partition and two logicals chained
/// through two EBRs.
fn dos_image() -> Image {
    let mut img = Image::new(3 << 20);
    img.le32(440, 0x1234_5678)
        .mbr_entry(0, 0, 0x80, 0x83, 2048, 100)
        .mbr_entry(0, 1, 0, 0x05, 4096, 1000)
        .mbr_magic(0)
        // The first EBR: a logical, and the link to the next EBR.
        .mbr_entry(4096, 0, 0, 0x83, 1, 50)
        .mbr_entry(4096, 1, 0, 0x05, 100, 60)
        .mbr_magic(4096)
        // The second: the last logical.
        .mbr_entry(4196, 0, 0, 0x82, 1, 20)
        .mbr_magic(4196);
    img
}

#[test]
fn dos_values_are_the_type_and_the_disk_id() {
    let dir = ScratchDir::new("ulblkid-dos-values");
    let mut pr = probe_of(&dir, "img", &dos_image());
    assert_eq!(pt_values(&mut pr, PARTS_MAGIC), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("dos"));
    assert_eq!(value(&pr, "PTUUID").as_deref(), Some("12345678"));
    assert_eq!(value(&pr, "PTMAGIC_OFFSET").as_deref(), Some("510"));
    assert_eq!(pr.lookup_value("PTMAGIC").unwrap().data(), b"\x55\xAA");
}

#[test]
fn dos_partitions_follow_the_ebr_chain() {
    let dir = ScratchDir::new("ulblkid-dos-parts");
    let mut pr = probe_of(&dir, "img", &dos_image());
    let ls = pr.partitions().unwrap();
    // Empty primaries use up 3 and 4; logicals start at 5.
    assert_eq!(
        parts(ls),
        vec![
            (1, 2048, 100, 0x83),
            (2, 4096, 1000, 0x05),
            (5, 4097, 50, 0x83),
            (6, 4197, 20, 0x82),
        ]
    );
    let all = ls.partitions();
    assert_eq!(all[0].flags, 0x80);
    assert_eq!(text(all[0].uuid()).as_deref(), Some("12345678-01"));
    assert_eq!(text(all[3].uuid()).as_deref(), Some("12345678-06"));
    assert!(ls.is_primary(&all[0]));
    assert!(ls.is_extended(&all[1]));
    assert!(ls.is_logical(&all[2]));
    let tab = ls.tab(ls.table().unwrap()).unwrap();
    assert_eq!((tab.ty, tab.offset), ("dos", 0x1be));
    assert_eq!(tab.id(), Some(&b"12345678"[..]));
}

#[test]
fn dos_ebr_loop_ends() {
    // The second EBR links back to itself: its partition is found once, and
    // the loop ends after a hundred records with nothing new.
    let dir = ScratchDir::new("ulblkid-dos-loop");
    let mut img = Image::new(3 << 20);
    img.mbr_entry(0, 0, 0, 0x05, 4096, 1000)
        .mbr_magic(0)
        .mbr_entry(4096, 0, 0, 0x83, 1, 50)
        .mbr_entry(4096, 1, 0, 0x05, 200, 60)
        .mbr_magic(4096)
        .mbr_entry(4296, 0, 0, 0x83, 1, 20)
        .mbr_entry(4296, 1, 0, 0x05, 200, 60)
        .mbr_magic(4296);
    let mut pr = probe_of(&dir, "img", &img);
    let ls = pr.partitions().unwrap();
    assert_eq!(
        parts(ls),
        vec![
            (1, 4096, 1000, 0x05),
            (5, 4097, 50, 0x83),
            (6, 4297, 20, 0x83)
        ]
    );
    // A link with a start of 0 is junk, and ends the chain.
    let mut img = Image::new(3 << 20);
    img.mbr_entry(0, 0, 0, 0x05, 4096, 1000)
        .mbr_magic(0)
        .mbr_entry(4096, 0, 0, 0x83, 1, 50)
        .mbr_entry(4096, 1, 0, 0x05, 0, 60)
        .mbr_magic(4096);
    let mut pr = probe_of(&dir, "img2", &img);
    let ls = pr.partitions().unwrap();
    assert_eq!(parts(ls), vec![(1, 4096, 1000, 0x05), (5, 4097, 50, 0x83)]);
}

#[test]
fn dos_ignores_a_bad_boot_indicator_and_gpt() {
    let dir = ScratchDir::new("ulblkid-dos-reject");
    let mut img = Image::new(1 << 20);
    img.mbr_entry(0, 0, 0x12, 0x83, 2048, 100).mbr_magic(0);
    let mut pr = probe_of(&dir, "boot", &img);
    assert_eq!(pt_values(&mut pr, 0), PROBE_NONE);
    // A protective MBR is GPT's, and with no GPT behind it, PMBR's.
    let mut img = Image::new(1 << 20);
    img.mbr_entry(0, 0, 0, 0xee, 1, 2047).mbr_magic(0);
    let mut pr = probe_of(&dir, "pmbr", &img);
    assert_eq!(pt_values(&mut pr, 0), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("PMBR"));
}

#[test]
fn aix_is_not_dos() {
    let dir = ScratchDir::new("ulblkid-aix");
    let mut img = Image::new(1 << 20);
    img.put(0, b"\xC9\xC2\xD4\xC1")
        .mbr_entry(0, 0, 0, 0x83, 2048, 100)
        .mbr_magic(0);
    let mut pr = probe_of(&dir, "img", &img);
    assert_eq!(pt_values(&mut pr, 0), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("aix"));
    let ls = pr.partitions().unwrap();
    assert!(ls.partitions().is_empty());
    assert_eq!(ls.tab(ls.table().unwrap()).unwrap().ty, "aix");
}

// ---------------------------------------------------------------- nested

/// A 4 MiB disk whose one primary, of type `ty`, is sectors 2048..6144.
fn nested_image(ty: u8) -> Image {
    let mut img = Image::new(4 << 20);
    img.mbr_entry(0, 0, 0, ty, 2048, 4096).mbr_magic(0);
    img
}

/// A table: its type, and whether it is nested.
type TabSummary = (&'static str, bool);
/// A partition: number, start, size, type, flags.
type PartSummary = (i32, u64, u64, i32, u64);

/// The tables, and every partition.
fn nested(pr: &mut Probe) -> (Vec<TabSummary>, Vec<PartSummary>) {
    let ls = pr.partitions().unwrap();
    let tabs = (0..4)
        .filter_map(|i| ls.tab(crate::partitions::TabId(i)))
        .map(|t| (t.ty, t.parent.is_some()))
        .collect();
    let p = ls
        .partitions()
        .iter()
        .map(|p: &Partition| (p.partno, p.start, p.size, p.ty, p.flags))
        .collect();
    (tabs, p)
}

#[test]
fn freebsd_label_in_a_dos_slice() {
    let dir = ScratchDir::new("ulblkid-bsd");
    let mut img = nested_image(0xa5);
    let l = 2049 * 512;
    img.le32(l, 0x8256_4557)
        .le32(l + 132, 0x8256_4557)
        .le16(l + 138, 3)
        // a: 16 sectors into the slice (FreeBSD 10's relative offsets).
        .le32(l + 148, 100)
        .le32(l + 148 + 4, 16)
        .put(l + 148 + 12, &[7])
        // c: the whole slice, at 0 -- which is what says "relative".
        .le32(l + 148 + 32, 4096)
        .le32(l + 148 + 32 + 4, 0)
        .put(l + 148 + 32 + 12, &[7]);
    let csum = (0..404).step_by(2).fold(0u16, |c, k| {
        c ^ u16::from_le_bytes([img.0[l + k], img.0[l + k + 1]])
    });
    img.le16(l + 136, csum);
    let mut pr = probe_of(&dir, "img", &img);
    let (tabs, p) = nested(&mut pr);
    assert_eq!(tabs, vec![("dos", false), ("freebsd", true)]);
    // 'c' is the slice itself, and dropped.
    assert_eq!(p, vec![(1, 2048, 4096, 0xa5, 0), (5, 2064, 100, 7, 0)]);
    // For values, the DOS table only.
    let mut pr = probe_of(&dir, "img2", &img);
    assert_eq!(pt_values(&mut pr, 0), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("dos"));
}

#[test]
fn bsd_label_with_a_bad_checksum_is_ignored() {
    let dir = ScratchDir::new("ulblkid-bsd-bad");
    let mut img = nested_image(0xa9);
    let l = 2049 * 512;
    img.le32(l, 0x8256_4557)
        .le16(l + 138, 1)
        .le32(l + 148, 100)
        .put(l + 148 + 12, &[7]);
    img.le16(l + 136, 0x1234);
    let mut pr = probe_of(&dir, "img", &img);
    let (tabs, _) = nested(&mut pr);
    assert_eq!(tabs, vec![("dos", false)]);
}

#[test]
fn minix_subpartitions() {
    let dir = ScratchDir::new("ulblkid-minix");
    let mut img = nested_image(0x81);
    img.mbr_entry(2048, 0, 0, 0x81, 2100, 100)
        .mbr_entry(2048, 1, 0, 0x83, 2300, 100)
        // Outside the parent: ignored.
        .mbr_entry(2048, 2, 0, 0x81, 9000, 100)
        .mbr_magic(2048);
    let mut pr = probe_of(&dir, "img", &img);
    let (tabs, p) = nested(&mut pr);
    assert_eq!(tabs, vec![("dos", false), ("minix", true)]);
    assert_eq!(p, vec![(1, 2048, 4096, 0x81, 0), (5, 2100, 100, 0x81, 0)]);
}

#[test]
fn solaris_slices_skip_the_last_one_as_upstream_does() {
    let dir = ScratchDir::new("ulblkid-solaris");
    let mut img = nested_image(0x82);
    let v = 2049 * 512;
    img.le32(v + 12, 0x600D_DEEE)
        .le32(v + 16, 1)
        .le16(v + 30, 3)
        // Slice 0: relative to the partition.
        .le16(v + 72, 2)
        .le16(v + 72 + 2, 0x10)
        .le32(v + 72 + 4, 10)
        .le32(v + 72 + 8, 100)
        // Slice 1: the whole disk, skipped.
        .le16(v + 84, 5)
        .le32(v + 84 + 8, 4096)
        // Slice 2: never read (the loop's off-by-one).
        .le16(v + 96, 3)
        .le32(v + 96 + 4, 200)
        .le32(v + 96 + 8, 50);
    let mut pr = probe_of(&dir, "img", &img);
    let (tabs, p) = nested(&mut pr);
    assert_eq!(tabs, vec![("dos", false), ("solaris", true)]);
    assert_eq!(p, vec![(1, 2048, 4096, 0x82, 0), (5, 2058, 100, 2, 0x10)]);
}

#[test]
fn unixware_is_found_where_upstream_looks() {
    let dir = ScratchDir::new("ulblkid-unixware");
    let mut img = nested_image(0x63);
    let base = 2048 * 512;
    // The magic where upstream's miscomputed offset looks, and the VTOC in
    // sector 29.
    img.put(base + 29174, b"\x0D\x60\xE5\xCA");
    let v = base + 29 * 512;
    img.le32(v + 156, 0x600D_DEEE)
        .le16(v + 216 + 12, 2)
        .le16(v + 216 + 12 + 2, 0x200)
        .le32(v + 216 + 12 + 4, 2148)
        .le32(v + 216 + 12 + 8, 50)
        // Not marked valid: skipped.
        .le16(v + 216 + 24, 2)
        .le32(v + 216 + 24 + 4, 2200)
        .le32(v + 216 + 24 + 8, 50);
    let mut pr = probe_of(&dir, "img", &img);
    let (tabs, p) = nested(&mut pr);
    assert_eq!(tabs, vec![("dos", false), ("unixware", true)]);
    assert_eq!(p, vec![(1, 2048, 4096, 0x63, 0), (5, 2148, 50, 2, 0x200)]);
    // The magic where the label really keeps it is not looked at.
    let mut img = nested_image(0x63);
    img.put(base + 29 * 512 + 4, b"\x0D\x60\xE5\xCA");
    img.le32(v + 156, 0x600D_DEEE);
    let mut pr = probe_of(&dir, "img2", &img);
    let (tabs, _) = nested(&mut pr);
    assert_eq!(tabs, vec![("dos", false)]);
}

#[test]
fn no_nesting_on_a_floppy() {
    let dir = ScratchDir::new("ulblkid-tiny");
    let mut img = Image::new(1 << 20);
    img.mbr_entry(0, 0, 0, 0x81, 100, 200)
        .mbr_magic(0)
        .mbr_entry(100, 0, 0, 0x81, 110, 10)
        .mbr_magic(100);
    let mut pr = probe_of(&dir, "img", &img);
    let (tabs, _) = nested(&mut pr);
    assert_eq!(tabs, vec![("dos", false)]);
}

// ---------------------------------------------------------------- GPT

const LINUX_FS: &str = "0FC63DAF-8483-4772-8E79-3D69D8477DE4";
const DISK_GUID: &str = "11223344-5566-7788-99AA-BBCCDDEEFF00";

/// A GPT header at `lba`, its entries at `entries_lba`, on a 2048-sector
/// disk.
fn gpt_header(img: &mut Image, lba: u64, alt: u64, entries_lba: u64) {
    let ents = &img.0[entries_lba as usize * 512..entries_lba as usize * 512 + 128 * 128];
    let ents_crc = crc32::crc32(ents);
    let h = lba as usize * 512;
    img.put(h, b"EFI PART")
        .le32(h + 8, 0x0001_0000)
        .le32(h + 12, 92)
        .le64(h + 24, lba)
        .le64(h + 32, alt)
        .le64(h + 40, 34)
        .le64(h + 48, 2014)
        .put(h + 56, &efi_guid(DISK_GUID))
        .le64(h + 72, entries_lba)
        .le32(h + 80, 128)
        .le32(h + 84, 128)
        .le32(h + 88, ents_crc);
    let crc = crc32::crc32(&img.0[h..h + 92]);
    img.le32(h + 16, crc);
}

/// Entries: a Linux filesystem, an unused slot, one past the usable
/// area, and one with an attribute bit.
fn gpt_entries(img: &mut Image, entries_lba: usize) {
    let e = entries_lba * 512;
    let name: Vec<u8> = "root".encode_utf16().flat_map(u16::to_le_bytes).collect();
    img.put(e, &efi_guid(LINUX_FS))
        .put(e + 16, &efi_guid("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"))
        .le64(e + 32, 40)
        .le64(e + 40, 1000)
        .put(e + 56, &name);
    let e2 = e + 2 * 128;
    img.put(e2, &efi_guid(LINUX_FS))
        .le64(e2 + 32, 1001)
        .le64(e2 + 40, 2100);
    let e3 = e + 3 * 128;
    img.put(e3, &efi_guid(LINUX_FS))
        .le64(e3 + 32, 1100)
        .le64(e3 + 40, 1200)
        .le64(e3 + 48, 1 << 63);
}

fn gpt_image() -> Image {
    let mut img = Image::new(1 << 20);
    img.mbr_entry(0, 0, 0, 0xee, 1, 2047).mbr_magic(0);
    gpt_entries(&mut img, 2);
    gpt_header(&mut img, 1, 2047, 2);
    img
}

#[test]
fn gpt_values() {
    let dir = ScratchDir::new("ulblkid-gpt-values");
    let mut pr = probe_of(&dir, "img", &gpt_image());
    assert_eq!(pt_values(&mut pr, PARTS_MAGIC), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("gpt"));
    assert_eq!(
        value(&pr, "PTUUID").as_deref(),
        Some("11223344-5566-7788-99aa-bbccddeeff00")
    );
    assert_eq!(value(&pr, "PTMAGIC_OFFSET").as_deref(), Some("512"));
    assert_eq!(pr.lookup_value("PTMAGIC").unwrap().data(), b"EFI PART");
}

#[test]
fn gpt_partitions() {
    let dir = ScratchDir::new("ulblkid-gpt-parts");
    let mut pr = probe_of(&dir, "img", &gpt_image());
    let ls = pr.partitions().unwrap();
    // The unused slot and the one out of range use up 2 and 3.
    assert_eq!(parts(ls), vec![(1, 40, 961, 0), (4, 1100, 101, 0)]);
    let p = &ls.partitions()[0];
    assert_eq!(text(p.name()).as_deref(), Some("root"));
    assert_eq!(
        text(p.uuid()).as_deref(),
        Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
    );
    assert_eq!(
        text(p.type_string()).as_deref(),
        Some("0fc63daf-8483-4772-8e79-3d69d8477de4")
    );
    assert_eq!(ls.partitions()[1].flags, 1 << 63);
    let tab = ls.tab(ls.table().unwrap()).unwrap();
    assert_eq!((tab.ty, tab.offset), ("gpt", 512));
    assert_eq!(tab.id(), Some(&b"11223344-5566-7788-99aa-bbccddeeff00"[..]));
}

#[test]
fn gpt_backup_header_stands_in_for_a_corrupt_primary() {
    let dir = ScratchDir::new("ulblkid-gpt-backup");
    let mut img = gpt_image();
    // The backup: entries just before the last LBA, the header on it.
    gpt_entries(&mut img, 2015);
    gpt_header(&mut img, 2047, 1, 2015);
    // Corrupt the primary header after its checksum was computed.
    img.0[512 + 60] ^= 0xff;
    let mut pr = probe_of(&dir, "img", &img);
    assert_eq!(pt_values(&mut pr, PARTS_MAGIC), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("gpt"));
    assert_eq!(
        value(&pr, "PTMAGIC_OFFSET").as_deref(),
        Some((2047 * 512).to_string().as_str())
    );
    let mut pr = probe_of(&dir, "img2", &img);
    let ls = pr.partitions().unwrap();
    assert_eq!(ls.tab(ls.table().unwrap()).unwrap().offset, 2047 * 512);
    assert_eq!(parts(ls), vec![(1, 40, 961, 0), (4, 1100, 101, 0)]);
}

#[test]
fn gpt_entry_array_checksum_is_checked() {
    let dir = ScratchDir::new("ulblkid-gpt-entries");
    let mut img = gpt_image();
    img.0[1024 + 100] ^= 1;
    let mut pr = probe_of(&dir, "img", &img);
    assert_eq!(pt_values(&mut pr, 0), PROBE_OK);
    // No valid GPT: the protective MBR alone.
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("PMBR"));
}

// ---------------------------------------------------------------- others

#[test]
fn sun_label() {
    let dir = ScratchDir::new("ulblkid-sun");
    let mut img = Image::new(1 << 20);
    img.be16(436, 16)
        .be16(438, 63)
        .be32(128, 1)
        .be16(140, 8)
        .be32(188, 0x600D_DEEE)
        .be16(142, 2)
        .be16(146, 3)
        .be16(148, 1)
        .be16(150, 5)
        .be32(444, 0)
        .be32(448, 1008)
        .be32(452, 1)
        .be32(456, 2016)
        .be32(460, 0)
        .be32(464, 16128)
        .put(508, b"\xDA\xBE");
    let csum = (0..510).step_by(2).fold(0u16, |c, k| {
        c ^ u16::from_le_bytes([img.0[k], img.0[k + 1]])
    });
    img.le16(510, csum);
    let mut pr = probe_of(&dir, "img", &img);
    let ls = pr.partitions().unwrap();
    // The whole-disk slice uses up 3.
    assert_eq!(parts(ls), vec![(1, 0, 1008, 2), (2, 1008, 2016, 3)]);
    assert_eq!(ls.partitions()[1].flags, 1);
    let mut pr = probe_of(&dir, "img2", &img);
    assert_eq!(pt_values(&mut pr, 0), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("sun"));
}

#[test]
fn sgi_label() {
    let dir = ScratchDir::new("ulblkid-sgi");
    let mut img = Image::new(1 << 20);
    img.be32(0, 0x0BE5_A941)
        .be32(312, 100)
        .be32(316, 10)
        .be32(320, 0x0a);
    let sum = (0..504).step_by(4).fold(0u32, |s, k| {
        s.wrapping_add(u32::from_be_bytes(img.0[k..k + 4].try_into().unwrap()))
    });
    img.be32(504, sum.wrapping_neg());
    let mut pr = probe_of(&dir, "img", &img);
    let ls = pr.partitions().unwrap();
    assert_eq!(parts(ls), vec![(1, 10, 100, 0x0a)]);
    // A wrong checksum: not a label.
    img.0[400] = 1;
    let mut pr = probe_of(&dir, "img2", &img);
    assert!(pr.partitions().is_none());
}

#[test]
fn mac_partition_map() {
    let dir = ScratchDir::new("ulblkid-mac");
    let mut img = Image::new(1 << 20);
    img.put(0, b"ER").be16(2, 512);
    for (k, (start, count, name, ty)) in [
        (1u32, 63u32, "Apple", "Apple_partition_map"),
        (64, 100, "disk image", "Apple_HFS"),
    ]
    .iter()
    .enumerate()
    {
        let b = 512 * (k + 1);
        img.put(b, b"PM")
            .be32(b + 4, 2)
            .be32(b + 8, *start)
            .be32(b + 12, *count)
            .put(b + 16, name.as_bytes())
            .put(b + 48, ty.as_bytes());
    }
    let mut pr = probe_of(&dir, "img", &img);
    let ls = pr.partitions().unwrap();
    assert_eq!(parts(ls), vec![(1, 1, 63, 0), (2, 64, 100, 0)]);
    let p = ls.partitions();
    assert_eq!(text(p[1].name()).as_deref(), Some("disk image"));
    assert_eq!(
        text(p[0].type_string()).as_deref(),
        Some("Apple_partition_map")
    );
}

#[test]
fn atari_root_sector() {
    let dir = ScratchDir::new("ulblkid-atari");
    let mut img = Image::new(1 << 20);
    img.be32(0x1c2, 2048)
        .put(0x1c6, b"\x01GEM")
        .be32(0x1c6 + 4, 10)
        .be32(0x1c6 + 8, 100)
        .put(0x1d2, b"\x01LNX")
        .be32(0x1d2 + 4, 200)
        .be32(0x1d2 + 8, 300);
    let mut pr = probe_of(&dir, "img", &img);
    assert_eq!(pt_values(&mut pr, PARTS_MAGIC), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("atari"));
    assert_eq!(value(&pr, "PTMAGIC_OFFSET").as_deref(), Some("454"));
    assert_eq!(pr.lookup_value("PTMAGIC").unwrap().data(), b"\x01GEM");
    let mut pr = probe_of(&dir, "img2", &img);
    let ls = pr.partitions().unwrap();
    assert_eq!(parts(ls), vec![(1, 10, 100, 0), (2, 200, 300, 0)]);
    assert_eq!(
        text(ls.partitions()[1].type_string()).as_deref(),
        Some("LNX")
    );
    // A recorded disk size larger than the disk: not a root sector.
    img.be32(0x1c2, 4096);
    let mut pr = probe_of(&dir, "img3", &img);
    assert_eq!(pt_values(&mut pr, 0), PROBE_NONE);
}

#[test]
fn ultrix_label() {
    let dir = ScratchDir::new("ulblkid-ultrix");
    let mut img = Image::new(1 << 20);
    let l = 31 * 512 + 440;
    img.le32(l, 0x0003_2957)
        .le32(l + 4, 1)
        .le32(l + 8, 100)
        .le32(l + 12, 10);
    let mut pr = probe_of(&dir, "img", &img);
    assert_eq!(pt_values(&mut pr, PARTS_MAGIC), PROBE_OK);
    assert_eq!(value(&pr, "PTTYPE").as_deref(), Some("ultrix"));
    assert_eq!(value(&pr, "PTMAGIC_OFFSET").as_deref(), Some("16312"));
    let mut pr = probe_of(&dir, "img2", &img);
    let ls = pr.partitions().unwrap();
    assert_eq!(parts(ls), vec![(1, 10, 100, 0)]);
}

// ---------------------------------------------------------------- swap

/// A SWAPSPACE2 area with a label and a UUID, 15 pages of 4 KiB.
fn swap_image(size: usize, at: usize) -> Image {
    let mut img = Image::new(size);
    img.put(at + 4086, b"SWAPSPACE2")
        .le32(at + 1024, 1)
        .le32(at + 1024 + 4, 15)
        .put(at + 1024 + 12, &[0x5a; 16])
        .put(at + 1024 + 28, b"swaplabel");
    img
}

#[test]
fn swap_values() {
    let dir = ScratchDir::new("ulblkid-swap");
    let mut pr = probe_of(&dir, "img", &swap_image(64 << 10, 0));
    pr.set_superblocks_flags(SUBLKS_DEFAULT | SUBLKS_VERSION | SUBLKS_FSINFO | SUBLKS_MAGIC);
    assert_eq!(pr.do_safeprobe(), PROBE_OK);
    assert_eq!(value(&pr, "TYPE").as_deref(), Some("swap"));
    assert_eq!(value(&pr, "LABEL").as_deref(), Some("swaplabel"));
    assert_eq!(
        value(&pr, "UUID").as_deref(),
        Some("5a5a5a5a-5a5a-5a5a-5a5a-5a5a5a5a5a5a")
    );
    assert_eq!(value(&pr, "VERSION").as_deref(), Some("1"));
    assert_eq!(value(&pr, "FSBLOCKSIZE").as_deref(), Some("4096"));
    assert_eq!(value(&pr, "FSSIZE").as_deref(), Some("61440"));
    assert_eq!(value(&pr, "ENDIANNESS").as_deref(), Some("LITTLE"));
    assert_eq!(value(&pr, "SBMAGIC_OFFSET").as_deref(), Some("4086"));
    assert_eq!(pr.lookup_value("SBMAGIC").unwrap().data(), b"SWAPSPACE2");
    // A string value counts its NUL; binary data does not.
    assert_eq!(pr.lookup_value("LABEL").unwrap().len(), 10);
}

#[test]
fn nothing_on_zeros_and_filters_work() {
    let dir = ScratchDir::new("ulblkid-filter");
    let mut pr = probe_of(&dir, "zeros", &Image::new(64 << 10));
    assert_eq!(pr.do_safeprobe(), PROBE_NONE);
    let mut pr = probe_of(&dir, "swap", &swap_image(64 << 10, 0));
    assert_eq!(pr.filter_superblocks_type(FLTR_NOTIN, &[b"swap"]), 0);
    assert_eq!(pr.do_safeprobe(), PROBE_NONE);
    assert_eq!(pr.invert_superblocks_filter(), 0);
    assert_eq!(pr.do_safeprobe(), PROBE_OK);
    assert_eq!(value(&pr, "TYPE").as_deref(), Some("swap"));
}

#[test]
fn a_window_probes_what_is_inside_it() {
    let dir = ScratchDir::new("ulblkid-window");
    let mut pr = probe_of(&dir, "img", &swap_image(2 << 20, 1 << 20));
    assert_eq!(pr.do_safeprobe(), PROBE_NONE);
    pr.set_dimension(1 << 20, 1 << 20);
    pr.reset_probe();
    assert_eq!(pr.do_safeprobe(), PROBE_OK);
    assert_eq!(value(&pr, "TYPE").as_deref(), Some("swap"));
    // A device of 1 KiB or less is not probed for superblocks.
    pr.set_dimension(1 << 20, 1024);
    pr.reset_probe();
    assert_eq!(pr.do_safeprobe(), PROBE_NONE);
}

#[test]
fn wipefs_walks_every_signature() {
    // swap and a DOS table on one device: do_probe reports each in chain
    // order, and a dry-run wipe hides each so the walk moves on.
    let dir = ScratchDir::new("ulblkid-wipe");
    let mut img = swap_image(3 << 20, 0);
    img.mbr_entry(0, 0, 0, 0x83, 2048, 100).mbr_magic(0);
    let mut pr = probe_of(&dir, "img", &img);
    pr.set_superblocks_flags(SUBLKS_DEFAULT | SUBLKS_MAGIC);
    pr.enable_partitions(true);
    pr.set_partitions_flags(PARTS_MAGIC);
    let mut seen = Vec::new();
    loop {
        let rc = pr.do_probe();
        if rc != PROBE_OK {
            assert_eq!(rc, PROBE_NONE);
            break;
        }
        let what = value(&pr, "TYPE").or_else(|| value(&pr, "PTTYPE")).unwrap();
        let off = value(&pr, "SBMAGIC_OFFSET")
            .or_else(|| value(&pr, "PTMAGIC_OFFSET"))
            .unwrap();
        seen.push((what, off));
        assert_eq!(pr.do_wipe(true), 0);
        assert!(seen.len() < 5, "the walk does not end: {seen:?}");
    }
    assert_eq!(
        seen,
        vec![
            ("swap".to_owned(), "4086".to_owned()),
            ("dos".to_owned(), "510".to_owned())
        ]
    );
}

// ---------------------------------------------------------------- probe

#[test]
fn buffers_are_windows_of_what_was_read() {
    let dir = ScratchDir::new("ulblkid-buf");
    let mut img = Image::new(4096);
    for (k, b) in img.0.iter_mut().enumerate() {
        *b = (k % 251) as u8;
    }
    let mut pr = probe_of(&dir, "img", &img);
    let whole = pr.get_buffer(0, 1024).unwrap();
    assert_eq!(whole.len(), 1024);
    // Served from the KiB already read: its tail runs on past the sector.
    let s = pr.get_sector(0).unwrap();
    assert_eq!(s.len(), 512);
    assert_eq!(s.through_end().len(), 1024);
    assert_eq!(s.through_end()[600], (600 % 251) as u8);
    // Outside the device: nothing, and no error.
    assert!(pr.get_buffer(4000, 200).is_none());
    assert_eq!(pr.errno, 0);
    // Hidden bytes read as zeros until the buffers are dropped.
    assert_eq!(pr.hide_range(10, 4), 0);
    assert_eq!(
        &pr.get_buffer(8, 8).unwrap()[..],
        &[8, 9, 0, 0, 0, 0, 14, 15]
    );
    pr.reset_buffers();
    assert_eq!(pr.get_buffer(10, 1).unwrap()[0], 10);
}

#[test]
fn hints_parse_as_upstream() {
    let mut pr = Probe::new();
    assert_eq!(pr.set_hint(b"session_offset=1024", 0), 0);
    assert_eq!(pr.get_hint(b"session_offset"), Some(1024));
    assert_eq!(pr.set_hint(b"other", 7), 0);
    assert_eq!(pr.get_hint(b"other"), Some(7));
    assert!(pr.set_hint(b"bad=x1", 0) < 0);
    pr.reset_hints();
    assert_eq!(pr.get_hint(b"other"), None);
    assert_eq!(
        crate::probe::parse_tag_string(b"LABEL=\"my disk\""),
        Some((b"LABEL".to_vec(), b"my disk".to_vec()))
    );
    assert_eq!(crate::probe::parse_tag_string(b"LABEL="), None);
    assert_eq!(crate::probe::parse_tag_string(b"LABEL=\"open"), None);
    assert_eq!(crate::probe::parse_tag_string(b"novalue"), None);
}

#[test]
fn the_tables_know_their_names() {
    assert!(crate::partitions::known_pttype(b"gpt"));
    assert!(crate::partitions::known_pttype(b"PMBR"));
    assert!(!crate::partitions::known_pttype(b"ext4"));
    assert!(crate::superblocks::known_fstype(b"ext4"));
    assert!(!crate::superblocks::known_fstype(b"dos"));
    assert_eq!(crate::partitions::get_name(0), Some("aix"));
    assert_eq!(crate::partitions::get_name(13), None);
    assert_eq!(
        crate::superblocks::get_name(0),
        Some(("crypto_LUKS", crate::USAGE_CRYPTO))
    );
}
