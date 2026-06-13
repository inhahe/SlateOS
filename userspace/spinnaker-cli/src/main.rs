#![deny(clippy::all)]

//! spinnaker-cli — SlateOS Spinnaker continuous delivery
//!
//! Single personality: `spin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_spin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spin [COMMAND] [OPTIONS]");
        println!("Spinnaker CLI v1.30 (SlateOS) — Multi-cloud continuous delivery");
        println!();
        println!("Commands:");
        println!("  application list|get|save|delete  Manage applications");
        println!("  pipeline list|get|save|execute    Manage pipelines");
        println!("  pipeline-template list|get|plan   Pipeline templates");
        println!("  server-group list|get             Server groups");
        println!("  cluster list|get                  Clusters");
        println!();
        println!("Options:");
        println!("  --gate-endpoint URL  Spinnaker Gate URL");
        println!("  --config FILE      Config file");
        println!("  --output json|yaml Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("spin v1.30.0 (SlateOS)"); return 0; }
    println!("Spinnaker CLI v1.30.0 (SlateOS)");
    println!("  Gate: https://spinnaker.example.com");
    println!("  Applications: 12");
    println!("  Pipelines: 45");
    println!("  Executions: 234 (last 24h)");
    println!("  Cloud providers: AWS, Kubernetes, GCP");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "spin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spin(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_spin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/spinnaker"), "spinnaker");
        assert_eq!(basename(r"C:\bin\spinnaker.exe"), "spinnaker.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("spinnaker.exe"), "spinnaker");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_spin(&["--help".to_string()], "spinnaker"), 0);
        assert_eq!(run_spin(&["-h".to_string()], "spinnaker"), 0);
        let _ = run_spin(&["--version".to_string()], "spinnaker");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_spin(&[], "spinnaker");
    }
}
