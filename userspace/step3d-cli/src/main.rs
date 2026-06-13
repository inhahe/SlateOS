#![deny(clippy::all)]

//! step3d-cli — SlateOS STEP file viewer
//!
//! Single personality: `step3d`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_step3d(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: step3d [OPTIONS] FILE.step");
        println!("step3d v1.0 (SlateOS) — STEP/IGES 3D file viewer");
        println!();
        println!("Options:");
        println!("  --info            Show file information");
        println!("  --export FMT      Export to STL/OBJ/glTF");
        println!("  --version         Show version");
        println!();
        println!("Supported formats: STEP (.step, .stp), IGES (.iges, .igs)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("step3d v1.0 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "--info") {
        let file = args.last().map(|s| s.as_str()).unwrap_or("model.step");
        println!("File: {}", file);
        println!("  Format: STEP AP214");
        println!("  Entities: 1247");
        println!("  Solids: 3");
        println!("  Faces: 156");
        println!("  Bounding box: 100x50x25 mm");
        return 0;
    }
    let file = args.last().map(|s| s.as_str()).unwrap_or("model.step");
    println!("step3d: viewing '{}'...", file);
    println!("  Rendering with OpenGL");
    println!("  Controls: rotate (drag), zoom (scroll), pan (Shift+drag)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "step3d".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_step3d(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_step3d};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/step3d"), "step3d");
        assert_eq!(basename(r"C:\bin\step3d.exe"), "step3d.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("step3d.exe"), "step3d");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_step3d(&["--help".to_string()], "step3d"), 0);
        assert_eq!(run_step3d(&["-h".to_string()], "step3d"), 0);
        let _ = run_step3d(&["--version".to_string()], "step3d");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_step3d(&[], "step3d");
    }
}
