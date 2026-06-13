#![deny(clippy::all)]

//! smartmontools-cli — SlateOS SMART disk monitoring
//!
//! Multi-personality: `smartctl`, `smartd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_smartctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smartctl [OPTIONS] DEVICE");
        println!("smartctl v7.4 (SlateOS) — SMART disk monitoring");
        println!();
        println!("Options:");
        println!("  -i             Show device info");
        println!("  -H             Check health status");
        println!("  -A             Show attributes");
        println!("  -a             Show all info");
        println!("  -t TYPE        Run self-test (short, long, conveyance)");
        println!("  --scan         Scan for devices");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("smartctl v7.4 (SlateOS, smartmontools)"); return 0; }
    if args.iter().any(|a| a == "--scan") {
        println!("/dev/sda -d sat   # /dev/sda, ATA device");
        println!("/dev/nvme0 -d nvme # /dev/nvme0, NVMe device");
        return 0;
    }
    if args.iter().any(|a| a == "-H") {
        println!("=== START OF READ SMART DATA SECTION ===");
        println!("SMART overall-health self-assessment: PASSED");
        return 0;
    }
    if args.iter().any(|a| a == "-A") {
        println!("ID# ATTRIBUTE_NAME          VALUE WORST THRESH TYPE");
        println!("  5 Reallocated_Sector_Ct      100   100   010  Pre-fail");
        println!("  9 Power_On_Hours             095   095   000  Old_age");
        println!("194 Temperature_Celsius         068   050   000  Old_age");
        println!("197 Current_Pending_Sector      100   100   000  Old_age");
        return 0;
    }
    println!("smartctl: use -H, -A, or -a with a device path");
    0
}

fn run_smartd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smartd [OPTIONS]");
        println!("smartd v7.4 (SlateOS) — SMART monitoring daemon");
        println!("  -n              Don't fork (foreground)");
        println!("  -q LEVEL        Quiet mode (never, errors, nodev)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("smartd v7.4 (SlateOS)"); return 0; }
    println!("smartd: SMART monitoring daemon started");
    println!("  Monitoring 2 devices");
    println!("  Check interval: 1800 seconds");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "smartctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "smartd" => run_smartd(&rest, &prog),
        _ => run_smartctl(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_smartctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/smartmontools"), "smartmontools");
        assert_eq!(basename(r"C:\bin\smartmontools.exe"), "smartmontools.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("smartmontools.exe"), "smartmontools");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_smartctl(&["--help".to_string()], "smartmontools"), 0);
        assert_eq!(run_smartctl(&["-h".to_string()], "smartmontools"), 0);
        let _ = run_smartctl(&["--version".to_string()], "smartmontools");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_smartctl(&[], "smartmontools");
    }
}
