#![deny(clippy::all)]

//! hyprpaper-cli — Slate OS hyprpaper wallpaper utility
//!
//! Multi-personality: `hyprpaper`, `hyprctl-paper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hyprpaper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hyprpaper [OPTIONS]");
        println!("hyprpaper v0.7 (Slate OS) — Wayland wallpaper utility");
        println!();
        println!("Options:");
        println!("  -c CONFIG         Config file path");
        println!("  --no-fractional   Disable fractional scaling");
        println!("  --version         Show version");
        println!();
        println!("Configure via ~/.config/hypr/hyprpaper.conf");
        println!("  preload = ~/Pictures/wallpaper.png");
        println!("  wallpaper = ,~/Pictures/wallpaper.png");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hyprpaper v0.7 (Slate OS)"); return 0; }
    println!("hyprpaper: wallpaper daemon running");
    println!("  Preloaded: 1 image");
    0
}

fn run_hyprctl_paper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hyprctl-paper COMMAND [ARGS]");
        println!("hyprctl-paper v0.7 (Slate OS) — Control hyprpaper");
        println!();
        println!("Commands:");
        println!("  preload PATH      Preload wallpaper");
        println!("  wallpaper OUT,PATH Set wallpaper");
        println!("  unload PATH       Unload wallpaper");
        println!("  listloaded        List preloaded wallpapers");
        println!("  listactive        List active wallpapers");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "listloaded" => println!("~/Pictures/wallpaper.png"),
        "listactive" => println!("HDMI-A-1 = ~/Pictures/wallpaper.png"),
        "preload" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("(none)");
            println!("Preloaded: {}", path);
        }
        "wallpaper" => {
            let spec = args.get(1).map(|s| s.as_str()).unwrap_or("(none)");
            println!("Set wallpaper: {}", spec);
        }
        _ => println!("hyprctl-paper: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hyprpaper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "hyprctl-paper" => run_hyprctl_paper(&rest, &prog),
        _ => run_hyprpaper(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hyprpaper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hyprpaper"), "hyprpaper");
        assert_eq!(basename(r"C:\bin\hyprpaper.exe"), "hyprpaper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hyprpaper.exe"), "hyprpaper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hyprpaper(&["--help".to_string()], "hyprpaper"), 0);
        assert_eq!(run_hyprpaper(&["-h".to_string()], "hyprpaper"), 0);
        let _ = run_hyprpaper(&["--version".to_string()], "hyprpaper");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hyprpaper(&[], "hyprpaper");
    }
}
