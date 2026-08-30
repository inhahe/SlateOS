#![deny(clippy::all)]

//! hdparm-cli — Slate OS hdparm disk parameter tool
//!
//! Single personality: `hdparm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hdparm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hdparm [OPTIONS] DEVICE");
        println!("hdparm 9.65 (Slate OS) — Get/set disk parameters");
        println!();
        println!("Options:");
        println!("  -a N          Get/set readahead sectors");
        println!("  -A N          Get/set read-lookahead (0/1)");
        println!("  -B N          Get/set APM (1-255)");
        println!("  -c N          Get/set I/O 32-bit support");
        println!("  -C             Check power mode");
        println!("  -d N          Get/set DMA (0/1)");
        println!("  -g             Display geometry");
        println!("  -i             Display identify info");
        println!("  -I             Detailed identify info");
        println!("  -M N          Get/set acoustic management");
        println!("  -S N          Set standby timeout");
        println!("  -t             Timing buffered reads");
        println!("  -T             Timing cached reads");
        println!("  -W N          Get/set write cache (0/1)");
        println!("  -V             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("hdparm v9.65 (Slate OS)");
        return 0;
    }
    let device = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/dev/sda");
    if args.iter().any(|a| a == "-i" || a == "-I") {
        println!("{}:", device);
        println!("  Model=Samsung SSD 980 PRO 1TB, FwRev=5B2QGXA7");
        println!("  Serial=S6B2NS0TB12345");
        println!("  Transport: Serial ATA 3.0");
        return 0;
    }
    if args.iter().any(|a| a == "-t") {
        println!("{}:", device);
        println!(" Timing buffered disk reads: 3200 MB in  3.00 seconds = 1066.67 MB/sec");
        return 0;
    }
    if args.iter().any(|a| a == "-T") {
        println!("{}:", device);
        println!(" Timing cached reads: 32000 MB in  2.00 seconds = 16000.00 MB/sec");
        return 0;
    }
    if args.iter().any(|a| a == "-C") {
        println!("{}:", device);
        println!(" drive state is:  active/idle");
        return 0;
    }
    if args.iter().any(|a| a == "-g") {
        println!("{}:", device);
        println!(" geometry = 60801/255/63, sectors = 976773168, start = 0");
        return 0;
    }
    println!("{}:", device);
    println!(" readonly     =  0 (off)");
    println!(" readahead    = 256 (on)");
    println!(" geometry     = 60801/255/63, sectors = 976773168, start = 0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hdparm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hdparm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hdparm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hdparm"), "hdparm");
        assert_eq!(basename(r"C:\bin\hdparm.exe"), "hdparm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hdparm.exe"), "hdparm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hdparm(&["--help".to_string()], "hdparm"), 0);
        assert_eq!(run_hdparm(&["-h".to_string()], "hdparm"), 0);
        let _ = run_hdparm(&["--version".to_string()], "hdparm");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hdparm(&[], "hdparm");
    }
}
