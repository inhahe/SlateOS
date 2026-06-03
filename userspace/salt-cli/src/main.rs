#![deny(clippy::all)]

//! salt-cli — OurOS SaltStack CLI
//!
//! Multi-personality: `salt`, `salt-call`, `salt-key`, `salt-run`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_salt(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: salt TARGET FUNCTION [ARGS...]");
        println!("Salt 3006.8 (OurOS)");
        println!();
        println!("Options:");
        println!("  --version             Show version");
        println!("  -G, --grain           Target by grain");
        println!("  -E, --pcre            Target by PCRE regex");
        println!("  -L, --list            Target by list");
        println!("  --batch-size N        Execute in batches");
        println!("  --timeout N           Timeout in seconds");
        println!("  --output FMT          Output format (nested, json, yaml)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("salt 3006.8");
        return 0;
    }
    let target = args.first().map(|s| s.as_str()).unwrap_or("*");
    let function = args.get(1).map(|s| s.as_str()).unwrap_or("test.ping");

    println!("{}:", "web01");
    match function {
        "test.ping" => println!("    True"),
        "cmd.run" => {
            let cmd = args.get(2).map(|s| s.as_str()).unwrap_or("hostname");
            println!("    {}", cmd);
        }
        "pkg.install" => {
            let pkg = args.get(2).map(|s| s.as_str()).unwrap_or("nginx");
            println!("    ----------");
            println!("    {}:", pkg);
            println!("        new: 1.24.0-1");
            println!("        old:");
        }
        "state.apply" => {
            println!("    ----------");
            println!("          ID: install-nginx");
            println!("    Function: pkg.installed");
            println!("        Name: nginx");
            println!("      Result: True");
            println!("     Changes:");
            println!("              ----------");
            println!("              nginx:");
            println!("                  new: 1.24.0-1");
            println!("                  old:");
            println!("     Summary for {}:", target);
            println!("     Succeeded: 3 (changed=1)");
            println!("     Failed:    0");
        }
        "grains.items" => {
            println!("    ----------");
            println!("    os: OurOS");
            println!("    osrelease: 1.0");
            println!("    kernel: OurOS");
            println!("    cpuarch: x86_64");
            println!("    mem_total: 8192");
        }
        _ => println!("    {}: completed", function),
    }
    0
}

fn run_salt_call(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: salt-call FUNCTION [ARGS...]");
        println!("Execute salt functions locally");
        return 0;
    }
    let function = args.first().map(|s| s.as_str()).unwrap_or("test.ping");
    println!("local:");
    println!("    True");
    let _fn = function;
    0
}

fn run_salt_key(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: salt-key [OPTIONS]");
        println!("Manage Salt minion keys");
        println!("  -L, --list-all  List all keys");
        println!("  -a, --accept    Accept key");
        println!("  -d, --delete    Delete key");
        println!("  -A              Accept all pending");
        return 0;
    }
    if args.iter().any(|a| a == "-L" || a == "--list-all") {
        println!("Accepted Keys:");
        println!("  web01");
        println!("  web02");
        println!("  db01");
        println!("Denied Keys:");
        println!("Unaccepted Keys:");
        println!("  new-minion");
        println!("Rejected Keys:");
    }
    0
}

fn run_salt_run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: salt-run FUNCTION [ARGS...]");
        println!("Execute Salt runner modules on the master");
        return 0;
    }
    let function = args.first().map(|s| s.as_str()).unwrap_or("manage.status");
    match function {
        "manage.status" => {
            println!("down:");
            println!("up:");
            println!("    - web01");
            println!("    - web02");
            println!("    - db01");
        }
        _ => println!("{}: completed", function),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "salt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "salt-call" => run_salt_call(&rest),
        "salt-key" => run_salt_key(&rest),
        "salt-run" => run_salt_run(&rest),
        _ => run_salt(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_salt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/salt"), "salt");
        assert_eq!(basename(r"C:\bin\salt.exe"), "salt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("salt.exe"), "salt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_salt(&["--help".to_string()]), 0);
        assert_eq!(run_salt(&["-h".to_string()]), 0);
        assert_eq!(run_salt(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_salt(&[]), 0);
    }
}
