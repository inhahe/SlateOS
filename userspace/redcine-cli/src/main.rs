#![deny(clippy::all)]

//! redcine-cli — OurOS RED REDCINE-X PRO raw workflow
//!
//! Single personality: `redcine`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_redcine(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: redcine [OPTIONS] [CLIP]");
        println!("RED REDCINE-X PRO 65 (OurOS) — Free RED RAW workflow & color tool");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .R3D file");
        println!("  --metadata             Show clip metadata");
        println!("  --rocket DEVICE        Use RED ROCKET-X / ROCKET (PCIe accelerator)");
        println!("  --export FILE          Export to ProRes/DPX/EXR/H.264");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("RED REDCINE-X PRO 65.0.0 (OurOS)"); return 0; }
    println!("RED REDCINE-X PRO 65.0.0 (OurOS)");
    println!("  Format: REDCODE RAW (.R3D) — all RED cameras supported");
    println!("  Color: IPP2 image processing pipeline (current standard)");
    println!("  Hardware: GPU (CUDA/OpenCL/Metal) and RED ROCKET cards");
    println!("  Export: ProRes, DNxHR, DPX, EXR, TIFF, H.264, H.265, MPEG-4");
    println!("  License: Free download from RED Digital Cinema");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "redcine".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_redcine(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_redcine};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/redcine"), "redcine");
        assert_eq!(basename(r"C:\bin\redcine.exe"), "redcine.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("redcine.exe"), "redcine");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_redcine(&["--help".to_string()], "redcine"), 0);
        assert_eq!(run_redcine(&["-h".to_string()], "redcine"), 0);
        assert_eq!(run_redcine(&["--version".to_string()], "redcine"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_redcine(&[], "redcine"), 0);
    }
}
