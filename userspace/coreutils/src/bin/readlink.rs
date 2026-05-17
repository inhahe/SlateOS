//! readlink — print resolved symbolic links or canonical file names.
//!
//! Usage: readlink [-f] FILE...
//!   Without -f: print the target of a symbolic link.
//!   With -f: canonicalize the entire path (resolve all symlinks,
//!            make absolute). Like `realpath`.

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut canonicalize = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in &args {
        if arg == "-f" || arg == "-e" || arg == "-m" {
            canonicalize = true;
        } else {
            files.push(arg);
        }
    }

    if files.is_empty() {
        eprintln!("readlink: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    for path_str in &files {
        if canonicalize {
            match fs::canonicalize(path_str) {
                Ok(p) => println!("{}", p.display()),
                Err(e) => {
                    eprintln!("readlink: {path_str}: {e}");
                    exit_code = 1;
                }
            }
        } else {
            match fs::read_link(path_str) {
                Ok(target) => println!("{}", target.display()),
                Err(e) => {
                    eprintln!("readlink: {path_str}: {e}");
                    exit_code = 1;
                }
            }
        }
    }

    process::exit(exit_code);
}
