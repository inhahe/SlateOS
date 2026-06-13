#![deny(clippy::all)]

//! raylib-cli — SlateOS Raylib game development helper
//!
//! Multi-personality: `raylib-config`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_raylib(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: raylib-config [OPTIONS]");
        println!("Raylib config 5.0 (SlateOS)");
        println!();
        println!("Options:");
        println!("  --version        Print raylib version");
        println!("  --cflags         Print compiler flags");
        println!("  --libs           Print linker flags");
        println!("  --static-libs    Print static linker flags");
        println!("  --prefix         Print install prefix");
        println!("  --info           Show build info");
        return 0;
    }
    for arg in args {
        match arg.as_str() {
            "--version" => println!("5.0"),
            "--cflags" => println!("-I/usr/include"),
            "--libs" => println!("-L/usr/lib -lraylib -lm -ldl -lpthread"),
            "--static-libs" => println!("-L/usr/lib -lraylib -lm -ldl -lpthread -lrt"),
            "--prefix" => println!("/usr"),
            "--info" => {
                println!("raylib 5.0 — A simple and easy-to-use library");
                println!("  Platform: SlateOS (Desktop)");
                println!("  Graphics: OpenGL 3.3");
                println!("  Audio: miniaudio");
                println!("  Build type: Release");
            }
            _ => {}
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "raylib-config".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_raylib(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_raylib};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/raylib"), "raylib");
        assert_eq!(basename(r"C:\bin\raylib.exe"), "raylib.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("raylib.exe"), "raylib");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_raylib(&["--help".to_string()]), 0);
        assert_eq!(run_raylib(&["-h".to_string()]), 0);
        let _ = run_raylib(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_raylib(&[]);
    }
}
