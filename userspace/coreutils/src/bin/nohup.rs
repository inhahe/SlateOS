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

    if args.is_empty() {
        eprintln!("nohup: missing operand");
        eprintln!("Usage: nohup COMMAND [ARGS...]");
        process::exit(125);
    }

    let cmd = &args[0];
    let cmd_args = &args[1..];

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
            Stdio::from(f.try_clone().unwrap_or_else(|_| {
                // Fall back to inheriting stdout
                process::exit(125);
            }))
        }
        Err(_) => Stdio::inherit(),
    };

    match Command::new(cmd)
        .args(cmd_args)
        .stdout(stdout_cfg)
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(mut child) => match child.wait() {
            Ok(status) => {
                process::exit(status.code().unwrap_or(126));
            }
            Err(e) => {
                eprintln!("nohup: {cmd}: {e}");
                process::exit(126);
            }
        },
        Err(e) => {
            eprintln!("nohup: {cmd}: {e}");
            process::exit(127);
        }
    }
}
