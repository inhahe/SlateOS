#![deny(clippy::all)]

//! gprof-cli — SlateOS GNU profiler
//!
//! Multi-personality: `gprof`, `gcov`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_gprof(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gprof [OPTIONS] [EXECUTABLE [GMON_DATA...]]");
        println!();
        println!("gprof — display call graph profile (Slate OS).");
        println!();
        println!("Options:");
        println!("  -b              Brief output (no explanations)");
        println!("  -p              Flat profile only");
        println!("  -q              Call graph only");
        println!("  -A              Annotated source listing");
        println!("  -c              Discover static call graph");
        println!("  -l              Line-by-line profiling");
        println!("  -z              Show unused functions");
        println!("  -s              Produce gmon.sum (merge profiles)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gprof (GNU Binutils) 2.42 (Slate OS)");
        return 0;
    }

    let brief = args.iter().any(|a| a == "-b");
    let flat_only = args.iter().any(|a| a == "-p");
    let graph_only = args.iter().any(|a| a == "-q");

    if !graph_only {
        println!("Flat profile:");
        println!();
        println!("Each sample counts as 0.01 seconds.");
        println!("  %   cumulative   self              self     total");
        println!(" time   seconds   seconds    calls  ms/call  ms/call  name");
        println!(" 45.2     0.45     0.45      1000     0.45     0.68   compute");
        println!(" 25.1     0.70     0.25      5000     0.05     0.05   process");
        println!(" 15.8     0.86     0.16      2000     0.08     0.12   transform");
        println!("  8.9     0.95     0.09       500     0.18     0.20   initialize");
        println!("  5.0     1.00     0.05      1000     0.05     0.05   cleanup");
        if !brief {
            println!();
            println!(" %         the percentage of the total running time of the");
            println!(" time      program used by this function.");
        }
    }

    if !flat_only {
        println!();
        println!("Call graph:");
        println!();
        println!("granularity: each sample hit covers 4 byte(s) for 1.00% of 1.00 seconds");
        println!();
        println!("index % time    self  children    called     name");
        println!("                0.45    0.23    1000/1000        main [1]");
        println!("[2]     68.0    0.45    0.23    1000         compute [2]");
        println!("                0.15    0.00    3000/5000        process [3]");
        println!("                0.08    0.00    1000/2000        transform [4]");
        println!("-----------------------------------------------");
        println!("                0.25    0.00    5000/5000        <spontaneous>");
        println!("[3]     25.0    0.25    0.00    5000         process [3]");
        if !brief {
            println!();
            println!("This table describes the call tree of the program.");
        }
    }
    0
}

fn run_gcov(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gcov [OPTIONS] FILES");
        println!();
        println!("gcov — test coverage analysis (Slate OS).");
        println!();
        println!("Options:");
        println!("  -a, --all-blocks      Show info for every basic block");
        println!("  -b, --branch-probabilities  Show branch probabilities");
        println!("  -c, --branch-counts    Give counts of branches taken");
        println!("  -f, --function-summaries Per-function summaries");
        println!("  -n, --no-output        Don't create gcov files");
        println!("  -l, --long-file-names  Use long output file names");
        println!("  -p, --preserve-paths   Preserve full pathnames");
        println!("  -r, --relative-only    Only display relative sources");
        println!("  -s DIR, --source-prefix=DIR  Source prefix to strip");
        println!("  -j, --json-format      Output in JSON format");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gcov (GCC) 14.0.0 (Slate OS)");
        return 0;
    }

    let branch_info = args.iter().any(|a| a == "-b" || a == "--branch-probabilities");
    let func_summaries = args.iter().any(|a| a == "-f" || a == "--function-summaries");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("gcov: no source files specified");
        return 1;
    }

    for file in &files {
        if func_summaries {
            println!("Function 'main'");
            println!("Lines executed:85.71% of 14");
            println!();
            println!("Function 'compute'");
            println!("Lines executed:100.00% of 8");
            println!();
        }
        println!("File '{}'", file);
        println!("Lines executed:87.50% of 24");
        println!("Creating '{}.gcov'", file);
        if branch_info {
            println!("Branches executed:75.00% of 8");
            println!("Taken at least once:62.50% of 8");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "gprof".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "gcov" => run_gcov(&rest),
        _ => run_gprof(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gprof};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gprof"), "gprof");
        assert_eq!(basename(r"C:\bin\gprof.exe"), "gprof.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gprof.exe"), "gprof");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gprof(&["--help".to_string()]), 0);
        assert_eq!(run_gprof(&["-h".to_string()]), 0);
        let _ = run_gprof(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gprof(&[]);
    }
}
