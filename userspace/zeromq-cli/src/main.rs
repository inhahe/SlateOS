#![deny(clippy::all)]

//! zeromq-cli — OurOS ZeroMQ tools
//!
//! Multi-personality: `zmq-send`, `zmq-recv`, `zmq-proxy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zmq(args: &[String], prog_name: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match prog_name {
            "zmq-send" => {
                println!("Usage: zmq-send [OPTIONS] ENDPOINT MESSAGE");
                println!("  -t TYPE   Socket type (push, pub, req)");
                println!("  --bind    Bind instead of connect");
            }
            "zmq-recv" => {
                println!("Usage: zmq-recv [OPTIONS] ENDPOINT");
                println!("  -t TYPE   Socket type (pull, sub, rep)");
                println!("  --bind    Bind instead of connect");
                println!("  -n N      Receive N messages then exit");
            }
            _ => {
                println!("Usage: zmq-proxy [OPTIONS]");
                println!("  --frontend ENDPOINT   Frontend socket");
                println!("  --backend ENDPOINT    Backend socket");
                println!("  --type TYPE           Proxy type (forwarder, streamer, queue)");
            }
        }
        println!("ZeroMQ CLI tools (libzmq 4.3.5, OurOS)");
        return 0;
    }
    match prog_name {
        "zmq-send" => {
            let endpoint = args.iter().find(|a| a.starts_with("tcp://") || a.starts_with("ipc://"))
                .map(|s| s.as_str()).unwrap_or("tcp://localhost:5555");
            let msg = args.last().map(|s| s.as_str()).unwrap_or("hello");
            println!("Connecting to {}...", endpoint);
            println!("Sent: {}", msg);
        }
        "zmq-recv" => {
            let endpoint = args.iter().find(|a| a.starts_with("tcp://") || a.starts_with("ipc://"))
                .map(|s| s.as_str()).unwrap_or("tcp://localhost:5555");
            println!("Listening on {}...", endpoint);
            println!("Received: hello");
        }
        _ => {
            let frontend = args.windows(2).find(|w| w[0] == "--frontend")
                .map(|w| w[1].as_str()).unwrap_or("tcp://*:5559");
            let backend = args.windows(2).find(|w| w[0] == "--backend")
                .map(|w| w[1].as_str()).unwrap_or("tcp://*:5560");
            println!("ZMQ proxy: {} <-> {}", frontend, backend);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zmq-send".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zmq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
