#![deny(clippy::all)]

//! vivaldi-cli — SlateOS Vivaldi browser (power user)
//!
//! Single personality: `vivaldi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vivaldi [URL] [OPTIONS]");
        println!("Vivaldi (SlateOS) — Power-user Chromium browser with deep customization");
        println!();
        println!("Options:");
        println!("  --private              Private window");
        println!("  --tab-stack            Tab Stacks (grouped tabs)");
        println!("  --mail                 Vivaldi Mail (built-in IMAP/POP3 client)");
        println!("  --calendar             Vivaldi Calendar (CalDAV)");
        println!("  --feeds                Vivaldi Feed Reader (RSS/Atom)");
        println!("  --workspaces           Workspaces");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Vivaldi 7.0.3495.29 (Stable channel) (SlateOS, 64-bit)"); return 0; }
    println!("Vivaldi 7.0.3495.29 (SlateOS)");
    println!("  Vendor: Vivaldi Technologies (Oslo, Norway, founded 2014)");
    println!("  Founder: Jon Stephenson von Tetzchner (co-founder of Opera)");
    println!("  Origin: built by ex-Opera engineers after Presto-era Opera died (2013)");
    println!("  Engine: Blink + V8 (Chromium), UI in React/HTML/CSS (not native Chromium UI)");
    println!("  Philosophy: 'Browser made for our friends' — power users, no compromise UI");
    println!("  Customization: ~3000 settings, panels, themes, keyboard shortcuts, mouse gestures,");
    println!("                 command chains, tab stacking, tab tiling, web panels, status bar widgets");
    println!("  Built-in apps: Mail (IMAP/POP3/SMTP), Calendar (CalDAV), Feeds (RSS), Notes,");
    println!("                 Translate (Lingvanex backend, on-device option)");
    println!("  Sync: Vivaldi Sync (end-to-end encrypted, hosted in Iceland)");
    println!("  Privacy: no telemetry, no Google services, ad blocker built-in");
    println!("  Forum: integrated community, polling, blogs (Vivaldi.net Mastodon instance)");
    println!("  Platforms: Win/macOS/Linux/Android, Vivaldi Auto (Android Automotive)");
    println!("  Funding: profitable, no VC, owned by employees + Tetzchner");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vivaldi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vv(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vivaldi"), "vivaldi");
        assert_eq!(basename(r"C:\bin\vivaldi.exe"), "vivaldi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vivaldi.exe"), "vivaldi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vv(&["--help".to_string()], "vivaldi"), 0);
        assert_eq!(run_vv(&["-h".to_string()], "vivaldi"), 0);
        let _ = run_vv(&["--version".to_string()], "vivaldi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vv(&[], "vivaldi");
    }
}
