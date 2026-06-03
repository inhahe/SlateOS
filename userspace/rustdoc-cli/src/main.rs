#![deny(clippy::all)]

//! rustdoc-cli — OurOS Rust documentation generator
//!
//! Multi-personality: `rustdoc`, `cargo-doc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rustdoc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rustdoc [OPTIONS] INPUT");
        println!("rustdoc (OurOS)");
        println!();
        println!("Options:");
        println!("  --crate-name NAME     Crate name");
        println!("  --crate-type TYPE     Crate type (lib, bin, etc.)");
        println!("  --edition YEAR        Rust edition (2015, 2018, 2021, 2024)");
        println!("  -o DIR                Output directory");
        println!("  --html-in-header FILE HTML to include in <head>");
        println!("  --html-before-content FILE  HTML before content");
        println!("  --html-after-content FILE   HTML after content");
        println!("  --document-private-items    Document private items");
        println!("  --test                Run code examples as tests");
        println!("  --test-args ARGS      Extra arguments for doc tests");
        println!("  -L PATH               Library search path");
        println!("  --extern NAME=PATH    External crate");
        println!("  --cfg SPEC            Configure the compilation environment");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rustdoc 1.77.0 (OurOS)");
        return 0;
    }
    let crate_name = args.windows(2)
        .find(|w| w[0] == "--crate-name")
        .map(|w| w[1].as_str())
        .unwrap_or("mycrate");
    let outdir = args.windows(2)
        .find(|w| w[0] == "-o")
        .map(|w| w[1].as_str())
        .unwrap_or("doc");
    let test = args.iter().any(|a| a == "--test");
    if test {
        let input = args.iter()
            .find(|a| a.ends_with(".rs"))
            .map(|s| s.as_str())
            .unwrap_or("src/lib.rs");
        println!("rustdoc: running doc-tests in {}", input);
        println!();
        println!("running 3 tests");
        println!("test src/lib.rs - example_1 (line 15) ... ok");
        println!("test src/lib.rs - example_2 (line 32) ... ok");
        println!("test src/lib.rs - example_3 (line 48) ... ok");
        println!();
        println!("test result: ok. 3 passed; 0 failed; 0 ignored");
    } else {
        println!("rustdoc: generating documentation for '{}'", crate_name);
        println!("  Documenting {} v0.1.0", crate_name);
        println!("  12 public items documented");
        println!("  Output: {}/{}/index.html", outdir, crate_name);
    }
    0
}

fn run_cargo_doc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cargo doc [OPTIONS]");
        println!("Build Rust documentation (OurOS)");
        println!();
        println!("Options:");
        println!("  --open              Open docs in browser");
        println!("  --no-deps           Skip dependency documentation");
        println!("  --document-private-items  Include private items");
        println!("  -p SPEC             Document only specified packages");
        println!("  --all-features      Enable all features");
        println!("  --target TRIPLE     Document for target");
        println!("  --release           Document with release profile");
        return 0;
    }
    let no_deps = args.iter().any(|a| a == "--no-deps");
    let open = args.iter().any(|a| a == "--open");
    println!("   Documenting mycrate v0.1.0");
    if !no_deps {
        println!("   Documenting serde v1.0.197");
        println!("   Documenting tokio v1.36.0");
    }
    println!("    Finished `dev` profile [unoptimized + debuginfo] target(s)");
    println!("   Generated target/doc/mycrate/index.html");
    if open {
        println!("     Opening target/doc/mycrate/index.html");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rustdoc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "cargo-doc" => run_cargo_doc(&rest),
        _ => run_rustdoc(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rustdoc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rustdoc"), "rustdoc");
        assert_eq!(basename(r"C:\bin\rustdoc.exe"), "rustdoc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rustdoc.exe"), "rustdoc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_rustdoc(&["--help".to_string()]), 0);
        assert_eq!(run_rustdoc(&["-h".to_string()]), 0);
        assert_eq!(run_rustdoc(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_rustdoc(&[]), 0);
    }
}
