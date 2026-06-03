#![deny(clippy::all)]

//! firefox-cli — OurOS Mozilla Firefox browser
//!
//! Single personality: `firefox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ff(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: firefox [URL] [OPTIONS]");
        println!("Mozilla Firefox (OurOS) — Open-source browser with Gecko engine");
        println!();
        println!("Options:");
        println!("  --private-window       Open Private Browsing window");
        println!("  --new-window           New window");
        println!("  --safe-mode            Disable add-ons + themes");
        println!("  --esr                  Firefox ESR (Extended Support Release)");
        println!("  --dev-edition          Firefox Developer Edition");
        println!("  --nightly              Firefox Nightly");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Mozilla Firefox 133.0 (OurOS)"); return 0; }
    println!("Mozilla Firefox 133.0 (OurOS)");
    println!("  Vendor: Mozilla Corporation (subsidiary of Mozilla Foundation 501(c)(3))");
    println!("  Engine: Gecko (HTML/CSS), SpiderMonkey (JS), Servo components (Stylo, WebRender)");
    println!("  Lineage: Netscape Navigator → Mozilla Suite → Phoenix → Firebird → Firefox (2004)");
    println!("  Channels: Release (4 wk), Beta, Developer Edition, Nightly, ESR (yearly)");
    println!("  Rust adoption: Gecko increasingly Rust (Stylo CSS engine, WebRender compositor)");
    println!("  Manifest V3: Firefox keeps MV2 support indefinitely (uBlock Origin full power)");
    println!("  Privacy: Total Cookie Protection, Enhanced Tracking Protection, no telemetry default-off (some)");
    println!("  Containers: Multi-Account Containers, Facebook Container, Temporary Containers");
    println!("  Sync: Firefox Sync (end-to-end encrypted)");
    println!("  Add-ons: AMO (addons.mozilla.org), reviewed extensions");
    println!("  Funding: ~85% from Google (default search deal) — controversial dependency");
    println!("  Forks: LibreWolf, Waterfox, Pale Moon (Goanna fork), IceCat, Tor Browser");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "firefox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ff(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ff};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/firefox"), "firefox");
        assert_eq!(basename(r"C:\bin\firefox.exe"), "firefox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("firefox.exe"), "firefox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ff(&["--help".to_string()], "firefox"), 0);
        assert_eq!(run_ff(&["-h".to_string()], "firefox"), 0);
        assert_eq!(run_ff(&["--version".to_string()], "firefox"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ff(&[], "firefox"), 0);
    }
}
