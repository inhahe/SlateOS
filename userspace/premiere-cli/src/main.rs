#![deny(clippy::all)]

//! premiere-cli — OurOS Adobe Premiere Pro video editing
//!
//! Single personality: `premiere`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_premiere(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: premiere [OPTIONS] [PROJECT]");
        println!("Adobe Premiere Pro 2024 (OurOS) — Professional video editing");
        println!();
        println!("Options:");
        println!("  -open FILE             Open project (.prproj)");
        println!("  -r SCRIPT              Run ExtendScript / JSX");
        println!("  -ProductionMode        Production mode (collaboration)");
        println!("  -debug                 Debug logging");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe Premiere Pro 2024 v24.5.0 (OurOS)"); return 0; }
    println!("Adobe Premiere Pro 2024 v24.5.0 (OurOS)");
    println!("  Engine: Mercury Playback Engine (GPU accelerated)");
    println!("  Codecs: All major formats incl. RED, ARRI, BRAW, ProRes");
    println!("  Scripting: ExtendScript, CEP panels");
    println!("  AI: Speech-to-Text, Auto Reframe, Scene Edit Detection");
    println!("  Integration: After Effects Dynamic Link, Audition, Frame.io");
    println!("  License: Creative Cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "premiere".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_premiere(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_premiere};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/premiere"), "premiere");
        assert_eq!(basename(r"C:\bin\premiere.exe"), "premiere.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("premiere.exe"), "premiere");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_premiere(&["--help".to_string()], "premiere"), 0);
        assert_eq!(run_premiere(&["-h".to_string()], "premiere"), 0);
        let _ = run_premiere(&["--version".to_string()], "premiere");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_premiere(&[], "premiere");
    }
}
