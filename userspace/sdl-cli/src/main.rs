#![deny(clippy::all)]

//! sdl-cli — SlateOS SDL2 config tool
//!
//! Multi-personality: `sdl2-config`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sdl2_config(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sdl2-config [OPTIONS]");
        println!("SDL2 config 2.30.0 (Slate OS)");
        println!();
        println!("Options:");
        println!("  --version        Print SDL version");
        println!("  --cflags         Print compiler flags");
        println!("  --libs           Print linker flags");
        println!("  --static-libs    Print static linker flags");
        println!("  --prefix         Print install prefix");
        println!("  --exec-prefix    Print exec prefix");
        return 0;
    }
    for arg in args {
        match arg.as_str() {
            "--version" => println!("2.30.0"),
            "--cflags" => println!("-I/usr/include/SDL2 -D_REENTRANT"),
            "--libs" => println!("-L/usr/lib -lSDL2"),
            "--static-libs" => println!("-L/usr/lib -lSDL2 -lm -ldl -lpthread -lrt"),
            "--prefix" => println!("/usr"),
            "--exec-prefix" => println!("/usr"),
            _ => {}
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sdl2-config".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sdl2_config(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sdl2_config};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sdl"), "sdl");
        assert_eq!(basename(r"C:\bin\sdl.exe"), "sdl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sdl.exe"), "sdl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sdl2_config(&["--help".to_string()]), 0);
        assert_eq!(run_sdl2_config(&["-h".to_string()]), 0);
        let _ = run_sdl2_config(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sdl2_config(&[]);
    }
}
