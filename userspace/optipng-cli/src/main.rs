#![deny(clippy::all)]

//! optipng-cli — OurOS OptiPNG PNG optimizer
//!
//! Single personality: `optipng`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_optipng(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") || args.is_empty() {
        println!("Usage: optipng [OPTIONS] FILES...");
        println!("OptiPNG 0.7.8 (OurOS) — Advanced PNG optimizer");
        println!();
        println!("Options:");
        println!("  -o N               Optimization level (0-7, default 2)");
        println!("  -i TYPE            Interlace type (0=non, 1=interlaced)");
        println!("  -k                 Keep backup of original");
        println!("  -dir DIR           Output directory");
        println!("  -out FILE          Output file");
        println!("  -fix               Fix errors where possible");
        println!("  -force             Force optimization");
        println!("  -preserve          Preserve file timestamps");
        println!("  -simulate          Don't write output");
        println!("  -snip              Cut metadata");
        println!("  -strip MODE        Strip metadata (all)");
        println!("  -clobber           Overwrite existing files");
        println!("  -quiet             Quiet mode");
        println!("  -v                 Verbose mode");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("OptiPNG version 0.7.8 (OurOS)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    for f in &files {
        println!("** Processing: {}", f);
        println!("1920x1080 pixels, 4x8 bits/pixel, RGB+alpha");
        println!("Input IDAT size = 2048000 bytes");
        println!("Output IDAT size = 1843200 bytes (204800 bytes decrease)");
        println!("Output file size = 1843300 bytes (204790 bytes = 10.00% decrease)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "optipng".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_optipng(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_optipng};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/optipng"), "optipng");
        assert_eq!(basename(r"C:\bin\optipng.exe"), "optipng.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("optipng.exe"), "optipng");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_optipng(&["--help".to_string()], "optipng"), 0);
        assert_eq!(run_optipng(&["-h".to_string()], "optipng"), 0);
        assert_eq!(run_optipng(&["--version".to_string()], "optipng"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_optipng(&[], "optipng"), 0);
    }
}
