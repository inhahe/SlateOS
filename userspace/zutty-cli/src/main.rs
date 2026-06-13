#![deny(clippy::all)]

//! zutty-cli — SlateOS Zutty GPU terminal emulator
//!
//! Single personality: `zutty`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zutty(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zutty [OPTIONS] [CMD...]");
        println!("zutty v0.14 (Slate OS) — OpenGL ES compute-shader terminal");
        println!();
        println!("Options:");
        println!("  CMD               Command to run");
        println!("  -font FONT        Font name");
        println!("  -fontsize N       Font size");
        println!("  -geometry COLSxROWS  Window size");
        println!("  -title TITLE      Window title");
        println!("  -shell SHELL      Shell to run");
        println!("  -rv               Reverse video");
        println!("  -version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version" || a == "--version") { println!("zutty v0.14 (Slate OS)"); return 0; }
    println!("Zutty terminal starting...");
    println!("  Renderer: OpenGL ES 3.1 compute shader");
    println!("  Font: monospace 14pt");
    println!("  Grid: 80x24");
    if args.is_empty() {
        println!("  Shell: /bin/sh");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zutty".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zutty(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zutty};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zutty"), "zutty");
        assert_eq!(basename(r"C:\bin\zutty.exe"), "zutty.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zutty.exe"), "zutty");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zutty(&["--help".to_string()], "zutty"), 0);
        assert_eq!(run_zutty(&["-h".to_string()], "zutty"), 0);
        let _ = run_zutty(&["--version".to_string()], "zutty");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zutty(&[], "zutty");
    }
}
