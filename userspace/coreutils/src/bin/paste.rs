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
            let lines: Vec<String> = buf.lines().map_while(Result::ok).collect();
            let _ = writeln!(out, "{}", join_serial(&lines, &delim));
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
            let mut pieces: Vec<String> = Vec::with_capacity(readers.len());

            for reader_opt in &mut readers {
                if let Some(reader) = reader_opt {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => pieces.push(String::new()), // EOF
                        Ok(_) => {
                            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                            pieces.push(trimmed.to_string());
                            any_line = true;
                        }
                        Err(_) => pieces.push(String::new()),
                    }
                } else {
                    pieces.push(String::new());
                }
            }

            if !any_line {
                break;
            }
            let _ = writeln!(out, "{}", join_parallel(&pieces, &delim));
        }
    }
}

/// Serial-mode line: join all of one file's lines with `delim`.
fn join_serial(lines: &[String], delim: &str) -> String {
    lines.join(delim)
}

/// Parallel-mode line: join one column from each file with `delim`. Missing
/// columns (from files that have hit EOF) come in as empty strings, which
/// is what POSIX `paste` requires (the delimiters are still emitted).
fn join_parallel(pieces: &[String], delim: &str) -> String {
    pieces.join(delim)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn serial_empty_file_produces_empty_string() {
        assert_eq!(join_serial(&[], "\t"), "");
    }

    #[test]
    fn serial_single_line_no_delim_appended() {
        assert_eq!(join_serial(&s(&["hello"]), "\t"), "hello");
    }

    #[test]
    fn serial_multiple_lines_joined_by_tab() {
        assert_eq!(
            join_serial(&s(&["a", "b", "c"]), "\t"),
            "a\tb\tc"
        );
    }

    #[test]
    fn serial_custom_delimiter() {
        assert_eq!(join_serial(&s(&["a", "b", "c"]), ", "), "a, b, c");
    }

    #[test]
    fn serial_empty_lines_preserved() {
        assert_eq!(join_serial(&s(&["", "b", ""]), "|"), "|b|");
    }

    #[test]
    fn parallel_two_files_one_column_each() {
        assert_eq!(join_parallel(&s(&["a", "1"]), "\t"), "a\t1");
    }

    #[test]
    fn parallel_eof_file_emits_empty_with_delim() {
        // The file that hit EOF contributes "" but the delimiter still
        // separates the columns.
        assert_eq!(join_parallel(&s(&["a", "", "c"]), "\t"), "a\t\tc");
    }

    #[test]
    fn parallel_custom_delimiter() {
        assert_eq!(join_parallel(&s(&["a", "b"]), ", "), "a, b");
    }

    #[test]
    fn parallel_single_column() {
        assert_eq!(join_parallel(&s(&["only"]), "\t"), "only");
    }

    #[test]
    fn parallel_no_columns_empty() {
        assert_eq!(join_parallel(&[], "\t"), "");
    }
}
