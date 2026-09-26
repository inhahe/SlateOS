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
