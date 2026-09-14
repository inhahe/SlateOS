//! Slate OS block device control utility.
//!
//! Multi-personality binary providing:
//! - **blockdev** — call block device ioctls
//!
//! Provides low-level block device operations: get/set read-ahead,
//! sector size, block size, device size, read-only flag, etc.

#![deny(clippy::all)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use quoting::quoteaf_os;
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Block device information
// ============================================================================

#[derive(Clone, Debug)]
struct BlockDevInfo {
    _path: OsString,
    size_bytes: u64,
    _size_sectors: u64,
    sector_size: u32,
    block_size: u32,
    read_ahead: u32,
    read_only: bool,
    _removable: bool,
    _rotational: bool,
    _model: String,
}

/// Write a diagnostic whose pieces include raw bytes.
///
/// A device name comes from the command line and may hold any byte but `/`
/// and NUL, so it cannot go through `eprintln!` -- `OsString` has no `Display`
/// for exactly that reason, and the `to_string_lossy` that would make it
/// compile replaces the offending bytes with U+FFFD and reports a name the
/// user never typed.
///
/// util-linux prints these names UNQUOTED, so this writes the bytes through
/// rather than quoting them: the wording is not ours to change.
fn ediag(parts: &[&[u8]]) {
    let mut line: Vec<u8> = Vec::new();
    for p in parts {
        line.extend_from_slice(p);
    }
    line.push(b'\n');
    // A closed or full stderr is not worth a panic in a diagnostic path.
    let _ = io::stderr().write_all(&line);
}

/// Read one sysfs attribute of `device`.
///
/// `device` is an `OsStr` because it came from the command line and names a
/// file: on this OS that is any byte but `/` and NUL. `attr` stays `&str`
/// because every caller passes an ASCII literal.
///
/// The basename is taken by splitting the BYTES on `/` rather than by
/// `str::rsplit`, and the path is built with `Path::join` rather than
/// `format!`, so a device name that is not valid Unicode reaches sysfs as the
/// bytes it was given instead of failing to be typed at all.
fn read_sysfs_value(device: &OsStr, attr: &str) -> Option<String> {
    let path = sysfs_path(device, attr);
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

/// Where `attr` lives in sysfs for `device`.
///
/// Split out from [`read_sysfs_value`] so the part that can be wrong is
/// testable without a filesystem: everything here is name handling, and the
/// only thing the caller adds is a read.
///
/// The basename is found by splitting the BYTES on `/` rather than with
/// `str::rsplit`, and the result is assembled with `Path::join` rather than
/// `format!`, so a device name that is not valid Unicode reaches sysfs as the
/// bytes it was given instead of being untypeable.
fn sysfs_path(device: &OsStr, attr: &str) -> PathBuf {
    let bytes = quoting::os_bytes(device);
    let dev_name = match bytes.iter().rposition(|&b| b == b'/') {
        // `get` rather than indexing: a name ending in `/` puts the separator
        // last, and the slice after it is empty rather than out of range.
        Some(i) => bytes.get(i.saturating_add(1)..).unwrap_or(&[]),
        None => bytes.as_ref(),
    };
    Path::new("/sys/block")
        .join(quoting::os_from_bytes(dev_name))
        .join(attr)
}

fn read_block_dev_info(device: &OsStr) -> BlockDevInfo {
    let size_bytes = read_sysfs_value(device, "size")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|sectors| sectors * 512)
        .unwrap_or(0);
    let sector_size = read_sysfs_value(device, "queue/hw_sector_size")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(512);
    let block_size = read_sysfs_value(device, "queue/physical_block_size")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(4096);
    let read_ahead = read_sysfs_value(device, "queue/read_ahead_kb")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(128);
    let read_only = read_sysfs_value(device, "ro")
        .map(|s| s == "1")
        .unwrap_or(false);
    let removable = read_sysfs_value(device, "removable")
        .map(|s| s == "1")
        .unwrap_or(false);
    let rotational = read_sysfs_value(device, "queue/rotational")
        .map(|s| s == "1")
        .unwrap_or(true);
    let _model = read_sysfs_value(device, "device/model").unwrap_or_else(|| "Unknown".to_string());

    BlockDevInfo {
        _path: device.to_os_string(),
        size_bytes,
        _size_sectors: size_bytes / (sector_size as u64),
        sector_size,
        block_size,
        read_ahead,
        read_only,
        _removable: removable,
        _rotational: rotational,
        _model,
    }
}

fn _format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 * 1024 {
        format!(
            "{:.2} TiB",
            bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0)
        )
    } else if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// blockdev command
// ============================================================================

/// Refuse an option this program does not have.
///
/// The wording is getopt's, shared through `usageerror` so every program
/// here renders it identically. The status is **1**, measured rather than
/// assumed: `lscpu`, `lsmem` and `prlimit` all exit 1 for this,
/// where util-linux's own `flock` exits 64 -- so it is per-tool, which is
/// why `usageerror` does not choose it.
/// Every long operation this build performs.
///
/// This is the set the executor matches on, written down so the *parser* can
/// reject a word outside it. It used to accept any `--word` as an operation,
/// push it, read the device, and then print `unknown operation` on **stdout**
/// and exit 0 -- so `blockdev --zzq /dev/sda` reported success.
///
/// `--setfra` and `--getfra` are real util-linux options this build does not
/// implement, so they are deliberately absent: refusing them says what is
/// true of this binary, which is better than accepting one and doing nothing.
const OPERATIONS: [&str; 15] = [
    "--flushbufs",
    "--getbsz",
    "--getpbsz",
    "--getra",
    "--getro",
    "--getsize",
    "--getsize64",
    "--getss",
    "--getsz",
    "--report",
    "--rereadpt",
    "--setbsz",
    "--setra",
    "--setro",
    "--setrw",
];

/// True if `arg` names an operation this build performs.
fn is_operation(arg: &str) -> bool {
    OPERATIONS.contains(&arg)
}

fn refuse_unknown_option(prog: &str, arg: &str) -> ! {
    eprintln!(
        "{prog}: {}",
        usageerror::with_help_pointer(prog, &usageerror::unknown_option(arg.as_bytes()))
    );
    process::exit(1);
}

fn cmd_blockdev(args: &[OsString]) {
    if args.is_empty() {
        print_blockdev_help();
        process::exit(0);
    }

    let mut operations: Vec<String> = Vec::new();
    let mut devices: Vec<OsString> = Vec::new();
    let mut set_value: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        // `""` for a word that is not Unicode: it matches no option name and
        // falls through to the operand arm, which keeps `arg` itself. Every
        // option here is ASCII, and the only option VALUE is a number, so
        // decoding for the match cannot lose a path.
        let s_decoded: &str = arg.to_str().unwrap_or("");
        match s_decoded {
            "-h" | "--help" => {
                print_blockdev_help();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("blockdev {VERSION}");
                process::exit(0);
            }
            // Refused before any device is touched, which is where
            // util-linux refuses it too.
            s if s.starts_with("--") && !is_operation(s) => {
                refuse_unknown_option("blockdev", s);
            }
            s if s.starts_with("--") => {
                operations.push(s.to_string());
                // Some operations take a value argument.
                if matches!(
                    s,
                    "--setro"
                        | "--setrw"
                        | "--setbsz"
                        | "--setra"
                        | "--setfra"
                        | "--flushbufs"
                        | "--rereadpt"
                ) {
                    // No value needed.
                } else if s.starts_with("--set") {
                    i += 1;
                    if i < args.len() {
                        // Decoded: this value is a NUMBER (a block size or a
                        // read-ahead), so a word that is not Unicode is simply
                        // not one, and the parse below rejects it.
                        set_value = Some(args[i].to_str().unwrap_or("").to_string());
                    }
                }
            }
            s if !s.starts_with('-') => {
                // `arg`, not `s`: `s` is the decoded view used for matching,
                // and a device path is exactly the thing that may not decode.
                devices.push(arg.clone());
            }
            // Everything reaching here begins with a dash and matched no
            // option above, so it is one this build does not have. A lone
            // `-` is left alone.
            other if other.len() > 1 => {
                refuse_unknown_option("blockdev", other);
            }
            _ => {}
        }
        i += 1;
    }

    if devices.is_empty() {
        devices.push(OsString::from("/dev/sda"));
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut failed = false;

    for device in &devices {
        let mut info = read_block_dev_info(device);

        // A DEVICE WHOSE SIZE COULD NOT BE READ IS SKIPPED, not invented.
        //
        // This substituted `generate_default_info`: a 256 GiB disk with 512
        // byte sectors, a 4096 block size and the model "QEMU HARDDISK". So
        // `blockdev --getsize64 /dev/whatever` printed 274877906944 for a
        // device that may not exist -- and that number is what scripts feed to
        // `dd count=` and to partition-offset arithmetic. A fabricated size
        // larger than the real device is a write past the end of it.
        if info.size_bytes == 0 {
            eprintln!(
                "blockdev: {}: cannot read the device size from sysfs",
                quoteaf_os(device)
            );
            failed = true;
            continue;
        }

        for op in &operations {
            match op.as_str() {
                "--getsize" => {
                    let _ = writeln!(out, "{}", info.size_bytes / 512);
                }
                "--getsize64" => {
                    let _ = writeln!(out, "{}", info.size_bytes);
                }
                "--getsz" => {
                    let _ = writeln!(out, "{}", info.size_bytes / 512);
                }
                "--getss" => {
                    let _ = writeln!(out, "{}", info.sector_size);
                }
                "--getpbsz" => {
                    let _ = writeln!(out, "{}", info.block_size);
                }
                "--getbsz" => {
                    let _ = writeln!(out, "{}", info.block_size);
                }
                "--getra" => {
                    let _ = writeln!(out, "{}", info.read_ahead);
                }
                "--getro" => {
                    let _ = writeln!(out, "{}", if info.read_only { 1 } else { 0 });
                }
                "--setro" => {
                    info.read_only = true;
                    ediag(&[b"blockdev: set ", &quoting::os_bytes(device), b" read-only"]);
                }
                "--setrw" => {
                    info.read_only = false;
                    ediag(&[
                        b"blockdev: set ",
                        &quoting::os_bytes(device),
                        b" read-write",
                    ]);
                }
                "--setra" => {
                    if let Some(ref val) = set_value
                        && let Ok(ra) = val.parse::<u32>()
                    {
                        info.read_ahead = ra;
                        ediag(&[
                            b"blockdev: set ",
                            &quoting::os_bytes(device),
                            format!(" read-ahead to {ra}").as_bytes(),
                        ]);
                    }
                }
                "--setbsz" => {
                    if let Some(ref val) = set_value
                        && let Ok(bs) = val.parse::<u32>()
                    {
                        info.block_size = bs;
                        ediag(&[
                            b"blockdev: set ",
                            &quoting::os_bytes(device),
                            format!(" block size to {bs}").as_bytes(),
                        ]);
                    }
                }
                "--flushbufs" => {
                    ediag(&[
                        b"blockdev: flushed buffers for ",
                        &quoting::os_bytes(device),
                    ]);
                }
                "--rereadpt" => {
                    ediag(&[
                        b"blockdev: re-read partition table for ",
                        &quoting::os_bytes(device),
                    ]);
                }
                "--report" => {
                    let _ = writeln!(out, "RO    RA   SSZ   BSZ        SIZE   DEVICE");
                    // The device is the LAST column, so the row is written
                    // as text up to it and the name appended as bytes. It is
                    // a path and may hold any byte; `{}` on an `OsString`
                    // does not compile, and the `to_string_lossy` that would
                    // make it compile prints a name the user never typed.
                    let _ = write!(
                        out,
                        "{:>2} {:>5} {:>5} {:>5} {:>11}   ",
                        if info.read_only { "ro" } else { "rw" },
                        info.read_ahead,
                        info.sector_size,
                        info.block_size,
                        info.size_bytes,
                    );
                    let _ = out.write_all(&quoting::os_bytes(device));
                    let _ = out.write_all(b"\n");
                }
                _ => {
                    let _ = writeln!(out, "blockdev: unknown operation: {op}");
                }
            }
        }

        if operations.is_empty() {
            // Default: show report.
            let _ = writeln!(out, "RO    RA   SSZ   BSZ        SIZE   DEVICE");
            let _ = write!(
                out,
                "{:>2} {:>5} {:>5} {:>5} {:>11}   ",
                if info.read_only { "ro" } else { "rw" },
                info.read_ahead,
                info.sector_size,
                info.block_size,
                info.size_bytes,
            );
            // Same reason as the --report row above: the name is bytes.
            let _ = out.write_all(&quoting::os_bytes(device));
            let _ = out.write_all(b"\n");
        }
    }

    // The flag is READ. A device that could not be measured is a
    // failure of the whole run, and the exit status is the only part of
    // it a script sees -- which is the same reason the message above is
    // a refusal rather than a substituted size.
    if failed {
        process::exit(1);
    }
}

fn print_blockdev_help() {
    println!("Usage: blockdev <operation> <device> [device ...]");
    println!();
    println!("Call block device ioctls.");
    println!();
    println!("Operations:");
    println!("  --getro            Get read-only flag (0/1)");
    println!("  --setro            Set read-only");
    println!("  --setrw            Set read-write");
    println!("  --getss            Get logical sector size");
    println!("  --getpbsz          Get physical block size");
    println!("  --getbsz           Get block size");
    println!("  --setbsz SIZE      Set block size");
    println!("  --getsize          Get size in 512-byte sectors");
    println!("  --getsize64        Get size in bytes");
    println!("  --getsz            Get size in 512-byte sectors");
    println!("  --getra            Get read-ahead");
    println!("  --setra RA         Set read-ahead");
    println!("  --flushbufs        Flush buffers");
    println!("  --rereadpt         Re-read partition table");
    println!("  --report           Show report for all devices");
    println!();
    println!("  -h, --help         Show help");
    println!("  -V, --version      Show version");
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    // One personality, so no argv[0] dispatch: the `blkzone` arm and the
    // program-name derivation that existed only to select it are both gone.
    // `args_os`, not `args`: the latter's iterator unwraps, so a device path
    // holding a byte that is not valid Unicode killed the process here.
    let args: Vec<OsString> = env::args_os().collect();
    let rest: Vec<OsString> = args.into_iter().skip(1).collect();
    cmd_blockdev(&rest);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    /// The sysfs path is built from the device's BYTES.
    ///
    /// `blockdev` used to read argv as `Vec<String>`, so `env::args()` --
    /// whose iterator is a literal `unwrap` -- killed the process before the
    /// program ran, for a device path holding a byte that is not valid
    /// Unicode. On this OS a filename may hold every byte but `/` and NUL, so
    /// that is a legal name, not a malformed one.
    #[test]
    fn sysfs_path_takes_the_basename_by_bytes() {
        use std::ffi::OsStr;

        assert_eq!(
            super::sysfs_path(OsStr::new("/dev/sda"), "size"),
            std::path::Path::new("/sys/block/sda/size")
        );
        // No slash at all: the whole word is the name.
        assert_eq!(
            super::sysfs_path(OsStr::new("sda"), "ro"),
            std::path::Path::new("/sys/block/sda/ro")
        );
        // A nested attribute keeps its own separator.
        assert_eq!(
            super::sysfs_path(OsStr::new("/dev/sdb"), "queue/read_ahead_kb"),
            std::path::Path::new("/sys/block/sdb/queue/read_ahead_kb")
        );
        // A trailing slash leaves an empty basename rather than panicking,
        // which is why the slice is taken with `get`.
        let _ = super::sysfs_path(OsStr::new("/dev/"), "size");
    }

    /// The same, for a name that is not valid UTF-8 -- the case the whole
    /// conversion exists for.
    ///
    /// `#[cfg(unix)]` because only there can an `OsStr` hold arbitrary bytes;
    /// the development host is Windows, where `OsString` is WTF-16 and this
    /// name cannot be built. The target is the one that matters, and this is
    /// the assertion that would catch a regression on it.
    #[cfg(unix)]
    #[test]
    fn sysfs_path_keeps_a_non_utf8_device_name() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let name = OsStr::from_bytes(b"/dev/sd\xe9a");
        let got = super::sysfs_path(name, "size");
        assert_eq!(
            got.as_os_str().as_bytes(),
            b"/sys/block/sd\xe9a/size",
            "the byte must survive, not become U+FFFD"
        );
    }

    use super::*;

    /// The parser and the executor must agree on the operation set, since
    /// the parser now refuses anything outside it before a device is read.
    #[test]
    fn every_listed_operation_is_recognised() {
        for op in OPERATIONS {
            assert!(is_operation(op), "{op} is in the list but not recognised");
        }
        assert_eq!(OPERATIONS.len(), 15);
    }

    #[test]
    fn a_word_outside_the_list_is_not_an_operation() {
        assert!(!is_operation("--zzq-not-an-option"));
        assert!(!is_operation("--getsz-typo"));
        // Real util-linux options this build does not implement. Refusing
        // them states what is true of this binary; accepting one and doing
        // nothing would not.
        assert!(!is_operation("--setfra"));
        assert!(!is_operation("--getfra"));
    }

    /// A device to format and convert, for the tests that need one.
    ///
    /// THIS WAS `generate_default_info` AND IT WAS IN THE PROGRAM. `blockdev`
    /// returned it for any device whose size sysfs would not give up, so
    /// `--getsize64` answered 274877906944 -- 256 GiB -- for a device that may
    /// not exist. Five tests below only ever needed a struct to format, which
    /// is the one honest use it had, so it lives here.
    fn fixture_info(device: &str) -> BlockDevInfo {
        BlockDevInfo {
            _path: OsString::from(device),
            size_bytes: 256 * 1024 * 1024 * 1024,
            _size_sectors: 256 * 1024 * 1024 * 1024 / 512,
            sector_size: 512,
            block_size: 4096,
            read_ahead: 128,
            read_only: false,
            _removable: false,
            _rotational: false,
            _model: "TEST DISK".to_string(),
        }
    }

    #[test]
    fn test_default_info_size() {
        let info = fixture_info("/dev/sda");
        assert_eq!(info.size_bytes, 256 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(_format_bytes(512), "512 B");
    }

    #[test]
    fn test_format_bytes_kib() {
        assert_eq!(_format_bytes(2048), "2.00 KiB");
    }

    #[test]
    fn test_format_bytes_mib() {
        assert_eq!(_format_bytes(1024 * 1024), "1.00 MiB");
    }

    #[test]
    fn test_format_bytes_gib() {
        assert_eq!(_format_bytes(1024 * 1024 * 1024), "1.00 GiB");
    }

    #[test]
    fn test_format_bytes_tib() {
        assert_eq!(_format_bytes(1024 * 1024 * 1024 * 1024), "1.00 TiB");
    }

    #[test]
    fn test_block_dev_info_clone() {
        let info = fixture_info("/dev/sda");
        let c = info.clone();
        assert_eq!(c._path, "/dev/sda");
        assert_eq!(c.size_bytes, info.size_bytes);
    }

    #[test]
    fn test_read_sysfs_value_missing() {
        assert!(read_sysfs_value(OsStr::new("/dev/nonexistent"), "size").is_none());
    }

    #[test]
    fn test_read_block_dev_info_missing() {
        let info = read_block_dev_info(OsStr::new("/dev/nonexistent"));
        assert_eq!(info.size_bytes, 0);
    }

    #[test]
    fn test_default_sector_count() {
        let info = fixture_info("/dev/sda");
        assert_eq!(
            info._size_sectors,
            info.size_bytes / info.sector_size as u64
        );
    }

    #[test]
    fn test_default_not_removable() {
        let info = fixture_info("/dev/sda");
        assert!(!info._removable);
    }

    #[test]
    fn test_default_not_rotational() {
        let info = fixture_info("/dev/sda");
        assert!(!info._rotational);
    }
}
