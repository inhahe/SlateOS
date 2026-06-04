//! xargs — build and execute command lines from stdin.
//!
//! Usage: xargs [-0] [-n MAX] [-I REPL] COMMAND [ARGS...]
//!   -0        input items are null-terminated (not newline)
//!   -n MAX    use at most MAX arguments per command invocation
//!   -I REPL   replace REPL in COMMAND with each input item (one per invocation)
//!   Default: append all stdin items to COMMAND and run once.

use std::env;
use std::io::{self, Read};
use std::process::{self, Command};

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct XargsOpts {
    null_delim: bool,
    max_args: Option<usize>,
    replace_str: Option<String>,
    cmd_args: Vec<String>,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = parse_args(&args);

    if opts.cmd_args.is_empty() {
        opts.cmd_args.push("echo".to_string());
    }

    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("xargs: failed to read stdin");
        process::exit(1);
    }

    let items = split_items(&input, opts.null_delim);
    if items.is_empty() {
        return;
    }

    let mut exit_code = 0;

    if let Some(ref repl) = opts.replace_str {
        for item in &items {
            let replaced = replace_in_args(&opts.cmd_args, repl, item);
            if let Some((cmd, args)) = replaced.split_first() {
                match Command::new(cmd).args(args).status() {
                    Ok(s) if !s.success() => exit_code = 1,
                    Err(e) => {
                        eprintln!("xargs: {cmd}: {e}");
                        exit_code = 1;
                    }
                    _ => {}
                }
            }
        }
    } else if let Some(n) = opts.max_args {
        for chunk in items.chunks(n) {
            let Some((cmd, initial_args)) = opts.cmd_args.split_first() else {
                continue;
            };
            let mut full_args: Vec<&str> = initial_args.iter().map(String::as_str).collect();
            full_args.extend(chunk.iter().map(String::as_str));

            match Command::new(cmd).args(&full_args).status() {
                Ok(s) if !s.success() => exit_code = 1,
                Err(e) => {
                    eprintln!("xargs: {cmd}: {e}");
                    exit_code = 1;
                }
                _ => {}
            }
        }
    } else {
        let Some((cmd, initial_args)) = opts.cmd_args.split_first() else {
            process::exit(exit_code);
        };
        let mut full_args: Vec<&str> = initial_args.iter().map(String::as_str).collect();
        for item in &items {
            full_args.push(item.as_str());
        }

        match Command::new(cmd).args(&full_args).status() {
            Ok(s) if !s.success() => exit_code = 1,
            Err(e) => {
                eprintln!("xargs: {cmd}: {e}");
                exit_code = 1;
            }
            _ => {}
        }
    }

    process::exit(exit_code);
}

/// Parse xargs's argv into options + the command tail.
fn parse_args(args: &[String]) -> XargsOpts {
    let mut null_delim = false;
    let mut max_args: Option<usize> = None;
    let mut replace_str: Option<String> = None;
    let mut cmd_args: Vec<String> = Vec::new();
    let mut i: usize = 0;

    while i < args.len() {
        let arg = args.get(i).map(String::as_str).unwrap_or("");
        match arg {
            "-0" => {
                null_delim = true;
                i = i.saturating_add(1);
            }
            "-n" => {
                i = i.saturating_add(1);
                if let Some(v) = args.get(i) {
                    max_args = v.parse().ok();
                }
                i = i.saturating_add(1);
            }
            "-I" => {
                i = i.saturating_add(1);
                if let Some(v) = args.get(i) {
                    replace_str = Some(v.clone());
                }
                i = i.saturating_add(1);
            }
            _ => {
                cmd_args = args.get(i..).map(<[String]>::to_vec).unwrap_or_default();
                break;
            }
        }
    }

    XargsOpts { null_delim, max_args, replace_str, cmd_args }
}

/// Split stdin input into items either by null bytes (`-0`) or by whitespace.
fn split_items(input: &str, null_delim: bool) -> Vec<String> {
    if null_delim {
        input.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect()
    } else {
        input.split_whitespace().map(str::to_string).collect()
    }
}

/// Replace every occurrence of `repl` in each arg with `item`.
fn replace_in_args(cmd_args: &[String], repl: &str, item: &str) -> Vec<String> {
    cmd_args.iter().map(|a| a.replace(repl, item)).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_no_args() {
        let o = parse_args(&s(&[]));
        assert!(!o.null_delim);
        assert!(o.max_args.is_none());
        assert!(o.replace_str.is_none());
        assert!(o.cmd_args.is_empty());
    }

    #[test]
    fn parse_null_delim() {
        let o = parse_args(&s(&["-0", "echo"]));
        assert!(o.null_delim);
        assert_eq!(o.cmd_args, vec!["echo"]);
    }

    #[test]
    fn parse_max_args() {
        let o = parse_args(&s(&["-n", "3", "echo"]));
        assert_eq!(o.max_args, Some(3));
        assert_eq!(o.cmd_args, vec!["echo"]);
    }

    #[test]
    fn parse_replace_str() {
        let o = parse_args(&s(&["-I", "{}", "echo", "{}"]));
        assert_eq!(o.replace_str.as_deref(), Some("{}"));
        assert_eq!(o.cmd_args, vec!["echo", "{}"]);
    }

    #[test]
    fn parse_combined_options() {
        let o = parse_args(&s(&["-0", "-n", "2", "cmd"]));
        assert!(o.null_delim);
        assert_eq!(o.max_args, Some(2));
        assert_eq!(o.cmd_args, vec!["cmd"]);
    }

    #[test]
    fn parse_cmd_with_initial_args() {
        let o = parse_args(&s(&["echo", "prefix"]));
        assert_eq!(o.cmd_args, vec!["echo", "prefix"]);
    }

    #[test]
    fn parse_max_args_invalid_is_none() {
        let o = parse_args(&s(&["-n", "abc", "echo"]));
        assert_eq!(o.max_args, None);
    }

    #[test]
    fn split_whitespace_basic() {
        let items = split_items("a b c\n", false);
        assert_eq!(items, vec!["a", "b", "c"]);
    }

    #[test]
    fn split_whitespace_collapses_runs() {
        let items = split_items("a   b\n\nc", false);
        assert_eq!(items, vec!["a", "b", "c"]);
    }

    #[test]
    fn split_null_delim() {
        let items = split_items("a\0b\0c\0", true);
        assert_eq!(items, vec!["a", "b", "c"]);
    }

    #[test]
    fn split_null_delim_drops_empty() {
        let items = split_items("a\0\0b\0", true);
        assert_eq!(items, vec!["a", "b"]);
    }

    #[test]
    fn split_empty_input() {
        assert!(split_items("", false).is_empty());
        assert!(split_items("", true).is_empty());
    }

    #[test]
    fn replace_simple() {
        let args = s(&["echo", "{}"]);
        let out = replace_in_args(&args, "{}", "hello");
        assert_eq!(out, vec!["echo", "hello"]);
    }

    #[test]
    fn replace_no_match_leaves_arg() {
        let args = s(&["echo", "static"]);
        let out = replace_in_args(&args, "{}", "x");
        assert_eq!(out, vec!["echo", "static"]);
    }

    #[test]
    fn replace_multiple_occurrences_in_one_arg() {
        let args = s(&["{}_{}"]);
        let out = replace_in_args(&args, "{}", "x");
        assert_eq!(out, vec!["x_x"]);
    }

    #[test]
    fn replace_embedded_token() {
        let args = s(&["pre-{}-post"]);
        let out = replace_in_args(&args, "{}", "MID");
        assert_eq!(out, vec!["pre-MID-post"]);
    }
}
