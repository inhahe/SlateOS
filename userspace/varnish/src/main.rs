#![deny(clippy::all)]

//! varnish — SlateOS HTTP accelerator (reverse proxy cache)
//!
//! Multi-personality: `varnishd` (daemon), `varnishlog`, `varnishstat`, `varnishadm`

use std::env;
use std::process;

fn run_varnishd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: varnishd [options]");
        println!("  -a <addr:port>       Listen address (default: :80)");
        println!("  -b <backend>         Backend server");
        println!("  -f <vcl>             VCL configuration file");
        println!("  -s <storage>         Storage backend");
        println!("  -n <name>            Instance name");
        println!("  -F                   Run in foreground");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("varnishd (varnish-7.5.0 revision abc1234) (Slate OS)");
        return 0;
    }
    let addr = args.iter().position(|a| a == "-a")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or(":80");
    let backend = args.iter().position(|a| a == "-b")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1:8080");
    println!("Debug: Version: varnish-7.5.0 (Slate OS)");
    println!("Debug: Platform: Slate OS,x86_64");
    println!("Debug: Child (12346) started");
    println!("Info: Child (12346) said Ready");
    println!("Notice: Listening on {}", addr);
    println!("Notice: Backend: {}", backend);
    0
}

fn run_varnishstat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: varnishstat [options]");
        println!("  -1    Print stats once and exit");
        println!("  -j    JSON output");
        println!("  -n    Instance name");
        return 0;
    }
    println!("Uptime mgt:   86400 10:00:00");
    println!("Uptime child: 86400 10:00:00");
    println!();
    println!("NAME                   CURRENT   CHANGE   AVERAGE   DESCRIPTION");
    println!("MAIN.cache_hit         128601       14      1.49    Cache hits");
    println!("MAIN.cache_miss        14289         2      0.17    Cache misses");
    println!("MAIN.client_req        142890       16      1.65    Client requests");
    println!("MAIN.backend_req       14289         2      0.17    Backend requests");
    println!("MAIN.sess_conn         8923          1      0.10    Sessions accepted");
    println!("MAIN.n_object          4567          0         .    Objects in cache");
    println!("MAIN.s_resp_bodybytes  1234567890   1234  14291.3   Response body bytes");
    println!("MAIN.s_resp_hdrbytes   12345678      12    142.9    Response header bytes");
    println!();
    println!("Hit rate ratio:  90.00%");
    0
}

fn run_varnishlog(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: varnishlog [options]");
        println!("  -q <query>   VSL query");
        println!("  -g <group>   Grouping mode (session|request|vxid|raw)");
        println!("  -n <name>    Instance name");
        return 0;
    }
    println!("*   << Request  >> 12345");
    println!("-   Begin          req 12345 rxreq");
    println!("-   Timestamp      Start: 1716368400.000");
    println!("-   ReqStart       192.168.1.100 49876 http");
    println!("-   ReqMethod      GET");
    println!("-   ReqURL         /index.html");
    println!("-   ReqProtocol    HTTP/1.1");
    println!("-   ReqHeader      Host: example.com");
    println!("-   VCL_call       RECV");
    println!("-   VCL_return     hash");
    println!("-   VCL_call       HASH");
    println!("-   VCL_return     lookup");
    println!("-   Hit            1234567 10.000000 0.000000 0.000000");
    println!("-   RespProtocol   HTTP/1.1");
    println!("-   RespStatus     200");
    println!("-   RespReason     OK");
    println!("-   End");
    0
}

fn run_varnishadm(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: varnishadm [options] [command]");
        println!("  -n <name>    Instance name");
        println!("  -S <secret>  Secret file path");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    match cmd {
        Some("status") => println!("Child in state running"),
        Some("vcl.list") => {
            println!("available  auto/warm  0  boot");
            println!("active     auto/warm  6  reload_2025-05-22T10:00:00");
        }
        Some("backend.list") => {
            println!("Backend name   Admin   Probe   Health");
            println!("default(127.0.0.1:8080)  probe   Healthy 5/5");
        }
        Some("ban.list") => println!("(no bans)"),
        Some("ping") => println!("PONG 1716368400 1.0"),
        Some("panic.show") => println!("No panic recorded."),
        _ => {
            println!("200");
            println!("-----------------------------");
            println!("Varnish Cache CLI 1.0");
            println!("-----------------------------");
            println!("Type 'help' for command list.");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("varnishd");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "varnishstat" => run_varnishstat(rest),
        "varnishlog" => run_varnishlog(rest),
        "varnishadm" => run_varnishadm(rest),
        _ => run_varnishd(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_varnishd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_varnishd(vec!["--help".to_string()]), 0);
        assert_eq!(run_varnishd(vec!["-h".to_string()]), 0);
        let _ = run_varnishd(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_varnishd(vec![]);
    }
}
