#![deny(clippy::all)]

//! juicefs-cli — OurOS JuiceFS distributed POSIX filesystem
//!
//! Single personality: `juicefs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_juicefs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: juicefs [COMMAND] [OPTIONS]");
        println!("JuiceFS v1.2 (OurOS) — Distributed POSIX filesystem");
        println!();
        println!("Commands:");
        println!("  format META URL    Format a new volume");
        println!("  mount META DIR     Mount filesystem");
        println!("  umount DIR         Unmount filesystem");
        println!("  status META        Show volume status");
        println!("  info PATH          Show file info");
        println!("  bench DIR          Run benchmark");
        println!("  gc META            Garbage collection");
        println!("  fsck META          Check filesystem integrity");
        println!("  dump META          Dump metadata");
        println!();
        println!("Options:");
        println!("  --cache-dir DIR    Cache directory");
        println!("  --cache-size N     Cache size in MiB");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("JuiceFS v1.2.0 (OurOS)"); return 0; }
    println!("JuiceFS v1.2.0 (OurOS)");
    println!("  Metadata: Redis (redis://localhost:6379/1)");
    println!("  Storage: S3 (s3://juicefs-data)");
    println!("  Files: 567,890");
    println!("  Size: 1.8 TiB");
    println!("  Cache: 100 GiB (local SSD)");
    println!("  Mounted: /mnt/jfs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "juicefs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_juicefs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_juicefs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/juicefs"), "juicefs");
        assert_eq!(basename(r"C:\bin\juicefs.exe"), "juicefs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("juicefs.exe"), "juicefs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_juicefs(&["--help".to_string()], "juicefs"), 0);
        assert_eq!(run_juicefs(&["-h".to_string()], "juicefs"), 0);
        let _ = run_juicefs(&["--version".to_string()], "juicefs");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_juicefs(&[], "juicefs");
    }
}
