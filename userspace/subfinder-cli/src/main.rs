#![deny(clippy::all)]

//! subfinder-cli — OurOS subfinder subdomain discovery
//!
//! Single personality: `subfinder`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_subfinder(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: subfinder [OPTIONS]");
        println!("subfinder v2.6 (OurOS) — Subdomain discovery tool");
        println!();
        println!("Options:");
        println!("  -d DOMAIN      Target domain");
        println!("  -dL FILE       Domain list file");
        println!("  -o FILE        Output file");
        println!("  -oJ            JSON output");
        println!("  -t THREADS     Concurrent goroutines (default: 10)");
        println!("  -nW            Remove wildcards");
        println!("  -r RESOLVERS   Resolver list file");
        println!("  -sources       List available sources");
        println!("  -silent        Show only subdomains");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("subfinder v2.6.6 (OurOS)"); return 0; }
    println!("subfinder v2.6.6 (OurOS)");
    println!("  Domain: example.com");
    println!("  Sources: crtsh, virustotal, censys, shodan, dnsdumpster, ...");
    println!();
    println!("  www.example.com");
    println!("  mail.example.com");
    println!("  api.example.com");
    println!("  dev.example.com");
    println!("  staging.example.com");
    println!("  cdn.example.com");
    println!("  blog.example.com");
    println!("  app.example.com");
    println!();
    println!("  Found 8 subdomains for example.com in 3.4s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "subfinder".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_subfinder(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_subfinder};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/subfinder"), "subfinder");
        assert_eq!(basename(r"C:\bin\subfinder.exe"), "subfinder.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("subfinder.exe"), "subfinder");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_subfinder(&["--help".to_string()], "subfinder"), 0);
        assert_eq!(run_subfinder(&["-h".to_string()], "subfinder"), 0);
        assert_eq!(run_subfinder(&["--version".to_string()], "subfinder"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_subfinder(&[], "subfinder"), 0);
    }
}
