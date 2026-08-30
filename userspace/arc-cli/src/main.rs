#![deny(clippy::all)]

//! arc-cli — Slate OS The Browser Company's Arc browser
//!
//! Single personality: `arc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_arc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: arc [URL] [OPTIONS]");
        println!("Arc (Slate OS) — The Browser Company's reimagined-UI browser");
        println!();
        println!("Options:");
        println!("  --space NAME           Switch Space (workspace, set of tabs+pinned)");
        println!("  --little-arc           Little Arc (pop-out single-tab window)");
        println!("  --easels               Easels (browser-canvas mixed-media)");
        println!("  --boost                Boost (per-site CSS/JS overrides)");
        println!("  --max                  Arc Max (AI features: instant links, summarize, rename)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Arc 1.78.0 (62012) (Slate OS)"); return 0; }
    println!("Arc 1.78.0 (62012) (Slate OS)");
    println!("  Vendor: The Browser Company of New York (founded 2019)");
    println!("  Founder: Josh Miller (ex-White House), Hursh Agrawal");
    println!("  Engine: Blink + V8 (Chromium fork), native Mac (Swift) and Win (C++/Swift) shells");
    println!("  Concept: 'reinvent the browser' — sidebar tabs, Spaces, vertical layout default");
    println!("  Features: Spaces (theme + tabs grouping), Pinned tabs, Today's tabs auto-archive,");
    println!("            Little Arc (pop-out), Split View, Picture-in-Picture by default,");
    println!("            Easels (mixed-media canvas blending web + drawings)");
    println!("  Arc Max: AI quick-summarize tabs, instant link previews, auto-rename downloads,");
    println!("           ChatGPT (cmd-T then ask), ask-on-page-help");
    println!("  Arc Search (iOS/Android): 'browse for me' agent — replaces search results with");
    println!("                            AI-generated page summarizing the answer");
    println!("  Pivot: 2024 Browser Company announced shifting focus to 'Dia' (AI-first browser),");
    println!("         Arc enters maintenance mode (stable but no new features)");
    println!("  Distribution: free, waitlist removed 2023");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "arc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_arc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_arc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/arc"), "arc");
        assert_eq!(basename(r"C:\bin\arc.exe"), "arc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("arc.exe"), "arc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_arc(&["--help".to_string()], "arc"), 0);
        assert_eq!(run_arc(&["-h".to_string()], "arc"), 0);
        let _ = run_arc(&["--version".to_string()], "arc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_arc(&[], "arc");
    }
}
