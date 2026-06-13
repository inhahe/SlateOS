#![deny(clippy::all)]

//! apisix-cli — SlateOS Apache APISIX API gateway
//!
//! Single personality: `apisix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_apisix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: apisix [COMMAND] [OPTIONS]");
        println!("Apache APISIX v3.9 (SlateOS) — Dynamic API gateway");
        println!();
        println!("Commands:");
        println!("  start              Start APISIX");
        println!("  stop               Stop APISIX");
        println!("  restart            Restart APISIX");
        println!("  reload             Reload plugins");
        println!("  test               Test configuration");
        println!("  discovery          Service discovery");
        println!();
        println!("Options:");
        println!("  -c FILE            Config file (config.yaml)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apache APISIX v3.9.1 (SlateOS)"); return 0; }
    println!("Apache APISIX v3.9.1 (SlateOS)");
    println!("  Proxy: http://0.0.0.0:9080, https://0.0.0.0:9443");
    println!("  Admin API: http://0.0.0.0:9180");
    println!("  etcd: localhost:2379");
    println!("  Routes: 23");
    println!("  Upstreams: 12");
    println!("  Services: 8");
    println!("  Plugins: 80+ available, 15 active");
    println!("  SSL certificates: 5");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "apisix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_apisix(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_apisix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/apisix"), "apisix");
        assert_eq!(basename(r"C:\bin\apisix.exe"), "apisix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("apisix.exe"), "apisix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_apisix(&["--help".to_string()], "apisix"), 0);
        assert_eq!(run_apisix(&["-h".to_string()], "apisix"), 0);
        let _ = run_apisix(&["--version".to_string()], "apisix");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_apisix(&[], "apisix");
    }
}
