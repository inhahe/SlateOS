//! rm — remove files or directories.
//!
//! Usage: rm [-r] [-f] FILE...
//!   -r  remove directories and their contents recursively
//!   -f  ignore nonexistent files, never prompt

use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut recursive = false;
    let mut force = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    'f' => force = true,
                    _ => {
                        eprintln!("rm: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.is_empty() {
        if !force {
            eprintln!("rm: missing operand");
            process::exit(1);
        }
        return;
    }

    let mut failed = false;
    for path_str in &paths {
        let path = Path::new(path_str);
        if !path.exists() {
            if !force {
                eprintln!("rm: cannot remove '{path_str}': No such file or directory");
                failed = true;
            }
            continue;
        }

        let result = if path.is_dir() {
            if recursive {
                fs::remove_dir_all(path)
            } else {
                eprintln!("rm: cannot remove '{path_str}': Is a directory");
                failed = true;
                continue;
            }
        } else {
            fs::remove_file(path)
        };

        if let Err(e) = result {
            if !force {
                eprintln!("rm: cannot remove '{path_str}': {e}");
                failed = true;
            }
        }
    }

    if failed {
        process::exit(1);
    }
}
