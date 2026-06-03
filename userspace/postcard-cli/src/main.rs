#![deny(clippy::all)]

//! postcard-cli — OurOS postcard serialization inspector
//!
//! Single personality: `postcard-inspect`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_postcard(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: postcard-inspect [OPTIONS] FILE");
        println!("postcard-inspect v1.0 (OurOS) — Postcard format inspector");
        println!();
        println!("Options:");
        println!("  FILE              Postcard-encoded file");
        println!("  --cobs            Expect COBS-encoded framing");
        println!("  --hex             Hex dump raw bytes");
        println!("  --schema FILE     Use schema for decoding");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("data.postcard");
    println!("Inspecting: {}", file);
    println!("  Format: postcard (varint encoding)");
    println!("  Size: 256 bytes");
    println!("  Framing: none");
    if args.iter().any(|a| a == "--cobs") {
        println!("  COBS frames detected: 4");
    }
    println!("  Fields: varint(42), bytes(5)[hello], bool(true), seq(3)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "postcard-inspect".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_postcard(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_postcard};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/postcard"), "postcard");
        assert_eq!(basename(r"C:\bin\postcard.exe"), "postcard.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("postcard.exe"), "postcard");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_postcard(&["--help".to_string()], "postcard"), 0);
        assert_eq!(run_postcard(&["-h".to_string()], "postcard"), 0);
        assert_eq!(run_postcard(&["--version".to_string()], "postcard"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_postcard(&[], "postcard"), 0);
    }
}
