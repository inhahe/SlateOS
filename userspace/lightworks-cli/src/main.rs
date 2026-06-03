#![deny(clippy::all)]

//! lightworks-cli — OurOS Lightworks professional NLE
//!
//! Single personality: `lightworks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightworks [OPTIONS] [PROJECT]");
        println!("Lightworks 2024 (OurOS) — Award-winning NLE (Pulp Fiction, LOTR, etc.)");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .lwks project");
        println!("  --headless             Headless render");
        println!("  --license-server URL   License server");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lightworks 2024.1 (OurOS)"); return 0; }
    println!("Lightworks 2024.1 (OurOS)");
    println!("  Editions: Free, Create, Pro");
    println!("  Used by: Hollywood (Pulp Fiction, Departed, LOTR, Wolf of Wall Street)");
    println!("  Hardware: Optional Console controller, color panel");
    println!("  Codecs: All FFmpeg, native AVCHD/XDCAM/RED/ProRes");
    println!("  Audio: 5.1, 7.1, Dolby Atmos");
    println!("  License: subscription (was free with limits, now Pro paid)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lightworks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lightworks"), "lightworks");
        assert_eq!(basename(r"C:\bin\lightworks.exe"), "lightworks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lightworks.exe"), "lightworks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lw(&["--help".to_string()], "lightworks"), 0);
        assert_eq!(run_lw(&["-h".to_string()], "lightworks"), 0);
        assert_eq!(run_lw(&["--version".to_string()], "lightworks"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lw(&[], "lightworks"), 0);
    }
}
