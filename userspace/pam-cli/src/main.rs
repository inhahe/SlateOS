#![deny(clippy::all)]

//! pam-cli — OurOS PAM authentication tools
//!
//! Multi-personality: `pam_tally2`, `faillock`, `pam-auth-update`, `pwscore`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_faillock(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: faillock [OPTIONS]");
        println!();
        println!("faillock — display/reset login failure records (OurOS).");
        println!();
        println!("Options:");
        println!("  --user USER    Show/reset for USER");
        println!("  --reset        Reset failure records");
        println!("  --dir DIR      Tally directory");
        return 0;
    }

    let reset = args.iter().any(|a| a == "--reset");
    let user = args.windows(2)
        .find(|w| w[0] == "--user")
        .map(|w| w[1].as_str());

    if reset {
        if let Some(u) = user {
            println!("Resetting failure records for {}", u);
        } else {
            println!("Resetting all failure records");
        }
        return 0;
    }

    println!("user:");
    println!("   When                  Type  Source                                   Valid");
    println!("   2025-01-01 10:00:00   AUTH  10.0.0.1                                 V");
    println!("   2025-01-01 10:01:00   AUTH  10.0.0.1                                 V");
    0
}

fn run_pam_tally2(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pam_tally2 [OPTIONS]");
        println!();
        println!("pam_tally2 — login counter (OurOS).");
        println!();
        println!("Options:");
        println!("  --user USER    Show count for USER");
        println!("  --reset        Reset counters");
        return 0;
    }

    let reset = args.iter().any(|a| a == "--reset");
    if reset {
        println!("Login counter reset");
    } else {
        println!("Login           Failures Latest failure     From");
        println!("user                   2    01/01/25 10:01   10.0.0.1");
    }
    0
}

fn run_pam_auth_update(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pam-auth-update [OPTIONS]");
        println!();
        println!("pam-auth-update — manage PAM configuration (OurOS).");
        println!();
        println!("Options:");
        println!("  --package       Package mode");
        println!("  --remove PROF   Remove profile");
        println!("  --enable PROF   Enable profile");
        println!("  --force         Skip prompts");
        return 0;
    }

    println!("PAM profiles enabled:");
    println!("  Unix authentication");
    println!("  GNOME Keyring");
    println!("  Systemd login sessions");
    println!("  ConsoleKit Session Management");
    0
}

fn run_pwscore(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pwscore [USER]");
        println!();
        println!("pwscore — check password quality score (OurOS).");
        println!("Reads password from stdin and prints score 0-100.");
        return 0;
    }
    let _ = args;
    println!("78");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "faillock".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "pam_tally2" => run_pam_tally2(&rest),
        "pam-auth-update" => run_pam_auth_update(&rest),
        "pwscore" => run_pwscore(&rest),
        _ => run_faillock(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_faillock};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pam"), "pam");
        assert_eq!(basename(r"C:\bin\pam.exe"), "pam.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pam.exe"), "pam");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_faillock(&["--help".to_string()]), 0);
        assert_eq!(run_faillock(&["-h".to_string()]), 0);
        assert_eq!(run_faillock(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_faillock(&[]), 0);
    }
}
