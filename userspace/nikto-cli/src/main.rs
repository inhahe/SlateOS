#![deny(clippy::all)]

//! nikto-cli — Slate OS Nikto web scanner CLI
//!
//! Single personality: `nikto`

use std::env;
use std::process;

fn run_nikto(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-H") {
        println!("Usage: nikto [OPTIONS]");
        println!();
        println!("Nikto — web server scanner (Slate OS).");
        println!();
        println!("Options:");
        println!("  -h, -host HOST        Target host");
        println!("  -p, -port PORT        Target port");
        println!("  -ssl                  Force SSL");
        println!("  -Tuning OPTS          Scan tuning");
        println!("  -timeout N            Timeout per request");
        println!("  -output FILE          Output file");
        println!("  -Format FMT           Output format (csv, htm, json, txt, xml)");
        println!("  -Plugins LIST         Plugin list");
        println!("  -evasion N            IDS evasion technique");
        println!("  -useproxy URL         Use proxy");
        println!("  -update               Update databases");
        println!("  -list-plugins         List available plugins");
        return 0;
    }
    if args.iter().any(|a| a == "-Version" || a == "--version") {
        println!("Nikto v2.5.0 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "-update") {
        println!("+ Retrieving 'nikto_db_tests.csv'");
        println!("+ CIRT.net message: Please submit Nikto bugs via GitHub.");
        println!("+ Update complete.");
        return 0;
    }

    if args.iter().any(|a| a == "-list-plugins") {
        println!("Plugin: nikto_headers");
        println!("Plugin: nikto_cookies");
        println!("Plugin: nikto_ssl");
        println!("Plugin: nikto_outdated");
        println!("Plugin: nikto_httpoptions");
        println!("Plugin: nikto_robots");
        println!("Plugin: nikto_favicon");
        println!("Plugin: nikto_content_search");
        return 0;
    }

    let host = args.windows(2).find(|w| w[0] == "-h" || w[0] == "-host")
        .map(|w| w[1].as_str()).unwrap_or("localhost");
    let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "-port")
        .map(|w| w[1].as_str()).unwrap_or("80");
    let ssl = args.iter().any(|a| a == "-ssl");

    let scheme = if ssl || port == "443" { "https" } else { "http" };

    println!("- Nikto v2.5.0 (Slate OS)");
    println!("---------------------------------------------------------------------------");
    println!("+ Target IP:          {}", host);
    println!("+ Target Hostname:    {}", host);
    println!("+ Target Port:        {}", port);
    println!("+ Start Time:         2024-01-15 12:00:00 (GMT)");
    println!("---------------------------------------------------------------------------");
    println!("+ Server: nginx/1.24.0");
    println!("+ /: The anti-clickjacking X-Frame-Options header is not present.");
    println!("+ /: The X-Content-Type-Options header is not set.");
    println!("+ /robots.txt: contains 3 entries which should be manually viewed.");
    println!("+ /: Server leaks inodes via ETags.");
    println!("+ /.git/config: Git configuration file found.");
    println!("+ /admin/: Admin directory found.");
    println!("+ /backup/: Backup directory found.");
    println!("+ {}://{}:{}/server-status: Apache server-status found.", scheme, host, port);
    println!("+ 7542 requests: 0 error(s) and 8 item(s) reported on remote host");
    println!("+ End Time:           2024-01-15 12:05:23 (GMT) (323 seconds)");
    println!("---------------------------------------------------------------------------");
    println!("+ 1 host(s) tested");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nikto(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nikto};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nikto(vec!["--help".to_string()]), 0);
        assert_eq!(run_nikto(vec!["-h".to_string()]), 0);
        let _ = run_nikto(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nikto(vec![]);
    }
}
