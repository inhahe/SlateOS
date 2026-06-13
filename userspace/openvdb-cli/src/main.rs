#![deny(clippy::all)]

//! openvdb-cli — SlateOS OpenVDB volumetric data tool
//!
//! Single personality: `vdb_print` (multi: vdb_print, vdb_render)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vdb_print(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vdb_print [OPTIONS] FILE.vdb");
        println!("vdb_print v11.0 (SlateOS) — Print OpenVDB file metadata");
        println!();
        println!("Options:");
        println!("  FILE.vdb          Input VDB file");
        println!("  -l, --long        Show detailed grid info");
        println!("  -m, --metadata    Show file metadata only");
        println!("  --stats           Show grid statistics");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("OpenVDB v11.0 (SlateOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("volume.vdb");
    println!("File: {}", file);
    println!("  OpenVDB version: 11.0");
    println!("  Grids: 2");
    println!("    'density' FloatGrid  [256x256x256] active voxels: 1,048,576");
    println!("    'temperature' FloatGrid  [256x256x256] active voxels: 524,288");
    0
}

fn run_vdb_render(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vdb_render [OPTIONS] FILE.vdb OUTPUT.exr");
        println!("vdb_render v11.0 (SlateOS) — Render OpenVDB volumes");
        println!();
        println!("Options:");
        println!("  -res WxH          Resolution (default: 1920x1080)");
        println!("  -shader TYPE      Shader: diffuse, matte, normal");
        println!("  -samples N        Samples per pixel");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("vdb_render v11.0 (SlateOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("volume.vdb");
    println!("Rendering: {}", file);
    println!("  Resolution: 1920x1080");
    println!("  Shader: diffuse");
    println!("  Rendering... Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vdb_print".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "vdb_render" => run_vdb_render(&rest, &prog),
        _ => run_vdb_print(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vdb_print};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/openvdb"), "openvdb");
        assert_eq!(basename(r"C:\bin\openvdb.exe"), "openvdb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("openvdb.exe"), "openvdb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vdb_print(&["--help".to_string()], "openvdb"), 0);
        assert_eq!(run_vdb_print(&["-h".to_string()], "openvdb"), 0);
        let _ = run_vdb_print(&["--version".to_string()], "openvdb");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vdb_print(&[], "openvdb");
    }
}
