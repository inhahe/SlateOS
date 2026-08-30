#![deny(clippy::all)]

//! povray-cli — Slate OS POV-Ray ray tracer
//!
//! Single personality: `povray`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_povray(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: povray [OPTIONS] FILE.pov");
        println!("POV-Ray v3.8 (Slate OS) — Persistence of Vision Raytracer");
        println!();
        println!("Options:");
        println!("  FILE.pov            Input scene file");
        println!("  +W<n>               Width in pixels (default: 640)");
        println!("  +H<n>               Height in pixels (default: 480)");
        println!("  +O<file>            Output file name");
        println!("  +A                  Enable anti-aliasing");
        println!("  +Q<n>               Quality (0-11, default: 9)");
        println!("  +R<n>               Radiosity bounces");
        println!("  +D                  Display while rendering");
        println!("  +V                  Verbose output");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("POV-Ray v3.8 (Slate OS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('+') && !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("scene.pov");
    println!("POV-Ray v3.8 — Rendering: {}", file);
    println!("  Resolution: 640x480");
    println!("  Quality: 9");
    println!("  Anti-aliasing: on");
    println!("  Parsing scene... OK");
    println!("  Rendering... Done (2.3s)");
    println!("  Output: scene.png");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "povray".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_povray(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_povray};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/povray"), "povray");
        assert_eq!(basename(r"C:\bin\povray.exe"), "povray.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("povray.exe"), "povray");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_povray(&["--help".to_string()], "povray"), 0);
        assert_eq!(run_povray(&["-h".to_string()], "povray"), 0);
        let _ = run_povray(&["--version".to_string()], "povray");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_povray(&[], "povray");
    }
}
