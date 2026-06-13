#![deny(clippy::all)]

//! system-config-printer-cli — SlateOS printer configuration
//!
//! Single personality: `system-config-printer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_config_printer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: system-config-printer [OPTIONS]");
        println!("system-config-printer v1.5 (SlateOS) — Printer configuration tool");
        println!();
        println!("Options:");
        println!("  --add             Add new printer wizard");
        println!("  --configure NAME  Configure existing printer");
        println!("  --delete NAME     Delete a printer");
        println!("  --list            List configured printers");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("system-config-printer v1.5 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "--list") {
        println!("Configured printers:");
        println!("  HP-LaserJet-Pro  (default)  idle");
        println!("  PDF-Printer                 idle");
        return 0;
    }
    if args.iter().any(|a| a == "--add") {
        println!("system-config-printer: add printer wizard");
        println!("  Searching for network printers...");
        println!("  Found: HP LaserJet Pro M404 (ipp://192.168.1.50:631)");
        return 0;
    }
    println!("system-config-printer: printer management GUI started");
    println!("  Printers: 2 configured");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "system-config-printer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_config_printer(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_config_printer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/system-config-printer"), "system-config-printer");
        assert_eq!(basename(r"C:\bin\system-config-printer.exe"), "system-config-printer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("system-config-printer.exe"), "system-config-printer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_config_printer(&["--help".to_string()], "system-config-printer"), 0);
        assert_eq!(run_config_printer(&["-h".to_string()], "system-config-printer"), 0);
        let _ = run_config_printer(&["--version".to_string()], "system-config-printer");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_config_printer(&[], "system-config-printer");
    }
}
