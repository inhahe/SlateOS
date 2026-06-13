#![deny(clippy::all)]

//! logrotate-cli — Slate OS logrotate CLI
//!
//! Single personality: `logrotate`

use std::env;
use std::process;

fn run_logrotate(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logrotate [OPTIONS] CONFIG_FILE...");
        println!();
        println!("logrotate — rotate, compress, and remove log files (Slate OS).");
        println!();
        println!("Options:");
        println!("  -d, --debug            Debug mode (dry run)");
        println!("  -f, --force            Force rotation");
        println!("  -v, --verbose          Verbose output");
        println!("  -s, --state FILE       State file path");
        println!("  -m, --mail COMMAND     Mail command");
        println!("  --usage                Show usage");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("logrotate 3.21.0 (Slate OS)");
        return 0;
    }

    let debug = args.iter().any(|a| a == "-d" || a == "--debug");
    let force = args.iter().any(|a| a == "-f" || a == "--force");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose") || debug;

    let configs: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let config = configs.first().copied().unwrap_or("/etc/logrotate.conf");

    if verbose {
        println!("reading config file {}", config);
        println!("  including /etc/logrotate.d");
        println!("  reading config file /etc/logrotate.d/nginx");
        println!("  reading config file /etc/logrotate.d/syslog");
    }

    if debug {
        println!();
        println!("Handling 2 log files");
        println!();
        println!("rotating pattern: /var/log/nginx/*.log after 1 days (7 rotations)");
        println!("empty log files are not rotated");
        println!("considering log /var/log/nginx/access.log");
        println!("  Now: 2024-01-15 12:00");
        println!("  Last rotated at 2024-01-14 00:00");
        if force {
            println!("  log needs rotating (forced)");
        } else {
            println!("  log needs rotating");
        }
        println!("  (dry run) rotating /var/log/nginx/access.log");
        println!("  (dry run) renaming /var/log/nginx/access.log.6.gz -> /var/log/nginx/access.log.7.gz");
        println!("  (dry run) compressing /var/log/nginx/access.log.1 -> /var/log/nginx/access.log.1.gz");
        println!("  (dry run) postrotate: /usr/sbin/nginx -s reload");
    } else {
        if verbose {
            println!();
            println!("rotating /var/log/nginx/access.log");
            println!("  renaming /var/log/nginx/access.log.6.gz -> /var/log/nginx/access.log.7.gz");
            println!("  renaming /var/log/nginx/access.log.5.gz -> /var/log/nginx/access.log.6.gz");
            println!("  compressing /var/log/nginx/access.log.1 -> /var/log/nginx/access.log.1.gz");
            println!("  running postrotate script");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logrotate(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_logrotate};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_logrotate(vec!["--help".to_string()]), 0);
        assert_eq!(run_logrotate(vec!["-h".to_string()]), 0);
        let _ = run_logrotate(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_logrotate(vec![]);
    }
}
