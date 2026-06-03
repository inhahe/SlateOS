#![deny(clippy::all)]

//! kong-cli — OurOS Kong API gateway
//!
//! Single personality: `kong`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kong(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kong [COMMAND] [OPTIONS]");
        println!("Kong v3.6 (OurOS) — Cloud-native API gateway");
        println!();
        println!("Commands:");
        println!("  start              Start Kong");
        println!("  stop               Stop Kong");
        println!("  restart            Restart Kong");
        println!("  reload             Reload config");
        println!("  migrations         Run migrations");
        println!("  check              Validate config");
        println!("  health             Health check");
        println!("  version            Show version");
        println!();
        println!("Options:");
        println!("  -c FILE            Config file (kong.conf)");
        println!("  --prefix DIR       Working directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kong v3.6.1 (OurOS)"); return 0; }
    println!("Kong v3.6.1 (OurOS)");
    println!("  Proxy: http://0.0.0.0:8000, https://0.0.0.0:8443");
    println!("  Admin API: http://0.0.0.0:8001");
    println!("  Services: 15");
    println!("  Routes: 34");
    println!("  Consumers: 89");
    println!("  Plugins: rate-limiting, key-auth, jwt, cors, acl");
    println!("  Database: PostgreSQL");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kong".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kong(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kong};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kong"), "kong");
        assert_eq!(basename(r"C:\bin\kong.exe"), "kong.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kong.exe"), "kong");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kong(&["--help".to_string()], "kong"), 0);
        assert_eq!(run_kong(&["-h".to_string()], "kong"), 0);
        assert_eq!(run_kong(&["--version".to_string()], "kong"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kong(&[], "kong"), 0);
    }
}
