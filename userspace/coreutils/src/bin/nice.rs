//! nice — run a command with modified scheduling priority.
//!
//! Usage: nice [-n ADJUST] COMMAND [ARGS...]
//!   -n ADJUST   add ADJUST to the niceness (default: 10)

use std::env;
use std::process::{self, Command};

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct NiceArgs {
    adjustment: i32,
    cmd_start: usize,
}

/// Parse the leading `-n` arguments off `args`.  Recognises `-n N` (two
/// tokens), `-nN` (one combined token), and otherwise leaves the slice
/// untouched.  `cmd_start` is the index of the first non-flag argument
/// (i.e. the command to run); it equals `args.len()` if there's no command.
fn parse_args(args: &[String]) -> NiceArgs {
    let mut adjustment: i32 = 10;
    let mut i: usize = 0;

    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        if arg == "-n" && i.saturating_add(1) < args.len() {
            if let Some(v) = args.get(i.saturating_add(1)) {
                adjustment = v.parse().unwrap_or(10);
            }
            i = i.saturating_add(2);
        } else if let Some(rest) = arg.strip_prefix("-n") {
            adjustment = rest.parse().unwrap_or(10);
            i = i.saturating_add(1);
        } else {
            break;
        }
    }

    NiceArgs {
        adjustment,
        cmd_start: i,
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("nice: missing operand");
        process::exit(125);
    }

    let parsed = parse_args(&args);
    let _adjustment = parsed.adjustment;
    let cmd_start = parsed.cmd_start;

    if cmd_start >= args.len() {
        eprintln!("nice: missing operand");
        process::exit(125);
    }

    // Note: actual niceness adjustment requires setpriority() syscall.
    // For now we run the command and document the limitation.
    // The POSIX layer will eventually support this.
    let Some(cmd) = args.get(cmd_start) else {
        eprintln!("nice: missing operand");
        process::exit(125);
    };
    let cmd_args: &[String] = args.get(cmd_start.saturating_add(1)..).unwrap_or(&[]);

    match Command::new(cmd).args(cmd_args).status() {
        Ok(status) => {
            process::exit(status.code().unwrap_or(126));
        }
        Err(e) => {
            eprintln!("nice: {cmd}: {e}");
            process::exit(127);
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

    #[test]
    fn no_flag_default_adjustment() {
        let a = parse_args(&s(&["echo", "hi"]));
        assert_eq!(a.adjustment, 10);
        assert_eq!(a.cmd_start, 0);
    }

    #[test]
    fn dash_n_value_two_tokens() {
        let a = parse_args(&s(&["-n", "5", "echo"]));
        assert_eq!(a.adjustment, 5);
        assert_eq!(a.cmd_start, 2);
    }

    #[test]
    fn dash_n_combined_token() {
        let a = parse_args(&s(&["-n5", "echo"]));
        assert_eq!(a.adjustment, 5);
        assert_eq!(a.cmd_start, 1);
    }

    #[test]
    fn dash_n_negative_value() {
        let a = parse_args(&s(&["-n", "-5", "echo"]));
        assert_eq!(a.adjustment, -5);
        assert_eq!(a.cmd_start, 2);
    }

    #[test]
    fn dash_n_combined_negative() {
        let a = parse_args(&s(&["-n-3", "echo"]));
        assert_eq!(a.adjustment, -3);
        assert_eq!(a.cmd_start, 1);
    }

    #[test]
    fn dash_n_invalid_falls_back_to_default() {
        let a = parse_args(&s(&["-n", "abc", "echo"]));
        assert_eq!(a.adjustment, 10);
        assert_eq!(a.cmd_start, 2);
    }

    #[test]
    fn dash_n_at_end_no_value_treats_as_flag_no_op() {
        // -n with no following arg is consumed as a non-matching token; the
        // current implementation falls into the strip_prefix branch which
        // parses the (empty) suffix and falls back to default.
        let a = parse_args(&s(&["-n"]));
        assert_eq!(a.adjustment, 10);
        assert_eq!(a.cmd_start, 1);
    }

    #[test]
    fn empty_args_default() {
        let a = parse_args(&s(&[]));
        assert_eq!(a.adjustment, 10);
        assert_eq!(a.cmd_start, 0);
    }

    #[test]
    fn multiple_dash_n_uses_last() {
        let a = parse_args(&s(&["-n1", "-n", "7", "echo"]));
        assert_eq!(a.adjustment, 7);
        assert_eq!(a.cmd_start, 3);
    }

    #[test]
    fn no_n_flag_with_dash_cmd_treated_as_cmd() {
        // `-x` is not -n; should stop scanning.
        let a = parse_args(&s(&["-x"]));
        assert_eq!(a.adjustment, 10);
        assert_eq!(a.cmd_start, 0);
    }
}
