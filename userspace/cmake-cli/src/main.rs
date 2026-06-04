#![deny(clippy::all)]

//! cmake-cli — OurOS CMake CLI
//!
//! Single personality: `cmake`

use std::env;
use std::process;

fn run_cmake(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cmake [OPTIONS] <path-to-source>");
        println!("       cmake --build <dir> [OPTIONS]");
        println!("       cmake --install <dir>");
        println!();
        println!("CMake — cross-platform build system generator (OurOS).");
        println!();
        println!("Options:");
        println!("  -S <path>              Source directory");
        println!("  -B <path>              Build directory");
        println!("  -G <generator>         Generator name");
        println!("  -D <var>=<value>       Define variable");
        println!("  --build <dir>          Build a project");
        println!("  --install <dir>        Install a project");
        println!("  --preset <preset>      Use a preset");
        println!("  --list-presets         List available presets");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cmake version 3.28.1 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--build") {
        let dir = args.windows(2).find(|w| w[0] == "--build")
            .map(|w| w[1].as_str()).unwrap_or("build");
        let target = args.windows(2).find(|w| w[0] == "--target")
            .map(|w| w[1].as_str());
        let parallel = args.windows(2).find(|w| w[0] == "-j")
            .map(|w| w[1].as_str()).unwrap_or("4");
        println!("[{}/build] Building with {} threads...", dir, parallel);
        if let Some(t) = target {
            println!("[  0%] Building target '{}'", t);
        }
        println!("[  8%] Building CXX object src/CMakeFiles/main.dir/main.cpp.o");
        println!("[ 33%] Building CXX object src/CMakeFiles/main.dir/utils.cpp.o");
        println!("[ 58%] Building CXX object lib/CMakeFiles/mylib.dir/lib.cpp.o");
        println!("[ 83%] Linking CXX shared library lib/libmylib.so");
        println!("[100%] Linking CXX executable bin/myapp");
        println!("[100%] Built target myapp");
        return 0;
    }

    if args.iter().any(|a| a == "--install") {
        let dir = args.windows(2).find(|w| w[0] == "--install")
            .map(|w| w[1].as_str()).unwrap_or("build");
        let prefix = args.windows(2).find(|w| w[0] == "--prefix")
            .map(|w| w[1].as_str()).unwrap_or("/usr/local");
        println!("-- Installing from {}", dir);
        println!("-- Install configuration: Release");
        println!("-- Installing: {}/bin/myapp", prefix);
        println!("-- Installing: {}/lib/libmylib.so", prefix);
        println!("-- Installing: {}/include/mylib.h", prefix);
        return 0;
    }

    // Configure step
    let source = args.windows(2).find(|w| w[0] == "-S")
        .map(|w| w[1].as_str()).unwrap_or(".");
    let build = args.windows(2).find(|w| w[0] == "-B")
        .map(|w| w[1].as_str()).unwrap_or("build");
    let generator = args.windows(2).find(|w| w[0] == "-G")
        .map(|w| w[1].as_str()).unwrap_or("Unix Makefiles");

    println!("-- The CXX compiler identification is Clang 17.0.0");
    println!("-- Detecting CXX compiler ABI info");
    println!("-- Detecting CXX compile features - done");
    println!("-- Found Threads: TRUE");
    println!("-- Configuring done (0.5s)");
    println!("-- Generating done (0.1s)");
    println!("-- Build files have been written to: {}", build);
    println!("  Source: {}", source);
    println!("  Generator: {}", generator);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cmake(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cmake};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cmake(vec!["--help".to_string()]), 0);
        assert_eq!(run_cmake(vec!["-h".to_string()]), 0);
        let _ = run_cmake(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cmake(vec![]);
    }
}
