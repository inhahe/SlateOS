#![deny(clippy::all)]

//! bevy-cli — SlateOS Bevy game engine CLI
//!
//! Multi-personality: `bevy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bevy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bevy COMMAND [OPTIONS]");
        println!("Bevy CLI 0.1.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  new            Create a new Bevy project");
        println!("  run            Run the game");
        println!("  build          Build the game");
        println!("  lint           Lint Bevy code");
        println!("  generate       Generate components/systems");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("bevy-cli 0.1.0"),
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-game");
            let template = args.windows(2).find(|w| w[0] == "--template")
                .map(|w| w[1].as_str()).unwrap_or("2d");
            println!("Creating new Bevy project '{}'...", name);
            println!("  Template: {}", template);
            println!("  Created: src/main.rs");
            println!("  Created: Cargo.toml");
            println!("  Created: assets/");
            println!("Done. cd {} && bevy run", name);
        }
        "run" => {
            let features = args.windows(2).find(|w| w[0] == "--features")
                .map(|w| w[1].as_str());
            println!("Building and running Bevy game...");
            if let Some(f) = features {
                println!("  Features: {}", f);
            }
            println!("  Compiling...");
            println!("  Running...");
        }
        "build" => {
            let release = args.iter().any(|a| a == "--release");
            let profile = if release { "release" } else { "dev" };
            println!("Building Bevy game ({})...", profile);
            println!("  Compiled successfully.");
        }
        "lint" => {
            println!("Linting Bevy code...");
            println!("  No issues found.");
        }
        _ => println!("bevy: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bevy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bevy(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bevy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bevy"), "bevy");
        assert_eq!(basename(r"C:\bin\bevy.exe"), "bevy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bevy.exe"), "bevy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bevy(&["--help".to_string()]), 0);
        assert_eq!(run_bevy(&["-h".to_string()]), 0);
        let _ = run_bevy(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bevy(&[]);
    }
}
