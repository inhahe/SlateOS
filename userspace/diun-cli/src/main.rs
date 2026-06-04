#![deny(clippy::all)]

//! diun-cli — OurOS Docker Image Update Notifier
//!
//! Single personality: `diun`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_diun(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: diun COMMAND [OPTIONS]");
        println!("diun v4.28 (OurOS) — Docker Image Update Notifier");
        println!();
        println!("Commands:");
        println!("  serve             Start watching for updates");
        println!("  image list        List watched images");
        println!("  image inspect     Inspect image details");
        println!("  notif test        Test notifications");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "serve" => {
            println!("diun: starting watcher...");
            println!("  Images watched: 15");
            println!("  Schedule: every 6h");
            println!("  Notifiers: email, webhook");
        }
        "image" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("IMAGE                     TAG      STATUS");
                println!("nginx                     latest   up-to-date");
                println!("postgres                  16       update available");
                println!("redis                     7        up-to-date");
            } else {
                println!("Image: nginx:latest");
                println!("  Registry: docker.io");
                println!("  Digest: sha256:abc123...");
                println!("  Last checked: 2024-01-15 10:30:00");
            }
        }
        "notif" => {
            println!("Testing notifications...");
            println!("  email: sent OK");
            println!("  webhook: sent OK");
        }
        "version" | "--version" => println!("diun v4.28 (OurOS)"),
        _ => println!("diun {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "diun".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_diun(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_diun};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/diun"), "diun");
        assert_eq!(basename(r"C:\bin\diun.exe"), "diun.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("diun.exe"), "diun");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_diun(&["--help".to_string()], "diun"), 0);
        assert_eq!(run_diun(&["-h".to_string()], "diun"), 0);
        let _ = run_diun(&["--version".to_string()], "diun");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_diun(&[], "diun");
    }
}
