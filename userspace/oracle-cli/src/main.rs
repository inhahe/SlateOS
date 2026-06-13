#![deny(clippy::all)]

//! oracle-cli — SlateOS Oracle Database (SQL*Plus + sqlcl)
//!
//! Single personality: `oracle`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_oracle(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: oracle [OPTIONS] [USER/PASS@TNS]");
        println!("Oracle Database 23ai (Slate OS) — SQL*Plus / SQLcl client");
        println!();
        println!("Options:");
        println!("  -S USER/PASS@TNS       Silent login");
        println!("  @SCRIPT                Run SQL script");
        println!("  --sqlcl                Use modern SQLcl (Java-based)");
        println!("  --sqldeveloper         Launch SQL Developer (GUI)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Oracle Database 23ai Enterprise Edition Release 23.4.0.24.05 (Slate OS)"); return 0; }
    println!("Oracle Database 23ai Enterprise Edition Release 23.4.0.24.05 (Slate OS)");
    println!("  Editions: Free, Standard, Enterprise, Express, Cloud");
    println!("  23ai: AI Vector Search, JSON Relational Duality, JavaScript stored procs");
    println!("  Engine: multi-tenant CDB/PDB architecture, Real Application Clusters (RAC)");
    println!("  Languages: PL/SQL, SQL, Java in DB, JavaScript (MLE), Python (in 23ai)");
    println!("  Tools: SQL*Plus, SQLcl, SQL Developer, Enterprise Manager, RMAN");
    println!("  Replication: Data Guard (physical/logical standby), GoldenGate");
    println!("  License: Free Edition (no support); Enterprise (very expensive per-core)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "oracle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_oracle(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_oracle};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/oracle"), "oracle");
        assert_eq!(basename(r"C:\bin\oracle.exe"), "oracle.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("oracle.exe"), "oracle");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_oracle(&["--help".to_string()], "oracle"), 0);
        assert_eq!(run_oracle(&["-h".to_string()], "oracle"), 0);
        let _ = run_oracle(&["--version".to_string()], "oracle");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_oracle(&[], "oracle");
    }
}
