//! Unit tests for the parts of `blockdev` that do not need a device: the
//! command table, the help it is printed into, and the reading of
//! `/proc/partitions`. What the requests do is `scripts/blockdev-diff.sh`'s
//! to measure, against upstream, under an `ioctl` shim.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests unwrap and index what they built"
)]

use super::*;

#[test]
fn the_table_is_upstreams_in_its_order() {
    let names: Vec<&str> = BDCMS.iter().map(|c| c.name).collect();
    assert_eq!(
        names,
        [
            "--setro",
            "--setrw",
            "--getro",
            "--getdiscardzeroes",
            "--getss",
            "--getpbsz",
            "--getiomin",
            "--getioopt",
            "--getalignoff",
            "--getmaxsect",
            "--getbsz",
            "--setbsz",
            "--getsize",
            "--getsize64",
            "--setra",
            "--getra",
            "--setfra",
            "--getfra",
            "--getdiskseq",
            "--flushbufs",
            "--rereadpt",
        ]
    );
}

#[test]
fn only_int_commands_take_an_argument() {
    for c in &BDCMS {
        if c.argname.is_some() {
            assert_eq!(c.argtype, Arg::Int, "{}", c.name);
            assert!(c.noresult, "{} sets, so it prints nothing", c.name);
        }
        if c.noptr {
            assert_eq!(c.argtype, Arg::Int, "{}", c.name);
        }
    }
}

#[test]
fn every_get_starts_at_minus_one() {
    for c in &BDCMS {
        if c.argtype != Arg::None && !c.noresult {
            assert_eq!(c.argval, -1, "{}", c.name);
        }
    }
    let setro = &BDCMS[find_cmd(b"--setro").unwrap()];
    let setrw = &BDCMS[find_cmd(b"--setrw").unwrap()];
    assert_eq!((setro.ioc, setro.argval), (BLKROSET, 1));
    assert_eq!((setrw.ioc, setrw.argval), (BLKROSET, 0));
}

#[test]
fn request_numbers_are_linuxs() {
    // `_IO(0x12, n)` and `_IOR`/`_IOW(0x12, n, size_t)` spelled out, so a
    // typo in one constant cannot hide behind another.
    let io = |n: u64| (0x12 << 8) | n;
    let ior8 = |n: u64| (2 << 30) | (8 << 16) | (0x12 << 8) | n;
    let iow8 = |n: u64| (1 << 30) | (8 << 16) | (0x12 << 8) | n;
    assert_eq!(BLKROSET, io(93));
    assert_eq!(BLKROGET, io(94));
    assert_eq!(BLKRRPART, io(95));
    assert_eq!(BLKGETSIZE, io(96));
    assert_eq!(BLKFLSBUF, io(97));
    assert_eq!(BLKRASET, io(98));
    assert_eq!(BLKRAGET, io(99));
    assert_eq!(BLKFRASET, io(100));
    assert_eq!(BLKFRAGET, io(101));
    assert_eq!(BLKSECTGET, io(103));
    assert_eq!(BLKSSZGET, io(104));
    assert_eq!(BLKBSZGET, ior8(112));
    assert_eq!(BLKBSZSET, iow8(113));
    assert_eq!(BLKGETSIZE64, ior8(114));
    assert_eq!(BLKIOMIN, io(120));
    assert_eq!(BLKIOOPT, io(121));
    assert_eq!(BLKALIGNOFF, io(122));
    assert_eq!(BLKPBSZGET, io(123));
    assert_eq!(BLKDISCARDZEROES, io(124));
    assert_eq!(BLKGETDISKSEQ, ior8(128));
}

#[test]
fn find_cmd_matches_whole_names_only() {
    assert_eq!(find_cmd(b"--setro"), Some(0));
    assert_eq!(find_cmd(b"--rereadpt"), Some(20));
    assert_eq!(find_cmd(b"--getsz"), None, "--getsz is handled apart");
    assert_eq!(find_cmd(b"--setr"), None);
    assert_eq!(find_cmd(b"--setro="), None);
    assert_eq!(find_cmd(b""), None);
}

#[test]
fn c_conversions_wrap() {
    assert_eq!(to_ushort(-1), 65535);
    assert_eq!(to_int(-1), -1);
    assert_eq!(to_uint(-1), u32::MAX);
    assert_eq!(to_ulong(-1), u64::MAX);
    assert_eq!(to_int(1), 1);
    assert_eq!(int_by_value(-1), -1);
    assert_eq!(int_by_value(256), 256);
    assert_eq!(int_by_value(i32::MIN), -2_147_483_648);
    assert_eq!(u64::from_ne_bytes(int_by_value(-1).to_ne_bytes()), u64::MAX);
}

/// `n` spaces.
fn sp(n: usize) -> String {
    " ".repeat(n)
}

#[test]
fn usage_lines_up_as_upstreams() {
    let text = String::from_utf8(usage(b"blockdev")).unwrap();
    assert!(text.starts_with(
        "\nUsage:\n blockdev [-v|-q] commands devices\n blockdev --report [devices]\n blockdev -h|-V\n\nCall block device ioctls from the command line.\n"
    ));
    // `%-16s`: " -h, --help" is 11 bytes, " -V, --version" 14.
    assert!(text.contains(&format!(
        "\n -h, --help{}display this help\n -V, --version{}display version\n",
        sp(5),
        sp(2)
    )));
    // `" %-25s %s"`: a seven-byte command is followed by 18 + 1 spaces.
    assert!(text.contains(&format!(
        "\nAvailable commands:\n --getsz{}get size in 512-byte sectors\n --setro{}set read-only\n",
        sp(19),
        sp(19)
    )));
    // `" %s %-*s %s"` with the width 24 - strlen(name): the command and its
    // argument share the column, so every description starts at byte 27.
    assert!(text.contains(&format!(
        "\n --setbsz <bytes>{}set blocksize on file descriptor opening the block device\n",
        sp(10)
    )));
    assert!(text.contains(&format!("\n --setra <sectors>{}set readahead\n", sp(9))));
    assert!(text.contains(&format!(
        "\n --setfra <sectors>{}set filesystem readahead\n",
        sp(8)
    )));
    for line in text.lines().filter(|l| l.starts_with(" --")) {
        assert_eq!(line.as_bytes()[26], b' ', "{line}");
        assert_ne!(line.as_bytes()[27], b' ', "{line}");
    }
    assert!(text.ends_with(&format!(
        " --rereadpt{}reread partition table\n\nFor more details see blockdev(8).\n",
        sp(16)
    )));
}

#[test]
fn fgets_pieces_split_at_newlines_and_at_the_buffer() {
    let pieces: Vec<&[u8]> = fgets_pieces(b"ab\ncd\n\nef", 200).collect();
    assert_eq!(pieces, [&b"ab\n"[..], b"cd\n", b"\n", b"ef"]);
    let pieces: Vec<&[u8]> = fgets_pieces(b"abcdefg\nh", 4).collect();
    assert_eq!(pieces, [&b"abc"[..], b"def", b"g\n", b"h"]);
    assert_eq!(fgets_pieces(b"", 200).count(), 0);
}

#[test]
fn partition_lines_are_scanned_as_sscanf_would() {
    assert_eq!(
        scan_partition_line(b"   8        0     397940 sda\n"),
        Some(&b"sda"[..])
    );
    assert_eq!(scan_partition_line(b"8 1 2 sda1"), Some(&b"sda1"[..]));
    assert_eq!(
        scan_partition_line(b" +8 -1 +1 loop7\n"),
        Some(&b"loop7"[..])
    );
    // A tab is part of the name; a space ends it.
    assert_eq!(
        scan_partition_line(b"1 1 1 ram1\tx\n"),
        Some(&b"ram1\tx"[..])
    );
    assert_eq!(
        scan_partition_line(b"1 2 1 ram2 trailing\n"),
        Some(&b"ram2"[..])
    );
    // Not four conversions.
    assert_eq!(scan_partition_line(b"major minor  #blocks  name\n"), None);
    assert_eq!(scan_partition_line(b"\n"), None);
    assert_eq!(scan_partition_line(b""), None);
    assert_eq!(scan_partition_line(b"8 x 1 sda\n"), None);
    assert_eq!(scan_partition_line(b"8 - 1 sda\n"), None);
    assert_eq!(scan_partition_line(b"8 0 1\n"), None);
    assert_eq!(scan_partition_line(b"8 0 1   \n"), None);
    // sscanf reads a C string.
    assert_eq!(scan_partition_line(b"8 0 1 s\0da\n"), Some(&b"s"[..]));
    assert_eq!(scan_partition_line(b"8 0\0 1 sda\n"), None);
    // At most 200 bytes of name.
    let mut long = b"1 2 3 ".to_vec();
    long.extend(std::iter::repeat_n(b'n', 250));
    assert_eq!(scan_partition_line(&long).map(<[u8]>::len), Some(200));
}

#[test]
fn start_is_cut_to_fifteen_bytes() {
    assert_eq!(cut15(&format!("{:>15}", 2048u64)), "           2048");
    assert_eq!(cut15(&format!("{:>15}", u64::MAX)), "184467440737095");
    assert_eq!(cut15(&format!("{:>15}", "N/A")), "            N/A");
}

#[test]
fn short_name_is_past_the_last_slash() {
    assert_eq!(short_name(OsStr::new("/sbin/blockdev")), b"blockdev");
    assert_eq!(short_name(OsStr::new("blockdev")), b"blockdev");
    assert_eq!(short_name(OsStr::new("dir/")), b"");
}
