//! mv -- move or rename files and directories.
//!
//! Usage: mv [-f] SOURCE... DEST
//!   -f  force: do not report errors when overwriting existing files

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut force = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'f' => force = true,
                    _ => {
                        eprintln!("mv: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.len() < 2 {
        eprintln!("mv: missing operand");
        process::exit(1);
    }

    let dest = paths.last().cloned().unwrap_or_default();
    let sources = &paths[..paths.len() - 1];
    let dest_is_dir = Path::new(&dest).is_dir();

    if sources.len() > 1 && !dest_is_dir {
        eprintln!("mv: target '{dest}' is not a directory");
        process::exit(1);
    }

    let mut failed = false;
    for src_str in sources {
        let src = Path::new(src_str);
        let target = if dest_is_dir {
            let name = src.file_name().unwrap_or_default();
            PathBuf::from(&dest).join(name)
        } else {
            PathBuf::from(&dest)
        };

        if target.exists() && !force {
            // Overwriting is allowed by default in standard mv; -f just suppresses
            // any hypothetical interactive prompt. We proceed with the rename.
        }

        if let Err(e) = fs::rename(src, &target) {
            // rename() can fail across filesystems; fall back to copy + remove.
            if src.is_dir() {
                eprintln!("mv: cannot move '{src_str}' to '{}': {e}", target.display());
                failed = true;
            } else {
                match fs::copy(src, &target).and_then(|_| fs::remove_file(src)) {
                    Ok(()) => {}
                    Err(e2) => {
                        if !force {
                            eprintln!("mv: cannot move '{src_str}' to '{}': {e2}", target.display());
                        }
                        failed = true;
                    }
                }
            }
        }
    }

    if failed {
        process::exit(1);
    }
}
