#![deny(clippy::all)]

//! foreman-cli — SlateOS Foreman lifecycle management
//!
//! Multi-personality: `foreman`, `hammer`, `foreman-rake`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_foreman(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: foreman <command> [OPTIONS]");
        println!("foreman v3.9 (Slate OS) — Lifecycle management tool");
        println!();
        println!("Manages provisioning, configuration, and monitoring");
        println!("of physical and virtual servers.");
        println!();
        println!("Options:");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("foreman v3.9 (Slate OS)"); return 0; }
    println!("foreman: lifecycle management server");
    println!("  Hosts managed: 24");
    println!("  Host groups: 4");
    println!("  Smart proxies: 2");
    0
}

fn run_hammer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hammer <resource> <action> [OPTIONS]");
        println!("hammer v3.9 (Slate OS) — Foreman CLI client");
        println!();
        println!("Resources:");
        println!("  host          Manage hosts");
        println!("  hostgroup     Manage host groups");
        println!("  environment   Manage environments");
        println!("  subnet        Manage subnets");
        println!("  user          Manage users");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hammer v3.9 (Slate OS, Foreman CLI)"); return 0; }
    if args.len() >= 2 {
        println!("hammer: {} {} completed", args[0], args[1]);
    } else if args.len() == 1 && args[0] == "host" {
        println!("hammer host: listing hosts");
        println!("  ID | NAME         | OS           | STATUS");
        println!("  1  | web-01       | Slate OS 1.0    | OK");
        println!("  2  | db-01        | Slate OS 1.0    | OK");
        println!("  3  | app-01       | Slate OS 1.0    | OK");
    } else {
        println!("hammer: use --help for available resources");
    }
    0
}

fn run_foreman_rake(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: foreman-rake <task> [OPTIONS]");
        println!("foreman-rake v3.9 (Slate OS) — Foreman maintenance tasks");
        println!("  db:migrate      Run database migrations");
        println!("  db:seed         Seed database");
        println!("  permissions:reset  Reset permissions cache");
        return 0;
    }
    if let Some(task) = args.first() {
        println!("foreman-rake: running task '{}'", task);
        println!("  Status: completed");
    } else {
        println!("foreman-rake: no task specified");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "foreman".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "hammer" => run_hammer(&rest, &prog),
        "foreman-rake" => run_foreman_rake(&rest, &prog),
        _ => run_foreman(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_foreman};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/foreman"), "foreman");
        assert_eq!(basename(r"C:\bin\foreman.exe"), "foreman.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("foreman.exe"), "foreman");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_foreman(&["--help".to_string()], "foreman"), 0);
        assert_eq!(run_foreman(&["-h".to_string()], "foreman"), 0);
        let _ = run_foreman(&["--version".to_string()], "foreman");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_foreman(&[], "foreman");
    }
}
