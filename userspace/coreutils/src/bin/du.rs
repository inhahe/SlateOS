//! du — estimate file space usage.
//!
//! Usage: du [-h] [-s] [-a] [FILE...]
//!   -h  human-readable sizes
//!   -s  show only total for each argument
//!   -a  show sizes for all files, not just directories
//!   Default: show each directory's total recursively.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut human = false;
    let mut summary = false;
    let mut show_all = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for c in arg[1..].chars() {
                match c {
                    'h' => human = true,
                    's' => summary = true,
                    'a' => show_all = true,
                    _ => {
                        eprintln!("du: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.is_empty() {
        paths.push(".".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut exit_code = 0;

    for path in &paths {
        let p = Path::new(path);
        match compute_du(p, summary, show_all, human, &mut out) {
            Ok(total) => {
                print_size(&mut out, total, path, human);
            }
            Err(e) => {
                eprintln!("du: {path}: {e}");
                exit_code = 1;
            }
        }
    }

    process::exit(exit_code);
}

fn compute_du(
    path: &Path,
    summary: bool,
    show_all: bool,
    human: bool,
    out: &mut impl Write,
) -> Result<u64, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{e}"))?;

    if meta.is_file() {
        let size = meta.blocks() * 512; // blocks are 512-byte units
        return Ok(size);
    }

    if !meta.is_dir() {
        return Ok(0);
    }

    let mut total: u64 = meta.blocks() * 512; // directory entry itself

    let entries = fs::read_dir(path).map_err(|e| format!("{e}"))?;
    for entry_result in entries {
        let entry = entry_result.map_err(|e| format!("{e}"))?;
        let entry_path = entry.path();
        let entry_meta = match fs::symlink_metadata(&entry_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("du: {}: {e}", entry_path.display());
                continue;
            }
        };

        if entry_meta.is_dir() {
            match compute_du(&entry_path, summary, show_all, human, out) {
                Ok(sub_total) => {
                    if !summary {
                        print_size(out, sub_total, &entry_path.display().to_string(), human);
                    }
                    total += sub_total;
                }
                Err(e) => {
                    eprintln!("du: {}: {e}", entry_path.display());
                }
            }
        } else {
            let size = entry_meta.blocks() * 512;
            total += size;
            if show_all && !summary {
                print_size(out, size, &entry_path.display().to_string(), human);
            }
        }
    }

    Ok(total)
}

fn print_size(out: &mut impl Write, bytes: u64, path: &str, human: bool) {
    if human {
        let _ = writeln!(out, "{}\t{}", human_size(bytes), path);
    } else {
        // Display in 1K blocks
        let _ = writeln!(out, "{}\t{}", bytes / 1024, path);
    }
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
