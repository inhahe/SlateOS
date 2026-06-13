#![deny(clippy::all)]

//! cinema4d-cli — Slate OS Maxon Cinema 4D
//!
//! Single personality: `cinema4d`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_c4d(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cinema4d [OPTIONS] [FILE]");
        println!("Maxon Cinema 4D 2024 (Slate OS) — 3D modeling, animation, motion graphics");
        println!();
        println!("Options:");
        println!("  -nogui                Headless mode");
        println!("  -render SCENE         Render scene");
        println!("  -frame N M S          Frame range (start end step)");
        println!("  -threads N            Render threads");
        println!("  -oimage FILE          Output image path");
        println!("  -oformat FORMAT       Output format (PNG/EXR/TIFF)");
        println!("  -license_server URL   License server URL");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Maxon Cinema 4D 2024.4.0 (Slate OS)"); return 0; }
    println!("Maxon Cinema 4D 2024.4.0 (Slate OS)");
    println!("  Renderers: Redshift (default), Standard, Physical, Arnold, V-Ray");
    println!("  Modules: MoGraph, Sculpt, Hair, Particles, Cloth");
    println!("  Scripting: Python, C.O.F.F.E.E.");
    println!("  License: Maxon One subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cinema4d".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_c4d(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_c4d};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cinema4d"), "cinema4d");
        assert_eq!(basename(r"C:\bin\cinema4d.exe"), "cinema4d.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cinema4d.exe"), "cinema4d");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_c4d(&["--help".to_string()], "cinema4d"), 0);
        assert_eq!(run_c4d(&["-h".to_string()], "cinema4d"), 0);
        let _ = run_c4d(&["--version".to_string()], "cinema4d");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_c4d(&[], "cinema4d");
    }
}
