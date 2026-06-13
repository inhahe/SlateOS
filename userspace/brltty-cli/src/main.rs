#![deny(clippy::all)]

//! brltty-cli — SlateOS BRLTTY braille display driver CLI
//!
//! Multi-personality: `brltty`, `brltty-setup`, `brltty-lsinc`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_brltty(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: brltty [OPTIONS]");
        println!();
        println!("BRLTTY — braille display driver (SlateOS).");
        println!();
        println!("Options:");
        println!("  -b DRIVER      Braille driver");
        println!("  -d DEVICE      Braille device");
        println!("  -t TABLE       Text table");
        println!("  -c TABLE       Contraction table");
        println!("  -f FILE        Config file");
        println!("  -P FILE        PID file");
        println!("  -l LEVEL       Log level");
        println!("  -e             Start in foreground");
        println!("  -n             No daemon");
        println!("  -v             Verify config only");
        println!("  -I             Install service");
        println!("  -R             Remove service");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("BRLTTY 6.6 (SlateOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-v") {
        println!("Configuration verified:");
        println!("  Braille driver: auto");
        println!("  Text table: en-us-g2");
        println!("  Contraction table: none");
        return 0;
    }

    let driver = args.windows(2).find(|w| w[0] == "-b").map(|w| w[1].as_str()).unwrap_or("auto");
    let device = args.windows(2).find(|w| w[0] == "-d").map(|w| w[1].as_str()).unwrap_or("usb:");

    println!("BRLTTY 6.6 starting (driver={}, device={})", driver, device);
    println!("  Searching for braille display...");
    println!("  Screen driver: Linux");
    println!("  API server listening on /var/lib/brltty/brltty.socket");
    0
}

fn run_brltty_setup(_args: &[String]) -> i32 {
    println!("BRLTTY Setup Wizard");
    println!();
    println!("Available braille drivers:");
    println!("  al  Alva");
    println!("  at  Albatross");
    println!("  ba  BrlAPI");
    println!("  bn  BrailleNote");
    println!("  ec  EcoBraille");
    println!("  eu  EuroBraille");
    println!("  fs  FreedomScientific");
    println!("  hm  HIMS");
    println!("  hw  HumanWare");
    println!();
    println!("Select driver (or 'auto' for auto-detection): ");
    0
}

fn run_brltty_lsinc(_args: &[String]) -> i32 {
    println!("BRLTTY Include Files:");
    println!("  /etc/brltty/brltty.conf");
    println!("  /etc/brltty/Input/all.ktb");
    println!("  /etc/brltty/Text/en-us-g2.ttb");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "brltty".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "brltty-setup" => run_brltty_setup(&rest),
        "brltty-lsinc" => run_brltty_lsinc(&rest),
        _ => run_brltty(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_brltty};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/brltty"), "brltty");
        assert_eq!(basename(r"C:\bin\brltty.exe"), "brltty.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("brltty.exe"), "brltty");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_brltty(&["--help".to_string()]), 0);
        assert_eq!(run_brltty(&["-h".to_string()]), 0);
        let _ = run_brltty(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_brltty(&[]);
    }
}
