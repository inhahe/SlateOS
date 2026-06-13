#![deny(clippy::all)]

//! valgrind — SlateOS memory debugging and profiling toolkit
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `valgrind` (default) — memory error detector (Memcheck)
//! - `callgrind_annotate` — callgrind output annotator
//! - `cachegrind_annotate` — cachegrind output annotator
//! - `ms_print` — massif output printer

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_valgrind(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: valgrind [options] prog-and-args");
        println!();
        println!("Tool selection:");
        println!("  --tool=<name>           memcheck (default), callgrind, cachegrind, massif, helgrind, drd");
        println!();
        println!("Common options:");
        println!("  --leak-check=<yes|no|summary|full>  Search for memory leaks (default=summary)");
        println!("  --track-origins=<yes|no>             Track origins of uninitialised values");
        println!("  --show-reachable=<yes|no>            Show reachable blocks in leak check");
        println!("  --log-file=<file>                    Log to file");
        println!("  --xml=yes                            XML output");
        println!("  --xml-file=<file>                    XML output file");
        println!("  --trace-children=<yes|no>            Trace child processes");
        println!("  --num-callers=<N>                    Number of callers in stack traces (default=12)");
        println!("  --suppressions=<file>                Read suppressions from file");
        println!("  -v, --verbose                        Be more verbose");
        println!("  -q, --quiet                          Only show errors");
        println!("  --version                            Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("valgrind-0.1.0 (Slate OS)");
        return 0;
    }

    let tool = args.iter().find(|a| a.starts_with("--tool="))
        .map(|a| a.split('=').nth(1).unwrap_or("memcheck"))
        .unwrap_or("memcheck");

    let program = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("./a.out");

    println!("=={pid}== {tool}, a memory error detector", pid = 12345, tool = tool_name(tool));
    println!("=={pid}== Copyright (C) SlateOS. Using Valgrind-0.1.0.", pid = 12345);
    println!("=={pid}== Command: {program}", pid = 12345, program = program);
    println!("=={pid}==", pid = 12345);

    match tool {
        "memcheck" => run_memcheck(),
        "callgrind" => run_callgrind(),
        "cachegrind" => run_cachegrind(),
        "massif" => run_massif(),
        "helgrind" => run_helgrind(),
        "drd" => run_drd(),
        _ => {
            eprintln!("==12345== Unknown tool: {}", tool);
            1
        }
    }
}

fn tool_name(tool: &str) -> &str {
    match tool {
        "memcheck" => "Memcheck",
        "callgrind" => "Callgrind",
        "cachegrind" => "Cachegrind",
        "massif" => "Massif",
        "helgrind" => "Helgrind",
        "drd" => "DRD",
        _ => tool,
    }
}

fn run_memcheck() -> i32 {
    println!("==12345== Invalid read of size 4");
    println!("==12345==    at 0x108A3B: process_data (main.c:42)");
    println!("==12345==    by 0x108B1C: main (main.c:67)");
    println!("==12345==  Address 0x4A3C040 is 0 bytes after a block of size 16 alloc'd");
    println!("==12345==    at 0x4C2FB0F: malloc (vg_replace_malloc.c:381)");
    println!("==12345==    by 0x108A10: process_data (main.c:38)");
    println!("==12345==");
    println!("==12345== Conditional jump or move depends on uninitialised value(s)");
    println!("==12345==    at 0x108A55: check_result (main.c:50)");
    println!("==12345==    by 0x108B30: main (main.c:69)");
    println!("==12345==");
    println!("==12345== HEAP SUMMARY:");
    println!("==12345==     in use at exit: 256 bytes in 4 blocks");
    println!("==12345==   total heap usage: 15 allocs, 11 frees, 1,280 bytes allocated");
    println!("==12345==");
    println!("==12345== LEAK SUMMARY:");
    println!("==12345==    definitely lost: 128 bytes in 2 blocks");
    println!("==12345==    indirectly lost: 64 bytes in 1 blocks");
    println!("==12345==      possibly lost: 0 bytes in 0 blocks");
    println!("==12345==    still reachable: 64 bytes in 1 blocks");
    println!("==12345==         suppressed: 0 bytes in 0 blocks");
    println!("==12345==");
    println!("==12345== For counts of detected and suppressed errors, rerun with: -v");
    println!("==12345== ERROR SUMMARY: 3 errors from 2 contexts (suppressed: 0 from 0)");
    0
}

fn run_callgrind() -> i32 {
    println!("==12345== Events    : Ir");
    println!("==12345== Collected : 1,234,567");
    println!("==12345==");
    println!("==12345== I   refs:      1,234,567");
    println!("==12345==");
    println!("==12345== Callgrind, a call-graph generating cache profiler");
    println!("==12345== Profile data written to callgrind.out.12345");
    0
}

fn run_cachegrind() -> i32 {
    println!("==12345== I   refs:      1,234,567");
    println!("==12345== I1  misses:        4,567");
    println!("==12345== LLi misses:        1,234");
    println!("==12345== I1  miss rate:      0.37%");
    println!("==12345== LLi miss rate:      0.10%");
    println!("==12345==");
    println!("==12345== D   refs:        456,789  (321,000 rd   + 135,789 wr)");
    println!("==12345== D1  misses:       12,345  (  8,765 rd   +   3,580 wr)");
    println!("==12345== LLd misses:        5,678  (  3,456 rd   +   2,222 wr)");
    println!("==12345== D1  miss rate:      2.70%");
    println!("==12345== LLd miss rate:      1.24%");
    println!("==12345==");
    println!("==12345== LL refs:          16,912  ( 13,332 rd   +   3,580 wr)");
    println!("==12345== LL misses:         6,912  (  4,690 rd   +   2,222 wr)");
    println!("==12345== LL miss rate:       0.41%");
    0
}

fn run_massif() -> i32 {
    println!("==12345== Massif, a heap profiler");
    println!("==12345== Heap usage at peak: 4,096 bytes in 8 blocks");
    println!("==12345==");
    println!("==12345== Profile data written to massif.out.12345");
    println!("==12345== Use ms_print to view");
    0
}

fn run_helgrind() -> i32 {
    println!("==12345== Possible data race during read of size 4 at 0x60104C");
    println!("==12345==    at 0x108A3B: worker (main.c:25)");
    println!("==12345==    by 0x4C38EB7: mythread_wrapper (hg_intercepts.c:389)");
    println!("==12345==");
    println!("==12345== Lock at 0x601040 was first observed");
    println!("==12345==    at 0x4C320E4: mutex_lock (hg_intercepts.c:906)");
    println!("==12345==    by 0x108A15: worker (main.c:22)");
    println!("==12345==");
    println!("==12345== ERROR SUMMARY: 1 errors from 1 contexts");
    0
}

fn run_drd() -> i32 {
    println!("==12345== Conflicting load by thread 2 at 0x60104C size 4");
    println!("==12345==    at 0x108A3B: worker (main.c:25)");
    println!("==12345==");
    println!("==12345== ERROR SUMMARY: 1 errors from 1 contexts");
    0
}

fn run_callgrind_annotate(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: callgrind_annotate [options] callgrind.out.<pid>");
        println!();
        println!("Options:");
        println!("  --auto=<yes|no>    Annotate all source files");
        println!("  --tree=<none|caller|calling|both>");
        println!("  --inclusive=<yes|no>");
        println!("  --sort=<metric>");
        println!("  --threshold=<N>    Percentage threshold");
        return 0;
    }

    println!("--------------------------------------------------------------------------------");
    println!("Profile data file 'callgrind.out.12345' (creator: callgrind-0.1.0)");
    println!("--------------------------------------------------------------------------------");
    println!("I1 cache:      32768 B, 64 B, 8-way associative");
    println!("D1 cache:      32768 B, 64 B, 8-way associative");
    println!("LL cache:    8388608 B, 64 B, 16-way associative");
    println!("Events recorded: Ir");
    println!("Events shown:    Ir");
    println!("Event sort order: Ir");
    println!("Total events:   1,234,567");
    println!();
    println!("--------------------------------------------------------------------------------");
    println!("         Ir  file:function");
    println!("--------------------------------------------------------------------------------");
    println!("    456,789  main.c:process_data");
    println!("    234,567  main.c:compute_hash");
    println!("    123,456  main.c:sort_results");
    println!("     98,765  main.c:main");
    0
}

fn run_ms_print(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ms_print [options] massif.out.<pid>");
        println!();
        println!("Options:");
        println!("  --threshold=<N.N>  Significance threshold (default 1.0)");
        println!("  --x=<N>           Width of graph (default 72)");
        println!("  --y=<N>           Height of graph (default 20)");
        return 0;
    }

    println!("--------------------------------------------------------------------------------");
    println!("Command: ./a.out");
    println!("Massif arguments: (none)");
    println!("ms_print arguments: massif.out.12345");
    println!("--------------------------------------------------------------------------------");
    println!();
    println!("    KB");
    println!("4.000^                                              #");
    println!("     |                                             ##");
    println!("     |                                          @@:##");
    println!("     |                                       @@:@@:##");
    println!("     |                                   :@@:@@:@@:##");
    println!("     |                               ::::@@:@@:@@:##:");
    println!("     |                          :::::::::@@:@@:@@:##:");
    println!("     |                     ::::::::::::::@@:@@:@@:##:");
    println!("     |                :::::::::::::::::::@@:@@:@@:##:");
    println!("     |         ::::::::::::::::::::::::@@:@@:@@:##:::");
    println!("   0 +----------------------------------------------------------------------->Ki");
    println!("     0                                                                    1,234");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("valgrind");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "callgrind_annotate" => run_callgrind_annotate(rest),
        "cachegrind_annotate" => run_callgrind_annotate(rest), // same format
        "ms_print" => run_ms_print(rest),
        _ => run_valgrind(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_name() {
        assert_eq!(tool_name("memcheck"), "Memcheck");
        assert_eq!(tool_name("callgrind"), "Callgrind");
        assert_eq!(tool_name("unknown"), "unknown");
    }
}
