//! realpath — print the resolved path.
//!
//! Usage: realpath FILE...
//!   Resolves all symlinks and relative components, prints the absolute path.

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("realpath: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    for path_str in &args {
        match fs::canonicalize(path_str) {
            Ok(p) => println!("{}", p.display()),
            Err(e) => {
                eprintln!("realpath: {path_str}: {e}");
                exit_code = 1;
            }
        }
    }

    process::exit(exit_code);
}
