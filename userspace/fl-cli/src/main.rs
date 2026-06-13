#![deny(clippy::all)]

//! fl-cli — SlateOS Image-Line FL Studio DAW
//!
//! Single personality: `fl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fl [OPTIONS] [PROJECT]");
        println!("Image-Line FL Studio 21 (SlateOS) — Pattern-based DAW & step sequencer");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .flp project");
        println!("  --render FILE          Render to WAV/MP3/OGG/FLAC");
        println!("  --tempo BPM            Set tempo");
        println!("  --piano-roll           Open Piano Roll");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FL Studio 21.2.3 Producer Edition (SlateOS)"); return 0; }
    println!("FL Studio 21.2.3 Producer Edition (SlateOS)");
    println!("  Editions: Fruity, Producer, Signature Bundle, All Plugins Bundle");
    println!("  Workflow: Pattern blocks → Playlist → Mixer");
    println!("  Generators: Sytrus, Harmor, FLEX, Slicex, FPC drum sampler");
    println!("  Lifetime free updates (signature feature)");
    println!("  Plug-in formats: VST2, VST3, native");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fl"), "fl");
        assert_eq!(basename(r"C:\bin\fl.exe"), "fl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fl.exe"), "fl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fl(&["--help".to_string()], "fl"), 0);
        assert_eq!(run_fl(&["-h".to_string()], "fl"), 0);
        let _ = run_fl(&["--version".to_string()], "fl");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fl(&[], "fl");
    }
}
