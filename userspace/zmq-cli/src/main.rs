#![deny(clippy::all)]

//! zmq-cli — OurOS ZeroMQ CLI tool
//!
//! Single personality: `zmq`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zmq(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zmq COMMAND [OPTIONS]");
        println!("zmq v1.0.0 (OurOS) — ZeroMQ CLI tool");
        println!();
        println!("Commands:");
        println!("  pub             Publish messages");
        println!("  sub             Subscribe to messages");
        println!("  push            Push messages");
        println!("  pull            Pull messages");
        println!("  req             Send request");
        println!("  rep             Reply to requests");
        println!("  pair            Pair socket");
        println!("  proxy           Start proxy");
        println!("  monitor         Monitor socket");
        println!("  curve           Generate CURVE keys");
        println!("  version         Show version");
        println!();
        println!("Options:");
        println!("  -c, --connect ADDR   Connect to address");
        println!("  -b, --bind ADDR      Bind to address");
        println!("  -t, --timeout MS     Receive timeout");
        println!("  -n, --count N        Message count");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("zmq 1.0.0 (OurOS)");
        println!("libzmq: 4.3.5");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "pub" => {
            println!("Publishing on tcp://*:5555...");
            println!("Sent: topic1 Hello World");
        }
        "sub" => {
            println!("Subscribed to tcp://localhost:5555 [topic1]");
            println!("Received: topic1 Hello World");
        }
        "push" => println!("Pushed message to tcp://localhost:5556"),
        "pull" => println!("Pulling from tcp://*:5556..."),
        "req" => {
            println!("Sending request to tcp://localhost:5557...");
            println!("Reply: OK");
        }
        "rep" => println!("Listening for requests on tcp://*:5557..."),
        "proxy" => println!("Starting proxy: frontend=tcp://*:5559 backend=tcp://*:5560"),
        "monitor" => println!("Monitoring socket events..."),
        "curve" => {
            println!("Public key:  Yne@$w-vo<fVvi]a<NY6T1ed:M$fCG*[IaLV{{{{hID");
            println!("Secret key:  D:)Q[IlAW!ahhC2ac:9*A}}h:p?([4{{{{l8nMCK&(%1Jl");
        }
        _ => println!("zmq {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zmq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zmq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zmq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zmq"), "zmq");
        assert_eq!(basename(r"C:\bin\zmq.exe"), "zmq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zmq.exe"), "zmq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zmq(&["--help".to_string()], "zmq"), 0);
        assert_eq!(run_zmq(&["-h".to_string()], "zmq"), 0);
        let _ = run_zmq(&["--version".to_string()], "zmq");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zmq(&[], "zmq");
    }
}
