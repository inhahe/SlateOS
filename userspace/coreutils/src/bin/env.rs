//! env -- run a command with a modified environment, or print all variables.
//!
//! Usage: env [NAME=VALUE...] [COMMAND [ARGS...]]
//!   With no COMMAND, print all environment variables.
//!   NAME=VALUE pairs are added to the environment before running COMMAND.

use std::env;
use std::process::{self, Command};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // Split args into env assignments (contain '=') and command + its args.
    let mut env_vars: Vec<(String, String)> = Vec::new();
    let mut cmd_start: Option<usize> = None;

    for (i, arg) in args.iter().enumerate() {
        if cmd_start.is_none() && arg.contains('=') && !arg.starts_with('=') {
            // This is a NAME=VALUE assignment.
            if let Some(eq_pos) = arg.find('=') {
                let name = arg[..eq_pos].to_string();
                let value = arg[eq_pos + 1..].to_string();
                env_vars.push((name, value));
            }
        } else {
            cmd_start = Some(i);
            break;
        }
    }

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
            let program = &args[start];
            let cmd_args = &args[start + 1..];

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
