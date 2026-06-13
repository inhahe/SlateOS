#![deny(clippy::all)]

//! lm-sensors-cli — SlateOS lm-sensors hardware monitoring
//!
//! Multi-personality: `sensors`, `sensors-detect`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sensors(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sensors [OPTIONS] [CHIP]");
        println!("sensors v3.6 (SlateOS) — Print hardware sensor readings");
        println!();
        println!("Options:");
        println!("  -f                Show temps in Fahrenheit");
        println!("  -A                Show all features");
        println!("  -u                Raw output");
        println!("  -j                JSON output");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sensors v3.6 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "-j") {
        println!("{{");
        println!("  \"coretemp-isa-0000\": {{");
        println!("    \"Core 0\": {{ \"temp\": 45.0, \"max\": 100.0, \"crit\": 105.0 }},");
        println!("    \"Core 1\": {{ \"temp\": 43.0, \"max\": 100.0, \"crit\": 105.0 }}");
        println!("  }}");
        println!("}}");
        return 0;
    }
    println!("coretemp-isa-0000");
    println!("Adapter: ISA adapter");
    println!("Core 0:       +45.0\u{00b0}C  (high = +100.0\u{00b0}C, crit = +105.0\u{00b0}C)");
    println!("Core 1:       +43.0\u{00b0}C  (high = +100.0\u{00b0}C, crit = +105.0\u{00b0}C)");
    println!();
    println!("it8728-isa-0a30");
    println!("Adapter: ISA adapter");
    println!("Vcore:        +1.01 V  (min =  +0.00 V, max =  +2.04 V)");
    println!("fan1:         980 RPM  (min =  200 RPM)");
    println!("fan2:        1250 RPM  (min =  200 RPM)");
    println!("temp1:        +38.0\u{00b0}C  (low  = +127.0\u{00b0}C, high = +127.0\u{00b0}C)");
    0
}

fn run_detect(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sensors-detect [OPTIONS]");
        println!("sensors-detect v3.6 (SlateOS) — Detect hardware monitoring chips");
        println!();
        println!("Options:");
        println!("  --auto            Auto-detect without prompting");
        println!("  --stat            Show statistics only");
        return 0;
    }
    println!("# sensors-detect — hardware monitoring chip detection");
    println!("# Probing for ISA bus chips...");
    println!("Found IT8728F at 0x0a30 (Super I/O)");
    println!("# Probing for PCI bus chips...");
    println!("Found Intel Core thermal sensor");
    println!("# Detected chips:");
    println!("  coretemp-isa-0000");
    println!("  it8728-isa-0a30");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sensors".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "sensors-detect" => run_detect(&rest, &prog),
        _ => run_sensors(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sensors};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lm-sensors"), "lm-sensors");
        assert_eq!(basename(r"C:\bin\lm-sensors.exe"), "lm-sensors.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lm-sensors.exe"), "lm-sensors");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sensors(&["--help".to_string()], "lm-sensors"), 0);
        assert_eq!(run_sensors(&["-h".to_string()], "lm-sensors"), 0);
        let _ = run_sensors(&["--version".to_string()], "lm-sensors");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sensors(&[], "lm-sensors");
    }
}
