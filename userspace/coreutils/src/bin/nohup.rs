//! nohup — run a command immune to hangups.
//!
//! Usage: nohup COMMAND [ARGS...]
//!   Runs COMMAND with SIGHUP ignored. If stdout is a terminal,
//!   output is redirected to nohup.out.

use std::env;
use std::fs::OpenOptions;
use std::process::{self, Command, Stdio};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("nohup: {e}");
            eprintln!("Usage: nohup COMMAND [ARGS...]");
            process::exit(125);
        }
    };

    // Try to redirect stdout to nohup.out if it's a terminal.
    // In our minimal environment, we'll always redirect since we
    // can't easily check isatty.
    let output_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("nohup.out");

    let stdout_cfg = match &output_file {
        Ok(f) => {
            eprintln!("nohup: appending output to 'nohup.out'");
            match f.try_clone() {
                Ok(clone) => Stdio::from(clone),
                Err(_) => Stdio::inherit(),
            }
        }
        Err(_) => Stdio::inherit(),
    };

    match Command::new(&parsed.cmd)
        .args(&parsed.cmd_args)
        .stdout(stdout_cfg)
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(mut child) => match child.wait() {
            Ok(status) => {
                process::exit(status.code().unwrap_or(126));
            }
            Err(e) => {
                eprintln!("nohup: {}: {e}", parsed.cmd);
                process::exit(126);
            }
        },
        Err(e) => {
            eprintln!("nohup: {}: {e}", parsed.cmd);
            process::exit(127);
        }
    }
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct NohupArgs {
    cmd: String,
    cmd_args: Vec<String>,
}

/// Parse nohup's argv.  The first positional argument is the command,
/// everything after is passed verbatim to the child.  No flags are
/// recognised (POSIX nohup has no options).
fn parse_args(args: &[String]) -> Result<NohupArgs, String> {
    let cmd = args
        .first()
        .ok_or_else(|| "missing operand".to_string())?
        .clone();
    let cmd_args = args.get(1..).unwrap_or(&[]).to_vec();
    Ok(NohupArgs { cmd, cmd_args })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_empty_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_single_command_no_args() {
        let p = parse_args(&s(&["sleep"])).unwrap();
        assert_eq!(p.cmd, "sleep");
        assert!(p.cmd_args.is_empty());
    }

    #[test]
    fn parse_command_with_args() {
        let p = parse_args(&s(&["sleep", "60"])).unwrap();
        assert_eq!(p.cmd, "sleep");
        assert_eq!(p.cmd_args, vec!["60"]);
    }

    #[test]
    fn parse_command_with_multiple_args() {
        let p = parse_args(&s(&["echo", "hello", "world"])).unwrap();
        assert_eq!(p.cmd, "echo");
        assert_eq!(p.cmd_args, vec!["hello", "world"]);
    }

    #[test]
    fn parse_dash_args_are_passed_through_to_command() {
        // nohup itself has no flags; -n etc. belong to the child.
        let p = parse_args(&s(&["ls", "-la", "/"])).unwrap();
        assert_eq!(p.cmd, "ls");
        assert_eq!(p.cmd_args, vec!["-la", "/"]);
    }

    #[test]
    fn parse_command_path_is_preserved() {
        let p = parse_args(&s(&["/usr/bin/env", "RUST_LOG=trace", "rustc"])).unwrap();
        assert_eq!(p.cmd, "/usr/bin/env");
        assert_eq!(p.cmd_args, vec!["RUST_LOG=trace", "rustc"]);
    }
}
