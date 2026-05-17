//! unexpand — convert spaces to tabs.
//!
//! Usage: unexpand [-t N] [-a] [FILE...]
//!   -t N   tab width (default: 8)
//!   -a     convert all sequences of spaces, not just leading

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut tab_width: usize = 8;
    let mut all_spaces = false;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                if i < args.len() {
                    tab_width = args[i].parse().unwrap_or(8);
                }
            }
            "-a" => all_spaces = true,
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
                    eprintln!("unexpand: {path}: {e}");
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

            if all_spaces {
                let _ = writeln!(out, "{}", convert_all_spaces(&line, tab_width));
            } else {
                let _ = writeln!(out, "{}", convert_leading_spaces(&line, tab_width));
            }
        }
    }
}

fn convert_leading_spaces(line: &str, tab_width: usize) -> String {
    let mut result = String::new();
    let mut col = 0;
    let mut in_leading = true;
    let mut space_count = 0;

    for c in line.chars() {
        if in_leading && c == ' ' {
            space_count += 1;
            col += 1;
            if col % tab_width == 0 {
                result.push('\t');
                space_count = 0;
            }
        } else {
            if in_leading {
                // Flush remaining spaces
                for _ in 0..space_count {
                    result.push(' ');
                }
                in_leading = false;
            }
            result.push(c);
        }
    }

    if in_leading {
        for _ in 0..space_count {
            result.push(' ');
        }
    }

    result
}

fn convert_all_spaces(line: &str, tab_width: usize) -> String {
    let mut result = String::new();
    let mut col = 0;
    let mut space_count = 0;

    for c in line.chars() {
        if c == ' ' {
            space_count += 1;
            col += 1;
            if col % tab_width == 0 && space_count > 1 {
                result.push('\t');
                space_count = 0;
            }
        } else {
            for _ in 0..space_count {
                result.push(' ');
            }
            space_count = 0;
            result.push(c);
            col += 1;
        }
    }

    for _ in 0..space_count {
        result.push(' ');
    }

    result
}
