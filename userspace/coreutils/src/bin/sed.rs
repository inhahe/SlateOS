//! sed — stream editor.
//!
//! Usage: sed [-n] [-e SCRIPT] [-i] [SCRIPT] [FILE...]
//!   -n         suppress automatic printing of pattern space
//!   -e SCRIPT  add SCRIPT to the commands
//!   -i         edit files in place
//!
//! Supported commands:
//!   s/PATTERN/REPLACEMENT/[g]  substitute
//!   d                          delete pattern space
//!   p                          print pattern space
//!   q                          quit
//!   N,M command                address range (line numbers)
//!   /PATTERN/ command          address by regex match

use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct SedArgs {
    suppress: bool,
    in_place: bool,
    scripts: Vec<String>,
    files: Vec<String>,
}

/// Parse sed's argv.  Recognises `-n`, `-i`, and `-e SCRIPT`.  The first
/// bare argument (not a flag and not the value of `-e`) is treated as
/// the script; any subsequent bare arguments are files.  Missing `-e`
/// value is reported as an error.
fn parse_args(args: &[String]) -> Result<SedArgs, String> {
    let mut out = SedArgs::default();
    let mut saw_script = false;
    let mut i: usize = 0;

    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-n" => out.suppress = true,
            "-i" => out.in_place = true,
            "-e" => {
                i = i.saturating_add(1);
                let v = args
                    .get(i)
                    .ok_or_else(|| "option -e requires an argument".to_string())?;
                out.scripts.push(v.clone());
                saw_script = true;
            }
            other => {
                if !saw_script && out.scripts.is_empty() {
                    out.scripts.push(other.to_string());
                    saw_script = true;
                } else {
                    out.files.push(other.to_string());
                }
            }
        }
        i = i.saturating_add(1);
    }

    Ok(out)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sed: {e}");
            process::exit(1);
        }
    };

    if parsed.scripts.is_empty() {
        eprintln!("sed: no script specified");
        process::exit(1);
    }

    let commands: Vec<SedCommand> = parsed.scripts.iter().flat_map(|s| parse_script(s)).collect();

    if parsed.files.is_empty() {
        parsed.files.push("-".to_string());
    }

    for path in &parsed.files {
        if parsed.in_place && path != "-" {
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("sed: {path}: {e}");
                    continue;
                }
            };
            let result = process_text(&content, &commands, parsed.suppress);
            if let Err(e) = fs::write(path, &result) {
                eprintln!("sed: {path}: {e}");
            }
        } else {
            let reader: Box<dyn Read> = if path == "-" {
                Box::new(io::stdin())
            } else {
                match fs::File::open(path) {
                    Ok(f) => Box::new(f),
                    Err(e) => {
                        eprintln!("sed: {path}: {e}");
                        continue;
                    }
                }
            };

            let stdout = io::stdout();
            let mut out = stdout.lock();
            let buf = BufReader::new(reader);
            let mut line_num: usize = 0;

            for line_result in buf.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => break,
                };
                line_num = line_num.saturating_add(1);

                let (output, should_quit) =
                    process_line(&line, &commands, parsed.suppress, line_num);
                if let Some(text) = output {
                    let _ = writeln!(out, "{text}");
                }
                if should_quit {
                    break;
                }
            }
        }
    }
}

/// Run `commands` against `content` as a single string and return the
/// resulting text.  Used by `-i` (in-place) mode and unit tests.
fn process_text(content: &str, commands: &[SedCommand], suppress: bool) -> String {
    let mut result = String::new();
    for (i, line) in content.lines().enumerate() {
        let (output, should_quit) =
            process_line(line, commands, suppress, i.saturating_add(1));
        if let Some(text) = output {
            result.push_str(&text);
            result.push('\n');
        }
        if should_quit {
            break;
        }
    }
    result
}

/// Apply each command in sequence to one input line.  Returns the
/// possibly-modified line to emit (or None if the line was deleted /
/// `-n` suppressed printing) and a quit flag.
fn process_line(
    line: &str,
    commands: &[SedCommand],
    suppress: bool,
    line_num: usize,
) -> (Option<String>, bool) {
    let mut pattern = line.to_string();
    let mut deleted = false;
    let mut printed = false;
    let mut quit = false;

    for cmd in commands {
        if deleted || quit {
            break;
        }
        if !address_matches(&cmd.address, line_num, &pattern) {
            continue;
        }

        match &cmd.action {
            Action::Substitute {
                pattern: pat,
                replacement,
                global,
            } => {
                pattern = substitute(&pattern, pat, replacement, *global);
            }
            Action::Delete => {
                deleted = true;
            }
            Action::Print => {
                printed = true;
            }
            Action::Quit => {
                quit = true;
            }
        }
    }

    let output = if deleted {
        None
    } else if printed || !suppress {
        Some(pattern)
    } else {
        None
    };

    (output, quit)
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Address {
    None,
    Line(usize),
    Pattern(String),
    Range(Box<Address>, Box<Address>),
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Action {
    Substitute {
        pattern: String,
        replacement: String,
        global: bool,
    },
    Delete,
    Print,
    Quit,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct SedCommand {
    address: Address,
    action: Action,
}

fn parse_script(script: &str) -> Vec<SedCommand> {
    let mut commands = Vec::new();

    // Split on semicolons or newlines for multiple commands
    for part in script.split([';', '\n']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(cmd) = parse_command(part) {
            commands.push(cmd);
        }
    }

    commands
}

fn parse_command(s: &str) -> Option<SedCommand> {
    let bytes = s.as_bytes();
    let mut pos = 0;

    // Parse address
    let (address, new_pos) = parse_address(s, pos);
    pos = new_pos;

    // Range: ADDR1,ADDR2 ACTION
    if bytes.get(pos).copied() == Some(b',') {
        pos = pos.saturating_add(1);
        let (addr2, new_pos2) = parse_address(s, pos);
        pos = new_pos2;
        let address = Address::Range(Box::new(address), Box::new(addr2));
        let action = parse_action(s.get(pos..).unwrap_or(""))?;
        return Some(SedCommand { address, action });
    }

    let action = parse_action(s.get(pos..).unwrap_or(""))?;
    Some(SedCommand { address, action })
}

fn parse_address(s: &str, mut pos: usize) -> (Address, usize) {
    let bytes = s.as_bytes();

    let Some(&first) = bytes.get(pos) else {
        return (Address::None, pos);
    };

    // Line number address
    if first.is_ascii_digit() {
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos = pos.saturating_add(1);
        }
        let n: usize = s.get(start..pos).and_then(|x| x.parse().ok()).unwrap_or(0);
        return (Address::Line(n), pos);
    }

    // Pattern address /regex/
    if first == b'/' {
        pos = pos.saturating_add(1);
        let start = pos;
        while let Some(&b) = bytes.get(pos) {
            if b == b'/' {
                break;
            }
            if b == b'\\' && bytes.get(pos.saturating_add(1)).is_some() {
                pos = pos.saturating_add(1);
            }
            pos = pos.saturating_add(1);
        }
        let pattern = s.get(start..pos).unwrap_or("").to_string();
        if bytes.get(pos).is_some() {
            pos = pos.saturating_add(1); // skip closing /
        }
        return (Address::Pattern(pattern), pos);
    }

    (Address::None, pos)
}

fn parse_action(s: &str) -> Option<Action> {
    let s = s.trim();
    let first = *s.as_bytes().first()?;
    match first {
        b's' => {
            // s/pattern/replacement/[flags]
            let delim = *s.as_bytes().get(1)?;
            let rest = s.get(2..).unwrap_or("");
            let parts: Vec<&str> = split_delim(rest, delim as char);
            if parts.len() < 2 {
                return None;
            }
            let pat = parts.first().copied().unwrap_or("").to_string();
            let repl = parts.get(1).copied().unwrap_or("").to_string();
            let global = parts.get(2).is_some_and(|flags| flags.contains('g'));
            Some(Action::Substitute {
                pattern: pat,
                replacement: repl,
                global,
            })
        }
        b'd' => Some(Action::Delete),
        b'p' => Some(Action::Print),
        b'q' => Some(Action::Quit),
        _ => None,
    }
}

fn split_delim(s: &str, delim: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();
    let delim_byte = delim as u8;
    let mut i = 0;

    while let Some(&b) = bytes.get(i) {
        if b == b'\\' && bytes.get(i.saturating_add(1)).is_some() {
            i = i.saturating_add(2);
            continue;
        }
        if b == delim_byte {
            if let Some(piece) = s.get(start..i) {
                parts.push(piece);
            }
            start = i.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    if let Some(piece) = s.get(start..) {
        parts.push(piece);
    }
    parts
}

fn address_matches(addr: &Address, line_num: usize, line: &str) -> bool {
    match addr {
        Address::None => true,
        Address::Line(n) => line_num == *n,
        Address::Pattern(pat) => simple_regex_match(pat, line),
        Address::Range(start, end) => {
            // Note: proper range tracking requires state across lines.
            // This simplified version matches lines that match either endpoint.
            address_matches(start, line_num, line) || address_matches(end, line_num, line)
        }
    }
}

/// Simple regex match — supports literal chars, `.` (any), `*` (zero or more),
/// `^` (start), `$` (end).
fn simple_regex_match(pattern: &str, text: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix('^') {
        return regex_match_at(rest, text, 0);
    }
    for i in 0..=text.chars().count() {
        if regex_match_at(pattern, text, i) {
            return true;
        }
    }
    false
}

fn regex_match_at(pattern: &str, text: &str, start: usize) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    regex_inner(&pat, &txt, 0, start)
}

fn regex_inner(pat: &[char], txt: &[char], pi: usize, ti: usize) -> bool {
    if pi == pat.len() {
        return true;
    }

    let p = match pat.get(pi) {
        Some(&c) => c,
        None => return true,
    };

    if p == '$' && pi.saturating_add(1) == pat.len() {
        return ti == txt.len();
    }

    // Check for * quantifier following pat[pi]
    if pat.get(pi.saturating_add(1)).copied() == Some('*') {
        let mut t = ti;
        loop {
            if regex_inner(pat, txt, pi.saturating_add(2), t) {
                return true;
            }
            let Some(&tc) = txt.get(t) else {
                break;
            };
            if !char_matches(p, tc) {
                break;
            }
            t = t.saturating_add(1);
        }
        return false;
    }

    let Some(&tc) = txt.get(ti) else {
        return false;
    };
    if char_matches(p, tc) {
        regex_inner(pat, txt, pi.saturating_add(1), ti.saturating_add(1))
    } else {
        false
    }
}

fn char_matches(pat_char: char, text_char: char) -> bool {
    pat_char == '.' || pat_char == text_char
}

/// Perform substitution using simple regex.
fn substitute(text: &str, pattern: &str, replacement: &str, global: bool) -> String {
    if global {
        let mut result = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if let Some(end) = find_match(pattern, text, i) {
                result.push_str(replacement);
                if end <= i {
                    // Zero-width match: emit one char and advance to avoid infinite loop.
                    if let Some(&c) = chars.get(i) {
                        result.push(c);
                    }
                    i = i.saturating_add(1);
                } else {
                    i = end;
                }
            } else {
                if let Some(&c) = chars.get(i) {
                    result.push(c);
                }
                i = i.saturating_add(1);
            }
        }
        result
    } else {
        let chars: Vec<char> = text.chars().collect();
        for i in 0..=chars.len() {
            if let Some(end) = find_match(pattern, text, i) {
                let mut result = String::new();
                result.push_str(text.get(..byte_index(text, i)).unwrap_or(""));
                result.push_str(replacement);
                result.push_str(text.get(byte_index(text, end)..).unwrap_or(""));
                return result;
            }
        }
        text.to_string()
    }
}

fn find_match(pattern: &str, text: &str, start: usize) -> Option<usize> {
    let txt: Vec<char> = text.chars().collect();
    for end in start..=txt.len() {
        let substr: String = txt.get(start..end)?.iter().collect();
        if simple_regex_match(&format!("^{pattern}$"), &substr) {
            return Some(end);
        }
    }
    None
}

fn byte_index(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map_or(s.len(), |(idx, _)| idx)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_empty_args() {
        let a = parse_args(&s(&[])).unwrap();
        assert_eq!(a, SedArgs::default());
    }

    #[test]
    fn parse_script_only() {
        let a = parse_args(&s(&["s/a/b/"])).unwrap();
        assert_eq!(a.scripts, vec!["s/a/b/"]);
        assert!(a.files.is_empty());
    }

    #[test]
    fn parse_script_and_file() {
        let a = parse_args(&s(&["s/a/b/", "input.txt"])).unwrap();
        assert_eq!(a.scripts, vec!["s/a/b/"]);
        assert_eq!(a.files, vec!["input.txt"]);
    }

    #[test]
    fn parse_dash_n_suppresses() {
        let a = parse_args(&s(&["-n", "p"])).unwrap();
        assert!(a.suppress);
        assert_eq!(a.scripts, vec!["p"]);
    }

    #[test]
    fn parse_dash_i_in_place() {
        let a = parse_args(&s(&["-i", "s/a/b/", "file"])).unwrap();
        assert!(a.in_place);
        assert_eq!(a.scripts, vec!["s/a/b/"]);
        assert_eq!(a.files, vec!["file"]);
    }

    #[test]
    fn parse_dash_e_consumes_next_arg() {
        let a = parse_args(&s(&["-e", "s/a/b/", "-e", "p", "file"])).unwrap();
        assert_eq!(a.scripts, vec!["s/a/b/", "p"]);
        assert_eq!(a.files, vec!["file"]);
    }

    #[test]
    fn parse_dash_e_missing_value_errors() {
        let err = parse_args(&s(&["-e"])).unwrap_err();
        assert!(err.contains("-e requires"));
    }

    #[test]
    fn parse_multiple_files() {
        let a = parse_args(&s(&["p", "a.txt", "b.txt", "c.txt"])).unwrap();
        assert_eq!(a.files, vec!["a.txt", "b.txt", "c.txt"]);
    }

    // ---------------- parse_script / parse_command ----------------

    #[test]
    fn parse_script_single_substitute() {
        let cmds = parse_script("s/foo/bar/");
        assert_eq!(cmds.len(), 1);
        if let Action::Substitute { pattern, replacement, global } = &cmds[0].action {
            assert_eq!(pattern, "foo");
            assert_eq!(replacement, "bar");
            assert!(!global);
        } else {
            panic!("expected substitute, got {:?}", cmds[0].action);
        }
    }

    #[test]
    fn parse_script_global_substitute() {
        let cmds = parse_script("s/x/y/g");
        if let Action::Substitute { global, .. } = &cmds[0].action {
            assert!(*global);
        } else {
            panic!("expected substitute");
        }
    }

    #[test]
    fn parse_script_delete() {
        let cmds = parse_script("d");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].action, Action::Delete);
    }

    #[test]
    fn parse_script_print_quit() {
        let cmds = parse_script("p");
        assert_eq!(cmds[0].action, Action::Print);
        let cmds = parse_script("q");
        assert_eq!(cmds[0].action, Action::Quit);
    }

    #[test]
    fn parse_script_multiple_commands_semicolon() {
        let cmds = parse_script("p;d");
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn parse_script_line_address() {
        let cmds = parse_script("3d");
        assert_eq!(cmds[0].address, Address::Line(3));
        assert_eq!(cmds[0].action, Action::Delete);
    }

    #[test]
    fn parse_script_regex_address() {
        let cmds = parse_script("/foo/d");
        assert_eq!(cmds[0].address, Address::Pattern("foo".to_string()));
    }

    #[test]
    fn parse_script_range_address() {
        let cmds = parse_script("1,5d");
        if let Address::Range(start, end) = &cmds[0].address {
            assert_eq!(**start, Address::Line(1));
            assert_eq!(**end, Address::Line(5));
        } else {
            panic!("expected range");
        }
    }

    #[test]
    fn parse_script_garbage_ignored() {
        let cmds = parse_script("Z");
        assert!(cmds.is_empty());
    }

    // ---------------- split_delim ----------------

    #[test]
    fn split_delim_basic() {
        assert_eq!(split_delim("a/b/c", '/'), vec!["a", "b", "c"]);
    }

    #[test]
    fn split_delim_empty_parts() {
        assert_eq!(split_delim("//", '/'), vec!["", "", ""]);
    }

    #[test]
    fn split_delim_escaped() {
        // Backslash escapes the delimiter so it doesn't split.
        let parts = split_delim(r"a\/b/c", '/');
        assert_eq!(parts, vec![r"a\/b", "c"]);
    }

    // ---------------- simple_regex_match ----------------

    #[test]
    fn regex_literal_match() {
        assert!(simple_regex_match("foo", "foobar"));
        assert!(!simple_regex_match("foo", "bar"));
    }

    #[test]
    fn regex_anchored_start() {
        assert!(simple_regex_match("^foo", "foobar"));
        assert!(!simple_regex_match("^foo", "barfoo"));
    }

    #[test]
    fn regex_anchored_end() {
        assert!(simple_regex_match("bar$", "foobar"));
        assert!(!simple_regex_match("bar$", "barfoo"));
    }

    #[test]
    fn regex_anchored_both() {
        assert!(simple_regex_match("^foo$", "foo"));
        assert!(!simple_regex_match("^foo$", "foobar"));
    }

    #[test]
    fn regex_dot_any_char() {
        assert!(simple_regex_match("a.c", "abc"));
        assert!(simple_regex_match("a.c", "axc"));
    }

    #[test]
    fn regex_star_zero_or_more() {
        assert!(simple_regex_match("ab*c", "ac"));
        assert!(simple_regex_match("ab*c", "abc"));
        assert!(simple_regex_match("ab*c", "abbbbc"));
    }

    #[test]
    fn regex_dot_star() {
        assert!(simple_regex_match(".*", ""));
        assert!(simple_regex_match(".*", "anything"));
    }

    // ---------------- substitute ----------------

    #[test]
    fn substitute_first_only() {
        assert_eq!(substitute("foo foo foo", "foo", "bar", false), "bar foo foo");
    }

    #[test]
    fn substitute_global() {
        assert_eq!(substitute("foo foo foo", "foo", "bar", true), "bar bar bar");
    }

    #[test]
    fn substitute_no_match() {
        assert_eq!(substitute("hello", "xyz", "abc", true), "hello");
    }

    #[test]
    fn substitute_empty_text() {
        assert_eq!(substitute("", "foo", "bar", false), "");
    }

    // ---------------- process_line / process_text ----------------

    #[test]
    fn process_line_substitute_default_prints() {
        let cmds = parse_script("s/a/b/");
        let (out, quit) = process_line("ax", &cmds, false, 1);
        assert_eq!(out, Some("bx".to_string()));
        assert!(!quit);
    }

    #[test]
    fn process_line_delete_returns_none() {
        let cmds = parse_script("d");
        let (out, quit) = process_line("anything", &cmds, false, 1);
        assert_eq!(out, None);
        assert!(!quit);
    }

    #[test]
    fn process_line_quit_sets_flag() {
        let cmds = parse_script("q");
        let (_, quit) = process_line("x", &cmds, false, 1);
        assert!(quit);
    }

    #[test]
    fn process_line_suppress_only_prints_after_p() {
        let cmds = parse_script("p");
        let (out, _) = process_line("hi", &cmds, true, 1);
        assert_eq!(out, Some("hi".to_string()));

        let cmds = parse_script("s/a/b/");
        let (out, _) = process_line("apple", &cmds, true, 1);
        // -n suppresses default print; substitute doesn't toggle p.
        assert_eq!(out, None);
    }

    #[test]
    fn process_line_address_line_number() {
        let cmds = parse_script("2d");
        let (out1, _) = process_line("a", &cmds, false, 1);
        let (out2, _) = process_line("b", &cmds, false, 2);
        let (out3, _) = process_line("c", &cmds, false, 3);
        assert_eq!(out1, Some("a".to_string()));
        assert_eq!(out2, None);
        assert_eq!(out3, Some("c".to_string()));
    }

    #[test]
    fn process_text_substitute_each_line() {
        let cmds = parse_script("s/o/0/g");
        let out = process_text("foo\nbar\nfoo\n", &cmds, false);
        assert_eq!(out, "f00\nbar\nf00\n");
    }

    #[test]
    fn process_text_quit_stops_processing() {
        let cmds = parse_script("2q");
        let out = process_text("a\nb\nc\nd\n", &cmds, false);
        // Line 1 prints, line 2 prints then quit fires before line 3.
        assert_eq!(out, "a\nb\n");
    }

    #[test]
    fn process_text_suppress_then_p_prints_pattern() {
        let cmds = parse_script("p");
        let out = process_text("x\ny\n", &cmds, true);
        assert_eq!(out, "x\ny\n");
    }
}
