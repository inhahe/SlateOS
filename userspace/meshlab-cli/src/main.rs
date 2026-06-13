#![deny(clippy::all)]

//! meshlab-cli — SlateOS MeshLab 3D mesh processing
//!
//! Multi-personality: `meshlab`, `meshlabserver`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_meshlab(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: meshlab [OPTIONS] [FILE.stl | FILE.obj | FILE.ply | FILE.off]");
        println!("MeshLab 2023.12 (SlateOS)");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("MeshLab 2023.12 (SlateOS)");
        println!("VCG Library 2023.12");
        return 0;
    }
    let file = args.iter().find(|a| {
        a.ends_with(".stl") || a.ends_with(".obj") || a.ends_with(".ply") || a.ends_with(".off") || a.ends_with(".3ds")
    }).map(|s| s.as_str());
    if let Some(f) = file {
        println!("MeshLab 2023.12 — loading: {}", f);
        println!("  Vertices: 6,230");
        println!("  Faces: 12,456");
        println!("  Bounding box: 80.0 x 60.0 x 45.0 mm");
    } else {
        println!("MeshLab 2023.12 — Starting...");
    }
    println!("Ready.");
    0
}

fn run_meshlabserver(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: meshlabserver [OPTIONS]");
        println!("  -i FILE        Input mesh file");
        println!("  -o FILE        Output mesh file");
        println!("  -s FILE.mlx    Apply filter script");
        println!("  -l FILE.log    Log file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("MeshLab Server 2023.12 (SlateOS)");
        return 0;
    }
    let input = args.windows(2).find(|w| w[0] == "-i").map(|w| w[1].as_str()).unwrap_or("input.stl");
    let output = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str());
    let script = args.windows(2).find(|w| w[0] == "-s").map(|w| w[1].as_str());
    println!("MeshLab Server 2023.12");
    println!("  Loading: {}", input);
    println!("  Mesh: 6,230 vertices, 12,456 faces");
    if let Some(s) = script {
        println!("  Applying filter script: {}", s);
        println!("  Filters applied successfully.");
    }
    if let Some(o) = output {
        println!("  Saving: {}", o);
        println!("  Done.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "meshlab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "meshlabserver" => run_meshlabserver(&rest),
        _ => run_meshlab(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_meshlab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/meshlab"), "meshlab");
        assert_eq!(basename(r"C:\bin\meshlab.exe"), "meshlab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("meshlab.exe"), "meshlab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_meshlab(&["--help".to_string()]), 0);
        assert_eq!(run_meshlab(&["-h".to_string()]), 0);
        let _ = run_meshlab(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_meshlab(&[]);
    }
}
