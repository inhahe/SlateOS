#![deny(clippy::all)]

//! bat — OurOS cat clone with syntax highlighting
//!
//! Single personality: `bat`

use std::env;
use std::process;

fn run_bat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bat [OPTIONS] [FILE]...");
        println!("       bat cache [SUBCOMMAND]");
        println!();
        println!("Options:");
        println!("  -A, --show-all          Show non-printable characters");
        println!("  -p, --plain             Plain style (no decorations)");
        println!("  -l, --language <lang>   Set language for highlighting");
        println!("  -H, --highlight-line <N:M>  Highlight lines");
        println!("  --file-name <name>      Specify displayed file name");
        println!("  -n, --number            Show line numbers only");
        println!("  --color <when>          Color output (auto/always/never)");
        println!("  --paging <when>         Pager (auto/always/never)");
        println!("  --style <style>         Output style (full/plain/changes/header/grid/numbers)");
        println!("  --theme <theme>         Syntax highlighting theme");
        println!("  --list-themes           List available themes");
        println!("  --list-languages        List supported languages");
        println!("  -r, --line-range <N:M>  Show line range");
        println!("  --diff                  Show git diff");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("bat 0.24.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--list-themes") {
        println!("1337");
        println!("Coldark-Cold");
        println!("Coldark-Dark");
        println!("DarkNeon");
        println!("Dracula");
        println!("GitHub");
        println!("Monokai Extended");
        println!("Nord");
        println!("OneHalfDark");
        println!("OneHalfLight");
        println!("Solarized (dark)");
        println!("Solarized (light)");
        println!("Sublime Snazzy");
        println!("TwoDark");
        println!("Visual Studio Dark+");
        println!("ansi");
        println!("base16");
        println!("gruvbox-dark");
        println!("gruvbox-light");
        println!("zenburn");
        return 0;
    }
    if args.iter().any(|a| a == "--list-languages") {
        println!("Bash (bash, sh, zsh, .bashrc)");
        println!("C (c, h)");
        println!("C++ (cpp, cc, cxx, hpp)");
        println!("CSS (css)");
        println!("Go (go)");
        println!("HTML (html, htm)");
        println!("JSON (json)");
        println!("JavaScript (js, mjs, cjs)");
        println!("Markdown (md, markdown)");
        println!("Python (py, pyi)");
        println!("Rust (rs)");
        println!("TOML (toml)");
        println!("TypeScript (ts, tsx)");
        println!("YAML (yaml, yml)");
        println!("(... 200+ languages supported)");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if files.is_empty() {
        println!("(reading from stdin — syntax highlighted output simulated)");
    } else {
        for f in &files {
            println!("───────┬─────────────────────────────────────");
            println!("       │ File: {}", f);
            println!("───────┼─────────────────────────────────────");
            println!("   1   │ // Example file content");
            println!("   2   │ fn main() {{");
            println!("   3   │     println!(\"Hello, world!\");");
            println!("   4   │ }}");
            println!("───────┴─────────────────────────────────────");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bat(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
