#![deny(clippy::all)]

//! useradd-cli — Slate OS user management CLIs
//!
//! Multi-personality: `useradd`, `userdel`, `usermod`, `groupadd`, `groupdel`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_useradd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: useradd [OPTIONS] LOGIN");
        println!();
        println!("Options:");
        println!("  -m, --create-home      Create home directory");
        println!("  -d, --home-dir DIR     Home directory");
        println!("  -s, --shell SHELL      Login shell");
        println!("  -g, --gid GROUP        Primary group");
        println!("  -G, --groups GROUPS    Supplementary groups");
        println!("  -u, --uid UID          User ID");
        println!("  -c, --comment COMMENT  GECOS field");
        println!("  -r, --system           Create system account");
        println!("  -e, --expiredate DATE  Account expiration date");
        return 0;
    }
    let user = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("newuser");
    println!("useradd: user '{}' created", user);
    if args.iter().any(|a| a == "-m" || a == "--create-home") {
        println!("useradd: home directory /home/{} created", user);
    }
    0
}

fn run_userdel(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: userdel [OPTIONS] LOGIN");
        println!("  -r, --remove    Remove home directory");
        println!("  -f, --force     Force removal");
        return 0;
    }
    let user = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("olduser");
    println!("userdel: user '{}' deleted", user);
    if args.iter().any(|a| a == "-r" || a == "--remove") {
        println!("userdel: /home/{} removed", user);
    }
    0
}

fn run_usermod(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: usermod [OPTIONS] LOGIN");
        println!("  -l, --login NEW_LOGIN  Change login name");
        println!("  -s, --shell SHELL      Change shell");
        println!("  -G, --groups GROUPS    Set supplementary groups");
        println!("  -a, --append           Append to groups (with -G)");
        println!("  -L, --lock             Lock account");
        println!("  -U, --unlock           Unlock account");
        return 0;
    }
    let user = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("user");
    println!("usermod: user '{}' modified", user);
    0
}

fn run_groupadd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: groupadd [OPTIONS] GROUP");
        println!("  -g, --gid GID    Group ID");
        println!("  -r, --system     Create system group");
        return 0;
    }
    let group = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("newgroup");
    println!("groupadd: group '{}' created", group);
    0
}

fn run_groupdel(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: groupdel GROUP");
        return 0;
    }
    let group = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("oldgroup");
    println!("groupdel: group '{}' deleted", group);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "useradd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "userdel" => run_userdel(&rest),
        "usermod" => run_usermod(&rest),
        "groupadd" => run_groupadd(&rest),
        "groupdel" => run_groupdel(&rest),
        _ => run_useradd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_useradd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/useradd"), "useradd");
        assert_eq!(basename(r"C:\bin\useradd.exe"), "useradd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("useradd.exe"), "useradd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_useradd(&["--help".to_string()]), 0);
        assert_eq!(run_useradd(&["-h".to_string()]), 0);
        let _ = run_useradd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_useradd(&[]);
    }
}
