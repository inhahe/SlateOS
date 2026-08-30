#![deny(clippy::all)]

//! hydra-cli — Slate OS Hydra password cracker CLI
//!
//! Single personality: `hydra`

use std::env;
use std::process;

fn run_hydra(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hydra [OPTIONS] TARGET SERVICE");
        println!();
        println!("Hydra — network login cracker (Slate OS).");
        println!();
        println!("Options:");
        println!("  -l LOGIN               Single login name");
        println!("  -L FILE                Login name list");
        println!("  -p PASS                Single password");
        println!("  -P FILE                Password list");
        println!("  -C FILE                Colon-separated login:pass file");
        println!("  -s PORT                Target port");
        println!("  -t TASKS               Parallel connections (default 16)");
        println!("  -w TIME                Wait time between connect (default 0)");
        println!("  -f                     Exit on first found");
        println!("  -v / -V                Verbose / show each attempt");
        println!("  -o FILE                Output file");
        println!("  -R                     Restore previous session");
        println!();
        println!("Supported services: ssh, ftp, telnet, http-get, http-post-form,");
        println!("  smtp, pop3, imap, mysql, mssql, postgres, vnc, rdp, smb, snmp");
        return 0;
    }
    if args.iter().any(|a| a == "-V" && a.len() == 2) || args.iter().any(|a| a == "--version") {
        // Note: -V is verbose mode but we check --version too
    }

    let login = args.windows(2).find(|w| w[0] == "-l")
        .map(|w| w[1].as_str()).unwrap_or("admin");
    let tasks = args.windows(2).find(|w| w[0] == "-t")
        .map(|w| w[1].as_str()).unwrap_or("16");
    let exit_first = args.iter().any(|a| a == "-f");

    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let target = positional.first().copied().unwrap_or("192.168.1.1");
    let service = positional.get(1).copied().unwrap_or("ssh");

    println!("Hydra v9.5 (Slate OS) starting at 2024-01-15 12:00:00");
    println!("[DATA] max {} tasks per 1 server, overall {} tasks", tasks, tasks);
    println!("[DATA] attacking {}://{}:{}", service, target, match service {
        "ssh" => "22",
        "ftp" => "21",
        "http-get" => "80",
        "rdp" => "3389",
        "mysql" => "3306",
        "smb" => "445",
        _ => "22",
    });

    println!("[ATTEMPT] target {} - login \"{}\" - pass \"password\" - 1 of 1000", target, login);
    println!("[ATTEMPT] target {} - login \"{}\" - pass \"123456\" - 2 of 1000", target, login);
    println!("[ATTEMPT] target {} - login \"{}\" - pass \"admin\" - 3 of 1000", target, login);
    println!("[22][{}] host: {}   login: {}   password: admin123", service, target, login);

    if exit_first {
        println!("[STATUS] attack finished for {} (valid pair found)", target);
    }

    println!("1 of 1 target successfully completed, 1 valid password found");
    println!("Hydra finished at 2024-01-15 12:00:45");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hydra(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_hydra};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hydra(vec!["--help".to_string()]), 0);
        assert_eq!(run_hydra(vec!["-h".to_string()]), 0);
        let _ = run_hydra(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hydra(vec![]);
    }
}
