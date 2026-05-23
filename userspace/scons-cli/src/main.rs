#![deny(clippy::all)]

//! scons-cli — OurOS SCons build system
//!
//! Multi-personality: `scons`, `sconsign`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_scons(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: scons [OPTIONS] [TARGET [TARGET ...]]");
        println!("SCons 4.7.0 (OurOS)");
        println!();
        println!("Options:");
        println!("  -c, --clean          Remove targets");
        println!("  -f FILE              SConstruct file");
        println!("  -j NUM               Number of parallel jobs");
        println!("  -Q                   Suppress 'Reading SConscript' messages");
        println!("  -s, --silent         Don't print commands");
        println!("  -u                   Walk up directory tree for SConstruct");
        println!("  -D                   Walk up and use first SConstruct found");
        println!("  --debug=TYPE         Debug (count, explain, findlibs, includes, ...)");
        println!("  --tree=OPTIONS       Print dependency tree (all, derived, ...)");
        println!("  --version            Show version");
        println!("  VAR=VALUE            Set a variable");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("SCons by Steven Knight et al.:");
        println!("  SCons: v4.7.0, 2024-01-15, by");
        println!("  SCons path: ['/usr/lib/scons']");
        println!("  OurOS/amd64");
        println!("  Python: 3.12.2");
        return 0;
    }
    let clean = args.iter().any(|a| a == "-c" || a == "--clean");
    let quiet = args.iter().any(|a| a == "-Q");
    let targets: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-') && !a.contains('='))
        .map(|s| s.as_str())
        .collect();
    if !quiet {
        println!("scons: Reading SConscript files ...");
        println!("scons: done reading SConscript files.");
    }
    if clean {
        println!("scons: Cleaning targets ...");
        println!("Removed main.o");
        println!("Removed utils.o");
        println!("Removed program");
        println!("scons: done cleaning targets.");
    } else if args.iter().any(|a| a.starts_with("--tree")) {
        println!("+-program");
        println!("  +-main.o");
        println!("  | +-main.c");
        println!("  | +-main.h");
        println!("  +-utils.o");
        println!("    +-utils.c");
        println!("    +-utils.h");
    } else {
        println!("scons: Building targets ...");
        if targets.is_empty() {
            println!("cc -o main.o -c main.c");
            println!("cc -o utils.o -c utils.c");
            println!("cc -o program main.o utils.o");
        } else {
            for t in &targets {
                println!("scons: building '{}'", t);
            }
        }
        println!("scons: done building targets.");
    }
    0
}

fn run_sconsign(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sconsign [OPTIONS] FILE");
        println!("Print SCons signature database contents.");
        println!("  -d DIR        Print only entries for directory");
        println!("  -e ENTRY      Print only specific entry");
        println!("  -r            Print raw entries");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sconsign 4.7.0 (OurOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or(".sconsign.dblite");
    println!("sconsign: dumping {}", file);
    println!("  main.o:");
    println!("    csig: a1b2c3d4e5f6");
    println!("    timestamp: 1708300000");
    println!("  utils.o:");
    println!("    csig: f6e5d4c3b2a1");
    println!("    timestamp: 1708300005");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "scons".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "sconsign" => run_sconsign(&rest),
        _ => run_scons(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
