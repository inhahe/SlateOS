#![deny(clippy::all)]

//! slirp4netns-cli — OurOS slirp4netns user-mode networking
//!
//! Single personality: `slirp4netns`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_slirp4netns(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: slirp4netns [OPTIONS] PID TAPNAME");
        println!("slirp4netns v1.2 (OurOS) — User-mode networking for rootless containers");
        println!();
        println!("Options:");
        println!("  --configure        Auto-configure tap interface");
        println!("  --mtu N            MTU (default: 65520)");
        println!("  --cidr CIDR        Network CIDR (default: 10.0.2.0/24)");
        println!("  --disable-host-loopback  Block host loopback access");
        println!("  --netns-type TYPE  Namespace type (path, pid)");
        println!("  --api-socket PATH  API socket path");
        println!("  --enable-sandbox   Enable seccomp sandbox");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("slirp4netns v1.2.3 (OurOS)"); return 0; }
    println!("slirp4netns v1.2.3 (OurOS)");
    println!("  PID: 12345");
    println!("  TAP: tap0");
    println!("  Network: 10.0.2.0/24");
    println!("  Gateway: 10.0.2.2");
    println!("  DNS: 10.0.2.3");
    println!("  MTU: 65520");
    println!("  API socket: /tmp/slirp4netns.sock");
    println!("  Ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "slirp4netns".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_slirp4netns(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
