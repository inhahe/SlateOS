//! rmdir — remove empty directories.
//!
//! Usage: rmdir [-p] DIRECTORY...
//!   -p  remove parent directories as well if they become empty

use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut parents = false;
    let mut dirs: Vec<String> = Vec::new();

    for arg in &args {
        if arg == "-p" {
            parents = true;
        } else if arg.starts_with('-') && arg.len() > 1 {
            eprintln!("rmdir: unknown option: {arg}");
            process::exit(1);
        } else {
            dirs.push(arg.clone());
        }
    }

    if dirs.is_empty() {
        eprintln!("rmdir: missing operand");
        process::exit(1);
    }

    let mut failed = false;
    for dir in &dirs {
        if let Err(e) = fs::remove_dir(dir) {
            eprintln!("rmdir: failed to remove '{dir}': {e}");
            failed = true;
            continue;
        }
        if parents {
            let mut p = Path::new(dir).parent();
            while let Some(parent) = p {
                if parent.as_os_str().is_empty() {
                    break;
                }
                if fs::remove_dir(parent).is_err() {
                    break; // stop when a directory isn't empty
                }
                p = parent.parent();
            }
        }
    }

    if failed {
        process::exit(1);
    }
}
