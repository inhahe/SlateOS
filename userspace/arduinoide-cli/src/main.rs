#![deny(clippy::all)]

//! arduinoide-cli — OurOS Arduino IDE 2.x
//!
//! Single personality: `arduinoide`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_aide(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: arduinoide [OPTIONS] [SKETCH]");
        println!("Arduino IDE 2.3 (OurOS) — Open-source microcontroller IDE");
        println!();
        println!("Options:");
        println!("  --sketch FILE          Open .ino sketch");
        println!("  --compile              Compile only");
        println!("  --upload               Compile and upload");
        println!("  --board FQBN           Fully Qualified Board Name");
        println!("  --port PORT            Serial port");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Arduino IDE 2.3.3 (OurOS)"); return 0; }
    println!("Arduino IDE 2.3.3 (OurOS)");
    println!("  Architecture: Electron + Theia + arduino-cli backend");
    println!("  Languages: Arduino C/C++ (sketches), .ino files");
    println!("  Boards: AVR (Uno/Mega/Nano), SAMD (Zero/MKR), ESP32/ESP8266, RP2040,");
    println!("          STM32, Nordic nRF52, Renesas RA, Teensy");
    println!("  Library Manager: 5000+ community libraries, semver versioning");
    println!("  Boards Manager: 3rd-party board packages (ESP32, Adafruit, Pico, etc.)");
    println!("  Debug: GDB integration (boards with hardware debug support)");
    println!("  Companion: arduinoide-cli (CLI), Arduino Cloud, Arduino Web Editor");
    println!("  License: AGPL v3 (Free / Open Source)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "arduinoide".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_aide(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_aide};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/arduinoide"), "arduinoide");
        assert_eq!(basename(r"C:\bin\arduinoide.exe"), "arduinoide.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("arduinoide.exe"), "arduinoide");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_aide(&["--help".to_string()], "arduinoide"), 0);
        assert_eq!(run_aide(&["-h".to_string()], "arduinoide"), 0);
        assert_eq!(run_aide(&["--version".to_string()], "arduinoide"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_aide(&[], "arduinoide"), 0);
    }
}
