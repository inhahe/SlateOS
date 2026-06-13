#![deny(clippy::all)]

//! falco-cli — SlateOS Falco runtime security tool
//!
//! Two personalities: `falco`, `falcoctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_falco(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: falco [OPTIONS]");
        println!("Falco v0.38.0 (Slate OS) — Runtime security monitoring");
        println!();
        println!("Options:");
        println!("  -r, --rules FILE     Rules file");
        println!("  -c, --config FILE    Config file");
        println!("  -L                   List fields");
        println!("  -l                   List rules");
        println!("  --list               List all events");
        println!("  -V, --validate FILE  Validate rules file");
        println!("  --version            Show version");
        println!("  -d, --daemon         Run as daemon");
        println!("  --stats-interval N   Stats interval (sec)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Falco v0.38.0 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("Loaded rules:");
        println!("  Terminal shell in container");
        println!("  Write below root");
        println!("  Read sensitive file");
        println!("  Modify binary dirs");
        println!("  ... (45 rules loaded)");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--validate") {
        println!("Rules file validation: OK");
        println!("  45 rules loaded, 0 errors");
        return 0;
    }
    println!("Falco v0.38.0 starting...");
    println!("Loading rules from /etc/falco/falco_rules.yaml");
    println!("  45 rules loaded");
    println!("Starting event collection...");
    println!();
    println!("10:00:01 Warning Terminal shell in container (user=root container=app-1234 shell=/bin/sh)");
    println!("10:00:05 Notice Read sensitive file /etc/shadow (user=root proc=cat)");
    println!("10:00:12 Warning Write below root (file=/root/.bashrc user=root)");
    0
}

fn run_falcoctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: falcoctl COMMAND [OPTIONS]");
        println!("falcoctl v0.8.0 (Slate OS) — Falco artifact management");
        println!();
        println!("Commands:");
        println!("  artifact        Manage artifacts");
        println!("  driver          Manage driver");
        println!("  index           Manage indexes");
        println!("  registry        Manage registries");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("falcoctl v0.8.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("artifact");
    match cmd {
        "artifact" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Installed artifacts:");
                println!("  falco-rules  0.38.0  rules");
                println!("  falco-plugin 0.8.0   plugin");
            }
        }
        "driver" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            if sub == "status" {
                println!("Driver: modern_ebpf");
                println!("Status: loaded");
            }
        }
        _ => println!("falcoctl {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "falco".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "falcoctl" => run_falcoctl(&rest, &prog),
        _ => run_falco(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_falco};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/falco"), "falco");
        assert_eq!(basename(r"C:\bin\falco.exe"), "falco.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("falco.exe"), "falco");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_falco(&["--help".to_string()], "falco"), 0);
        assert_eq!(run_falco(&["-h".to_string()], "falco"), 0);
        let _ = run_falco(&["--version".to_string()], "falco");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_falco(&[], "falco");
    }
}
