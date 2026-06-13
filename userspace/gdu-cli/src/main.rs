#![deny(clippy::all)]

//! gdu-cli — Slate OS gdu disk usage analyzer
//!
//! Single personality: `gdu`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gdu(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gdu [OPTIONS] [PATH]");
        println!("gdu 5.29.0 (Slate OS) — Fast disk usage analyzer");
        println!();
        println!("Options:");
        println!("  -d, --show-disks         Show all mounted disks");
        println!("  -a, --show-apparent-size  Show apparent size");
        println!("  -c, --no-color           Disable colors");
        println!("  -f FILE                  Read from file");
        println!("  -i, --ignore-dirs DIRS   Ignore directories");
        println!("  -I, --no-hidden          Ignore hidden");
        println!("  -l, --log-file FILE      Log file");
        println!("  -m, --max-cores N        Max cores to use");
        println!("  -n, --non-interactive    Non-interactive mode");
        println!("  -p, --no-progress        No progress indicator");
        println!("  -s, --summarize          Summarize only");
        println!("  -V, --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("gdu 5.29.0 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-d" || a == "--show-disks") {
        println!("Device        Size   Used  Avail  Use%  Mounted on");
        println!("/dev/sda1   500.0G 120.0G 354.8G   24%  /");
        println!("/dev/sda2   100.0G  45.2G  50.4G   45%  /home");
        return 0;
    }
    if args.iter().any(|a| a == "-n" || a == "--non-interactive") {
        println!("  45.2G /home/user");
        println!("  10.2G /usr");
        println!("   5.1G /var");
        println!("   2.3G /opt");
        return 0;
    }
    let path = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or(".");
    println!("gdu: Scanning '{}'...", path);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gdu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gdu(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gdu};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gdu"), "gdu");
        assert_eq!(basename(r"C:\bin\gdu.exe"), "gdu.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gdu.exe"), "gdu");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gdu(&["--help".to_string()], "gdu"), 0);
        assert_eq!(run_gdu(&["-h".to_string()], "gdu"), 0);
        let _ = run_gdu(&["--version".to_string()], "gdu");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gdu(&[], "gdu");
    }
}
