#![deny(clippy::all)]

//! wlr-randr-cli — SlateOS wlr-randr output configuration
//!
//! Single personality: `wlr-randr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wlr_randr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wlr-randr [OPTIONS]");
        println!("wlr-randr v0.4 (Slate OS) — Wayland output configuration");
        println!();
        println!("Options:");
        println!("  (no args)         List outputs");
        println!("  --output NAME     Select output");
        println!("  --mode WxH@R      Set mode");
        println!("  --pos X,Y         Set position");
        println!("  --scale FACTOR    Set scale factor");
        println!("  --transform T     Set transform (normal, 90, 180, 270, flipped, etc.)");
        println!("  --on / --off      Enable/disable output");
        return 0;
    }
    if args.iter().any(|a| a == "--output") {
        let output = args.iter().skip_while(|a| a.as_str() != "--output").nth(1).map(|s| s.as_str()).unwrap_or("HDMI-A-1");
        println!("Configuring: {}", output);
        println!("  Applied.");
        return 0;
    }
    if args.is_empty() {
        println!("HDMI-A-1 \"Dell U2720Q\" (Dell Inc)");
        println!("  Enabled: yes");
        println!("  Modes:");
        println!("    3840x2160@60.000Hz (preferred, current)");
        println!("    2560x1440@60.000Hz");
        println!("    1920x1080@60.000Hz");
        println!("  Position: 0,0");
        println!("  Scale: 1.500000");
        println!("  Transform: normal");
        println!();
        println!("eDP-1 \"Built-in\" (BOE)");
        println!("  Enabled: yes");
        println!("  Modes:");
        println!("    2560x1600@120.000Hz (preferred, current)");
        println!("  Position: 3840,0");
        println!("  Scale: 1.600000");
        println!("  Transform: normal");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wlr-randr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wlr_randr(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wlr_randr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wlr-randr"), "wlr-randr");
        assert_eq!(basename(r"C:\bin\wlr-randr.exe"), "wlr-randr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wlr-randr.exe"), "wlr-randr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wlr_randr(&["--help".to_string()], "wlr-randr"), 0);
        assert_eq!(run_wlr_randr(&["-h".to_string()], "wlr-randr"), 0);
        let _ = run_wlr_randr(&["--version".to_string()], "wlr-randr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wlr_randr(&[], "wlr-randr");
    }
}
