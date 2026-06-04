#![deny(clippy::all)]

//! procreate-cli — OurOS Procreate digital illustration
//!
//! Single personality: `procreate`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_procreate(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: procreate [OPTIONS] [FILE]");
        println!("Procreate (OurOS) — Award-winning illustration & painting");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .procreate file");
        println!("  --new CANVAS           New from canvas preset");
        println!("  --export FORMAT FILE   Export (psd/png/jpg/tiff/pdf)");
        println!("  --timelapse FILE       Export timelapse video");
        println!("  --brushset FILE        Import brush set");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Procreate 5.3.10 (OurOS)"); return 0; }
    println!("Procreate 5.3.10 (OurOS)");
    println!("  Engine: Valkyrie graphics engine (Metal-based)");
    println!("  Brushes: 200+ default + StreamLine prediction");
    println!("  Color: 8/16-bit, P3 wide gamut, ColorDrop fill");
    println!("  Animation: Animation Assist, onion skins, export to MP4/GIF");
    println!("  License: one-time purchase (no subscription)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "procreate".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_procreate(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_procreate};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/procreate"), "procreate");
        assert_eq!(basename(r"C:\bin\procreate.exe"), "procreate.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("procreate.exe"), "procreate");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_procreate(&["--help".to_string()], "procreate"), 0);
        assert_eq!(run_procreate(&["-h".to_string()], "procreate"), 0);
        let _ = run_procreate(&["--version".to_string()], "procreate");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_procreate(&[], "procreate");
    }
}
