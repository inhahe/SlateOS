//! fold — wrap each input line to fit in specified width.
//!
//! Usage: fold [-w WIDTH] [-s] [FILE...]
//!   -w N   wrap at width N (default: 80)
//!   -s     break at spaces when possible

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut width: usize = 80;
    let mut break_spaces = false;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-w" => {
                i += 1;
                if i < args.len() {
                    width = args[i].parse().unwrap_or(80);
                }
            }
            "-s" => break_spaces = true,
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
                    eprintln!("fold: {path}: {e}");
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
            for chunk in fold_line(&line, width, break_spaces) {
                let _ = writeln!(out, "{chunk}");
            }
        }
    }
}

/// Wrap `line` into chunks of at most `width` bytes. Pure helper —
/// unit-testable without I/O.
///
/// When `break_spaces` is true and the chunk-boundary slice contains a
/// space, the break is taken at the last space (inclusive) so that words
/// stay intact. When `width` is zero, returns the original line as a
/// single chunk (POSIX-undefined behavior; we choose pass-through).
fn fold_line(line: &str, width: usize, break_spaces: bool) -> Vec<String> {
    if width == 0 || line.len() <= width {
        return vec![line.to_string()];
    }

    let mut out = Vec::new();
    let mut pos = 0;
    while pos < line.len() {
        let remaining = &line[pos..];
        if remaining.len() <= width {
            out.push(remaining.to_string());
            break;
        }

        let mut break_at = width;
        if break_spaces
            && let Some(last_space) = remaining.get(..width).and_then(|s| s.rfind(' '))
            && last_space > 0
        {
            break_at = last_space + 1; // include the space
        }

        // Defensive: break_at must land inside `remaining`.
        let take = break_at.min(remaining.len());
        if let Some(slice) = remaining.get(..take) {
            out.push(slice.to_string());
        }
        pos += take;
        if take == 0 {
            // Should never happen, but guard against an infinite loop.
            break;
        }
    }

    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn shorter_than_width_unchanged() {
        assert_eq!(fold_line("hello", 80, false), vec!["hello".to_string()]);
    }

    #[test]
    fn empty_line_returns_empty_chunk() {
        assert_eq!(fold_line("", 80, false), vec!["".to_string()]);
    }

    #[test]
    fn exactly_at_width_single_chunk() {
        assert_eq!(fold_line("abcde", 5, false), vec!["abcde".to_string()]);
    }

    #[test]
    fn hard_break_at_width() {
        assert_eq!(
            fold_line("abcdefghij", 5, false),
            vec!["abcde".to_string(), "fghij".to_string()]
        );
    }

    #[test]
    fn hard_break_into_three_chunks() {
        assert_eq!(
            fold_line("aaaaabbbbbccccc", 5, false),
            vec!["aaaaa".to_string(), "bbbbb".to_string(), "ccccc".to_string()]
        );
    }

    #[test]
    fn break_at_space_with_s_flag() {
        // 'hello world!' width 8 -> "hello " then "world!".
        assert_eq!(
            fold_line("hello world!", 8, true),
            vec!["hello ".to_string(), "world!".to_string()]
        );
    }

    #[test]
    fn break_at_space_keeps_words_intact() {
        // "the quick brown fox" wrapped at 10 with -s -> "the quick " then
        // "brown fox".
        assert_eq!(
            fold_line("the quick brown fox", 10, true),
            vec!["the quick ".to_string(), "brown fox".to_string()]
        );
    }

    #[test]
    fn no_space_in_chunk_falls_back_to_hard_break() {
        // Even with -s, if no space exists in the first chunk, fold hard
        // at width.
        assert_eq!(
            fold_line("supercalifragilistic", 5, true),
            vec![
                "super".to_string(),
                "calif".to_string(),
                "ragil".to_string(),
                "istic".to_string(),
            ]
        );
    }

    #[test]
    fn break_at_space_at_position_zero_falls_back_to_hard_break() {
        // Space at index 0 isn't usable (would emit empty chunk), so fold
        // at width instead.
        assert_eq!(
            fold_line(" abcdefghi", 5, true),
            vec![" abcd".to_string(), "efghi".to_string()]
        );
    }

    #[test]
    fn trailing_chunk_shorter_than_width() {
        assert_eq!(
            fold_line("abcdefg", 5, false),
            vec!["abcde".to_string(), "fg".to_string()]
        );
    }

    #[test]
    fn width_zero_returns_original_as_one_chunk() {
        // Guard: width=0 -> pass-through.
        assert_eq!(fold_line("hello", 0, false), vec!["hello".to_string()]);
    }
}
