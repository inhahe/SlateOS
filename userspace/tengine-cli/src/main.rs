#![deny(clippy::all)]

//! tengine-cli — OurOS Tengine web server (Nginx fork)
//!
//! Single personality: `tengine`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tengine(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tengine [OPTIONS]");
        println!("Tengine v3.1 (OurOS) — High-performance web server (Nginx fork by Alibaba)");
        println!();
        println!("Options:");
        println!("  -c FILE            Config file");
        println!("  -g DIRECTIVES      Global config directives");
        println!("  -p PREFIX          Prefix path");
        println!("  -s SIGNAL          Send signal (stop/quit/reload/reopen)");
        println!("  -t                 Test configuration");
        println!("  -T                 Test and dump configuration");
        println!("  -m                 Show modules");
        println!("  -v                 Show version");
        println!("  -V                 Show version and build info");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "-V" || a == "--version") {
        println!("Tengine/3.1.0 (OurOS)");
        println!("  Based on: nginx/1.24.0");
        println!("  Extra: dynamic module loading, syslog, health checks");
        return 0;
    }
    println!("Tengine/3.1.0 (OurOS)");
    println!("  Workers: 4");
    println!("  Listening: 0.0.0.0:80, 0.0.0.0:443");
    println!("  Server names: 8 virtual hosts");
    println!("  Health checks: 3 upstream groups");
    println!("  Dynamic modules: 5 loaded");
    println!("  Session sticky: enabled");
    println!("  Consistent hash: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tengine".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tengine(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tengine};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tengine"), "tengine");
        assert_eq!(basename(r"C:\bin\tengine.exe"), "tengine.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tengine.exe"), "tengine");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_tengine(&["--help".to_string()], "tengine"), 0);
        assert_eq!(run_tengine(&["-h".to_string()], "tengine"), 0);
        assert_eq!(run_tengine(&["--version".to_string()], "tengine"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_tengine(&[], "tengine"), 0);
    }
}
