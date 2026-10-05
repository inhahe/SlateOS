//! The system-wide figures procps' library reads for `ps`: the width of a
//! PID, when the machine booted, how much memory it has, how long it has been
//! up, and what a process is waiting in.
//!
//! | here | procps-ng 4.0.4 |
//! |---|---|
//! | [`pid_length`] | `procps_pid_length` (`sysinfo.c`) |
//! | [`boot_time`] | `procps_stat_new` + `STAT_SYS_TIME_OF_BOOT` (`stat.c`) |
//! | [`memory_total`] | `procps_meminfo_new` + `MEMINFO_MEM_TOTAL` (`meminfo.c`) |
//! | [`uptime`] | `procps_uptime` (`uptime.c`) |
//! | [`lookup_wchan`] | `lookup_wchan` (`wchan.c`) |

use std::fs;
use std::io::Read;
use std::path::Path;

use super::readproc::cstr;
use super::scanf::{self, Scan};

/// `DEFAULT_PID_LENGTH`.
const DEFAULT_PID_LENGTH: usize = 5;

/// `procps_pid_length`: how many characters the largest PID takes -- the
/// length of `/proc/sys/kernel/pid_max`'s first line, without its newline --
/// or 5 if it cannot be read. `fgets` reads at most 23 bytes of the line.
#[must_use]
pub fn pid_length(root: &Path) -> usize {
    let Ok(text) = fs::read(root.join("sys/kernel/pid_max")) else {
        return DEFAULT_PID_LENGTH;
    };
    if text.is_empty() {
        return DEFAULT_PID_LENGTH;
    }
    // `fgets (pidbuf, 24, fp)`: up to and including a newline, at most 23.
    let line_end = text
        .iter()
        .position(|&b| b == b'\n')
        .map_or(text.len(), |n| n.saturating_add(1));
    let got = text.get(..line_end.min(23)).unwrap_or_default();
    let mut len = cstr(got).len();
    if len > 0 && got.get(len.saturating_sub(1)) == Some(&b'\n') {
        len = len.saturating_sub(1);
    }
    len
}

/// The `cpu` lines `stat_read_failed` requires: the summary line must convert
/// at least eight numbers.
fn cpu_summary_ok(line: &[u8]) -> bool {
    let mut sc = Scan::new(line);
    if sc.lit(b"cpu ").is_none() {
        return false;
    }
    (0..8).all(|_| sc.ulong().is_some())
}

/// Whether a line after the summary is a per-CPU line: `cpu%d` and at least
/// seven more numbers.
fn cpu_line_ok(line: &[u8]) -> bool {
    let mut sc = Scan::new(line);
    if sc.lit(b"cpu").is_none() || sc.int().is_none() {
        return false;
    }
    (0..7).all(|_| sc.ulong().is_some())
}

/// `procps_stat_new`'s reading of `/proc/stat`, reduced to what `ps` asks
/// of it: the `btime` line's number, or 0 without one -- or `None`, which is
/// `ps`'s "Unable to get system boot time", when the file cannot be read or
/// its first line is not a `cpu` summary.
///
/// `btime` is searched for only after the per-CPU lines, which is where the
/// kernel writes it. A file whose CPU lines end without a newline crashes
/// upstream (`strchr` finds none, and the scan goes on at address 1); here it
/// is refused like one that cannot be read.
#[must_use]
pub fn boot_time(root: &Path) -> Option<u64> {
    let text = fs::read(root.join("stat")).ok()?;
    let buf = cstr(&text);
    if !cpu_summary_ok(buf) {
        return None;
    }
    // `bp = 1 + strchr (bp, '\n')` for each line that parses as a CPU.
    let mut bp = 0usize;
    loop {
        let nl = buf.get(bp..)?.iter().position(|&b| b == b'\n')?;
        bp = bp.saturating_add(nl).saturating_add(1);
        if !cpu_line_ok(buf.get(bp..).unwrap_or_default()) {
            break;
        }
    }
    let rest = buf.get(bp..).unwrap_or_default();
    let Some(at) = rest.windows(6).position(|w| w == b"btime ") else {
        return Some(0);
    };
    let mut sc = Scan::new(rest.get(at..).unwrap_or_default());
    if sc.lit(b"btime ").is_none() {
        return Some(0);
    }
    Some(sc.ulong().unwrap_or(0))
}

/// `MEMINFO_BUFF - 1`: how much of `/proc/meminfo` is read.
const MEMINFO_READ: usize = 8191;

/// `procps_meminfo_new`'s `MemTotal`: the number after the last `MemTotal:`
/// key -- `strtoul`'s, so `8000 kB` is 8000 -- 0 if there is no such line,
/// or `None` if the file cannot be read or is empty (`Unable to get total
/// memory`).
#[must_use]
pub fn memory_total(root: &Path) -> Option<u64> {
    let mut file = fs::File::open(root.join("meminfo")).ok()?;
    let mut buf = vec![0u8; MEMINFO_READ];
    let n = file.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    buf.truncate(n);
    let text = cstr(&buf);
    let mut total = 0u64;
    let mut head = 0usize;
    while let Some(colon) = text
        .get(head..)
        .and_then(|r| r.iter().position(|&b| b == b':'))
    {
        let key_end = head.saturating_add(colon);
        let key = text.get(head..key_end).unwrap_or_default();
        head = key_end.saturating_add(1);
        if key == b"MemTotal" {
            total = scanf::strtoul(text.get(head..).unwrap_or_default()).0;
        }
        let Some(nl) = text
            .get(head..)
            .and_then(|r| r.iter().position(|&b| b == b'\n'))
        else {
            break;
        };
        head = head.saturating_add(nl).saturating_add(1);
    }
    Some(total)
}

/// `procps_uptime`'s seconds since boot: the first number of `/proc/uptime`
/// as `fscanf` reads it, 0 if there is none. (A file that cannot be opened
/// leaves upstream's caller with an uninitialised `double`; 0 here.)
#[must_use]
pub fn uptime(root: &Path) -> f64 {
    fs::read(root.join("uptime"))
        .ok()
        .and_then(|t| super::scan_doubles(&t, 2).first().copied())
        .unwrap_or(0.0)
}

/// `lookup_wchan`: the kernel function `pid` sleeps in, from
/// `/proc/<pid>/wchan` -- `?` when it cannot be read or is empty, `-` when it
/// is `0`, and otherwise its text after a leading `.` and any `_`s.
#[must_use]
pub fn lookup_wchan(root: &Path, pid: i32) -> Vec<u8> {
    let Ok(mut file) = fs::File::open(root.join(format!("{pid}/wchan"))) else {
        return b"?".to_vec();
    };
    let mut buf = vec![0u8; 63];
    let Ok(n) = file.read(&mut buf) else {
        return b"?".to_vec();
    };
    if n < 1 {
        return b"?".to_vec();
    }
    buf.truncate(n);
    let text = cstr(&buf);
    if text == b"0" {
        return b"-".to_vec();
    }
    let mut t = text;
    if t.first() == Some(&b'.') {
        t = t.get(1..).unwrap_or_default();
    }
    let underscores = t.iter().take_while(|&&b| b == b'_').count();
    t.get(underscores..).unwrap_or_default().to_vec()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tree(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lane-b-sysinfo-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (path, text) in files {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }
        dir
    }

    #[test]
    fn pid_length_is_the_first_line_measured() {
        let d = tree("pidlen", &[("sys/kernel/pid_max", b"4194304\n")]);
        assert_eq!(pid_length(&d), 7);
        fs::write(d.join("sys/kernel/pid_max"), b"32768").unwrap();
        assert_eq!(pid_length(&d), 5);
        fs::write(
            d.join("sys/kernel/pid_max"),
            b"123456789012345678901234567890\n",
        )
        .unwrap();
        assert_eq!(pid_length(&d), 23);
        fs::write(d.join("sys/kernel/pid_max"), b"").unwrap();
        assert_eq!(pid_length(&d), 5);
        fs::remove_dir_all(&d).unwrap();
        assert_eq!(pid_length(&d), 5);
    }

    #[test]
    fn boot_time_needs_a_cpu_summary_and_reads_btime_after_the_cpus() {
        let d = tree(
            "btime",
            &[(
                "stat",
                b"cpu  1 2 3 4 5 6 7 8 0 0\ncpu0 1 2 3 4 5 6 7 8 0 0\nintr 5\nbtime 1700000000\n",
            )],
        );
        assert_eq!(boot_time(&d), Some(1_700_000_000));
        fs::write(d.join("stat"), b"cpu  1 2 3\nbtime 5\n").unwrap();
        assert_eq!(boot_time(&d), None);
        fs::write(d.join("stat"), b"cpu  1 2 3 4 5 6 7 8\nintr 1\n").unwrap();
        assert_eq!(boot_time(&d), Some(0));
        fs::remove_dir_all(&d).unwrap();
        assert_eq!(boot_time(&d), None);
    }

    #[test]
    fn memtotal_is_the_last_one_given() {
        let d = tree(
            "mem",
            &[(
                "meminfo",
                b"MemTotal:  8000 kB\nMemFree: 1 kB\nMemTotal: 9000 kB\n",
            )],
        );
        assert_eq!(memory_total(&d), Some(9000));
        fs::write(d.join("meminfo"), b"MemFree: 1 kB\n").unwrap();
        assert_eq!(memory_total(&d), Some(0));
        fs::write(d.join("meminfo"), b"").unwrap();
        assert_eq!(memory_total(&d), None);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn wchan_spellings() {
        let d = tree(
            "wchan",
            &[
                ("5/wchan", b"0"),
                ("6/wchan", b"._do_wait"),
                ("7/wchan", b""),
            ],
        );
        assert_eq!(lookup_wchan(&d, 5), b"-");
        assert_eq!(lookup_wchan(&d, 6), b"do_wait");
        assert_eq!(lookup_wchan(&d, 7), b"?");
        assert_eq!(lookup_wchan(&d, 8), b"?");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn uptime_is_the_first_number() {
        let d = tree("up", &[("uptime", b"1000.50 2000.00\n")]);
        assert!((uptime(&d) - 1000.5).abs() < 1e-9);
        fs::write(d.join("uptime"), b"junk").unwrap();
        assert!(uptime(&d).abs() < 1e-9);
        fs::remove_dir_all(&d).unwrap();
    }
}
