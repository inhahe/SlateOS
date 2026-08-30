#![deny(clippy::all)]

//! journalctl-cli — Slate OS journalctl CLI
//!
//! Single personality: `journalctl`

use std::env;
use std::process;

fn run_journalctl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: journalctl [OPTIONS]");
        println!();
        println!("journalctl — query the systemd journal (Slate OS).");
        println!();
        println!("Options:");
        println!("  -u, --unit UNIT        Show logs for unit");
        println!("  -f, --follow           Follow new messages");
        println!("  -n, --lines N          Show last N lines");
        println!("  -b, --boot [ID]        Show logs from boot");
        println!("  -p, --priority LEVEL   Filter by priority (emerg..debug)");
        println!("  -S, --since TIME       Show entries since TIME");
        println!("  -U, --until TIME       Show entries until TIME");
        println!("  -o, --output FORMAT    Output format (short, json, verbose, cat)");
        println!("  -k, --dmesg            Show kernel messages");
        println!("  -r, --reverse          Reverse order");
        println!("  --disk-usage           Show disk usage");
        println!("  --vacuum-size SIZE     Reduce to SIZE");
        println!("  --vacuum-time TIME     Remove older than TIME");
        println!("  --list-boots           List recorded boots");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("systemd 255 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "--disk-usage") {
        println!("Archived and active journals take up 256.0M in the file system.");
        return 0;
    }

    if args.iter().any(|a| a == "--list-boots") {
        println!(" -2 abc123 Mon 2024-01-13 08:00:00 UTC—Mon 2024-01-13 23:59:59 UTC");
        println!(" -1 def456 Tue 2024-01-14 08:00:00 UTC—Tue 2024-01-14 23:59:59 UTC");
        println!("  0 ghi789 Wed 2024-01-15 08:00:00 UTC—Wed 2024-01-15 12:00:00 UTC");
        return 0;
    }

    let unit = args.windows(2).find(|w| w[0] == "-u" || w[0] == "--unit")
        .map(|w| w[1].as_str());
    let lines = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--lines")
        .and_then(|w| w[1].parse::<usize>().ok()).unwrap_or(10);
    let json = args.windows(2).any(|w| (w[0] == "-o" || w[0] == "--output") && w[1] == "json");
    let kernel = args.iter().any(|a| a == "-k" || a == "--dmesg");

    if kernel {
        println!("Jan 15 08:00:00 slateos kernel: Linux version 6.7.0-slateos");
        println!("Jan 15 08:00:00 slateos kernel: Command line: root=/dev/sda2 ro");
        println!("Jan 15 08:00:00 slateos kernel: Memory: 16384MB available");
        println!("Jan 15 08:00:00 slateos kernel: CPU: 4 cores detected");
        println!("Jan 15 08:00:01 slateos kernel: ACPI: RSDP found");
        return 0;
    }

    let svc = unit.unwrap_or("system");
    let max = lines.min(8);

    if json {
        for i in 0..max {
            println!("{{\"__REALTIME_TIMESTAMP\":\"170523600{}000000\",\"_HOSTNAME\":\"slateos\",\"SYSLOG_IDENTIFIER\":\"{}\",\"MESSAGE\":\"Log entry {}\"}}", i, svc, i + 1);
        }
    } else {
        let messages = [
            "Started service",
            "Listening on port 8080",
            "Connection from 192.168.1.100",
            "Request processed in 12ms",
            "Health check passed",
            "Cache refreshed",
            "Worker thread started",
            "Configuration reloaded",
        ];
        for i in 0..max {
            let msg = messages.get(i).copied().unwrap_or("Log entry");
            println!("Jan 15 12:{:02}:{:02} slateos {}[1234]: {}", i, i * 5, svc, msg);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_journalctl(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_journalctl};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_journalctl(vec!["--help".to_string()]), 0);
        assert_eq!(run_journalctl(vec!["-h".to_string()]), 0);
        let _ = run_journalctl(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_journalctl(vec![]);
    }
}
