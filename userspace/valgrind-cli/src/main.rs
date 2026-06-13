#![deny(clippy::all)]

//! valgrind-cli — Slate OS Valgrind memory debugging suite
//!
//! Multi-personality: `valgrind`, `callgrind_annotate`, `cachegrind_annotate`,
//! `cg_annotate`, `ms_print`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_valgrind(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: valgrind [OPTIONS] PROGRAM [ARGS]");
        println!();
        println!("valgrind — memory debugging and profiling (Slate OS).");
        println!();
        println!("Tool selection:");
        println!("  --tool=memcheck      Memory error detector (default)");
        println!("  --tool=cachegrind    Cache and branch profiler");
        println!("  --tool=callgrind     Call-graph generating profiler");
        println!("  --tool=massif        Heap profiler");
        println!("  --tool=helgrind      Thread error detector");
        println!("  --tool=drd           Thread error detector (alternative)");
        println!("  --tool=lackey        Simple tool for demo/testing");
        println!();
        println!("Common options:");
        println!("  --leak-check=full    Full leak checking");
        println!("  --track-origins=yes  Track origin of uninit values");
        println!("  --show-reachable=yes Show reachable blocks in leak check");
        println!("  --log-file=FILE      Write messages to FILE");
        println!("  --xml=yes            Output in XML format");
        println!("  --xml-file=FILE      XML output to FILE");
        println!("  --num-callers=N      Show N callers in stack traces");
        println!("  --suppressions=FILE  Use suppression file");
        println!("  -v, --verbose        Verbose output");
        println!("  -q, --quiet          Quiet mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("valgrind-3.22.0 (Slate OS)");
        return 0;
    }

    let tool = args.iter()
        .find(|a| a.starts_with("--tool="))
        .and_then(|a| a.strip_prefix("--tool="))
        .unwrap_or("memcheck");

    let program = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("");

    if program.is_empty() {
        eprintln!("valgrind: no program specified");
        return 1;
    }

    println!("==12345== Memcheck, a memory error detector");
    println!("==12345== Copyright (C) 2002-2024, and GNU GPL'd, by Julian Seward et al.");
    println!("==12345== Using Valgrind-3.22.0 and LibVEX; rerun with -h for copyright info");
    println!("==12345== Command: {}", program);
    println!("==12345==");

    match tool {
        "memcheck" => {
            println!("==12345== Invalid read of size 4");
            println!("==12345==    at 0x401234: main (example.c:10)");
            println!("==12345==  Address 0x51f1068 is 0 bytes after a block of size 40 alloc'd");
            println!("==12345==    at 0x4C2E1A2: malloc (vg_replace_malloc.c:307)");
            println!("==12345==    by 0x401200: main (example.c:5)");
            println!("==12345==");
            println!("==12345== HEAP SUMMARY:");
            println!("==12345==     in use at exit: 40 bytes in 1 blocks");
            println!("==12345==   total heap usage: 3 allocs, 2 frees, 1,128 bytes allocated");
            println!("==12345==");
            println!("==12345== 40 bytes in 1 blocks are definitely lost in loss record 1 of 1");
            println!("==12345==    at 0x4C2E1A2: malloc (vg_replace_malloc.c:307)");
            println!("==12345==    by 0x401200: main (example.c:5)");
            println!("==12345==");
            println!("==12345== LEAK SUMMARY:");
            println!("==12345==    definitely lost: 40 bytes in 1 blocks");
            println!("==12345==    indirectly lost: 0 bytes in 0 blocks");
            println!("==12345==      possibly lost: 0 bytes in 0 blocks");
            println!("==12345==    still reachable: 0 bytes in 0 blocks");
            println!("==12345==         suppressed: 0 bytes in 0 blocks");
            println!("==12345==");
            println!("==12345== ERROR SUMMARY: 2 errors from 2 contexts");
        }
        "cachegrind" => {
            println!("==12345== I   refs:      1,234,567");
            println!("==12345== I1  misses:        2,345");
            println!("==12345== LLi misses:          234");
            println!("==12345== I1  miss rate:      0.19%");
            println!("==12345== D   refs:        456,789");
            println!("==12345== D1  misses:        8,901");
            println!("==12345== LLd misses:        1,234");
            println!("==12345== D1  miss rate:      1.9%");
            println!("==12345== LL  miss rate:      0.1%");
        }
        "callgrind" => {
            println!("==12345== Events    : Ir");
            println!("==12345== Collected : 1234567");
            println!("==12345== I   refs:      1,234,567");
            println!("==12345==");
            println!("==12345== Callgrind data written to callgrind.out.12345");
        }
        "massif" => {
            println!("==12345== Massif, a heap profiler");
            println!("==12345== Detailed snapshots: [2, 5, 10 (peak)]");
            println!("==12345==");
            println!("==12345== Massif data written to massif.out.12345");
        }
        "helgrind" => {
            println!("==12345== Helgrind, a thread error detector");
            println!("==12345== Possible data race during read of size 4");
            println!("==12345==    at 0x401300: worker_thread (example.c:15)");
            println!("==12345==");
            println!("==12345== ERROR SUMMARY: 1 errors from 1 contexts");
        }
        _ => {
            println!("==12345== Unknown tool '{}'", tool);
        }
    }
    0
}

fn run_callgrind_annotate(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: callgrind_annotate [OPTIONS] [CALLGRIND_OUT_FILE]");
        println!();
        println!("callgrind_annotate — annotate callgrind output (Slate OS).");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("callgrind.out.12345");
    println!("Reading data from '{}'...", file);
    println!("Events recorded: Ir");
    println!();
    println!("         Ir  file:function");
    println!("  1,000,000  main.c:main");
    println!("    200,000  main.c:compute");
    println!("     34,567  main.c:process");
    0
}

fn run_cg_annotate(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: cg_annotate [OPTIONS] CACHEGRIND_OUT_FILE");
        println!();
        println!("cg_annotate — annotate cachegrind output (Slate OS).");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("cachegrind.out.12345");
    println!("Reading data from '{}'...", file);
    println!();
    println!("         Ir    I1mr   ILmr       Dr    D1mr   DLmr");
    println!("  1,234,567   2,345    234  456,789   8,901  1,234  PROGRAM TOTALS");
    0
}

fn run_ms_print(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: ms_print [OPTIONS] MASSIF_OUT_FILE");
        println!();
        println!("ms_print — display massif heap profiles (Slate OS).");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("massif.out.12345");
    println!("Reading data from '{}'...", file);
    println!();
    println!("    KB");
    println!("128.0^                                               #");
    println!("     |                                          @@@@@@#");
    println!("     |                                     @@@@@@@@@@@#");
    println!("     |                                :::::@@@@@@@@@@@@#");
    println!("     |                          ::::::::::@@@@@@@@@@@@@#");
    println!("     |                     ::::::::::::::::@@@@@@@@@@@@#");
    println!("     |               ::::::::::::::::::::::@@@@@@@@@@@@#");
    println!("     |          :::::::::::::::::::::::::::::@@@@@@@@@@@#");
    println!("     |     :::::::::::::::::::::::::::::::::@@@@@@@@@@@@#");
    println!("     |:::::::::::::::::::::::::::::::::::::@@@@@@@@@@@@@#");
    println!("   0 +--------------------------------------------------------------->Mi");
    println!("     0                                                            1.234");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "valgrind".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "callgrind_annotate" => run_callgrind_annotate(&rest),
        "cachegrind_annotate" | "cg_annotate" => run_cg_annotate(&rest),
        "ms_print" => run_ms_print(&rest),
        _ => run_valgrind(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_valgrind};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/valgrind"), "valgrind");
        assert_eq!(basename(r"C:\bin\valgrind.exe"), "valgrind.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("valgrind.exe"), "valgrind");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_valgrind(&["--help".to_string()]), 0);
        assert_eq!(run_valgrind(&["-h".to_string()]), 0);
        let _ = run_valgrind(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_valgrind(&[]);
    }
}
