#![deny(clippy::all)]

//! chrome-cli — OurOS Google Chrome browser
//!
//! Single personality: `chrome`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_chr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chrome [URL] [OPTIONS]");
        println!("Google Chrome (OurOS) — Chromium-based browser");
        println!();
        println!("Options:");
        println!("  --incognito            Incognito (private browsing) window");
        println!("  --new-window           New window");
        println!("  --headless             Headless mode (for automation)");
        println!("  --enterprise           Chrome Enterprise (Chrome Browser Cloud Management)");
        println!("  --canary               Chrome Canary (daily channel)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Google Chrome 131.0.6778.86 (OurOS, 64-bit)"); return 0; }
    println!("Google Chrome 131.0.6778.86 (Official Build) (OurOS)");
    println!("  Vendor: Google LLC (Mountain View, California)");
    println!("  Engine: Blink (forked from WebKit Apr 2013), V8 JavaScript engine");
    println!("  Launched: Sep 2008 (Windows first), now Win/macOS/Linux/Android/iOS/ChromeOS");
    println!("  Market share: ~65% global browser market (StatCounter, Nov 2024)");
    println!("  Channels: Stable (6 wk), Beta, Dev, Canary, Extended Stable (enterprise)");
    println!("  Sync: bookmarks, history, passwords, tabs, autofill across signed-in devices");
    println!("  Profiles: multiple profiles per OS user, work/personal separation");
    println!("  Extensions: Chrome Web Store, Manifest V3 (Jun 2024 sunset of MV2)");
    println!("  Security: Site Isolation, sandboxed renderers, Safe Browsing, HSTS preload list");
    println!("  Enterprise: Chrome Browser Cloud Management, 1000+ policies via GPO/MDM");
    println!("  Sandboxing: per-tab process isolation, GPU process, network service");
    println!("  Sister: Chromium (open source upstream, no Google-only features/codecs)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chrome".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_chr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_chr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/chrome"), "chrome");
        assert_eq!(basename(r"C:\bin\chrome.exe"), "chrome.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("chrome.exe"), "chrome");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_chr(&["--help".to_string()], "chrome"), 0);
        assert_eq!(run_chr(&["-h".to_string()], "chrome"), 0);
        let _ = run_chr(&["--version".to_string()], "chrome");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_chr(&[], "chrome");
    }
}
