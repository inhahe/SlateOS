#![deny(clippy::all)]

//! panda3d-cli — SlateOS Panda3D game engine
//!
//! Single personality: `panda3d`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_panda3d(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: panda3d [COMMAND] [OPTIONS]");
        println!("Panda3D v1.10 (SlateOS) — Open-source 3D engine (Python/C++)");
        println!();
        println!("Commands:");
        println!("  run FILE           Run a Panda3D script");
        println!("  egg2bam IN OUT     Convert .egg to .bam (binary)");
        println!("  bam2egg IN OUT     Convert .bam to .egg (text)");
        println!("  pview MESH         Preview a model");
        println!("  pstats             Performance statistics viewer");
        println!("  build_apps         Build distributable apps");
        println!();
        println!("Options:");
        println!("  --window-type TYPE Window type (onscreen/offscreen/none)");
        println!("  --renderer API     Render API (gl/gles/dx9)");
        println!("  --threading-model  Threading model");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Panda3D v1.10.14 (SlateOS)"); return 0; }
    println!("Panda3D v1.10.14 (SlateOS)");
    println!("  Language bindings: Python, C++");
    println!("  Renderers: OpenGL, OpenGL ES, DirectX 9");
    println!("  Audio: OpenAL, FMOD");
    println!("  Physics: Bullet, ODE, PhysX");
    println!("  Networking: built-in TCP/UDP");
    println!("  Model formats: .egg, .bam, .gltf, .fbx, .obj");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "panda3d".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_panda3d(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_panda3d};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/panda3d"), "panda3d");
        assert_eq!(basename(r"C:\bin\panda3d.exe"), "panda3d.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("panda3d.exe"), "panda3d");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_panda3d(&["--help".to_string()], "panda3d"), 0);
        assert_eq!(run_panda3d(&["-h".to_string()], "panda3d"), 0);
        let _ = run_panda3d(&["--version".to_string()], "panda3d");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_panda3d(&[], "panda3d");
    }
}
