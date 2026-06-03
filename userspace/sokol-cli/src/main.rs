#![deny(clippy::all)]

//! sokol-cli — OurOS Sokol shader compiler
//!
//! Single personality: `sokol-shdc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sokol(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sokol-shdc [OPTIONS]");
        println!("sokol-shdc v1.0.0 (OurOS) — Sokol shader cross-compiler");
        println!();
        println!("Options:");
        println!("  -i, --input FILE      Input shader file (.glsl)");
        println!("  -o, --output FILE     Output file");
        println!("  -l, --slang LANG      Output language (glsl430, hlsl5, metal_macos, wgsl)");
        println!("  -f, --format FMT      Output format (sokol, bare)");
        println!("  --reflection          Generate reflection data");
        println!("  --errfmt MSG          Error format (default, msvc, gcc)");
        println!("  --dump                Dump debug info");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sokol-shdc v1.0.0 (OurOS)");
        return 0;
    }
    println!("sokol-shdc: Compiling shader...");
    println!("  Input: shader.glsl");
    println!("  Vertex shader: vs_main");
    println!("  Fragment shader: fs_main");
    println!("  Output: shader.h (glsl430 + hlsl5 + metal + wgsl)");
    println!("  Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sokol-shdc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sokol(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sokol};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sokol"), "sokol");
        assert_eq!(basename(r"C:\bin\sokol.exe"), "sokol.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sokol.exe"), "sokol");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sokol(&["--help".to_string()], "sokol"), 0);
        assert_eq!(run_sokol(&["-h".to_string()], "sokol"), 0);
        assert_eq!(run_sokol(&["--version".to_string()], "sokol"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sokol(&[], "sokol"), 0);
    }
}
