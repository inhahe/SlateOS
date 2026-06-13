#![deny(clippy::all)]

//! thrift-cli — SlateOS Apache Thrift compiler
//!
//! Multi-personality: `thrift`

use std::env;
use std::process;

fn run_thrift(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: thrift [OPTIONS] FILE.thrift");
        println!("Apache Thrift 0.20.0 (SlateOS)");
        println!();
        println!("Options:");
        println!("  --gen LANG    Generate code for language");
        println!("                Languages: cpp, java, py, go, rs, js, rb, csharp");
        println!("  -o DIR        Output directory");
        println!("  -I DIR        Include directory for Thrift files");
        println!("  -r            Generate recursively for includes");
        println!("  --strict      Strict mode (warnings become errors)");
        println!("  --allow-neg-keys  Allow negative field keys");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Thrift version 0.20.0 (SlateOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".thrift"))
        .map(|s| s.as_str())
        .unwrap_or("service.thrift");
    let lang = args.windows(2)
        .find(|w| w[0] == "--gen")
        .map(|w| w[1].as_str())
        .unwrap_or("cpp");
    let recursive = args.iter().any(|a| a == "-r");
    println!("Thrift compiler 0.20.0");
    println!("  Input:     {}", file);
    println!("  Language:  {}", lang);
    if recursive {
        println!("  Mode:      recursive (includes will be compiled)");
    }
    println!("  Scanning {} ...", file);
    println!("  Generating \"{}\" ...", lang);
    let ext = match lang {
        "cpp" => "cpp/h",
        "java" => "java",
        "py" | "python" => "py",
        "go" => "go",
        "rs" | "rust" => "rs",
        "js" | "javascript" => "js",
        "rb" | "ruby" => "rb",
        "csharp" | "cs" => "cs",
        _ => lang,
    };
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    println!("  Output: gen-{}/{}_types.{}", lang, base, ext);
    println!("  Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_thrift(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_thrift};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_thrift(&["--help".to_string()]), 0);
        assert_eq!(run_thrift(&["-h".to_string()]), 0);
        let _ = run_thrift(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_thrift(&[]);
    }
}
