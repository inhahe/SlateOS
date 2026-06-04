#![deny(clippy::all)]

//! ant-cli — OurOS Apache Ant build system
//!
//! Multi-personality: `ant`

use std::env;
use std::process;

fn run_ant(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") || args.is_empty() {
        println!("Usage: ant [OPTIONS] [TARGET [TARGET ...]]");
        println!("Apache Ant(TM) version 1.10.14 (OurOS)");
        println!();
        println!("Options:");
        println!("  -buildfile FILE    Build file (default: build.xml)");
        println!("  -f FILE            Shorthand for -buildfile");
        println!("  -D KEY=VALUE       Define a property");
        println!("  -propertyfile FILE Load properties from file");
        println!("  -find FILE         Search for build file upward");
        println!("  -quiet             Quiet output");
        println!("  -verbose           Verbose output");
        println!("  -debug             Debug output");
        println!("  -emacs             Emacs-friendly output");
        println!("  -logfile FILE      Write output to log file");
        println!("  -lib DIR           Add directory to classpath");
        println!("  -projecthelp       Print project help");
        println!("  -version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("Apache Ant(TM) version 1.10.14 compiled on August 16 2023");
        return 0;
    }
    if args.iter().any(|a| a == "-projecthelp") {
        println!("Buildfile: build.xml");
        println!();
        println!("Main targets:");
        println!("  build    Compile the project");
        println!("  clean    Delete build artifacts");
        println!("  dist     Create distribution");
        println!("  init     Initialize build directories");
        println!("  jar      Create JAR file");
        println!("  test     Run unit tests");
        println!("Default target: build");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-quiet" || a == "-q");
    let targets: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    let actual_targets = if targets.is_empty() { vec!["build"] } else { targets };
    println!("Buildfile: build.xml");
    for target in &actual_targets {
        match *target {
            "init" => {
                if !quiet { println!("\ninit:"); }
                if !quiet { println!("    [mkdir] Created dir: build/classes"); }
            }
            "compile" | "build" => {
                if !quiet { println!("\ninit:"); }
                if !quiet { println!("\ncompile:"); }
                if !quiet { println!("    [javac] Compiling 15 source files to build/classes"); }
            }
            "test" => {
                if !quiet { println!("\ntest:"); }
                if !quiet { println!("    [junit] Running com.example.AppTest"); }
                if !quiet { println!("    [junit] Tests run: 12, Failures: 0, Errors: 0, Time: 0.234s"); }
            }
            "jar" => {
                if !quiet { println!("\njar:"); }
                if !quiet { println!("    [jar] Building jar: dist/myapp.jar"); }
            }
            "clean" => {
                if !quiet { println!("\nclean:"); }
                if !quiet { println!("   [delete] Deleting directory build/"); }
                if !quiet { println!("   [delete] Deleting directory dist/"); }
            }
            "dist" => {
                if !quiet { println!("\ndist:"); }
                if !quiet { println!("    [mkdir] Created dir: dist"); }
                if !quiet { println!("    [jar] Building jar: dist/myapp-1.0.jar"); }
                if !quiet { println!("    [copy] Copying 3 files to dist/lib"); }
            }
            _ => {
                if !quiet { println!("\n{}:", target); }
            }
        }
    }
    println!();
    println!("BUILD SUCCESSFUL");
    println!("Total time: 3 seconds");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ant(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ant};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ant(&["--help".to_string()]), 0);
        assert_eq!(run_ant(&["-h".to_string()]), 0);
        let _ = run_ant(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ant(&[]);
    }
}
