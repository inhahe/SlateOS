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

    let lines_per_page = terminal_lines(env::var("LINES").ok().as_deref()).saturating_sub(1);

    for (fi, path) in files.iter().enumerate() {
        if files.len() > 1 {
            if fi > 0 {
                println!();
            }
            for line in file_header(path) {
                println!("{line}");
            }
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
        let mut line_count: usize = 0;
        let stdout = io::stdout();
        let mut out = stdout.lock();

        for line_result in buf.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let _ = writeln!(out, "{line}");
            line_count = line_count.saturating_add(1);

            if line_count >= lines_per_page {
                let _ = out.flush();
                eprint!("--More--");
                let _ = io::stderr().flush();

                match read_key() {
                    Key::Quit => return,
                    Key::Line => line_count = lines_per_page.saturating_sub(1),
                    Key::Page => line_count = 0,
                }

                eprint!("\r        \r");
                let _ = io::stderr().flush();
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Page, // space
    Line, // enter
    Quit, // q
}

fn read_key() -> Key {
    let stdin = io::stdin();
    let mut buf = [0u8; 1];
    match stdin.lock().read(&mut buf) {
        Ok(0) | Err(_) => Key::Quit,
        Ok(_) => parse_key_byte(buf.first().copied().unwrap_or(b' ')),
    }
}

/// Translate one byte of user input into a `Key` action.
fn parse_key_byte(b: u8) -> Key {
    match b {
        b'q' | b'Q' => Key::Quit,
        b' ' => Key::Page,
        b'\n' | b'\r' => Key::Line,
        _ => Key::Page,
    }
}

/// Compute the terminal line count from a `LINES` env value; falls back to 24.
fn terminal_lines(env_value: Option<&str>) -> usize {
    if let Some(val) = env_value
        && let Ok(n) = val.parse::<usize>()
        && n > 0
    {
        return n;
    }
    24
}

/// Build the three header lines printed before each file in multi-file mode.
fn file_header(path: &str) -> [String; 3] {
    [
        ":::::::::::::".to_string(),
        path.to_string(),
        ":::::::::::::".to_string(),
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn quit_keys() {
        assert_eq!(parse_key_byte(b'q'), Key::Quit);
        assert_eq!(parse_key_byte(b'Q'), Key::Quit);
    }

    #[test]
    fn space_is_page() {
        assert_eq!(parse_key_byte(b' '), Key::Page);
    }

    #[test]
    fn newline_is_line() {
        assert_eq!(parse_key_byte(b'\n'), Key::Line);
        assert_eq!(parse_key_byte(b'\r'), Key::Line);
    }

    #[test]
    fn unknown_byte_defaults_to_page() {
        assert_eq!(parse_key_byte(b'x'), Key::Page);
        assert_eq!(parse_key_byte(0), Key::Page);
        assert_eq!(parse_key_byte(255), Key::Page);
    }

    #[test]
    fn terminal_lines_default_when_unset() {
        assert_eq!(terminal_lines(None), 24);
    }

    #[test]
    fn terminal_lines_parses_env() {
        assert_eq!(terminal_lines(Some("40")), 40);
    }

    #[test]
    fn terminal_lines_falls_back_on_garbage() {
        assert_eq!(terminal_lines(Some("notanumber")), 24);
    }

    #[test]
    fn terminal_lines_falls_back_on_zero() {
        assert_eq!(terminal_lines(Some("0")), 24);
    }

    #[test]
    fn terminal_lines_falls_back_on_empty() {
        assert_eq!(terminal_lines(Some("")), 24);
    }

    #[test]
    fn file_header_contains_path() {
        let h = file_header("data.txt");
        assert_eq!(h[0], ":::::::::::::");
        assert_eq!(h[1], "data.txt");
        assert_eq!(h[2], ":::::::::::::");
    }

    #[test]
    fn file_header_with_unusual_chars() {
        let h = file_header("a b/c.txt");
        assert_eq!(h[1], "a b/c.txt");
    }
}
