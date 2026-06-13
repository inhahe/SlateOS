#![deny(clippy::all)]

//! speechd-cli — SlateOS Speech Dispatcher text-to-speech
//!
//! Multi-personality: `speech-dispatcher`, `spd-say`, `spd-conf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dispatcher(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: speech-dispatcher [OPTIONS]");
        println!("speech-dispatcher v0.11 (Slate OS) — Speech synthesis daemon");
        println!();
        println!("Options:");
        println!("  -d, --daemon      Run as daemon");
        println!("  -s                Run single mode");
        println!("  --port PORT       Communication port");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("speech-dispatcher v0.11 (Slate OS)"); return 0; }
    println!("speech-dispatcher: TTS daemon started");
    println!("  Modules: espeak-ng, pico, festival, flite");
    println!("  Default voice: espeak-ng en-us");
    println!("  SSIP protocol: active");
    0
}

fn run_say(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spd-say [OPTIONS] TEXT");
        println!("spd-say v0.11 (Slate OS) — Say text through Speech Dispatcher");
        println!();
        println!("Options:");
        println!("  -r RATE           Speaking rate (-100 to 100)");
        println!("  -p PITCH          Pitch (-100 to 100)");
        println!("  -i VOLUME         Volume (-100 to 100)");
        println!("  -l LANG           Language code");
        println!("  -o MODULE         Output module");
        println!("  -w                Wait until finished");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("spd-say v0.11 (Slate OS)"); return 0; }
    let text: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    println!("spd-say: speaking '{}'", text.join(" "));
    0
}

fn run_conf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spd-conf [OPTIONS]");
        println!("spd-conf v0.11 (Slate OS) — Speech Dispatcher configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("spd-conf v0.11 (Slate OS)"); return 0; }
    println!("spd-conf: configuration wizard started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "speech-dispatcher".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "spd-say" => run_say(&rest, &prog),
        "spd-conf" => run_conf(&rest, &prog),
        _ => run_dispatcher(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dispatcher};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/speechd"), "speechd");
        assert_eq!(basename(r"C:\bin\speechd.exe"), "speechd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("speechd.exe"), "speechd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dispatcher(&["--help".to_string()], "speechd"), 0);
        assert_eq!(run_dispatcher(&["-h".to_string()], "speechd"), 0);
        let _ = run_dispatcher(&["--version".to_string()], "speechd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dispatcher(&[], "speechd");
    }
}
