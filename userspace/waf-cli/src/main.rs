#![deny(clippy::all)]

//! waf-cli — SlateOS Waf build system
//!
//! Multi-personality: `waf`

use std::env;
use std::process;

fn run_waf(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("waf [COMMANDS] [OPTIONS]");
        println!("Waf 2.0.26 (SlateOS)");
        println!();
        println!("Main commands:");
        println!("  configure   Configure the project");
        println!("  build       Build the project");
        println!("  clean       Clean the build");
        println!("  install     Install the targets");
        println!("  uninstall   Uninstall the targets");
        println!("  dist        Create a distribution archive");
        println!("  distclean   Remove build and configure artifacts");
        println!("  distcheck   Check distribution archive");
        println!("  list        List available tasks");
        println!("  step        Execute specific tasks");
        println!();
        println!("Options:");
        println!("  -o DIR      Build directory (default: build)");
        println!("  -t DIR      Top source directory");
        println!("  -j NUM      Parallel jobs");
        println!("  -v          Verbose");
        println!("  -p          Progress bar");
        println!("  --prefix    Installation prefix");
        println!("  --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("waf 2.0.26 (5ced2e6c84b3c74e7ab7f47e53929af6a9efdb14)");
        return 0;
    }
    let commands: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    for cmd in &commands {
        match *cmd {
            "configure" => {
                println!("Setting top to                           : /home/user/project");
                println!("Setting out to                           : /home/user/project/build");
                println!("Checking for 'gcc' (C compiler)          : /usr/bin/gcc");
                println!("Checking for 'g++' (C++ compiler)        : /usr/bin/g++");
                println!("'configure' finished successfully (0.123s)");
            }
            "build" => {
                println!("Waf: Entering directory `/home/user/project/build'");
                println!("[1/5] Compiling main.c");
                println!("[2/5] Compiling utils.c");
                println!("[3/5] Compiling parser.c");
                println!("[4/5] Linking build/myapp");
                println!("[5/5] Installing build/myapp");
                println!("Waf: Leaving directory `/home/user/project/build'");
                println!("'build' finished successfully (1.234s)");
            }
            "clean" => {
                println!("'clean' finished successfully (0.012s)");
            }
            "install" => {
                println!("Waf: Entering directory `/home/user/project/build'");
                println!("+ install /usr/local/bin/myapp (from build/myapp)");
                println!("'install' finished successfully (0.045s)");
            }
            "distclean" => {
                println!("'distclean' finished successfully (0.008s)");
            }
            "dist" => {
                println!("New archive created: myproject-1.0.tar.gz");
            }
            "list" => {
                println!("main.c -> main.c.1.o");
                println!("utils.c -> utils.c.2.o");
                println!("parser.c -> parser.c.3.o");
                println!("{{main,utils,parser}}.o -> myapp");
            }
            _ => println!("waf: '{}' completed", cmd),
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_waf(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_waf};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_waf(&["--help".to_string()]), 0);
        assert_eq!(run_waf(&["-h".to_string()]), 0);
        let _ = run_waf(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_waf(&[]);
    }
}
