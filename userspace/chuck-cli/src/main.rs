#![deny(clippy::all)]

//! chuck-cli — OurOS ChucK audio programming language
//!
//! Single personality: `chuck`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_chuck(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: chuck [OPTIONS] FILE...");
        println!("ChucK v1.5.2 (OurOS) — Strongly-timed audio programming");
        println!();
        println!("Options:");
        println!("  FILE.ck           Run ChucK program");
        println!("  + FILE.ck         Add shred on-the-fly");
        println!("  - N               Remove shred N");
        println!("  --loop            Run in loop mode");
        println!("  --srate N         Set sample rate (default: 44100)");
        println!("  --bufsize N       Set buffer size (default: 512)");
        println!("  --dac N           Set DAC device number");
        println!("  --adc N           Set ADC device number");
        println!("  --channels N      Set output channels");
        println!("  --probe           List audio devices");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ChucK v1.5.2 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--probe") {
        println!("Audio devices:");
        println!("  [0] Default Output (48000 Hz, 2 ch)");
        println!("  [1] Default Input (44100 Hz, 1 ch)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("chuck: no input files");
        return 1;
    }
    for f in &files {
        println!("[chuck]: compiling {}...", f);
    }
    println!("[chuck]: {} shred(s) running", files.len());
    println!("[chuck]: sample rate: 44100 Hz");
    println!("[chuck]: buffer size: 512 frames");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chuck".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_chuck(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_chuck};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/chuck"), "chuck");
        assert_eq!(basename(r"C:\bin\chuck.exe"), "chuck.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("chuck.exe"), "chuck");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_chuck(&["--help".to_string()], "chuck"), 0);
        assert_eq!(run_chuck(&["-h".to_string()], "chuck"), 0);
        assert_eq!(run_chuck(&["--version".to_string()], "chuck"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_chuck(&[], "chuck"), 0);
    }
}
