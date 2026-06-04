//! env -- run a command with a modified environment, or print all variables.
//!
//! Usage: env [NAME=VALUE...] [COMMAND [ARGS...]]
//!   With no COMMAND, print all environment variables.
//!   NAME=VALUE pairs are added to the environment before running COMMAND.

use std::env;
use std::process::{self, Command};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let (env_vars, cmd_start) = parse_assignments(&args);

    match cmd_start {
        None => {
            // No command: apply env vars and print all.
            for (name, value) in &env_vars {
                // SAFETY: single-threaded at this point — no other threads
                // are reading the environment concurrently.
                unsafe { env::set_var(name, value); }
            }
            for (key, value) in env::vars() {
                println!("{key}={value}");
            }
        }
        Some(start) => {
            let Some(program) = args.get(start) else {
                eprintln!("env: internal error: bad command index");
                process::exit(1);
            };
            let cmd_args: &[String] = args.get(start.saturating_add(1)..).unwrap_or(&[]);

            let mut cmd = Command::new(program);
            cmd.args(cmd_args);
            for (name, value) in &env_vars {
                cmd.env(name, value);
            }

            match cmd.status() {
                Ok(status) => {
                    process::exit(status.code().unwrap_or(1));
                }
                Err(e) => {
                    eprintln!("env: {program}: {e}");
                    process::exit(127);
                }
            }
        }
    }
}

/// Walk `args` from the start. Leading `NAME=VALUE` tokens become env
/// assignments; the first token that is not an assignment (or `=`-prefixed)
/// marks where the command begins.
///
/// Returns `(assignments, Some(cmd_index))` if a command was found, or
/// `(assignments, None)` if every argument was an assignment.
fn parse_assignments(args: &[String]) -> (Vec<(String, String)>, Option<usize>) {
    let mut env_vars: Vec<(String, String)> = Vec::new();
    let mut cmd_start: Option<usize> = None;

    for (i, arg) in args.iter().enumerate() {
        if arg.contains('=') && !arg.starts_with('=') {
            if let Some(eq_pos) = arg.find('=') {
                // Safe slicing: eq_pos came from find(), so the +1 boundary is
                // always within bounds of `arg`.
                let name = arg.get(..eq_pos).unwrap_or("").to_string();
                let value = arg.get(eq_pos.saturating_add(1)..).unwrap_or("").to_string();
                env_vars.push((name, value));
            }
        } else {
            cmd_start = Some(i);
            break;
        }
    }

    (env_vars, cmd_start)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn no_args_returns_empty_and_no_cmd() {
        let (vars, cmd) = parse_assignments(&s(&[]));
        assert!(vars.is_empty());
        assert_eq!(cmd, None);
    }

    #[test]
    fn single_assignment_no_command() {
        let (vars, cmd) = parse_assignments(&s(&["FOO=bar"]));
        assert_eq!(vars, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(cmd, None);
    }

    #[test]
    fn multiple_assignments_no_command() {
        let (vars, cmd) = parse_assignments(&s(&["A=1", "B=2", "C=3"]));
        assert_eq!(
            vars,
            vec![
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "2".to_string()),
                ("C".to_string(), "3".to_string()),
            ]
        );
        assert_eq!(cmd, None);
    }

    #[test]
    fn command_only_no_assignments() {
        let (vars, cmd) = parse_assignments(&s(&["ls", "-la"]));
        assert!(vars.is_empty());
        assert_eq!(cmd, Some(0));
    }

    #[test]
    fn assignments_then_command() {
        let (vars, cmd) = parse_assignments(&s(&["FOO=bar", "ls", "-la"]));
        assert_eq!(vars, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(cmd, Some(1));
    }

    #[test]
    fn empty_value_assignment() {
        let (vars, cmd) = parse_assignments(&s(&["FOO="]));
        assert_eq!(vars, vec![("FOO".to_string(), String::new())]);
        assert_eq!(cmd, None);
    }

    #[test]
    fn value_with_equals_sign_inside() {
        // Only the first '=' splits — rest is value.
        let (vars, _cmd) = parse_assignments(&s(&["KEY=a=b=c"]));
        assert_eq!(vars, vec![("KEY".to_string(), "a=b=c".to_string())]);
    }

    #[test]
    fn leading_equals_is_treated_as_command_not_assignment() {
        // "=foo" starts with '=' so it's not a valid NAME=VALUE.
        let (vars, cmd) = parse_assignments(&s(&["=foo", "bar"]));
        assert!(vars.is_empty());
        assert_eq!(cmd, Some(0));
    }

    #[test]
    fn arg_with_assignment_after_command_stays_with_command() {
        // Once we see the first non-assignment, everything after is the command.
        let (vars, cmd) = parse_assignments(&s(&["FOO=bar", "ls", "BAR=baz"]));
        assert_eq!(vars, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(cmd, Some(1));
    }

    #[test]
    fn assignment_with_empty_name_skipped_by_starts_with_check() {
        // "=value" starts with '=' so it's not parsed as an assignment.
        let (vars, cmd) = parse_assignments(&s(&["=value"]));
        assert!(vars.is_empty());
        assert_eq!(cmd, Some(0));
    }
}
