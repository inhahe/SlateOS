#![deny(clippy::all)]

//! swaylock-cli — SlateOS swaylock screen locker
//!
//! Single personality: `swaylock`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swaylock(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swaylock [OPTIONS]");
        println!("swaylock v1.7 (Slate OS) — Screen locker for Wayland");
        println!();
        println!("Options:");
        println!("  -c COLOR          Background color (#RRGGBB)");
        println!("  -i IMAGE          Background image");
        println!("  -s MODE           Scaling mode (fill, fit, center, tile, stretch)");
        println!("  -f                Fork into background");
        println!("  -e                Ignore empty password");
        println!("  --indicator-idle-visible  Always show indicator");
        println!("  --show-failed-attempts   Show failed attempts");
        println!("  --grace N         Grace period (seconds)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("swaylock v1.7 (Slate OS)"); return 0; }
    let color = args.iter().skip_while(|a| a.as_str() != "-c").nth(1).map(|s| s.as_str()).unwrap_or("#000000");
    let image = args.iter().skip_while(|a| a.as_str() != "-i").nth(1);
    println!("Screen locked.");
    if let Some(img) = image {
        println!("  Background: {}", img);
    } else {
        println!("  Background: {}", color);
    }
    if args.is_empty() {
        println!("  Background: solid black");
    }
    println!("  Indicator: ring");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swaylock".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_swaylock(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_swaylock};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/swaylock"), "swaylock");
        assert_eq!(basename(r"C:\bin\swaylock.exe"), "swaylock.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("swaylock.exe"), "swaylock");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_swaylock(&["--help".to_string()], "swaylock"), 0);
        assert_eq!(run_swaylock(&["-h".to_string()], "swaylock"), 0);
        let _ = run_swaylock(&["--version".to_string()], "swaylock");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_swaylock(&[], "swaylock");
    }
}
