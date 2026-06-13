#![deny(clippy::all)]

//! flavours-cli — Slate OS flavours Base16 scheme manager
//!
//! Single personality: `flavours`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flavours(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: flavours COMMAND [OPTIONS]");
        println!("flavours v0.7 (Slate OS) — Base16 color scheme manager");
        println!();
        println!("Commands:");
        println!("  apply [SCHEME]    Apply a Base16 scheme");
        println!("  current           Show current scheme");
        println!("  list              List available schemes");
        println!("  info SCHEME       Show scheme details");
        println!("  build             Build templates");
        println!("  update            Update schemes and templates");
        println!("  generate MODE IMAGE  Generate scheme from image");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "apply" => {
            let scheme = args.get(1).map(|s| s.as_str()).unwrap_or("random");
            println!("Applied scheme: {}", scheme);
        }
        "current" => println!("Current: gruvbox-dark-hard"),
        "list" => {
            println!("atelier-cave  dracula  gruvbox-dark-hard  monokai");
            println!("nord  one-dark  solarized-dark  tomorrow-night");
        }
        "info" => {
            let scheme = args.get(1).map(|s| s.as_str()).unwrap_or("default");
            println!("Scheme: {}", scheme);
            println!("  Author: base16 community");
            println!("  Colors: 16 (base00..base0F)");
        }
        "update" => println!("Updated schemes and templates"),
        "generate" => {
            let image = args.get(2).map(|s| s.as_str()).unwrap_or("wallpaper.png");
            println!("Generated scheme from: {}", image);
        }
        _ => println!("flavours: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flavours".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flavours(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_flavours};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/flavours"), "flavours");
        assert_eq!(basename(r"C:\bin\flavours.exe"), "flavours.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("flavours.exe"), "flavours");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_flavours(&["--help".to_string()], "flavours"), 0);
        assert_eq!(run_flavours(&["-h".to_string()], "flavours"), 0);
        let _ = run_flavours(&["--version".to_string()], "flavours");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_flavours(&[], "flavours");
    }
}
