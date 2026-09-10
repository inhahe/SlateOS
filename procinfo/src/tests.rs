//! Tests for the `/proc` readers and parsers.
//!
//! Two kinds. The parser tests are pure functions over captured content and
//! say what the format means. The reader tests build a fixture directory and
//! point [`ProcFs::at`] at it, which is the only way to exercise the
//! difference between "absent" and "unreadable" on a host that has no
//! `/proc` — and that difference is the thing this crate was written to get
//! right, so it is the thing that most needs a test.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use super::*;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// A throwaway directory laid out like `/proc`, removed when dropped.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        // `std::env::temp_dir` plus pid plus a serial number plus a per-test
        // tag: `cargo test` runs tests concurrently on separate threads, so two
        // fixtures that shared a path would race, and the failure would be a
        // flake rather than a finding (see `scripts/flake-hunt.sh` on why that
        // matters here). The pid separates concurrent `cargo test` runs; the
        // counter separates fixtures within one run even if two tests pass the
        // same tag.
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "procinfo-test-{}-{}-{tag}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(unique);
        // A leftover from a previous run with the same pid is not an error, and
        // `create_dir_all` below is what actually has to succeed.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn write(&self, relative: &str, content: &[u8]) {
        let path = self.dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(content).unwrap();
    }

    fn procfs(&self) -> ProcFs {
        ProcFs::at(&self.dir)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Best effort: a leaked temp dir is untidy, a panic in `drop` while a
        // test is already failing hides the real failure.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// unescape_octal
// ---------------------------------------------------------------------------

#[test]
fn unescape_leaves_ordinary_text_alone() {
    assert_eq!(unescape_octal(b"/dev/sda1"), b"/dev/sda1".to_vec());
    assert_eq!(unescape_octal(b""), Vec::<u8>::new());
}

#[test]
fn unescape_decodes_the_four_the_kernel_emits() {
    // Linux `fs/proc_namespace.c` escapes exactly these, and nothing else.
    assert_eq!(unescape_octal(br"a\040b"), b"a b".to_vec());
    assert_eq!(unescape_octal(br"a\011b"), b"a\tb".to_vec());
    assert_eq!(unescape_octal(br"a\012b"), b"a\nb".to_vec());
    assert_eq!(unescape_octal(br"a\134b"), br"a\b".to_vec());
}

#[test]
fn unescape_decodes_several_in_one_field() {
    assert_eq!(
        unescape_octal(br"/mnt/my\040very\040own\040disk"),
        b"/mnt/my very own disk".to_vec()
    );
}

#[test]
fn unescape_handles_an_escape_at_each_end() {
    assert_eq!(unescape_octal(br"\040lead"), b" lead".to_vec());
    assert_eq!(unescape_octal(br"trail\040"), b"trail ".to_vec());
    assert_eq!(unescape_octal(br"\040"), b" ".to_vec());
}

#[test]
fn unescape_leaves_a_non_escape_backslash_as_itself() {
    // Three cases the kernel never produces, so the conservative answer is to
    // change nothing: a decoder that invented a byte here would corrupt a path
    // a caller may be about to open.
    assert_eq!(unescape_octal(br"\09"), br"\09".to_vec()); // too short
    assert_eq!(unescape_octal(br"\098"), br"\098".to_vec()); // 9 is not octal
    assert_eq!(unescape_octal(br"\"), br"\".to_vec()); // nothing follows
    assert_eq!(unescape_octal(br"a\zb"), br"a\zb".to_vec()); // not digits
}

#[test]
fn unescape_does_not_rescan_the_bytes_it_produced() {
    // `\134` decodes to a backslash. If the decoder looped over its own output
    // it would then read that backslash plus the following `040` as a second
    // escape and produce a space -- turning `\134040` (a path containing
    // `\040` literally) into a path containing a space. Distinct paths must
    // stay distinct.
    assert_eq!(unescape_octal(br"\134040"), br"\040".to_vec());
    assert_ne!(unescape_octal(br"\134040"), b" ".to_vec());
}

#[test]
fn unescape_accepts_high_octal_values() {
    // `\377` is 255, the largest a byte can hold. A path byte can be any of
    // them: SlateOS paths allow every byte except `/` and NUL.
    assert_eq!(unescape_octal(br"\377"), vec![0xFFu8]);
    assert_eq!(unescape_octal(br"\000"), vec![0x00u8]);
    // 0o400 is 256, which does not fit in a byte. Three octal digits can spell
    // a value a byte cannot hold, and the answer is to leave the text alone
    // rather than to truncate it to 0 -- a truncating decoder would turn a
    // path containing the literal text `\400` into one containing a NUL, which
    // is the one byte a SlateOS path may not contain.
    assert_eq!(unescape_octal(br"\400"), br"\400".to_vec());
}

// ---------------------------------------------------------------------------
// Key-value parsing
// ---------------------------------------------------------------------------

const CPUINFO: &[u8] = b"\
processor\t: 0
vendor_id\t: GenuineIntel
model name\t: Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz
cpu MHz\t\t: 2592.000
cache size\t: 12288 KB
flags\t\t: fpu vme de pse tsc msr

processor\t: 1
vendor_id\t: GenuineIntel
model name\t: Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz
cpu MHz\t\t: 2592.000
cache size\t: 12288 KB
";

#[test]
fn key_values_skip_blank_and_separatorless_lines() {
    let pairs = parse_key_values(b"a: 1\n\n\nnoseparator\nb: 2\n");
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].key_str(), Some("a"));
    assert_eq!(pairs[1].value_str(), Some("2"));
}

#[test]
fn key_values_keep_duplicates_in_order() {
    // Collapsing duplicates into a map is how a reader reports one core on a
    // multi-core machine: `/proc/cpuinfo` repeats every key per logical CPU.
    let pairs = parse_key_values(CPUINFO);
    let processors: Vec<_> = pairs.iter().filter(|kv| kv.key == b"processor").collect();
    assert_eq!(processors.len(), 2);
    assert_eq!(processors[0].value_str(), Some("0"));
    assert_eq!(processors[1].value_str(), Some("1"));
}

#[test]
fn key_values_prefer_a_colon_over_a_tab() {
    // `cpuinfo` writes `key<TAB><TAB>: value`. Splitting on the first tab
    // would make the key `cpu MHz` become `cpu MHz` with an empty value and
    // the real value would be lost.
    let pairs = parse_key_values(b"cpu MHz\t\t: 2592.000\n");
    assert_eq!(pairs[0].key_str(), Some("cpu MHz"));
    assert_eq!(pairs[0].value_str(), Some("2592.000"));
}

#[test]
fn key_values_split_on_a_tab_when_there_is_no_colon() {
    let pairs = parse_key_values(b"Name\tinit\n");
    assert_eq!(pairs[0].key_str(), Some("Name"));
    assert_eq!(pairs[0].value_str(), Some("init"));
}

#[test]
fn key_values_keep_a_colon_inside_a_value() {
    let pairs = parse_key_values(b"time: 12:34:56\n");
    assert_eq!(pairs[0].value_str(), Some("12:34:56"));
}

#[test]
fn key_values_tolerate_crlf() {
    // Not a thing the kernel writes, but this parser is also pointed at
    // captured files, and a capture can acquire a carriage return.
    let pairs = parse_key_values(b"a: 1\r\nb: 2\r\n");
    assert_eq!(pairs[0].value_str(), Some("1"));
    assert_eq!(pairs[1].value_str(), Some("2"));
}

#[test]
fn key_value_finds_the_first_match_only() {
    assert_eq!(key_value(CPUINFO, "processor"), Some(b"0".to_vec()));
    assert_eq!(key_value(CPUINFO, "nonesuch"), None);
}

#[test]
fn key_value_requires_an_exact_key() {
    // A prefix match would let `MemFree` answer a query for `Mem`, and
    // `MemTotal` answer one for `MemT`.
    let meminfo = b"MemTotal:  16384 kB\nMemFree:  1024 kB\n";
    assert_eq!(key_value(meminfo, "Mem"), None);
    assert_eq!(key_value(meminfo, "MemTotal"), Some(b"16384 kB".to_vec()));
}

// ---------------------------------------------------------------------------
// parse_kib
// ---------------------------------------------------------------------------

#[test]
fn kib_parses_the_form_meminfo_writes() {
    assert_eq!(parse_kib(b"16384 kB"), Some(16_384));
    assert_eq!(parse_kib(b"  16384 kB  "), Some(16_384));
    assert_eq!(parse_kib(b"16384kB"), Some(16_384));
    assert_eq!(parse_kib(b"16384 KB"), Some(16_384));
    assert_eq!(parse_kib(b"16384"), Some(16_384));
}

#[test]
fn kib_refuses_a_unit_it_does_not_know() {
    // Guessing here means reporting 16 GiB as 16 KiB, in the same font.
    assert_eq!(parse_kib(b"16 MB"), None);
    assert_eq!(parse_kib(b"16 GB"), None);
    assert_eq!(parse_kib(b"lots"), None);
    assert_eq!(parse_kib(b""), None);
}

// ---------------------------------------------------------------------------
// CpuInfo
// ---------------------------------------------------------------------------

#[test]
fn cpuinfo_counts_every_processor_line() {
    let cpu = CpuInfo::parse(CPUINFO).unwrap();
    assert_eq!(cpu.logical_cpus, 2);
    assert_eq!(cpu.vendor.as_deref(), Some(&b"GenuineIntel"[..]));
    assert_eq!(cpu.mhz.as_deref(), Some(&b"2592.000"[..]));
    assert_eq!(cpu.cache.as_deref(), Some(&b"12288 KB"[..]));
    assert!(
        cpu.model
            .as_deref()
            .unwrap()
            .starts_with(b"Intel(R) Core(TM) i7")
    );
}

#[test]
fn cpuinfo_with_no_processor_line_is_none_not_one_core() {
    // The code this replaces substituted 1 for a zero count, so a cpuinfo it
    // could not read at all was indistinguishable from a uniprocessor.
    assert!(CpuInfo::parse(b"").is_none());
    assert!(CpuInfo::parse(b"some other file entirely\n").is_none());
}

#[test]
fn cpuinfo_missing_fields_are_none_not_unknown_strings() {
    // "Unknown" is a display decision and belongs to the caller; a parser that
    // bakes it in cannot be asked whether the field was there.
    let cpu = CpuInfo::parse(b"processor: 0\n").unwrap();
    assert_eq!(cpu.logical_cpus, 1);
    assert!(cpu.model.is_none());
    assert!(cpu.vendor.is_none());
}

// ---------------------------------------------------------------------------
// MemInfo
// ---------------------------------------------------------------------------

const MEMINFO: &[u8] = b"\
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:    8192000 kB
Buffers:          512000 kB
Cached:          4096000 kB
SwapTotal:             0 kB
";

#[test]
fn meminfo_reads_the_five_fields_sysinfo_shows() {
    let mem = MemInfo::parse(MEMINFO);
    assert_eq!(mem.total_kib, Some(16_384_000));
    assert_eq!(mem.free_kib, Some(2_048_000));
    assert_eq!(mem.available_kib, Some(8_192_000));
    assert_eq!(mem.buffers_kib, Some(512_000));
    assert_eq!(mem.cached_kib, Some(4_096_000));
}

#[test]
fn meminfo_used_is_total_minus_free() {
    let mem = MemInfo::parse(MEMINFO);
    assert_eq!(mem.used_kib(), Some(16_384_000 - 2_048_000));
    let pct = mem.used_percent().unwrap();
    assert!((pct - 87.5).abs() < 0.001, "{pct}");
}

#[test]
fn meminfo_used_needs_both_figures() {
    assert!(MemInfo::parse(b"MemTotal: 100 kB\n").used_kib().is_none());
    assert!(MemInfo::parse(b"MemFree: 100 kB\n").used_kib().is_none());
    assert!(MemInfo::parse(b"").used_percent().is_none());
}

#[test]
fn meminfo_percent_of_a_zero_total_is_none_not_nan() {
    let mem = MemInfo::parse(b"MemTotal: 0 kB\nMemFree: 0 kB\n");
    assert_eq!(mem.total_kib, Some(0));
    assert!(mem.used_percent().is_none());
}

#[test]
fn meminfo_free_above_total_does_not_underflow() {
    // Not a thing a healthy kernel writes, but `saturating_sub` here is the
    // difference between "0 kB used" and 18 exabytes used.
    let mem = MemInfo::parse(b"MemTotal: 100 kB\nMemFree: 200 kB\n");
    assert_eq!(mem.used_kib(), Some(0));
    assert_eq!(mem.used_percent(), Some(0.0));
}

// ---------------------------------------------------------------------------
// Mount
// ---------------------------------------------------------------------------

const MOUNTS: &[u8] = b"\
/dev/sda1 / ext4 rw,relatime,errors=remount-ro 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev,mode=755 0 0
/dev/sdb1 /mnt/my\\040backup ext4 ro,noatime 0 0
";

#[test]
fn mounts_read_the_fields_in_the_kernels_order() {
    let mounts = Mount::parse_all(MOUNTS);
    assert_eq!(mounts.len(), 4);
    assert_eq!(mounts[0].device, b"/dev/sda1".to_vec());
    assert_eq!(mounts[0].mount_point, b"/".to_vec());
    assert_eq!(mounts[0].fstype, b"ext4".to_vec());
    assert!(mounts[0].options.starts_with(b"rw,"));
    assert_eq!(mounts[0].dump, 0);
    assert_eq!(mounts[0].pass, 0);
}

#[test]
fn a_mount_point_with_a_space_comes_back_with_the_space() {
    // The bug this crate exists to fix, one of three: splitting on whitespace
    // gets the field boundaries right (the kernel escaped the space for
    // exactly that reason) and the contents wrong, so the mount displayed as
    // `/mnt/my\040backup`.
    let mounts = Mount::parse_all(MOUNTS);
    assert_eq!(mounts[3].mount_point, b"/mnt/my backup".to_vec());
    assert_eq!(mounts[3].device, b"/dev/sdb1".to_vec());
    assert_eq!(mounts[3].fstype, b"ext4".to_vec());
}

#[test]
fn a_mount_point_that_is_not_utf8_survives() {
    // A SlateOS path is any bytes but `/` and NUL. Through `String` this line
    // is either dropped or corrupted; as bytes it is just a path.
    let line = b"/dev/sdc1 /mnt/\xff\xfe ext4 rw 0 0";
    let mount = Mount::parse_line(line).unwrap();
    assert_eq!(mount.mount_point, b"/mnt/\xff\xfe".to_vec());
    assert!(std::str::from_utf8(&mount.mount_point).is_err());
}

#[test]
fn mounts_skip_a_line_too_short_to_mean_anything() {
    // A three-field line has no options, and a mount shown without its options
    // can be shown as read-write when it is read-only.
    assert!(Mount::parse_line(b"/dev/sda1 / ext4").is_none());
    assert!(Mount::parse_line(b"").is_none());
    assert!(Mount::parse_line(b"   ").is_none());
}

#[test]
fn mounts_accept_a_four_field_mtab_line() {
    let mount = Mount::parse_line(b"/dev/sda1 / ext4 rw").unwrap();
    assert_eq!(mount.dump, 0);
    assert_eq!(mount.pass, 0);
}

#[test]
fn mount_options_are_matched_whole() {
    let mounts = Mount::parse_all(MOUNTS);
    assert!(!mounts[0].is_read_only());
    assert!(mounts[3].is_read_only());
    // The substring trap: `rootcontext=…` contains `ro`, and a mount reported
    // read-only when it is not is a claim a user may act on.
    let tricky = Mount::parse_line(b"d /m ext4 rw,rootcontext=system_u 0 0").unwrap();
    assert!(!tricky.is_read_only());
    assert!(tricky.has_option("rw"));
    assert!(tricky.has_option("rootcontext=system_u"));
}

#[test]
fn mount_option_list_drops_empty_entries() {
    let mount = Mount::parse_line(b"d /m ext4 rw,,noatime, 0 0").unwrap();
    assert_eq!(mount.option_list(), vec![&b"rw"[..], &b"noatime"[..]]);
}

#[test]
fn mounts_tolerate_a_trailing_newline_and_blank_lines() {
    let mounts = Mount::parse_all(b"\n/dev/sda1 / ext4 rw 0 0\n\n");
    assert_eq!(mounts.len(), 1);
}

// ---------------------------------------------------------------------------
// LoadAvg / Uptime / StatCounters / NetDevice
// ---------------------------------------------------------------------------

#[test]
fn loadavg_reads_all_five_fields() {
    let load = LoadAvg::parse(b"0.52 0.31 0.20 2/431 8123\n").unwrap();
    assert!((load.one - 0.52).abs() < 1e-9);
    assert!((load.five - 0.31).abs() < 1e-9);
    assert!((load.fifteen - 0.20).abs() < 1e-9);
    assert_eq!(load.runnable, Some(2));
    assert_eq!(load.total, Some(431));
    assert_eq!(load.last_pid, Some(8123));
}

#[test]
fn loadavg_needs_three_averages() {
    assert!(LoadAvg::parse(b"0.52 0.31\n").is_none());
    assert!(LoadAvg::parse(b"").is_none());
    assert!(LoadAvg::parse(b"a b c\n").is_none());
}

#[test]
fn loadavg_without_the_trailing_fields_still_parses() {
    let load = LoadAvg::parse(b"0.52 0.31 0.20\n").unwrap();
    assert_eq!(load.runnable, None);
    assert_eq!(load.last_pid, None);
}

#[test]
fn uptime_splits_into_days_hours_minutes_seconds() {
    let up = Uptime::parse(b"93784.42 370000.00\n").unwrap();
    assert_eq!(up.dhms(), (1, 2, 3, 4));
    assert!(up.idle.is_some());
}

#[test]
fn uptime_without_an_idle_field_still_parses() {
    let up = Uptime::parse(b"10.0\n").unwrap();
    assert_eq!(up.idle, None);
    assert_eq!(up.dhms(), (0, 0, 0, 10));
}

#[test]
fn uptime_refuses_values_duration_would_panic_on() {
    // `Duration::from_secs_f64` panics on a negative or a NaN, and this is a
    // file whose contents we do not control. A system-information tool that
    // aborts because a pseudo-file said `nan` is worse than one that omits the
    // line.
    assert!(Uptime::parse(b"-1.0\n").is_none());
    assert!(Uptime::parse(b"nan\n").is_none());
    assert!(Uptime::parse(b"inf\n").is_none());
    assert!(Uptime::parse(b"\n").is_none());
}

/// Every non-CPU line of `/proc/stat`, in one pass.
///
/// This fixture already carried `intr 100` and `ctxt 5000` when the struct read
/// neither -- the test asserted three fields and the data for five was sitting
/// in front of it. Worth noticing: a fixture containing a field nothing reads
/// looks exactly like a fixture whose field is checked.
#[test]
fn stat_counters_read_every_non_cpu_line() {
    let stat = StatCounters::parse(
        b"cpu  1 2 3 4 5\nintr 100\nctxt 5000\nbtime 1757000000\nprocesses 1234\nprocs_running 3\nprocs_blocked 1\n",
    );
    assert_eq!(stat.running, Some(3));
    assert_eq!(stat.blocked, Some(1));
    assert_eq!(stat.forks, Some(1234));
    assert_eq!(stat.context_switches, Some(5000));
    assert_eq!(stat.interrupts, Some(100));
    assert_eq!(stat.boot_time, Some(1_757_000_000));
}

/// **`intr` is a total followed by one count per IRQ**, and only the total is
/// wanted.
///
/// A real machine's `intr` line has hundreds of fields. A reader that summed
/// the line, or took the last field, would report a number that is not the
/// interrupt count and would drift as IRQs are registered -- and on a quiet
/// machine the difference is small enough to look plausible.
#[test]
fn stat_counters_take_only_the_total_from_the_intr_line() {
    let stat = StatCounters::parse(b"intr 987 12 0 0 44 0 0 7 0 0 0 3\n");
    assert_eq!(stat.interrupts, Some(987));
}

/// A `btime` of zero is a value, not an absence -- the epoch is a real answer
/// for a machine whose clock was never set, and an `unwrap_or_default` on the
/// other side would make the two indistinguishable.
#[test]
fn stat_counters_keep_a_zero_apart_from_a_missing_line() {
    assert_eq!(StatCounters::parse(b"btime 0\n").boot_time, Some(0));
    assert_eq!(StatCounters::parse(b"cpu 1\n").boot_time, None);
}

#[test]
fn stat_counters_absent_lines_stay_none() {
    let stat = StatCounters::parse(b"cpu  1 2 3 4 5\n");
    assert_eq!(stat, StatCounters::default());
}

const NETDEV: &[u8] = b"\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo:  123456    1000    0    0    0     0          0         0   123456    1000    0    0    0     0       0          0
  eth0: 9876543   54321    0    0    0     0          0         0  1234567    7654    0    0    0     0       0          0
";

#[test]
fn netdev_skips_headers_by_shape_not_by_count() {
    // Neither header line contains a colon, so "has a colon" identifies an
    // interface line without knowing how many headers there are. The rule it
    // replaces -- skip exactly one line, then skip anything starting with
    // `Inter` or `face` -- is a rule about this kernel's spelling of the
    // header, and it produces a phantom interface or a missing one the moment
    // the header gains or loses a line.
    let devices = NetDevice::parse_all(NETDEV);
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].name, b"lo".to_vec());
    assert_eq!(devices[1].name, b"eth0".to_vec());
}

#[test]
fn netdev_takes_transmitted_bytes_from_the_ninth_counter() {
    let devices = NetDevice::parse_all(NETDEV);
    assert_eq!(devices[1].rx_bytes, Some(9_876_543));
    assert_eq!(devices[1].rx_packets, Some(54_321));
    assert_eq!(devices[1].tx_bytes, Some(1_234_567));
    assert_eq!(devices[1].tx_packets, Some(7_654));
}

#[test]
fn netdev_ignores_a_line_whose_name_is_empty() {
    assert!(NetDevice::parse_all(b": 1 2 3\n").is_empty());
}

// ---------------------------------------------------------------------------
// ProcFs — the reader
// ---------------------------------------------------------------------------

#[test]
fn absent_is_ok_none_not_an_error() {
    let fixture = Fixture::new("absent");
    let proc = fixture.procfs();
    assert_eq!(proc.read_optional("meminfo").unwrap(), None);
    assert_eq!(proc.memory().unwrap(), None);
    assert_eq!(proc.cpu().unwrap(), None);
    assert_eq!(proc.mounts().unwrap(), None);
    assert_eq!(proc.uptime().unwrap(), None);
    assert_eq!(proc.load_average().unwrap(), None);
    assert_eq!(proc.net_devices().unwrap(), None);
    assert_eq!(proc.swaps().unwrap(), None);
    assert_eq!(proc.version().unwrap(), None);
    assert_eq!(proc.cmdline().unwrap(), None);
    assert_eq!(proc.hostname().unwrap(), None);
}

#[test]
fn a_read_error_that_is_not_absence_is_reported() {
    // The whole point of `read_optional`. A directory where a file is expected
    // is not "not found" -- on every platform this builds for, reading it
    // fails with some *other* error -- so it must not be silently reported to
    // the user as "(meminfo not available)".
    let fixture = Fixture::new("isdir");
    fs::create_dir_all(fixture.dir.join("meminfo")).unwrap();
    let err = fixture
        .procfs()
        .read_optional("meminfo")
        .expect_err("reading a directory as a file must not look like absence");
    assert_ne!(err.kind(), io::ErrorKind::NotFound);
    // And the collector propagates it rather than flattening it to None.
    assert!(fixture.procfs().memory().is_err());
}

#[test]
fn collectors_parse_what_the_fixture_holds() {
    let fixture = Fixture::new("full");
    fixture.write("cpuinfo", CPUINFO);
    fixture.write("meminfo", MEMINFO);
    fixture.write("mounts", MOUNTS);
    fixture.write("loadavg", b"0.52 0.31 0.20 2/431 8123\n");
    fixture.write("uptime", b"93784.42 370000.00\n");
    fixture.write("stat", b"procs_running 3\nprocs_blocked 1\n");
    fixture.write("net/dev", NETDEV);
    fixture.write("version", b"  SlateOS 0.1.0  \n");
    fixture.write("cmdline", b"root=/dev/sda1 quiet\n");
    fixture.write("sys/kernel/hostname", b"slate\n");

    let proc = fixture.procfs();
    assert_eq!(proc.cpu().unwrap().unwrap().logical_cpus, 2);
    assert_eq!(proc.memory().unwrap().unwrap().total_kib, Some(16_384_000));
    assert_eq!(proc.mounts().unwrap().unwrap().len(), 4);
    assert_eq!(proc.load_average().unwrap().unwrap().last_pid, Some(8123));
    assert_eq!(proc.uptime().unwrap().unwrap().dhms(), (1, 2, 3, 4));
    assert_eq!(proc.stat_counters().unwrap().unwrap().running, Some(3));
    assert_eq!(proc.net_devices().unwrap().unwrap().len(), 2);
    assert_eq!(proc.version().unwrap(), Some(b"SlateOS 0.1.0".to_vec()));
    assert_eq!(
        proc.cmdline().unwrap(),
        Some(b"root=/dev/sda1 quiet".to_vec())
    );
    assert_eq!(proc.hostname().unwrap(), Some(b"slate".to_vec()));
}

#[test]
fn an_empty_cmdline_or_hostname_reads_as_absent() {
    // A kernel that exports the file but leaves it blank has told us nothing,
    // and `Cmdline:` followed by nothing is worse than no line at all.
    let fixture = Fixture::new("blank");
    fixture.write("cmdline", b"\n");
    fixture.write("sys/kernel/hostname", b"   \n");
    let proc = fixture.procfs();
    assert_eq!(proc.cmdline().unwrap(), None);
    assert_eq!(proc.hostname().unwrap(), None);
}

#[test]
fn swaps_distinguishes_none_configured_from_no_such_file() {
    let fixture = Fixture::new("swaps");
    let proc = fixture.procfs();
    assert_eq!(proc.swaps().unwrap(), None, "no file at all");

    fixture.write("swaps", b"Filename\t\t\tType\t\tSize\tUsed\tPriority\n");
    assert_eq!(
        proc.swaps().unwrap(),
        Some(Vec::new()),
        "header only means swap is off, which is not the same as no file"
    );

    fixture.write(
        "swaps",
        b"Filename\t\t\tType\t\tSize\tUsed\tPriority\n/swapfile\tfile\t\t2097148\t0\t-2\n",
    );
    let lines = proc.swaps().unwrap().unwrap();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with(b"/swapfile"));
}

#[test]
fn process_ids_are_the_numeric_entries_only_and_are_sorted() {
    let fixture = Fixture::new("pids");
    for name in ["1", "42", "7", "self", "cpuinfo", "sys", "1a", "-3"] {
        fs::create_dir_all(fixture.dir.join(name)).unwrap();
    }
    let pids = fixture.procfs().process_ids().unwrap();
    assert_eq!(pids, vec![1, 7, 42]);
}

#[test]
fn process_ids_on_a_missing_root_is_an_error_not_an_empty_list() {
    // An empty list means "no processes", which is impossible and would be
    // displayed as `Running: 0 processes`. A missing /proc is a failure and
    // must say so.
    let proc = ProcFs::at(std::env::temp_dir().join("procinfo-no-such-dir-xyzzy"));
    assert!(proc.process_ids().is_err());
}

#[test]
fn default_and_new_point_at_the_kernels_proc() {
    assert_eq!(ProcFs::new().root(), Path::new("/proc"));
    assert_eq!(ProcFs::default().root(), Path::new("/proc"));
}

// ---------------------------------------------------------------------------
// Per-process: /proc/<pid>/stat, statm, status, cmdline
// ---------------------------------------------------------------------------

/// A `stat` line with the field numbering `proc(5)` gives, so the indices in
/// [`ProcessStat::parse`] can be checked against something readable.
///
/// Fields 3.. after the comm: state R, ppid 1, … utime 1234 (14), stime 567
/// (15), … priority 20 (18), nice -5 (19), threads 7 (20), … vsize 4096000
/// (23), rss 250 (24).
fn stat_line(comm: &str) -> Vec<u8> {
    format!(
        "42 ({comm}) R 1 42 42 0 -1 4194304 100 0 0 0 1234 567 0 0 20 -5 7 0 \
         99 4096000 250 18446744073709551615 1 2 3 4 5 6 7 8 9"
    )
    .into_bytes()
}

#[test]
fn process_stat_reads_the_fields_proc5_numbers() {
    let st = ProcessStat::parse(&stat_line("bash")).unwrap();
    assert_eq!(st.pid, 42);
    assert_eq!(st.comm, b"bash");
    assert_eq!(st.state, b'R');
    assert_eq!(st.ppid, 1);
    assert_eq!(st.pgrp, 42);
    assert_eq!(st.session, 42);
    assert_eq!(st.tty_nr, 0);
    assert_eq!(st.starttime_ticks, 99);
    assert_eq!(st.utime_ticks, 1234);
    assert_eq!(st.stime_ticks, 567);
    assert_eq!(st.priority, 20);
    assert_eq!(st.nice, -5);
    assert_eq!(st.num_threads, 7);
    assert_eq!(st.vsize_bytes, 4_096_000);
    assert_eq!(st.rss_pages, 250);
    assert_eq!(st.cpu_ticks(), 1234 + 567);
}

/// The whole reason this is not a `split_whitespace`.
///
/// A process may be named `my (odd) name`, and `/proc/<pid>/stat` wraps it in
/// one more pair of parentheses without escaping anything. Splitting on
/// whitespace mis-numbers every field after it — so a viewer would report the
/// wrong parent, the wrong memory and the wrong CPU time for exactly the
/// processes whose names are worth a second look.
#[test]
fn a_comm_with_spaces_and_parentheses_does_not_shift_the_fields() {
    let st = ProcessStat::parse(&stat_line("my (odd) name")).unwrap();
    assert_eq!(st.comm, b"my (odd) name");
    assert_eq!(st.state, b'R');
    assert_eq!(st.ppid, 1);
    assert_eq!(st.rss_pages, 250, "fields after comm must not shift");
}

/// A name that is not UTF-8 is kept, byte for byte.
///
/// `htop` read this file through `read_to_string`, so such a process was
/// dropped from the list entirely rather than shown with an odd name — and our
/// own filesystem allows every byte but `/` and NUL, so this is not exotic.
#[test]
fn a_comm_that_is_not_utf8_survives() {
    let mut line = b"7 (od".to_vec();
    line.push(0xff);
    line.extend_from_slice(b"d) S 1 7 7 0 -1 0 0 0 0 0 1 2 0 0 20 0 1 0 0 1024 3 0 0 0 0 0 0");
    let st = ProcessStat::parse(&line).unwrap();
    assert_eq!(st.comm, b"od\xffd");
    assert_eq!(st.state, b'S');
    assert!(
        core::str::from_utf8(&st.comm).is_err(),
        "the fixture must be non-UTF-8"
    );
}

/// Not shaped like `stat` at all: no parentheses, or too few fields.
#[test]
fn process_stat_refuses_a_line_that_is_not_stat() {
    assert!(ProcessStat::parse(b"").is_none());
    assert!(
        ProcessStat::parse(b"42 bash R 1").is_none(),
        "no parentheses"
    );
    assert!(
        ProcessStat::parse(b"42 (bash) R 1 2 3").is_none(),
        "too few fields"
    );
}

/// 16 KiB pages, not 4.
///
/// `stat` reports RSS in *pages*, and every `/proc` example on the internet
/// assumes 4 KiB. `design.txt` fixes SlateOS at 16, so a reader that takes the
/// internet's word is out by a factor of four and still prints a plausible
/// number — which is why this has a test rather than a comment.
#[test]
fn rss_converts_with_slateos_page_size() {
    let st = ProcessStat::parse(&stat_line("x")).unwrap();
    assert_eq!(PAGE_SIZE_KIB, 16);
    assert_eq!(st.rss_kib(), 250 * 16);
    // …and vsize is already bytes in the same line, which is the trap.
    assert_eq!(st.vsize_kib(), 4_096_000 / 1024);
}

#[test]
fn statm_reads_three_page_counts() {
    let m = ProcessStatm::parse(b"1000 250 100 5 0 60 0").unwrap();
    assert_eq!(m.size_pages, 1000);
    assert_eq!(m.resident_pages, 250);
    assert_eq!(m.shared_pages, 100);
    assert_eq!(m.shared_kib(), 100 * 16);
    assert!(ProcessStatm::parse(b"1 2").is_none());
}

/// `Uid:` and `Gid:` each carry four values -- real, effective, saved-set,
/// filesystem -- and the real one is the first. Taking a different one is a
/// one-character change at a call site and a security question, which is why
/// the choice is made in the crate once.
#[test]
fn status_takes_the_real_id_of_four() {
    let content = b"Name:\tbash\n\
Uid:\t1000\t1001\t1002\t1003\n\
Gid:\t100\t101\t102\t103\n\
Groups:\t4 24 27\n\
VmSize:\t  4096 kB\n\
VmRSS:\t   512 kB\n";
    let st = ProcessStatus::parse(content);
    assert_eq!(st.uid, Some(1000), "real, not effective");
    assert_eq!(st.gid, Some(100), "real, not effective");
    assert_eq!(st.groups, vec![4, 24, 27]);
    assert_eq!(st.vm_size_kib, Some(4096));
    assert_eq!(st.vm_rss_kib, Some(512));
}

/// A kernel thread has no address space, so `VmSize`/`VmRSS` are absent -- and
/// absent is not zero. It may also genuinely have no supplementary groups,
/// which is an empty list rather than a missing one.
#[test]
fn a_status_without_vm_fields_reports_none_not_zero() {
    let st = ProcessStatus::parse(b"Name:\tkthreadd\nUid:\t0\t0\t0\t0\n");
    assert_eq!(st.uid, Some(0));
    assert_eq!(st.vm_size_kib, None);
    assert_eq!(st.vm_rss_kib, None);
    assert!(st.groups.is_empty());

    // …and a status with nothing in it reports nothing, not zeros.
    let empty = ProcessStatus::parse(b"");
    assert_eq!(empty.uid, None);
    assert_eq!(empty.gid, None);
}

/// NUL-separated, NUL-terminated, and not text.
#[test]
fn cmdline_splits_on_nul_and_keeps_bytes() {
    assert_eq!(
        cmdline_args(b"/bin/ls\0-l\0/tmp\0"),
        vec![b"/bin/ls".to_vec(), b"-l".to_vec(), b"/tmp".to_vec()],
        "the trailing NUL must not produce an empty argument"
    );
    assert_eq!(
        cmdline_args(b"a\0\xff\0"),
        vec![b"a".to_vec(), b"\xff".to_vec()]
    );
    // A kernel thread has no command line. Empty, not absent.
    assert!(cmdline_args(b"").is_empty());
}

/// The readers, against a fixture `/proc`.
#[test]
fn procfs_reads_one_process() {
    let fx = Fixture::new("process");
    fx.write("42/stat", &stat_line("bash"));
    fx.write("42/statm", b"1000 250 100 5 0 60 0");
    fx.write("42/status", b"Name:\tbash\nUid:\t1000\t1000\t1000\t1000\n");
    fx.write("42/cmdline", b"/bin/bash\0-i\0");
    let procfs = fx.procfs();

    let st = procfs.process_stat(42).unwrap().unwrap();
    assert_eq!(st.comm, b"bash");
    assert_eq!(procfs.process_statm(42).unwrap().unwrap().shared_pages, 100);
    assert_eq!(procfs.process_status(42).unwrap().unwrap().uid, Some(1000));
    assert_eq!(
        procfs.process_cmdline(42).unwrap().unwrap(),
        vec![b"/bin/bash".to_vec(), b"-i".to_vec()]
    );
    assert_eq!(procfs.process_ids().unwrap(), vec![42]);
}

/// A process that exits between the listing and the read is `Ok(None)`, not an
/// error — that race is the normal case for anything walking `/proc`.
#[test]
fn a_vanished_process_is_not_an_error() {
    let fx = Fixture::new("vanished");
    let procfs = fx.procfs();
    assert_eq!(procfs.process_stat(999).unwrap(), None);
    assert_eq!(procfs.process_statm(999).unwrap(), None);
    assert_eq!(procfs.process_status(999).unwrap(), None);
    assert_eq!(procfs.process_cmdline(999).unwrap(), None);
}

/// A kernel thread and a vanished process gave the same answer before this
/// crate: both were "no command line". They are different facts.
#[test]
fn a_kernel_thread_is_an_empty_cmdline_not_a_missing_one() {
    let fx = Fixture::new("kthread");
    fx.write("3/cmdline", b"");
    let procfs = fx.procfs();
    assert_eq!(procfs.process_cmdline(3).unwrap(), Some(vec![]));
    assert_eq!(procfs.process_cmdline(4).unwrap(), None);
}

// ---------------------------------------------------------------------------
// /proc/stat: CPU time
// ---------------------------------------------------------------------------

/// A `/proc/stat` with an aggregate line, two CPUs, and the non-CPU lines a
/// real one carries — so the parser is asked to ignore things, not just to
/// read things.
const STAT: &[u8] = b"cpu  100 20 30 400 5 6 7 8 9 10\n\
cpu0 50 10 15 200 2 3 3 4 4 5\n\
cpu1 50 10 15 200 3 3 4 4 5 5\n\
intr 12345 0 0\n\
ctxt 999\n\
btime 1700000000\n\
processes 42\n\
procs_running 3\n\
procs_blocked 1\n";

#[test]
fn cpu_stats_reads_the_aggregate_and_each_cpu() {
    let st = CpuStats::parse(STAT);
    let total = st.total.unwrap();
    assert_eq!(total.user, 100);
    assert_eq!(total.nice, 20);
    assert_eq!(total.system, 30);
    assert_eq!(total.idle, 400);
    assert_eq!(total.iowait, 5);
    assert_eq!(total.irq, 6);
    assert_eq!(total.softirq, 7);
    assert_eq!(total.steal, 8);
    assert_eq!(total.guest, 9);
    assert_eq!(total.guest_nice, 10);
    assert_eq!(st.per_cpu.len(), 2);
    assert_eq!(st.per_cpu[0].user, 50);
    assert_eq!(st.per_cpu[1].iowait, 3);
}

/// `steal` is in the total and `guest`/`guest_nice` are not.
///
/// Both halves are load-bearing and for opposite reasons. Omitting `steal`
/// makes the total too small, so every process's CPU percentage comes out too
/// large — and SlateOS develops under QEMU, where `steal` is exactly the field
/// that is non-zero. Including `guest` double-counts, because Linux already
/// counts it inside `user`.
/// `since` already had tests -- for an interval, and for a counter going
/// backwards. Neither looked at `guest` or `guest_nice`, because
/// [`CpuTimes::total`] deliberately leaves those two out, so nothing that
/// checks a total can notice them being dropped.
///
/// This is what would break silently: a caller that later starts reporting
/// guest time would get a delta that had been zero all along, and zero is a
/// plausible number for guest time on hardware.
#[test]
fn since_subtracts_the_two_fields_total_ignores() {
    let prev = CpuTimes {
        user: 100,
        nice: 10,
        system: 30,
        idle: 800,
        iowait: 20,
        irq: 5,
        softirq: 3,
        steal: 2,
        guest: 11,
        guest_nice: 4,
    };
    let cur = CpuTimes {
        user: 200,
        nice: 15,
        system: 50,
        idle: 900,
        iowait: 25,
        irq: 7,
        softirq: 4,
        steal: 3,
        guest: 13,
        guest_nice: 9,
    };
    let d = cur.since(&prev);
    assert_eq!(d.user, 100);
    assert_eq!(d.nice, 5);
    assert_eq!(d.system, 20);
    assert_eq!(d.idle, 100);
    assert_eq!(d.iowait, 5);
    assert_eq!(d.irq, 2);
    assert_eq!(d.softirq, 1);
    assert_eq!(d.steal, 1);
    // `total()` leaves these two out; `since()` must not, or a caller that
    // later starts counting guest time gets a delta that was silently zero.
    assert_eq!(d.guest, 2);
    assert_eq!(d.guest_nice, 5);
}

#[test]
fn total_includes_steal_and_excludes_guest() {
    let t = CpuStats::parse(STAT).total.unwrap();
    assert_eq!(t.total(), 100 + 20 + 30 + 400 + 5 + 6 + 7 + 8);
    // The two ways of getting it wrong, stated as what they would produce:
    assert_ne!(
        t.total(),
        100 + 20 + 30 + 400 + 5 + 6 + 7,
        "steal was dropped"
    );
    assert_ne!(
        t.total(),
        100 + 20 + 30 + 400 + 5 + 6 + 7 + 8 + 9 + 10,
        "guest was double-counted"
    );
    assert_eq!(t.busy(), t.total() - 400 - 5);
}

/// An older kernel publishes fewer fields, and that is a valid file rather
/// than a broken one — `iowait` arrived in 2.5.41 and `steal` in 2.6.11. The
/// reader this replaces required seven fields and silently dropped any CPU
/// line with fewer, turning an older kernel into an empty CPU list.
#[test]
fn a_short_cpu_line_reads_the_fields_it_has() {
    let st = CpuStats::parse(b"cpu  1 2 3 4\ncpu0 1 2 3 4\n");
    let t = st.total.unwrap();
    assert_eq!((t.user, t.nice, t.system, t.idle), (1, 2, 3, 4));
    assert_eq!(t.iowait, 0);
    assert_eq!(t.steal, 0);
    assert_eq!(t.total(), 10);
    assert_eq!(
        st.per_cpu.len(),
        1,
        "the short cpu0 line must not be dropped"
    );
}

/// Lines that are not CPU lines are ignored, including ones that merely start
/// with the same letters.
#[test]
fn non_cpu_lines_are_ignored() {
    assert!(CpuTimes::parse_line(b"intr 1 2 3").is_none());
    assert!(CpuTimes::parse_line(b"ctxt 999").is_none());
    assert!(CpuTimes::parse_line(b"").is_none());
    // `cpu` followed by something that is not a number is not `cpuN`.
    assert!(CpuTimes::parse_line(b"cpufreq 1 2 3").is_none());
    assert_eq!(CpuTimes::parse_line(b"cpu 1 2 3").unwrap().0, None);
    assert_eq!(CpuTimes::parse_line(b"cpu7 1 2 3").unwrap().0, Some(7));
}

/// One bar for the whole machine beats no bars.
#[test]
fn per_cpu_falls_back_to_the_aggregate() {
    let one = CpuStats::parse(b"cpu  1 2 3 4 5 6 7 8\n");
    assert!(one.per_cpu.is_empty());
    assert_eq!(one.per_cpu_or_total().len(), 1);
    assert_eq!(CpuStats::parse(STAT).per_cpu_or_total().len(), 2);
    assert!(CpuStats::parse(b"intr 1\n").per_cpu_or_total().is_empty());
}

/// The whole file is read once. The reader this replaces opened `/proc/stat` a
/// second time when it found no `cpuN` lines, so on a single-CPU machine it
/// read two different instants and compared them.
#[test]
fn procfs_reads_cpu_stats_from_a_fixture() {
    let fx = Fixture::new("cpustat");
    fx.write("stat", STAT);
    let st = fx.procfs().cpu_stats().unwrap().unwrap();
    assert_eq!(st.per_cpu.len(), 2);
    assert!(st.total.is_some());
    // …and the scheduler counters in the same file still parse, since both
    // readers see every line.
    let sched = StatCounters::parse(STAT);
    assert_eq!(sched.running, Some(3));
    assert_eq!(sched.blocked, Some(1));
}

/// The subtraction a viewer needs before it divides.
#[test]
fn since_gives_the_interval_not_the_lifetime() {
    let earlier = CpuTimes {
        user: 100,
        idle: 900,
        ..CpuTimes::default()
    };
    let later = CpuTimes {
        user: 150,
        idle: 950,
        ..CpuTimes::default()
    };
    let d = later.since(&earlier);
    assert_eq!(d.user, 50);
    assert_eq!(d.idle, 50);
    assert_eq!(d.total(), 100);
    // The point of doing it at all: the lifetime ratio and the interval ratio
    // are different numbers, and only the second one moves.
    assert_eq!(later.user * 100 / later.total(), 13);
    assert_eq!(d.user * 100 / d.total(), 50);
}

/// A CPU taken offline and brought back restarts its counters, so `earlier`
/// can legitimately be the larger sample. That must be a zero-length interval
/// for one refresh, not a panic and not a wrap to something enormous.
#[test]
fn since_saturates_when_a_counter_goes_backwards() {
    let earlier = CpuTimes {
        user: 500,
        idle: 500,
        ..CpuTimes::default()
    };
    let later = CpuTimes {
        user: 10,
        idle: 20,
        ..CpuTimes::default()
    };
    let d = later.since(&earlier);
    assert_eq!(d.user, 0);
    assert_eq!(d.idle, 0);
    assert_eq!(
        d.total(),
        0,
        "a zero interval, which a caller must treat as no data"
    );
}

/// Swap, and the two different "used" figures.
#[test]
fn meminfo_reads_swap_and_both_used_figures() {
    let content = b"MemTotal:       8000 kB\n\
MemFree:        1000 kB\n\
MemAvailable:   5000 kB\n\
Buffers:         500 kB\n\
Cached:         2500 kB\n\
SwapTotal:      4000 kB\n\
SwapFree:       3000 kB\n";
    let m = MemInfo::parse(content);
    assert_eq!(m.swap_total_kib, Some(4000));
    assert_eq!(m.swap_free_kib, Some(3000));
    assert_eq!(m.swap_used_kib(), Some(1000));
    // The kernel's figure: everything that is not free.
    assert_eq!(m.used_kib(), Some(7000));
    // The one a person is shown: cache and buffers are reclaimable, so a
    // machine holding 2.5 MiB of cache is not short of memory.
    assert_eq!(m.used_excluding_cache_kib(), Some(4000));
}

/// A machine with no swap and a kernel that does not export swap at all are
/// different answers.
#[test]
fn no_swap_and_no_swap_field_are_distinguishable() {
    let none = MemInfo::parse(b"MemTotal: 8000 kB\n");
    assert_eq!(none.swap_total_kib, None);
    assert_eq!(none.swap_used_kib(), None);

    let zero = MemInfo::parse(b"SwapTotal: 0 kB\nSwapFree: 0 kB\n");
    assert_eq!(zero.swap_total_kib, Some(0));
    assert_eq!(zero.swap_used_kib(), Some(0));
}

/// Every figure must be present, because a used-memory number derived from a
/// missing one is not worth showing.
#[test]
fn used_excluding_cache_needs_every_figure() {
    let m = MemInfo::parse(b"MemTotal: 8000 kB\nMemFree: 1000 kB\nBuffers: 500 kB\n");
    assert_eq!(m.used_kib(), Some(7000), "the simple figure still works");
    assert_eq!(m.used_excluding_cache_kib(), None, "Cached is missing");
}

// ---------------------------------------------------------------------------
// Showing bytes to a person
// ---------------------------------------------------------------------------

/// The ordinary case costs nothing and changes nothing.
#[test]
fn valid_utf8_passes_through_unchanged() {
    assert_eq!(display_bytes(b"bash"), "bash");
    assert_eq!(display_bytes("na\u{ef}ve".as_bytes()), "na\u{ef}ve");
    assert_eq!(display_bytes(b""), "");
}

/// An invalid byte is shown, not swallowed. This is the case that used to
/// remove the whole process from the list: the old reader went through
/// `read_to_string`, which fails, and `read_process` returned `None`.
#[test]
fn an_invalid_byte_is_shown_as_hex() {
    assert_eq!(display_bytes(b"od\xffd"), r"od\xffd");
    assert_eq!(display_bytes(b"\xc3"), r"\xc3");
}

/// The valid parts either side of a bad byte survive intact.
#[test]
fn text_around_an_invalid_byte_is_kept() {
    assert_eq!(display_bytes(b"a\xffb\xfec"), r"a\xffb\xfec");
    // `\xc3\xa9` is a valid `é`; the `\xff` after it is not.
    assert_eq!(display_bytes(b"\xc3\xa9\xff"), "\u{e9}\\xff");
}

/// The reason this is not `from_utf8_lossy`: that maps every invalid byte
/// to U+FFFD, so two different names become the same string and a viewer
/// cannot tell one process from another. `CLAUDE.md` self-review item 7
/// calls the lossy conversion silent data corruption; here it would be
/// corruption of the very thing the user is reading.
///
/// The second assertion is the one that makes the point: it shows the
/// alternative really does collide, rather than asserting that ours does
/// not and leaving the comparison to the reader.
#[test]
fn two_different_invalid_names_do_not_collide() {
    assert_ne!(display_bytes(b"x\xff"), display_bytes(b"x\xfe"));
    assert_eq!(
        String::from_utf8_lossy(b"x\xff"),
        String::from_utf8_lossy(b"x\xfe"),
        "if this ever fails, from_utf8_lossy has changed and this test's premise with it"
    );
}

/// A truncated multi-byte sequence at the very end has no continuation to
/// consume, which is the loop's one exit that is not `Ok`.
#[test]
fn a_truncated_sequence_at_the_end_terminates() {
    assert_eq!(display_bytes(b"ok\xe2\x82"), r"ok\xe2\x82");
}
