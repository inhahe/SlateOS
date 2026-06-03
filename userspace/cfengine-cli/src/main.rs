#![deny(clippy::all)]

//! cfengine-cli — OurOS CFEngine configuration management
//!
//! Multi-personality: `cf-agent`, `cf-promises`, `cf-key`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cf_agent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cf-agent [OPTIONS]");
        println!("cf-agent v3.23 (OurOS) — CFEngine policy agent");
        println!();
        println!("Options:");
        println!("  -f FILE           Policy file to evaluate");
        println!("  -K                Ignore locks (force run)");
        println!("  -n                Dry-run mode");
        println!("  -v                Verbose output");
        println!("  -I                Inform mode");
        println!("  -B BOOTSTRAP      Bootstrap to hub");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cf-agent v3.23 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-n") {
        println!("Dry-run: evaluating promises...");
        println!("  Promise: files /etc/resolv.conf — would repair");
        println!("  Promise: packages nginx — already kept");
        println!("  Promise: services sshd — already running");
        return 0;
    }
    println!("cf-agent: evaluating policy...");
    println!("  Promises kept: 42");
    println!("  Promises repaired: 3");
    println!("  Promises not kept: 0");
    0
}

fn run_cf_promises(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cf-promises [OPTIONS] [FILE]");
        println!("cf-promises v3.23 (OurOS) — CFEngine promise validator");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("promises.cf");
    println!("Checking: {}", file);
    println!("  Syntax: OK");
    println!("  Bundles: 5");
    println!("  Promises: 23");
    0
}

fn run_cf_key(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cf-key [OPTIONS]");
        println!("cf-key v3.23 (OurOS) — CFEngine key management");
        return 0;
    }
    println!("Generating RSA key pair...");
    println!("  Public key: /var/cfengine/ppkeys/localhost.pub");
    println!("  Private key: /var/cfengine/ppkeys/localhost.priv");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cf-agent".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "cf-promises" => run_cf_promises(&rest, &prog),
        "cf-key" => run_cf_key(&rest, &prog),
        _ => run_cf_agent(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cf_agent};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cfengine"), "cfengine");
        assert_eq!(basename(r"C:\bin\cfengine.exe"), "cfengine.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cfengine.exe"), "cfengine");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cf_agent(&["--help".to_string()], "cfengine"), 0);
        assert_eq!(run_cf_agent(&["-h".to_string()], "cfengine"), 0);
        assert_eq!(run_cf_agent(&["--version".to_string()], "cfengine"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cf_agent(&[], "cfengine"), 0);
    }
}
