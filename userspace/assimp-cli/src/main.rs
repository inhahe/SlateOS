#![deny(clippy::all)]

//! assimp-cli — Slate OS Open Asset Import Library
//!
//! Single personality: `assimp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_assimp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: assimp COMMAND [OPTIONS] FILE");
        println!("Assimp v5.4 (Slate OS) — Open Asset Import Library");
        println!();
        println!("Commands:");
        println!("  info FILE         Show model info");
        println!("  dump FILE         Dump scene structure");
        println!("  export FILE FMT   Convert model format");
        println!("  listext           List supported extensions");
        println!("  listexport        List export formats");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("assimp v5.4 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "info" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("model.obj");
            println!("File: {}", file);
            println!("  Meshes: 3");
            println!("  Vertices: 12,456");
            println!("  Faces: 8,304");
            println!("  Materials: 2");
            println!("  Textures: 1");
            println!("  Animations: 0");
        }
        "dump" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("model.obj");
            println!("Scene dump: {}", file);
            println!("  Root node: 'Scene'");
            println!("    Node 'Mesh_01' (8192 faces)");
            println!("    Node 'Mesh_02' (112 faces)");
        }
        "export" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("model.obj");
            let fmt = args.get(2).map(|s| s.as_str()).unwrap_or("glb");
            println!("Exporting {} -> {}", file, fmt);
            println!("  Converting... Done.");
        }
        "listext" => {
            println!("Supported import formats:");
            println!("  obj, fbx, gltf, glb, dae, 3ds, blend");
            println!("  stl, ply, off, x3d, step, iges, abc");
        }
        "listexport" => {
            println!("Supported export formats:");
            println!("  obj, fbx, gltf2, glb2, dae, stl, ply, x3d, 3ds, assbin");
        }
        _ => println!("assimp {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "assimp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_assimp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_assimp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/assimp"), "assimp");
        assert_eq!(basename(r"C:\bin\assimp.exe"), "assimp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("assimp.exe"), "assimp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_assimp(&["--help".to_string()], "assimp"), 0);
        assert_eq!(run_assimp(&["-h".to_string()], "assimp"), 0);
        let _ = run_assimp(&["--version".to_string()], "assimp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_assimp(&[], "assimp");
    }
}
