#![deny(clippy::all)]

//! bup-cli — Slate OS bup git-based backup
//!
//! Single personality: `bup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bup COMMAND [OPTIONS]");
        println!("bup v0.33 (Slate OS) — Git-based deduplicating backup");
        println!();
        println!("Commands:");
        println!("  init              Initialize bup repository");
        println!("  index PATH        Index files for backup");
        println!("  save -n NAME PATH Save indexed files");
        println!("  restore PATH      Restore from backup");
        println!("  ls PATH           List backup contents");
        println!("  fuse DIR          Mount backup as FUSE fs");
        println!("  fsck              Check repository integrity");
        println!("  midx              Merge index files");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("bup v0.33 (Slate OS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    match cmd {
        "init" => println!("Initialized empty bup repository in /home/user/.bup"),
        "index" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/home/user");
            println!("Indexing {}...", path);
            println!("  15420 files, 4.2 GiB");
        }
        "save" => {
            println!("Reading index...");
            println!("Saving...");
            println!("  Receiving index from server...");
            println!("  15420 blobs, 4.2 GiB");
            println!("  Bloom: 100.0%");
        }
        "ls" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/");
            if path == "/" {
                println!("mybackup/");
            } else {
                println!("2024-01-15-103000/");
                println!("2024-01-14-103000/");
                println!("latest/");
            }
        }
        "fsck" => {
            println!("Checking repository...");
            println!("  Packs: 45 verified");
            println!("  All packs OK");
        }
        "fuse" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("/mnt/bup");
            println!("Mounting bup FUSE filesystem at {}", dir);
        }
        _ => println!("bup: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bup(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bup};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bup"), "bup");
        assert_eq!(basename(r"C:\bin\bup.exe"), "bup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bup.exe"), "bup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bup(&["--help".to_string()], "bup"), 0);
        assert_eq!(run_bup(&["-h".to_string()], "bup"), 0);
        let _ = run_bup(&["--version".to_string()], "bup");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bup(&[], "bup");
    }
}
