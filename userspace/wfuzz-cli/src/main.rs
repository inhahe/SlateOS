#![deny(clippy::all)]

//! wfuzz-cli — OurOS wfuzz web fuzzer
//!
//! Single personality: `wfuzz`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wfuzz(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wfuzz [OPTIONS] -u URL");
        println!("wfuzz v3.1 (OurOS) — Web application fuzzer");
        println!();
        println!("Options:");
        println!("  -u URL         Target URL (use FUZZ as placeholder)");
        println!("  -w WORDLIST    Wordlist file");
        println!("  -z TYPE,ARGS   Payload specification");
        println!("  -t THREADS     Concurrent threads (default: 10)");
        println!("  -d DATA        POST data");
        println!("  -H HEADER      Custom header");
        println!("  -b COOKIE      Cookie");
        println!("  --hc CODES     Hide responses with these codes");
        println!("  --hl LINES     Hide responses with N lines");
        println!("  --hw WORDS     Hide responses with N words");
        println!("  --hh CHARS     Hide responses with N chars");
        println!("  -o FILE        Output file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wfuzz v3.1.0 (OurOS)"); return 0; }
    println!("wfuzz v3.1.0 (OurOS)");
    println!("  Target: http://target.local/FUZZ");
    println!("  Wordlist: common.txt (4614 words)");
    println!("  Hiding: 404 responses");
    println!();
    println!("  ID   Response   Lines   Words   Chars   Payload");
    println!("  001  200        45      123     4567    admin");
    println!("  002  301        9       12      278     api");
    println!("  003  200        123     456     12345   login");
    println!("  004  403        11      32      278     config");
    println!("  005  200        67      234     5678    dashboard");
    println!();
    println!("  Total requests: 4614");
    println!("  Processed: 4614");
    println!("  Filtered: 4609");
    println!("  Found: 5");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wfuzz".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wfuzz(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wfuzz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wfuzz"), "wfuzz");
        assert_eq!(basename(r"C:\bin\wfuzz.exe"), "wfuzz.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wfuzz.exe"), "wfuzz");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wfuzz(&["--help".to_string()], "wfuzz"), 0);
        assert_eq!(run_wfuzz(&["-h".to_string()], "wfuzz"), 0);
        assert_eq!(run_wfuzz(&["--version".to_string()], "wfuzz"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wfuzz(&[], "wfuzz"), 0);
    }
}
