//! sleep -- suspend execution for a specified duration.
//!
//! Usage: sleep SECONDS
//!   SECONDS may be an integer or a decimal number.

use std::env;
use std::process;
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("sleep: missing operand");
        process::exit(1);
    }

    let secs: f64 = match args[0].parse() {
        Ok(v) if v >= 0.0 => v,
        _ => {
            eprintln!("sleep: invalid time interval '{}'", args[0]);
            process::exit(1);
        }
    };

    thread::sleep(Duration::from_secs_f64(secs));
}
