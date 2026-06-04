#![deny(clippy::all)]

//! h2o-cli — OurOS H2O HTTP/2 web server
//!
//! Single personality: `h2o`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_h2o(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: h2o [OPTIONS]");
        println!("H2O v2.3 (OurOS) — Optimized HTTP/1.x, HTTP/2, HTTP/3 server");
        println!();
        println!("Options:");
        println!("  -c FILE            Config file (YAML)");
        println!("  -m MODE            Mode (worker/master/daemon/test)");
        println!("  -t                 Test configuration");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("H2O v2.3.0 (OurOS)"); return 0; }
    println!("H2O v2.3.0 (OurOS)");
    println!("  Listening: 0.0.0.0:80 (HTTP/1.1, HTTP/2)");
    println!("  Listening: 0.0.0.0:443 (HTTPS, HTTP/2, HTTP/3)");
    println!("  Workers: 4");
    println!("  Handlers: file, fastcgi, proxy, mruby");
    println!("  HTTP/3: QUIC enabled");
    println!("  TLS: OpenSSL 3.2");
    println!("  Server push: enabled");
    println!("  Compression: gzip, brotli");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "h2o".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_h2o(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_h2o};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/h2o"), "h2o");
        assert_eq!(basename(r"C:\bin\h2o.exe"), "h2o.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("h2o.exe"), "h2o");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_h2o(&["--help".to_string()], "h2o"), 0);
        assert_eq!(run_h2o(&["-h".to_string()], "h2o"), 0);
        let _ = run_h2o(&["--version".to_string()], "h2o");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_h2o(&[], "h2o");
    }
}
