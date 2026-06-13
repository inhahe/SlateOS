#![deny(clippy::all)]

//! bitwig-cli — SlateOS Bitwig Studio DAW
//!
//! Single personality: `bitwig`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bitwig(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bitwig [OPTIONS] [FILE.bwproject]");
        println!("Bitwig Studio v5.2 (Slate OS) — Modern music production");
        println!();
        println!("Options:");
        println!("  FILE.bwproject    Open project file");
        println!("  --crash-log       Show last crash log");
        println!("  --scan-plugins    Scan for VST/CLAP plugins");
        println!("  --audio-setup     Configure audio device");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Bitwig Studio v5.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--scan-plugins") {
        println!("Scanning for plugins...");
        println!("  VST3: /usr/lib/vst3/ — 0 found");
        println!("  CLAP: /usr/lib/clap/ — 0 found");
        println!("  Built-in: 42 devices");
        return 0;
    }
    if args.iter().any(|a| a == "--audio-setup") {
        println!("Audio configuration:");
        println!("  Driver: ALSA");
        println!("  Device: Default");
        println!("  Sample rate: 48000 Hz");
        println!("  Buffer: 256 samples (5.3ms)");
        return 0;
    }
    let file = args.first().map(|s| s.as_str()).unwrap_or("Untitled.bwproject");
    println!("Bitwig Studio v5.2 — Opening: {}", file);
    println!("  Tracks: 8 (4 audio, 4 MIDI)");
    println!("  Modulators: Grid, Polymer, Phase-4");
    println!("  Audio engine: multi-core");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bitwig".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bitwig(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bitwig};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bitwig"), "bitwig");
        assert_eq!(basename(r"C:\bin\bitwig.exe"), "bitwig.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bitwig.exe"), "bitwig");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bitwig(&["--help".to_string()], "bitwig"), 0);
        assert_eq!(run_bitwig(&["-h".to_string()], "bitwig"), 0);
        let _ = run_bitwig(&["--version".to_string()], "bitwig");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bitwig(&[], "bitwig");
    }
}
