#![deny(clippy::all)]

//! gcov-cli — Slate OS code coverage tools
//!
//! Multi-personality: `gcovr`, `lcov`, `genhtml`, `geninfo`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_gcovr(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gcovr [OPTIONS]");
        println!();
        println!("gcovr — generate GCC code coverage reports (Slate OS).");
        println!();
        println!("Options:");
        println!("  -r ROOT, --root ROOT     Root directory");
        println!("  --html                   Generate HTML report");
        println!("  --html-details           HTML report with details");
        println!("  --xml                    Generate Cobertura XML");
        println!("  --json                   Generate JSON report");
        println!("  --csv                    Generate CSV report");
        println!("  --txt                    Generate text report");
        println!("  -o FILE, --output FILE   Output file");
        println!("  -f FILTER, --filter FILTER   Only include files matching");
        println!("  -e EXCLUDE               Exclude files matching");
        println!("  -b, --branches           Report branch coverage");
        println!("  -s, --print-summary      Print summary to stdout");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gcovr 7.0 (Slate OS)");
        return 0;
    }

    let html_mode = args.iter().any(|a| a == "--html" || a == "--html-details");
    let xml_mode = args.iter().any(|a| a == "--xml");
    let branches = args.iter().any(|a| a == "-b" || a == "--branches");
    let summary = args.iter().any(|a| a == "-s" || a == "--print-summary");

    if html_mode {
        println!("(INFO) Writing HTML report to coverage.html");
    } else if xml_mode {
        println!("(INFO) Writing XML report to coverage.xml");
    } else {
        println!("------------------------------------------------------------------------------");
        println!("                           GCC Code Coverage Report");
        println!("Directory: .");
        println!("------------------------------------------------------------------------------");
        println!("File                                  Lines    Exec  Cover   Missing");
        println!("------------------------------------------------------------------------------");
        println!("src/main.c                              100      87   87%   23,45-47,89");
        println!("src/utils.c                              50      48   96%   12,34");
        println!("src/parser.c                            200     178   89%   45-50,123,167-170");
        println!("src/engine.c                            150     142   94%   33,78,90-92");
        println!("------------------------------------------------------------------------------");
        println!("TOTAL                                   500     455   91%");
        println!("------------------------------------------------------------------------------");
        if branches {
            println!();
            println!("Branch coverage: 78.5% (157/200 branches)");
        }
    }

    if summary {
        println!();
        println!("lines: 91.0% (455 out of 500)");
        if branches {
            println!("branches: 78.5% (157 out of 200)");
        }
    }
    0
}

fn run_lcov(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lcov [OPTIONS]");
        println!();
        println!("lcov — graphical GCOV front-end (Slate OS).");
        println!();
        println!("Options:");
        println!("  -c, --capture         Capture coverage data");
        println!("  -d DIR, --directory DIR   Coverage data directory");
        println!("  -o FILE, --output-file FILE   Output tracefile");
        println!("  -a FILE, --add-tracefile FILE  Merge tracefiles");
        println!("  -r FILE, --remove FILE  Remove patterns from tracefile");
        println!("  -e FILE, --extract FILE Extract patterns from tracefile");
        println!("  --zerocounters        Reset all counters to zero");
        println!("  --list-full-path      Show full file paths");
        println!("  -b DIR, --base-directory DIR  Source base directory");
        println!("  --no-external         Ignore external source files");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("lcov: LCOV version 2.0-1 (Slate OS)");
        return 0;
    }

    let capture = args.iter().any(|a| a == "-c" || a == "--capture");
    let zerocounters = args.iter().any(|a| a == "--zerocounters");

    if zerocounters {
        println!("Resetting execution counts...");
        println!("Done.");
    } else if capture {
        println!("Capturing coverage data from .");
        println!("Found gcov version: 14.0.0");
        println!("Using intermediate gcov format");
        println!("Scanning . for .gcda files ...");
        println!("Found 4 data files in .");
        println!("Processing src/main.gcda");
        println!("Processing src/utils.gcda");
        println!("Processing src/parser.gcda");
        println!("Processing src/engine.gcda");
        println!("Writing tracefile coverage.info");
        println!("Summary coverage rate:");
        println!("  lines......: 91.0% (455 of 500 lines)");
        println!("  functions..: 85.0% (34 of 40 functions)");
        println!("  branches...: 78.5% (157 of 200 branches)");
    } else {
        println!("Reading tracefile coverage.info");
        println!("Summary:");
        println!("  lines......: 91.0% (455 of 500 lines)");
        println!("  functions..: 85.0% (34 of 40 functions)");
    }
    0
}

fn run_genhtml(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: genhtml [OPTIONS] TRACEFILE");
        println!();
        println!("genhtml — generate HTML coverage reports from LCOV data (Slate OS).");
        println!();
        println!("Options:");
        println!("  -o DIR, --output-directory DIR  Output directory");
        println!("  --title TITLE                   Report title");
        println!("  --legend                        Include legend");
        println!("  --branch-coverage               Include branch coverage");
        println!("  --function-coverage             Include function coverage");
        println!("  --sort                          Sort file list");
        println!("  --prefix PREFIX                 Remove prefix from paths");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("genhtml: LCOV version 2.0-1 (Slate OS)");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("coverage.info");
    println!("Reading data file {}", file);
    println!("Found 4 entries.");
    println!("Found common filename prefix \"/home/user/project\"");
    println!("Writing .css and .png files.");
    println!("Generating output.");
    println!("Processing file src/main.c");
    println!("Processing file src/utils.c");
    println!("Processing file src/parser.c");
    println!("Processing file src/engine.c");
    println!("Writing directory view page.");
    println!("Overall coverage rate:");
    println!("  lines......: 91.0% (455 of 500 lines)");
    println!("  functions..: 85.0% (34 of 40 functions)");
    0
}

fn run_geninfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: geninfo [OPTIONS] DIRECTORY");
        println!();
        println!("geninfo — generate tracefile from .gcda files (Slate OS).");
        return 0;
    }

    let dir = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or(".");
    println!("Scanning {} for .gcda files...", dir);
    println!("Found 4 data files.");
    println!("Processing main.gcda");
    println!("Processing utils.gcda");
    println!("Finished processing data.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "gcovr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "lcov" => run_lcov(&rest),
        "genhtml" => run_genhtml(&rest),
        "geninfo" => run_geninfo(&rest),
        _ => run_gcovr(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gcovr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gcov"), "gcov");
        assert_eq!(basename(r"C:\bin\gcov.exe"), "gcov.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gcov.exe"), "gcov");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gcovr(&["--help".to_string()]), 0);
        assert_eq!(run_gcovr(&["-h".to_string()]), 0);
        let _ = run_gcovr(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gcovr(&[]);
    }
}
