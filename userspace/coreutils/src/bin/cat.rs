//! cat — concatenate and print files.
//!
//! Usage: cat [-n] [FILE...]
//!   -n  number all output lines
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut number_lines = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg == "-n" {
            number_lines = true;
        } else {
            files.push(arg.clone());
        }
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
                    eprintln!("cat: {path}: {e}");
                    continue;
                }
            }
        };

        if number_lines {
            let buf = BufReader::new(reader);
            for line in buf.lines() {
                match line {
                    Ok(l) => {
                        let _ = writeln!(out, "{line_num:6}\t{l}");
                        line_num += 1;
                    }
                    Err(e) => {
                        eprintln!("cat: {path}: {e}");
                        break;
                    }
                }
            }
        } else {
            let mut buf = BufReader::new(reader);
            let mut chunk = [0u8; 4096];
            loop {
                match buf.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = out.write_all(&chunk[..n]);
                    }
                    Err(e) => {
                        eprintln!("cat: {path}: {e}");
                        break;
                    }
                }
            }
        }
    }
}
