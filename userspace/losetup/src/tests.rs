#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

fn losetup() -> Losetup {
    Losetup {
        short: b"losetup".to_vec(),
        out: Stdout::new(1),
        no_headings: false,
        raw: false,
        json: false,
        columns: Vec::new(),
    }
}

fn run(args: &[&str]) -> Result<u8, Exit> {
    let argv: Vec<OsString> = std::iter::once("losetup")
        .chain(args.iter().copied())
        .map(OsString::from)
        .collect();
    losetup().run(&argv)
}

#[test]
fn the_column_table_is_in_enum_order() {
    let all = [
        Col::Name,
        Col::Autoclr,
        Col::BackFile,
        Col::BackIno,
        Col::BackMajmin,
        Col::Majmin,
        Col::Offset,
        Col::Partscan,
        Col::Ro,
        Col::Sizelimit,
        Col::Dio,
        Col::Logsec,
    ];
    for (i, col) in all.into_iter().enumerate() {
        assert_eq!(INFOS[i].id, col);
        assert_eq!(info(col).name, INFOS[i].name);
    }
}

#[test]
fn a_column_is_named_whole_in_any_case() {
    assert_eq!(
        column_name_to_id(b"back-file", b"back-file", b"losetup"),
        Some(Col::BackFile)
    );
    assert_eq!(
        column_name_to_id(b"LOG-SEC", b"LOG-SEC,x", b"losetup"),
        Some(Col::Logsec)
    );
    assert_eq!(column_name_to_id(b"NAM", b"NAM", b"losetup"), None);
    assert_eq!(column_name_to_id(b"NAMES", b"NAMES", b"losetup"), None);
}

#[test]
fn direct_io_takes_on_or_off_and_nothing_else() {
    assert_eq!(parse_switch(b"on", "argument error"), Ok(1));
    assert_eq!(parse_switch(b"off", "argument error"), Ok(0));
    assert_eq!(
        parse_switch(b"maybe", "argument error"),
        Err("argument error: 'maybe'".to_string())
    );
    assert!(
        parse_switch(b"ON", "argument error").is_err(),
        "case matters"
    );
}

#[test]
fn a_backing_file_that_is_not_whole_sectors_is_warned_about() {
    let none = warn_size_message(b"f", 0, 0, 0, None);
    assert_eq!(none, None, "no size to judge: a block device, or no file");
    let small = warn_size_message(b"f", 0, 0, 0, Some(100)).unwrap();
    assert!(
        small.starts_with("f: Warning: file is smaller than 512 bytes"),
        "{small}"
    );
    let odd = warn_size_message(b"f", 0, 0, 0, Some(1000)).unwrap();
    assert!(
        odd.starts_with("f: Warning: file does not fit into a 512-byte sector"),
        "{odd}"
    );
    assert_eq!(warn_size_message(b"f", 0, 0, 0, Some(1024)), None);
    // The part from the offset is what is judged; a size limit replaces it.
    assert!(warn_size_message(b"f", 0, 1000, LOOPDEV_FL_OFFSET, Some(1024)).is_some());
    assert_eq!(
        warn_size_message(b"f", 0, 512, LOOPDEV_FL_OFFSET, Some(1024)),
        None
    );
    assert_eq!(warn_size_message(b"f", 2048, 0, 0, Some(1)), None);
    // Past the end, the subtraction wraps as upstream's `uint64_t` does:
    // 2^64 - 1024 is a whole number of sectors, so no warning at all.
    assert_eq!(
        warn_size_message(b"f", 0, 2048, LOOPDEV_FL_OFFSET, Some(1024)),
        None
    );
    assert!(warn_size_message(b"f", 0, 2000, LOOPDEV_FL_OFFSET, Some(1024)).is_some());
}

#[test]
fn the_usage_lists_the_columns_and_ends_with_the_manual() {
    let text = String::from_utf8(usage(b"losetup")).unwrap();
    assert!(text.starts_with(
        "\nUsage:\n losetup [options] [<loopdev>]\n losetup [options] -f | <loopdev> <file>\n"
    ));
    assert!(text.contains(&format!(" -h, --help{}display this help\n", " ".repeat(20))));
    assert!(text.contains("\n BACK-MAJ:MIN  backing file major:minor device number\n"));
    assert!(text.contains("\n         NAME  loop device name\n"));
    assert!(text.ends_with("\nFor more details see losetup(8).\n"));
}

#[test]
fn options_decode_to_upstream_s_values() {
    assert_eq!(option_to_longopt(OPT_RAW), Some("raw"));
    assert_eq!(option_to_longopt(i32::from(b'D')), Some("detach-all"));
    assert_eq!(option_to_longopt(i32::from(b'z')), None);
    assert_eq!(
        option_code(&Opt::Long("direct-io", None)),
        Some((OPT_DIO, None))
    );
    assert_eq!(
        option_code(&Opt::Short(b'o', Some(OsString::from("10")))),
        Some((i32::from(b'o'), Some(OsString::from("10"))))
    );
}

#[test]
fn the_refusals_are_upstream_s() {
    // Each refused before any device is touched.
    for (args, want) in [
        (&["-o", "10", "-a"][..], Err(Exit(1))),
        (&["--sizelimit", "1", "-a"], Err(Exit(1))),
        (&["-J", "--raw"], Err(Exit(1))),
        (&["--bogus"], Err(Exit(1))),
        (&["-o", "1x"], Err(Exit(1))),
        (&["--direct-io=maybe", "x"], Err(Exit(1))),
        (&["-f", "a", "b"], Err(Exit(1))),
        (&["--output-all", "-J"], Err(Exit(1))),
        (&["-V"], Ok(0)),
        (&["--help"], Ok(0)),
        (&["-O", "NAME,BOGUS"], Ok(1)),
    ] {
        assert_eq!(run(args), want, "{args:?}");
    }
}

#[test]
fn a_name_that_is_no_device_is_refused_with_enodev() {
    assert_eq!(
        Losetup::use_device(&mut LoopCxt::new(0), b"/nonexistent/loop9"),
        Err(19)
    );
}
