#![deny(clippy::all)]

//! minicom-cli — OurOS serial terminal emulator
//!
//! Multi-personality: `minicom`, `screen` (serial mode)

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_minicom(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: minicom [OPTIONS] [DEVICE]");
        println!();
        println!("minicom — serial communication program (OurOS).");
        println!();
        println!("Options:");
        println!("  -b BAUD         Set baud rate");
        println!("  -D DEVICE       Device to open");
        println!("  -o              Don't send init/reset strings");
        println!("  -s              Setup mode");
        println!("  -c on|off       Color mode");
        println!("  -S SCRIPT       Run script after setup");
        println!("  -C FILE         Capture file");
        println!("  -w              Line wrap");
        println!("  -H              Start with hex display");
        println!("  -8              8-bit mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("minicom version 2.8 (OurOS)");
        return 0;
    }

    let setup = args.iter().any(|a| a == "-s");
    let device = args.iter()
        .find(|a| a.starts_with("/dev/") || a.starts_with("COM"))
        .or_else(|| args.windows(2).find(|w| w[0] == "-D").map(|w| &w[1]))
        .map(|s| s.as_str())
        .unwrap_or("/dev/ttyUSB0");

    let baud = args.windows(2)
        .find(|w| w[0] == "-b")
        .and_then(|w| w[1].parse::<u32>().ok())
        .unwrap_or(115200);

    if setup {
        println!("Minicom Setup:");
        println!("  Serial Device     : {}", device);
        println!("  Baud Rate         : {}", baud);
        println!("  Data Bits         : 8");
        println!("  Parity            : None");
        println!("  Stop Bits         : 1");
        println!("  Hardware Flow Ctrl: No");
        println!("  Software Flow Ctrl: No");
        return 0;
    }

    println!("Welcome to minicom 2.8 (OurOS)");
    println!();
    println!("OPTIONS: I18n");
    println!("Port {device}, {baud} 8N1");
    println!();
    println!("Press CTRL-A Z for help on special keys");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "minicom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_minicom(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_minicom};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/minicom"), "minicom");
        assert_eq!(basename(r"C:\bin\minicom.exe"), "minicom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("minicom.exe"), "minicom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_minicom(&["--help".to_string()]), 0);
        assert_eq!(run_minicom(&["-h".to_string()]), 0);
        let _ = run_minicom(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_minicom(&[]);
    }
}
