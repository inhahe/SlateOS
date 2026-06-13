#![deny(clippy::all)]

//! rhino-cli — SlateOS Robert McNeel Rhinoceros 3D NURBS modeler
//!
//! Single personality: `rhino`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rhino(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rhino [OPTIONS] [FILE]");
        println!("McNeel Rhinoceros 8 (Slate OS) — NURBS 3D modeler with Grasshopper");
        println!();
        println!("Options:");
        println!("  /runscript CMD         Execute Rhino command-line script");
        println!("  /nosplash              Skip splash screen");
        println!("  --grasshopper FILE     Run Grasshopper definition (.gh)");
        println!("  --python FILE          Run RhinoPython script");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("McNeel Rhinoceros 8.10 (Slate OS)"); return 0; }
    println!("McNeel Rhinoceros 8.10 (Slate OS)");
    println!("  Modeling: NURBS surfaces/curves, SubD, mesh, point clouds");
    println!("  Format: .3dm native + STEP/IGES/STL/OBJ/DWG/DXF/FBX (40+ formats)");
    println!("  Grasshopper: visual programming for parametric/generative design");
    println!("  Scripting: RhinoPython, RhinoScript (VB), RhinoCommon (.NET), C++ SDK");
    println!("  Plug-ins: VRay, Enscape, RhinoCAM, Karamba, Kangaroo, Ladybug");
    println!("  Render: Cycles (built-in), Bongo (animation)");
    println!("  License: perpetual (one-time purchase) — unusual in industry");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rhino".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rhino(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rhino};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rhino"), "rhino");
        assert_eq!(basename(r"C:\bin\rhino.exe"), "rhino.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rhino.exe"), "rhino");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rhino(&["--help".to_string()], "rhino"), 0);
        assert_eq!(run_rhino(&["-h".to_string()], "rhino"), 0);
        let _ = run_rhino(&["--version".to_string()], "rhino");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rhino(&[], "rhino");
    }
}
