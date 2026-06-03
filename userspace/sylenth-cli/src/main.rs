#![deny(clippy::all)]

//! sylenth-cli — OurOS LennarDigital Sylenth1 synthesizer
//!
//! Single personality: `sylenth`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sylenth(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sylenth [OPTIONS] [PRESET]");
        println!("LennarDigital Sylenth1 v3.0 (OurOS) — Virtual analog VSTi synthesizer");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .fxp/.fxb preset/bank");
        println!("  --bank FILE            Load preset bank");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LennarDigital Sylenth1 v3.0.7 (OurOS)"); return 0; }
    println!("LennarDigital Sylenth1 v3.0.7 (OurOS)");
    println!("  Architecture: 4 unison oscillator engines (16 voices total)");
    println!("  Filters: 2 with 4 filter modes each, drive, warm-mode");
    println!("  Modulation: 2 ADSR + 2 LFO + 2 X/Y mod-mats, master ENV");
    println!("  Effects: Arp, Distortion, Phaser, Chorus, EQ, Delay, Reverb, Compressor");
    println!("  Plug-in formats: VST2, VST3, AU");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sylenth".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sylenth(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sylenth};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sylenth"), "sylenth");
        assert_eq!(basename(r"C:\bin\sylenth.exe"), "sylenth.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sylenth.exe"), "sylenth");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sylenth(&["--help".to_string()], "sylenth"), 0);
        assert_eq!(run_sylenth(&["-h".to_string()], "sylenth"), 0);
        assert_eq!(run_sylenth(&["--version".to_string()], "sylenth"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sylenth(&[], "sylenth"), 0);
    }
}
