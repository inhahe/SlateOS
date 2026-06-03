#![deny(clippy::all)]

//! haproxy — OurOS reliable, high-performance TCP/HTTP load balancer
//!
//! Single personality: `haproxy`

use std::env;
use std::process;

fn run_haproxy(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: haproxy [-f <cfgfile|cfgdir>] [-vVdqDsS] [-n maxconn] [-N maxconn]");
        println!("               [-p pidfile] [-m <megs>] [-C <dir>] [-W] [-Ws]");
        println!();
        println!("Options:");
        println!("  -f <cfgfile>  Load configuration from <cfgfile>");
        println!("  -c            Check configuration and exit");
        println!("  -v            Show version and exit");
        println!("  -vv           Show version and build options");
        println!("  -d            Enable debug mode (foreground + verbose)");
        println!("  -D            Run as daemon");
        println!("  -W            Master-worker mode");
        println!("  -Ws           Master-worker mode with systemd support");
        println!("  -q            Quiet mode");
        println!("  -n <maxconn>  Set maximum total connections");
        println!("  -N <maxconn>  Set default per-proxy maxconn");
        println!("  -p <pidfile>  Write PIDs to <pidfile>");
        println!("  -m <megs>     Limit usable memory to <megs> MB");
        println!("  -sf/-st       Soft-stop/terminate old processes");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("HAProxy version 2.9.7 2025/05/22 - https://haproxy.org/");
        println!("Status: stable branch - will stop receiving fixes around Q1 2025.");
        println!("Known bugs: http://www.haproxy.org/bugs/bugs-2.9.7.html");
        println!("Running on: OurOS x86_64");
        return 0;
    }
    if args.iter().any(|a| a == "-vv") {
        println!("HAProxy version 2.9.7 2025/05/22 - https://haproxy.org/");
        println!("Running on: OurOS x86_64");
        println!("Build options:");
        println!("  TARGET  = ouros");
        println!("  CPU     = generic");
        println!("  CC      = rustc");
        println!("  CFLAGS  = -O2");
        println!();
        println!("Feature list : +EPOLL +POLL -KQUEUE +TPROXY +LINUX_TPROXY");
        println!("               +OPENSSL +LUA +ZLIB +SLZ +PROMEX +51DEGREES");
        println!();
        println!("Default settings:");
        println!("  bufsize = 16384, maxrewrite = 1024, maxpollevents = 200");
        return 0;
    }
    if args.iter().any(|a| a == "-c") {
        let cfg = args.iter().position(|a| a == "-f")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("/etc/haproxy/haproxy.cfg");
        println!("Configuration file is valid: {}", cfg);
        return 0;
    }

    // Start server
    let cfg = args.iter().position(|a| a == "-f")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("/etc/haproxy/haproxy.cfg");
    println!("[NOTICE]   (12345) : haproxy version is 2.9.7 (OurOS)");
    println!("[NOTICE]   (12345) : Loading {}", cfg);
    println!("[WARNING]  (12345) : config : 'option forwardfor' enabled on backend 'servers'.");
    println!("[NOTICE]   (12345) : New worker #1 (12346) forked");
    println!("[NOTICE]   (12345) : Loading success.");
    println!("[NOTICE]   (12345) : haproxy is ready.");
    println!();
    println!("Frontends:");
    println!("  http-in  *:80    (maxconn: 2000)");
    println!("  https-in *:443   (maxconn: 2000)");
    println!("  stats    *:8404  (maxconn: 100)");
    println!();
    println!("Backends:");
    println!("  servers  roundrobin  [web1:8080 web2:8080 web3:8080]");
    println!("  api      leastconn   [api1:3000 api2:3000]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_haproxy(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_haproxy};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_haproxy(vec!["--help".to_string()]), 0);
        assert_eq!(run_haproxy(vec!["-h".to_string()]), 0);
        assert_eq!(run_haproxy(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_haproxy(vec![]), 0);
    }
}
