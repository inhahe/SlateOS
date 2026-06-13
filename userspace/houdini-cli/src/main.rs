#![deny(clippy::all)]

//! houdini-cli — SlateOS SideFX Houdini procedural 3D
//!
//! Single personality: `houdini`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_houdini(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: houdini [OPTIONS] [FILE]");
        println!("SideFX Houdini 20 (Slate OS) — Procedural 3D animation & VFX");
        println!();
        println!("Options:");
        println!("  -batch                Batch (no GUI) mode");
        println!("  -hbatch SCRIPT        Run hscript batch script");
        println!("  -hython SCRIPT        Run Python script with Houdini bindings");
        println!("  -frange N M           Frame range");
        println!("  -driver NODE          Output driver (Mantra, Karma, Redshift)");
        println!("  -verbose              Verbose output");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SideFX Houdini 20.0.625 (Slate OS)"); return 0; }
    println!("SideFX Houdini 20.0.625 (Slate OS)");
    println!("  Renderers: Mantra, Karma XPU, Redshift, RenderMan");
    println!("  Solvers: Pyro, FLIP, Vellum, Bullet, RBD");
    println!("  Scripting: HScript, Python, VEX");
    println!("  Networks: SOP, DOP, VOP, COP, LOP (Solaris/USD)");
    println!("  License: Houdini FX (network)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "houdini".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_houdini(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_houdini};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/houdini"), "houdini");
        assert_eq!(basename(r"C:\bin\houdini.exe"), "houdini.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("houdini.exe"), "houdini");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_houdini(&["--help".to_string()], "houdini"), 0);
        assert_eq!(run_houdini(&["-h".to_string()], "houdini"), 0);
        let _ = run_houdini(&["--version".to_string()], "houdini");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_houdini(&[], "houdini");
    }
}
