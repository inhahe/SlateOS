//! paste — merge lines of files.
//!
//! Usage: paste [-d DELIM] [-s] FILE...
//!   -d DELIM   use DELIM instead of TAB
//!   -s         paste one file at a time instead of side-by-side

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut delim = "\t".to_string();
    let mut serial = false;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-d" => {
                i += 1;
                if i < args.len() {
                    delim = args[i].clone();
                }
            }
            "-s" => serial = true,
            arg => files.push(arg.to_string()),
        }
        i += 1;
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if serial {
        // Serial mode: output each file's lines joined by delimiter
        for path in &files {
            let reader: Box<dyn Read> = if path == "-" {
                Box::new(io::stdin())
            } else {
                match File::open(path) {
                    Ok(f) => Box::new(f),
                    Err(e) => {
                        eprintln!("paste: {path}: {e}");
                        continue;
                    }
                }
            };

            let buf = BufReader::new(reader);
            let mut first = true;
            for line_result in buf.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => break,
                };
                if !first {
                    let _ = write!(out, "{delim}");
                }
                let _ = write!(out, "{line}");
                first = false;
            }
            let _ = writeln!(out);
        }
    } else {
        // Parallel mode: merge corresponding lines from all files
        let mut readers: Vec<Option<BufReader<Box<dyn Read>>>> = files
            .iter()
            .map(|path| {
                let r: Box<dyn Read> = if path == "-" {
                    Box::new(io::stdin())
                } else {
                    match File::open(path) {
                        Ok(f) => Box::new(f),
                        Err(e) => {
                            eprintln!("paste: {path}: {e}");
                            return None;
                        }
                    }
                };
                Some(BufReader::new(r))
            })
            .collect();

        loop {
            let mut any_line = false;
            let mut first = true;

            for reader_opt in &mut readers {
                if !first {
                    let _ = write!(out, "{delim}");
                }
                first = false;

                if let Some(reader) = reader_opt {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {} // EOF
                        Ok(_) => {
                            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                            let _ = write!(out, "{trimmed}");
                            any_line = true;
                        }
                        Err(_) => {}
                    }
                }
            }

            if !any_line {
                break;
            }
            let _ = writeln!(out);
        }
    }
}
