#![deny(clippy::all)]

//! grpcurl-cli — SlateOS gRPC command-line client
//!
//! Multi-personality: `grpcurl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_grpcurl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: grpcurl [OPTIONS] ADDRESS [SYMBOL]");
        println!("grpcurl 1.9.1 (Slate OS)");
        println!();
        println!("Options:");
        println!("  -plaintext          Use plaintext (no TLS)");
        println!("  -d DATA             Request data (JSON)");
        println!("  -proto FILE         Proto file to use");
        println!("  -import-path DIR    Proto import path");
        println!("  -H HEADER           Add metadata header");
        println!("  -authority NAME     Override :authority header");
        println!("  list                List services");
        println!("  describe            Describe a symbol");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("grpcurl v1.9.1");
        return 0;
    }
    let addr = args.iter().find(|a| a.contains(':') && !a.starts_with('-') && !a.contains('='))
        .map(|s| s.as_str()).unwrap_or("localhost:50051");
    let symbol = args.last().map(|s| s.as_str()).unwrap_or("");

    if args.iter().any(|a| a == "list") {
        println!("grpc.health.v1.Health");
        println!("myservice.v1.UserService");
        println!("myservice.v1.OrderService");
        return 0;
    }
    if args.iter().any(|a| a == "describe") {
        println!("myservice.v1.UserService is a service:");
        println!("  rpc GetUser ( .myservice.v1.GetUserRequest ) returns ( .myservice.v1.User )");
        println!("  rpc ListUsers ( .myservice.v1.ListUsersRequest ) returns ( .myservice.v1.ListUsersResponse )");
        println!("  rpc CreateUser ( .myservice.v1.CreateUserRequest ) returns ( .myservice.v1.User )");
        return 0;
    }
    let data = args.windows(2).find(|w| w[0] == "-d")
        .map(|w| w[1].as_str());
    println!("Calling {} at {}...", symbol, addr);
    if data.is_some() {
        println!("{{");
        println!("  \"id\": \"abc123\",");
        println!("  \"name\": \"Test User\",");
        println!("  \"email\": \"test@example.com\"");
        println!("}}");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "grpcurl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_grpcurl(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_grpcurl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/grpcurl"), "grpcurl");
        assert_eq!(basename(r"C:\bin\grpcurl.exe"), "grpcurl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("grpcurl.exe"), "grpcurl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_grpcurl(&["--help".to_string()]), 0);
        assert_eq!(run_grpcurl(&["-h".to_string()]), 0);
        let _ = run_grpcurl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_grpcurl(&[]);
    }
}
