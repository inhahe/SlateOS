//! tail -- output the last part of files.
//!
//! Usage: tail [-n COUNT] [FILE...]
//!   -n COUNT  print last COUNT lines (default 10)
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
        } else if args[i].starts_with('-') && args[i].len() > 1 && args[i] != "-" {
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
                    eprintln!("tail: {path}: {e}");
                    continue;
                }
            }
        };

        // Read all lines into a ring buffer of size `count`.
        let buf = BufReader::new(reader);
        let mut ring: Vec<String> = Vec::with_capacity(count);
        let mut pos: usize = 0;
        let mut total: usize = 0;

        for line in buf.lines() {
            match line {
                Ok(l) => {
                    if ring.len() < count {
                        ring.push(l);
                    } else {
                        ring[pos] = l;
                    }
                    pos = (pos + 1) % count.max(1);
                    total += 1;
                }
                Err(e) => {
                    eprintln!("tail: {path}: {e}");
                    break;
                }
            }
        }

        // Output from the ring buffer in order.
        if ring.len() < count || total <= count {
            // Fewer lines than requested; print all in order.
            for line in &ring {
                let _ = writeln!(out, "{line}");
            }
        } else {
            // Ring is full; `pos` points to the oldest entry.
            for j in 0..ring.len() {
                let idx = (pos + j) % ring.len();
                let _ = writeln!(out, "{}", ring[idx]);
            }
        }
    }
}
