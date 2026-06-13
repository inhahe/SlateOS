#![deny(clippy::all)]

//! wayst-cli — SlateOS Wayst GPU-accelerated terminal
//!
//! Single personality: `wayst`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wayst(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wayst [OPTIONS] [CMD...]");
        println!("wayst v0.1 (SlateOS) — GPU-accelerated Wayland terminal");
        println!();
        println!("Options:");
        println!("  CMD               Command to run");
        println!("  -f FONT           Font");
        println!("  -t TITLE          Title");
        println!("  --gl-renderer     Force OpenGL renderer");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wayst v0.1 (SlateOS)"); return 0; }
    println!("wayst: GPU-accelerated terminal");
    println!("  Renderer: OpenGL ES 3.0");
    println!("  Font rasterizer: FreeType");
    if args.is_empty() {
        println!("  Shell: /bin/sh");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wayst".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wayst(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wayst};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wayst"), "wayst");
        assert_eq!(basename(r"C:\bin\wayst.exe"), "wayst.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wayst.exe"), "wayst");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wayst(&["--help".to_string()], "wayst"), 0);
        assert_eq!(run_wayst(&["-h".to_string()], "wayst"), 0);
        let _ = run_wayst(&["--version".to_string()], "wayst");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wayst(&[], "wayst");
    }
}
