//! csplit — context split: split a file into sections determined by context.
//!
//! Usage: csplit [-f PREFIX] [-n DIGITS] [-k] [-s] FILE PATTERN...
//!   -f PREFIX  use PREFIX as the output file prefix (default: "xx")
//!   -n DIGITS  use DIGITS digits in output filenames (default: 2)
//!   -k         do not remove output files on error
//!   -s         suppress output of file sizes
//!
//! Patterns:
//!   /REGEX/          split before the next line matching REGEX
//!   /REGEX/+OFFSET   split at OFFSET lines after the match
//!   /REGEX/-OFFSET   split at OFFSET lines before the match
//!   %REGEX%          skip to (but don't include) the matching line
//!   NUMBER           split at line NUMBER
//!   {N}              repeat the previous pattern N more times
//!   {*}              repeat the previous pattern until end of input
//!
//! Exit codes:
//!   0  success
//!   1  error

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

#[derive(Clone, Debug)]
enum Pattern {
    /// Split before the next line matching this substring pattern.
    Regex { pattern: String, offset: i64 },
    /// Skip to (but don't include) the matching line.
    Skip { pattern: String },
    /// Split at this line number.
    LineNumber(usize),
}

fn parse_patterns(args: &[String]) -> Vec<Pattern> {
    let mut patterns: Vec<Pattern> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg.starts_with('/') {
            // /REGEX/ or /REGEX/+N or /REGEX/-N
            let rest = &arg[1..];
            let (regex_str, offset) = if let Some(end) = rest.rfind('/') {
                let pat = &rest[..end];
                let after = &rest[end + 1..];
                let off = if after.is_empty() {
                    0i64
                } else {
                    match after.parse::<i64>() {
                        Ok(n) => n,
                        Err(_) => {
                            eprintln!("csplit: invalid offset: {after}");
                            process::exit(1);
                        }
                    }
                };
                (pat.to_string(), off)
            } else {
                (rest.to_string(), 0i64)
            };
            patterns.push(Pattern::Regex {
                pattern: regex_str,
                offset,
            });
        } else if arg.starts_with('%') {
            // %REGEX%
            let rest = &arg[1..];
            let regex_str = if let Some(end) = rest.rfind('%') {
                &rest[..end]
            } else {
                rest
            };
            patterns.push(Pattern::Skip {
                pattern: regex_str.to_string(),
            });
        } else if arg.starts_with('{') && arg.ends_with('}') {
            // {N} or {*} — repeat previous pattern
            let inner = &arg[1..arg.len() - 1];
            if patterns.is_empty() {
                eprintln!("csplit: {{}} has no preceding pattern");
                process::exit(1);
            }
            let prev = patterns.last().cloned().unwrap();
            if inner == "*" {
                // Repeat many times (we'll use a large sentinel).
                for _ in 0..10000 {
                    patterns.push(prev.clone());
                }
            } else {
                match inner.parse::<usize>() {
                    Ok(n) => {
                        for _ in 0..n {
                            patterns.push(prev.clone());
                        }
                    }
                    Err(_) => {
                        eprintln!("csplit: invalid repeat count: {inner}");
                        process::exit(1);
                    }
                }
            }
        } else {
            // Line number
            match arg.parse::<usize>() {
                Ok(n) => patterns.push(Pattern::LineNumber(n)),
                Err(_) => {
                    eprintln!("csplit: unrecognized pattern: {arg}");
                    process::exit(1);
                }
            }
        }
        i += 1;
    }

    patterns
}

fn format_filename(prefix: &str, index: usize, digits: usize) -> String {
    format!("{prefix}{index:0>width$}", width = digits)
}

/// Simple substring match (not full regex, but sufficient for common usage).
fn matches_pattern(line: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }

    // Support ^ (start of line) and $ (end of line) anchors,
    // plus basic literal matching.
    if let Some(rest) = pattern.strip_prefix('^') {
        if let Some(inner) = rest.strip_suffix('$') {
            line == inner
        } else {
            line.starts_with(rest)
        }
    } else if let Some(inner) = pattern.strip_suffix('$') {
        line.ends_with(inner)
    } else {
        line.contains(pattern)
    }
}

fn write_section(
    lines: &[String],
    prefix: &str,
    index: usize,
    digits: usize,
    silent: bool,
) -> usize {
    let filename = format_filename(prefix, index, digits);
    match File::create(&filename) {
        Ok(f) => {
            let mut writer = io::BufWriter::new(f);
            let mut bytes = 0;
            for line in lines {
                let line_bytes = format!("{line}\n");
                bytes += line_bytes.len();
                if writer.write_all(line_bytes.as_bytes()).is_err() {
                    eprintln!("csplit: write error: {filename}");
                    process::exit(1);
                }
            }
            if !silent {
                println!("{bytes}");
            }
            bytes
        }
        Err(e) => {
            eprintln!("csplit: cannot create {filename}: {e}");
            process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut prefix = "xx".to_string();
    let mut digits: usize = 2;
    let mut keep_files = false;
    let mut silent = false;
    let mut file_path: Option<String> = None;
    let mut pattern_args: Vec<String> = Vec::new();
    let mut i = 0;

    // Parse options, then FILE, then PATTERN...
    while i < args.len() {
        match args[i].as_str() {
            "-f" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("csplit: option -f requires an argument");
                    process::exit(1);
                }
                prefix = args[i].clone();
            }
            "-n" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("csplit: option -n requires an argument");
                    process::exit(1);
                }
                match args[i].parse::<usize>() {
                    Ok(n) => digits = n,
                    Err(_) => {
                        eprintln!("csplit: invalid digit count: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-k" => keep_files = true,
            "-s" | "-q" => silent = true,
            arg => {
                if file_path.is_none() {
                    file_path = Some(arg.to_string());
                } else {
                    pattern_args.push(arg.to_string());
                }
            }
        }
        i += 1;
    }

    let file_path = match file_path {
        Some(p) => p,
        None => {
            eprintln!("csplit: missing file operand");
            process::exit(1);
        }
    };

    if pattern_args.is_empty() {
        eprintln!("csplit: missing pattern operand");
        process::exit(1);
    }

    let patterns = parse_patterns(&pattern_args);

    // Read all lines from input.
    let reader: Box<dyn Read> = if file_path == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(&file_path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("csplit: {file_path}: {e}");
                process::exit(1);
            }
        }
    };

    let buf = BufReader::new(reader);
    let all_lines: Vec<String> = buf
        .lines()
        .map(|l| match l {
            Ok(line) => line,
            Err(e) => {
                eprintln!("csplit: read error: {e}");
                process::exit(1);
            }
        })
        .collect();

    let mut sections: Vec<Vec<String>> = Vec::new();
    let mut current_pos = 0;
    let mut error = false;

    for pat in &patterns {
        if current_pos >= all_lines.len() {
            break;
        }

        match pat {
            Pattern::Regex { pattern, offset } => {
                // Find the next line matching the pattern, starting from current_pos.
                let mut found = None;
                for j in current_pos..all_lines.len() {
                    if matches_pattern(&all_lines[j], pattern) {
                        found = Some(j);
                        break;
                    }
                }

                match found {
                    Some(match_line) => {
                        // Calculate split point with offset.
                        let split_at = (match_line as i64 + *offset) as usize;
                        let split_at = split_at.max(current_pos).min(all_lines.len());

                        let section: Vec<String> =
                            all_lines[current_pos..split_at].to_vec();
                        sections.push(section);
                        current_pos = split_at;
                    }
                    None => {
                        if !keep_files {
                            // Pattern not found — this is an error unless {*} was used.
                            // For {*} repeats we silently stop.
                        }
                        error = true;
                        break;
                    }
                }
            }
            Pattern::Skip { pattern } => {
                // Skip lines until pattern matches (exclusive).
                let mut found = None;
                for j in current_pos..all_lines.len() {
                    if matches_pattern(&all_lines[j], pattern) {
                        found = Some(j);
                        break;
                    }
                }

                match found {
                    Some(match_line) => {
                        // Skip everything up to and including the match.
                        current_pos = match_line + 1;
                    }
                    None => {
                        error = true;
                        break;
                    }
                }
            }
            Pattern::LineNumber(n) => {
                let split_at = (*n).min(all_lines.len());
                if split_at <= current_pos {
                    eprintln!("csplit: line number {n} is not after current position");
                    error = true;
                    break;
                }
                // Line numbers are 1-based; split before line N means
                // take lines current_pos..(n-1).
                let effective = if *n > 0 { n - 1 } else { 0 };
                let effective = effective.max(current_pos).min(all_lines.len());
                let section: Vec<String> =
                    all_lines[current_pos..effective].to_vec();
                sections.push(section);
                current_pos = effective;
            }
        }
    }

    // Remaining lines go into the last section.
    if current_pos < all_lines.len() {
        let section: Vec<String> = all_lines[current_pos..].to_vec();
        sections.push(section);
    }

    // Write output files.
    let mut output_files: Vec<String> = Vec::new();
    for (idx, section) in sections.iter().enumerate() {
        let filename = format_filename(&prefix, idx, digits);
        output_files.push(filename);
        write_section(section, &prefix, idx, digits, silent);
    }

    if error && !keep_files {
        // Remove output files on error.
        for f in &output_files {
            let _ = std::fs::remove_file(f);
        }
        process::exit(1);
    }

    if error {
        process::exit(1);
    }
}
