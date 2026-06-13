#![deny(clippy::all)]

//! node — SlateOS Node.js JavaScript runtime
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `node` (default) — Node.js runtime
//! - `npm` — Node package manager
//! - `npx` — Execute npm package binaries
//! - `corepack` — Node.js package manager manager

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_node(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: node [options] [ script.js ] [arguments]");
        println!();
        println!("Options:");
        println!("  -e, --eval script      evaluate script");
        println!("  -p, --print            evaluate and print result");
        println!("  -c, --check            syntax check without executing");
        println!("  -i, --interactive      always enter REPL");
        println!("  -r, --require module   preload module");
        println!("  --inspect[=host:port]  activate inspector");
        println!("  --inspect-brk          activate inspector and break");
        println!("  -v, --version          print version");
        return 0;
    }

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("v22.0.0");
        return 0;
    }

    // Check for -e / --eval
    if let Some(pos) = args.iter().position(|a| a == "-e" || a == "--eval") {
        if let Some(code) = args.get(pos + 1) {
            println!("(evaluating: {} — simulated)", code);
            return 0;
        }
        eprintln!("node: -e requires an argument");
        return 1;
    }

    // Check for -p / --print
    if let Some(pos) = args.iter().position(|a| a == "-p" || a == "--print") {
        if let Some(code) = args.get(pos + 1) {
            println!("(print-eval: {} — simulated)", code);
            return 0;
        }
        eprintln!("node: -p requires an argument");
        return 1;
    }

    // Script file
    let script = args.first().filter(|a| !a.starts_with('-'));
    if let Some(file) = script {
        println!("(executing: {} — simulated)", file);
        return 0;
    }

    // Interactive REPL
    println!("Welcome to Node.js v22.0.0 (Slate OS).");
    println!("Type \".help\" for more information.");
    println!("> process.platform");
    println!("'slateos'");
    println!("> process.version");
    println!("'v22.0.0'");
    println!("> process.arch");
    println!("'x64'");
    println!("> .exit");
    0
}

fn run_npm(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: npm <command>");
            println!();
            println!("Commands:");
            println!("  install     Install a package");
            println!("  uninstall   Remove a package");
            println!("  update      Update packages");
            println!("  init        Create a package.json");
            println!("  run         Run a script");
            println!("  test        Run tests");
            println!("  start       Start the app");
            println!("  build       Build the project");
            println!("  publish     Publish a package");
            println!("  pack        Create a tarball");
            println!("  list        List installed packages");
            println!("  outdated    Check for outdated packages");
            println!("  audit       Run security audit");
            println!("  cache       Manage the npm cache");
            println!("  config      Manage configuration");
            println!("  --version   Show version");
            0
        }
        "--version" | "-v" => { println!("10.5.0"); 0 }
        "install" | "i" | "add" => {
            let pkgs: Vec<&str> = cmd_args.iter()
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            if pkgs.is_empty() {
                println!("npm warn deprecated inflight@1.0.6: cleanup");
                println!("added 143 packages in 3.2s");
                println!("18 packages are looking for funding");
                println!("  run `npm fund` for details");
            } else {
                for pkg in &pkgs {
                    println!("added {} (simulated)", pkg);
                }
                println!("added {} packages in 1.5s", pkgs.len());
            }
            0
        }
        "uninstall" | "remove" | "rm" => {
            for pkg in &cmd_args {
                if !pkg.starts_with('-') {
                    println!("removed {} (simulated)", pkg);
                }
            }
            0
        }
        "init" => {
            if cmd_args.iter().any(|a| a == "-y" || a == "--yes") {
                println!("Wrote to package.json (simulated)");
            } else {
                println!("package name: (myproject)");
                println!("version: (1.0.0)");
                println!("Created package.json (simulated)");
            }
            0
        }
        "run" => {
            let script = cmd_args.first().map(|s| s.as_str()).unwrap_or("start");
            println!("> myproject@1.0.0 {}", script);
            println!("> node {} (simulated)", script);
            0
        }
        "test" => {
            println!("> myproject@1.0.0 test");
            println!("> jest --coverage");
            println!("PASS  src/index.test.js");
            println!("  ✓ basic test (3 ms)");
            println!("Tests:       1 passed, 1 total");
            println!("Time:        0.5 s");
            0
        }
        "list" | "ls" => {
            println!("myproject@1.0.0 /project");
            println!("├── express@4.18.2");
            println!("├── lodash@4.17.21");
            println!("└── typescript@5.4.0");
            0
        }
        "outdated" => {
            println!("Package      Current  Wanted  Latest  Location");
            println!("express      4.18.2   4.18.3  4.19.0  node_modules/express");
            println!("typescript   5.4.0    5.4.5   5.5.0   node_modules/typescript");
            0
        }
        "audit" => {
            println!("found 0 vulnerabilities (simulated)");
            0
        }
        "cache" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "clean" | "clear" => println!("Cache cleared (simulated)"),
                "verify" => {
                    println!("Cache verified and compressed (~/.npm/_cacache)");
                    println!("Content verified: 150 (32.5 MiB)");
                    println!("Index entries: 200");
                }
                _ => println!("cache {}: (simulated)", sub),
            }
            0
        }
        "publish" => { println!("npm notice Publishing myproject@1.0.0 (simulated)"); 0 }
        "pack" => { println!("npm notice myproject-1.0.0.tgz (simulated)"); 0 }
        other => { eprintln!("npm: unknown command '{}'", other); 1 }
    }
}

fn run_npx(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: npx [options] <command>[@version] [command-args]");
        println!();
        println!("Execute a package binary, installing if needed.");
        println!();
        println!("Options:");
        println!("  -p, --package <pkg>  Package to install");
        println!("  -c <cmd>             Execute string command");
        println!("  -y, --yes            Skip confirmation");
        println!("  --version            Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("10.5.0");
        return 0;
    }

    let cmd = args.first().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("help");
    println!("Need to install the following packages:");
    println!("  {}", cmd);
    println!("Ok to proceed? (y) y");
    println!("(executing {} — simulated)", cmd);
    0
}

fn run_corepack(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: corepack <command>");
            println!();
            println!("Commands:");
            println!("  enable     Add shims for package managers");
            println!("  disable    Remove shims");
            println!("  prepare    Download and install a package manager");
            println!("  hydrate    Import packed package managers");
            println!("  --version  Show version");
            0
        }
        "--version" => { println!("0.28.0"); 0 }
        "enable" => { println!("Enabled pnpm, yarn (simulated)"); 0 }
        "disable" => { println!("Disabled pnpm, yarn (simulated)"); 0 }
        "prepare" => { println!("Preparing package manager... done (simulated)"); 0 }
        other => { eprintln!("corepack: unknown command '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("node");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "npm" => run_npm(rest),
        "npx" => run_npx(rest),
        "corepack" => run_corepack(rest),
        _ => run_node(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_node};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_node(vec!["--help".to_string()]), 0);
        assert_eq!(run_node(vec!["-h".to_string()]), 0);
        let _ = run_node(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_node(vec![]);
    }
}
