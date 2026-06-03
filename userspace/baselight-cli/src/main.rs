#![deny(clippy::all)]

//! baselight-cli — OurOS FilmLight Baselight color grading
//!
//! Single personality: `baselight`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: baselight [OPTIONS] [SCENE]");
        println!("FilmLight Baselight 6 (OurOS) — High-end DI color grading & finishing");
        println!();
        println!("Options:");
        println!("  --open FILE            Open scene");
        println!("  --truelight CDL        Apply Truelight CDL/LUT");
        println!("  --texture-equalizer    Open Texture Equalizer");
        println!("  --pleasing-skintones   Pleasing Skintones tool");
        println!("  --rendering-pool URL   Use FLUX rendering pool");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FilmLight Baselight 6.0.13720 (OurOS)"); return 0; }
    println!("FilmLight Baselight 6.0.13720 (OurOS)");
    println!("  Editions: Baselight, Baselight Studio, Baselight One, Daylight, Editions");
    println!("  Hardware: Blackboard Classic/2, Slate panel");
    println!("  Features: Truelight color science, Boris FX integration, ARRI/RED/SONY native");
    println!("  Workflow: BLG (Baselight Linked Grade) for round-trip with NLEs");
    println!("  Used in: Top-tier feature films, episodics, commercials");
    println!("  License: Subscription / Perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "baselight".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/baselight"), "baselight");
        assert_eq!(basename(r"C:\bin\baselight.exe"), "baselight.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("baselight.exe"), "baselight");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bl(&["--help".to_string()], "baselight"), 0);
        assert_eq!(run_bl(&["-h".to_string()], "baselight"), 0);
        assert_eq!(run_bl(&["--version".to_string()], "baselight"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bl(&[], "baselight"), 0);
    }
}
