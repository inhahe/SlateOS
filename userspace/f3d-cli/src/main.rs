#![deny(clippy::all)]

//! f3d-cli — Slate OS F3D 3D model viewer
//!
//! Single personality: `f3d`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_f3d(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: f3d [OPTIONS] FILE...");
        println!("F3D v2.5 (Slate OS) — Fast and minimalist 3D viewer");
        println!();
        println!("Options:");
        println!("  FILE              3D model file(s) to view");
        println!("  --output FILE     Save screenshot");
        println!("  --resolution WxH  Window resolution");
        println!("  --bg COLOR        Background color (hex)");
        println!("  --hdri FILE       Environment map for PBR");
        println!("  --raytracing      Enable ray tracing");
        println!("  --axis            Show axis widget");
        println!("  --grid            Show grid");
        println!("  --edges           Show edges");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("F3D v2.5 (Slate OS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("model.glb");
    let raytracing = args.iter().any(|a| a == "--raytracing");
    println!("F3D v2.5 — Viewing: {}", file);
    println!("  Resolution: 1280x720");
    if raytracing {
        println!("  Renderer: ray tracing (OSPRay)");
    } else {
        println!("  Renderer: rasterization (OpenGL)");
    }
    println!("  Press 'h' for help, 'q' to quit");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "f3d".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_f3d(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_f3d};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/f3d"), "f3d");
        assert_eq!(basename(r"C:\bin\f3d.exe"), "f3d.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("f3d.exe"), "f3d");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_f3d(&["--help".to_string()], "f3d"), 0);
        assert_eq!(run_f3d(&["-h".to_string()], "f3d"), 0);
        let _ = run_f3d(&["--version".to_string()], "f3d");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_f3d(&[], "f3d");
    }
}
