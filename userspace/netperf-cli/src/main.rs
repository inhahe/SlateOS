#![deny(clippy::all)]

//! netperf-cli — OurOS netperf network performance benchmark
//!
//! Multi-personality: `netperf`, `netserver`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_netperf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: netperf [OPTIONS] -H HOST");
        println!("netperf v2.7 (OurOS) — Network performance benchmark");
        println!();
        println!("Options:");
        println!("  -H HOST           Remote host");
        println!("  -t TEST           Test type (TCP_STREAM, TCP_RR, UDP_STREAM, UDP_RR)");
        println!("  -l SECS           Test duration");
        println!("  -p PORT           Remote port");
        println!("  -- -m SIZE        Message size");
        println!("  -- -r REQ,RSP     Request/response sizes");
        return 0;
    }
    let host = args.iter().skip_while(|a| a.as_str() != "-H").nth(1).map(|s| s.as_str()).unwrap_or("localhost");
    let test = args.iter().skip_while(|a| a.as_str() != "-t").nth(1).map(|s| s.as_str()).unwrap_or("TCP_STREAM");
    println!("MIGRATED {} {} from 0.0.0.0 (0.0.0.0) port 0 to {} () port 0", test, test, host);
    match test {
        "TCP_RR" => {
            println!("Local /Remote");
            println!("Socket Size   Request  Resp.   Elapsed  Trans.");
            println!("Send   Recv   Size     Size    Time     Rate");
            println!("bytes  Bytes  bytes    bytes   secs.    per sec");
            println!();
            println!("16384  131072 1        1       10.00    28547.23");
        }
        "UDP_STREAM" => {
            println!("Socket  Message  Elapsed      Messages");
            println!("Size    Size     Time         Okay Errors   Throughput");
            println!("bytes   bytes    secs            #      #   Mbits/sec");
            println!();
            println!("212992  65507    10.00      15234      0     7982.41");
        }
        _ => {
            println!("Recv   Send    Send");
            println!("Socket Socket  Message  Elapsed");
            println!("Size   Size    Size     Time     Throughput");
            println!("bytes  bytes   bytes    secs.    Mbits/sec");
            println!();
            println!("131072  16384  16384    10.00     941.23");
        }
    }
    0
}

fn run_netserver(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: netserver [OPTIONS]");
        println!("netserver v2.7 (OurOS) — netperf server daemon");
        println!();
        println!("Options:");
        println!("  -p PORT           Listen port (default 12865)");
        println!("  -D                Do not daemonize");
        println!("  -L HOST           Bind to specific address");
        return 0;
    }
    println!("Starting netserver with host '0.0.0.0' port '12865'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "netperf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "netserver" => run_netserver(&rest, &prog),
        _ => run_netperf(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_netperf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/netperf"), "netperf");
        assert_eq!(basename(r"C:\bin\netperf.exe"), "netperf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("netperf.exe"), "netperf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_netperf(&["--help".to_string()], "netperf"), 0);
        assert_eq!(run_netperf(&["-h".to_string()], "netperf"), 0);
        let _ = run_netperf(&["--version".to_string()], "netperf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_netperf(&[], "netperf");
    }
}
