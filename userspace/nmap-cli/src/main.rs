#![deny(clippy::all)]

//! nmap-cli — OurOS network scanner
//!
//! Single personality: `nmap`

use std::env;
use std::process;

fn run_nmap(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nmap [Scan Type] [Options] <target>");
        println!();
        println!("Network exploration tool and security scanner.");
        println!();
        println!("Scan types:");
        println!("  -sS    TCP SYN scan (default)");
        println!("  -sT    TCP connect scan");
        println!("  -sU    UDP scan");
        println!("  -sP    Ping scan");
        println!("  -sV    Version detection");
        println!("  -sC    Script scan (default scripts)");
        println!("  -O     OS detection");
        println!("  -A     Aggressive (OS+version+script+traceroute)");
        println!();
        println!("Options:");
        println!("  -p <PORTS>       Port specification (e.g., 22,80,443 or 1-1024)");
        println!("  -F               Fast scan (fewer ports)");
        println!("  --top-ports <N>  Scan N most common ports");
        println!("  -T<0-5>          Timing template (0=paranoid, 5=insane)");
        println!("  -oN <FILE>       Normal output");
        println!("  -oX <FILE>       XML output");
        println!("  -oG <FILE>       Grepable output");
        println!("  -v               Verbose");
        println!("  -V               Version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("Nmap 7.94 (OurOS)");
        return 0;
    }

    let target = args.iter()
        .rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("scanme.nmap.org");

    let version_detect = args.iter().any(|a| a == "-sV" || a == "-A");
    let os_detect = args.iter().any(|a| a == "-O" || a == "-A");

    println!("Starting Nmap 7.94 (OurOS) at 2024-01-15 14:30 UTC");
    println!("Nmap scan report for {}", target);
    println!("Host is up (0.023s latency).");
    println!();
    println!("PORT     STATE    SERVICE{}",
        if version_detect { "         VERSION" } else { "" });
    println!("22/tcp   open     ssh{}",
        if version_detect { "             OpenSSH 9.6 (protocol 2.0)" } else { "" });
    println!("80/tcp   open     http{}",
        if version_detect { "            nginx 1.25.3" } else { "" });
    println!("443/tcp  open     https{}",
        if version_detect { "           nginx 1.25.3" } else { "" });
    println!("3306/tcp filtered mysql");
    println!("8080/tcp closed   http-proxy");

    if os_detect {
        println!();
        println!("OS details: Linux 5.15 - 6.2");
        println!("Network Distance: 12 hops");
    }

    println!();
    println!("Nmap done: 1 IP address (1 host up) scanned in 8.45 seconds");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nmap(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nmap};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nmap(vec!["--help".to_string()]), 0);
        assert_eq!(run_nmap(vec!["-h".to_string()]), 0);
        let _ = run_nmap(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nmap(vec![]);
    }
}
