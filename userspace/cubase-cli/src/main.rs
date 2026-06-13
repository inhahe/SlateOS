#![deny(clippy::all)]

//! cubase-cli — Slate OS Steinberg Cubase DAW
//!
//! Single personality: `cubase`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cubase(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cubase [OPTIONS] [PROJECT]");
        println!("Steinberg Cubase Pro 13 (Slate OS) — Professional MIDI/audio DAW");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .cpr project");
        println!("  --export FORMAT FILE   Export mix (wav/mp3/aac/flac)");
        println!("  --tempo BPM            Set tempo");
        println!("  --halion               Open Halion sampler");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Steinberg Cubase Pro 13.0.40 (Slate OS)"); return 0; }
    println!("Steinberg Cubase Pro 13.0.40 (Slate OS)");
    println!("  Editions: Elements, Artist, Pro");
    println!("  Plug-in format: VST (Steinberg invented it), VST3, ASIO");
    println!("  Features: VariAudio (pitch), Chord Track, Score Editor");
    println!("  Instruments: HALion Sonic, Groove Agent, Padshop, Retrologue");
    println!("  Surround: Dolby Atmos, Ambisonics");
    println!("  License: Steinberg License Manager (no dongle since v12)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cubase".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cubase(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cubase};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cubase"), "cubase");
        assert_eq!(basename(r"C:\bin\cubase.exe"), "cubase.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cubase.exe"), "cubase");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cubase(&["--help".to_string()], "cubase"), 0);
        assert_eq!(run_cubase(&["-h".to_string()], "cubase"), 0);
        let _ = run_cubase(&["--version".to_string()], "cubase");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cubase(&[], "cubase");
    }
}
