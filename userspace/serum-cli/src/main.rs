#![deny(clippy::all)]

//! serum-cli — SlateOS Xfer Records Serum wavetable synthesizer
//!
//! Single personality: `serum`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_serum(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: serum [OPTIONS] [PRESET]");
        println!("Xfer Records Serum 2 (Slate OS) — Advanced wavetable synthesizer");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .fxp preset");
        println!("  --wavetable FILE       Load wavetable (.wav/.serum)");
        println!("  --import-audio FILE    Import audio as wavetable");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Xfer Serum 2.0.7 (Slate OS)"); return 0; }
    println!("Xfer Serum 2.0.7 (Slate OS)");
    println!("  Oscillators: 2 wavetable + sub + noise");
    println!("  Modulation: Drag-and-drop, 4 LFOs, 3 ENV, 4 macros");
    println!("  Effects: 10 high-quality (hyper/dimension, chorus, distortion, etc.)");
    println!("  Wavetable editor: built-in import/morph/process");
    println!("  Plug-in formats: VST2, VST3, AU, AAX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "serum".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_serum(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_serum};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/serum"), "serum");
        assert_eq!(basename(r"C:\bin\serum.exe"), "serum.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("serum.exe"), "serum");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_serum(&["--help".to_string()], "serum"), 0);
        assert_eq!(run_serum(&["-h".to_string()], "serum"), 0);
        let _ = run_serum(&["--version".to_string()], "serum");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_serum(&[], "serum");
    }
}
