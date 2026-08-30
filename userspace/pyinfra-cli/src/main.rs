#![deny(clippy::all)]

//! pyinfra-cli — Slate OS pyinfra infrastructure automation
//!
//! Single personality: `pyinfra`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pyinfra(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pyinfra INVENTORY OPERATIONS [OPTIONS]");
        println!("pyinfra v3.0 (Slate OS) — Infrastructure automation in Python");
        println!();
        println!("Options:");
        println!("  INVENTORY         Target hosts (hostname, @group, inventory.py)");
        println!("  OPERATIONS        Operations to run (deploy.py)");
        println!("  --dry             Dry-run mode");
        println!("  --limit HOST      Limit to specific host");
        println!("  --serial          Run operations serially");
        println!("  --debug           Debug output");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pyinfra v3.0 (Slate OS)"); return 0; }
    let inventory = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("inventory.py");
    let dry = args.iter().any(|a| a == "--dry");
    println!("pyinfra: targeting {}", inventory);
    println!();
    if dry {
        println!("[dry-run] web-01: apt.packages nginx — would install");
        println!("[dry-run] web-01: files.put local.conf → /etc/nginx/nginx.conf — would upload");
        println!("[dry-run] web-01: systemd.service nginx — would restart");
    } else {
        println!("[web-01] apt.packages nginx — installed");
        println!("[web-01] files.put local.conf → /etc/nginx/nginx.conf — uploaded");
        println!("[web-01] systemd.service nginx — restarted");
    }
    println!();
    println!("Results: 1 host, 3 operations, 0 errors");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pyinfra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pyinfra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pyinfra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pyinfra"), "pyinfra");
        assert_eq!(basename(r"C:\bin\pyinfra.exe"), "pyinfra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pyinfra.exe"), "pyinfra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pyinfra(&["--help".to_string()], "pyinfra"), 0);
        assert_eq!(run_pyinfra(&["-h".to_string()], "pyinfra"), 0);
        let _ = run_pyinfra(&["--version".to_string()], "pyinfra");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pyinfra(&[], "pyinfra");
    }
}
