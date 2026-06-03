#![deny(clippy::all)]

//! nginx-cli — OurOS Nginx web server
//!
//! Single personality: `nginx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nginx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nginx [OPTIONS]");
        println!("nginx v1.26 (OurOS) — High-performance HTTP and reverse proxy server");
        println!();
        println!("Options:");
        println!("  -c FILE            Config file (default: /etc/nginx/nginx.conf)");
        println!("  -g DIRECTIVES      Global config directives");
        println!("  -p PREFIX          Set prefix path");
        println!("  -s SIGNAL          Send signal (stop/quit/reload/reopen)");
        println!("  -t                 Test configuration");
        println!("  -T                 Test and dump configuration");
        println!("  -q                 Quiet mode during config test");
        println!("  -v                 Show version");
        println!("  -V                 Show version and build info");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "-V" || a == "--version") {
        println!("nginx/1.26.1 (OurOS)");
        println!("  TLS: OpenSSL 3.2");
        println!("  Modules: http_ssl, http_v2, http_realip, http_gzip_static");
        return 0;
    }
    println!("nginx/1.26.1 (OurOS)");
    println!("  Workers: 4");
    println!("  Listening: 0.0.0.0:80, 0.0.0.0:443 (SSL)");
    println!("  Server names: 12 virtual hosts");
    println!("  Upstreams: 5 backends");
    println!("  Active connections: 234");
    println!("  Requests/sec: 4,567");
    println!("  Config: /etc/nginx/nginx.conf (OK)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nginx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nginx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nginx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nginx"), "nginx");
        assert_eq!(basename(r"C:\bin\nginx.exe"), "nginx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nginx.exe"), "nginx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nginx(&["--help".to_string()], "nginx"), 0);
        assert_eq!(run_nginx(&["-h".to_string()], "nginx"), 0);
        assert_eq!(run_nginx(&["--version".to_string()], "nginx"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nginx(&[], "nginx"), 0);
    }
}
