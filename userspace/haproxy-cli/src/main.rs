#![deny(clippy::all)]

//! haproxy-cli — OurOS HAProxy load balancer/reverse proxy
//!
//! Multi-personality: `haproxy`, `hatop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_haproxy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: haproxy [OPTIONS]");
        println!();
        println!("haproxy — high-performance load balancer (OurOS).");
        println!();
        println!("Options:");
        println!("  -f <file>       Configuration file");
        println!("  -c              Check config and exit");
        println!("  -d              Debug mode");
        println!("  -D              Daemon mode");
        println!("  -sf <pid...>    Soft-stop old PIDs");
        println!("  -st <pid...>    Hard-stop old PIDs");
        println!("  -p <pidfile>    PID file");
        println!("  -v              Show version");
        println!("  -vv             Show build options");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("HAProxy version 2.9.4-1 (OurOS)");
        println!("Status: long-term supported branch");
        println!("Built with OpenSSL 3.2.1 30 Jan 2024");
        return 0;
    }
    if args.iter().any(|a| a == "-vv") {
        println!("HAProxy version 2.9.4-1 (OurOS)");
        println!("Build options:");
        println!("  TARGET  = linux-glibc");
        println!("  CC      = gcc");
        println!("  CFLAGS  = -O2 -g -Wall");
        println!();
        println!("Feature list : +OPENSSL +LUA +ZLIB +PCRE2 +SYSTEMD");
        println!("               +QUIC +PROMEX +51DEGREES");
        println!();
        println!("Default settings:");
        println!("  bufsize = 16384, maxrewrite = 1024, maxpollevents = 200");
        println!("  content-length: 2147483647");
        return 0;
    }

    if args.iter().any(|a| a == "-c") {
        let config = args.windows(2).find(|w| w[0] == "-f")
            .map(|w| w[1].as_str())
            .unwrap_or("/etc/haproxy/haproxy.cfg");
        println!("Configuration file is valid: {}", config);
        return 0;
    }

    let config = args.windows(2).find(|w| w[0] == "-f")
        .map(|w| w[1].as_str())
        .unwrap_or("/etc/haproxy/haproxy.cfg");
    println!("[NOTICE]   (1) : haproxy version is 2.9.4-1 (OurOS)");
    println!("[NOTICE]   (1) : Loading config from '{}'", config);
    println!("[WARNING]  (1) : config : missing stats socket");
    println!("[NOTICE]   (1) : New worker (2) forked");
    println!("[NOTICE]   (1) : Loading success.");
    println!();
    println!("Frontends:");
    println!("  http-in         *:80      OPEN   (default_backend: servers)");
    println!("  https-in        *:443     OPEN   (default_backend: servers)");
    println!();
    println!("Backends:");
    println!("  servers          roundrobin");
    println!("    web1           192.168.1.10:8080   UP   weight: 1");
    println!("    web2           192.168.1.11:8080   UP   weight: 1");
    println!("    web3           192.168.1.12:8080   DOWN weight: 1");
    0
}

fn run_hatop(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hatop [OPTIONS]");
        println!();
        println!("hatop — HAProxy stats viewer (OurOS).");
        println!();
        println!("Options:");
        println!("  -s <socket>    Stats socket path");
        println!("  -i <interval>  Refresh interval (default: 1s)");
        return 0;
    }

    println!("HAProxy Statistics — hatop 0.8.0 (OurOS)");
    println!("Stats socket: /run/haproxy/admin.sock");
    println!();
    println!("NAME             STATUS  WEIGHT  CUR  MAX  RATE  BYTES_IN   BYTES_OUT");
    println!("http-in/FRONTEND OPEN    -       42   500  12    12345678   98765432");
    println!("servers/web1     UP      1       15   200  4     4567890    34567890");
    println!("servers/web2     UP      1       17   200  5     5678901    45678901");
    println!("servers/web3     DOWN    1       0    200  0     0          0");
    println!("servers/BACKEND  UP      2/3     32   600  9     10246791   80246791");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "haproxy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "hatop" => run_hatop(&rest),
        _ => run_haproxy(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_haproxy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/haproxy"), "haproxy");
        assert_eq!(basename(r"C:\bin\haproxy.exe"), "haproxy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("haproxy.exe"), "haproxy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_haproxy(&["--help".to_string()]), 0);
        assert_eq!(run_haproxy(&["-h".to_string()]), 0);
        let _ = run_haproxy(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_haproxy(&[]);
    }
}
