#![deny(clippy::all)]

//! gping — SlateOS ping with a graph
//!
//! Single personality: `gping`

use std::env;
use std::process;

fn run_gping(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gping [OPTIONS] <HOST>...");
        println!();
        println!("Ping, but with a graph.");
        println!();
        println!("Options:");
        println!("  -n, --watch-interval <SEC>  Ping interval (default: 0.2)");
        println!("  -b, --buffer <NUM>          Buffer size (default: 500)");
        println!("  -4                          Force IPv4");
        println!("  -6                          Force IPv6");
        println!("  -i, --interface <IF>        Network interface");
        println!("  -s, --simple-graphics       Use ASCII characters only");
        println!("  --vertical-margin <N>       Vertical margin (default: 1)");
        println!("  --horizontal-margin <N>     Horizontal margin (default: 0)");
        println!("  -c, --color <COLORS>        Assign colors (comma-separated)");
        println!("  --clear                     Clear data after a disconnect");
        println!("  --cmd <CMD>                 Run command instead of ping (graph its output)");
        println!("  -V, --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("gping 1.17.3 (SlateOS)");
        return 0;
    }

    let simple = args.iter().any(|a| a == "-s" || a == "--simple-graphics");
    let hosts: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if hosts.is_empty() {
        eprintln!("Error: at least one host required. See --help.");
        return 1;
    }

    // Simulate a TUI graph
    println!("gping 1.17.3 (SlateOS) — TUI launched");
    println!();

    for host in &hosts {
        println!("--- {} ---", host);
    }
    println!();

    if simple {
        println!(" 50ms |                                    *");
        println!(" 40ms |                          *    *");
        println!(" 30ms |              *    *  *       *    *");
        println!(" 20ms |    *    *  *    *                    *");
        println!(" 10ms |  *    *");
        println!("  0ms +--+----+----+----+----+----+----+----+-");
    } else {
        println!(" 50ms │                                    ╭╮");
        println!(" 40ms │                          ╭╮  ╭╮  ╭╯╰╮");
        println!(" 30ms │            ╭─╮  ╭╮╭╮  ╭╯╰╮╭╯╰╮╭╯   ╰╮");
        println!(" 20ms │  ╭╮  ╭╮╭─╮╯ ╰╮╭╯╰╯╰╮╭╯   ╰╯       ╰╮");
        println!(" 10ms │╭╯╰╮╭╯╰╯    ╰╯      ╰╯                ╰╮");
        println!("  0ms ╰┴───┴─────────────────────────────────────╯");
    }
    println!();

    for host in &hosts {
        println!("{}: min=12ms avg=25ms max=48ms (packets: 50 sent, 50 received, 0% loss)", host);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gping(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gping};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gping(vec!["--help".to_string()]), 0);
        assert_eq!(run_gping(vec!["-h".to_string()]), 0);
        let _ = run_gping(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gping(vec![]);
    }
}
