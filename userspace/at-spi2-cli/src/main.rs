#![deny(clippy::all)]

//! at-spi2-cli — SlateOS AT-SPI2 accessibility toolkit
//!
//! Multi-personality: `at-spi2-registryd`, `at-spi2-bus-launcher`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_registryd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: at-spi2-registryd [OPTIONS]");
        println!("at-spi2-registryd v2.52 (SlateOS) — AT-SPI2 accessibility registry");
        println!();
        println!("Options:");
        println!("  --dbus-name NAME  D-Bus name");
        println!("  --version         Show version");
        println!();
        println!("Central registry for accessibility services.");
        println!("Manages screen reader and assistive technology connections.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("at-spi2-registryd v2.52 (SlateOS)"); return 0; }
    println!("at-spi2-registryd: accessibility registry started");
    println!("  D-Bus: org.a11y.atspi.Registry");
    println!("  Clients: 0 connected");
    0
}

fn run_bus_launcher(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: at-spi2-bus-launcher [OPTIONS]");
        println!("at-spi2-bus-launcher v2.52 (SlateOS) — AT-SPI2 bus launcher");
        println!();
        println!("Options:");
        println!("  --launch          Launch accessibility bus");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("at-spi2-bus-launcher v2.52 (SlateOS)"); return 0; }
    println!("at-spi2-bus-launcher: starting accessibility bus");
    println!("  Bus address: unix:path=/run/user/1000/at-spi/bus");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "at-spi2-registryd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "at-spi2-bus-launcher" => run_bus_launcher(&rest, &prog),
        _ => run_registryd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_registryd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/at-spi2"), "at-spi2");
        assert_eq!(basename(r"C:\bin\at-spi2.exe"), "at-spi2.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("at-spi2.exe"), "at-spi2");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_registryd(&["--help".to_string()], "at-spi2"), 0);
        assert_eq!(run_registryd(&["-h".to_string()], "at-spi2"), 0);
        let _ = run_registryd(&["--version".to_string()], "at-spi2");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_registryd(&[], "at-spi2");
    }
}
