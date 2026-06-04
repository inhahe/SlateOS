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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- convert_leading_spaces ----------------

    #[test]
    fn leading_no_spaces_unchanged() {
        assert_eq!(convert_leading_spaces("hello", 8), "hello");
    }

    #[test]
    fn leading_empty_string() {
        assert_eq!(convert_leading_spaces("", 8), "");
    }

    #[test]
    fn leading_full_tab_width_becomes_tab() {
        assert_eq!(convert_leading_spaces("        text", 8), "\ttext");
    }

    #[test]
    fn leading_two_tabs_worth() {
        assert_eq!(
            convert_leading_spaces("                text", 8),
            "\t\ttext"
        );
    }

    #[test]
    fn leading_partial_tab_remains_as_spaces() {
        // 5 leading spaces, tab_width=8 -> not a full tab stop, kept as 5
        // spaces.
        assert_eq!(convert_leading_spaces("     text", 8), "     text");
    }

    #[test]
    fn leading_mixed_full_plus_partial() {
        // 10 spaces, tab_width=8: 8 become a tab, 2 remain as spaces.
        assert_eq!(convert_leading_spaces("          text", 8), "\t  text");
    }

    #[test]
    fn leading_only_affects_leading_spaces() {
        // Spaces in the middle of the line are not touched.
        assert_eq!(
            convert_leading_spaces("        a   b", 8),
            "\ta   b"
        );
    }

    #[test]
    fn leading_all_spaces_no_text() {
        // 8 spaces only -> single tab.
        assert_eq!(convert_leading_spaces("        ", 8), "\t");
    }

    #[test]
    fn leading_width_four() {
        assert_eq!(convert_leading_spaces("    text", 4), "\ttext");
        assert_eq!(convert_leading_spaces("        text", 4), "\t\ttext");
    }

    // ---------------- convert_all_spaces ----------------

    #[test]
    fn all_no_spaces_unchanged() {
        assert_eq!(convert_all_spaces("hello", 8), "hello");
    }

    #[test]
    fn all_single_space_not_converted() {
        // Implementation requires >1 space to insert a tab.
        assert_eq!(convert_all_spaces("a b c d", 8), "a b c d");
    }

    #[test]
    fn all_runs_of_spaces_at_tab_stop_become_tab() {
        // Spaces from col 1 to col 8 (7 spaces after the 'a') would land at
        // col 8 = multiple of 8, and since >1 spaces, becomes \t.
        assert_eq!(convert_all_spaces("a       b", 8), "a\tb");
    }

    #[test]
    fn all_leading_spaces_handled() {
        assert_eq!(convert_all_spaces("        text", 8), "\ttext");
    }

    #[test]
    fn all_trailing_spaces_kept() {
        // Trailing spaces with no following non-space are flushed verbatim.
        assert_eq!(convert_all_spaces("text   ", 8), "text   ");
    }

    #[test]
    fn all_empty_string() {
        assert_eq!(convert_all_spaces("", 8), "");
    }
}
