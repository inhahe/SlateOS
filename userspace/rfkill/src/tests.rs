#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;

/// A program whose `/sys/class/rfkill` is `sys`, and whose `/dev/rfkill`
/// is `dev`.
fn rfkill_at(dev: &str, sys: &std::path::Path) -> Rfkill {
    let mut rf = Rfkill::new(b"rfkill".to_vec(), 0, dev, sys);
    rf.log = keep_log;
    rf
}

thread_local! {
    /// What this test thread's program logged, a line each.
    static LOGGED: std::cell::RefCell<Vec<Vec<u8>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The tests' log: kept for them to read, never the machine's syslog.
fn keep_log(line: &[u8]) {
    LOGGED.with(|l| l.borrow_mut().push(line.to_vec()));
}

/// What was logged since the last call.
#[cfg(unix)]
fn logged() -> Vec<Vec<u8>> {
    LOGGED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// A scratch sysfs: `rfkillN/name` and `rfkillN/type` for each device.
fn sysfs(tag: &str, devices: &[(u32, &[u8], &[u8])]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rfkill-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for &(idx, name, ty) in devices {
        let d = dir.join(format!("rfkill{idx}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("name"), name).unwrap();
        std::fs::write(d.join("type"), ty).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ev(idx: u32, ty: u8, op: u8, soft: u8, hard: u8) -> Event {
    Event {
        idx,
        ty,
        op,
        soft,
        hard,
    }
}

#[test]
fn an_event_is_the_kernel_s_eight_bytes() {
    let e = ev(0x0102_0304, 2, 3, 1, 0);
    let bytes = e.to_bytes();
    assert_eq!(bytes.len(), RFKILL_EVENT_SIZE_V1);
    assert_eq!(&bytes[4..], &[2, 3, 1, 0]);
    assert_eq!(Event::from_bytes(bytes), e);
}

#[test]
fn actions_are_matched_whole() {
    assert_eq!(string_to_action(b"list"), Some(Action::List));
    assert_eq!(string_to_action(b"toggle"), Some(Action::Toggle));
    assert_eq!(string_to_action(b"lis"), None);
    assert_eq!(string_to_action(b"LIST"), None);
}

#[test]
fn a_description_is_the_first_row_s_and_none_for_all_and_unknown_types() {
    assert_eq!(rfkill_type_to_desc(RFKILL_TYPE_WLAN), Some("Wireless LAN"));
    assert_eq!(rfkill_type_to_desc(RFKILL_TYPE_NFC), Some("NFC"));
    assert_eq!(rfkill_type_to_desc(RFKILL_TYPE_ALL), None);
    assert_eq!(rfkill_type_to_desc(NUM_RFKILL_TYPES), None);
    assert_eq!(rfkill_type_to_desc(200), None);
}

#[test]
fn only_add_events_of_what_was_asked_for_match() {
    let wlan0 = ev(0, 1, RFKILL_OP_ADD, 0, 0);
    assert!(event_match(&wlan0, RfkillId::All));
    assert!(event_match(&wlan0, RfkillId::Type(RFKILL_TYPE_WLAN)));
    assert!(!event_match(&wlan0, RfkillId::Type(RFKILL_TYPE_BLUETOOTH)));
    assert!(event_match(&wlan0, RfkillId::Index(0)));
    assert!(!event_match(&wlan0, RfkillId::Index(1)));
    assert!(!event_match(
        &ev(0, 1, RFKILL_OP_CHANGE, 0, 0),
        RfkillId::All
    ));
}

#[test]
fn identifiers_are_types_all_or_indices_that_exist() {
    let sys = sysfs("ids", &[(0, b"phy0\n", b"wlan\n")]);
    let mut rf = rfkill_at("/nonexistent", &sys);
    assert_eq!(
        rf.rfkill_id_to_type(b"wlan"),
        Ok(RfkillId::Type(RFKILL_TYPE_WLAN))
    );
    assert_eq!(
        rf.rfkill_id_to_type(b"wifi"),
        Ok(RfkillId::Type(RFKILL_TYPE_WLAN))
    );
    assert_eq!(
        rf.rfkill_id_to_type(b"ultrawideband"),
        Ok(RfkillId::Type(RFKILL_TYPE_UWB))
    );
    assert_eq!(rf.rfkill_id_to_type(b"all"), Ok(RfkillId::All));
    assert_eq!(
        rf.rfkill_id_to_type(b"WLAN"),
        Ok(RfkillId::Invalid),
        "upper case is neither"
    );
    assert_eq!(rf.rfkill_id_to_type(b"nosuch"), Ok(RfkillId::Invalid));
    assert_eq!(rf.rfkill_id_to_type(b""), Ok(RfkillId::Invalid));
    assert_eq!(rf.rfkill_id_to_type(b"0"), Ok(RfkillId::Index(0)));
    assert_eq!(
        rf.rfkill_id_to_type(b"1"),
        Ok(RfkillId::Invalid),
        "no rfkill1 in sysfs"
    );
    assert_eq!(
        rf.rfkill_id_to_type(b"1x"),
        Err(Stop::Exit(1)),
        "strtou32_or_err exits"
    );
    assert_eq!(rf.rfkill_id_to_type(b"4294967296"), Err(Stop::Exit(1)));
    std::fs::remove_dir_all(&sys).unwrap();
}

#[test]
fn the_sysfs_reader_keeps_its_last_answer() {
    let sys = sysfs(
        "attr",
        &[(0, b"phy0\nsecond line", b"wlan\n"), (1, b"", b"x\0y\n")],
    );
    let mut rf = rfkill_at("/nonexistent", &sys);
    assert_eq!(rf.get_sys_attr(0, "name"), b"phy0");
    assert_eq!(rf.get_sys_attr(0, "type"), b"wlan");
    assert_eq!(
        rf.get_sys_attr(1, "name"),
        b"wlan",
        "empty: fgets leaves the buffer"
    );
    assert_eq!(rf.get_sys_attr(7, "name"), b"wlan", "missing: the same");
    assert_eq!(rf.get_sys_attr(1, "type"), b"x", "a NUL ends the C string");
    std::fs::write(sys.join("rfkill0/name"), [b'n'; 200]).unwrap();
    assert_eq!(
        rf.get_sys_attr(0, "name"),
        vec![b'n'; 127],
        "at most 127 bytes"
    );
    std::fs::remove_dir_all(&sys).unwrap();
}

#[test]
fn the_timestamp_is_iso_with_a_comma_and_the_zone() {
    let utc = localtime::Zone::utc();
    assert_eq!(timestamp(&utc, 0, 5), "1970-01-01 00:00:00,000005+00:00");
    assert_eq!(
        timestamp(&utc, 1_700_000_000, 999_999),
        "2023-11-14 22:13:20,999999+00:00"
    );
}

#[test]
fn the_zone_is_hours_and_minutes_and_half_an_hour_west_is_plus() {
    let east = localtime::Zone::from_tz(Some(b"<+0530>-5:30"));
    assert_eq!(timestamp(&east, 0, 0), "1970-01-01 05:30:00,000000+05:30");
    let west = localtime::Zone::from_tz(Some(b"<-0330>3:30"));
    assert_eq!(timestamp(&west, 0, 0), "1969-12-31 20:30:00,000000-03:30");
    // The minutes are always positive, and the hours of half an hour west
    // are 0, which `%+03d` prints with a plus: util-linux's own quirk.
    let half = localtime::Zone::from_tz(Some(b"<-0030>0:30"));
    assert_eq!(timestamp(&half, 0, 0), "1969-12-31 23:30:00,000000+00:30");
}

#[test]
fn states_are_worded_as_upstream_words_them() {
    assert_eq!(blocked(0), b"unblocked");
    assert_eq!(blocked(1), b"blocked");
    assert_eq!(blocked(2), b"blocked");
    assert_eq!(yes_no(0), "no");
    assert_eq!(yes_no(1), "yes");
}

#[test]
fn options_decode_to_upstream_s_values() {
    assert_eq!(option_to_longopt(i32::from(b'J')), Some("json"));
    assert_eq!(option_to_longopt(OPT_LIST_TYPES), Some("output-all"));
    assert_eq!(option_to_longopt(i32::from(b'x')), None);
    let id = Some(OsString::from("ID"));
    assert_eq!(
        option_code(&Opt::Short(b'o', id.clone())),
        Some((i32::from(b'o'), id.clone()))
    );
    assert_eq!(
        option_code(&Opt::Long("output", id.clone())),
        Some((i32::from(b'o'), id))
    );
    assert_eq!(
        option_code(&Opt::Long("output-all", None)),
        Some((OPT_LIST_TYPES, None))
    );
    assert_eq!(option_code(&Opt::Long("nosuch", None)), None);
}

#[test]
fn the_program_is_named_by_argv0_s_last_component_escaped() {
    assert_eq!(short_name(OsStr::new("/usr/sbin/rfkill")), b"rfkill");
    assert_eq!(short_name(OsStr::new("rfkill")), b"rfkill");
    assert_eq!(short_name(OsStr::new("dir/")), b"");
    assert_eq!(shown(b"a\nb"), "a\\012b");
}

#[test]
fn xstrdup_copies_a_string_and_aborts_on_none() {
    let rf = rfkill_at("/nonexistent", std::path::Path::new("/nonexistent"));
    assert_eq!(rf.xstrdup(Some("WiMAX")), Ok(b"WiMAX".to_vec()));
    assert_eq!(rf.xstrdup(None), Err(Stop::Abort));
}

#[test]
fn the_switch_sets_what_upstream_s_sets() {
    let sys = sysfs("switch", &[]);
    let argv = |a: &[&str]| -> Vec<OsString> { a.iter().map(OsString::from).collect() };
    let mut rf = rfkill_at("/nonexistent", &sys);
    // `help` prints the usage into a buffer the test never flushes.
    assert_eq!(
        rf.run(&argv(&["rfkill", "-J", "-n", "help"])),
        Stop::Exit(0)
    );
    assert!(rf.json && rf.no_headings && !rf.raw);
    let mut rf = rfkill_at("/nonexistent", &sys);
    assert_eq!(rf.run(&argv(&["rfkill", "-r", "help"])), Stop::Exit(0));
    assert!(rf.raw && !rf.json && !rf.no_headings);
    for refused in [
        &["rfkill", "-J", "-r"][..],
        &["rfkill", "--bogus"],
        &["rfkill", "-x"],
        &["rfkill", "bogus"],
        &["rfkill", "LIST"],
    ] {
        let mut rf = rfkill_at("/nonexistent", &sys);
        assert_eq!(rf.run(&argv(refused)), Stop::Exit(1), "{refused:?}");
    }
    for fine in [
        &["rfkill", "-V"][..],
        &["rfkill", "--help"],
        &["rfkill", "-h", "bogus"],
    ] {
        let mut rf = rfkill_at("/nonexistent", &sys);
        assert_eq!(rf.run(&argv(fine)), Stop::Exit(0), "{fine:?}");
    }
    // No device: listing and blocking fail, as `-errno` ORed into 1.
    for failing in [
        &["rfkill"][..],
        &["rfkill", "list"],
        &["rfkill", "--output-all"],
        &["rfkill", "block", "all"],
    ] {
        let mut rf = rfkill_at("/nonexistent", &sys);
        assert_eq!(rf.run(&argv(failing)), Stop::Exit(1), "{failing:?}");
    }
    std::fs::remove_dir_all(&sys).unwrap();
}

#[test]
fn usage_lists_every_column_and_command() {
    let text = String::from_utf8(usage(b"rfkill")).unwrap();
    assert!(text.starts_with("\nUsage:\n rfkill [options] command [identifier ...]\n"));
    assert!(text.contains(&format!(" -h, --help{}display this help\n", " ".repeat(13))));
    assert!(text.contains(" TYPE-DESC   device type description\n"));
    assert!(text.ends_with(" toggle  identifier\n\nFor more details see rfkill(8).\n"));
}

/// A pipe whose read end stands for `/dev/rfkill`, reopened by name as a
/// FIFO is: the events written first, the write end kept open so a read of
/// what is left says `EAGAIN`.
#[cfg(unix)]
struct FakeDevice {
    r: i32,
    w: i32,
}

#[cfg(unix)]
impl FakeDevice {
    fn with(events: &[Event]) -> Self {
        let (r, w) = libcall::fd::pipe().unwrap();
        for e in events {
            libcall::fd::write(w, &e.to_bytes()).unwrap();
        }
        FakeDevice { r, w }
    }

    fn path(&self) -> String {
        format!("/proc/self/fd/{}", self.r)
    }

    /// What is left in the pipe.
    fn drain(&self) -> Vec<u8> {
        libcall::file::set_status_flags(self.r, libcall::file::O_NONBLOCK).unwrap();
        let mut all = Vec::new();
        let mut buf = [0u8; 64];
        while let Ok(n) = libcall::fd::read(self.r, &mut buf) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        all
    }
}

#[cfg(unix)]
impl Drop for FakeDevice {
    fn drop(&mut self) {
        let _ = libcall::fd::close(self.r);
        let _ = libcall::fd::close(self.w);
    }
}

#[cfg(unix)]
#[test]
fn the_table_has_a_row_for_each_device_asked_for() {
    let sys = sysfs(
        "table",
        &[(0, b"phy0\n", b"wlan\n"), (1, b"hci0\n", b"bluetooth\n")],
    );
    let dev = FakeDevice::with(&[ev(0, 1, RFKILL_OP_ADD, 0, 0), ev(1, 2, RFKILL_OP_ADD, 1, 0)]);
    let mut rf = rfkill_at(&dev.path(), &sys);
    rf.columns = vec![Col::Id, Col::Type, Col::Device, Col::Soft, Col::Hard];
    let (mut tb, ids) = rf.rfkill_list_init();
    assert_eq!(rf.rfkill_list_fill(&mut tb, &ids, None), Ok(0));
    let mut text = Vec::new();
    tb.print_into(&mut text).unwrap();
    assert_eq!(
        String::from_utf8(text).unwrap(),
        "ID TYPE      DEVICE      SOFT      HARD\n \
         0 wlan      phy0   unblocked unblocked\n \
         1 bluetooth hci0     blocked unblocked\n"
    );
    std::fs::remove_dir_all(&sys).unwrap();
}

#[cfg(unix)]
#[test]
fn a_type_with_no_description_fails_its_assertion() {
    let sys = sysfs("abort", &[(0, b"phy0\n", b"wlan\n")]);
    let dev = FakeDevice::with(&[ev(0, 0, RFKILL_OP_ADD, 0, 0)]);
    let mut rf = rfkill_at(&dev.path(), &sys);
    rf.columns = vec![Col::Id, Col::Desc];
    let (mut tb, ids) = rf.rfkill_list_init();
    assert_eq!(rf.rfkill_list_fill(&mut tb, &ids, None), Err(Stop::Abort));
    std::fs::remove_dir_all(&sys).unwrap();
}

#[cfg(unix)]
#[test]
fn a_short_event_is_skipped_and_the_listing_goes_on() {
    let sys = sysfs("short", &[(0, b"phy0\n", b"wlan\n")]);
    let dev = FakeDevice::with(&[]);
    libcall::fd::write(dev.w, &[1, 2, 3]).unwrap();
    let mut rf = rfkill_at(&dev.path(), &sys);
    rf.columns = vec![Col::Id];
    let (mut tb, ids) = rf.rfkill_list_init();
    // Three bytes, warned about; then `EAGAIN`, which ends it.
    assert_eq!(rf.rfkill_list_fill(&mut tb, &ids, None), Ok(0));
    assert_eq!(rf.errno, EAGAIN);
    std::fs::remove_dir_all(&sys).unwrap();
}

#[cfg(unix)]
#[test]
fn block_writes_one_request_of_the_identifier_s_kind() {
    let sys = sysfs("block", &[(3, b"phy3\n", b"wlan\n")]);
    let dev = FakeDevice::with(&[]);
    let mut rf = rfkill_at(&dev.path(), &sys);
    assert_eq!(rf.rfkill_block(1, b"3"), Ok(0));
    assert_eq!(rf.rfkill_block(0, b"bluetooth"), Ok(0));
    assert_eq!(rf.rfkill_block(1, b"all"), Ok(0));
    assert_eq!(rf.rfkill_block(1, b"nosuch"), Ok(-EINVAL));
    let mut want = ev(3, 0, RFKILL_OP_CHANGE, 1, 0).to_bytes().to_vec();
    want.extend_from_slice(&ev(0, 2, RFKILL_OP_CHANGE_ALL, 0, 0).to_bytes());
    want.extend_from_slice(&ev(0, 0, RFKILL_OP_CHANGE_ALL, 1, 0).to_bytes());
    assert_eq!(dev.drain(), want);
    // Each request logged as upstream words it: the index as a signed `%d`.
    assert_eq!(
        logged(),
        [
            &b"block set for id 3"[..],
            b"unblock set for type bluetooth",
            b"block set for all"
        ]
    );
    std::fs::remove_dir_all(&sys).unwrap();
}

#[cfg(unix)]
#[test]
fn toggle_turns_each_matching_device_s_block_over() {
    let sys = sysfs(
        "toggle",
        &[(0, b"phy0\n", b"wlan\n"), (1, b"hci0\n", b"bluetooth\n")],
    );
    let dev = FakeDevice::with(&[ev(0, 1, RFKILL_OP_ADD, 1, 0), ev(1, 2, RFKILL_OP_ADD, 0, 0)]);
    let mut rf = rfkill_at(&dev.path(), &sys);
    assert_eq!(rf.rfkill_toggle(b"wlan"), Ok(0));
    // The request went into the same pipe, and was read back as an event
    // that matches nothing -- as on a FIFO, not on the kernel's device; the
    // log says which way it went: the blocked LAN unblocked, as a type.
    assert_eq!(dev.drain(), Vec::<u8>::new());
    assert_eq!(logged(), [b"unblock set for type wlan"]);
    let dev = FakeDevice::with(&[ev(1, 2, RFKILL_OP_ADD, 0, 0)]);
    let mut rf = rfkill_at(&dev.path(), &sys);
    assert_eq!(rf.rfkill_toggle(b"1"), Ok(0));
    assert_eq!(logged(), [b"block set for id 1"]);
    std::fs::remove_dir_all(&sys).unwrap();
}

#[test]
fn the_column_table_is_in_enum_order() {
    for (i, col) in [
        Col::Device,
        Col::Id,
        Col::Type,
        Col::Desc,
        Col::Soft,
        Col::Hard,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(INFOS[i].id, col);
        assert_eq!(info(col).id, col);
        assert_eq!(info(col).name, INFOS[i].name);
    }
}

#[test]
fn a_clock_before_1970_counts_back_whole_seconds() {
    use std::time::{Duration, UNIX_EPOCH};
    assert_eq!(
        timeval(UNIX_EPOCH + Duration::from_millis(2_500)),
        (2, 500_000)
    );
    assert_eq!(
        timeval(UNIX_EPOCH - Duration::from_millis(1_500)),
        (-2, 500_000)
    );
    assert_eq!(timeval(UNIX_EPOCH - Duration::from_secs(3)), (-3, 0));
}

#[test]
fn no_device_is_said_and_fails() {
    let sys = sysfs("nodev", &[]);
    let mut rf = rfkill_at("/nonexistent/rfkill", &sys);
    rf.columns = vec![Col::Id];
    let (mut tb, ids) = rf.rfkill_list_init();
    let rc = rf.rfkill_list_fill(&mut tb, &ids, None).unwrap();
    assert!(rc < 0);
    std::fs::remove_dir_all(&sys).unwrap();
}
