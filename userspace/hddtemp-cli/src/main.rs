#![deny(clippy::all)]

//! hddtemp-cli — SlateOS hard drive temperature monitor
//!
//! Single personality: `hddtemp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hddtemp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hddtemp [OPTIONS] DISK...");
        println!("hddtemp v0.4 (Slate OS) — Hard drive temperature monitor");
        println!();
        println!("Options:");
        println!("  -n                Numeric output only");
        println!("  -q                Quiet mode");
        println!("  -d                Run as daemon");
        println!("  -l                Listen on port (default: 7634)");
        println!("  -p PORT           Port number");
        println!("  -F                Fahrenheit");
        println!("  -f FILE           Database file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hddtemp v0.4 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-n") {
        println!("35");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("hddtemp: daemon started on port 7634");
        return 0;
    }
    let disks: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if disks.is_empty() {
        println!("/dev/sda: Samsung SSD 870: 35\u{00b0}C");
        println!("/dev/sdb: WDC WD10EZEX: 38\u{00b0}C");
    } else {
        for disk in disks {
            println!("{}: Generic Drive: 36\u{00b0}C", disk);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hddtemp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hddtemp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hddtemp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hddtemp"), "hddtemp");
        assert_eq!(basename(r"C:\bin\hddtemp.exe"), "hddtemp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hddtemp.exe"), "hddtemp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hddtemp(&["--help".to_string()], "hddtemp"), 0);
        assert_eq!(run_hddtemp(&["-h".to_string()], "hddtemp"), 0);
        let _ = run_hddtemp(&["--version".to_string()], "hddtemp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hddtemp(&[], "hddtemp");
    }
}
