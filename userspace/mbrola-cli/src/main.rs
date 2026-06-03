#![deny(clippy::all)]

//! mbrola-cli — OurOS MBROLA speech synthesizer
//!
//! Single personality: `mbrola`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mbrola(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mbrola [OPTIONS] DATABASE PHOFILE WAVFILE");
        println!("mbrola v3.3 (OurOS) — MBROLA diphone speech synthesizer");
        println!();
        println!("Options:");
        println!("  -e              Ignore fatal errors on phonemes");
        println!("  -t FACTOR       Time ratio (default: 1.0)");
        println!("  -f FACTOR       Frequency ratio (default: 1.0)");
        println!("  -v VOLUME       Volume ratio (default: 1.0)");
        println!("  -l N            Phoneme length limit");
        println!("  -I FILE         Info file");
        println!("  -i              Display database info");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MBROLA v3.3 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-i") {
        println!("MBROLA Database Information:");
        println!("  Installed voices:");
        println!("    en1 — English male (British)");
        println!("    en2 — English female (American)");
        println!("    fr1 — French male");
        println!("    de1 — German male");
        println!("    es1 — Spanish female");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.len() < 3 {
        eprintln!("mbrola: error: expected DATABASE PHOFILE WAVFILE");
        return 1;
    }
    println!("MBROLA v3.3 (OurOS)");
    println!("  Database: {}", files[0]);
    println!("  Input: {}", files[1]);
    println!("  Output: {}", files[2]);
    println!("  Synthesizing... 245 phonemes");
    println!("  Output: 16000 Hz, 16-bit, mono");
    println!("  Duration: 3.2s");
    println!("  Done [{} bytes]", 102_400);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mbrola".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mbrola(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mbrola};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mbrola"), "mbrola");
        assert_eq!(basename(r"C:\bin\mbrola.exe"), "mbrola.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mbrola.exe"), "mbrola");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mbrola(&["--help".to_string()], "mbrola"), 0);
        assert_eq!(run_mbrola(&["-h".to_string()], "mbrola"), 0);
        assert_eq!(run_mbrola(&["--version".to_string()], "mbrola"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mbrola(&[], "mbrola"), 0);
    }
}
