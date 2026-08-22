//! df — report filesystem disk space usage.
//!
//! Usage: df [-h] [FILE...]
//!   -h  human-readable sizes (K, M, G, T, ...)
//!   Without arguments: show all mounted filesystems.
//!   With FILE: show the filesystem containing FILE.
//!
//! Portability: on POSIX-y targets (Linux, and our `x86_64-slateos` custom
//! target which reports `target_os = "linux"`) we use the `statvfs()`
//! C-runtime call to read filesystem stats.  On Windows hosts the symbol
//! is not in mingw-w64; `stat_fs` falls back to a stub error so the
//! crate still builds for `cargo test` on the dev machine.

use coreutils::human::{Opts, human_readable};
use coreutils::quote::quotef_os;
use std::env;
use std::io::{self, Write};
use std::process;

/// statvfs-like struct returned by our POSIX layer.
#[cfg(target_os = "linux")]
#[repr(C)]
struct Statvfs {
    f_bsize: u64,
    f_frsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    f_fsid: u64,
    f_flag: u64,
    f_namemax: u64,
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn statvfs(path: *const u8, buf: *mut Statvfs) -> i32;
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct DfArgs {
    human: bool,
    paths: Vec<String>,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut parsed = parse_args(&args);
    if parsed.paths.is_empty() {
        parsed.paths.push("/".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(
        out,
        "{:<20} {:>10} {:>10} {:>10} {:>5} Mounted on",
        "Filesystem", "Size", "Used", "Avail", "Use%"
    );

    let mut exit_code = 0;
    for path in &parsed.paths {
        match stat_fs(path) {
            Ok(info) => {
                let row = format_row(&info, path, parsed.human);
                let _ = writeln!(out, "{row}");
            }
            Err(e) => {
                eprintln!("df: {}: {e}", quotef_os(path));
                exit_code = 1;
            }
        }
    }

    process::exit(exit_code);
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq, Clone))]
struct FsInfo {
    name: String,
    total_bytes: u64,
    free_bytes: u64,
    avail_bytes: u64,
}

/// Parse df's argv into `(human, paths)`.
fn parse_args(args: &[String]) -> DfArgs {
    let mut human = false;
    let mut paths: Vec<String> = Vec::new();
    for arg in args {
        if arg == "-h" {
            human = true;
        } else {
            paths.push(arg.clone());
        }
    }
    DfArgs { human, paths }
}

/// Format a single filesystem row. `human` switches between IEC suffixes
/// and 1 KiB block counts.
fn format_row(info: &FsInfo, path: &str, human: bool) -> String {
    let total = info.total_bytes;
    let free = info.avail_bytes;
    let used = total.saturating_sub(info.free_bytes);
    let pct = used.saturating_mul(100).checked_div(total).unwrap_or(0) as u32;

    if human {
        format!(
            "{:<20} {:>10} {:>10} {:>10} {:>4}% {}",
            info.name,
            human_size(total),
            human_size(used),
            human_size(free),
            pct,
            path
        )
    } else {
        format!(
            "{:<20} {:>10} {:>10} {:>10} {:>4}% {}",
            info.name,
            total / 1024,
            used / 1024,
            free / 1024,
            pct,
            path
        )
    }
}

#[cfg(target_os = "linux")]
fn stat_fs(path: &str) -> Result<FsInfo, String> {
    let mut c_path: Vec<u8> = path.as_bytes().to_vec();
    c_path.push(0);

    // SAFETY: zeroed bytes are a valid bit pattern for `Statvfs` (all-u64 POD).
    let mut buf: Statvfs = unsafe { core::mem::zeroed() };

    // SAFETY: c_path is null-terminated, buf is properly aligned and sized.
    let ret = unsafe { statvfs(c_path.as_ptr(), &mut buf) };
    if ret != 0 {
        return Err("statvfs failed".to_string());
    }

    let bsize = if buf.f_frsize > 0 {
        buf.f_frsize
    } else {
        buf.f_bsize
    };

    Ok(FsInfo {
        name: path.to_string(),
        total_bytes: buf.f_blocks.saturating_mul(bsize),
        free_bytes: buf.f_bfree.saturating_mul(bsize),
        avail_bytes: buf.f_bavail.saturating_mul(bsize),
    })
}

#[cfg(not(target_os = "linux"))]
fn stat_fs(_path: &str) -> Result<FsInfo, String> {
    Err("statvfs not available on this platform".to_string())
}

/// Format a byte count the way `df -h` does: `1.9T`, `554G`, `1.0K`, or a bare
/// count below a kibibyte.
///
/// The option set is the one `du -h` uses, confirmed against GNU rather than
/// assumed: reading `df --block-size=1` and `df -h` for the same filesystems
/// gives 553.9 GiB → `554G` and 299.8 GiB → `300G`, so the rounding is upward,
/// and 1.818 TiB → `1.9T` fixes the base at 1024.
///
/// This replaces a hand-rolled `{:.1}` chain byte-identical to the one `du`
/// carried, with the same four divergences from GNU — a spurious `B` on bare
/// counts, round-to-nearest instead of up, a decimal that never disappeared at
/// ten, and no prefix above `G`. On the machine this was written on, that last
/// one alone made `df -h` report `1860.7G` where every other `df` says `1.9T`.
fn human_size(bytes: u64) -> String {
    human_readable(
        bytes,
        Opts::AUTOSCALE | Opts::CEILING | Opts::SI | Opts::BASE_1024,
        1,
        1,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_no_args() {
        let a = parse_args(&s(&[]));
        assert!(!a.human);
        assert!(a.paths.is_empty());
    }

    #[test]
    fn parse_dash_h() {
        let a = parse_args(&s(&["-h"]));
        assert!(a.human);
        assert!(a.paths.is_empty());
    }

    #[test]
    fn parse_paths_only() {
        let a = parse_args(&s(&["/", "/home"]));
        assert!(!a.human);
        assert_eq!(a.paths, vec!["/", "/home"]);
    }

    #[test]
    fn parse_dash_h_with_path() {
        let a = parse_args(&s(&["-h", "/var"]));
        assert!(a.human);
        assert_eq!(a.paths, vec!["/var"]);
    }

    #[test]
    fn parse_dash_h_at_end() {
        let a = parse_args(&s(&["/var", "-h"]));
        assert!(a.human);
        assert_eq!(a.paths, vec!["/var"]);
    }

    #[test]
    fn human_size_bytes_under_kib() {
        // `0` and `1023`, not `0B` and `1023B`: `df` suffixes only a scaled
        // value. These assertions previously demanded the `B`, which no `df`
        // prints -- they described the old implementation, not the utility.
        assert_eq!(human_size(0), "0");
        assert_eq!(human_size(1023), "1023");
    }

    /// The four figures below are a real `df` reading from the machine this was
    /// written on, cross-checked against `df --block-size=1` on the same
    /// filesystems at the same moment. They pin the two properties a `{:.1}`
    /// format string cannot express: the rounding is upward, and the decimal
    /// disappears once the mantissa reaches ten.
    #[test]
    fn matches_a_real_df_reading() {
        assert_eq!(human_size(594_744_090_624), "554G"); // 553.9 GiB, rounded up
        assert_eq!(human_size(321_934_987_264), "300G"); // 299.8 GiB, rounded up
        assert_eq!(human_size(1_999_372_283_904), "1.9T"); // 1.818 TiB
        assert_eq!(human_size(1_657_357_066_240), "1.6T"); // 1.507 TiB
    }

    #[test]
    fn human_size_kib() {
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1536), "1.5K");
    }

    #[test]
    fn human_size_mib() {
        assert_eq!(human_size(1024 * 1024), "1.0M");
        assert_eq!(human_size(1024 * 1024 + 512 * 1024), "1.5M");
    }

    #[test]
    fn human_size_gib() {
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn format_row_blocks_mode() {
        let info = FsInfo {
            name: "/dev/sda1".to_string(),
            total_bytes: 1024 * 1024 * 100, // 100 MiB
            free_bytes: 1024 * 1024 * 30,
            avail_bytes: 1024 * 1024 * 20,
        };
        let row = format_row(&info, "/", false);
        // Used = 100M - 30M = 70M = 71680 KiB. Total in KiB = 102400.
        assert!(row.contains("/dev/sda1"));
        assert!(row.contains("102400"));
        assert!(row.contains("71680"));
        assert!(row.ends_with("/"));
    }

    #[test]
    fn format_row_human_mode() {
        let info = FsInfo {
            name: "/dev/sda1".to_string(),
            total_bytes: 1024 * 1024 * 1024 * 2, // 2 GiB
            free_bytes: 1024 * 1024 * 1024,
            avail_bytes: 1024 * 1024 * 1024,
        };
        let row = format_row(&info, "/home", true);
        assert!(row.contains("2.0G"));
        assert!(row.contains("1.0G"));
        assert!(row.ends_with("/home"));
    }

    #[test]
    fn format_row_zero_total_no_division_panic() {
        let info = FsInfo {
            name: "fs".to_string(),
            total_bytes: 0,
            free_bytes: 0,
            avail_bytes: 0,
        };
        let row = format_row(&info, "/", false);
        // pct should be 0.
        assert!(row.contains("  0%"));
    }

    #[test]
    fn format_row_percentage_calculation() {
        // total=100M, free=25M (so used=75M = 75%).
        let info = FsInfo {
            name: "fs".to_string(),
            total_bytes: 1024 * 1024 * 100,
            free_bytes: 1024 * 1024 * 25,
            avail_bytes: 1024 * 1024 * 25,
        };
        let row = format_row(&info, "/", false);
        assert!(row.contains(" 75%"));
    }

    #[test]
    fn format_row_full_filesystem() {
        let info = FsInfo {
            name: "fs".to_string(),
            total_bytes: 1024,
            free_bytes: 0,
            avail_bytes: 0,
        };
        let row = format_row(&info, "/", false);
        assert!(row.contains("100%"));
    }
}
