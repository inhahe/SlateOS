//! fold — wrap each input line to fit in specified width.
//!
//! Usage: fold [-w WIDTH] [-s] [FILE...]
//!   -w N   wrap at width N (default: 80)
//!   -s     break at spaces when possible

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut width: usize = 80;
    let mut break_spaces = false;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-w" => {
                i += 1;
                if i < args.len() {
                    width = args[i].parse().unwrap_or(80);
                }
            }
            "-s" => break_spaces = true,
            arg => files.push(arg.to_string()),
        }
        i += 1;
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
                    eprintln!("fold: {path}: {e}");
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

            if line.len() <= width {
                let _ = writeln!(out, "{line}");
                continue;
            }

            let mut pos = 0;
            while pos < line.len() {
                let remaining = &line[pos..];
                if remaining.len() <= width {
                    let _ = writeln!(out, "{remaining}");
                    break;
                }

                let mut break_at = width;

                if break_spaces {
                    // Find last space within width
                    if let Some(last_space) = remaining[..width].rfind(' ')
                        && last_space > 0 {
                            break_at = last_space + 1; // include the space
                        }
                }

                let _ = writeln!(out, "{}", &remaining[..break_at]);
                pos += break_at;
            }
        }
    }
}
