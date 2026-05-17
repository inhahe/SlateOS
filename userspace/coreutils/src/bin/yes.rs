//! yes — output a string repeatedly until killed.
//!
//! Usage: yes [STRING]
//!   Default STRING is "y".

use std::env;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let text = if args.is_empty() {
        "y".to_string()
    } else {
        args.join(" ")
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    // Use a large buffer for efficiency
    let line = format!("{text}\n");
    let bytes = line.as_bytes();
    loop {
        if out.write_all(bytes).is_err() {
            break; // pipe closed
        }
    }
}
