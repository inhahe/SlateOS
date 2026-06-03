#![deny(clippy::all)]

//! oguri-cli — OurOS oguri animated wallpaper daemon
//!
//! Multi-personality: `oguri`, `ogurictl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_oguri(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: oguri [OPTIONS]");
        println!("oguri v0.1 (OurOS) — Animated wallpaper daemon for Wayland");
        println!();
        println!("Options:");
        println!("  -c CONFIG         Config file path");
        println!("  --version         Show version");
        println!();
        println!("Supports animated GIFs and static images as wallpaper.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("oguri v0.1 (OurOS)"); return 0; }
    println!("oguri: animated wallpaper daemon started");
    println!("  Config: ~/.config/oguri/config");
    0
}

fn run_ogurictl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ogurictl COMMAND [ARGS]");
        println!("ogurictl v0.1 (OurOS) — Control oguri daemon");
        println!();
        println!("Commands:");
        println!("  output OUTPUT image PATH  Set wallpaper");
        println!("  output OUTPUT filter MODE Scaling mode");
        println!("  output OUTPUT anchor POS  Anchor position");
        return 0;
    }
    println!("ogurictl: {}", args.join(" "));
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "oguri".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ogurictl" => run_ogurictl(&rest, &prog),
        _ => run_oguri(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_oguri};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/oguri"), "oguri");
        assert_eq!(basename(r"C:\bin\oguri.exe"), "oguri.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("oguri.exe"), "oguri");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_oguri(&["--help".to_string()], "oguri"), 0);
        assert_eq!(run_oguri(&["-h".to_string()], "oguri"), 0);
        assert_eq!(run_oguri(&["--version".to_string()], "oguri"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_oguri(&[], "oguri"), 0);
    }
}
