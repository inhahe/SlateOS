//! strings — find printable strings in binary files.
//!
//! Usage: strings [-n MIN] [FILE...]
//!   -n MIN   minimum string length (default: 4)
//!   Prints all sequences of printable characters of length >= MIN.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut min_len: usize = 4;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            min_len = args[i + 1].parse().unwrap_or(4);
            i += 2;
        } else if args[i].starts_with("-") && args[i].len() > 1 {
            // Also accept -N as shorthand
            if let Ok(n) = args[i][1..].parse::<usize>() {
                min_len = n;
            }
            i += 1;
        } else {
            files.push(args[i].clone());
            i += 1;
        }
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &files {
        let mut reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("strings: {path}: {e}");
                    continue;
                }
            }
        };

        let mut data = Vec::new();
        if reader.read_to_end(&mut data).is_err() {
            eprintln!("strings: {path}: read error");
            continue;
        }

        let mut current = Vec::new();
        for &b in &data {
            if is_printable(b) {
                current.push(b);
            } else {
                if current.len() >= min_len {
                    let _ = out.write_all(&current);
                    let _ = writeln!(out);
                }
                current.clear();
            }
        }
        // Flush any remaining string
        if current.len() >= min_len {
            let _ = out.write_all(&current);
            let _ = writeln!(out);
        }
    }
}

fn is_printable(b: u8) -> bool {
    // Printable ASCII (space through tilde) plus tab
    (0x20..=0x7e).contains(&b) || b == b'\t'
}
