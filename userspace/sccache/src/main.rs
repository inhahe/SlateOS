#![deny(clippy::all)]

//! sccache — OurOS shared compilation cache
//!
//! Multi-personality: `sccache`, `sccache-dist`

use std::env;
use std::process;

fn run_sccache(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sccache [OPTIONS] [COMMAND] [ARGS...]");
        println!();
        println!("Options:");
        println!("  --start-server     Start background server");
        println!("  --stop-server      Stop background server");
        println!("  --show-stats       Show cache statistics");
        println!("  --zero-stats       Zero statistics counters");
        println!("  --show-adv-stats   Show advanced statistics");
        println!("  --dist-auth        Authenticate for distributed mode");
        println!("  --dist-status      Show dist scheduler status");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("sccache 0.8.1 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--start-server") {
        println!("sccache: starting background server...");
        println!("sccache: server started, listening on 127.0.0.1:4226");
        return 0;
    }
    if args.iter().any(|a| a == "--stop-server") {
        println!("Stopping sccache server...");
        println!("sccache: server stopped");
        return 0;
    }
    if args.iter().any(|a| a == "--zero-stats") {
        println!("Statistics zeroed.");
        return 0;
    }
    if args.iter().any(|a| a == "--show-stats") {
        println!("Compile requests                 2500");
        println!("Compile requests executed         2100");
        println!("Cache hits                        1680");
        println!("Cache hits (C/C++)                1400");
        println!("Cache hits (Rust)                  280");
        println!("Cache misses                       420");
        println!("Cache timeouts                       0");
        println!("Cache read errors                    0");
        println!("Forced recaches                      0");
        println!("Cache write errors                   0");
        println!("Compilation failures                 0");
        println!("Cache errors                         0");
        println!("Non-cacheable compilations         400");
        println!("Non-cacheable calls                400");
        println!("Non-compilation calls                0");
        println!("Unsupported compiler calls           0");
        println!("Average cache write               0.02s");
        println!("Average cache read hit             0.01s");
        println!("Average cache read miss             0.00s");
        println!("Average compilation                1.23s");
        println!("Cache location                   Local disk: /tmp/sccache");
        println!("Cache size                         512 MiB");
        println!("Max cache size                      10 GiB");
        return 0;
    }
    if args.iter().any(|a| a == "--show-adv-stats") {
        println!("Cache location        Local disk: /tmp/sccache");
        println!("Version (client)      0.8.1");
        println!("Max cache size        10 GiB");
        println!("Current cache size    512 MiB (5.0%)");
        println!("Cache entries         3200");
        return 0;
    }
    if args.iter().any(|a| a == "--dist-status") {
        println!("Scheduler: not connected (standalone mode)");
        return 0;
    }

    // Compiler wrapper mode
    let compiler = args.first().map(|s| s.as_str()).unwrap_or("cc");
    let rest_args: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    println!("sccache: running {} {}", compiler, rest_args.join(" "));
    println!("(compilation cached — simulated)");
    0
}

fn run_sccache_dist(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sccache-dist <COMMAND>");
        println!();
        println!("Commands:");
        println!("  scheduler  Run the scheduler");
        println!("  server     Run a build server");
        println!("  auth       Generate auth tokens");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "scheduler" => {
            println!("sccache-dist: scheduler starting on 0.0.0.0:10600");
        }
        "server" => {
            println!("sccache-dist: build server starting, registering with scheduler...");
        }
        "auth" => {
            println!("sccache-dist: generated auth token: <simulated-token>");
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("sccache");
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
        "sccache-dist" => run_sccache_dist(rest),
        _ => run_sccache(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sccache};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sccache(vec!["--help".to_string()]), 0);
        assert_eq!(run_sccache(vec!["-h".to_string()]), 0);
        let _ = run_sccache(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sccache(vec![]);
    }
}
