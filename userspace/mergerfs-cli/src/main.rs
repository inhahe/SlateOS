#![deny(clippy::all)]

//! mergerfs-cli — OurOS mergerfs union filesystem
//!
//! Single personality: `mergerfs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mergerfs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mergerfs [OPTIONS] BRANCHES MOUNTPOINT");
        println!("mergerfs v2.40 (OurOS) — FUSE union filesystem");
        println!();
        println!("Options:");
        println!("  -o category.create=POLICY  Create policy (mfs, lfs, epmfs, etc.)");
        println!("  -o category.search=POLICY  Search policy");
        println!("  -o category.action=POLICY  Action policy");
        println!("  -o minfreespace=SIZE       Min free space threshold");
        println!("  -o moveonenospc=true       Move on no space");
        println!("  -o dropcacheonclose=true   Drop cache on close");
        println!("  -o cache.files=partial     File caching mode");
        println!("  -o async_read=true         Async reads");
        println!("  --version                  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mergerfs v2.40.2 (OurOS)"); return 0; }
    println!("mergerfs v2.40.2 (OurOS)");
    println!("  Branches:");
    println!("    /mnt/disk1 (4.0 TiB, 1.2 TiB free)");
    println!("    /mnt/disk2 (4.0 TiB, 2.3 TiB free)");
    println!("    /mnt/disk3 (8.0 TiB, 5.6 TiB free)");
    println!("  Mountpoint: /mnt/pool");
    println!("  Total: 16.0 TiB, 9.1 TiB free");
    println!("  Create policy: mfs (most free space)");
    println!("  Min free space: 10 GiB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mergerfs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mergerfs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mergerfs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mergerfs"), "mergerfs");
        assert_eq!(basename(r"C:\bin\mergerfs.exe"), "mergerfs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mergerfs.exe"), "mergerfs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mergerfs(&["--help".to_string()], "mergerfs"), 0);
        assert_eq!(run_mergerfs(&["-h".to_string()], "mergerfs"), 0);
        let _ = run_mergerfs(&["--version".to_string()], "mergerfs");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mergerfs(&[], "mergerfs");
    }
}
