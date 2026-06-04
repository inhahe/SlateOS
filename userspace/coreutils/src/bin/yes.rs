//! yes — output a string repeatedly until killed.
//!
//! Usage: yes [STRING]
//!   Default STRING is "y".

use std::env;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let text = yes_text(&args);

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let line = format!("{text}\n");
    let bytes = line.as_bytes();
    loop {
        if out.write_all(bytes).is_err() {
            break; // pipe closed
        }
    }
}

/// Build the per-line text from argv: default "y", otherwise space-joined args.
fn yes_text(args: &[String]) -> String {
    if args.is_empty() {
        "y".to_string()
    } else {
        args.join(" ")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn default_is_y() {
        assert_eq!(yes_text(&s(&[])), "y");
    }

    #[test]
    fn single_word_used_directly() {
        assert_eq!(yes_text(&s(&["no"])), "no");
    }

    #[test]
    fn multiple_args_joined_with_space() {
        assert_eq!(yes_text(&s(&["a", "b", "c"])), "a b c");
    }

    #[test]
    fn empty_string_arg_preserved() {
        // An empty argument participates in the join.
        assert_eq!(yes_text(&s(&["a", "", "b"])), "a  b");
    }

    #[test]
    fn args_with_spaces_inside_kept() {
        assert_eq!(yes_text(&s(&["hello world", "x"])), "hello world x");
    }

    #[test]
    fn unicode_passed_through() {
        assert_eq!(yes_text(&s(&["κόσμε"])), "κόσμε");
    }
}
