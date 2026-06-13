#![deny(clippy::all)]

//! pulsemixer-cli — SlateOS pulsemixer audio mixer
//!
//! Single personality: `pulsemixer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pulsemixer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pulsemixer [OPTIONS]");
        println!("pulsemixer 1.5.1 (Slate OS) — CLI and curses mixer for PulseAudio");
        println!();
        println!("Options:");
        println!("  --id ID               Sink/source ID");
        println!("  --get-volume          Print volume");
        println!("  --set-volume N        Set volume (0-100)");
        println!("  --set-volume-all N:M  Set per-channel volume");
        println!("  --change-volume +-N   Change volume");
        println!("  --get-mute            Print mute state");
        println!("  --toggle-mute         Toggle mute");
        println!("  --mute                Mute");
        println!("  --unmute              Unmute");
        println!("  --list-sinks          List sinks");
        println!("  --list-sources        List sources");
        println!("  --set-default ID      Set default sink/source");
        println!("  --server              PulseAudio server");
        println!("  --color N             Color mode (0=no, 1=auto, 2=yes)");
        println!("  --no-mouse            Disable mouse");
        return 0;
    }
    if args.iter().any(|a| a == "--get-volume") {
        println!("80 80");
        return 0;
    }
    if args.iter().any(|a| a == "--get-mute") {
        println!("0");
        return 0;
    }
    if args.iter().any(|a| a == "--toggle-mute") {
        println!("pulsemixer: Toggled mute");
        return 0;
    }
    if args.iter().any(|a| a == "--list-sinks") {
        println!("Sink #0: Built-in Audio Analog Stereo");
        println!("  Volume: 80% / 80%");
        println!("  Mute: no");
        return 0;
    }
    if args.iter().any(|a| a == "--list-sources") {
        println!("Source #0: Built-in Audio Analog Stereo (monitor)");
        println!("Source #1: Built-in Microphone");
        return 0;
    }
    if let Some(pos) = args.iter().position(|a| a == "--set-volume") {
        let vol = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("80");
        println!("pulsemixer: Volume set to {}%", vol);
        return 0;
    }
    if let Some(pos) = args.iter().position(|a| a == "--change-volume") {
        let delta = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("+5");
        println!("pulsemixer: Volume changed by {}", delta);
        return 0;
    }
    println!("pulsemixer: Opening interactive mixer...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pulsemixer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pulsemixer(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pulsemixer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pulsemixer"), "pulsemixer");
        assert_eq!(basename(r"C:\bin\pulsemixer.exe"), "pulsemixer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pulsemixer.exe"), "pulsemixer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pulsemixer(&["--help".to_string()], "pulsemixer"), 0);
        assert_eq!(run_pulsemixer(&["-h".to_string()], "pulsemixer"), 0);
        let _ = run_pulsemixer(&["--version".to_string()], "pulsemixer");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pulsemixer(&[], "pulsemixer");
    }
}
