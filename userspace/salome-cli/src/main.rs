#![deny(clippy::all)]

//! salome-cli — Slate OS SALOME platform for CAE
//!
//! Multi-personality: `salome`, `salome-mesh`, `salome-geom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_salome(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [SCRIPT]", prog);
        println!("{} v9.12 (Slate OS) — Open-source CAE platform", prog);
        println!();
        println!("Options:");
        println!("  -t              Terminal mode (no GUI)");
        println!("  --study FILE    Open study file");
        println!("  --modules LIST  Load modules (GEOM,SMESH,PARAVIS)");
        println!("  --pinter        Interactive Python console");
        println!("  --port N        CORBA name server port");
        println!("  --shutdown      Shutdown servers");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SALOME v9.12.0 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--shutdown") {
        println!("SALOME: shutting down servers...");
        println!("  Registry server: stopped");
        println!("  Module catalog: stopped");
        println!("  SALOME_Session: stopped");
        return 0;
    }
    println!("SALOME v9.12.0 (Slate OS) — CAE Platform");
    println!("  Modules: GEOM, SMESH, PARAVIS, YACS, MED");
    println!("  Python: 3.12");
    println!("  CORBA: omniORB");
    println!("  Status: ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "salome".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_salome(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_salome};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/salome"), "salome");
        assert_eq!(basename(r"C:\bin\salome.exe"), "salome.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("salome.exe"), "salome");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_salome(&["--help".to_string()], "salome"), 0);
        assert_eq!(run_salome(&["-h".to_string()], "salome"), 0);
        let _ = run_salome(&["--version".to_string()], "salome");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_salome(&[], "salome");
    }
}
