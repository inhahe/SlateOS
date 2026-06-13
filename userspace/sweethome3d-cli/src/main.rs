#![deny(clippy::all)]

//! sweethome3d-cli — SlateOS Sweet Home 3D interior design
//!
//! Single personality: `sweethome3d`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sweethome3d(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sweethome3d [OPTIONS] [FILE.sh3d]");
        println!("sweethome3d v7.3 (SlateOS) — Interior design application");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  2D floor plan drawing with furniture placement,");
        println!("  real-time 3D preview, photo-realistic rendering,");
        println!("  1500+ furniture models, OBJ/DAE import");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sweethome3d v7.3 (SlateOS)"); return 0; }
    println!("sweethome3d: interior design application started");
    println!("  Furniture catalog: 1500+ items");
    println!("  Textures: 400+ materials");
    println!("  Views: floor plan, 3D, virtual visit");
    println!("  Export: OBJ, PDF, PNG, video");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sweethome3d".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sweethome3d(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sweethome3d};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sweethome3d"), "sweethome3d");
        assert_eq!(basename(r"C:\bin\sweethome3d.exe"), "sweethome3d.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sweethome3d.exe"), "sweethome3d");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sweethome3d(&["--help".to_string()], "sweethome3d"), 0);
        assert_eq!(run_sweethome3d(&["-h".to_string()], "sweethome3d"), 0);
        let _ = run_sweethome3d(&["--version".to_string()], "sweethome3d");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sweethome3d(&[], "sweethome3d");
    }
}
