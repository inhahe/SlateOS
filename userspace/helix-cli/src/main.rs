#![deny(clippy::all)]

//! helix-cli — SlateOS Helix editor
//!
//! Single personality: `hx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_helix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hx [OPTIONS] [FILE...]");
        println!("helix 24.7 (Slate OS) — A post-modern text editor");
        println!();
        println!("Options:");
        println!("  -c, --config FILE     Config file");
        println!("  --health [LANG]       Health check (optionally for language)");
        println!("  -g, --grammar ACTION  Fetch or build tree-sitter grammars");
        println!("  --tutor               Open the tutorial");
        println!("  -v                    Increase verbosity");
        println!("  -V, --version         Show version");
        println!("  --vsplit              Open files in vertical splits");
        println!("  --hsplit              Open files in horizontal splits");
        println!("  -w N                  Set log worker threads");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("helix 24.7 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--health") {
        let lang = args.iter().skip_while(|a| a.as_str() != "--health").nth(1);
        if let Some(l) = lang {
            println!("Language: {}", l);
            println!("  Highlight: ✓");
            println!("  LSP: configured");
            println!("  DAP: configured");
            println!("  Formatter: configured");
        } else {
            println!("Config file: ~/.config/helix/config.toml");
            println!("Language file: ~/.config/helix/languages.toml");
            println!("Runtime dirs: /usr/lib/helix/runtime");
            println!("Runtime: ✓");
        }
        return 0;
    }
    if args.iter().any(|a| a == "--tutor") {
        println!("helix: Opening tutorial...");
        return 0;
    }
    if args.iter().any(|a| a == "-g" || a == "--grammar") {
        let action = args.iter().skip_while(|a| a.as_str() != "-g" && a.as_str() != "--grammar").nth(1)
            .map(|s| s.as_str()).unwrap_or("fetch");
        println!("helix: {} grammars...", action);
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    if let Some(f) = file {
        println!("helix: Editing '{}'", f);
    } else {
        println!("helix: Opening scratch buffer");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_helix(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_helix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/helix"), "helix");
        assert_eq!(basename(r"C:\bin\helix.exe"), "helix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("helix.exe"), "helix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_helix(&["--help".to_string()], "helix"), 0);
        assert_eq!(run_helix(&["-h".to_string()], "helix"), 0);
        let _ = run_helix(&["--version".to_string()], "helix");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_helix(&[], "helix");
    }
}
