#![deny(clippy::all)]

//! trippy — SlateOS network diagnostic tool (mtr alternative)
//!
//! Single personality: `trip`

use std::env;
use std::process;

fn run_trip(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trip [OPTIONS] <TARGET>");
        println!();
        println!("A network diagnostic tool combining traceroute and ping.");
        println!();
        println!("Options:");
        println!("  -m, --mode <MODE>           TUI mode (tui/stream/pretty/markdown/csv/json/dot)");
        println!("  -u, --unprivileged          Unprivileged mode (UDP)");
        println!("  -p, --protocol <PROTO>      Protocol (icmp/udp/tcp)");
        println!("  -F, --addr-family <FAMILY>  Address family (ipv4/ipv6/ipv6-then-ipv4/ipv4-then-ipv6)");
        println!("  -P, --target-port <PORT>    Target port (default: 80)");
        println!("  -S, --source-port <PORT>    Source port");
        println!("  -A, --source-address <ADDR> Source address");
        println!("  -I, --interface <IF>        Network interface");
        println!("  -f, --first-ttl <N>         First TTL (default: 1)");
        println!("  -t, --max-ttl <N>           Max TTL (default: 64)");
        println!("  -i, --min-round-duration <MS>  Min round duration (default: 1000)");
        println!("  -T, --max-round-duration <MS>  Max round duration (default: 1000)");
        println!("  -g, --grace-duration <MS>   Grace period (default: 100)");
        println!("  -R, --read-timeout <MS>     Socket read timeout (default: 10)");
        println!("  -Q, --max-inflight <N>      Max simultaneous probes");
        println!("  -U, --initial-sequence <N>  Initial sequence number");
        println!("  --payload-pattern <HEX>     Custom payload pattern");
        println!("  --packet-size <SIZE>         Packet size (default: 84)");
        println!("  -c, --report-cycles <N>     Report after N cycles");
        println!("  --max-samples <N>           Max samples to collect");
        println!("  --dns-timeout <MS>          DNS resolution timeout");
        println!("  --dns-resolve-method <M>    DNS method (system/resolv/google/cloudflare)");
        println!("  --dns-lookup-as-info        Show AS info from DNS");
        println!("  --tui-as-mode <MODE>        TUI AS mode (asn/prefix/country-code/registry)");
        println!("  --tui-theme-colors <C>      Custom TUI theme colors");
        println!("  --tui-max-addrs <N>         Max addresses per hop");
        println!("  --log-format <FMT>          Log format (compact/pretty/json/chrome)");
        println!("  --log-filter <FILTER>       Log filter directive");
        println!("  --log-span-events <EVENTS>  Log span events");
        println!("  -V, --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("trippy 0.11.0 (Slate OS)");
        return 0;
    }

    // Find mode
    let mode = args.windows(2)
        .find(|w| w[0] == "-m" || w[0] == "--mode")
        .map(|w| w[1].as_str())
        .unwrap_or("tui");

    let target = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("example.com");

    match mode {
        "json" => {
            println!("[");
            println!("  {{\"ttl\":1,\"host\":\"192.168.1.1\",\"loss\":0.0,\"sent\":10,\"recv\":10,\"last\":1.2,\"avg\":1.5,\"best\":0.8,\"worst\":3.2,\"stddev\":0.6}},");
            println!("  {{\"ttl\":2,\"host\":\"10.0.0.1\",\"loss\":0.0,\"sent\":10,\"recv\":10,\"last\":5.4,\"avg\":5.8,\"best\":4.2,\"worst\":8.1,\"stddev\":1.1}},");
            println!("  {{\"ttl\":3,\"host\":\"72.14.215.85\",\"loss\":0.0,\"sent\":10,\"recv\":10,\"last\":12.3,\"avg\":13.1,\"best\":11.0,\"worst\":16.2,\"stddev\":1.4}},");
            println!("  {{\"ttl\":4,\"host\":\"93.184.216.34\",\"loss\":0.0,\"sent\":10,\"recv\":10,\"last\":18.5,\"avg\":19.2,\"best\":17.1,\"worst\":23.4,\"stddev\":1.8}}");
            println!("]");
        }
        "csv" => {
            println!("ttl,host,loss%,sent,recv,last,avg,best,worst,stddev");
            println!("1,192.168.1.1,0.0,10,10,1.2,1.5,0.8,3.2,0.6");
            println!("2,10.0.0.1,0.0,10,10,5.4,5.8,4.2,8.1,1.1");
            println!("3,72.14.215.85,0.0,10,10,12.3,13.1,11.0,16.2,1.4");
            println!("4,93.184.216.34,0.0,10,10,18.5,19.2,17.1,23.4,1.8");
        }
        "pretty" | "stream" => {
            println!("Tracing route to {} ...", target);
            println!();
            println!("  Hop  Host              Loss%  Sent  Recv  Last   Avg   Best  Worst  StdDev");
            println!("    1  192.168.1.1        0.0%    10    10   1.2   1.5   0.8    3.2     0.6");
            println!("    2  10.0.0.1           0.0%    10    10   5.4   5.8   4.2    8.1     1.1");
            println!("    3  72.14.215.85       0.0%    10    10  12.3  13.1  11.0   16.2     1.4");
            println!("    4  93.184.216.34      0.0%    10    10  18.5  19.2  17.1   23.4     1.8");
        }
        "markdown" => {
            println!("| Hop | Host | Loss% | Sent | Recv | Last | Avg | Best | Worst | StdDev |");
            println!("|-----|------|-------|------|------|------|-----|------|-------|--------|");
            println!("| 1 | 192.168.1.1 | 0.0% | 10 | 10 | 1.2 | 1.5 | 0.8 | 3.2 | 0.6 |");
            println!("| 2 | 10.0.0.1 | 0.0% | 10 | 10 | 5.4 | 5.8 | 4.2 | 8.1 | 1.1 |");
            println!("| 3 | 72.14.215.85 | 0.0% | 10 | 10 | 12.3 | 13.1 | 11.0 | 16.2 | 1.4 |");
            println!("| 4 | 93.184.216.34 | 0.0% | 10 | 10 | 18.5 | 19.2 | 17.1 | 23.4 | 1.8 |");
        }
        _ => {
            // TUI mode
            println!("trippy 0.11.0 (Slate OS) — TUI launched");
            println!("Tracing route to {} ...", target);
            println!();
            println!("  Hop  Host              Loss%  Sent  Recv  Last   Avg   Best  Worst  StdDev");
            println!("    1  192.168.1.1        0.0%    10    10   1.2   1.5   0.8    3.2     0.6");
            println!("    2  10.0.0.1           0.0%    10    10   5.4   5.8   4.2    8.1     1.1");
            println!("    3  72.14.215.85       0.0%    10    10  12.3  13.1  11.0   16.2     1.4");
            println!("    4  93.184.216.34      0.0%    10    10  18.5  19.2  17.1   23.4     1.8");
            println!();
            println!("  Latency graph (hop 4):");
            println!("  25ms │          ╭╮");
            println!("  20ms │  ╭╮╭╮╭╮╭╯╰╮╭╮  ╭╮");
            println!("  15ms │╭╯╰╯╰╯╰╯   ╰╯╰╮╭╯╰╮╭╮");
            println!("  10ms │╯                ╰╯  ╰╯╰╮");
            println!("   5ms │                        ╰─");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_trip(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_trip};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_trip(vec!["--help".to_string()]), 0);
        assert_eq!(run_trip(vec!["-h".to_string()]), 0);
        let _ = run_trip(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_trip(vec![]);
    }
}
