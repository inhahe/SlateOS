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
// LoadAvg / Uptime / SchedCounters / NetDevice
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

#[test]
fn sched_counters_read_the_three_lines_that_matter() {
    let stat = SchedCounters::parse(
        b"cpu  1 2 3 4 5\nintr 100\nctxt 5000\nprocesses 1234\nprocs_running 3\nprocs_blocked 1\n",
    );
    assert_eq!(stat.running, Some(3));
    assert_eq!(stat.blocked, Some(1));
    assert_eq!(stat.forks, Some(1234));
}

#[test]
fn sched_counters_absent_lines_stay_none() {
    let stat = SchedCounters::parse(b"cpu  1 2 3 4 5\n");
    assert_eq!(stat, SchedCounters::default());
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
    assert_eq!(proc.sched_counters().unwrap().unwrap().running, Some(3));
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
