//! expand — convert tabs to spaces.
//!
//! Usage: expand [-t N] [FILE...]
//!   -t N   set tab width (default: 8)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut tab_width: usize = 8;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "-t" && i + 1 < args.len() {
            tab_width = args[i + 1].parse().unwrap_or(8);
            i += 2;
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
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("expand: {path}: {e}");
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

            let mut col = 0;
            for c in line.chars() {
                if c == '\t' {
                    let spaces = tab_width - (col % tab_width);
                    for _ in 0..spaces {
                        let _ = write!(out, " ");
                    }
                    col += spaces;
                } else {
                    let _ = write!(out, "{c}");
                    col += 1;
                }
            }
            let _ = writeln!(out);
        }
    }
}
