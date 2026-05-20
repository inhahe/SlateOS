//! OurOS nologin shell.
//!
//! Multi-personality binary providing:
//! - **nologin** — politely refuse a login
//! - **false** — do nothing, unsuccessfully
//! - **true** — do nothing, successfully
//!
//! `nologin` displays a message and exits non-zero, used as the shell for
//! system accounts that should not have interactive logins.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::process;

const VERSION: &str = "0.1.0";
const NOLOGIN_MSG_FILE: &str = "/etc/nologin.txt";

const DEFAULT_MESSAGE: &str = "This account is currently not available.";

fn cmd_nologin(args: &[String]) {
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: nologin [options]");
                println!();
                println!("Politely refuse a login.");
                println!("Displays /etc/nologin.txt if it exists, otherwise a default message.");
                println!();
                println!("Options:");
                println!("  -h, --help     Show this help");
                println!("  --version      Show version");
                // Even --help exits non-zero for nologin.
                process::exit(1);
            }
            "--version" => {
                println!("nologin {VERSION}");
                process::exit(1);
            }
            _ => {}
        }
    }

    // Display custom message or default.
    let message = fs::read_to_string(NOLOGIN_MSG_FILE).unwrap_or_else(|_| DEFAULT_MESSAGE.to_string());
    eprintln!("{message}");
    process::exit(1);
}

fn cmd_false(_args: &[String]) {
    process::exit(1);
}

fn cmd_true(_args: &[String]) {
    process::exit(0);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("nologin");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "false" => cmd_false(&rest),
        "true" => cmd_true(&rest),
        _ => cmd_nologin(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_message() {
        assert!(!DEFAULT_MESSAGE.is_empty());
        assert!(DEFAULT_MESSAGE.contains("not available"));
    }

    #[test]
    fn test_version_constant() {
        assert_eq!(VERSION, "0.1.0");
    }

    #[test]
    fn test_nologin_msg_path() {
        assert_eq!(NOLOGIN_MSG_FILE, "/etc/nologin.txt");
    }
}
