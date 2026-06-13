#![deny(clippy::all)]

//! contour-cli — SlateOS Contour terminal emulator
//!
//! Single personality: `contour`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_contour(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: contour [OPTIONS]");
        println!("contour v0.4 (Slate OS) — Modern terminal emulator");
        println!();
        println!("Options:");
        println!("  config PATH       Configuration file");
        println!("  --debug COMP      Enable debug for component");
        println!("  --live-config     Live config reload");
        println!("  --version         Show version");
        println!();
        println!("Subcommands:");
        println!("  capture           Capture terminal output");
        println!("  set               Set terminal property");
        println!("  list              List available profiles");
        println!("  info              Show system info");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("contour v0.4 (Slate OS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "info" => {
            println!("Contour Terminal v0.4");
            println!("  Renderer: OpenGL");
            println!("  Font: JetBrains Mono, 12pt");
            println!("  Color scheme: default");
            println!("  VT features: sixel, hyperlinks, images");
        }
        "list" => {
            println!("Profiles:");
            println!("  default (active)");
            println!("  programming");
            println!("  presentation");
        }
        _ => {
            println!("Contour terminal starting...");
            println!("  Profile: default");
            println!("  Shell: /bin/sh");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "contour".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_contour(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_contour};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/contour"), "contour");
        assert_eq!(basename(r"C:\bin\contour.exe"), "contour.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("contour.exe"), "contour");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_contour(&["--help".to_string()], "contour"), 0);
        assert_eq!(run_contour(&["-h".to_string()], "contour"), 0);
        let _ = run_contour(&["--version".to_string()], "contour");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_contour(&[], "contour");
    }
}
