//! stat — display file status.
//!
//! Usage: stat FILE...
//!   Shows file type, permissions, size, timestamps, etc.

use std::env;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::process;

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

    // Timestamps
    let atime = meta.atime();
    let mtime = meta.mtime();
    let ctime = meta.ctime();
    println!("Access: {}", format_timestamp(atime));
    println!("Modify: {}", format_timestamp(mtime));
    println!("Change: {}", format_timestamp(ctime));

    Ok(())
}

fn format_mode(mode: u32) -> String {
    let file_type = match mode & 0o170000 {
        0o140000 => 's', // socket
        0o120000 => 'l', // symlink
        0o100000 => '-', // regular
        0o060000 => 'b', // block
        0o040000 => 'd', // directory
        0o020000 => 'c', // char
        0o010000 => 'p', // fifo
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

    // Handle setuid/setgid/sticky
    let bytes = unsafe { s.as_bytes_mut() };
    if mode & 0o4000 != 0 {
        bytes[3] = if bytes[3] == b'x' { b's' } else { b'S' };
    }
    if mode & 0o2000 != 0 {
        bytes[6] = if bytes[6] == b'x' { b's' } else { b'S' };
    }
    if mode & 0o1000 != 0 {
        bytes[9] = if bytes[9] == b'x' { b't' } else { b'T' };
    }

    s
}

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
        days -= diy;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;

    format!("{year}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}")
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
