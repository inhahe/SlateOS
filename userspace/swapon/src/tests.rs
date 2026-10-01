//! Unit tests of the header parsing and option handling; what the program
//! prints and does is `scripts/swapon-diff.sh`'s to compare with upstream.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests unwrap, index and size what they built"
)]

use super::*;

/// A swap area's first page, as `mkswap` makes it for `pagesize`: version
/// 1, `last_page`, a UUID and a label, the signature at the page's end.
fn area(pagesize: usize, last_page: u32, big_endian: bool) -> Vec<u8> {
    let mut b = vec![0u8; pagesize];
    let enc = |v: u32| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    b[1024..1028].copy_from_slice(&enc(1));
    b[1028..1032].copy_from_slice(&enc(last_page));
    b[1036..1052].copy_from_slice(&[
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        0xef,
    ]);
    b[1052..1057].copy_from_slice(b"swap1");
    b[pagesize - 10..].copy_from_slice(b"SWAPSPACE2");
    b
}

#[test]
fn signatures_are_found_at_the_end_of_a_page() {
    assert_eq!(swap_detect_signature(b"SWAPSPACE2"), Some(Sig::SwapSpace));
    assert_eq!(swap_detect_signature(b"S1SUSPEND\0"), Some(Sig::SwSuspend));
    assert_eq!(swap_detect_signature(b"LINHIB0001"), Some(Sig::SwSuspend));
    assert_eq!(swap_detect_signature(b"SWAP-SPACE"), None);
}

#[test]
fn the_page_size_is_where_the_signature_is() {
    let dir = scratchdir::ScratchDir::new("swapon-header");
    for pagesize in [4096usize, 8192, 16384, 65536] {
        let p = dir.path(&format!("area{pagesize}"));
        std::fs::write(&p, area(pagesize, 9, false)).unwrap();
        let mut f = File::open(&p).unwrap();
        let (_, sig, got) = swap_get_header(&mut f).unwrap();
        assert_eq!((sig, got), (Sig::SwapSpace, pagesize));
    }
    // 32 KiB pages are not swap's, and are not looked for.
    let p = dir.path("area32k");
    std::fs::write(&p, area(32768, 9, false)).unwrap();
    assert!(swap_get_header(&mut File::open(&p).unwrap()).is_none());
    // Too short for even the smallest page.
    let p = dir.path("short");
    std::fs::write(&p, b"SWAPSPACE2").unwrap();
    assert!(swap_get_header(&mut File::open(&p).unwrap()).is_none());
}

#[test]
fn the_size_and_identity_are_read_in_either_byte_order() {
    let dev = SwapDevice {
        pagesize: 4096,
        ..SwapDevice::default()
    };
    assert_eq!(swap_get_size(&dev, &area(4096, 9, false)), 10 * 4096);
    assert_eq!(swap_get_size(&dev, &area(4096, 9, true)), 10 * 4096);
    let mut dev = dev;
    swap_get_info(&mut dev, &area(4096, 9, false));
    assert_eq!(dev.label.as_deref(), Some(&b"swap1"[..]));
    assert_eq!(
        dev.uuid.as_deref(),
        Some(&b"12345678-9abc-def0-0123-456789abcdef"[..])
    );
}

#[test]
fn options_are_read_as_upstream_reads_them() {
    let mut p = SwapProp {
        priority: -1,
        ..SwapProp::default()
    };
    parse_options(&mut p, b"nofail,discard=pages,pri=7");
    assert!(p.no_fail);
    assert_eq!(p.discard, SWAP_FLAG_DISCARD | SWAP_FLAG_DISCARD_PAGES);
    assert_eq!(p.priority, 7);
    // A discard policy is compared over the bytes given: `on` is `once`.
    let mut p = SwapProp::default();
    parse_options(&mut p, b"discard=on");
    assert_eq!(p.discard, SWAP_FLAG_DISCARD | SWAP_FLAG_DISCARD_ONCE);
    // A priority without digits changes nothing.
    let mut p = SwapProp {
        priority: -1,
        ..SwapProp::default()
    };
    parse_options(&mut p, b"pri=x");
    assert_eq!(p.priority, -1);
}

#[test]
fn columns_are_named_whole_in_any_case() {
    assert_eq!(
        column_name_to_id(b"name", b"name", b"swapon"),
        Some(Col::Path)
    );
    assert_eq!(
        column_name_to_id(b"UUID", b"UUID", b"swapon"),
        Some(Col::Uuid)
    );
    assert_eq!(column_name_to_id(b"NAM", b"NAM", b"swapon"), None);
}

#[test]
fn a_status_is_its_low_byte() {
    assert_eq!(exit_byte(-1), 255);
    assert_eq!(exit_byte(0), 0);
}

#[test]
fn usage_is_upstreams() {
    let u = String::from_utf8(usage(b"swapon")).unwrap();
    assert!(u.starts_with("\nUsage:\n swapon [options] [<spec>]\n"));
    assert!(u.contains("\n NAME   device file or partition path\n"));
    // `%-26s` of an 11-byte option: 15 blanks.
    assert!(u.contains(" -h, --help               display this help\n"));
    assert!(u.ends_with("\nFor more details see swapon(8).\n"));
}
