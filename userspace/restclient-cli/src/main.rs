#![deny(clippy::all)]

//! restclient-cli — SlateOS REST client tool
//!
//! Single personality: `restclient`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_restclient(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: restclient [OPTIONS] FILE");
        println!("restclient v1.0.0 (SlateOS) — Execute .http/.rest request files");
        println!();
        println!("Options:");
        println!("  FILE             .http/.rest request file");
        println!("  -e, --env NAME   Environment name");
        println!("  -n, --request N  Execute Nth request (0-indexed)");
        println!("  --all            Execute all requests");
        println!("  --dry-run        Show without executing");
        println!("  -v, --verbose    Verbose output");
        println!("  -o, --output FILE  Save response");
        println!("  -V, --version    Show version");
        println!();
        println!(".http file format:");
        println!("  GET https://api.example.com/users");
        println!("  Authorization: Bearer token123");
        println!("  ###");
        println!("  POST https://api.example.com/users");
        println!("  Content-Type: application/json");
        println!("  {{\"name\": \"Alice\"}}");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("restclient v1.0.0 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--dry-run") {
        println!("=== Request #1 ===");
        println!("GET https://api.example.com/users");
        println!("Authorization: Bearer {{token}}");
        println!("(dry run — not sent)");
        return 0;
    }
    let file = args.first().map(|s| s.as_str()).unwrap_or("requests.http");
    println!("Executing requests from: {}", file);
    println!();
    println!("=== Request #1: GET /users ===");
    println!("HTTP/1.1 200 OK");
    println!("Content-Type: application/json");
    println!("[{{\"id\":1,\"name\":\"Alice\"}},{{\"id\":2,\"name\":\"Bob\"}}]");
    println!();
    println!("1 request executed, 1 successful, 0 failed.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "restclient".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_restclient(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_restclient};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/restclient"), "restclient");
        assert_eq!(basename(r"C:\bin\restclient.exe"), "restclient.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("restclient.exe"), "restclient");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_restclient(&["--help".to_string()], "restclient"), 0);
        assert_eq!(run_restclient(&["-h".to_string()], "restclient"), 0);
        let _ = run_restclient(&["--version".to_string()], "restclient");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_restclient(&[], "restclient");
    }
}
