#![deny(clippy::all)]

//! photoshop-cli — Slate OS Adobe Photoshop raster image editor
//!
//! Single personality: `photoshop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ps(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: photoshop [OPTIONS] [FILE]");
        println!("Adobe Photoshop 2024 (Slate OS) — Professional raster image editor");
        println!();
        println!("Options:");
        println!("  -r SCRIPT              Run ExtendScript / JSX");
        println!("  -batch ACTION FOLDER   Run action on folder");
        println!("  -open FILE             Open file");
        println!("  -saveas FORMAT FILE    Save as format (psd/png/jpg/tiff/webp)");
        println!("  -size WxH              Resize");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe Photoshop 2024 v25.7.0 (Slate OS)"); return 0; }
    println!("Adobe Photoshop 2024 v25.7.0 (Slate OS)");
    println!("  Engine: GPU acceleration (OpenGL, Metal)");
    println!("  Scripting: JavaScript (ExtendScript), CEP, UXP");
    println!("  Features: Generative Fill (Firefly AI), Neural Filters, Camera Raw");
    println!("  Color: 8/16/32-bit per channel, ProPhoto/sRGB/Adobe RGB");
    println!("  License: Creative Cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "photoshop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ps(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ps};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/photoshop"), "photoshop");
        assert_eq!(basename(r"C:\bin\photoshop.exe"), "photoshop.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("photoshop.exe"), "photoshop");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ps(&["--help".to_string()], "photoshop"), 0);
        assert_eq!(run_ps(&["-h".to_string()], "photoshop"), 0);
        let _ = run_ps(&["--version".to_string()], "photoshop");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ps(&[], "photoshop");
    }
}
