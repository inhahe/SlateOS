#![deny(clippy::all)]

//! illustrator-cli — OurOS Adobe Illustrator vector graphics
//!
//! Single personality: `illustrator`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ai(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: illustrator [OPTIONS] [FILE]");
        println!("Adobe Illustrator 2024 (OurOS) — Vector graphics design");
        println!();
        println!("Options:");
        println!("  -r SCRIPT              Run ExtendScript / JSX");
        println!("  -open FILE             Open file (.ai/.svg/.pdf/.eps)");
        println!("  -saveas FORMAT FILE    Save as (ai/svg/pdf/eps/png/jpg)");
        println!("  -newdoc PRESET         New document from preset");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe Illustrator 2024 v28.4.1 (OurOS)"); return 0; }
    println!("Adobe Illustrator 2024 v28.4.1 (OurOS)");
    println!("  Engine: GPU acceleration");
    println!("  Scripting: ExtendScript, CEP, UXP");
    println!("  Features: Generative Recolor, Text to Vector, 3D & Materials");
    println!("  Path operations: Boolean ops, Live Paint, Pattern Maker");
    println!("  License: Creative Cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "illustrator".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ai(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ai};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/illustrator"), "illustrator");
        assert_eq!(basename(r"C:\bin\illustrator.exe"), "illustrator.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("illustrator.exe"), "illustrator");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ai(&["--help".to_string()], "illustrator"), 0);
        assert_eq!(run_ai(&["-h".to_string()], "illustrator"), 0);
        let _ = run_ai(&["--version".to_string()], "illustrator");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ai(&[], "illustrator");
    }
}
