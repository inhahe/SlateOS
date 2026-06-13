#![deny(clippy::all)]

//! notepadpp-cli — SlateOS Notepad++ (Don Ho's Windows-only free editor)
//!
//! Single personality: `notepadpp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_npp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: notepadpp [OPTIONS]");
        println!("Notepad++ 8.7.5 (Slate OS) — Free Windows source code editor");
        println!();
        println!("Options:");
        println!("  --new                  New tab");
        println!("  --compare-plugin       Compare plugin (side-by-side diff)");
        println!("  --regex                Regex search + replace (PCRE)");
        println!("  --column-mode          Alt+drag — column selection editing");
        println!("  --hex-editor           HEX-Editor plugin (binary view)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Notepad++ v8.7.5 (Slate OS)"); return 0; }
    println!("Notepad++ v8.7.5 (Slate OS)");
    println!("  Author: Don Ho (Vietnam → France-based developer)");
    println!("  License: GPL v3 (free + open source)");
    println!("  First release: Nov 2003");
    println!("  Platform: Windows-only (Win32 / Win64 — Wine-friendly on Linux via Bottles/Lutris)");
    println!("  Pricing: FREE — donation-supported, distributed via notepad-plus-plus.org");
    println!("  Engine: C++ with the Scintilla editing component (also used by SciTE, Geany, Code::Blocks)");
    println!("          tiny installer (~5 MB), instant launch");
    println!("  Features:");
    println!("    - Syntax highlighting for ~80 languages out of the box");
    println!("    - Tabbed multi-document interface");
    println!("    - Code folding, bracket matching, auto-indent, smart highlighting");
    println!("    - Multi-line regex search/replace (PCRE), search in files, mark all instances");
    println!("    - Column-mode editing (block selection)");
    println!("    - Multiple views (split horizontal/vertical, clone document)");
    println!("    - Macro recording + playback");
    println!("    - Document Map (minimap)");
    println!("    - File compare via Compare plugin (de facto standard for quick file diffs)");
    println!("  Plugin Admin: built-in plugin manager — install Compare, NppExec, XML Tools, JSON Viewer, etc.");
    println!("  Politics: Don Ho is famous for issuing politically-themed releases (Tiananmen, Stand with Ukraine,");
    println!("           Stand with Hong Kong, Free Uyghur) — release names + about-box messaging");
    println!("  Usage: still THE default editor for many Windows sysadmins / lightweight scripting tasks");
    println!("  Differentiator: FREE, FAST, Windows-native (no Electron), strong regex/multi-cursor, donations not subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "notepadpp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_npp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_npp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/notepadpp"), "notepadpp");
        assert_eq!(basename(r"C:\bin\notepadpp.exe"), "notepadpp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("notepadpp.exe"), "notepadpp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_npp(&["--help".to_string()], "notepadpp"), 0);
        assert_eq!(run_npp(&["-h".to_string()], "notepadpp"), 0);
        let _ = run_npp(&["--version".to_string()], "notepadpp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_npp(&[], "notepadpp");
    }
}
