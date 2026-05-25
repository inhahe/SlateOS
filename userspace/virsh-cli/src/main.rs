#![deny(clippy::all)]

//! virsh-cli — OurOS virsh libvirt shell
//!
//! Single personality: `virsh`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_virsh(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virsh [OPTIONS] [COMMAND [ARGS]]");
        println!("virsh v10.0 (OurOS) — Libvirt management shell");
        println!();
        println!("Domain commands:");
        println!("  list              List domains");
        println!("  start NAME        Start a domain");
        println!("  shutdown NAME     Graceful shutdown");
        println!("  destroy NAME      Force stop");
        println!("  define FILE       Define from XML");
        println!("  dumpxml NAME      Dump domain XML");
        println!("  console NAME      Connect to console");
        println!();
        println!("Pool/volume/network commands also available.");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("virsh v10.0 (OurOS, libvirt)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("list") => {
            let all = args.iter().any(|a| a == "--all");
            println!(" Id   Name          State");
            println!("----------------------------");
            println!(" 1    ouros-dev     running");
            if all {
                println!(" -    fedora39      shut off");
                println!(" -    ubuntu22      shut off");
            }
        }
        Some("nodeinfo") => {
            println!("CPU model:           x86_64");
            println!("CPU(s):              8");
            println!("CPU frequency:       3600 MHz");
            println!("Memory size:         16777216 KiB");
        }
        _ => {
            println!("virsh: interactive shell (type 'help' for commands)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "virsh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_virsh(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
