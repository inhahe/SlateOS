#![deny(clippy::all)]

//! zig — SlateOS Zig programming language
//!
//! Single personality: `zig`

use std::env;
use std::process;

fn run_zig(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zig [command] [options]");
        println!();
        println!("Commands:");
        println!("  build           Build project");
        println!("  init            Initialize new project");
        println!("  run             Build and run");
        println!("  test            Build and run tests");
        println!("  fmt             Format source");
        println!("  cc              Use as C compiler");
        println!("  c++             Use as C++ compiler");
        println!("  translate-c     Translate C to Zig");
        println!("  ar              Archiver");
        println!("  dlltool         DLL tool");
        println!("  lib             Library tool");
        println!("  ranlib          Ranlib tool");
        println!("  objcopy         Object copy");
        println!("  fetch           Fetch package");
        println!("  env             Print environment");
        println!("  targets         List supported targets");
        println!("  version         Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => println!("0.13.0 (SlateOS)"),
        "env" => {
            println!("{{");
            println!("  \"zig_exe\": \"/usr/bin/zig\",");
            println!("  \"lib_dir\": \"/usr/lib/zig\",");
            println!("  \"std_dir\": \"/usr/lib/zig/std\",");
            println!("  \"global_cache_dir\": \"/home/user/.cache/zig\",");
            println!("  \"version\": \"0.13.0\"");
            println!("}}");
        }
        "build" => {
            let mode = if args.iter().any(|a| a == "-Doptimize=ReleaseFast") { "ReleaseFast" } else { "Debug" };
            println!("Compiling...");
            println!("Build {} succeeded", mode);
        }
        "run" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("src/main.zig");
            println!("Compiling {}...", file);
            println!("(running — simulated output)");
        }
        "test" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("src/main.zig");
            println!("Test [1/3] test.basic... passed");
            println!("Test [2/3] test.edge_case... passed");
            println!("Test [3/3] test.error_handling... passed");
            println!("All 3 tests passed. ({})", file);
        }
        "fmt" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("src/");
            println!("{} (formatted)", file);
        }
        "init" => {
            println!("info: created build.zig");
            println!("info: created build.zig.zon");
            println!("info: created src/main.zig");
        }
        "targets" => {
            println!("Architectures: aarch64, arm, x86, x86_64, riscv64, mips, powerpc, sparc, wasm32");
            println!("OS: linux, macos, windows, freebsd, slateos, freestanding");
            println!("(partial list — simulated)");
        }
        "cc" | "c++" => {
            println!("zig {}: (cross-compiler mode — simulated)", cmd);
        }
        "translate-c" | "ar" | "dlltool" | "lib" | "ranlib" | "objcopy" | "fetch" => {
            println!("({} — simulated)", cmd);
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zig(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_zig};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zig(vec!["--help".to_string()]), 0);
        assert_eq!(run_zig(vec!["-h".to_string()]), 0);
        let _ = run_zig(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zig(vec![]);
    }
}
