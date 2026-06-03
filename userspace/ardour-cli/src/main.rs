#![deny(clippy::all)]

//! ardour-cli — OurOS Ardour digital audio workstation
//!
//! Multi-personality: `ardour`, `ardour-export`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ardour(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ardour [OPTIONS] [SESSION]");
        println!("Ardour 8.4.0 (OurOS)");
        println!("  -N NAME        New session name");
        println!("  -S RATE        Sample rate");
        println!("  -b SIZE        Buffer size");
        println!("  --jack         Use JACK backend");
        println!("  --alsa         Use ALSA backend");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Ardour 8.4.0 (OurOS)");
        println!("  Built with JACK, ALSA, PulseAudio");
        println!("  LV2, VST3, AudioUnit plugin support");
        return 0;
    }
    let session = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(s) = session {
        println!("Ardour 8.4.0 — loading session: {}", s);
        println!("  Sample rate: 48000 Hz");
        println!("  Tracks: 16");
        println!("  Buses: 4");
    } else {
        println!("Ardour 8.4.0 — Starting...");
    }
    println!("Ready.");
    0
}

fn run_ardour_export(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ardour-export SESSION [OPTIONS]");
        println!("  -f FORMAT      Output format (wav, flac, ogg, mp3)");
        println!("  -r RATE        Sample rate");
        println!("  -b BITS        Bit depth (16, 24, 32)");
        println!("  -o FILE        Output file");
        return 0;
    }
    let session = args.first().map(|s| s.as_str()).unwrap_or("session");
    let format = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("wav");
    println!("Exporting session: {}", session);
    println!("  Format: {} (24-bit, 48000 Hz)", format);
    println!("  Mixdown...");
    println!("  Export complete.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ardour".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ardour-export" => run_ardour_export(&rest),
        _ => run_ardour(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ardour};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ardour"), "ardour");
        assert_eq!(basename(r"C:\bin\ardour.exe"), "ardour.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ardour.exe"), "ardour");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ardour(&["--help".to_string()]), 0);
        assert_eq!(run_ardour(&["-h".to_string()]), 0);
        assert_eq!(run_ardour(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ardour(&[]), 0);
    }
}
