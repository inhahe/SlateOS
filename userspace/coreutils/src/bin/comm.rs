//! comm — compare two sorted files line by line.
//!
//! Usage: comm [-1] [-2] [-3] FILE1 FILE2
//!   -1  suppress lines unique to FILE1
//!   -2  suppress lines unique to FILE2
//!   -3  suppress lines common to both

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut suppress1 = false;
    let mut suppress2 = false;
    let mut suppress3 = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 && arg.chars().skip(1).all(|c| c.is_ascii_digit())
        {
            for c in arg[1..].chars() {
                match c {
                    '1' => suppress1 = true,
                    '2' => suppress2 = true,
                    '3' => suppress3 = true,
                    _ => {}
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    if files.len() != 2 {
        eprintln!("comm: requires exactly two files");
        process::exit(1);
    }

    let lines1 = read_lines(&files[0]);
    let lines2 = read_lines(&files[1]);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut i = 0;
    let mut j = 0;

    while i < lines1.len() || j < lines2.len() {
        if i >= lines1.len() {
            // Only file2 lines remain
            if !suppress2 {
                let prefix = if suppress1 { "" } else { "\t" };
                let _ = writeln!(out, "{}{}", prefix, lines2[j]);
            }
            j += 1;
        } else if j >= lines2.len() {
            // Only file1 lines remain
            if !suppress1 {
                let _ = writeln!(out, "{}", lines1[i]);
            }
            i += 1;
        } else if lines1[i] < lines2[j] {
            // Unique to file1
            if !suppress1 {
                let _ = writeln!(out, "{}", lines1[i]);
            }
            i += 1;
        } else if lines1[i] > lines2[j] {
            // Unique to file2
            if !suppress2 {
                let prefix = if suppress1 { "" } else { "\t" };
                let _ = writeln!(out, "{}{}", prefix, lines2[j]);
            }
            j += 1;
        } else {
            // Common to both
            if !suppress3 {
                let prefix = match (suppress1, suppress2) {
                    (true, true) => "",
                    (true, false) => "\t",
                    (false, true) => "\t",
                    (false, false) => "\t\t",
                };
                let _ = writeln!(out, "{}{}", prefix, lines1[i]);
            }
            i += 1;
            j += 1;
        }
    }
}

fn read_lines(path: &str) -> Vec<String> {
    let reader: Box<dyn Read> = if path == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("comm: {path}: {e}");
                process::exit(1);
            }
        }
    };

    BufReader::new(reader)
        .lines()
        .filter_map(|l| l.ok())
        .collect()
}
