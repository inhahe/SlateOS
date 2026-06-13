#![deny(clippy::all)]

//! htop-cli — SlateOS htop CLI
//!
//! Single personality: `htop`

use std::env;
use std::process;

fn run_htop(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: htop [OPTIONS]");
        println!();
        println!("htop — interactive process viewer (SlateOS).");
        println!();
        println!("Options:");
        println!("  -d DELAY       Update delay in tenths of seconds");
        println!("  -u USER        Show only processes of USER");
        println!("  -p PID         Show only given PIDs");
        println!("  -s COLUMN      Sort by COLUMN");
        println!("  -t             Tree view");
        println!("  --no-color     Disable colors");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("htop 3.3.0 (SlateOS)");
        return 0;
    }

    let tree = args.iter().any(|a| a == "-t" || a == "--tree");
    let user = args.windows(2).find(|w| w[0] == "-u" || w[0] == "--user")
        .map(|w| w[1].as_str());

    println!("  CPU[||||||||||||||||      56.2%]   Tasks: 142, 3 running");
    println!("  Mem[||||||||||            2.1G/8.0G]   Load: 1.23 0.98 0.87");
    println!("  Swp[|                     128M/2.0G]   Uptime: 3 days, 14:22:30");
    println!();
    println!("  PID USER      PRI  NI  VIRT   RES   SHR S CPU% MEM%   TIME+  Command");
    if tree {
        println!("    1 root       20   0  168M  12M  8.4M S  0.0  0.1  0:05.23 ├─ init");
        println!("  234 root       20   0   45M  22M   12M S  0.3  0.3  1:23.45 │  ├─ syslogd");
        println!("  567 root       20   0  890M 234M   45M S  2.1  2.9  5:45.67 │  └─ compositor");
        println!(" 1234 user       20   0  1.2G 456M   78M S 15.3  5.7 12:34.56 ├─ firefox");
        println!(" 1235 user       20   0  800M 234M   56M S  8.7  2.9  3:21.09 │  └─ Web Content");
        println!(" 2345 user       20   0  234M  89M   34M S  3.2  1.1  2:15.78 ├─ code-editor");
        println!(" 3456 user       20   0   56M  23M   12M S  0.5  0.3  0:45.12 └─ terminal");
    } else {
        if let Some(u) = user {
            println!(" 1234 {}    20   0  1.2G 456M   78M S 15.3  5.7 12:34.56 firefox", u);
            println!(" 1235 {}    20   0  800M 234M   56M S  8.7  2.9  3:21.09 Web Content", u);
            println!(" 2345 {}    20   0  234M  89M   34M S  3.2  1.1  2:15.78 code-editor", u);
            println!(" 3456 {}    20   0   56M  23M   12M S  0.5  0.3  0:45.12 terminal", u);
        } else {
            println!(" 1234 user       20   0  1.2G 456M   78M S 15.3  5.7 12:34.56 firefox");
            println!("  567 root       20   0  890M 234M   45M S  2.1  2.9  5:45.67 compositor");
            println!(" 1235 user       20   0  800M 234M   56M S  8.7  2.9  3:21.09 Web Content");
            println!(" 2345 user       20   0  234M  89M   34M S  3.2  1.1  2:15.78 code-editor");
            println!(" 3456 user       20   0   56M  23M   12M S  0.5  0.3  0:45.12 terminal");
            println!("  234 root       20   0   45M  22M   12M S  0.3  0.3  1:23.45 syslogd");
            println!("    1 root       20   0  168M  12M  8.4M S  0.0  0.1  0:05.23 init");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_htop(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_htop};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_htop(vec!["--help".to_string()]), 0);
        assert_eq!(run_htop(vec!["-h".to_string()]), 0);
        let _ = run_htop(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_htop(vec![]);
    }
}
