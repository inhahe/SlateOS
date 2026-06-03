#![deny(clippy::all)]

//! otter-cli — OurOS Otter infrastructure automation
//!
//! Single personality: `otter`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_otter(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: otter [COMMAND] [OPTIONS]");
        println!("Otter v2024 (OurOS) — Infrastructure automation & config management");
        println!();
        println!("Commands:");
        println!("  server list|get    Manage servers");
        println!("  role list|assign   Manage roles");
        println!("  job start|get|list Manage jobs");
        println!("  plan create|get    Execution plans");
        println!("  drift check        Check configuration drift");
        println!("  remediate          Auto-remediate drift");
        println!();
        println!("Options:");
        println!("  --api URL          Otter API URL");
        println!("  --api-key KEY      API key");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Otter v2024.3 (OurOS)"); return 0; }
    println!("Otter v2024.3 (OurOS)");
    println!("  Servers: 45 managed");
    println!("  Roles: 12 defined");
    println!("  Drift: 3 servers out of compliance");
    println!("  Jobs: 8 running, 234 completed");
    println!("  Last scan: 15m ago");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "otter".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_otter(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_otter};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/otter"), "otter");
        assert_eq!(basename(r"C:\bin\otter.exe"), "otter.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("otter.exe"), "otter");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_otter(&["--help".to_string()], "otter"), 0);
        assert_eq!(run_otter(&["-h".to_string()], "otter"), 0);
        assert_eq!(run_otter(&["--version".to_string()], "otter"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_otter(&[], "otter"), 0);
    }
}
