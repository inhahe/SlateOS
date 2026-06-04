//! echo — write arguments to standard output.
//!
//! Usage: echo [-n] [-e] [STRING...]
//!   -n  do not output trailing newline
//!   -e  enable interpretation of backslash escapes

use std::env;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = out.write_all(&render(&args));
    let _ = out.flush();
}

/// Parsed echo flag state.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct EchoFlags {
    no_newline: bool,
    interpret_escapes: bool,
    /// Index of the first non-flag argument.
    first_text: usize,
}

/// Parse echo's flag arguments. Echo is unusual: flag parsing stops at the
/// first non-flag argument, treating everything after as text.
fn parse_flags(args: &[String]) -> EchoFlags {
    let mut flags = EchoFlags {
        no_newline: false,
        interpret_escapes: false,
        first_text: 0,
    };

    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "-n" => flags.no_newline = true,
            "-e" => flags.interpret_escapes = true,
            "-ne" | "-en" => {
                flags.no_newline = true;
                flags.interpret_escapes = true;
            }
            _ => {
                flags.first_text = i;
                return flags;
            }
        }
        flags.first_text = i + 1;
    }

    flags
}

/// Render the full output of `echo` for the given arguments, as a byte
/// buffer. Pure helper — unit-testable without I/O.
fn render(args: &[String]) -> Vec<u8> {
    let flags = parse_flags(args);
    let mut out = Vec::new();

    for (i, arg) in args[flags.first_text..].iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        if flags.interpret_escapes {
            out.extend_from_slice(&unescape(arg));
        } else {
            out.extend_from_slice(arg.as_bytes());
        }
    }

    if !flags.no_newline {
        out.push(b'\n');
    }
    out
}

fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'\\' => out.push(b'\\'),
                b'0' => out.push(0),
                b'a' => out.push(7),  // BEL
                b'b' => out.push(8),  // BS
                b'f' => out.push(12), // FF
                other => {
                    out.push(b'\\');
                    out.push(other);
                }
            }
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    // ---------------- parse_flags ----------------

    #[test]
    fn flags_no_args_defaults() {
        let f = parse_flags(&args(&[]));
        assert_eq!(
            f,
            EchoFlags {
                no_newline: false,
                interpret_escapes: false,
                first_text: 0,
            }
        );
    }

    #[test]
    fn flags_n_only() {
        let f = parse_flags(&args(&["-n", "hello"]));
        assert!(f.no_newline);
        assert!(!f.interpret_escapes);
        assert_eq!(f.first_text, 1);
    }

    #[test]
    fn flags_e_only() {
        let f = parse_flags(&args(&["-e", "hello"]));
        assert!(!f.no_newline);
        assert!(f.interpret_escapes);
        assert_eq!(f.first_text, 1);
    }

    #[test]
    fn flags_n_then_e() {
        let f = parse_flags(&args(&["-n", "-e", "hello"]));
        assert!(f.no_newline);
        assert!(f.interpret_escapes);
        assert_eq!(f.first_text, 2);
    }

    #[test]
    fn flags_combined_ne() {
        let f = parse_flags(&args(&["-ne", "hello"]));
        assert!(f.no_newline);
        assert!(f.interpret_escapes);
        assert_eq!(f.first_text, 1);
    }

    #[test]
    fn flags_combined_en() {
        let f = parse_flags(&args(&["-en", "hello"]));
        assert!(f.no_newline);
        assert!(f.interpret_escapes);
        assert_eq!(f.first_text, 1);
    }

    #[test]
    fn flags_stop_at_non_flag() {
        // Flags after a non-flag argument are treated as text.
        let f = parse_flags(&args(&["hello", "-n"]));
        assert!(!f.no_newline);
        assert!(!f.interpret_escapes);
        assert_eq!(f.first_text, 0);
    }

    #[test]
    fn flags_unknown_flag_treated_as_text() {
        // GNU echo treats -x as text.
        let f = parse_flags(&args(&["-x", "rest"]));
        assert!(!f.no_newline);
        assert!(!f.interpret_escapes);
        assert_eq!(f.first_text, 0);
    }

    // ---------------- render ----------------

    #[test]
    fn render_empty_args_just_newline() {
        assert_eq!(render(&args(&[])), b"\n");
    }

    #[test]
    fn render_single_word() {
        assert_eq!(render(&args(&["hello"])), b"hello\n");
    }

    #[test]
    fn render_multiple_words_space_separated() {
        assert_eq!(render(&args(&["a", "b", "c"])), b"a b c\n");
    }

    #[test]
    fn render_no_newline_with_n_flag() {
        assert_eq!(render(&args(&["-n", "hello"])), b"hello");
    }

    #[test]
    fn render_no_escapes_without_e_flag() {
        // Without -e, backslash sequences are literal.
        assert_eq!(render(&args(&["hello\\nworld"])), b"hello\\nworld\n");
    }

    #[test]
    fn render_escapes_with_e_flag() {
        assert_eq!(render(&args(&["-e", "hello\\nworld"])), b"hello\nworld\n");
    }

    #[test]
    fn render_n_and_e_combined() {
        assert_eq!(render(&args(&["-ne", "tab\\there"])), b"tab\there");
    }

    #[test]
    fn render_no_args_with_n_flag_emits_nothing() {
        assert!(render(&args(&["-n"])).is_empty());
    }

    // ---------------- unescape ----------------

    #[test]
    fn unescape_no_backslashes() {
        assert_eq!(unescape("hello"), b"hello");
    }

    #[test]
    fn unescape_newline() {
        assert_eq!(unescape("a\\nb"), b"a\nb");
    }

    #[test]
    fn unescape_tab() {
        assert_eq!(unescape("a\\tb"), b"a\tb");
    }

    #[test]
    fn unescape_cr() {
        assert_eq!(unescape("a\\rb"), b"a\rb");
    }

    #[test]
    fn unescape_backslash() {
        assert_eq!(unescape("a\\\\b"), b"a\\b");
    }

    #[test]
    fn unescape_null() {
        assert_eq!(unescape("a\\0b"), b"a\0b");
    }

    #[test]
    fn unescape_bell_backspace_formfeed() {
        assert_eq!(unescape("\\a\\b\\f"), [7u8, 8u8, 12u8]);
    }

    #[test]
    fn unescape_unknown_escape_passes_through() {
        // \x is not a recognized escape — print backslash + x verbatim.
        assert_eq!(unescape("a\\xb"), b"a\\xb");
    }

    #[test]
    fn unescape_trailing_backslash_kept() {
        // A trailing backslash has no following char — emitted as literal.
        assert_eq!(unescape("a\\"), b"a\\");
    }

    #[test]
    fn unescape_empty_string() {
        assert!(unescape("").is_empty());
    }
}
