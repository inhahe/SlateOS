#![deny(clippy::all)]

//! studioone-cli — SlateOS PreSonus Studio One DAW
//!
//! Single personality: `studioone`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_s1(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: studioone [OPTIONS] [SONG]");
        println!("PreSonus Studio One 6 Professional (SlateOS) — Single-window DAW");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .song or .project");
        println!("  --export FORMAT FILE   Export mix");
        println!("  --tempo BPM            Set tempo");
        println!("  --show                 Open Show page (live performance)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PreSonus Studio One 6.6.2 Professional (SlateOS)"); return 0; }
    println!("PreSonus Studio One 6.6.2 Professional (SlateOS)");
    println!("  Editions: Prime (free), Artist, Professional");
    println!("  Pages: Start, Song, Project (mastering), Show (live)");
    println!("  Plug-in formats: VST2, VST3, AU, ARA (Melodyne integrated)");
    println!("  Features: Drag-and-drop workflow, Scratch Pads, Arranger Track");
    println!("  License: perpetual or PreSonus Sphere subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "studioone".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_s1(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_s1};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/studioone"), "studioone");
        assert_eq!(basename(r"C:\bin\studioone.exe"), "studioone.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("studioone.exe"), "studioone");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_s1(&["--help".to_string()], "studioone"), 0);
        assert_eq!(run_s1(&["-h".to_string()], "studioone"), 0);
        let _ = run_s1(&["--version".to_string()], "studioone");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_s1(&[], "studioone");
    }
}
