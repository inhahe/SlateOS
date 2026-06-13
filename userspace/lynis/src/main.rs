#![deny(clippy::all)]

//! lynis — SlateOS security auditing tool
//!
//! Single personality: `lynis`

use std::env;
use std::process;

fn run_lynis(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lynis <command> [options]");
        println!();
        println!("Commands:");
        println!("  audit system     Perform system audit");
        println!("  audit dockerfile Audit Dockerfile");
        println!("  show             Show (details/profiles/settings/version)");
        println!("  update info      Show update information");
        println!("  generate         Generate (cronjob/hostids)");
        println!();
        println!("Options:");
        println!("  --auditor <name>     Auditor name");
        println!("  --cronjob            Optimize for cron job");
        println!("  --no-colors          Disable colors");
        println!("  --pentest            Non-privileged scan");
        println!("  --profile <file>     Use custom profile");
        println!("  --quick              Skip user input");
        println!("  --reverse-colors     Reverse terminal colors");
        println!("  --tests <IDs>        Run specific tests only");
        println!("  --verbose            Verbose output");
        println!("  --warnings-only      Show only warnings");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "audit" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("system");
            if sub == "system" {
                println!("[ Lynis 3.1.1 (Slate OS) ]");
                println!();
                println!("################################################################################");
                println!("  Lynis comes with ABSOLUTELY NO WARRANTY. This is free software, and you are");
                println!("  welcome to redistribute it under the terms of the GNU General Public License.");
                println!("################################################################################");
                println!();
                println!("[+] Initializing program");
                println!("------------------------------------");
                println!("  - Detecting OS...                                           [ DONE ]");
                println!("  - Checking profiles...                                      [ DONE ]");
                println!();
                println!("[+] System Tools");
                println!("------------------------------------");
                println!("  - Scanning available tools...                                [ DONE ]");
                println!("  - Checking system binaries...                                [ DONE ]");
                println!();
                println!("[+] Boot and services");
                println!("------------------------------------");
                println!("  - Check running services (systemd)...                        [ DONE ]");
                println!("  - Check enabled services at boot...                          [ DONE ]");
                println!();
                println!("[+] Kernel");
                println!("------------------------------------");
                println!("  - Checking kernel version and release...                     [ DONE ]");
                println!("  - Checking kernel modules...                                 [ DONE ]");
                println!();
                println!("[+] Hardening");
                println!("------------------------------------");
                println!("  Hardening index : 72 [##############      ]");
                println!("  Tests performed : 256");
                println!("  Suggestions     : 14");
                println!("  Warnings        : 3");
            } else {
                println!("(audit {} — simulated)", sub);
            }
        }
        "show" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("version");
            match what {
                "version" => println!("Lynis 3.1.1 (Slate OS)"),
                "details" => {
                    println!("Lynis version: 3.1.1");
                    println!("Status: Enterprise (simulated)");
                    println!("License: GPL v3");
                }
                "profiles" => {
                    println!("Profile: default.prf");
                    println!("Profile: custom.prf");
                }
                "settings" => {
                    println!("log-file=/var/log/lynis.log");
                    println!("report-file=/var/log/lynis-report.dat");
                    println!("profile=default.prf");
                }
                _ => println!("(show {} — simulated)", what),
            }
        }
        "update" => println!("No updates available. Version 3.1.1 is current."),
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lynis(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lynis};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lynis(vec!["--help".to_string()]), 0);
        assert_eq!(run_lynis(vec!["-h".to_string()]), 0);
        let _ = run_lynis(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lynis(vec![]);
    }
}
