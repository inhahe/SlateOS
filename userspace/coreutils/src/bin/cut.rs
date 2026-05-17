//! cut — remove sections from each line of files.
//!
//! Usage: cut -d DELIM -f FIELDS [FILE...]
//!        cut -c CHARS [FILE...]
//!   -d DELIM   use DELIM as field delimiter (default TAB)
//!   -f FIELDS  select fields (comma-separated, e.g. 1,3 or 1-3)
//!   -c CHARS   select characters (comma-separated ranges)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut delim = '\t';
    let mut field_spec: Option<String> = None;
    let mut char_spec: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "-d" && i + 1 < args.len() {
            let d = &args[i + 1];
            delim = d.chars().next().unwrap_or('\t');
            i += 2;
        } else if args[i].starts_with("-d") && args[i].len() > 2 {
            delim = args[i][2..].chars().next().unwrap_or('\t');
            i += 1;
        } else if args[i] == "-f" && i + 1 < args.len() {
            field_spec = Some(args[i + 1].clone());
            i += 2;
        } else if args[i].starts_with("-f") && args[i].len() > 2 {
            field_spec = Some(args[i][2..].to_string());
            i += 1;
        } else if args[i] == "-c" && i + 1 < args.len() {
            char_spec = Some(args[i + 1].clone());
            i += 2;
        } else if args[i].starts_with("-c") && args[i].len() > 2 {
            char_spec = Some(args[i][2..].to_string());
            i += 1;
        } else {
            files.push(args[i].clone());
            i += 1;
        }
    }

    if field_spec.is_none() && char_spec.is_none() {
        eprintln!("cut: you must specify -f or -c");
        process::exit(1);
    }

    let indices = parse_ranges(field_spec.as_deref().or(char_spec.as_deref()).unwrap_or(""));

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
                    eprintln!("cut: {path}: {e}");
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

            if field_spec.is_some() {
                let fields: Vec<&str> = line.split(delim).collect();
                let mut first = true;
                for &idx in &indices {
                    if idx > 0 && idx <= fields.len() {
                        if !first {
                            let _ = write!(out, "{delim}");
                        }
                        let _ = write!(out, "{}", fields[idx - 1]);
                        first = false;
                    }
                }
                let _ = writeln!(out);
            } else {
                // character mode
                let chars: Vec<char> = line.chars().collect();
                for &idx in &indices {
                    if idx > 0 && idx <= chars.len() {
                        let _ = write!(out, "{}", chars[idx - 1]);
                    }
                }
                let _ = writeln!(out);
            }
        }
    }
}

/// Parse range specs like "1,3,5-7" into a sorted list of 1-based indices.
fn parse_ranges(spec: &str) -> Vec<usize> {
    let mut result = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            let start: usize = a.parse().unwrap_or(1);
            let end: usize = b.parse().unwrap_or(start);
            for i in start..=end {
                result.push(i);
            }
        } else if let Ok(n) = part.parse::<usize>() {
            result.push(n);
        }
    }
    result.sort();
    result.dedup();
    result
}
