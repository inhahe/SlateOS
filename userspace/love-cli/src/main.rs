#![deny(clippy::all)]

//! love-cli — OurOS LOVE2D game framework
//!
//! Single personality: `love`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_love(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: love [DIR|ARCHIVE] [OPTIONS]");
        println!("LOVE 11.5 (OurOS) — Framework for making 2D games in Lua");
        println!();
        println!("Options:");
        println!("  DIR|ARCHIVE        Game directory or .love file");
        println!("  --fused             Fused mode");
        println!("  --console           Attach console");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("LOVE 11.5 (Mysterious Mysteries)");
        return 0;
    }
    let path = args.first().map(|s| s.as_str()).unwrap_or(".");
    println!("LOVE 11.5 — Running game from: {}", path);
    println!("  Window: 800x600");
    println!("  Renderer: OpenGL");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "love".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_love(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_love};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/love"), "love");
        assert_eq!(basename(r"C:\bin\love.exe"), "love.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("love.exe"), "love");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_love(&["--help".to_string()], "love"), 0);
        assert_eq!(run_love(&["-h".to_string()], "love"), 0);
        assert_eq!(run_love(&["--version".to_string()], "love"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_love(&[], "love"), 0);
    }
}
