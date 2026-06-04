//! ed — the standard line editor.
//!
//! Usage: ed [FILE]
//!   Classic line editor. Commands operate on a buffer of lines.
//!
//! Commands:
//!   NUM           set current line
//!   p / NUM p     print current/specified line
//!   N,M p         print lines N through M
//!   a             append after current line (end with '.')
//!   i             insert before current line (end with '.')
//!   d / N,M d     delete current/specified lines
//!   c / N,M c     change (replace) lines (end with '.')
//!   s/PAT/REPL/   substitute on current line
//!   w [FILE]      write buffer to file
//!   q             quit (warns if unsaved)
//!   Q             quit without saving
//!   f [FILE]      show/set filename
//!   = / NUM =     print line count / specified line number
//!   n / NUM n     print with line numbers
//!   , p           print all lines
//!   $ p           print last line

use std::env;
use std::fs;
use std::io::{self, BufRead};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut buffer: Vec<String> = Vec::new();
    let mut current: usize = 0; // 1-based, 0 = empty
    let mut filename: Option<String> = None;
    let mut modified = false;

    if let Some(path) = args.first() {
        filename = Some(path.clone());
        match fs::read_to_string(path) {
            Ok(content) => {
                buffer = content.lines().map(str::to_string).collect();
                current = buffer.len();
                let bytes = content.len();
                println!("{bytes}");
            }
            Err(_) => {
                // New file — empty buffer
                println!("0");
            }
        }
    }

    let stdin = io::stdin();

    loop {
        // No prompt in ed (traditional)
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');

        let parsed = parse_command(line, current, buffer.len());

        match parsed.cmd {
            'q' => {
                if modified {
                    println!("?");
                    modified = false; // next 'q' will quit
                } else {
                    break;
                }
            }
            'Q' => break,

            'p' => {
                for i in parsed.addr_start..=parsed.addr_end {
                    if let Some(s) = nth_line(&buffer, i) {
                        println!("{s}");
                    }
                }
                current = parsed.addr_end.min(buffer.len());
            }

            'n' => {
                for i in parsed.addr_start..=parsed.addr_end {
                    if let Some(s) = nth_line(&buffer, i) {
                        println!("{i}\t{s}");
                    }
                }
                current = parsed.addr_end.min(buffer.len());
            }

            'a' => {
                let insert_at = parsed.addr_start;
                let new_lines = read_input_lines(&stdin);
                insert_lines_at(&mut buffer, insert_at, &new_lines);
                current = insert_at.saturating_add(new_lines.len());
                modified = true;
            }

            'i' => {
                let insert_at = parsed.addr_start.saturating_sub(1);
                let new_lines = read_input_lines(&stdin);
                insert_lines_at(&mut buffer, insert_at, &new_lines);
                current = insert_at.saturating_add(new_lines.len());
                modified = true;
            }

            'c' if parsed.addr_start >= 1 && parsed.addr_end <= buffer.len() => {
                let new_lines = read_input_lines(&stdin);
                let new_current = change_lines(
                    &mut buffer,
                    parsed.addr_start,
                    parsed.addr_end,
                    &new_lines,
                );
                current = new_current;
                modified = true;
            }

            'd' if parsed.addr_start >= 1 && parsed.addr_end <= buffer.len() => {
                let new_current = delete_lines(&mut buffer, parsed.addr_start, parsed.addr_end);
                current = new_current;
                modified = true;
            }

            's' if current >= 1 && current <= buffer.len() => {
                if let Some((pat, repl, global)) = parse_substitute(&parsed.arg) {
                    let target_idx = current.saturating_sub(1);
                    if let Some(line) = buffer.get(target_idx) {
                        let new_line = substitute_line(line, &pat, &repl, global);
                        if let Some(slot) = buffer.get_mut(target_idx) {
                            *slot = new_line;
                            println!("{slot}");
                        }
                        modified = true;
                    }
                } else {
                    println!("?");
                }
            }

            'w' => {
                let path = if parsed.arg.is_empty() {
                    filename.clone()
                } else {
                    let p = parsed.arg.clone();
                    filename = Some(p.clone());
                    Some(p)
                };

                match path {
                    Some(p) => {
                        let content: String = buffer.iter().map(|l| format!("{l}\n")).collect();
                        match fs::write(&p, &content) {
                            Ok(()) => {
                                println!("{}", content.len());
                                modified = false;
                            }
                            Err(e) => {
                                println!("? {e}");
                            }
                        }
                    }
                    None => {
                        println!("? no filename");
                    }
                }
            }

            'f' => {
                if !parsed.arg.is_empty() {
                    filename = Some(parsed.arg.clone());
                }
                match &filename {
                    Some(f) => println!("{f}"),
                    None => println!("?"),
                }
            }

            '=' => {
                println!("{}", buffer.len());
            }

            '\0' => {
                // Just a line number — print it
                if let Some(s) = nth_line(&buffer, parsed.addr_start) {
                    current = parsed.addr_start;
                    println!("{s}");
                } else if parsed.addr_start > 0 {
                    println!("?");
                }
            }

            _ => {
                println!("?");
            }
        }
    }
}

fn read_input_lines(stdin: &io::Stdin) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        if line == "." {
            break;
        }
        lines.push(line.to_string());
    }
    lines
}

// ============================================================================
// Pure helpers (covered by unit tests)
// ============================================================================

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ParsedCommand {
    addr_start: usize,
    addr_end: usize,
    cmd: char,
    arg: String,
}

/// Look up the 1-based line `n` in `buffer`.  Returns None for 0 or
/// out-of-range indices.
fn nth_line(buffer: &[String], n: usize) -> Option<&str> {
    if n == 0 {
        return None;
    }
    buffer.get(n.saturating_sub(1)).map(String::as_str)
}

/// Insert `new_lines` after position `insert_at` (0-based offset into
/// the buffer).  Out-of-range insertion positions are clamped to the
/// buffer length.
fn insert_lines_at(buffer: &mut Vec<String>, insert_at: usize, new_lines: &[String]) {
    let clamped = insert_at.min(buffer.len());
    for (j, line) in new_lines.iter().enumerate() {
        let idx = clamped.saturating_add(j).min(buffer.len());
        buffer.insert(idx, line.clone());
    }
}

/// Delete lines `addr_start..=addr_end` (1-based, inclusive) from
/// `buffer` and return the new value of `current`.  Caller must have
/// already verified the range is in-bounds.
fn delete_lines(buffer: &mut Vec<String>, addr_start: usize, addr_end: usize) -> usize {
    let drain_start = addr_start.saturating_sub(1);
    let drain_end = addr_end.min(buffer.len());
    if drain_start <= drain_end {
        buffer.drain(drain_start..drain_end);
    }
    if addr_start <= buffer.len() {
        addr_start
    } else {
        buffer.len()
    }
}

/// Change lines `addr_start..=addr_end` (1-based) to `new_lines` and
/// return the new current-line value.
fn change_lines(
    buffer: &mut Vec<String>,
    addr_start: usize,
    addr_end: usize,
    new_lines: &[String],
) -> usize {
    let drain_start = addr_start.saturating_sub(1);
    let drain_end = addr_end.min(buffer.len());
    if drain_start <= drain_end {
        buffer.drain(drain_start..drain_end);
    }
    for (j, line) in new_lines.iter().enumerate() {
        let idx = drain_start.saturating_add(j).min(buffer.len());
        buffer.insert(idx, line.clone());
    }
    drain_start.saturating_add(new_lines.len())
}

/// Substitute the first (or all, if `global`) occurrence of `pat` with
/// `repl` in `line`.
fn substitute_line(line: &str, pat: &str, repl: &str, global: bool) -> String {
    if pat.is_empty() {
        return line.to_string();
    }
    if global {
        line.replace(pat, repl)
    } else {
        line.replacen(pat, repl, 1)
    }
}

fn parse_command(input: &str, current: usize, total: usize) -> ParsedCommand {
    let input = input.trim();
    if input.is_empty() {
        let next = current.saturating_add(1);
        return ParsedCommand {
            addr_start: next,
            addr_end: next,
            cmd: '\0',
            arg: String::new(),
        };
    }

    let bytes = input.as_bytes();
    let mut pos = 0;

    let addr1 = parse_address(input, &mut pos, current, total);

    let addr2 = if bytes.get(pos).copied() == Some(b',') {
        pos = pos.saturating_add(1);
        parse_address(input, &mut pos, current, total)
    } else {
        addr1
    };

    // Parse command character. Anything past the end of the address is either
    // a literal command char or, for a bare line number / empty input, '\0'.
    let cmd = match bytes.get(pos).copied() {
        Some(c) => {
            pos = pos.saturating_add(1);
            c as char
        }
        None => '\0',
    };

    let arg = input.get(pos..).unwrap_or("").trim().to_string();

    ParsedCommand {
        addr_start: addr1,
        addr_end: addr2,
        cmd,
        arg,
    }
}

fn parse_address(input: &str, pos: &mut usize, current: usize, total: usize) -> usize {
    let bytes = input.as_bytes();

    while bytes.get(*pos).copied() == Some(b' ') {
        *pos = pos.saturating_add(1);
    }

    match bytes.get(*pos).copied() {
        Some(b'.') => {
            *pos = pos.saturating_add(1);
            current
        }
        Some(b'$') => {
            *pos = pos.saturating_add(1);
            total
        }
        Some(b) if b.is_ascii_digit() => {
            let start = *pos;
            while bytes.get(*pos).is_some_and(u8::is_ascii_digit) {
                *pos = pos.saturating_add(1);
            }
            input
                .get(start..*pos)
                .and_then(|s| s.parse().ok())
                .unwrap_or(current)
        }
        _ => current,
    }
}

/// Parse the body of an `s` command (everything after the leading 's').
/// Returns `(pattern, replacement, global)` on success.
fn parse_substitute(arg: &str) -> Option<(String, String, bool)> {
    let bytes = arg.as_bytes();
    let delim = *bytes.first()?;
    let rest = arg.get(1..).unwrap_or("");

    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let rest_bytes = rest.as_bytes();
    let mut i = 0;

    while let Some(&b) = rest_bytes.get(i) {
        if b == b'\\' && rest_bytes.get(i.saturating_add(1)).is_some() {
            if let Some(&next) = rest_bytes.get(i.saturating_add(1)) {
                current.push(next as char);
            }
            i = i.saturating_add(2);
        } else if b == delim {
            parts.push(current.clone());
            current.clear();
            i = i.saturating_add(1);
        } else {
            current.push(b as char);
            i = i.saturating_add(1);
        }
    }
    parts.push(current);

    if parts.len() < 2 {
        return None;
    }

    let global = parts.get(2).is_some_and(|f| f.contains('g'));
    let pat = parts.first().cloned().unwrap_or_default();
    let repl = parts.get(1).cloned().unwrap_or_default();
    Some((pat, repl, global))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- nth_line ----------------

    #[test]
    fn nth_line_basic() {
        let b = lines(&["a", "b", "c"]);
        assert_eq!(nth_line(&b, 1), Some("a"));
        assert_eq!(nth_line(&b, 3), Some("c"));
    }

    #[test]
    fn nth_line_zero_is_none() {
        let b = lines(&["a"]);
        assert_eq!(nth_line(&b, 0), None);
    }

    #[test]
    fn nth_line_out_of_range() {
        let b = lines(&["a"]);
        assert_eq!(nth_line(&b, 5), None);
    }

    // ---------------- insert_lines_at ----------------

    #[test]
    fn insert_at_zero_is_prepend() {
        let mut b = lines(&["a", "b"]);
        insert_lines_at(&mut b, 0, &lines(&["X", "Y"]));
        assert_eq!(b, vec!["X", "Y", "a", "b"]);
    }

    #[test]
    fn insert_at_end_is_append() {
        let mut b = lines(&["a", "b"]);
        insert_lines_at(&mut b, 2, &lines(&["Z"]));
        assert_eq!(b, vec!["a", "b", "Z"]);
    }

    #[test]
    fn insert_at_middle() {
        let mut b = lines(&["a", "b", "c"]);
        insert_lines_at(&mut b, 1, &lines(&["X"]));
        assert_eq!(b, vec!["a", "X", "b", "c"]);
    }

    #[test]
    fn insert_at_out_of_range_clamps_to_end() {
        let mut b = lines(&["a"]);
        insert_lines_at(&mut b, 99, &lines(&["X"]));
        assert_eq!(b, vec!["a", "X"]);
    }

    #[test]
    fn insert_nothing_is_noop() {
        let mut b = lines(&["a"]);
        insert_lines_at(&mut b, 0, &[]);
        assert_eq!(b, vec!["a"]);
    }

    // ---------------- delete_lines ----------------

    #[test]
    fn delete_single() {
        let mut b = lines(&["a", "b", "c"]);
        let cur = delete_lines(&mut b, 2, 2);
        assert_eq!(b, vec!["a", "c"]);
        assert_eq!(cur, 2);
    }

    #[test]
    fn delete_range() {
        let mut b = lines(&["a", "b", "c", "d"]);
        let cur = delete_lines(&mut b, 2, 3);
        assert_eq!(b, vec!["a", "d"]);
        assert_eq!(cur, 2);
    }

    #[test]
    fn delete_last_line_current_clamps_to_buf_len() {
        let mut b = lines(&["a", "b"]);
        let cur = delete_lines(&mut b, 2, 2);
        assert_eq!(b, vec!["a"]);
        assert_eq!(cur, 1, "deleting last line leaves current = new len");
    }

    #[test]
    fn delete_all() {
        let mut b = lines(&["a", "b"]);
        let cur = delete_lines(&mut b, 1, 2);
        assert!(b.is_empty());
        assert_eq!(cur, 0);
    }

    // ---------------- change_lines ----------------

    #[test]
    fn change_replaces_range() {
        let mut b = lines(&["a", "b", "c"]);
        let cur = change_lines(&mut b, 2, 2, &lines(&["X", "Y"]));
        assert_eq!(b, vec!["a", "X", "Y", "c"]);
        assert_eq!(cur, 3);
    }

    #[test]
    fn change_with_no_replacement_is_delete() {
        let mut b = lines(&["a", "b"]);
        let cur = change_lines(&mut b, 1, 2, &[]);
        assert!(b.is_empty());
        assert_eq!(cur, 0);
    }

    // ---------------- substitute_line ----------------

    #[test]
    fn substitute_first_only() {
        assert_eq!(
            substitute_line("foo foo foo", "foo", "bar", false),
            "bar foo foo"
        );
    }

    #[test]
    fn substitute_global() {
        assert_eq!(
            substitute_line("foo foo foo", "foo", "bar", true),
            "bar bar bar"
        );
    }

    #[test]
    fn substitute_no_match_passthrough() {
        assert_eq!(substitute_line("hello", "X", "Y", true), "hello");
    }

    #[test]
    fn substitute_empty_pattern_is_noop() {
        // Avoids infinite loop / matches-everywhere semantics of String::replace("").
        assert_eq!(substitute_line("hello", "", "X", true), "hello");
    }

    // ---------------- parse_substitute ----------------

    #[test]
    fn parse_sub_simple() {
        let (pat, repl, global) = parse_substitute("/foo/bar/").unwrap();
        assert_eq!(pat, "foo");
        assert_eq!(repl, "bar");
        assert!(!global);
    }

    #[test]
    fn parse_sub_global() {
        let (_, _, global) = parse_substitute("/x/y/g").unwrap();
        assert!(global);
    }

    #[test]
    fn parse_sub_empty_pattern() {
        let (pat, repl, _) = parse_substitute("///").unwrap();
        assert_eq!(pat, "");
        assert_eq!(repl, "");
    }

    #[test]
    fn parse_sub_escape_delim() {
        let (pat, repl, _) = parse_substitute(r"/a\/b/c/").unwrap();
        assert_eq!(pat, "a/b");
        assert_eq!(repl, "c");
    }

    #[test]
    fn parse_sub_arbitrary_delim() {
        let (pat, repl, _) = parse_substitute("|foo|bar|").unwrap();
        assert_eq!(pat, "foo");
        assert_eq!(repl, "bar");
    }

    #[test]
    fn parse_sub_empty_arg_is_none() {
        assert!(parse_substitute("").is_none());
    }

    #[test]
    fn parse_sub_too_few_parts_is_none() {
        // Just a delimiter (no replacement segment): can't be a sub.
        assert!(parse_substitute("/foo").is_none());
    }

    // ---------------- parse_command ----------------

    #[test]
    fn parse_empty_input_uses_next_current() {
        let p = parse_command("", 5, 10);
        assert_eq!(p.addr_start, 6);
        assert_eq!(p.addr_end, 6);
        assert_eq!(p.cmd, '\0');
        assert!(p.arg.is_empty());
    }

    #[test]
    fn parse_bare_command_uses_current() {
        let p = parse_command("p", 3, 10);
        assert_eq!(p.addr_start, 3);
        assert_eq!(p.addr_end, 3);
        assert_eq!(p.cmd, 'p');
    }

    #[test]
    fn parse_line_number_only() {
        let p = parse_command("5", 1, 10);
        assert_eq!(p.addr_start, 5);
        assert_eq!(p.addr_end, 5);
        assert_eq!(p.cmd, '\0');
    }

    #[test]
    fn parse_line_number_with_command() {
        let p = parse_command("5p", 1, 10);
        assert_eq!(p.addr_start, 5);
        assert_eq!(p.cmd, 'p');
    }

    #[test]
    fn parse_range() {
        let p = parse_command("2,4d", 1, 10);
        assert_eq!(p.addr_start, 2);
        assert_eq!(p.addr_end, 4);
        assert_eq!(p.cmd, 'd');
    }

    #[test]
    fn parse_dot_is_current() {
        let p = parse_command(".p", 7, 10);
        assert_eq!(p.addr_start, 7);
    }

    #[test]
    fn parse_dollar_is_last_line() {
        let p = parse_command("$p", 1, 42);
        assert_eq!(p.addr_start, 42);
    }

    #[test]
    fn parse_substitute_argument_kept() {
        let p = parse_command("s/foo/bar/", 1, 10);
        assert_eq!(p.cmd, 's');
        assert_eq!(p.arg, "/foo/bar/");
    }

    #[test]
    fn parse_write_with_filename() {
        let p = parse_command("w out.txt", 1, 10);
        assert_eq!(p.cmd, 'w');
        assert_eq!(p.arg, "out.txt");
    }

    #[test]
    fn parse_range_with_dollar() {
        let p = parse_command("1,$p", 1, 7);
        assert_eq!(p.addr_start, 1);
        assert_eq!(p.addr_end, 7);
    }
}
