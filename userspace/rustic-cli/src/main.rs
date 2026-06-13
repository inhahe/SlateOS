#![deny(clippy::all)]

//! rustic-cli — Slate OS Rustic fast backup tool
//!
//! Single personality: `rustic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rustic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rustic COMMAND [OPTIONS]");
        println!("rustic v0.7 (Slate OS) — Fast, encrypted backup tool");
        println!();
        println!("Commands:");
        println!("  init              Initialize repository");
        println!("  backup PATH       Create backup");
        println!("  restore SNAP DST  Restore snapshot");
        println!("  snapshots         List snapshots");
        println!("  forget            Remove old snapshots");
        println!("  prune             Remove unreferenced data");
        println!("  check             Verify repository integrity");
        println!("  diff SNAP SNAP    Show differences");
        println!("  cat OBJ           Show repository object");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rustic v0.7 (Slate OS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("snapshots");
    match cmd {
        "init" => {
            println!("created rustic repository at /mnt/backup/rustic");
            println!("  New password saved");
        }
        "backup" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/home/user");
            println!("scanning {}...", path);
            println!("Files:        15420 new,  0 changed,  0 unmodified");
            println!("Dirs:          1230 new,  0 changed,  0 unmodified");
            println!("Data Blobs:    2450 new");
            println!("Added to repo: 1.2 GiB");
            println!("processed 15420 files, 4.2 GiB in 1:45");
            println!("snapshot abc12345 saved");
        }
        "snapshots" => {
            println!("ID        Time                Host    Tags   Paths");
            println!("------------------------------------------------------");
            println!("abc12345  2024-01-15 10:30:00  mypc           /home/user");
            println!("def67890  2024-01-14 10:30:00  mypc           /home/user");
            println!("ghi11111  2024-01-13 10:30:00  mypc           /home/user");
            println!("3 snapshots");
        }
        "check" => {
            println!("using temporary cache in /tmp/rustic-check");
            println!("checking repository index...");
            println!("repository passes check");
        }
        "forget" => {
            println!("Applying policy: keep last 7 daily, 4 weekly, 6 monthly");
            println!("Removed 2 snapshots");
        }
        _ => println!("rustic: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rustic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rustic(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rustic};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rustic"), "rustic");
        assert_eq!(basename(r"C:\bin\rustic.exe"), "rustic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rustic.exe"), "rustic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rustic(&["--help".to_string()], "rustic"), 0);
        assert_eq!(run_rustic(&["-h".to_string()], "rustic"), 0);
        let _ = run_rustic(&["--version".to_string()], "rustic");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rustic(&[], "rustic");
    }
}
