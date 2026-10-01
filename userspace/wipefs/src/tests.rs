//! Unit tests of the pieces that do not print, and of the signature walk
//! on an image; what the program prints is `scripts/wipefs-diff.sh`'s to
//! compare with upstream's.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests unwrap and index what they built"
)]

use super::*;

#[test]
fn basenames_are_posix() {
    assert_eq!(posix_basename(b"/dev/sda1"), b"sda1");
    assert_eq!(posix_basename(b"/dev/sda1/"), b"sda1");
    assert_eq!(posix_basename(b"img"), b"img");
    assert_eq!(posix_basename(b"///"), b"/");
    assert_eq!(posix_basename(b""), b".");
}

#[test]
fn offsets_are_strtoll() {
    assert_eq!(strtoll(b"1080"), Some(1080));
    assert_eq!(strtoll(b"-5"), Some(-5));
    assert_eq!(strtoll(b"x"), Some(0));
    assert_eq!(strtoll(b"99999999999999999999"), None);
}

#[test]
fn offsets_are_kept_once_each() {
    let mut v = Vec::new();
    assert_eq!(add_offset(&mut v, 10), 0);
    assert_eq!(add_offset(&mut v, 20), 1);
    assert_eq!(add_offset(&mut v, 10), 0);
    assert_eq!(v.len(), 2);
}

#[test]
fn columns_match_whole_names() {
    assert_eq!(
        column_name_to_id(b"uuid", b"uuid", b"wipefs"),
        Some(Col::Uuid)
    );
    assert_eq!(
        column_name_to_id(b"LENGTH", b"LENGTH", b"wipefs"),
        Some(Col::Len)
    );
    assert_eq!(column_name_to_id(b"LEN", b"LEN", b"wipefs"), None);
    assert_eq!(LONGS.len(), LONG_VALS.len());
    assert_eq!(option_to_longopt(i32::from(b'O')), Some("output"));
}

/// A 64 KiB swap area, with the DOS table of a disk in front of it.
fn two_signatures() -> Vec<u8> {
    let mut img = vec![0u8; 64 << 10];
    img[4086..4096].copy_from_slice(b"SWAPSPACE2");
    img[1024..1028].copy_from_slice(&1u32.to_le_bytes());
    img[1028..1032].copy_from_slice(&15u32.to_le_bytes());
    // A partition, then 55AA.
    img[0x1be + 4] = 0x83;
    img[0x1be + 8..0x1be + 12].copy_from_slice(&2048u32.to_le_bytes());
    img[0x1be + 12..0x1be + 16].copy_from_slice(&100u32.to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xAA;
    img
}

#[test]
fn every_signature_is_found_and_filtered() {
    let dir = scratchdir::ScratchDir::new("wipefs-read");
    let p = dir.path("img");
    std::fs::write(&p, two_signatures()).unwrap();
    let mut ctl = Ctl {
        devname: quoting::os_bytes(p.as_os_str()).into_owned(),
        ..Ctl::default()
    };
    let found = read_offsets(&mut ctl, b"wipefs").ok().unwrap();
    let got: Vec<(i64, Option<Vec<u8>>, usize)> = found
        .iter()
        .map(|w| (w.offset, w.ty.clone(), w.len))
        .collect();
    assert_eq!(
        got,
        vec![
            (4086, Some(b"swap".to_vec()), 10),
            (510, Some(b"dos".to_vec()), 2)
        ]
    );
    assert_eq!(found[1].usage.as_deref(), Some(&b"partition-table"[..]));
    // -t leaves out what it does not name.
    ctl.type_pattern = Some(b"dos".to_vec());
    let found = read_offsets(&mut ctl, b"wipefs").ok().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].offset, 510);
}
