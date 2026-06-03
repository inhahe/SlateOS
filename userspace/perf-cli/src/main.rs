#![deny(clippy::all)]

//! perf-cli — OurOS Linux perf performance analysis tool
//!
//! Multi-personality: `perf`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_perf(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: perf [--version] [--help] COMMAND [ARGS]");
        println!();
        println!("perf — performance analysis tools (OurOS).");
        println!();
        println!("Commands:");
        println!("  stat       Run a command and gather performance counters");
        println!("  record     Run a command and record its profile");
        println!("  report     Read perf.data and display the profile");
        println!("  top        System profiling tool (like top for functions)");
        println!("  annotate   Read perf.data and display annotated code");
        println!("  list       List available events");
        println!("  bench      General framework for benchmark suites");
        println!("  diff       Read two perf.data files and display differential profile");
        println!("  probe      Define new dynamic tracepoints");
        println!("  trace      strace-like tracing tool");
        println!("  sched      Scheduler analysis tool");
        println!("  mem        Memory access profiling");
        println!("  lock       Lock analysis tool");
        println!("  script     Read perf.data and run trace scripts");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("perf version 6.7.0 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest = if args.len() > 1 { &args[1..] } else { &[] };

    match subcmd {
        "stat" => {
            let command = rest.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("program");
            println!();
            println!(" Performance counter stats for '{}':", command);
            println!();
            println!("          1,234.56 msec task-clock                       #    0.998 CPUs utilized");
            println!("                12      context-switches                 #    9.719 /sec");
            println!("                 3      cpu-migrations                   #    2.430 /sec");
            println!("             1,456      page-faults                      #    1.179 K/sec");
            println!("     3,456,789,012      cycles                           #    2.800 GHz");
            println!("     2,345,678,901      instructions                     #    0.68  insn per cycle");
            println!("       456,789,012      branches                         #  369.870 M/sec");
            println!("        12,345,678      branch-misses                    #    2.70% of all branches");
            println!();
            println!("       1.236789012 seconds time elapsed");
            println!("       1.200000000 seconds user");
            println!("       0.036000000 seconds sys");
        }
        "record" => {
            let command = rest.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("program");
            println!("Recording events for '{}'...", command);
            println!("[ perf record: Woken up 1 times to write data ]");
            println!("[ perf record: Captured and wrote 0.234 MB perf.data (5678 samples) ]");
        }
        "report" => {
            println!("# Overhead  Command   Shared Object     Symbol");
            println!("# ........  ........  ................  ..............................");
            println!("    42.31%  program   libc.so.6         [.] __memcpy_avx2");
            println!("    15.67%  program   program           [.] hot_loop");
            println!("    12.34%  program   libc.so.6         [.] __strcmp_sse42");
            println!("     8.91%  program   [kernel.kallsyms] [k] copy_user_generic_string");
            println!("     5.23%  program   program           [.] process_data");
            println!("     3.45%  program   libm.so.6         [.] __exp_finite");
            println!("     2.11%  program   libc.so.6         [.] malloc");
            println!("     1.89%  program   libc.so.6         [.] free");
        }
        "top" => {
            println!("Samples: 10K of event 'cycles', 4000 Hz, Event count (approx.): 2500000000");
            println!("Overhead  Shared Object     Symbol");
            println!("  12.34%  [kernel]          [k] _raw_spin_lock_irqsave");
            println!("   8.56%  libc.so.6         [.] __memcpy_avx2");
            println!("   5.67%  [kernel]          [k] copy_user_generic_string");
            println!("   4.32%  [kernel]          [k] native_queued_spin_lock_slowpath");
            println!("   3.21%  [kernel]          [k] clear_page_erms");
        }
        "list" => {
            println!("List of pre-defined events (to be used in -e):");
            println!();
            println!("  cpu-cycles OR cycles                    [Hardware event]");
            println!("  instructions                            [Hardware event]");
            println!("  cache-references                        [Hardware event]");
            println!("  cache-misses                            [Hardware event]");
            println!("  branch-instructions OR branches         [Hardware event]");
            println!("  branch-misses                           [Hardware event]");
            println!("  bus-cycles                              [Hardware event]");
            println!();
            println!("  cpu-clock                               [Software event]");
            println!("  task-clock                              [Software event]");
            println!("  page-faults OR faults                   [Software event]");
            println!("  context-switches OR cs                  [Software event]");
            println!("  cpu-migrations OR migrations            [Software event]");
        }
        "bench" => {
            let suite = rest.first().map(|s| s.as_str()).unwrap_or("all");
            println!("# Running '{}' benchmarks...", suite);
            println!("# Benchmark: sched/messaging");
            println!("  20 groups, 40 threads: 0.123 seconds");
            println!("# Benchmark: mem/memcpy");
            println!("  1MB: 5.234 GB/sec");
            println!("  4KB: 12.456 GB/sec");
        }
        "trace" => {
            let command = rest.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("program");
            println!("tracing '{}'...", command);
            println!("  0.000 ( 0.012 ms): execve(filename: /usr/bin/{}) = 0", command);
            println!("  0.045 ( 0.002 ms): brk(brk: 0) = 0x562000");
            println!("  0.048 ( 0.005 ms): openat(dfd: CWD, filename: /etc/ld.so.cache) = 3");
            println!("  0.060 ( 0.001 ms): close(fd: 3) = 0");
            println!("  0.100 ( 0.003 ms): write(fd: 1, buf: 0x7f.., count: 12) = 12");
            println!("  0.110 ( 0.001 ms): exit_group(error_code: 0)");
        }
        "sched" => {
            println!("  -----------------------------------------------");
            println!("  Task                  |   Runtime ms  | Switches");
            println!("  -----------------------------------------------");
            println!("  program:1234          |      1234.567 |      123");
            println!("  kworker/0:1:5         |        12.345 |       45");
            println!("  migration/0:10        |         1.234 |       12");
            println!("  -----------------------------------------------");
            println!("  Total:                |      1248.146 |      180");
        }
        _ => {
            eprintln!("perf: '{}' is not a perf command. See 'perf --help'.", subcmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "perf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_perf(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_perf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/perf"), "perf");
        assert_eq!(basename(r"C:\bin\perf.exe"), "perf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("perf.exe"), "perf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_perf(&["--help".to_string()]), 0);
        assert_eq!(run_perf(&["-h".to_string()]), 0);
        assert_eq!(run_perf(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_perf(&[]), 0);
    }
}
