#![deny(clippy::all)]

//! saltstack-cli — Slate OS SaltStack configuration management
//!
//! Multi-personality: `salt`, `salt-key`, `salt-call`, `salt-run`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_salt(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: salt [OPTIONS] <target> <function> [arguments]");
        println!();
        println!("salt — SaltStack remote execution (Slate OS).");
        println!();
        println!("Options:");
        println!("  -G    Match by grain");
        println!("  -E    Match by regex");
        println!("  -L    Match by list");
        println!("  --out json   JSON output");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("salt 3007.0 (Potassium) (Slate OS)");
        return 0;
    }

    let target = args.first().map(|s| s.as_str()).unwrap_or("*");
    let func = args.get(1).map(|s| s.as_str()).unwrap_or("test.ping");
    match func {
        "test.ping" => {
            println!("slateos-node-1:");
            println!("    True");
            println!("slateos-node-2:");
            println!("    True");
        }
        "cmd.run" => {
            let cmd = args.get(2).map(|s| s.as_str()).unwrap_or("uname -a");
            println!("{}:", target);
            println!("    Slate OS slateos-desktop 1.0 x86_64 ({})", cmd);
        }
        "grains.items" | "grains.item" => {
            println!("slateos-node-1:");
            println!("    ----------");
            println!("    os:");
            println!("        Slate OS");
            println!("    osrelease:");
            println!("        1.0");
            println!("    kernel:");
            println!("        slateos");
            println!("    cpuarch:");
            println!("        x86_64");
        }
        "state.apply" | "state.highstate" => {
            println!("slateos-node-1:");
            println!("----------");
            println!("          ID: nginx");
            println!("    Function: pkg.installed");
            println!("      Result: True");
            println!("     Comment: Package nginx is already installed.");
            println!("     Started: 12:00:00.000000");
            println!("    Duration: 456.789 ms");
            println!("     Changes:");
            println!();
            println!("Summary for slateos-node-1");
            println!("------------");
            println!("Succeeded: 5 (changed=1)");
            println!("Failed:    0");
            println!("Total states run: 5");
        }
        "pkg.install" | "pkg.remove" => {
            let pkg = args.get(2).map(|s| s.as_str()).unwrap_or("nginx");
            println!("{}:", target);
            println!("    ----------");
            println!("    {}:", pkg);
            println!("        new: 1.24.0");
            println!("        old:");
        }
        _ => {
            println!("{}:", target);
            println!("    {} completed", func);
        }
    }
    0
}

fn run_salt_key(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: salt-key [OPTIONS]");
        println!("  -L    List all keys");
        println!("  -A    Accept all pending");
        println!("  -a    Accept key");
        println!("  -d    Delete key");
        return 0;
    }

    if args.iter().any(|a| a == "-L") {
        println!("Accepted Keys:");
        println!("slateos-node-1");
        println!("slateos-node-2");
        println!("Denied Keys:");
        println!("Unaccepted Keys:");
        println!("slateos-node-3");
        println!("Rejected Keys:");
    } else if args.iter().any(|a| a == "-A") {
        println!("The following keys are going to be accepted:");
        println!("Unaccepted Keys:");
        println!("slateos-node-3");
        println!("Key for minion slateos-node-3 accepted.");
    } else {
        println!("salt-key: operation completed");
    }
    0
}

fn run_salt_call(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: salt-call [OPTIONS] <function> [arguments]");
        println!("  --local    Run locally without master");
        return 0;
    }
    let func = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("test.ping");
    println!("local:");
    println!("    True ({})", func);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "salt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "salt-key" => run_salt_key(&rest),
        "salt-call" => run_salt_call(&rest),
        "salt-run" => { println!("salt-run: runner completed"); 0 }
        _ => run_salt(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_salt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/saltstack"), "saltstack");
        assert_eq!(basename(r"C:\bin\saltstack.exe"), "saltstack.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("saltstack.exe"), "saltstack");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_salt(&["--help".to_string()]), 0);
        assert_eq!(run_salt(&["-h".to_string()]), 0);
        let _ = run_salt(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_salt(&[]);
    }
}
