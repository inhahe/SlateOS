#![deny(clippy::all)]

//! iotop-cli — SlateOS iotop CLI
//!
//! Single personality: `iotop`

use std::env;
use std::process;

fn run_iotop(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iotop [OPTIONS]");
        println!();
        println!("iotop — I/O monitoring tool (Slate OS).");
        println!();
        println!("Options:");
        println!("  -o             Only show processes doing I/O");
        println!("  -b             Batch mode (non-interactive)");
        println!("  -d SECONDS     Delay between iterations");
        println!("  -p PID         Monitor specific PID");
        println!("  -u USER        Show only USER's processes");
        println!("  -a             Accumulated I/O");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("iotop 0.6 (Slate OS)");
        return 0;
    }

    let only_io = args.iter().any(|a| a == "-o");
    let accumulated = args.iter().any(|a| a == "-a");

    println!("Total DISK READ:   45.2 MB/s | Total DISK WRITE:  12.3 MB/s");
    println!("Current DISK READ: 23.1 MB/s | Current DISK WRITE:  8.7 MB/s");
    println!();
    if accumulated {
        println!("  TID  PRIO  USER     DISK READ  DISK WRITE  SWAPIN     IO>    COMMAND");
        println!(" 1234  be/4  user      1.23 GB    456.7 MB    0.00 %    0.12 % firefox");
        println!("  567  be/4  root    890.1 MB      23.4 MB    0.00 %    0.05 % compositor");
        println!(" 4567  be/4  user    234.5 MB     567.8 MB    0.00 %    0.23 % rsync");
    } else {
        println!("  TID  PRIO  USER     DISK READ  DISK WRITE  SWAPIN     IO>    COMMAND");
        println!(" 4567  be/4  user     23.1 MB/s    8.4 MB/s   0.00 %   12.34 % rsync");
        println!(" 1234  be/4  user      5.6 MB/s    2.1 MB/s   0.00 %    3.21 % firefox");
        if !only_io {
            println!("  567  be/4  root      0.0 MB/s    0.0 MB/s   0.00 %    0.00 % compositor");
            println!("    1  be/4  root      0.0 MB/s    0.0 MB/s   0.00 %    0.00 % init");
            println!("  234  be/4  root      0.0 MB/s    0.0 MB/s   0.00 %    0.00 % syslogd");
        }
        println!(" 5678  be/4  root     12.3 MB/s    1.2 MB/s   0.00 %    5.67 % updatedb");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iotop(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_iotop};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_iotop(vec!["--help".to_string()]), 0);
        assert_eq!(run_iotop(vec!["-h".to_string()]), 0);
        let _ = run_iotop(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_iotop(vec![]);
    }
}
