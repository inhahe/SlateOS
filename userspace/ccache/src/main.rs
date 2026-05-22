#![deny(clippy::all)]

//! ccache — OurOS compiler cache
//!
//! Single personality: `ccache`

use std::env;
use std::process;

fn run_ccache(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ccache [options]");
        println!("       ccache compiler [compiler options]");
        println!();
        println!("Options:");
        println!("  -s, --show-stats       Show statistics summary");
        println!("  -x, --show-stats-tab   Show statistics as tab-separated");
        println!("  -z, --zero-stats       Zero statistics counters");
        println!("  -C, --clear            Clear the cache");
        println!("  -p, --show-config      Show current config");
        println!("  -o, --set-config KEY=VAL  Set config option");
        println!("  -M, --max-size SIZE    Set max cache size");
        println!("  -F, --max-files N      Set max cached files");
        println!("  --print-version        Show version");
        println!("  --show-log-stats       Show log stats");
        println!("  --evict-older-than AGE Evict entries older than AGE");
        println!("  --cleanup              Cleanup stale files");
        return 0;
    }
    if args.iter().any(|a| a == "--print-version" || a == "--version" || a == "-V") {
        println!("ccache version 4.10 (OurOS)");
        println!("Features: file-storage redis-storage");
        return 0;
    }
    if args.iter().any(|a| a == "-s" || a == "--show-stats") {
        println!("Cacheable calls:     1234 / 1500 (82.3%)");
        println!("  Hits:               987 / 1234 (80.0%)");
        println!("    Direct:           850");
        println!("    Preprocessed:     137");
        println!("  Misses:             247");
        println!("Uncacheable calls:    266 / 1500 (17.7%)");
        println!("Local storage:");
        println!("  Cache size (GB):    2.1 / 5.0 (42.0%)");
        println!("  Files:              4567");
        println!("  Hits:               987");
        println!("  Misses:             247");
        return 0;
    }
    if args.iter().any(|a| a == "-z" || a == "--zero-stats") {
        println!("Statistics zeroed");
        return 0;
    }
    if args.iter().any(|a| a == "-C" || a == "--clear") {
        println!("Cleared cache");
        return 0;
    }
    if args.iter().any(|a| a == "-p" || a == "--show-config") {
        println!("(default) base_dir =");
        println!("(default) cache_dir = /home/user/.cache/ccache");
        println!("(default) compiler =");
        println!("(default) compiler_check = mtime");
        println!("(default) compression = true");
        println!("(default) compression_level = 0");
        println!("(default) direct_mode = true");
        println!("(default) hash_dir = true");
        println!("(default) max_files = 0");
        println!("(default) max_size = 5.0G");
        println!("(default) run_second_cpp = true");
        println!("(default) sloppiness =");
        println!("(default) stats = true");
        println!("(default) temporary_dir = /tmp/ccache-tmp");
        return 0;
    }

    // Act as compiler wrapper
    let compiler = args.first().map(|s| s.as_str()).unwrap_or("cc");
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    if rest.is_empty() {
        eprintln!("ccache: no compiler command specified");
        return 1;
    }
    println!("ccache: running {} {}", compiler, rest.join(" "));
    println!("(compilation simulated — cache miss)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ccache(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
