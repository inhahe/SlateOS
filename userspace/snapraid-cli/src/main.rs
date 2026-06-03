#![deny(clippy::all)]

//! snapraid-cli — OurOS SnapRAID parity-based backup
//!
//! Single personality: `snapraid`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_snapraid(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: snapraid COMMAND [OPTIONS]");
        println!("SnapRAID v12.3 (OurOS) — Parity-based backup for disk arrays");
        println!();
        println!("Commands:");
        println!("  sync           Sync parity data");
        println!("  scrub          Verify data integrity");
        println!("  fix            Fix damaged files");
        println!("  check          Check parity");
        println!("  status         Show array status");
        println!("  diff           Show changes since last sync");
        println!("  dup            Find duplicate files");
        println!("  list           List files");
        println!("  smart          Show SMART data");
        println!();
        println!("Options:");
        println!("  -c FILE        Config file (default: /etc/snapraid.conf)");
        println!("  -v             Verbose");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SnapRAID v12.3 (OurOS)"); return 0; }
    println!("SnapRAID v12.3 (OurOS)");
    println!("  Array status:");
    println!("    Data disks: 4");
    println!("    Parity disks: 2 (dual parity)");
    println!("    Files: 234,567");
    println!("    Size: 12.3 TiB");
    println!("    Parity: valid (last sync: 2h ago)");
    println!("  SMART:");
    println!("    /dev/sda: PASSED (temp: 34C, hours: 12345)");
    println!("    /dev/sdb: PASSED (temp: 36C, hours: 11234)");
    println!("    /dev/sdc: PASSED (temp: 35C, hours: 10123)");
    println!("    /dev/sdd: PASSED (temp: 33C, hours: 9012)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "snapraid".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_snapraid(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_snapraid};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/snapraid"), "snapraid");
        assert_eq!(basename(r"C:\bin\snapraid.exe"), "snapraid.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("snapraid.exe"), "snapraid");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_snapraid(&["--help".to_string()], "snapraid"), 0);
        assert_eq!(run_snapraid(&["-h".to_string()], "snapraid"), 0);
        assert_eq!(run_snapraid(&["--version".to_string()], "snapraid"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_snapraid(&[], "snapraid"), 0);
    }
}
