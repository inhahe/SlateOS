#![deny(clippy::all)]

//! rex-cli — SlateOS Rex remote execution framework
//!
//! Single personality: `rex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rex(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rex [OPTIONS] <task>");
        println!("rex v1.14 (Slate OS) — Remote execution framework");
        println!();
        println!("Options:");
        println!("  -H HOST       Target host(s)");
        println!("  -u USER       SSH username");
        println!("  -p PORT       SSH port");
        println!("  -G GROUP      Server group");
        println!("  -E ENV        Environment");
        println!("  -T            List tasks");
        println!("  -f FILE       Rexfile path");
        println!("  --version     Show version");
        println!();
        println!("Automate deployments and server management.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rex v1.14 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-T") {
        println!("Tasks:");
        println!("  deploy         Deploy application");
        println!("  setup          Initial setup");
        println!("  service:start  Start services");
        println!("  service:stop   Stop services");
        println!("  update         System update");
        return 0;
    }
    if let Some(task) = args.iter().find(|a| !a.starts_with('-')) {
        println!("rex: running task '{}'", task);
        println!("  Hosts: 1");
        println!("  Status: OK");
    } else {
        println!("rex: no task specified. Use -T to list tasks.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rex(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rex"), "rex");
        assert_eq!(basename(r"C:\bin\rex.exe"), "rex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rex.exe"), "rex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rex(&["--help".to_string()], "rex"), 0);
        assert_eq!(run_rex(&["-h".to_string()], "rex"), 0);
        let _ = run_rex(&["--version".to_string()], "rex");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rex(&[], "rex");
    }
}
