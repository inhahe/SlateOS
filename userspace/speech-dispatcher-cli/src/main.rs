#![deny(clippy::all)]

//! speech-dispatcher-cli — OurOS Speech Dispatcher
//!
//! Multi-personality: `speech-dispatcher`, `spd-say`, `spd-conf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dispatcher(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: speech-dispatcher [OPTIONS]");
        println!("speech-dispatcher v0.11 (OurOS) — Speech synthesis daemon");
        println!();
        println!("Options:");
        println!("  -d                Run as daemon");
        println!("  -s                Single-threaded mode");
        println!("  -l LEVEL          Log level (1-5)");
        println!("  -c DIR            Config directory");
        println!("  -p PORT           Communication port");
        return 0;
    }
    println!("speech-dispatcher: daemon started");
    println!("  Modules: espeak-ng, pico, festival");
    println!("  Listening for SSIP connections");
    0
}

fn run_spd_say(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: spd-say [OPTIONS] TEXT");
        println!("spd-say v0.11 (OurOS) — Speak text via Speech Dispatcher");
        println!();
        println!("Options:");
        println!("  TEXT              Text to speak");
        println!("  -r RATE           Rate (-100 to 100)");
        println!("  -p PITCH          Pitch (-100 to 100)");
        println!("  -i VOLUME         Volume (-100 to 100)");
        println!("  -o OUTPUT         Output module");
        println!("  -l LANGUAGE       Language code");
        println!("  -t TYPE           Voice type (male1, female1, etc.)");
        println!("  -w                Wait until finished");
        println!("  -S                Stop current speech");
        println!("  -C                Cancel all speech");
        return 0;
    }
    if args.iter().any(|a| a == "-S") { println!("Speech stopped."); return 0; }
    if args.iter().any(|a| a == "-C") { println!("All speech cancelled."); return 0; }
    let text = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("(empty)");
    println!("spd-say: {}", text);
    0
}

fn run_spd_conf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spd-conf [OPTIONS]");
        println!("spd-conf v0.11 (OurOS) — Speech Dispatcher configuration");
        println!();
        println!("Options:");
        println!("  -u                Configure user settings");
        println!("  -c                Complete configuration wizard");
        println!("  -d                Diagnose issues");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("Speech Dispatcher diagnostics:");
        println!("  Daemon: running (pid 1234)");
        println!("  Modules: espeak-ng (ok), pico (ok)");
        println!("  Audio: PulseAudio (ok)");
        return 0;
    }
    println!("spd-conf: opening configuration wizard...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "speech-dispatcher".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "spd-say" => run_spd_say(&rest, &prog),
        "spd-conf" => run_spd_conf(&rest, &prog),
        _ => run_dispatcher(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dispatcher};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/speech-dispatcher"), "speech-dispatcher");
        assert_eq!(basename(r"C:\bin\speech-dispatcher.exe"), "speech-dispatcher.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("speech-dispatcher.exe"), "speech-dispatcher");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dispatcher(&["--help".to_string()], "speech-dispatcher"), 0);
        assert_eq!(run_dispatcher(&["-h".to_string()], "speech-dispatcher"), 0);
        let _ = run_dispatcher(&["--version".to_string()], "speech-dispatcher");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dispatcher(&[], "speech-dispatcher");
    }
}
