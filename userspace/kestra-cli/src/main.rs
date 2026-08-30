#![deny(clippy::all)]

//! kestra-cli — Slate OS Kestra orchestration platform
//!
//! Single personality: `kestra`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kestra(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kestra [COMMAND] [OPTIONS]");
        println!("Kestra v0.18 (Slate OS) — Declarative data orchestration");
        println!();
        println!("Commands:");
        println!("  server standalone  Start standalone server");
        println!("  server worker      Start worker");
        println!("  flow list|validate List or validate flows");
        println!("  flow export|import Export/import flows");
        println!("  namespace list     List namespaces");
        println!("  execution list     List executions");
        println!("  template list      List templates");
        println!("  plugin list|install Manage plugins");
        println!();
        println!("Options:");
        println!("  --server URL       Kestra server URL");
        println!("  --api-token TOKEN  API token");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kestra v0.18.3 (Slate OS)"); return 0; }
    println!("Kestra v0.18.3 (Slate OS)");
    println!("  Server: http://0.0.0.0:8080");
    println!("  Namespaces: 5");
    println!("  Flows: 67 (45 active, 22 disabled)");
    println!("  Executions: 1,234 (last 24h)");
    println!("  Workers: 4");
    println!("  Plugins: 89 loaded");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kestra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kestra(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kestra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kestra"), "kestra");
        assert_eq!(basename(r"C:\bin\kestra.exe"), "kestra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kestra.exe"), "kestra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kestra(&["--help".to_string()], "kestra"), 0);
        assert_eq!(run_kestra(&["-h".to_string()], "kestra"), 0);
        let _ = run_kestra(&["--version".to_string()], "kestra");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kestra(&[], "kestra");
    }
}
