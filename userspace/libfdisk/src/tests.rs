#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

//! The library driven as `fdisk` and `sfdisk` drive it, on image files:
//! each label made, written, and probed back.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::script::{FileInput, Script, ScriptRef};

/// The messages the library printed.
type Log = Rc<RefCell<Vec<Vec<u8>>>>;

/// An image of `mib` MiB of zeros in `dir`; its path.
fn image(dir: &scratchdir::ScratchDir, name: &str, mib: u64) -> Vec<u8> {
    let p = dir.path(name);
    std::fs::File::create(&p)
        .unwrap()
        .set_len(mib * 1024 * 1024)
        .unwrap();
    p.to_str().unwrap().as_bytes().to_vec()
}

/// A context on `path` that records every message and answers no
/// question, with `geometry` (cylinders, heads, sectors) as the user's.
fn context(path: &[u8], geometry: Option<(u32, u32, u32)>) -> (Context, Log) {
    let mut cxt = Context::new();
    let log: Log = Rc::default();
    let sink = Rc::clone(&log);
    cxt.set_ask(Some(Rc::new(
        move |_: &mut Context, ask: &mut Ask| match ask.get_type() {
            AskType::Info | AskType::Warn | AskType::Warnx => {
                sink.borrow_mut()
                    .push(ask.print_get_mesg().unwrap_or_default().to_vec());
                0
            }
            _ => -errno::EINVAL,
        },
    )));
    if let Some((c, h, s)) = geometry {
        assert_eq!(cxt.save_user_geometry(c, h, s), 0);
    }
    assert_eq!(cxt.assign_device(path, false), 0);
    (cxt, log)
}

/// The image's bytes.
fn read(path: &[u8]) -> Vec<u8> {
    std::fs::read(std::str::from_utf8(path).unwrap()).unwrap()
}

/// A partition template: number, start, size and a type as the label
/// parses it.
fn part(cxt: &Context, partno: usize, start: u64, size: u64, ty: &[u8]) -> Partition {
    let mut pa = Partition::new();
    assert_eq!(pa.set_partno(partno), 0);
    assert_eq!(pa.set_start(start), 0);
    assert_eq!(pa.set_size(size), 0);
    pa.ty = cxt.label_ref().unwrap().parse_parttype(ty);
    assert!(pa.ty.is_some(), "type {}", String::from_utf8_lossy(ty));
    pa
}

/// The label's partitions: number, start, size and type code.
fn partitions(cxt: &mut Context) -> Vec<(usize, u64, u64, u32)> {
    let mut tb = Table::new();
    assert_eq!(cxt.get_partitions(&mut tb), 0);
    tb.iter()
        .map(|p| {
            (
                p.partno,
                p.start,
                p.size,
                p.ty.as_ref().map_or(0, |t| t.code),
            )
        })
        .collect()
}

/// A big-endian field of `raw`.
fn be(raw: &[u8], at: usize, n: usize) -> u32 {
    utils::be_get(raw, at, n)
}

#[test]
fn a_dos_label_is_written_and_read_back() {
    let dir = scratchdir::ScratchDir::new("libfdisk-dos");
    let img = image(&dir, "disk.img", 16);
    let (mut cxt, _) = context(&img, None);
    assert_eq!(cxt.create_disklabel(Some(b"dos")), 0);
    for (n, start, size, ty) in [(0, 2048, 8192, &b"83"[..]), (1, 10240, 4096, b"82")] {
        let mut pa = part(&cxt, n, start, size, ty);
        assert_eq!(cxt.add_partition(Some(&mut pa), None), 0);
    }
    assert_eq!(cxt.write_disklabel(), 0);
    assert_eq!(cxt.deassign_device(false), 0);

    let raw = read(&img);
    assert_eq!(&raw[510..512], &[0x55, 0xaa]);
    let first = &raw[446..462];
    assert_eq!(first[4], 0x83);
    assert_eq!(u32::from_le_bytes(first[8..12].try_into().unwrap()), 2048);
    assert_eq!(u32::from_le_bytes(first[12..16].try_into().unwrap()), 8192);
    assert_eq!(raw[462 + 4], 0x82);

    let (mut cxt, _) = context(&img, None);
    assert!(cxt.is_labeltype(LabelType::Dos));
    assert_eq!(
        partitions(&mut cxt),
        vec![(0, 2048, 8192, 0x83), (1, 10240, 4096, 0x82)]
    );
}

#[test]
fn a_gpt_label_is_written_with_both_headers_and_read_back() {
    let dir = scratchdir::ScratchDir::new("libfdisk-gpt");
    let img = image(&dir, "disk.img", 16);
    let (mut cxt, _) = context(&img, None);
    assert_eq!(cxt.create_disklabel(Some(b"gpt")), 0);
    let mut pa = part(&cxt, 0, 2048, 4096, b"0FC63DAF-8483-4772-8E79-3D69D8477DE4");
    assert_eq!(cxt.add_partition(Some(&mut pa), None), 0);
    assert_eq!(cxt.write_disklabel(), 0);
    assert_eq!(cxt.deassign_device(false), 0);

    let raw = read(&img);
    // The protective MBR, the primary header and the backup at the end.
    assert_eq!(raw[446 + 4], 0xee);
    assert_eq!(&raw[512..520], b"EFI PART");
    assert_eq!(&raw[raw.len() - 512..raw.len() - 504], b"EFI PART");

    let (mut cxt, log) = context(&img, None);
    assert!(cxt.is_labeltype(LabelType::Gpt));
    assert_eq!(partitions(&mut cxt), vec![(0, 2048, 4096, 0)]);
    assert_eq!(cxt.verify_disklabel(), 0, "{:?}", log.borrow());
}

#[test]
fn an_sgi_label_gets_its_volume_and_volume_header_and_a_valid_checksum() {
    let dir = scratchdir::ScratchDir::new("libfdisk-sgi");
    let img = image(&dir, "disk.img", 16);
    let (mut cxt, _) = context(&img, Some((0, 16, 63)));
    assert_eq!(cxt.create_disklabel(Some(b"sgi")), 0);
    // 16 MiB is 32 cylinders of 16 x 63 sectors.
    assert_eq!(
        partitions(&mut cxt),
        vec![(8, 0, 4096, 0x00), (10, 0, 32 * 16 * 63, 0x06)]
    );
    assert_eq!(cxt.write_disklabel(), 0);
    assert_eq!(cxt.deassign_device(false), 0);

    let raw = read(&img);
    assert_eq!(be(&raw, 0, 4), 0x0be5_a941);
    let sum = (0..128).fold(0u32, |s, k| s.wrapping_add(be(&raw, k * 4, 4)));
    assert_eq!(sum, 0, "the words of a valid SGI label sum to zero");
    assert_eq!(&raw[8..13], b"/unix");

    let (mut cxt, log) = context(&img, Some((0, 16, 63)));
    assert!(cxt.is_labeltype(LabelType::Sgi), "{:?}", log.borrow());
    assert_eq!(partitions(&mut cxt).len(), 2);
}

#[test]
fn a_sun_label_gets_upstreams_default_layout_and_a_valid_checksum() {
    let dir = scratchdir::ScratchDir::new("libfdisk-sun");
    let img = image(&dir, "disk.img", 16);
    let (mut cxt, _) = context(&img, Some((0, 16, 63)));
    assert_eq!(cxt.create_disklabel(Some(b"sun")), 0);
    // A disk under 150 MiB: two thirds of its 32 cylinders Linux, the
    // rest swap, and the whole disk as the third.
    let cyl = 16 * 63;
    assert_eq!(
        partitions(&mut cxt),
        vec![
            (0, 0, 21 * cyl, 0x83),
            (1, 21 * cyl, 11 * cyl, 0x82),
            (2, 0, 32 * cyl, 0x05)
        ]
    );
    assert_eq!(cxt.write_disklabel(), 0);
    assert_eq!(cxt.deassign_device(false), 0);

    let raw = read(&img);
    assert_eq!(be(&raw, 508, 2), 0xdabe);
    let xor = raw[..512]
        .chunks(2)
        .fold([0u8; 2], |s, w| [s[0] ^ w[0], s[1] ^ w[1]]);
    assert_eq!(xor, [0, 0], "the words of a valid Sun label XOR to zero");
    assert_eq!(&raw[..31], b"Linux cyl 32 alt 0 hd 16 sec 63");

    let (mut cxt, log) = context(&img, None);
    assert!(cxt.is_labeltype(LabelType::Sun), "{:?}", log.borrow());
    let mut tb = Table::new();
    assert_eq!(cxt.get_partitions(&mut tb), 0);
    // Swap is unmountable.
    assert_eq!(
        tb.get_partition(1).unwrap().attrs.as_deref(),
        Some(&b"u "[..])
    );
}

/// A script read from `text` for `cxt`.
fn script(cxt: &mut Context, text: &[u8]) -> (Script, i32) {
    let mut dp = Script::new();
    let rc = dp.read_file(cxt, &mut FileInput::new(text));
    (dp, rc)
}

#[test]
fn a_script_reads_headers_and_both_partition_line_formats() {
    let dir = scratchdir::ScratchDir::new("libfdisk-script");
    let img = image(&dir, "disk.img", 16);
    let (mut cxt, _) = context(&img, None);
    let (mut dp, rc) = script(
        &mut cxt,
        b"# a comment\n\
          label: dos\n\
          unit: sectors\n\
          \n\
          /dev/sda1 : start=2048, size=4096, type=83, bootable\n\
          start= 6144, size=1M, Id=82, name=\"a\\x20b\"\n\
          8192,,L,*\r\n\
          ,+\n",
    );
    assert_eq!(rc, 0);
    assert_eq!(dp.get_nlines(), 8);
    assert!(dp.has_force_label());
    assert_eq!(dp.get_header(b"LABEL"), Some(&b"dos"[..]));
    let parts: Vec<_> = dp.get_table().iter().cloned().collect();
    assert_eq!(parts.len(), 4);

    let (a, b, c, d) = (&parts[0], &parts[1], &parts[2], &parts[3]);
    assert_eq!((a.partno, a.start, a.size, a.boot), (0, 2048, 4096, 1));
    assert_eq!(a.ty.as_ref().unwrap().code, 0x83);
    assert!(a.size_explicit);
    // A size with a suffix is bytes, in sectors.
    assert_eq!(
        (b.start, b.size, b.ty.as_ref().unwrap().code),
        (6144, 2048, 0x82)
    );
    assert!(!b.size_explicit && b.partno_follow_default);
    assert_eq!(b.name.as_deref(), Some(&b"a b"[..]));
    assert_eq!(
        (c.start, c.ty.as_ref().unwrap().code, c.boot),
        (8192, 0x83, 1)
    );
    assert!(c.end_follow_default);
    assert!(d.start_follow_default && d.end_follow_default && d.resize == RESIZE_ENLARGE);
}

#[test]
fn a_script_refuses_what_upstream_refuses() {
    let dir = scratchdir::ScratchDir::new("libfdisk-script-bad");
    let img = image(&dir, "disk.img", 16);
    let (mut cxt, _) = context(&img, None);
    // An unknown header is skipped; an unknown label, unit or field is not.
    assert_eq!(script(&mut cxt, b"colour: blue\nlabel: dos\n").1, 0);
    assert_eq!(script(&mut cxt, b"label: atari\n").1, -errno::EINVAL);
    assert_eq!(script(&mut cxt, b"unit: cylinders\n").1, -errno::EINVAL);
    assert_eq!(script(&mut cxt, b"sector-size: 1000\n").1, -errno::ERANGE);
    assert_eq!(
        script(&mut cxt, b"start=1, colour=blue\n").1,
        -errno::EINVAL
    );
    assert_eq!(
        script(&mut cxt, b"label: dos\n1,2,83,x\n").1,
        -errno::EINVAL
    );
    // A type no rule recognises is not refused: it is an unknown type,
    // code 0 on an MBR.
    let (mut dp, rc) = script(&mut cxt, b"label: dos\n1,2,nosuchtype\n");
    assert_eq!(rc, 0);
    let t = dp.get_table().get_partition(0).unwrap().ty.clone().unwrap();
    assert_eq!((t.code, t.get_name()), (0, Some(&b"unknown"[..])));
    assert_eq!(script(&mut cxt, b"label: dos\n1,2,83,*\n").1, 0);
}

#[test]
fn an_applied_script_makes_the_table_and_a_dump_describes_it() {
    let dir = scratchdir::ScratchDir::new("libfdisk-apply");
    let img = image(&dir, "disk.img", 16);
    let (mut cxt, _) = context(&img, None);
    let (dp, rc) = script(
        &mut cxt,
        b"label: dos\nlabel-id: 0x12345678\n2048,4096,83,*\n,4096,82\n",
    );
    assert_eq!(rc, 0);
    let dp: ScriptRef = Rc::new(RefCell::new(dp));
    assert_eq!(cxt.apply_script(&dp), 0);
    assert!(
        cxt.get_script().is_none(),
        "the script is followed only while it is applied"
    );
    assert_eq!(cxt.write_disklabel(), 0);
    assert_eq!(
        partitions(&mut cxt),
        vec![(0, 2048, 4096, 0x83), (1, 6144, 4096, 0x82)]
    );

    let mut dump = Script::new();
    assert_eq!(dump.read_context(&mut cxt), 0);
    let mut out = Vec::new();
    assert_eq!(dump.write_file(&cxt, &mut out), 0);
    let text = String::from_utf8(out).unwrap();
    let dev = std::str::from_utf8(&img).unwrap();
    assert_eq!(
        text,
        format!(
            "label: dos\nlabel-id: 0x12345678\ndevice: {dev}\nunit: sectors\nsector-size: 512\n\n\
             {dev}1 : start=        2048, size=        4096, type=83, bootable\n\
             {dev}2 : start=        6144, size=        4096, type=82\n"
        )
    );

    dump.enable_json(true);
    let mut out = Vec::new();
    assert_eq!(dump.write_file(&cxt, &mut out), 0);
    let json = String::from_utf8(out).unwrap();
    assert!(json.starts_with("{\n   \"partitiontable\": {\n      \"label\": \"dos\",\n      \"id\": \"0x12345678\",\n"), "{json}");
    assert!(json.contains("\"sectorsize\": 512,\n"), "{json}");
    assert!(json.contains("\"start\": 2048,\n            \"size\": 4096,\n            \"type\": \"83\",\n            \"bootable\": true\n"), "{json}");
    assert!(json.ends_with("      ]\n   }\n}\n"), "{json}");
}
