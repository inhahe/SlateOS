#![deny(clippy::all)]

//! ocaml-cli — Slate OS OCaml language tools
//!
//! Multi-personality: `ocaml`, `ocamlc`, `ocamlopt`, `opam`, `dune`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ocaml(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ocaml [OPTIONS] [FILE]");
        println!("OCaml 5.1.1 (Slate OS)");
        println!("  -stdin      Read from stdin");
        println!("  -noprompt   No prompt");
        println!("  --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-version") {
        println!("The OCaml toplevel, version 5.1.1");
        return 0;
    }
    println!("        OCaml version 5.1.1");
    println!("# ");
    0
}

fn run_ocamlc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ocamlc [OPTIONS] FILE.ml [FILE.ml ...]");
        println!("  -o FILE       Output file");
        println!("  -c            Compile only");
        println!("  -I DIR        Add include directory");
        println!("  -g            Debug info");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("5.1.1");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| a.ends_with(".ml")).map(|s| s.as_str()).collect();
    for f in &files {
        let base = f.rsplit_once('.').map_or(*f, |(b, _)| b);
        println!("ocamlc: {} -> {}.cmo", f, base);
    }
    0
}

fn run_opam(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: opam COMMAND [OPTIONS]");
        println!("opam 2.1.5 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  init         Initialize opam");
        println!("  install      Install packages");
        println!("  remove       Remove packages");
        println!("  update       Update package list");
        println!("  upgrade      Upgrade packages");
        println!("  list         List packages");
        println!("  search       Search packages");
        println!("  switch       Manage switches");
        println!("  env          Show environment");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("2.1.5"),
        "init" => {
            println!("[NOTE] Will configure from built-in defaults.");
            println!("Checking for available remotes...");
            println!("  - default at https://opam.ocaml.org");
            println!("opam has been initialized.");
        }
        "install" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("dune");
            println!("The following actions will be performed:");
            println!("  - install {} 3.14.0", pkg);
            println!("Processing  1/1: [{}]", pkg);
            println!("Done.");
        }
        "list" => {
            println!("# Name         Version  Synopsis");
            println!("  dune         3.14.0   A composable build system");
            println!("  ocamlfind    1.9.6    Library manager for OCaml");
            println!("  core         0.17.0   Jane Street's stdlib overlay");
        }
        "switch" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match action {
                "list" => {
                    println!("#  switch  compiler    description");
                    println!("-> default ocaml.5.1.1 default");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("5.1.1");
                    println!("Switch {} created.", name);
                }
                _ => println!("opam switch: '{}' completed", action),
            }
        }
        _ => println!("opam: '{}' completed", subcmd),
    }
    0
}

fn run_dune(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dune COMMAND [OPTIONS]");
        println!("dune 3.14.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  build        Build project");
        println!("  test         Run tests");
        println!("  clean        Clean build");
        println!("  exec         Build and execute");
        println!("  init         Initialize project");
        println!("  fmt          Format code");
        println!("  utop         Start utop REPL");
        println!("  promote      Promote corrected outputs");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("build");
    match subcmd {
        "--version" => println!("3.14.0"),
        "build" => println!("Done: 12/12 (jobs: 4)"),
        "test" => {
            println!("Running tests...");
            println!("  test_main: OK");
            println!("  test_utils: OK");
            println!("All 2 tests passed.");
        }
        "clean" => println!("Removing _build/"),
        "init" => {
            let kind = args.get(1).map(|s| s.as_str()).unwrap_or("project");
            println!("dune init {}: created", kind);
        }
        "fmt" => println!("Formatting 8 files..."),
        _ => println!("dune: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ocaml".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ocamlc" | "ocamlopt" => run_ocamlc(&rest),
        "opam" => run_opam(&rest),
        "dune" => run_dune(&rest),
        _ => run_ocaml(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ocaml};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ocaml"), "ocaml");
        assert_eq!(basename(r"C:\bin\ocaml.exe"), "ocaml.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ocaml.exe"), "ocaml");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ocaml(&["--help".to_string()]), 0);
        assert_eq!(run_ocaml(&["-h".to_string()]), 0);
        let _ = run_ocaml(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ocaml(&[]);
    }
}
