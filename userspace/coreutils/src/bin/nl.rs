//! nl — number lines of files.
//!
//! Usage: nl [-b a|t|n] [-w WIDTH] [FILE...]
//!   -b a   number all lines (default)
//!   -b t   number only non-empty lines
//!   -b n   no numbering
//!   -w N   width of line number field (default: 6)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut body_num = 'a'; // a=all, t=non-empty, n=none
    let mut width: usize = 6;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-b" => {
                i += 1;
                if i < args.len() {
                    body_num = args[i].chars().next().unwrap_or('a');
                }
            }
            "-w" => {
                i += 1;
                if i < args.len() {
                    width = args[i].parse().unwrap_or(6);
                }
            }
            arg if !arg.starts_with('-') || arg == "-" => {
                files.push(arg.to_string());
            }
            _ => {
                // Ignore unknown flags gracefully
            }
        }
        i += 1;
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut line_num: usize = 1;

    for path in &files {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("nl: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        for line_result in buf.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let should_number = match body_num {
                'a' => true,
                't' => !line.is_empty(),
                'n' => false,
                _ => true,
            };

            if should_number {
                let _ = writeln!(out, "{:>width$}\t{}", line_num, line);
                line_num += 1;
            } else {
                let _ = writeln!(out, "{:>width$}\t{}", "", line);
            }
        }
    }
}
