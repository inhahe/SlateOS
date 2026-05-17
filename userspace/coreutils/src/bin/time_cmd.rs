//! time — run a command and report its execution time.
//!
//! Usage: time COMMAND [ARGS...]
//!   Runs COMMAND and prints elapsed wall-clock time to stderr.
//!
//! Note: This is named time_cmd.rs to avoid conflict with Rust's
//! std::time module. The binary is installed as "time".

use std::env;
use std::process::{self, Command};
use std::time::Instant;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("time: missing command");
        process::exit(1);
    }

    let cmd = &args[0];
    let cmd_args = &args[1..];

    let start = Instant::now();

    let status = match Command::new(cmd).args(cmd_args).status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("time: {cmd}: {e}");
            process::exit(127);
        }
    };

    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs_f64();
    let mins = (total_secs / 60.0) as u64;
    let secs = total_secs % 60.0;

    eprintln!();
    eprintln!("real\t{mins}m{secs:.3}s");
    // We can't distinguish user/sys time without kernel support,
    // so just show real time.

    process::exit(status.code().unwrap_or(126));
}
