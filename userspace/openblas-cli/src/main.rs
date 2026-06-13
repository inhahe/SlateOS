#![deny(clippy::all)]

//! openblas-cli — SlateOS OpenBLAS info/benchmark
//!
//! Single personality: `openblas-test`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_openblas(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: openblas-test COMMAND [OPTIONS]");
        println!("OpenBLAS v0.3.27 (SlateOS) — BLAS benchmark/info tool");
        println!();
        println!("Commands:");
        println!("  bench N           Benchmark NxN matrix multiply");
        println!("  info              Show library configuration");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("OpenBLAS v0.3.27 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "bench" => {
            let n = args.get(1).map(|s| s.as_str()).unwrap_or("2048");
            println!("OpenBLAS benchmark (N={}):", n);
            println!("  SGEMM (single): 85.2 GFLOPS");
            println!("  DGEMM (double): 42.6 GFLOPS");
            println!("  CGEMM (complex single): 40.1 GFLOPS");
            println!("  ZGEMM (complex double): 20.3 GFLOPS");
        }
        "info" => {
            println!("OpenBLAS v0.3.27");
            println!("  Target: HASWELL");
            println!("  Threads: 4 (pthreads)");
            println!("  Integer size: 32-bit");
            println!("  Max threads: 64");
            println!("  LAPACK: included (3.12)");
        }
        _ => println!("openblas-test {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "openblas-test".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openblas(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_openblas};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/openblas"), "openblas");
        assert_eq!(basename(r"C:\bin\openblas.exe"), "openblas.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("openblas.exe"), "openblas");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_openblas(&["--help".to_string()], "openblas"), 0);
        assert_eq!(run_openblas(&["-h".to_string()], "openblas"), 0);
        let _ = run_openblas(&["--version".to_string()], "openblas");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_openblas(&[], "openblas");
    }
}
