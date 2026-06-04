//! df — report filesystem disk space usage.
//!
//! Usage: df [-h] [FILE...]
//!   -h  human-readable sizes (K, M, G)
//!   Without arguments: show all mounted filesystems.
//!   With FILE: show the filesystem containing FILE.

use std::env;
use std::io::{self, Write};
use std::process;

/// statvfs-like struct returned by our POSIX layer.
#[repr(C)]
struct Statvfs {
    f_bsize: u64,   // block size
    f_frsize: u64,  // fragment size
    f_blocks: u64,  // total blocks
    f_bfree: u64,   // free blocks
    f_bavail: u64,  // available blocks (non-root)
    f_files: u64,   // total inodes
    f_ffree: u64,   // free inodes
    f_favail: u64,  // available inodes
    f_fsid: u64,    // filesystem id
    f_flag: u64,    // mount flags
    f_namemax: u64, // max filename length
}

unsafe extern "C" {
    fn statvfs(path: *const u8, buf: *mut Statvfs) -> i32;
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut human = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg == "-h" {
            human = true;
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.is_empty() {
        paths.push("/".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(
        out,
        "{:<20} {:>10} {:>10} {:>10} {:>5} Mounted on",
        "Filesystem", "Size", "Used", "Avail", "Use%"
    );

    let mut exit_code = 0;
    for path in &paths {
        match stat_fs(path) {
            Ok(info) => {
                let total = info.total_bytes;
                let free = info.avail_bytes;
                let used = total.saturating_sub(info.free_bytes);
                let pct = if total > 0 {
                    (used * 100 / total) as u32
                } else {
                    0
                };

                if human {
                    let _ = writeln!(
                        out,
                        "{:<20} {:>10} {:>10} {:>10} {:>4}% {}",
                        info.name,
                        human_size(total),
                        human_size(used),
                        human_size(free),
                        pct,
                        path
                    );
                } else {
                    // Show in 1K blocks like traditional df
                    let _ = writeln!(
                        out,
                        "{:<20} {:>10} {:>10} {:>10} {:>4}% {}",
                        info.name,
                        total / 1024,
                        used / 1024,
                        free / 1024,
                        pct,
                        path
                    );
                }
            }
            Err(e) => {
                eprintln!("df: {path}: {e}");
                exit_code = 1;
            }
        }
    }

    process::exit(exit_code);
}

struct FsInfo {
    name: String,
    total_bytes: u64,
    free_bytes: u64,
    avail_bytes: u64,
}

fn stat_fs(path: &str) -> Result<FsInfo, String> {
    let mut c_path: Vec<u8> = path.as_bytes().to_vec();
    c_path.push(0);

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
        total_bytes: buf.f_blocks * bsize,
        free_bytes: buf.f_bfree * bsize,
        avail_bytes: buf.f_bavail * bsize,
    })
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}G", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}
