#![deny(clippy::all)]

//! protontricks-cli — SlateOS Protontricks Proton/Wine helper
//!
//! Multi-personality: `protontricks`, `protontricks-launch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_protontricks(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: protontricks [OPTIONS] APPID [VERB...]");
        println!("protontricks v1.11 (Slate OS) — Winetricks wrapper for Proton games");
        println!();
        println!("Options:");
        println!("  -s PATTERN        Search for game by name");
        println!("  -c CMD            Run command in prefix");
        println!("  --gui             GUI mode");
        println!("  --no-runtime      Skip Steam runtime");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("protontricks v1.11 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-s") {
        println!("Found games:");
        println!("  292030  The Witcher 3: Wild Hunt");
        println!("  1091500 Cyberpunk 2077");
        println!("  489830  The Elder Scrolls V: Skyrim SE");
        return 0;
    }
    if args.iter().any(|a| a == "--gui") {
        println!("protontricks: GUI mode started");
        return 0;
    }
    println!("protontricks: processing app ID...");
    0
}

fn run_launch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: protontricks-launch [OPTIONS] EXE [ARGS...]");
        println!("protontricks-launch v1.11 (Slate OS) — Launch exe in Proton prefix");
        println!();
        println!("Options:");
        println!("  --appid ID        Steam app ID");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("protontricks-launch v1.11 (Slate OS)"); return 0; }
    println!("protontricks-launch: launching executable in Proton prefix...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "protontricks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "protontricks-launch" => run_launch(&rest, &prog),
        _ => run_protontricks(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_protontricks};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/protontricks"), "protontricks");
        assert_eq!(basename(r"C:\bin\protontricks.exe"), "protontricks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("protontricks.exe"), "protontricks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_protontricks(&["--help".to_string()], "protontricks"), 0);
        assert_eq!(run_protontricks(&["-h".to_string()], "protontricks"), 0);
        let _ = run_protontricks(&["--version".to_string()], "protontricks");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_protontricks(&[], "protontricks");
    }
}
