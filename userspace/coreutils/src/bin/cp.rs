//! cp — copy files and directories.
//!
//! Usage: cp [-r] SOURCE DEST
//!        cp [-r] SOURCE... DIRECTORY
//!   -r  copy directories recursively

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut recursive = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    _ => {
                        eprintln!("cp: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.len() < 2 {
        eprintln!("cp: missing operand");
        process::exit(1);
    }

    let dest = paths.last().cloned().unwrap_or_default();
    let sources = &paths[..paths.len() - 1];
    let dest_is_dir = Path::new(&dest).is_dir();

    if sources.len() > 1 && !dest_is_dir {
        eprintln!("cp: target '{dest}' is not a directory");
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

        if src.is_dir() {
            if !recursive {
                eprintln!("cp: omitting directory '{src_str}'");
                failed = true;
                continue;
            }
            if let Err(e) = copy_dir_recursive(src, &target) {
                eprintln!("cp: error copying '{src_str}': {e}");
                failed = true;
            }
        } else {
            if let Err(e) = fs::copy(src, &target) {
                eprintln!("cp: error copying '{src_str}' to '{}': {e}",
                         target.display());
                failed = true;
            }
        }
    }

    if failed {
        process::exit(1);
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}
