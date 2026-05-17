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

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut null_delim = false;
    let mut max_args: Option<usize> = None;
    let mut replace_str: Option<String> = None;
    let mut cmd_args: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-0" => {
                null_delim = true;
                i += 1;
            }
            "-n" => {
                i += 1;
                if i < args.len() {
                    max_args = args[i].parse().ok();
                }
                i += 1;
            }
            "-I" => {
                i += 1;
                if i < args.len() {
                    replace_str = Some(args[i].clone());
                }
                i += 1;
            }
            _ => {
                // Everything from here on is the command + initial args
                cmd_args = args[i..].to_vec();
                break;
            }
        }
    }

    if cmd_args.is_empty() {
        cmd_args.push("echo".to_string());
    }

    // Read all input
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("xargs: failed to read stdin");
        process::exit(1);
    }

    let items: Vec<&str> = if null_delim {
        input.split('\0').filter(|s| !s.is_empty()).collect()
    } else {
        input.split_whitespace().collect()
    };

    if items.is_empty() {
        return;
    }

    let mut exit_code = 0;

    if let Some(ref repl) = replace_str {
        // -I mode: one invocation per item, replacing REPL in each arg
        for item in &items {
            let replaced: Vec<String> = cmd_args
                .iter()
                .map(|a| a.replace(repl.as_str(), item))
                .collect();

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
    } else if let Some(n) = max_args {
        // -n mode: batch items
        for chunk in items.chunks(n) {
            let (cmd, initial_args) = cmd_args.split_first().unwrap_or((&cmd_args[0], &[]));
            let mut full_args: Vec<&str> = initial_args.iter().map(|s| s.as_str()).collect();
            full_args.extend_from_slice(chunk);

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
        // Default: all items in one invocation
        let (cmd, initial_args) = cmd_args.split_first().unwrap_or((&cmd_args[0], &[]));
        let mut full_args: Vec<&str> = initial_args.iter().map(|s| s.as_str()).collect();
        for item in &items {
            full_args.push(item);
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
