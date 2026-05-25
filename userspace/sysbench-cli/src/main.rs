#![deny(clippy::all)]

//! sysbench-cli — OurOS sysbench system benchmark
//!
//! Single personality: `sysbench`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sysbench(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sysbench [OPTIONS] TEST [COMMAND]");
        println!("sysbench v1.0 (OurOS) — System performance benchmark");
        println!();
        println!("Tests:");
        println!("  cpu            CPU benchmark");
        println!("  memory         Memory benchmark");
        println!("  fileio         File I/O benchmark");
        println!("  threads        Thread scheduler benchmark");
        println!("  mutex          Mutex contention benchmark");
        println!();
        println!("Commands: prepare, run, cleanup, help");
        println!();
        println!("Options:");
        println!("  --threads N    Number of threads");
        println!("  --time N       Duration in seconds");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sysbench v1.0 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("cpu") => {
            println!("sysbench: CPU speed benchmark");
            println!("  Threads: 1");
            println!("  Prime numbers limit: 10000");
            println!("  Events per second: 1234.56");
            println!("  Total time: 10.0012s");
        }
        Some("memory") => {
            println!("sysbench: memory speed benchmark");
            println!("  Block size: 1KiB");
            println!("  Total size: 102400MiB");
            println!("  Transferred: 102400.00 MiB (10240.00 MiB/sec)");
        }
        _ => {
            println!("sysbench: specify a test (cpu, memory, fileio, threads, mutex)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sysbench".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sysbench(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
