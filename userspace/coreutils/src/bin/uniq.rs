//! uniq — report or filter out repeated lines.
//!
//! Usage: uniq [-c] [-d] [-u] [INPUT [OUTPUT]]
//!   -c  prefix lines by the number of occurrences
//!   -d  only print duplicate lines
//!   -u  only print unique lines

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut count_mode = false;
    let mut duplicates_only = false;
    let mut unique_only = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'c' => count_mode = true,
                    'd' => duplicates_only = true,
                    'u' => unique_only = true,
                    _ => {
                        eprintln!("uniq: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    let reader: Box<dyn Read> = if files.is_empty() || files[0] == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(&files[0]) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("uniq: {}: {e}", files[0]);
                process::exit(1);
            }
        }
    };

    let mut writer: Box<dyn Write> = if files.len() >= 2 {
        match File::create(&files[1]) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("uniq: {}: {e}", files[1]);
                process::exit(1);
            }
        }
    } else {
        Box::new(io::stdout())
    };

    let buf = BufReader::new(reader);
    let mut prev_line: Option<String> = None;
    let mut prev_count: usize = 0;

    let emit = |w: &mut dyn Write, line: &str, count: usize| {
        let show = if duplicates_only && unique_only {
            false // contradictory, show nothing
        } else if duplicates_only {
            count > 1
        } else if unique_only {
            count == 1
        } else {
            true
        };
        if show {
            if count_mode {
                let _ = writeln!(w, "{count:>7} {line}");
            } else {
                let _ = writeln!(w, "{line}");
            }
        }
    };

    for line_result in buf.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("uniq: read error: {e}");
                break;
            }
        };

        match &prev_line {
            Some(prev) if *prev == line => {
                prev_count += 1;
            }
            _ => {
                if let Some(prev) = &prev_line {
                    emit(&mut *writer, prev, prev_count);
                }
                prev_line = Some(line);
                prev_count = 1;
            }
        }
    }

    if let Some(prev) = &prev_line {
        emit(&mut *writer, prev, prev_count);
    }
}
