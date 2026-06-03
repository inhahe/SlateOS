//! more — file perusal filter for viewing text one screen at a time.
//!
//! Usage: more [FILE...]
//!   Displays text one screen at a time.
//!   Press Enter for next line, Space for next page, q to quit.
//!   Without files, reads from stdin.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut files: Vec<String> = args;

    if files.is_empty() {
        files.push("-".to_string());
    }

    let lines_per_page = get_terminal_lines().saturating_sub(1); // leave room for prompt

    for (fi, path) in files.iter().enumerate() {
        if files.len() > 1 {
            if fi > 0 {
                println!();
            }
            println!(":::::::::::::");
            println!("{path}");
            println!(":::::::::::::");
        }

        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("more: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        let mut line_count = 0;
        let stdout = io::stdout();
        let mut out = stdout.lock();

        for line_result in buf.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let _ = writeln!(out, "{line}");
            line_count += 1;

            if line_count >= lines_per_page {
                let _ = out.flush();
                // Show prompt
                eprint!("--More--");
                let _ = io::stderr().flush();

                // Wait for user input
                match read_key() {
                    Key::Quit => return,
                    Key::Line => line_count = lines_per_page - 1, // show one more line
                    Key::Page => line_count = 0,                  // show full page
                }

                // Clear the --More-- prompt
                eprint!("\r        \r");
                let _ = io::stderr().flush();
            }
        }
    }
}

enum Key {
    Page,  // space
    Line,  // enter
    Quit,  // q
}

fn read_key() -> Key {
    // Read one byte from stdin (in raw mode ideally, but
    // we fall back to line-buffered if raw mode isn't available).
    let stdin = io::stdin();
    let mut buf = [0u8; 1];
    match stdin.lock().read(&mut buf) {
        Ok(0) | Err(_) => Key::Quit,
        Ok(_) => match buf[0] {
            b'q' | b'Q' => Key::Quit,
            b' ' => Key::Page,
            b'\n' | b'\r' => Key::Line,
            _ => Key::Page, // default: next page
        },
    }
}

fn get_terminal_lines() -> usize {
    // Try reading from LINES env var or default to 24
    if let Ok(val) = std::env::var("LINES") {
        if let Ok(n) = val.parse::<usize>() {
            return n;
        }
    }
    24
}
