#![deny(clippy::all)]

//! hindenburg-cli — OurOS Hindenburg PRO podcast editor
//!
//! Single personality: `hindenburg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hindenburg [OPTIONS] [PROJECT]");
        println!("Hindenburg PRO 2 (OurOS) — Radio & podcast production");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .nhsx project");
        println!("  --auto-leveler         Apply Auto Leveler on import");
        println!("  --voice-profiler       Apply Voice Profiler");
        println!("  --publish DEST         Publish to host (Buzzsprout/Spreaker/etc.)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Hindenburg PRO 2.4.0 (OurOS)"); return 0; }
    println!("Hindenburg PRO 2.4.0 (OurOS)");
    println!("  Editions: Hindenburg PRO (was Journalist), Narrator (audiobooks)");
    println!("  Features: Auto Leveler, Voice Profiler, automatic transcripts");
    println!("  Workflow: Magic clipboard, multi-track non-destructive editing");
    println!("  Publishing: Direct upload to PodBean, Buzzsprout, SoundCloud, etc.");
    println!("  Plug-in formats: VST3, AU host");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hindenburg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hindenburg"), "hindenburg");
        assert_eq!(basename(r"C:\bin\hindenburg.exe"), "hindenburg.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hindenburg.exe"), "hindenburg");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_hb(&["--help".to_string()], "hindenburg"), 0);
        assert_eq!(run_hb(&["-h".to_string()], "hindenburg"), 0);
        assert_eq!(run_hb(&["--version".to_string()], "hindenburg"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_hb(&[], "hindenburg"), 0);
    }
}
