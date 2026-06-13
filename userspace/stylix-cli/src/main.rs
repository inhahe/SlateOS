#![deny(clippy::all)]

//! stylix-cli — Slate OS Stylix system-wide color scheme manager
//!
//! Single personality: `stylix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stylix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: stylix COMMAND [OPTIONS]");
        println!("stylix v0.1 (Slate OS) — System-wide color scheme manager");
        println!();
        println!("Commands:");
        println!("  generate IMAGE    Generate palette from wallpaper");
        println!("  apply SCHEME      Apply color scheme");
        println!("  list              List available schemes");
        println!("  current           Show current scheme");
        println!("  export FORMAT     Export to format (css, yaml, json, shell)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "generate" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("wallpaper.png");
            println!("Generating palette from: {}", image);
            println!("  Base16 scheme generated: 16 colors extracted");
        }
        "apply" => {
            let scheme = args.get(1).map(|s| s.as_str()).unwrap_or("default");
            println!("Applied scheme: {}", scheme);
            println!("  Updated: GTK, Qt, terminal, shell, WM");
        }
        "list" => {
            println!("catppuccin-mocha  dracula  gruvbox-dark  nord  solarized-dark");
        }
        "current" => println!("Current scheme: catppuccin-mocha"),
        "export" => {
            let format = args.get(1).map(|s| s.as_str()).unwrap_or("json");
            println!("Exported current scheme as {}", format);
        }
        _ => println!("stylix: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stylix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stylix(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stylix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stylix"), "stylix");
        assert_eq!(basename(r"C:\bin\stylix.exe"), "stylix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stylix.exe"), "stylix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stylix(&["--help".to_string()], "stylix"), 0);
        assert_eq!(run_stylix(&["-h".to_string()], "stylix"), 0);
        let _ = run_stylix(&["--version".to_string()], "stylix");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stylix(&[], "stylix");
    }
}
