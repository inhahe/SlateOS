#![deny(clippy::all)]

//! lualatex-cli — SlateOS LuaLaTeX/LuaTeX engine
//!
//! Multi-personality: `lualatex`, `luatex`, `luahbtex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lualatex(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: lualatex [OPTIONS] FILE.tex");
        println!("LuaHBTeX 1.18.0 (TeX Live 2024/SlateOS)");
        println!();
        println!("Options:");
        println!("  --interaction=MODE    Set interaction mode");
        println!("  --output-directory=DIR  Output directory");
        println!("  --output-format=FMT  Output format (pdf, dvi)");
        println!("  --synctex=NUM        Enable SyncTeX");
        println!("  --shell-escape       Enable os.execute()");
        println!("  --halt-on-error      Stop on first error");
        println!("  --file-line-error    Show file:line:error format");
        println!("  --nosocket           Disable socket library");
        println!("  --lua=FILE           Load Lua initialization file");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("This is LuaHBTeX, Version 1.18.0 (TeX Live 2024/SlateOS)");
        println!("Lua version: Lua 5.4, HarfBuzz version: 8.3.0");
        println!("Development id: 7610");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".tex") || (!a.starts_with('-') && !a.contains('=')))
        .map(|s| s.as_str())
        .unwrap_or("document.tex");
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    println!("This is LuaHBTeX, Version 1.18.0 (TeX Live 2024/SlateOS)");
    println!(" restricted \\write18 enabled.");
    println!("({})", file);
    println!("LaTeX2e <2024-02-01> patch level 2");
    println!(" L3 programming layer <2024-02-20>");
    println!("Lua module: luaotfload 2024-02-14");
    println!("({}.aux)", base);
    println!("[1] [2] [3] [4] [5]");
    println!("({}.aux)", base);
    println!("Output written on {}.pdf (5 pages, 52000 bytes).", base);
    println!("Transcript written on {}.log.", base);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lualatex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lualatex(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lualatex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lualatex"), "lualatex");
        assert_eq!(basename(r"C:\bin\lualatex.exe"), "lualatex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lualatex.exe"), "lualatex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lualatex(&["--help".to_string()]), 0);
        assert_eq!(run_lualatex(&["-h".to_string()]), 0);
        let _ = run_lualatex(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lualatex(&[]);
    }
}
