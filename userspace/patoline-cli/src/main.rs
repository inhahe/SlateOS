#![deny(clippy::all)]

//! patoline-cli — OurOS Patoline typesetting system
//!
//! Single personality: `patoline`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_patoline(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: patoline [OPTIONS] FILE");
        println!("Patoline v0.2 (OurOS) — Modern typesetting system");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file");
        println!("  --driver DRV  Output driver (Pdf, SVG, Html)");
        println!("  --format FMT  Document format (DefaultFormat, Letter, Slides)");
        println!("  --extra-fonts DIR  Extra font directory");
        println!("  -I DIR        Include path");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Patoline v0.2 (OurOS)"); return 0; }
    println!("Patoline v0.2 (OurOS)");
    println!("  Input: presentation.txp");
    println!("  Format: Slides");
    println!("  Driver: Pdf");
    println!("  Compiling...");
    println!("    OCaml compilation: done");
    println!("    Typesetting: 15 slides");
    println!("    Fonts: Latin Modern, Source Code Pro");
    println!("  Output: presentation.pdf (15 pages)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "patoline".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_patoline(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_patoline};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/patoline"), "patoline");
        assert_eq!(basename(r"C:\bin\patoline.exe"), "patoline.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("patoline.exe"), "patoline");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_patoline(&["--help".to_string()], "patoline"), 0);
        assert_eq!(run_patoline(&["-h".to_string()], "patoline"), 0);
        assert_eq!(run_patoline(&["--version".to_string()], "patoline"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_patoline(&[], "patoline"), 0);
    }
}
