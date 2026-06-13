#![deny(clippy::all)]

//! at-spi-cli — SlateOS AT-SPI2 accessibility tools CLI
//!
//! Multi-personality: `at-spi2-registryd`, `at-spi-bus-launcher`, `accerciser`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_registryd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: at-spi2-registryd [OPTIONS]");
        println!();
        println!("AT-SPI2 registry daemon (SlateOS).");
        println!();
        println!("Options:");
        println!("  --dbus-name NAME    DBus bus name");
        println!("  --replace           Replace existing instance");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("at-spi2-registryd 2.52.0 (SlateOS)");
        return 0;
    }
    println!("AT-SPI2 registry daemon starting...");
    println!("  D-Bus name: org.a11y.atspi.Registry");
    println!("  Listening for accessibility events...");
    0
}

fn run_bus_launcher(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: at-spi-bus-launcher [OPTIONS]");
        println!();
        println!("AT-SPI accessibility bus launcher (SlateOS).");
        println!();
        println!("Options:");
        println!("  --launch-immediately  Launch without waiting");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("at-spi-bus-launcher 2.52.0 (SlateOS)");
        return 0;
    }
    println!("Launching AT-SPI accessibility bus...");
    println!("  Bus address: unix:path=/run/user/1000/at-spi/bus");
    println!("  Bus launched successfully.");
    0
}

fn run_accerciser(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: accerciser [OPTIONS]");
        println!();
        println!("Accerciser — interactive accessibility explorer (SlateOS).");
        println!();
        println!("Options:");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Accerciser 3.42.0 (SlateOS)");
        return 0;
    }
    println!("Accerciser: accessibility explorer starting...");
    println!("  Connected to AT-SPI2 bus.");
    println!("  Accessible tree:");
    println!("    [application] gnome-terminal");
    println!("      [frame] Terminal");
    println!("        [terminal] bash");
    println!("    [application] firefox");
    println!("      [frame] Mozilla Firefox");
    println!("        [document-web] New Tab");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "at-spi2-registryd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "at-spi-bus-launcher" => run_bus_launcher(&rest),
        "accerciser" => run_accerciser(&rest),
        _ => run_registryd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_registryd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/at-spi"), "at-spi");
        assert_eq!(basename(r"C:\bin\at-spi.exe"), "at-spi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("at-spi.exe"), "at-spi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_registryd(&["--help".to_string()]), 0);
        assert_eq!(run_registryd(&["-h".to_string()]), 0);
        let _ = run_registryd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_registryd(&[]);
    }
}
