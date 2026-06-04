//! find — search for files in a directory hierarchy.
//!
//! Usage: find [PATH...] [EXPRESSION]
//!   -name PATTERN    match filename (shell glob: *, ?, [])
//!   -type TYPE       match type: f (file), d (dir), l (symlink)
//!   -maxdepth N      descend at most N levels
//!   -print           print matching paths (default action)

use std::env;
use std::fs;
use std::path::Path;
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct FindArgs {
    paths: Vec<String>,
    name_pattern: Option<String>,
    type_filter: Option<char>,
    max_depth: Option<usize>,
}

/// Parse find's argv.  `-name`, `-type`, and `-maxdepth` each consume one
/// following argument.  Unknown flags return an error.  Bare arguments
/// not starting with `-` are accumulated as paths; if none were given,
/// `["."]` is used by the caller.
fn parse_args(args: &[String]) -> Result<FindArgs, String> {
    let mut out = FindArgs::default();
    let mut i: usize = 0;

    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        match arg.as_str() {
            "-name" => {
                i = i.saturating_add(1);
                let v = args
                    .get(i)
                    .ok_or_else(|| "option -name requires an argument".to_string())?;
                out.name_pattern = Some(v.clone());
            }
            "-type" => {
                i = i.saturating_add(1);
                let v = args
                    .get(i)
                    .ok_or_else(|| "option -type requires an argument".to_string())?;
                out.type_filter = v.chars().next();
            }
            "-maxdepth" => {
                i = i.saturating_add(1);
                let v = args
                    .get(i)
                    .ok_or_else(|| "option -maxdepth requires an argument".to_string())?;
                out.max_depth = Some(
                    v.parse::<usize>()
                        .map_err(|_| format!("invalid maxdepth: {v}"))?,
                );
            }
            "-print" => {} // default action, ignore
            a if !a.starts_with('-') => {
                out.paths.push(a.to_string());
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
        i = i.saturating_add(1);
    }

    Ok(out)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("find: {e}");
            process::exit(1);
        }
    };

    let mut paths = parsed.paths;
    if paths.is_empty() {
        paths.push(".".to_string());
    }

    for path in &paths {
        let p = Path::new(path);
        find_recursive(
            p,
            0,
            parsed.max_depth,
            parsed.name_pattern.as_deref(),
            parsed.type_filter,
        );
    }
}

fn find_recursive(
    dir: &Path,
    depth: usize,
    max_depth: Option<usize>,
    name_pattern: Option<&str>,
    type_filter: Option<char>,
) {
    // Check the directory itself at depth 0
    if depth == 0 && matches_filters(dir, name_pattern, type_filter) {
        println!("{}", dir.display());
    }

    if let Some(max) = max_depth
        && depth >= max
    {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("find: '{}': {e}", dir.display());
            return;
        }
    };

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!("find: {e}");
                continue;
            }
        };

        let path = entry.path();

        if matches_filters(&path, name_pattern, type_filter) {
            println!("{}", path.display());
        }

        // Recurse into directories
        if path.is_dir() {
            find_recursive(
                &path,
                depth.saturating_add(1),
                max_depth,
                name_pattern,
                type_filter,
            );
        }
    }
}

fn matches_filters(path: &Path, name_pattern: Option<&str>, type_filter: Option<char>) -> bool {
    // Type filter
    if let Some(t) = type_filter {
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(_) => return false,
        };
        let matches_type = match t {
            'f' => meta.is_file(),
            'd' => meta.is_dir(),
            'l' => meta.file_type().is_symlink(),
            _ => true,
        };
        if !matches_type {
            return false;
        }
    }

    // Name filter (glob-like)
    if let Some(pattern) = name_pattern {
        let filename = match path.file_name() {
            Some(n) => n.to_string_lossy(),
            None => return false,
        };
        if !glob_match(pattern, &filename) {
            return false;
        }
    }

    true
}

/// Simple glob matching: `*` matches any sequence (including empty),
/// `?` matches one character, `[abc]` matches one listed character.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_inner(&pat, &txt, 0, 0)
}

fn glob_match_inner(pat: &[char], txt: &[char], pi: usize, ti: usize) -> bool {
    if pi == pat.len() {
        return ti == txt.len();
    }

    let Some(&p) = pat.get(pi) else { return ti == txt.len() };

    match p {
        '*' => {
            // Try matching zero or more characters
            let remaining = txt.len().saturating_sub(ti);
            for skip in 0..=remaining {
                if glob_match_inner(pat, txt, pi.saturating_add(1), ti.saturating_add(skip)) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < txt.len() {
                glob_match_inner(pat, txt, pi.saturating_add(1), ti.saturating_add(1))
            } else {
                false
            }
        }
        '[' => {
            if ti >= txt.len() {
                return false;
            }
            // Find closing ]
            let mut end = pi.saturating_add(1);
            while end < pat.len() && pat.get(end) != Some(&']') {
                end = end.saturating_add(1);
            }
            if end >= pat.len() {
                return false; // malformed
            }
            let chars = pat.get(pi.saturating_add(1)..end).unwrap_or(&[]);
            let Some(&c) = txt.get(ti) else { return false };
            let matches = chars.contains(&c);
            if matches {
                glob_match_inner(pat, txt, end.saturating_add(1), ti.saturating_add(1))
            } else {
                false
            }
        }
        c => {
            if let Some(&tc) = txt.get(ti)
                && tc == c
            {
                glob_match_inner(pat, txt, pi.saturating_add(1), ti.saturating_add(1))
            } else {
                false
            }
        }
    }
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
    fn parse_empty() {
        let a = parse_args(&s(&[])).unwrap();
        assert!(a.paths.is_empty());
        assert!(a.name_pattern.is_none());
        assert!(a.type_filter.is_none());
        assert!(a.max_depth.is_none());
    }

    #[test]
    fn parse_single_path() {
        let a = parse_args(&s(&["/etc"])).unwrap();
        assert_eq!(a.paths, vec!["/etc"]);
    }

    #[test]
    fn parse_multiple_paths() {
        let a = parse_args(&s(&["a", "b", "c"])).unwrap();
        assert_eq!(a.paths, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_name_pattern() {
        let a = parse_args(&s(&["-name", "*.rs"])).unwrap();
        assert_eq!(a.name_pattern.as_deref(), Some("*.rs"));
    }

    #[test]
    fn parse_type_filter() {
        let a = parse_args(&s(&["-type", "f"])).unwrap();
        assert_eq!(a.type_filter, Some('f'));
    }

    #[test]
    fn parse_type_filter_takes_first_char() {
        let a = parse_args(&s(&["-type", "dir"])).unwrap();
        assert_eq!(a.type_filter, Some('d'));
    }

    #[test]
    fn parse_maxdepth() {
        let a = parse_args(&s(&["-maxdepth", "3"])).unwrap();
        assert_eq!(a.max_depth, Some(3));
    }

    #[test]
    fn parse_maxdepth_invalid_errors() {
        let err = parse_args(&s(&["-maxdepth", "abc"])).unwrap_err();
        assert!(err.contains("invalid maxdepth"));
    }

    #[test]
    fn parse_name_missing_value_errors() {
        let err = parse_args(&s(&["-name"])).unwrap_err();
        assert!(err.contains("-name requires"));
    }

    #[test]
    fn parse_print_ignored() {
        let a = parse_args(&s(&["/etc", "-print"])).unwrap();
        assert_eq!(a.paths, vec!["/etc"]);
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = parse_args(&s(&["-foo"])).unwrap_err();
        assert!(err.contains("unknown option"));
        assert!(err.contains("-foo"));
    }

    #[test]
    fn parse_mixed_paths_and_filters() {
        let a = parse_args(&s(&["src", "/usr", "-name", "*.c", "-type", "f"])).unwrap();
        assert_eq!(a.paths, vec!["src", "/usr"]);
        assert_eq!(a.name_pattern.as_deref(), Some("*.c"));
        assert_eq!(a.type_filter, Some('f'));
    }

    // ---------------- glob_match ----------------

    #[test]
    fn glob_literal() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
        assert!(!glob_match("hello", "hell"));
        assert!(!glob_match("hello", "helloo"));
    }

    #[test]
    fn glob_star_matches_everything() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "a"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn glob_star_at_end() {
        assert!(glob_match("foo*", "foo"));
        assert!(glob_match("foo*", "foobar"));
        assert!(!glob_match("foo*", "bar"));
    }

    #[test]
    fn glob_star_at_start() {
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(glob_match("*.rs", ".rs"));
        assert!(!glob_match("*.rs", "lib.c"));
    }

    #[test]
    fn glob_star_in_middle() {
        assert!(glob_match("a*z", "az"));
        assert!(glob_match("a*z", "abcz"));
        assert!(!glob_match("a*z", "ab"));
    }

    #[test]
    fn glob_question() {
        assert!(glob_match("a?c", "abc"));
        assert!(glob_match("a?c", "aXc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match("a?c", "abbc"));
    }

    #[test]
    fn glob_bracket_class() {
        assert!(glob_match("[abc]", "a"));
        assert!(glob_match("[abc]", "b"));
        assert!(glob_match("[abc]", "c"));
        assert!(!glob_match("[abc]", "d"));
        assert!(!glob_match("[abc]", "ab"));
    }

    #[test]
    fn glob_bracket_in_context() {
        assert!(glob_match("file.[ch]", "file.c"));
        assert!(glob_match("file.[ch]", "file.h"));
        assert!(!glob_match("file.[ch]", "file.cpp"));
    }

    #[test]
    fn glob_unmatched_bracket_is_no_match() {
        // Malformed pattern: returns false (rather than panicking).
        assert!(!glob_match("[abc", "a"));
    }

    #[test]
    fn glob_empty_pattern_matches_only_empty() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "a"));
    }

    #[test]
    fn glob_multiple_stars() {
        assert!(glob_match("a*b*c", "abc"));
        assert!(glob_match("a*b*c", "axxxbyyyc"));
        assert!(!glob_match("a*b*c", "abca"));
    }

    #[test]
    fn glob_unicode_chars() {
        assert!(glob_match("café", "café"));
        assert!(glob_match("c?fé", "café"));
    }
}
