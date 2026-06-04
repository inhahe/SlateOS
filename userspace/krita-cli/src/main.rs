#![deny(clippy::all)]

//! krita-cli — OurOS Krita digital painting (open source)
//!
//! Single personality: `krita`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_krita(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: krita [OPTIONS] [FILE]");
        println!("Krita 5.2 (OurOS) — Open-source digital painting (KDE Frameworks)");
        println!();
        println!("Options:");
        println!("  --canvasonly           Start in canvas-only mode");
        println!("  --new-image PARAMS     New image (W,H,format,name)");
        println!("  --workspace NAME       Use named workspace");
        println!("  --export FORMAT FILE   Export to format");
        println!("  --export-sequence      Export animation sequence");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Krita 5.2.5 (OurOS)"); return 0; }
    println!("Krita 5.2.5 (OurOS)");
    println!("  Brush engines: 9 (Pixel, Color Smudge, Hatching, Bristle, ...)");
    println!("  Color: 8/16/32-bit, OpenColorIO, ICC profiles");
    println!("  Features: Animation, Vector layers, Storyboard, Comics");
    println!("  Scripting: Python (PyKrita)");
    println!("  License: GNU GPLv3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "krita".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_krita(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_krita};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/krita"), "krita");
        assert_eq!(basename(r"C:\bin\krita.exe"), "krita.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("krita.exe"), "krita");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_krita(&["--help".to_string()], "krita"), 0);
        assert_eq!(run_krita(&["-h".to_string()], "krita"), 0);
        let _ = run_krita(&["--version".to_string()], "krita");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_krita(&[], "krita");
    }
}
