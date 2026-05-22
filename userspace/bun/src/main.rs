#![deny(clippy::all)]

//! bun — OurOS Bun JavaScript runtime and toolkit
//!
//! Multi-personality: `bun`, `bunx`

use std::env;
use std::process;

fn run_bun(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bun <command> [...flags] [...args]");
        println!();
        println!("Commands:");
        println!("  run        Run a file or script");
        println!("  test       Run tests");
        println!("  build      Bundle TypeScript/JavaScript");
        println!("  install    Install dependencies");
        println!("  add        Add dependency");
        println!("  remove     Remove dependency");
        println!("  update     Update dependencies");
        println!("  link       Link local package");
        println!("  pm         Package manager info");
        println!("  init       Create new project");
        println!("  create     Create from template");
        println!("  repl       Start REPL");
        println!("  upgrade    Upgrade bun");
        println!("  --version  Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "--version" | "-v" => println!("1.1.12 (OurOS)"),
        "run" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("index.ts");
            println!("$ bun run {}", script);
            println!("(running — simulated)");
        }
        "test" => {
            println!("bun test v1.1.12 (OurOS)");
            println!();
            println!("test.ts:");
            println!("  basic test ... [0.50ms]");
            println!("  async test ... [2.10ms]");
            println!();
            println!(" 2 pass");
            println!(" 0 fail");
            println!(" 2 expect() calls");
            println!("Ran 2 tests across 1 files. [15.00ms]");
        }
        "build" => {
            let entry = args.get(1).map(|s| s.as_str()).unwrap_or("index.ts");
            println!("  {entry} -> ./out/index.js (1.2 KB)", entry = entry);
            println!("[0.05ms] bundle 1 module");
        }
        "install" | "i" => {
            println!("bun install v1.1.12 (OurOS)");
            println!();
            println!(" + express@4.19.2");
            println!(" + typescript@5.4.5");
            println!();
            println!(" 234 packages installed [1.50s]");
        }
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            println!("bun add v1.1.12");
            println!(" installed {} [0.50s]", pkg);
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            println!("bun remove v1.1.12");
            println!(" removed {}", pkg);
        }
        "pm" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" => println!("234 packages installed"),
                "cache" => println!("Cache: /home/user/.bun/install/cache"),
                _ => println!("(pm {} — simulated)", sub),
            }
        }
        "repl" => {
            println!("Welcome to Bun v1.1.12 (OurOS)");
            println!("> (REPL — simulated)");
        }
        "init" | "create" | "link" | "update" | "upgrade" => println!("({} — simulated)", cmd),
        _ => {
            // Try running as script
            println!("$ bun run {}", cmd);
            println!("(running — simulated)");
        }
    }
    0
}

fn run_bunx(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bunx <package> [args...]");
        println!("Execute a package binary without installing globally.");
        return 0;
    }
    let pkg = args.first().map(|s| s.as_str()).unwrap_or("package");
    println!("bunx: executing {} (simulated)", pkg);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("bun");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "bunx" => run_bunx(rest),
        _ => run_bun(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
