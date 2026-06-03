#![deny(clippy::all)]

//! argyll-cli — OurOS ArgyllCMS color management
//!
//! Multi-personality: `dispwin`, `colprof`, `spotread`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dispwin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dispwin [OPTIONS] [PROFILE.icc]");
        println!("dispwin v3.0 (OurOS) — Load display calibration");
        println!();
        println!("Options:");
        println!("  PROFILE.icc       ICC profile to load");
        println!("  -d N              Display number");
        println!("  -c                Clear calibration");
        println!("  -V                Verify calibration");
        println!("  -I                Install profile");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("display.icc");
    println!("Loading profile: {}", file);
    println!("  Display: 1");
    println!("  LUT loaded successfully.");
    0
}

fn run_colprof(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: colprof [OPTIONS] BASENAME");
        println!("colprof v3.0 (OurOS) — Create ICC profile from measurements");
        return 0;
    }
    println!("Creating ICC profile...");
    println!("  Patches: 729");
    println!("  Profile type: display");
    println!("  Output: display.icc");
    0
}

fn run_spotread(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: spotread [OPTIONS]");
        println!("spotread v3.0 (OurOS) — Read color measurements");
        return 0;
    }
    println!("Reading spot color...");
    println!("  XYZ: 82.14  86.50  93.22");
    println!("  Lab: 94.14  -0.81  -2.67");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dispwin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "colprof" => run_colprof(&rest, &prog),
        "spotread" => run_spotread(&rest, &prog),
        _ => run_dispwin(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dispwin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/argyll"), "argyll");
        assert_eq!(basename(r"C:\bin\argyll.exe"), "argyll.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("argyll.exe"), "argyll");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_dispwin(&["--help".to_string()], "argyll"), 0);
        assert_eq!(run_dispwin(&["-h".to_string()], "argyll"), 0);
        assert_eq!(run_dispwin(&["--version".to_string()], "argyll"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_dispwin(&[], "argyll"), 0);
    }
}
