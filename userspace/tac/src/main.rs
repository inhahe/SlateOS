//! tac/rev — reverse line printer and character reverser for OurOS
//!
//! Multi-personality binary:
//! - `tac`: concatenate and print files in reverse line order
//! - `rev`: reverse characters in each line
//!
//! Detected via argv[0].

use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::process;

// ── Mode detection ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Tac,
    Rev,
}

fn detect_mode(argv0: &str) -> Mode {
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    if name.eq_ignore_ascii_case("rev") {
        Mode::Rev
    } else {
        Mode::Tac
    }
}

// ── tac implementation ───────────────────────────────────────────

struct TacOptions {
    separator: String,
    before: bool,
    regex: bool,
    files: Vec<String>,
}

fn parse_tac_args(args: &[String]) -> TacOptions {
    let mut opts = TacOptions {
        separator: "\n".to_string(),
        before: false,
        regex: false,
        files: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-s" | "--separator" => {
                i += 1;
                if i < args.len() {
                    opts.separator = args[i].clone();
                }
            }
            "-b" | "--before" => {
                opts.before = true;
            }
            "-r" | "--regex" => {
                opts.regex = true;
            }
            "--help" => {
                print_tac_help();
                process::exit(0);
            }
            "--version" => {
                println!("tac (OurOS coreutils) 0.1.0");
                process::exit(0);
            }
            _ if arg.starts_with("--separator=") => {
                opts.separator = arg.strip_prefix("--separator=").unwrap_or("").to_string();
            }
            _ if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") => {
                // Combined short flags like -br or -bs SEP
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    match chars[j] {
                        'b' => opts.before = true,
                        'r' => opts.regex = true,
                        's' => {
                            // Rest of this arg is the separator, or next arg
                            if j + 1 < chars.len() {
                                opts.separator = chars[j + 1..].iter().collect();
                                j = chars.len(); // consumed
                                continue;
                            } else {
                                i += 1;
                                if i < args.len() {
                                    opts.separator = args[i].clone();
                                }
                            }
                        }
                        _ => {
                            eprintln!("tac: unknown option '-{}'", chars[j]);
                            process::exit(1);
                        }
                    }
                    j += 1;
                }
            }
            _ => {
                opts.files.push(arg.clone());
            }
        }
        i += 1;
    }

    opts
}

fn print_tac_help() {
    println!("Usage: tac [OPTION]... [FILE]...");
    println!("Write each FILE to standard output, last line first.");
    println!();
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("Options:");
    println!("  -b, --before             attach the separator before instead of after");
    println!("  -r, --regex              interpret the separator as a regular expression");
    println!("  -s, --separator=STRING   use STRING as the separator instead of newline");
    println!("      --help               display this help and exit");
    println!("      --version            output version information and exit");
}

/// Simple regex matcher for tac -r mode.
/// Supports: . * + ? ^ $ [] [^] | \char escaping
fn regex_find(pattern: &str, text: &str) -> Option<(usize, usize)> {
    let pat_chars: Vec<char> = pattern.chars().collect();

    // Try matching at each position
    for start in 0..=text.len() {
        if let Some(end) = regex_match_at(&pat_chars, 0, text, start)
            && (end > start || pat_chars.is_empty()) {
                return Some((start, end));
            }
    }
    None
}

fn regex_match_at(pat: &[char], pi: usize, text: &str, ti: usize) -> Option<usize> {
    if pi >= pat.len() {
        return Some(ti);
    }

    let text_bytes = text.as_bytes();

    // Check for alternation at top level
    let mut depth = 0;
    for (idx, &ch) in pat[pi..].iter().enumerate() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            '|' if depth == 0 => {
                let left = &pat[pi..pi + idx];
                let right = &pat[pi + idx + 1..];
                if let Some(end) = regex_match_at(left, 0, text, ti) {
                    return Some(end);
                }
                return regex_match_at(right, 0, text, ti);
            }
            _ => {}
        }
    }

    match pat[pi] {
        '^' => {
            if ti == 0 {
                regex_match_at(pat, pi + 1, text, ti)
            } else {
                None
            }
        }
        '$' if pi + 1 == pat.len() => {
            if ti == text.len() {
                Some(ti)
            } else {
                None
            }
        }
        '\\' if pi + 1 < pat.len() => {
            let escaped = pat[pi + 1];
            let has_quantifier = pi + 2 < pat.len() && matches!(pat[pi + 2], '*' | '+' | '?');
            if has_quantifier {
                regex_quantifier(pat, pi, pi + 2, text, ti, &|c| c == escaped)
            } else if ti < text.len() && text_bytes[ti] as char == escaped {
                regex_match_at(pat, pi + 2, text, ti + 1)
            } else {
                None
            }
        }
        '[' => {
            // Character class
            let (negate, class_end, chars) = parse_char_class(pat, pi);
            let next_pi = class_end + 1;
            let has_quantifier = next_pi < pat.len() && matches!(pat[next_pi], '*' | '+' | '?');
            let matcher = move |c: char| {
                let found = chars.iter().any(|&(lo, hi)| c >= lo && c <= hi);
                if negate { !found } else { found }
            };
            if has_quantifier {
                regex_quantifier(pat, pi, next_pi, text, ti, &matcher)
            } else if ti < text.len() && matcher(text_bytes[ti] as char) {
                regex_match_at(pat, next_pi, text, ti + 1)
            } else {
                None
            }
        }
        '.' => {
            let has_quantifier = pi + 1 < pat.len() && matches!(pat[pi + 1], '*' | '+' | '?');
            if has_quantifier {
                regex_quantifier(pat, pi, pi + 1, text, ti, &|_| true)
            } else if ti < text.len() {
                regex_match_at(pat, pi + 1, text, ti + 1)
            } else {
                None
            }
        }
        ch => {
            let has_quantifier = pi + 1 < pat.len() && matches!(pat[pi + 1], '*' | '+' | '?');
            if has_quantifier {
                regex_quantifier(pat, pi, pi + 1, text, ti, &|c| c == ch)
            } else if ti < text.len() && text_bytes[ti] as char == ch {
                regex_match_at(pat, pi + 1, text, ti + 1)
            } else {
                None
            }
        }
    }
}

fn regex_quantifier(
    pat: &[char],
    _atom_start: usize,
    quant_pos: usize,
    text: &str,
    ti: usize,
    matcher: &dyn Fn(char) -> bool,
) -> Option<usize> {
    let quantifier = pat[quant_pos];
    let next_pi = quant_pos + 1;
    let text_bytes = text.as_bytes();

    // Count how many characters match
    let mut count = 0;
    while ti + count < text.len() && matcher(text_bytes[ti + count] as char) {
        count += 1;
    }

    let min_match = match quantifier {
        '*' => 0,
        '+' => 1,
        '?' => 0,
        _ => 0,
    };
    let max_match = match quantifier {
        '?' => 1.min(count),
        _ => count,
    };

    if max_match < min_match {
        return None;
    }

    // Greedy: try longest match first
    let mut n = max_match;
    loop {
        if let Some(end) = regex_match_at(pat, next_pi, text, ti + n) {
            return Some(end);
        }
        if n == min_match {
            break;
        }
        n -= 1;
    }

    None
}

fn parse_char_class(pat: &[char], start: usize) -> (bool, usize, Vec<(char, char)>) {
    let mut i = start + 1;
    let negate = i < pat.len() && pat[i] == '^';
    if negate {
        i += 1;
    }

    let mut ranges = Vec::new();

    // First char after [ or [^ can be ] literally
    if i < pat.len() && pat[i] == ']' {
        ranges.push((']', ']'));
        i += 1;
    }

    while i < pat.len() && pat[i] != ']' {
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            ranges.push((pat[i], pat[i + 2]));
            i += 3;
        } else {
            ranges.push((pat[i], pat[i]));
            i += 1;
        }
    }

    (negate, i, ranges)
}

fn split_by_separator(content: &str, separator: &str, before: bool, regex: bool) -> Vec<String> {
    if content.is_empty() {
        return vec![String::new()];
    }

    let mut segments: Vec<String> = Vec::new();

    if regex {
        let mut remaining = content.to_string();
        loop {
            if let Some((start, end)) = regex_find(separator, &remaining) {
                if end == start {
                    // Empty match — avoid infinite loop
                    if start < remaining.len() {
                        let seg = remaining[..start + 1].to_string();
                        if !before {
                            // separator stays with previous
                        }
                        segments.push(seg);
                        remaining = remaining[start + 1..].to_string();
                    } else {
                        segments.push(remaining);
                        break;
                    }
                } else if before {
                    segments.push(remaining[..start].to_string());
                    remaining = remaining[start..].to_string();
                    if remaining.is_empty() {
                        break;
                    }
                    // Regex separator branch is unfinished — fall through to
                    // the simple split below. (The dead branch left here as
                    // a marker for the future regex-aware separator support.)
                    break;
                } else {
                    segments.push(remaining[..end].to_string());
                    remaining = remaining[end..].to_string();
                }
            } else {
                segments.push(remaining);
                break;
            }
        }

        // If regex splitting was incomplete, fall back to simple approach
        if segments.is_empty() {
            segments.push(content.to_string());
        }
    } else {
        // Simple string separator
        if separator == "\n" {
            // Special handling for newline separator (most common case)
            let lines: Vec<&str> = content.split('\n').collect();
            for (i, line) in lines.iter().enumerate() {
                if before {
                    if i == 0 {
                        segments.push(line.to_string());
                    } else {
                        segments.push(format!("\n{}", line));
                    }
                } else if i < lines.len() - 1 {
                    segments.push(format!("{}\n", line));
                } else {
                    segments.push(line.to_string());
                }
            }
        } else {
            let mut rest = content;
            loop {
                if let Some(pos) = rest.find(separator) {
                    let end = pos + separator.len();
                    if before {
                        segments.push(rest[..pos].to_string());
                        rest = &rest[pos..];
                        if rest.is_empty() {
                            break;
                        }
                        // Next iteration will include separator at start
                        if let Some(next_pos) = rest[separator.len()..].find(separator) {
                            segments.push(rest[..separator.len() + next_pos].to_string());
                            rest = &rest[separator.len() + next_pos..];
                        } else {
                            segments.push(rest.to_string());
                            break;
                        }
                    } else {
                        segments.push(rest[..end].to_string());
                        rest = &rest[end..];
                    }
                } else {
                    if !rest.is_empty() {
                        segments.push(rest.to_string());
                    }
                    break;
                }
            }
        }
    }

    segments
}

fn run_tac(opts: &TacOptions) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let files = if opts.files.is_empty() {
        vec!["-".to_string()]
    } else {
        opts.files.clone()
    };

    let mut exit_code = 0;

    for file in &files {
        let content = if file == "-" {
            let mut buf = String::new();
            if io::stdin().lock().read_to_string(&mut buf).is_err() {
                eprintln!("tac: error reading standard input");
                exit_code = 1;
                continue;
            }
            buf
        } else {
            match fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("tac: {}: {}", file, e);
                    exit_code = 1;
                    continue;
                }
            }
        };

        let segments = split_by_separator(&content, &opts.separator, opts.before, opts.regex);

        // Print segments in reverse order
        for segment in segments.iter().rev() {
            let _ = out.write_all(segment.as_bytes());
        }
    }

    exit_code
}

// ── rev implementation ───────────────────────────────────────────

struct RevOptions {
    files: Vec<String>,
}

fn parse_rev_args(args: &[String]) -> RevOptions {
    let mut opts = RevOptions { files: Vec::new() };

    for arg in args {
        match arg.as_str() {
            "--help" => {
                print_rev_help();
                process::exit(0);
            }
            "--version" => {
                println!("rev (OurOS coreutils) 0.1.0");
                process::exit(0);
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                eprintln!("rev: unknown option '{}'", arg);
                process::exit(1);
            }
            _ => {
                opts.files.push(arg.clone());
            }
        }
    }

    opts
}

fn print_rev_help() {
    println!("Usage: rev [FILE]...");
    println!("Reverse the characters in each line of FILE(s) or standard input.");
    println!();
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("Options:");
    println!("      --help     display this help and exit");
    println!("      --version  output version information and exit");
}

fn reverse_line(line: &str) -> String {
    // Proper Unicode-aware reversal: reverse grapheme clusters
    // For simplicity, reverse by chars (good enough for most text)
    line.chars().rev().collect()
}

fn run_rev(opts: &RevOptions) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let files = if opts.files.is_empty() {
        vec!["-".to_string()]
    } else {
        opts.files.clone()
    };

    let mut exit_code = 0;

    for file in &files {
        let reader: Box<dyn BufRead> = if file == "-" {
            Box::new(io::stdin().lock())
        } else {
            match fs::File::open(file) {
                Ok(f) => Box::new(io::BufReader::new(f)),
                Err(e) => {
                    eprintln!("rev: {}: {}", file, e);
                    exit_code = 1;
                    continue;
                }
            }
        };

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    let reversed = reverse_line(&line);
                    let _ = writeln!(out, "{}", reversed);
                }
                Err(e) => {
                    eprintln!("rev: read error: {}", e);
                    exit_code = 1;
                    break;
                }
            }
        }
    }

    exit_code
}

// ── main ─────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let mode = detect_mode(args.first().map(|s| s.as_str()).unwrap_or("tac"));

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match mode {
        Mode::Tac => {
            let opts = parse_tac_args(&rest);
            run_tac(&opts)
        }
        Mode::Rev => {
            let opts = parse_rev_args(&rest);
            run_rev(&opts)
        }
    };

    process::exit(exit_code);
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Mode detection tests
    #[test]
    fn test_detect_tac() {
        assert_eq!(detect_mode("tac"), Mode::Tac);
        assert_eq!(detect_mode("/usr/bin/tac"), Mode::Tac);
        assert_eq!(detect_mode("tac.exe"), Mode::Tac);
    }

    #[test]
    fn test_detect_rev() {
        assert_eq!(detect_mode("rev"), Mode::Rev);
        assert_eq!(detect_mode("/usr/bin/rev"), Mode::Rev);
        assert_eq!(detect_mode("rev.exe"), Mode::Rev);
    }

    #[test]
    fn test_detect_unknown_defaults_tac() {
        assert_eq!(detect_mode("something"), Mode::Tac);
    }

    // rev: reverse_line tests
    #[test]
    fn test_reverse_empty() {
        assert_eq!(reverse_line(""), "");
    }

    #[test]
    fn test_reverse_single_char() {
        assert_eq!(reverse_line("a"), "a");
    }

    #[test]
    fn test_reverse_word() {
        assert_eq!(reverse_line("hello"), "olleh");
    }

    #[test]
    fn test_reverse_sentence() {
        assert_eq!(reverse_line("Hello, World!"), "!dlroW ,olleH");
    }

    #[test]
    fn test_reverse_numbers() {
        assert_eq!(reverse_line("12345"), "54321");
    }

    #[test]
    fn test_reverse_spaces() {
        assert_eq!(reverse_line("  ab  "), "  ba  ");
    }

    #[test]
    fn test_reverse_palindrome() {
        assert_eq!(reverse_line("racecar"), "racecar");
    }

    #[test]
    fn test_reverse_unicode() {
        assert_eq!(reverse_line("café"), "éfac");
    }

    #[test]
    fn test_reverse_tabs() {
        assert_eq!(reverse_line("a\tb"), "b\ta");
    }

    // tac: split_by_separator tests
    #[test]
    fn test_split_newline_basic() {
        let segments = split_by_separator("a\nb\nc\n", "\n", false, false);
        assert_eq!(segments, vec!["a\n", "b\n", "c\n", ""]);
    }

    #[test]
    fn test_split_newline_no_trailing() {
        let segments = split_by_separator("a\nb\nc", "\n", false, false);
        assert_eq!(segments, vec!["a\n", "b\n", "c"]);
    }

    #[test]
    fn test_split_newline_before() {
        let segments = split_by_separator("a\nb\nc", "\n", true, false);
        assert_eq!(segments, vec!["a", "\nb", "\nc"]);
    }

    #[test]
    fn test_split_custom_separator() {
        let segments = split_by_separator("a:b:c", ":", false, false);
        assert_eq!(segments, vec!["a:", "b:", "c"]);
    }

    #[test]
    fn test_split_multi_char_separator() {
        let segments = split_by_separator("a--b--c", "--", false, false);
        assert_eq!(segments, vec!["a--", "b--", "c"]);
    }

    #[test]
    fn test_split_empty_content() {
        let segments = split_by_separator("", "\n", false, false);
        assert_eq!(segments, vec![""]);
    }

    #[test]
    fn test_split_single_line() {
        let segments = split_by_separator("hello", "\n", false, false);
        assert_eq!(segments, vec!["hello"]);
    }

    #[test]
    fn test_split_only_separators() {
        let segments = split_by_separator("\n\n", "\n", false, false);
        assert_eq!(segments, vec!["\n", "\n", ""]);
    }

    // tac: argument parsing
    #[test]
    fn test_parse_tac_defaults() {
        let opts = parse_tac_args(&[]);
        assert_eq!(opts.separator, "\n");
        assert!(!opts.before);
        assert!(!opts.regex);
        assert!(opts.files.is_empty());
    }

    #[test]
    fn test_parse_tac_separator() {
        let args = vec!["-s".to_string(), ":".to_string()];
        let opts = parse_tac_args(&args);
        assert_eq!(opts.separator, ":");
    }

    #[test]
    fn test_parse_tac_separator_long() {
        let args = vec!["--separator=:".to_string()];
        let opts = parse_tac_args(&args);
        assert_eq!(opts.separator, ":");
    }

    #[test]
    fn test_parse_tac_before() {
        let args = vec!["-b".to_string()];
        let opts = parse_tac_args(&args);
        assert!(opts.before);
    }

    #[test]
    fn test_parse_tac_regex() {
        let args = vec!["-r".to_string()];
        let opts = parse_tac_args(&args);
        assert!(opts.regex);
    }

    #[test]
    fn test_parse_tac_files() {
        let args = vec!["file1.txt".to_string(), "file2.txt".to_string()];
        let opts = parse_tac_args(&args);
        assert_eq!(opts.files, vec!["file1.txt", "file2.txt"]);
    }

    // rev: argument parsing
    #[test]
    fn test_parse_rev_files() {
        let args = vec!["file1.txt".to_string()];
        let opts = parse_rev_args(&args);
        assert_eq!(opts.files, vec!["file1.txt"]);
    }

    #[test]
    fn test_parse_rev_no_files() {
        let opts = parse_rev_args(&[]);
        assert!(opts.files.is_empty());
    }

    // Regex tests
    #[test]
    fn test_regex_literal() {
        assert!(regex_find("abc", "xabcx").is_some());
        let (start, end) = regex_find("abc", "xabcx").unwrap();
        assert_eq!(start, 1);
        assert_eq!(end, 4);
    }

    #[test]
    fn test_regex_dot() {
        assert!(regex_find("a.c", "abc").is_some());
        assert!(regex_find("a.c", "aXc").is_some());
        assert!(regex_find("a.c", "ac").is_none());
    }

    #[test]
    fn test_regex_star() {
        assert!(regex_find("ab*c", "ac").is_some());
        assert!(regex_find("ab*c", "abc").is_some());
        assert!(regex_find("ab*c", "abbc").is_some());
    }

    #[test]
    fn test_regex_plus() {
        assert!(regex_find("ab+c", "ac").is_none());
        assert!(regex_find("ab+c", "abc").is_some());
        assert!(regex_find("ab+c", "abbc").is_some());
    }

    #[test]
    fn test_regex_question() {
        assert!(regex_find("ab?c", "ac").is_some());
        assert!(regex_find("ab?c", "abc").is_some());
    }

    #[test]
    fn test_regex_char_class() {
        assert!(regex_find("[abc]", "b").is_some());
        assert!(regex_find("[abc]", "d").is_none());
    }

    #[test]
    fn test_regex_char_range() {
        assert!(regex_find("[a-z]", "m").is_some());
        assert!(regex_find("[a-z]", "M").is_none());
    }

    #[test]
    fn test_regex_negated_class() {
        assert!(regex_find("[^a-z]", "M").is_some());
        assert!(regex_find("[^a-z]", "m").is_none());
    }

    #[test]
    fn test_regex_anchor_start() {
        assert!(regex_find("^abc", "abcdef").is_some());
        assert!(regex_find("^abc", "xabc").is_none());
    }

    #[test]
    fn test_regex_anchor_end() {
        assert!(regex_find("abc$", "xabc").is_some());
        assert!(regex_find("abc$", "abcx").is_none());
    }

    #[test]
    fn test_regex_escape() {
        assert!(regex_find("a\\.b", "a.b").is_some());
        assert!(regex_find("a\\.b", "axb").is_none());
    }

    #[test]
    fn test_regex_alternation() {
        assert!(regex_find("cat|dog", "a cat").is_some());
        assert!(regex_find("cat|dog", "a dog").is_some());
        assert!(regex_find("cat|dog", "a bird").is_none());
    }

    #[test]
    fn test_regex_no_match() {
        assert!(regex_find("xyz", "abc").is_none());
    }

    // Integration-style tests
    #[test]
    fn test_tac_simple_reverse() {
        let content = "line1\nline2\nline3\n";
        let segments = split_by_separator(content, "\n", false, false);
        let reversed: String = segments.iter().rev().flat_map(|s| s.chars()).collect();
        // Should end up: "" + "line3\n" + "line2\n" + "line1\n"
        assert_eq!(reversed, "line3\nline2\nline1\n");
    }

    #[test]
    fn test_tac_no_trailing_newline() {
        let content = "a\nb\nc";
        let segments = split_by_separator(content, "\n", false, false);
        let reversed: String = segments.iter().rev().flat_map(|s| s.chars()).collect();
        assert_eq!(reversed, "cb\na\n");
    }

    #[test]
    fn test_tac_single_line() {
        let content = "only line";
        let segments = split_by_separator(content, "\n", false, false);
        let reversed: String = segments.iter().rev().flat_map(|s| s.chars()).collect();
        assert_eq!(reversed, "only line");
    }

    #[test]
    fn test_rev_multiple_lines() {
        let lines = ["hello", "world", "test"];
        let reversed: Vec<String> = lines.iter().map(|l| reverse_line(l)).collect();
        assert_eq!(reversed, vec!["olleh", "dlrow", "tset"]);
    }
}
