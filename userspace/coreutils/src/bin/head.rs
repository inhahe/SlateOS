//! head — output the first part of files.
//!
//! Usage: head [-n COUNT] [FILE...]
//!   -n COUNT  print first COUNT lines (default 10)
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut count: usize = 10;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            count = args[i + 1].parse().unwrap_or(10);
            i += 2;
        } else if args[i].starts_with("-n") {
            count = args[i][2..].parse().unwrap_or(10);
            i += 1;
        } else if args[i].starts_with('-') && args[i].len() > 1 {
            // -N shorthand
            if let Ok(n) = args[i][1..].parse::<usize>() {
                count = n;
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

    let show_header = files.len() > 1;
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (idx, path) in files.iter().enumerate() {
        if show_header {
            if idx > 0 {
                let _ = writeln!(out);
            }
            let _ = writeln!(out, "==> {path} <==");
        }

        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("head: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        for (line_idx, line) in buf.lines().enumerate() {
            if line_idx >= count {
                break;
            }
            match line {
                Ok(l) => {
                    let _ = writeln!(out, "{l}");
                }
                Err(e) => {
                    eprintln!("head: {path}: {e}");
                    break;
                }
            }
        }
    }
}
