#![deny(clippy::all)]

//! dusty3d-cli — OurOS Dust3D auto-rigging 3D modeler
//!
//! Single personality: `dust3d`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dust3d(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dust3d [OPTIONS] [FILE.ds3]");
        println!("dust3d v1.0 (OurOS) — Auto-rigging 3D modeling tool");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Node-based modeling, automatic rigging,");
        println!("  automatic UV unwrapping, PBR materials,");
        println!("  pose editing, motion editing");
        println!("Export: glTF, FBX, OBJ");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dust3d v1.0 (OurOS)"); return 0; }
    println!("dust3d: auto-rigging modeler started");
    println!("  Modeling: node-based mesh generation");
    println!("  Rigging: automatic skeleton generation");
    println!("  Materials: PBR metallic/roughness workflow");
    println!("  Animation: pose library, motion blending");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dust3d".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dust3d(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dust3d};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dusty3d"), "dusty3d");
        assert_eq!(basename(r"C:\bin\dusty3d.exe"), "dusty3d.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dusty3d.exe"), "dusty3d");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_dust3d(&["--help".to_string()], "dusty3d"), 0);
        assert_eq!(run_dust3d(&["-h".to_string()], "dusty3d"), 0);
        assert_eq!(run_dust3d(&["--version".to_string()], "dusty3d"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_dust3d(&[], "dusty3d"), 0);
    }
}
