use super::*;
use crate::testing::{archive, dir, file, header, padded, seal, tar};
use std::io::Cursor;

fn listed(bytes: &[u8]) -> Listing {
    list(&mut Cursor::new(bytes.to_vec())).expect("list")
}

fn names(l: &Listing) -> Vec<String> {
    l.entries
        .iter()
        .map(|e| String::from_utf8_lossy(&e.name).into_owned())
        .collect()
}

/// The bytes of `e` in `a`.
fn data<'a>(a: &'a [u8], e: &Entry) -> &'a [u8] {
    &a[e.offset as usize..(e.offset + e.size) as usize]
}

#[test]
fn a_ustar_archive_is_listed_with_where_each_member_is() {
    let a = archive(&[
        dir("docs/"),
        file("docs/readme.txt", b"hello, archive"),
        file("empty", b""),
        header(b"link", b'2', 0, b"docs/readme.txt"),
        file("big.bin", &[7; 1500]),
    ]);
    let l = listed(&a);
    assert_eq!(l.end, End::Marker);
    assert_eq!(
        names(&l),
        ["docs/", "docs/readme.txt", "empty", "link", "big.bin"]
    );
    let kinds: Vec<Kind> = l.entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            Kind::Directory,
            Kind::File,
            Kind::File,
            Kind::Symlink,
            Kind::File
        ]
    );
    assert_eq!(data(&a, &l.entries[1]), b"hello, archive");
    assert_eq!(l.entries[2].size, 0);
    assert_eq!(l.entries[3].link.as_deref(), Some(&b"docs/readme.txt"[..]));
    assert_eq!(data(&a, &l.entries[4]), &[7; 1500][..]);
    assert_eq!(l.entries[1].mode, 0o644);
    assert_eq!(l.entries[1].mtime, 0o14712316540);
}

#[test]
fn every_kind_of_member_is_named() {
    for (flag, kind) in [
        (b'0', Kind::File),
        (0, Kind::File),
        (b'7', Kind::File),
        (b'1', Kind::HardLink),
        (b'3', Kind::CharDevice),
        (b'4', Kind::BlockDevice),
        (b'6', Kind::Fifo),
        (b'V', Kind::Other(b'V')),
    ] {
        let l = listed(&archive(&[header(b"m", flag, 0, b"")]));
        assert_eq!(l.entries[0].kind, kind, "{flag}");
    }
}

#[test]
fn a_ustar_prefix_is_joined_to_the_name() {
    let mut h = header(b"file.txt", b'0', 0, b"");
    h[345..345 + 9].copy_from_slice(b"some/deep");
    seal(&mut h);
    let l = listed(&archive(&[h]));
    assert_eq!(names(&l), ["some/deep/file.txt"]);
    // GNU's magic uses those bytes for other things: no prefix.
    let mut gnu = header(b"file.txt", b'0', 0, b"");
    gnu[257..265].copy_from_slice(b"ustar  \0");
    gnu[345..345 + 9].copy_from_slice(b"some/deep");
    seal(&mut gnu);
    assert_eq!(names(&listed(&archive(&[gnu]))), ["file.txt"]);
}

#[test]
fn gnu_long_names_and_link_targets_are_the_next_members() {
    let long = "d/".repeat(120) + "end.txt";
    let target = "t/".repeat(80) + "target";
    let mut l_entry = header(b"././@LongLink", b'L', (long.len() + 1) as u64, b"");
    let mut name = long.clone().into_bytes();
    name.push(0);
    l_entry.extend(padded(&name));
    let mut k_entry = header(b"././@LongLink", b'K', target.len() as u64, b"");
    k_entry.extend(padded(target.as_bytes()));
    let a = archive(&[
        l_entry,
        file("short-truncated", b"x"),
        k_entry,
        header(b"ln", b'2', 0, b"short"),
        file("plain", b"y"),
    ]);
    let l = listed(&a);
    assert_eq!(names(&l), [long.as_str(), "ln", "plain"]);
    assert_eq!(l.entries[1].link.as_deref(), Some(target.as_bytes()));
    assert_eq!(data(&a, &l.entries[0]), b"x");
    assert_eq!(
        l.entries[2].link, None,
        "a long name is for one member only"
    );
}

/// PAX records, `LENGTH KEY=VALUE\n`, the length counting itself.
fn pax(records: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in records {
        let body = format!(" {k}={v}\n");
        let mut n = body.len() + 1;
        while format!("{n}").len() + body.len() != n {
            n += 1;
        }
        out.extend(format!("{n}{body}").into_bytes());
    }
    out
}

#[test]
fn pax_records_give_the_name_the_size_and_the_time() {
    let records = pax(&[
        (
            "path",
            "caf\u{e9}/a very long name that ustar could not hold.txt",
        ),
        ("mtime", "1700000000.75"),
        ("linkpath", "elsewhere"),
    ]);
    let mut x = header(b"PaxHeaders/x", b'x', records.len() as u64, b"");
    x.extend(padded(&records));
    let global = pax(&[("mtime", "-5.5")]);
    let mut g = header(b"GlobalHead", b'g', global.len() as u64, b"");
    g.extend(padded(&global));
    let a = archive(&[x, file("fallback", b"data"), g, file("next", b"z")]);
    let l = listed(&a);
    assert_eq!(
        names(&l),
        [
            "caf\u{e9}/a very long name that ustar could not hold.txt",
            "next"
        ]
    );
    assert_eq!(l.entries[0].mtime, 1_700_000_000);
    assert_eq!(l.entries[0].link.as_deref(), Some(&b"elsewhere"[..]));
    assert_eq!(data(&a, &l.entries[0]), b"data");
    assert_eq!(
        l.entries[1].mtime, -6,
        "a global time, a fraction before -5"
    );
}

#[test]
fn a_pax_size_is_the_members_size() {
    // A member over 8 GiB has its size in a PAX record and 0 in its header:
    // here, a smaller one says so the same way.
    let records = pax(&[("size", "3000")]);
    let mut x = header(b"PaxHeaders/x", b'x', records.len() as u64, b"");
    x.extend(padded(&records));
    let mut m = header(b"sized", b'0', 0, b"");
    m.extend(padded(&[1; 3000]));
    let a = archive(&[x, m, file("after", b"a")]);
    let l = listed(&a);
    assert_eq!(names(&l), ["sized", "after"]);
    assert_eq!(l.entries[0].size, 3000);
    assert_eq!(
        data(&a, &l.entries[1]),
        b"a",
        "the next header is after the PAX size"
    );
}

#[test]
fn base_256_numbers_are_read() {
    let mut h = header(b"huge", b'0', 0, b"");
    // Size 2^33 + 5, base-256; a time before 1970.
    h[124..136].copy_from_slice(&[0x80, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0, 5]);
    h[136..148].copy_from_slice(&[0xFF; 12]);
    seal(&mut h);
    let l = listed(&h);
    // The member runs past the end of this file: damage, but its header read.
    assert_eq!(
        l.end,
        End::Damaged {
            at: 0,
            why: Damage::Truncated
        }
    );
    assert_eq!(
        number(&[0x80, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0, 5]),
        Some((1 << 33) + 5)
    );
    assert_eq!(number(&[0xFF; 12]), Some(-1));
    assert_eq!(
        number(&[
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE
        ]),
        Some(-2)
    );
}

#[test]
fn octal_fields_are_read_as_tars_write_them() {
    assert_eq!(number(b"0000644\0"), Some(0o644));
    assert_eq!(number(b"   644 \0"), Some(0o644));
    assert_eq!(number(b"\0\0\0\0\0\0\0\0"), Some(0));
    assert_eq!(number(b"0000648\0"), None, "8 is not an octal digit");
    assert_eq!(number(b"77777777777777777777777"), None, "too large");
}

#[test]
fn a_header_that_does_not_add_up_stops_the_listing_and_says_where() {
    let mut bad = file("second", b"2");
    bad[0] = b'S';
    let a = archive(&[file("first", b"1"), bad, file("third", b"3")]);
    let l = listed(&a);
    assert_eq!(names(&l), ["first"], "what came before still lists");
    assert_eq!(
        l.end,
        End::Damaged {
            at: 1024,
            why: Damage::Checksum
        }
    );
    assert_eq!(
        Damage::Checksum.to_string(),
        "a header's checksum does not add up"
    );
}

#[test]
fn a_signed_checksum_is_a_checksum() {
    // A name with a byte past 127, summed as a signed byte by an old tar.
    let mut h = header(b"caf\xe9", b'0', 0, b"");
    h[148..156].copy_from_slice(b"        ");
    let signed: i64 = h.iter().map(|&b| i64::from(b as i8)).sum();
    h[148..156].copy_from_slice(format!("{signed:06o}\0 ").as_bytes());
    assert!(is_header(&h));
    assert_eq!(names(&listed(&archive(&[h]))), ["caf\u{fffd}"]);
}

#[test]
fn a_member_cut_short_and_an_archive_with_no_end_are_told_apart() {
    let whole = archive(&[file("a", b"aaaa"), file("b", &[2; 700])]);
    // No closing blocks: nothing is missing.
    let unclosed = &whole[..whole.len() - 1024];
    let l = listed(unclosed);
    assert_eq!((names(&l).len(), l.end.clone()), (2, End::EndOfFile));
    // The second member's bytes cut: damage, at its header.
    let cut = &whole[..512 + 512 + 512 + 100];
    let l = listed(cut);
    assert_eq!(names(&l), ["a"]);
    assert_eq!(
        l.end,
        End::Damaged {
            at: 1024,
            why: Damage::Truncated
        }
    );
}

#[test]
fn a_long_name_or_pax_header_larger_than_anyone_writes_is_damage() {
    let l = listed(&archive(&[header(b"././@LongLink", b'L', 1 << 20, b"")]));
    assert_eq!(
        l.end,
        End::Damaged {
            at: 0,
            why: Damage::TooLong
        }
    );
    let mut x = header(b"x", b'x', 12, b"");
    x.extend(padded(b"99 path=abc\n"));
    let l = listed(&archive(&[x, file("f", b"")]));
    assert!(
        matches!(
            l.end,
            End::Damaged {
                why: Damage::Truncated | Damage::Number(_),
                ..
            }
        ),
        "{:?}",
        l.end
    );
}

#[test]
fn what_is_not_an_archive_is_not_a_header() {
    assert!(!is_header(&[0; 512]));
    assert!(!is_header(b"not a tar at all"));
    let mut text = vec![b'a'; 512];
    text[148..156].copy_from_slice(b"0000000\0");
    assert!(!is_header(&text));
    assert!(is_header(&file("x", b"")));
    let l = listed(b"");
    assert_eq!((l.entries.len(), l.end), (0, End::EndOfFile));
    let l = listed(&[b'z'; 700]);
    assert_eq!(
        l.end,
        End::Damaged {
            at: 0,
            why: Damage::Checksum
        }
    );
}

#[test]
fn every_prefix_of_an_archive_lists_without_a_panic() {
    let a = tar(&[("one", b"1".as_slice()), ("two", &[2; 600])]);
    for cut in 0..a.len() {
        let _ = listed(&a[..cut]);
        let _ = list_bytes(&a[..cut]);
    }
}

#[test]
fn list_bytes_is_list() {
    let a = tar(&[("x", b"xyz".as_slice())]);
    assert_eq!(list_bytes(&a), listed(&a));
}

// ============================================================================
// Writing
// ============================================================================

/// An archive written with the writer: each member's header, its bytes and
/// their padding, then the end.
fn written(members: &[(NewMember, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (m, data) in members {
        write_header(&mut out, m).unwrap();
        out.extend_from_slice(data);
        write_padding(&mut out, data.len() as u64).unwrap();
    }
    write_end(&mut out).unwrap();
    out
}

fn member(name: &[u8], kind: Kind, size: u64) -> NewMember<'_> {
    NewMember {
        name,
        kind,
        mode: 0o640,
        mtime: 1_700_000_000,
        size,
        link: None,
    }
}

#[test]
fn what_is_written_lists_back_as_it_was() {
    let long = "deep/".repeat(40) + "file.txt"; // 208 bytes: a prefix split
    let very_long = "x".repeat(150) + "/" + &"y".repeat(150); // no split fits
    let target = "t/".repeat(60) + "target"; // a link past 100 bytes
    let a = written(&[
        (member(b"dir/", Kind::Directory, 0), b""),
        (member(b"dir/a.txt", Kind::File, 5), b"hello"),
        (member(long.as_bytes(), Kind::File, 3), b"abc"),
        (member(very_long.as_bytes(), Kind::File, 600), &[9; 600]),
        (
            NewMember {
                link: Some(target.as_bytes()),
                ..member(b"ln", Kind::Symlink, 0)
            },
            b"",
        ),
        (
            NewMember {
                mtime: -86_400,
                ..member(b"old", Kind::File, 1)
            },
            b"o",
        ),
    ]);
    let l = listed(&a);
    assert_eq!(l.end, End::Marker);
    assert_eq!(
        names(&l),
        [
            "dir/",
            "dir/a.txt",
            long.as_str(),
            very_long.as_str(),
            "ln",
            "old"
        ]
    );
    assert_eq!(l.entries[0].kind, Kind::Directory);
    assert_eq!(data(&a, &l.entries[1]), b"hello");
    assert_eq!(data(&a, &l.entries[2]), b"abc");
    assert_eq!(data(&a, &l.entries[3]), &[9; 600][..]);
    assert_eq!(l.entries[4].link.as_deref(), Some(target.as_bytes()));
    assert_eq!(
        l.entries[5].mtime, -86_400,
        "a time before 1970, through PAX"
    );
    assert_eq!(l.entries[1].mode, 0o640);
    assert_eq!(l.entries[1].mtime, 1_700_000_000);
    // Only the members that needed one have a PAX header: the rest are plain
    // ustar, which every reader reads.
    let pax_headers = a
        .chunks(512)
        .filter(|b| is_header(b) && b[156] == b'x')
        .count();
    assert_eq!(pax_headers, 3, "very long name, long link, negative time");
}

#[test]
fn a_size_past_eleven_octal_digits_goes_in_a_pax_record() {
    let mut out = Vec::new();
    write_header(&mut out, &member(b"huge", Kind::File, 1 << 40)).unwrap();
    // The PAX header, its record, then the member's own header.
    assert_eq!(out[156], b'x');
    let record = until_nul(&out[512..1024]);
    assert_eq!(record, format!("22 size={}\n", 1_u64 << 40).as_bytes());
    assert!(is_header(&out[1024..1536]));
    assert_eq!(number(&out[1024 + 124..1024 + 136]), Some(0));
}

#[test]
fn a_pax_record_counts_its_own_length() {
    let mut out = Vec::new();
    pax_record(&mut out, b"path", b"a");
    assert_eq!(out, b"9 path=a\n", "nine bytes, the 9 among them");
    // Where adding the length's own digits crosses a power of ten.
    let mut out = Vec::new();
    let value = "v".repeat(92);
    pax_record(&mut out, b"path", value.as_bytes());
    let text = String::from_utf8(out).unwrap();
    let (n, _) = text.split_once(' ').unwrap();
    assert_eq!(n.parse::<usize>().unwrap(), text.len());
}

#[test]
fn a_name_is_split_at_the_slash_that_lets_both_parts_fit() {
    assert_eq!(split_name(b"short"), Some((&b""[..], &b"short"[..])));
    let name = [b"p".repeat(120), b"/".to_vec(), b"n".repeat(90)].concat();
    let (prefix, rest) = split_name(&name).unwrap();
    assert_eq!((prefix.len(), rest.len()), (120, 90));
    assert_eq!(
        split_name(&[b"p".repeat(160), b"/".to_vec(), b"n".repeat(90)].concat()),
        None,
        "prefix too long"
    );
    assert_eq!(split_name(&b"n".repeat(101)), None, "no slash");
    assert_eq!(
        split_name(&[b"p".repeat(100), b"/".to_vec()].concat()),
        None,
        "nothing after the slash"
    );
}

#[test]
fn every_kind_has_the_flag_it_is_read_back_as() {
    for kind in [
        Kind::File,
        Kind::Directory,
        Kind::Symlink,
        Kind::HardLink,
        Kind::CharDevice,
        Kind::BlockDevice,
        Kind::Fifo,
        Kind::Other(b'V'),
    ] {
        assert_eq!(Kind::from_flag(kind.flag()), kind);
    }
}

#[test]
fn padding_fills_to_a_block_and_the_end_is_two() {
    for (size, pad) in [(0, 0), (1, 511), (511, 1), (512, 0), (513, 511)] {
        let mut out = Vec::new();
        write_padding(&mut out, size).unwrap();
        assert_eq!(out.len(), pad, "{size}");
    }
    let mut end = Vec::new();
    write_end(&mut end).unwrap();
    assert_eq!(end, [0; 1024]);
}
