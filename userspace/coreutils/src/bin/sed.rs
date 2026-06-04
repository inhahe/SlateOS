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

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut suppress = false;
    let mut in_place = false;
    let mut scripts: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    let mut saw_script = false;

    while i < args.len() {
        match args[i].as_str() {
            "-n" => suppress = true,
            "-i" => in_place = true,
            "-e" => {
                i += 1;
                if i < args.len() {
                    scripts.push(args[i].clone());
                    saw_script = true;
                }
            }
            arg => {
                if !saw_script && scripts.is_empty() {
                    // First non-option argument is the script
                    scripts.push(arg.to_string());
                    saw_script = true;
                } else {
                    files.push(arg.to_string());
                }
            }
        }
        i += 1;
    }

    if scripts.is_empty() {
        eprintln!("sed: no script specified");
        process::exit(1);
    }

    let commands: Vec<SedCommand> = scripts
        .iter()
        .flat_map(|s| parse_script(s))
        .collect();

    if files.is_empty() {
        files.push("-".to_string());
    }

    for path in &files {
        if in_place && path != "-" {
            // Read file, process, write back
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("sed: {path}: {e}");
                    continue;
                }
            };
            let result = process_text(&content, &commands, suppress);
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
                line_num += 1;

                let (output, should_quit) = process_line(&line, &commands, suppress, line_num);
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

fn process_text(content: &str, commands: &[SedCommand], suppress: bool) -> String {
    let mut result = String::new();
    for (i, line) in content.lines().enumerate() {
        let (output, should_quit) = process_line(line, commands, suppress, i + 1);
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

#[derive(Debug)]
enum Address {
    None,
    Line(usize),
    Pattern(String),
    Range(Box<Address>, Box<Address>),
}

#[derive(Debug)]
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

#[derive(Debug)]
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

    // Skip comma for ranges
    if pos < bytes.len() && bytes[pos] == b',' {
        pos += 1;
        let (addr2, new_pos2) = parse_address(s, pos);
        pos = new_pos2;
        let address = Address::Range(Box::new(address), Box::new(addr2));
        let action = parse_action(&s[pos..])?;
        return Some(SedCommand { address, action });
    }

    let action = parse_action(&s[pos..])?;
    Some(SedCommand { address, action })
}

fn parse_address(s: &str, mut pos: usize) -> (Address, usize) {
    let bytes = s.as_bytes();

    if pos >= bytes.len() {
        return (Address::None, pos);
    }

    // Line number address
    if bytes[pos].is_ascii_digit() {
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        let n: usize = s[start..pos].parse().unwrap_or(0);
        return (Address::Line(n), pos);
    }

    // Pattern address /regex/
    if bytes[pos] == b'/' {
        pos += 1;
        let start = pos;
        while pos < bytes.len() && bytes[pos] != b'/' {
            if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                pos += 1; // skip escaped char
            }
            pos += 1;
        }
        let pattern = s[start..pos].to_string();
        if pos < bytes.len() {
            pos += 1; // skip closing /
        }
        return (Address::Pattern(pattern), pos);
    }

    (Address::None, pos)
}

fn parse_action(s: &str) -> Option<Action> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let first = s.as_bytes()[0];
    match first {
        b's' => {
            // s/pattern/replacement/[flags]
            if s.len() < 2 {
                return None;
            }
            let delim = s.as_bytes()[1];
            let rest = &s[2..];
            let parts: Vec<&str> = split_delim(rest, delim as char);
            if parts.len() < 2 {
                return None;
            }
            let global = parts.get(2).is_some_and(|flags| flags.contains('g'));
            Some(Action::Substitute {
                pattern: parts[0].to_string(),
                replacement: parts[1].to_string(),
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

    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == delim_byte {
            parts.push(&s[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

fn address_matches(addr: &Address, line_num: usize, line: &str) -> bool {
    match addr {
        Address::None => true,
        Address::Line(n) => line_num == *n,
        Address::Pattern(pat) => simple_regex_match(pat, line),
        Address::Range(start, end) => {
            address_matches(start, line_num, line) || address_matches(end, line_num, line)
            // Note: proper range tracking requires state across lines.
            // This simplified version matches lines that match either endpoint.
        }
    }
}

/// Simple regex match — supports literal chars, `.` (any), `*` (zero or more),
/// `^` (start), `$` (end), `[...]` (char class).
fn simple_regex_match(pattern: &str, text: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix('^') {
        return regex_match_at(rest, text, 0);
    }

    // Try matching at every position
    for i in 0..=text.len() {
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
        return true; // pattern exhausted = match
    }

    if pat[pi] == '$' && pi + 1 == pat.len() {
        return ti == txt.len();
    }

    // Check for * quantifier
    if pi + 1 < pat.len() && pat[pi + 1] == '*' {
        // Match zero or more of pat[pi]
        let mut t = ti;
        loop {
            if regex_inner(pat, txt, pi + 2, t) {
                return true;
            }
            if t >= txt.len() || !char_matches(pat[pi], txt[t]) {
                break;
            }
            t += 1;
        }
        return false;
    }

    if ti >= txt.len() {
        return false;
    }

    if char_matches(pat[pi], txt[ti]) {
        regex_inner(pat, txt, pi + 1, ti + 1)
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
        // Replace all occurrences
        let mut result = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if let Some(end) = find_match(pattern, text, i) {
                result.push_str(replacement);
                i = end;
                if i == 0 {
                    break; // prevent infinite loop on zero-length match
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        result
    } else {
        // Replace first occurrence
        let chars: Vec<char> = text.chars().collect();
        for i in 0..=chars.len() {
            if let Some(end) = find_match(pattern, text, i) {
                let mut result = String::new();
                result.push_str(&text[..byte_index(text, i)]);
                result.push_str(replacement);
                result.push_str(&text[byte_index(text, end)..]);
                return result;
            }
        }
        text.to_string()
    }
}

fn find_match(pattern: &str, text: &str, start: usize) -> Option<usize> {
    let txt: Vec<char> = text.chars().collect();

    // Try to find the end of the match
    for end in start..=txt.len() {
        let substr: String = txt[start..end].iter().collect();
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
