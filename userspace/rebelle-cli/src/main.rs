#![deny(clippy::all)]

//! rebelle-cli — SlateOS Escape Motions Rebelle realistic painting
//!
//! Single personality: `rebelle`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rebelle(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rebelle [OPTIONS] [FILE]");
        println!("Escape Motions Rebelle 7 Pro (Slate OS) — Realistic watercolor & wet/dry media");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .reb project");
        println!("  --export FORMAT FILE   Export (png/jpg/tiff/psd)");
        println!("  --canvas TEXTURE       Set canvas texture");
        println!("  --tilt N               Tilt canvas N degrees");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rebelle 7 Pro v7.2.5 (Slate OS)"); return 0; }
    println!("Rebelle 7 Pro v7.2.5 (Slate OS)");
    println!("  Media: Watercolor, oil, acrylic, pastels, ink, pencils");
    println!("  Engine: Real Watercolor (wet diffusion), Real Oil tech");
    println!("  Layers: pixel, vector, reference, watercolor wet");
    println!("  Stencils & rulers: Symmetry, perspective, French curve");
    println!("  License: perpetual + free updates within major version");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rebelle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rebelle(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rebelle};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rebelle"), "rebelle");
        assert_eq!(basename(r"C:\bin\rebelle.exe"), "rebelle.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rebelle.exe"), "rebelle");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rebelle(&["--help".to_string()], "rebelle"), 0);
        assert_eq!(run_rebelle(&["-h".to_string()], "rebelle"), 0);
        let _ = run_rebelle(&["--version".to_string()], "rebelle");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rebelle(&[], "rebelle");
    }
}
