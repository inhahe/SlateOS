//! nice — run a command with modified scheduling priority.
//!
//! Usage: nice [-n ADJUST] COMMAND [ARGS...]
//!   -n ADJUST   add ADJUST to the niceness (default: 10)

use std::env;
use std::process::{self, Command};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut _adjustment: i32 = 10;
    let mut cmd_start = 0;

    if args.is_empty() {
        eprintln!("nice: missing operand");
        process::exit(125);
    }

    let mut i = 0;
    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            _adjustment = args[i + 1].parse().unwrap_or(10);
            i += 2;
        } else if args[i].starts_with("-n") {
            _adjustment = args[i][2..].parse().unwrap_or(10);
            i += 1;
        } else {
            cmd_start = i;
            break;
        }
    }

    if cmd_start >= args.len() {
        eprintln!("nice: missing operand");
        process::exit(125);
    }

    // Note: actual niceness adjustment requires setpriority() syscall.
    // For now we run the command and document the limitation.
    // The POSIX layer will eventually support this.
    let cmd = &args[cmd_start];
    let cmd_args = &args[cmd_start + 1..];

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
