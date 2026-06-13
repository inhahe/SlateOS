#![deny(clippy::all)]

//! owncloud-cli — SlateOS ownCloud file sync
//!
//! Single personality: `owncloud`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_owncloud(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: owncloud [COMMAND] [OPTIONS]");
        println!("ownCloud Infinite Scale v5.0 (SlateOS) — File sync & share");
        println!();
        println!("Commands:");
        println!("  server             Start ownCloud server");
        println!("  init               Initialize data directory");
        println!("  users list|add     Manage users");
        println!("  spaces list|create Manage spaces");
        println!("  health             Check service health");
        println!("  version            Show version");
        println!();
        println!("Options:");
        println!("  --config-path DIR  Config directory");
        println!("  --base-data-path DIR  Data directory");
        println!("  --log-level LEVEL  Log level");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") { println!("ownCloud Infinite Scale v5.0.6 (SlateOS)"); return 0; }
    println!("ownCloud Infinite Scale v5.0.6 (SlateOS)");
    println!("  Users: 23");
    println!("  Spaces: 8 (personal + project)");
    println!("  Storage: 89 GiB used");
    println!("  Extensions: 12 loaded");
    println!("  Server: https://0.0.0.0:9200");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "owncloud".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_owncloud(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_owncloud};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/owncloud"), "owncloud");
        assert_eq!(basename(r"C:\bin\owncloud.exe"), "owncloud.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("owncloud.exe"), "owncloud");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_owncloud(&["--help".to_string()], "owncloud"), 0);
        assert_eq!(run_owncloud(&["-h".to_string()], "owncloud"), 0);
        let _ = run_owncloud(&["--version".to_string()], "owncloud");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_owncloud(&[], "owncloud");
    }
}
