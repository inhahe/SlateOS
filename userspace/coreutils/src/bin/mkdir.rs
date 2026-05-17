//! mkdir — make directories.
//!
//! Usage: mkdir [-p] DIRECTORY...
//!   -p  make parent directories as needed, no error if existing

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut parents = false;
    let mut dirs: Vec<String> = Vec::new();

    for arg in &args {
        if arg == "-p" {
            parents = true;
        } else if arg.starts_with('-') {
            eprintln!("mkdir: unknown option: {arg}");
            process::exit(1);
        } else {
            dirs.push(arg.clone());
        }
    }

    if dirs.is_empty() {
        eprintln!("mkdir: missing operand");
        process::exit(1);
    }

    let mut failed = false;
    for dir in &dirs {
        let result = if parents {
            fs::create_dir_all(dir)
        } else {
            fs::create_dir(dir)
        };
        if let Err(e) = result {
            eprintln!("mkdir: cannot create directory '{dir}': {e}");
            failed = true;
        }
    }

    if failed {
        process::exit(1);
    }
}
