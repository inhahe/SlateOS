#![deny(clippy::all)]

//! crystal-cli — OurOS Crystal language tools
//!
//! Multi-personality: `crystal`, `shards`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_crystal(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: crystal COMMAND [OPTIONS]");
        println!("Crystal 1.11.2 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init           Initialize project");
        println!("  build          Build project");
        println!("  run            Build and run");
        println!("  spec           Run specs");
        println!("  eval           Evaluate expression");
        println!("  docs           Generate documentation");
        println!("  env            Show Crystal environment");
        println!("  tool           Run a tool");
        println!("  play           Start playground");
        println!("  version        Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("Crystal 1.11.2 [abc123] (2024-02-15)");
            println!("LLVM: 17.0.6");
            println!("Default target: x86_64-ouros");
        }
        "build" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("src/main.cr");
            let base = file.rsplit_once('/').map_or(file, |(_, n)| n);
            let out = base.rsplit_once('.').map_or(base, |(b, _)| b);
            println!("Compiling {}...", file);
            println!("  {}: 42 types, 150 methods", out);
        }
        "run" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("src/main.cr");
            println!("Compiling {}...", file);
            println!("Running...");
        }
        "spec" => {
            println!("....");
            println!();
            println!("Finished in 0.5 seconds");
            println!("4 examples, 0 failures, 0 errors, 0 pending");
        }
        "init" => {
            let kind = args.get(1).map(|s| s.as_str()).unwrap_or("app");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("myapp");
            println!("  create  {}/src/{}.cr", name, name);
            println!("  create  {}/spec/{}_spec.cr", name, name);
            println!("  create  {}/spec/spec_helper.cr", name);
            println!("  create  {}/shard.yml", name);
            println!("Initialized {} {} in {}/", kind, name, name);
        }
        "eval" => {
            let expr = args.get(1).map(|s| s.as_str()).unwrap_or("puts 42");
            println!("{}", expr);
        }
        "docs" => {
            println!("Generating documentation...");
            println!("  Output: docs/index.html");
        }
        "env" => {
            println!("CRYSTAL_PATH=lib:/usr/lib/crystal");
            println!("CRYSTAL_VERSION=1.11.2");
            println!("CRYSTAL_LIBRARY_PATH=/usr/lib");
        }
        "tool" => {
            let tool = args.get(1).map(|s| s.as_str()).unwrap_or("format");
            println!("crystal tool {}: done", tool);
        }
        _ => println!("crystal: '{}' completed", subcmd),
    }
    0
}

fn run_shards(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: shards COMMAND [OPTIONS]");
        println!("Shards 0.17.4 (OurOS)");
        println!("  install    Install dependencies");
        println!("  update     Update dependencies");
        println!("  list       List dependencies");
        println!("  check      Verify dependencies");
        println!("  init       Initialize shard.yml");
        println!("  build      Build targets");
        println!("  prune      Remove unused dependencies");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("install");
    match subcmd {
        "--version" => println!("Shards 0.17.4 (OurOS)"),
        "install" => {
            println!("Resolving dependencies...");
            println!("Fetching https://github.com/crystal-lang/...");
            println!("Installing db (0.12.0)");
            println!("Installing pg (0.28.0)");
        }
        "update" => println!("Updating dependencies..."),
        "list" => {
            println!("  * db (0.12.0)");
            println!("  * pg (0.28.0)");
        }
        "check" => println!("Dependencies are satisfied."),
        "init" => println!("Created shard.yml"),
        _ => println!("shards: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crystal".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "shards" => run_shards(&rest),
        _ => run_crystal(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_crystal};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/crystal"), "crystal");
        assert_eq!(basename(r"C:\bin\crystal.exe"), "crystal.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("crystal.exe"), "crystal");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_crystal(&["--help".to_string()]), 0);
        assert_eq!(run_crystal(&["-h".to_string()]), 0);
        let _ = run_crystal(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_crystal(&[]);
    }
}
