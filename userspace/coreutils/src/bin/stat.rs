//! stat — display file status.
//!
//! Usage: stat FILE...
//!   Shows file type, permissions, size, timestamps, etc.
//!
//! Built only on unix-family targets (our x86_64-slateos presents as
//! linux-musl, so `cfg(unix)` matches).  On non-unix hosts (e.g.
//! Windows when running `cargo test --workspace`), a stub `main` keeps
//! the workspace compile-clean.  The pure formatting helpers
//! (`format_mode`, `format_timestamp`, `is_leap`) live outside the cfg
//! gate so they're testable on the developer host.

#![cfg_attr(not(unix), allow(dead_code))]

/// Format a POSIX mode word as a 10-character permission string like
/// `-rwxr-xr--`.  Handles setuid/setgid/sticky in the standard way.
fn format_mode(mode: u32) -> String {
    let file_type = match mode & 0o170000 {
        0o140000 => 's',
        0o120000 => 'l',
        0o100000 => '-',
        0o060000 => 'b',
        0o040000 => 'd',
        0o020000 => 'c',
        0o010000 => 'p',
        _ => '?',
    };

    let mut s = String::with_capacity(10);
    s.push(file_type);

    let perms = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];

    for &(bit, ch) in &perms {
        if mode & bit != 0 {
            s.push(ch);
        } else {
            s.push('-');
        }
    }

    // SAFETY: every char pushed above is a 1-byte ASCII char, so the buffer
    // is plain ASCII and remains valid UTF-8 after these byte swaps.
    let bytes = unsafe { s.as_bytes_mut() };
    if mode & 0o4000 != 0
        && let Some(b) = bytes.get_mut(3)
    {
        *b = if *b == b'x' { b's' } else { b'S' };
    }
    if mode & 0o2000 != 0
        && let Some(b) = bytes.get_mut(6)
    {
        *b = if *b == b'x' { b's' } else { b'S' };
    }
    if mode & 0o1000 != 0
        && let Some(b) = bytes.get_mut(9)
    {
        *b = if *b == b'x' { b't' } else { b'T' };
    }

    s
}

/// Format a Unix epoch-seconds timestamp as `YYYY-MM-DD HH:MM:SS` (UTC).
fn format_timestamp(epoch_secs: i64) -> String {
    if epoch_secs <= 0 {
        return "0".to_string();
    }
    let secs = epoch_secs as u64;

    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let mut days = secs / 86400;

    let mut year: u64 = 1970;
    loop {
        let diy = if is_leap(year) { 366 } else { 365 };
        if days < diy {
            break;
        }
        days = days.saturating_sub(diy);
        year = year.saturating_add(1);
    }

    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month: u64 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days = days.saturating_sub(md);
        month = month.saturating_add(1);
    }
    let day = days.saturating_add(1);

    format!("{year}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}")
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

#[cfg(not(unix))]
fn main() {
    eprintln!("stat: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::process;

#[cfg(unix)]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("stat: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    for (i, path_str) in args.iter().enumerate() {
        if i > 0 {
            println!();
        }
        match show_stat(path_str) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("stat: cannot stat '{path_str}': {e}");
                exit_code = 1;
            }
        }
    }

    process::exit(exit_code);
}

#[cfg(unix)]
fn show_stat(path: &str) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{e}"))?;

    let ft = meta.file_type();
    let type_str = if ft.is_file() {
        "regular file"
    } else if ft.is_dir() {
        "directory"
    } else if ft.is_symlink() {
        "symbolic link"
    } else if ft.is_block_device() {
        "block device"
    } else if ft.is_char_device() {
        "character device"
    } else if ft.is_fifo() {
        "fifo"
    } else if ft.is_socket() {
        "socket"
    } else {
        "unknown"
    };

    let mode = meta.permissions().mode();
    let mode_str = format_mode(mode);

    println!("  File: {path}");
    println!(
        "  Size: {:<15} Blocks: {:<10} IO Block: {} {}",
        meta.len(),
        meta.blocks(),
        meta.blksize(),
        type_str
    );
    println!(
        "Device: {:x}h/{}d\tInode: {:<10} Links: {}",
        meta.dev(),
        meta.dev(),
        meta.ino(),
        meta.nlink()
    );
    println!(
        "Access: ({:04o}/{})  Uid: {:>5}  Gid: {:>5}",
        mode & 0o7777,
        mode_str,
        meta.uid(),
        meta.gid()
    );

    println!("Access: {}", format_timestamp(meta.atime()));
    println!("Modify: {}", format_timestamp(meta.mtime()));
    println!("Change: {}", format_timestamp(meta.ctime()));

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn mode_regular_file_rwxr_xr_x() {
        // 0o100755 -> regular file, rwxr-xr-x.
        assert_eq!(format_mode(0o100755), "-rwxr-xr-x");
    }

    #[test]
    fn mode_directory_drwxr_xr_x() {
        assert_eq!(format_mode(0o040755), "drwxr-xr-x");
    }

    #[test]
    fn mode_symlink_lrwxrwxrwx() {
        assert_eq!(format_mode(0o120777), "lrwxrwxrwx");
    }

    #[test]
    fn mode_fifo_p() {
        assert_eq!(format_mode(0o010644), "prw-r--r--");
    }

    #[test]
    fn mode_block_device_b() {
        assert_eq!(format_mode(0o060660), "brw-rw----");
    }

    #[test]
    fn mode_char_device_c() {
        assert_eq!(format_mode(0o020660), "crw-rw----");
    }

    #[test]
    fn mode_socket_s() {
        assert_eq!(format_mode(0o140755), "srwxr-xr-x");
    }

    #[test]
    fn mode_unknown_type_questionmark() {
        // 0o030000 is not one of the supported types.
        assert_eq!(format_mode(0o030755), "?rwxr-xr-x");
    }

    #[test]
    fn mode_no_perms_all_dashes() {
        assert_eq!(format_mode(0o100000), "----------");
    }

    #[test]
    fn mode_owner_only_rwx() {
        assert_eq!(format_mode(0o100700), "-rwx------");
    }

    #[test]
    fn mode_setuid_lowercase_s_when_x_set() {
        // setuid on a file that's also user-executable -> 's' in user-x slot.
        assert_eq!(format_mode(0o104755), "-rwsr-xr-x");
    }

    #[test]
    fn mode_setuid_uppercase_s_when_x_clear() {
        // setuid without user execute -> 'S' in user-x slot.
        assert_eq!(format_mode(0o104644), "-rwSr--r--");
    }

    #[test]
    fn mode_setgid_lowercase_s_when_x_set() {
        assert_eq!(format_mode(0o102755), "-rwxr-sr-x");
    }

    #[test]
    fn mode_setgid_uppercase_s_when_x_clear() {
        assert_eq!(format_mode(0o102644), "-rw-r-Sr--");
    }

    #[test]
    fn mode_sticky_lowercase_t_when_x_set() {
        assert_eq!(format_mode(0o041777), "drwxrwxrwt");
    }

    #[test]
    fn mode_sticky_uppercase_t_when_x_clear() {
        assert_eq!(format_mode(0o041776), "drwxrwxrwT");
    }

    #[test]
    fn ts_zero_returns_zero() {
        assert_eq!(format_timestamp(0), "0");
    }

    #[test]
    fn ts_negative_returns_zero() {
        assert_eq!(format_timestamp(-1), "0");
    }

    #[test]
    fn ts_epoch_plus_one_second() {
        // 1 second past epoch.
        assert_eq!(format_timestamp(1), "1970-01-01 00:00:01");
    }

    #[test]
    fn ts_known_2024_date() {
        // 2024-06-15 12:30:45 UTC = 1718454645.
        assert_eq!(format_timestamp(1_718_454_645), "2024-06-15 12:30:45");
    }

    #[test]
    fn ts_leap_day_2000() {
        // 2000-02-29 00:00:00 UTC = 951782400.
        assert_eq!(format_timestamp(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn leap_basics() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
        assert!(!is_leap(2100));
    }
}
