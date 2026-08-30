#![deny(clippy::all)]

//! sublime-cli — Slate OS Sublime Text (Jon Skinner's classic fast editor)
//!
//! Single personality: `sublime`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_st(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sublime [OPTIONS]");
        println!("Sublime Text 4 (build 4180) (Slate OS) — Fast cross-platform text editor");
        println!();
        println!("Options:");
        println!("  --new                  New file");
        println!("  --command-palette      Command Palette (Ctrl/Cmd+Shift+P)");
        println!("  --goto-anything        Goto Anything (Ctrl/Cmd+P) — fuzzy file/symbol/line jump");
        println!("  --multi-cursor         Multiple selections (Ctrl/Cmd+D, Ctrl+Click)");
        println!("  --package-control      Package Control (community plugin manager)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sublime Text 4 Build 4180 (Slate OS)"); return 0; }
    println!("Sublime Text 4 Build 4180 (Slate OS)");
    println!("  Vendor: Sublime HQ Pty Ltd (Sydney, Australia)");
    println!("  Creator: Jon Skinner (ex-Google, solo dev until 2017)");
    println!("  History: 1.0 in 2008, 2.0 in 2013, 3.0 in 2017, ST4 stable Aug 2021");
    println!("  Pricing: $99 USD perpetual license, 3-year free updates");
    println!("          'Evaluation' indefinitely (unlimited free trial; just nag screen every ~30 saves)");
    println!("  Engine: C++ with custom GUI framework (NOT Electron — extremely fast)");
    println!("          ~80MB install, instant cold start (<100ms even on huge projects)");
    println!("  License: proprietary (paid), but extension framework Python-based + open");
    println!("  Plugin API: Python 3.3 + 3.8 (dual interpreter) — Package Control by wbond is the de facto registry");
    println!("  Killer features:");
    println!("    - Goto Anything (Ctrl/Cmd+P): fuzzy file finder, also @symbol, :line, #word, > command");
    println!("    - Multiple Cursors: Ctrl+D to add next match (popularized this UX)");
    println!("    - Command Palette: every action by name");
    println!("    - Minimap (right-side rendered overview of file)");
    println!("    - Split editing (multiple panes, configurable grid)");
    println!("    - Distraction-free mode (Shift+F11)");
    println!("    - Project-wide search (Ctrl+Shift+F) — much faster than VS Code for huge codebases");
    println!("  ST4 additions: native Apple Silicon, GPU rendering, multi-select tabs, context-aware autocomplete,");
    println!("                tab multi-select, hot exit, refreshed default theme (Adaptive)");
    println!("  Companions: Sublime Merge (Git GUI, $99) — by the same team, same shortcuts");
    println!("  Differentiator: still the fastest 'feel' of any modern editor; ST inspired VS Code's Quick Open");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sublime".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_st(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_st};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sublime"), "sublime");
        assert_eq!(basename(r"C:\bin\sublime.exe"), "sublime.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sublime.exe"), "sublime");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_st(&["--help".to_string()], "sublime"), 0);
        assert_eq!(run_st(&["-h".to_string()], "sublime"), 0);
        let _ = run_st(&["--version".to_string()], "sublime");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_st(&[], "sublime");
    }
}
