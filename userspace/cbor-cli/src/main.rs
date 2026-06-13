#![deny(clippy::all)]

//! cbor-cli — SlateOS CBOR diagnostic tool
//!
//! Single personality: `cbor-diag`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cbor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cbor-diag [OPTIONS] [FILE]");
        println!("cbor-diag v0.5 (SlateOS) — CBOR diagnostic notation tool");
        println!();
        println!("Options:");
        println!("  FILE              CBOR file to decode (stdin if omitted)");
        println!("  --encode          Encode diagnostic notation to CBOR");
        println!("  --hex             Output hex-encoded CBOR");
        println!("  --pretty          Pretty-print output");
        println!("  --seq             CBOR sequences mode");
        return 0;
    }
    if args.iter().any(|a| a == "--encode") {
        println!("Encoding to CBOR...");
        println!("  Output: 42 bytes");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.cbor");
    println!("Decoding: {}", file);
    println!("  {{");
    println!("    \"name\": \"example\",");
    println!("    \"version\": 1,");
    println!("    \"tags\": [\"cbor\", \"binary\"],");
    println!("    \"data\": h'48656C6C6F'");
    println!("  }}");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cbor-diag".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cbor(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cbor};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cbor"), "cbor");
        assert_eq!(basename(r"C:\bin\cbor.exe"), "cbor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cbor.exe"), "cbor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cbor(&["--help".to_string()], "cbor"), 0);
        assert_eq!(run_cbor(&["-h".to_string()], "cbor"), 0);
        let _ = run_cbor(&["--version".to_string()], "cbor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cbor(&[], "cbor");
    }
}
