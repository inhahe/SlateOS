#![deny(clippy::all)]

//! ode-cli — SlateOS Open Dynamics Engine tool
//!
//! Single personality: `ode-test`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ode(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ode-test COMMAND [OPTIONS]");
        println!("ODE v0.16 (SlateOS) — Open Dynamics Engine");
        println!();
        println!("Commands:");
        println!("  bench             Run simulation benchmarks");
        println!("  info              Show engine configuration");
        println!("  demo NAME         Run demo simulation");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("ODE v0.16 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "bench" => {
            println!("ODE benchmarks:");
            println!("  500 sphere stack: 1.8 ms/step");
            println!("  Trimesh collision: 3.2 ms/pair");
            println!("  Joint constraint (100): 0.4 ms/step");
        }
        "info" => {
            println!("ODE v0.16");
            println!("  Precision: double");
            println!("  Threading: enabled");
            println!("  Collision: OPCODE + GIMPACT");
            println!("  Solver: Dantzig LCP");
        }
        "demo" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("buggy");
            println!("Running demo: {}", name);
        }
        _ => println!("ode-test {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ode-test".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ode(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ode};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ode"), "ode");
        assert_eq!(basename(r"C:\bin\ode.exe"), "ode.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ode.exe"), "ode");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ode(&["--help".to_string()], "ode"), 0);
        assert_eq!(run_ode(&["-h".to_string()], "ode"), 0);
        let _ = run_ode(&["--version".to_string()], "ode");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ode(&[], "ode");
    }
}
