#![deny(clippy::all)]

//! silhouette-cli — OurOS Boris FX Silhouette rotoscoping & paint
//!
//! Single personality: `silhouette`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_silhouette(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: silhouette [OPTIONS] [PROJECT]");
        println!("Boris FX Silhouette 2024 (OurOS) — Rotoscoping, paint, VFX node compositor");
        println!();
        println!("Options:");
        println!("  --batch                Headless batch mode");
        println!("  --script FILE          Run Python script");
        println!("  --node NODE            Process specific node");
        println!("  --frame N              Process single frame");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Boris FX Silhouette 2024.0.0 (OurOS)"); return 0; }
    println!("Boris FX Silhouette 2024.0.0 (OurOS)");
    println!("  Nodes: Roto, Paint, MultiFrame, ZMatte, Tracker, Stereo");
    println!("  Roto: Bezier, B-spline, X-spline tools");
    println!("  Paint: Auto Paint clone & repair");
    println!("  GPU: OpenCL accelerated paint & playback");
    println!("  Scripting: Python");
    println!("  Workflows: Used on Mandalorian, Avengers, many features");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "silhouette".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_silhouette(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_silhouette};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/silhouette"), "silhouette");
        assert_eq!(basename(r"C:\bin\silhouette.exe"), "silhouette.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("silhouette.exe"), "silhouette");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_silhouette(&["--help".to_string()], "silhouette"), 0);
        assert_eq!(run_silhouette(&["-h".to_string()], "silhouette"), 0);
        assert_eq!(run_silhouette(&["--version".to_string()], "silhouette"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_silhouette(&[], "silhouette"), 0);
    }
}
