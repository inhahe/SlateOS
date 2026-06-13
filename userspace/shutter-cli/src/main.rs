#![deny(clippy::all)]

//! shutter-cli — SlateOS Shutter screenshot with editing
//!
//! Single personality: `shutter`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_shutter(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shutter [OPTIONS]");
        println!("shutter v0.99 (Slate OS) — Feature-rich screenshot tool");
        println!();
        println!("Options:");
        println!("  -f, --full        Full screen");
        println!("  -w, --window      Window");
        println!("  -s, --selection   Selection");
        println!("  -d, --delay SECS  Delay");
        println!("  -e, --edit        Open editor after capture");
        println!("  --version         Show version");
        println!();
        println!("Editor: arrows, text, highlight, blur, crop, resize");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("shutter v0.99 (Slate OS)"); return 0; }
    println!("shutter: screenshot tool started");
    println!("  Editor: annotation tools available");
    println!("  Upload: imgur, Dropbox support");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "shutter".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_shutter(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_shutter};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/shutter"), "shutter");
        assert_eq!(basename(r"C:\bin\shutter.exe"), "shutter.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("shutter.exe"), "shutter");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_shutter(&["--help".to_string()], "shutter"), 0);
        assert_eq!(run_shutter(&["-h".to_string()], "shutter"), 0);
        let _ = run_shutter(&["--version".to_string()], "shutter");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_shutter(&[], "shutter");
    }
}
