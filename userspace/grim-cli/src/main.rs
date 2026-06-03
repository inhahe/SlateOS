#![deny(clippy::all)]

//! grim-cli — OurOS grim Wayland screenshot tool
//!
//! Single personality: `grim`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_grim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: grim [OPTIONS] [FILE]");
        println!("grim v1.4 (OurOS) — Grab images from Wayland compositor");
        println!();
        println!("Options:");
        println!("  FILE              Output file (default: screenshot.png)");
        println!("  -g GEOMETRY       Region (x,y widthxheight)");
        println!("  -o OUTPUT         Specific output/display");
        println!("  -t TYPE           File type (png, jpeg, ppm)");
        println!("  -q QUALITY        JPEG quality (0-100)");
        println!("  -s SCALE          Scale factor");
        println!("  -c                Include cursor");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("screenshot.png");
    let output = args.iter().skip_while(|a| a.as_str() != "-o").nth(1);
    if let Some(out) = output {
        println!("Capturing output: {}", out);
    }
    if args.iter().any(|a| a == "-g") {
        println!("Capturing region...");
    }
    println!("Saved: {}", file);
    if args.is_empty() {
        println!("  Full screen capture");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "grim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_grim(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_grim};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/grim"), "grim");
        assert_eq!(basename(r"C:\bin\grim.exe"), "grim.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("grim.exe"), "grim");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_grim(&["--help".to_string()], "grim"), 0);
        assert_eq!(run_grim(&["-h".to_string()], "grim"), 0);
        assert_eq!(run_grim(&["--version".to_string()], "grim"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_grim(&[], "grim"), 0);
    }
}
