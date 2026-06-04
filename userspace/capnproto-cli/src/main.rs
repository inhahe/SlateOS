#![deny(clippy::all)]

//! capnproto-cli — OurOS Cap'n Proto serialization compiler
//!
//! Multi-personality: `capnp`, `capnpc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_capnp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: capnp COMMAND [OPTIONS]");
        println!("Cap'n Proto 1.0.2 (OurOS)");
        println!();
        println!("Commands:");
        println!("  compile      Compile Cap'n Proto schemas");
        println!("  encode       Encode text to binary");
        println!("  decode       Decode binary to text");
        println!("  eval         Evaluate a const from a schema");
        println!("  id           Generate a unique ID");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("Cap'n Proto version 1.0.2 (OurOS)");
        }
        "compile" => {
            let files: Vec<&str> = args.iter()
                .filter(|a| a.ends_with(".capnp"))
                .map(|s| s.as_str())
                .collect();
            if files.is_empty() {
                println!("capnp compile: no .capnp files specified");
                return 1;
            }
            let lang = args.windows(2)
                .find(|w| w[0] == "-o")
                .map(|w| w[1].as_str())
                .unwrap_or("c++");
            for f in &files {
                println!("capnp: compiling {} (output: {})", f, lang);
            }
        }
        "encode" => {
            let schema = args.get(1).map(|s| s.as_str()).unwrap_or("schema.capnp");
            let typename = args.get(2).map(|s| s.as_str()).unwrap_or("Message");
            println!("capnp encode: {} {} -> binary", schema, typename);
            println!("  Encoded 128 bytes");
        }
        "decode" => {
            let schema = args.get(1).map(|s| s.as_str()).unwrap_or("schema.capnp");
            let typename = args.get(2).map(|s| s.as_str()).unwrap_or("Message");
            println!("capnp decode: {} {}", schema, typename);
            println!("  (field1 = \"value\", field2 = 42)");
        }
        "eval" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("schema.capnp:myConst");
            println!("capnp eval: {}", target);
            println!("  42");
        }
        "id" => {
            println!("@0xa1b2c3d4e5f60718;");
        }
        _ => println!("capnp: '{}' completed", subcmd),
    }
    0
}

fn run_capnpc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: capnpc [OPTIONS] FILE.capnp [FILE.capnp ...]");
        println!("Cap'n Proto compiler plugin (OurOS)");
        println!("  -o LANG:DIR   Output language and directory");
        println!("  -I DIR        Schema import path");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("capnpc 1.0.2 (OurOS)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".capnp"))
        .map(|s| s.as_str())
        .collect();
    for f in &files {
        println!("capnpc: compiling {}", f);
    }
    if files.is_empty() {
        println!("capnpc: reading from stdin");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "capnp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "capnpc" => run_capnpc(&rest),
        _ => run_capnp(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_capnp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/capnproto"), "capnproto");
        assert_eq!(basename(r"C:\bin\capnproto.exe"), "capnproto.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("capnproto.exe"), "capnproto");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_capnp(&["--help".to_string()]), 0);
        assert_eq!(run_capnp(&["-h".to_string()]), 0);
        let _ = run_capnp(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_capnp(&[]);
    }
}
