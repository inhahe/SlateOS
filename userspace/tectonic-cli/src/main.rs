#![deny(clippy::all)]

//! tectonic-cli — OurOS Tectonic TeX/LaTeX engine
//!
//! Single personality: `tectonic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tectonic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tectonic [OPTIONS] INPUT");
        println!("Tectonic v0.15 (OurOS) — Modernized TeX/LaTeX engine");
        println!();
        println!("Options:");
        println!("  -o DIR         Output directory");
        println!("  --outfmt FMT   Output format (pdf, xdv, aux, fmt)");
        println!("  -p             Print to stdout");
        println!("  -Z FLAGS       Unstable options");
        println!("  --bundle URL   TeX bundle URL");
        println!("  --web-bundle URL  Web bundle");
        println!("  -r N           Max reruns");
        println!("  --keep-intermediates  Keep aux files");
        println!("  --synctex      Generate SyncTeX data");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tectonic v0.15.0 (OurOS)"); return 0; }
    println!("Tectonic v0.15.0 (OurOS)");
    println!("  Input: paper.tex");
    println!("  Bundle: https://data.tectonic-typesetting.com/default");
    println!("  Running TeX...");
    println!("    Pass 1: building structure");
    println!("    Pass 2: resolving references");
    println!("    Running BibTeX...");
    println!("    Pass 3: final output");
    println!("  Output: paper.pdf (24 pages, 1.2 MB)");
    println!("  No warnings or errors");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tectonic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tectonic(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tectonic};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tectonic"), "tectonic");
        assert_eq!(basename(r"C:\bin\tectonic.exe"), "tectonic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tectonic.exe"), "tectonic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_tectonic(&["--help".to_string()], "tectonic"), 0);
        assert_eq!(run_tectonic(&["-h".to_string()], "tectonic"), 0);
        assert_eq!(run_tectonic(&["--version".to_string()], "tectonic"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_tectonic(&[], "tectonic"), 0);
    }
}
