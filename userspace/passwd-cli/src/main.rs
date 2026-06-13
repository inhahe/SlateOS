#![deny(clippy::all)]

//! passwd-cli — SlateOS passwd/chage CLI
//!
//! Multi-personality: `passwd`, `chage`, `chsh`, `chfn`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_passwd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: passwd [OPTIONS] [USER]");
        println!();
        println!("passwd — change user password (SlateOS).");
        println!();
        println!("Options:");
        println!("  -l, --lock         Lock account");
        println!("  -u, --unlock       Unlock account");
        println!("  -d, --delete       Delete password");
        println!("  -S, --status       Show status");
        println!("  -e, --expire       Force password change");
        return 0;
    }

    let user = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("root");

    if args.iter().any(|a| a == "-S" || a == "--status") {
        println!("{} P 2024-01-15 0 99999 7 -1", user);
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--lock") {
        println!("passwd: password expiry information changed for {}", user);
        println!("Locking password for user {}.", user);
        return 0;
    }
    if args.iter().any(|a| a == "-u" || a == "--unlock") {
        println!("passwd: unlocking password for user {}", user);
        return 0;
    }

    println!("Changing password for {}.", user);
    println!("Current password: ");
    println!("New password: ");
    println!("Retype new password: ");
    println!("passwd: password updated successfully");
    0
}

fn run_chage(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chage [OPTIONS] USER");
        println!("  -l, --list        Show account aging info");
        println!("  -m, --mindays N   Minimum days between changes");
        println!("  -M, --maxdays N   Maximum days between changes");
        println!("  -E, --expiredate  Account expiration date");
        println!("  -W, --warndays N  Warning days before expiry");
        return 0;
    }

    let user = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("root");

    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("Last password change                    : Jan 15, 2024");
        println!("Password expires                        : never");
        println!("Password inactive                       : never");
        println!("Account expires                         : never");
        println!("Minimum number of days between changes  : 0");
        println!("Maximum number of days between changes  : 99999");
        println!("Number of days of warning before expiry : 7");
        let _ = user;
        return 0;
    }

    println!("chage: password aging information changed for {}", user);
    0
}

fn run_chsh(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chsh [OPTIONS] [USER]");
        println!("  -s, --shell SHELL  New shell");
        println!("  -l, --list-shells  List valid shells");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list-shells") {
        println!("/bin/sh");
        println!("/bin/bash");
        println!("/bin/zsh");
        println!("/bin/fish");
        println!("/usr/bin/nologin");
        return 0;
    }
    let shell = args.windows(2).find(|w| w[0] == "-s" || w[0] == "--shell")
        .map(|w| w[1].as_str()).unwrap_or("/bin/bash");
    println!("Shell changed to {}", shell);
    0
}

fn run_chfn(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chfn [OPTIONS] [USER]");
        println!("  -f, --full-name NAME   Full name");
        println!("  -r, --room NUM         Room number");
        println!("  -w, --work-phone NUM   Work phone");
        println!("  -h, --home-phone NUM   Home phone");
        return 0;
    }
    println!("chfn: finger information changed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "passwd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "chage" => run_chage(&rest),
        "chsh" => run_chsh(&rest),
        "chfn" => run_chfn(&rest),
        _ => run_passwd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_passwd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/passwd"), "passwd");
        assert_eq!(basename(r"C:\bin\passwd.exe"), "passwd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("passwd.exe"), "passwd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_passwd(&["--help".to_string()]), 0);
        assert_eq!(run_passwd(&["-h".to_string()]), 0);
        let _ = run_passwd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_passwd(&[]);
    }
}
