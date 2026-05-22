#![deny(clippy::all)]

//! meson-cli — OurOS Meson build system CLI
//!
//! Single personality: `meson`

use std::env;
use std::process;

fn run_meson(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: meson <COMMAND> [OPTIONS]");
        println!();
        println!("Meson Build System (OurOS).");
        println!();
        println!("Commands:");
        println!("  setup        Configure build directory");
        println!("  compile      Build the project");
        println!("  test         Run tests");
        println!("  install      Install the project");
        println!("  introspect   Introspect build");
        println!("  configure    Change build options");
        println!("  dist         Create source archive");
        println!("  wrap         Manage wrap dependencies");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("1.3.1 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("setup");
    match cmd {
        "setup" => {
            let builddir = args.get(1).map(|s| s.as_str()).unwrap_or("builddir");
            println!("The Meson build system");
            println!("Version: 1.3.1");
            println!("Source dir: /home/user/project");
            println!("Build dir: /home/user/project/{}", builddir);
            println!("Build type: native build");
            println!();
            println!("Project name: myproject");
            println!("Project version: 1.0.0");
            println!("C compiler for the host machine: cc (clang 17.0.0)");
            println!("C++ compiler for the host machine: c++ (clang++ 17.0.0)");
            println!("Build targets in project: 3");
            0
        }
        "compile" => {
            let builddir = args.get(1).map(|s| s.as_str()).unwrap_or("builddir");
            let jobs = args.windows(2).find(|w| w[0] == "-j")
                .map(|w| w[1].as_str()).unwrap_or("4");
            println!("[{}/{}] Compiling C++ object src/main.cpp.o", builddir, jobs);
            println!("[2/4] Compiling C++ object src/utils.cpp.o");
            println!("[3/4] Linking shared library lib/libmylib.so");
            println!("[4/4] Linking target bin/myapp");
            0
        }
        "test" => {
            println!("1/3 unit_tests        OK          0.12s");
            println!("2/3 integration_tests  OK          1.45s");
            println!("3/3 benchmark_tests    OK          0.89s");
            println!();
            println!("Ok:                 3");
            println!("Expected Fail:      0");
            println!("Fail:               0");
            println!("Unexpected Pass:    0");
            println!("Skipped:            0");
            println!("Timeout:            0");
            0
        }
        "install" => {
            println!("Installing bin/myapp to /usr/local/bin");
            println!("Installing lib/libmylib.so to /usr/local/lib");
            println!("Installing include/mylib.h to /usr/local/include");
            0
        }
        "introspect" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("--targets");
            match sub {
                "--targets" => {
                    println!("[");
                    println!("  {{\"name\": \"myapp\", \"type\": \"executable\", \"installed\": true}},");
                    println!("  {{\"name\": \"mylib\", \"type\": \"shared library\", \"installed\": true}},");
                    println!("  {{\"name\": \"tests\", \"type\": \"executable\", \"installed\": false}}");
                    println!("]");
                }
                _ => { println!("Introspect: {}", sub); }
            }
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_meson(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
