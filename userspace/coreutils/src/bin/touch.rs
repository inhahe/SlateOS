//! touch -- create files or update their timestamps.
//!
//! Usage: touch FILE...
//!   Creates each FILE if it does not exist.
//!   Updates the modification timestamp if it does exist.

use std::env;
use std::fs::OpenOptions;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("touch: missing operand");
        process::exit(1);
    }

    let mut failed = false;
    for path in &args {
        // Open with create + write to create if missing.
        // Opening with append avoids truncating existing files.
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
                // Update modification time by setting file length to its current length.
                // This is a portable way to bump the timestamp without external crates.
                if let Ok(meta) = file.metadata() {
                    let len = meta.len();
                    if let Err(e) = file.set_len(len) {
                        eprintln!("touch: cannot update timestamp for '{path}': {e}");
                        failed = true;
                    }
                }
            }
            Err(e) => {
                eprintln!("touch: cannot touch '{path}': {e}");
                failed = true;
            }
        }
    }

    if failed {
        process::exit(1);
    }
}
