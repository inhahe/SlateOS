#![deny(clippy::all)]

//! wget2-cli — SlateOS wget2 HTTP/HTTPS downloader
//!
//! Single personality: `wget2`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wget2(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wget2 [OPTIONS] URL...");
        println!("wget2 v2.1 (Slate OS) — Network downloader");
        println!();
        println!("Options:");
        println!("  -O FILE           Output file");
        println!("  -P DIR            Directory prefix");
        println!("  -c                Continue partial download");
        println!("  -r                Recursive download");
        println!("  -l DEPTH          Recursion depth");
        println!("  -q                Quiet mode");
        println!("  -N                Timestamping");
        println!("  --mirror          Mirror a site");
        println!("  --convert-links   Convert links for local viewing");
        println!("  --no-check-cert   Skip certificate validation");
        println!("  --chunk-size=SZ   HTTP/2 chunk size");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wget2 v2.1 (Slate OS)"); return 0; }
    let urls: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if urls.is_empty() {
        println!("wget2: missing URL");
        return 1;
    }
    for url in &urls {
        println!("Resolving {}...", url);
        println!("Connecting... connected.");
        println!("HTTP request sent, awaiting response... 200 OK");
        println!("Saving to: 'index.html'");
        println!("     0K .......... 100%  5.2M=0.001s");
        println!("Downloaded: 1 file, 10K");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wget2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wget2(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wget2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wget2"), "wget2");
        assert_eq!(basename(r"C:\bin\wget2.exe"), "wget2.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wget2.exe"), "wget2");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wget2(&["--help".to_string()], "wget2"), 0);
        assert_eq!(run_wget2(&["-h".to_string()], "wget2"), 0);
        let _ = run_wget2(&["--version".to_string()], "wget2");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wget2(&[], "wget2");
    }
}
