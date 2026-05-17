//! wc — word, line, character, and byte count.
//!
//! Usage: wc [-l] [-w] [-c] [-m] [FILE...]
//!   -l  count lines
//!   -w  count words
//!   -c  count bytes
//!   -m  count characters
//!   If no flags, show all of: lines words bytes.
//!   If no FILE or FILE is "-", read from standard input.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

struct Counts {
    lines: usize,
    words: usize,
    bytes: usize,
    chars: usize,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut show_lines = false;
    let mut show_words = false;
    let mut show_bytes = false;
    let mut show_chars = false;
    let mut files: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'l' => show_lines = true,
                    'w' => show_words = true,
                    'c' => show_bytes = true,
                    'm' => show_chars = true,
                    _ => eprintln!("wc: unknown option: -{c}"),
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    // Default: show everything except chars
    if !show_lines && !show_words && !show_bytes && !show_chars {
        show_lines = true;
        show_words = true;
        show_bytes = true;
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut total = Counts { lines: 0, words: 0, bytes: 0, chars: 0 };

    for path in &files {
        let mut reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("wc: {path}: {e}");
                    continue;
                }
            }
        };

        let mut data = Vec::new();
        if let Err(e) = reader.read_to_end(&mut data) {
            eprintln!("wc: {path}: {e}");
            continue;
        }

        let counts = count_data(&data);
        total.lines += counts.lines;
        total.words += counts.words;
        total.bytes += counts.bytes;
        total.chars += counts.chars;

        print_counts(&mut out, &counts, path, show_lines, show_words, show_bytes, show_chars);
    }

    if files.len() > 1 {
        print_counts(&mut out, &total, "total", show_lines, show_words, show_bytes, show_chars);
    }
}

fn count_data(data: &[u8]) -> Counts {
    let lines = data.iter().filter(|&&b| b == b'\n').count();
    let bytes = data.len();

    // Count words (runs of non-whitespace)
    let mut words = 0;
    let mut in_word = false;
    for &b in data {
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            in_word = false;
        } else if !in_word {
            in_word = true;
            words += 1;
        }
    }

    // Count UTF-8 characters
    let text = String::from_utf8_lossy(data);
    let chars = text.chars().count();

    Counts { lines, words, bytes, chars }
}

fn print_counts(
    out: &mut impl Write,
    c: &Counts,
    name: &str,
    lines: bool,
    words: bool,
    bytes: bool,
    chars: bool,
) {
    if lines { let _ = write!(out, "{:>7} ", c.lines); }
    if words { let _ = write!(out, "{:>7} ", c.words); }
    if bytes { let _ = write!(out, "{:>7} ", c.bytes); }
    if chars { let _ = write!(out, "{:>7} ", c.chars); }
    let _ = writeln!(out, "{name}");
}
