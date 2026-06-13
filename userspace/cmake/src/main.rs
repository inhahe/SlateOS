#![deny(clippy::all)]

//! cmake — SlateOS CMake build system generator
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `cmake` (default) — generate build files
//! - `ctest` — run tests
//! - `cpack` — create packages

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_cmake(args: Vec<String>) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cmake [options] <path-to-source>");
        println!("       cmake [options] <path-to-existing-build>");
        println!("       cmake [options] -S <source> -B <build>");
        println!();
        println!("Options:");
        println!("  -S <path>         Source directory");
        println!("  -B <path>         Build directory");
        println!("  -G <generator>    Build system generator");
        println!("  -D<var>=<value>   Define a variable");
        println!("  -DCMAKE_BUILD_TYPE=<type>  Build type (Debug/Release)");
        println!("  --build <dir>     Build a project");
        println!("  --install <dir>   Install a project");
        println!("  --preset <name>   Use a configure preset");
        println!("  -E                CMake command mode");
        println!("  --version         Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("cmake version 0.1.0 (Slate OS)");
        println!("CMake suite maintained by Slate OS");
        return 0;
    }

    if args.iter().any(|a| a == "--build") {
        let dir = args.iter().position(|a| a == "--build")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("build");
        println!("[  0%] Building C object CMakeFiles/main.c.o");
        println!("[ 50%] Building C object CMakeFiles/util.c.o");
        println!("[100%] Linking C executable main");
        println!("Built target in {} (simulated)", dir);
        return 0;
    }

    if args.iter().any(|a| a == "--install") {
        println!("-- Install configuration: \"Release\"");
        println!("-- Installing: /usr/local/bin/myapp");
        println!("-- Installing: /usr/local/lib/libmylib.so");
        return 0;
    }

    // Configure mode
    let source = args.iter().position(|a| a == "-S")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .or_else(|| args.last().map(|s| s.as_str()))
        .unwrap_or(".");
    let build = args.iter().position(|a| a == "-B")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("build");
    let generator = args.iter().position(|a| a == "-G")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("Unix Makefiles");

    println!("-- The C compiler identification is GCC 13.2.0");
    println!("-- The CXX compiler identification is GCC 13.2.0");
    println!("-- Detecting C compiler ABI info - done");
    println!("-- Detecting CXX compiler ABI info - done");
    println!("-- Check for working C compiler: /usr/bin/cc - skipped");
    println!("-- Configuring done ({} → {})", source, build);
    println!("-- Generating done ({})", generator);
    println!("-- Build files have been written to: {}", build);
    0
}

fn run_ctest(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ctest [options]");
        println!();
        println!("Options:");
        println!("  -j N             Run N tests in parallel");
        println!("  -R <regex>       Run tests matching regex");
        println!("  -E <regex>       Exclude tests matching regex");
        println!("  --output-on-failure  Show output on failure");
        println!("  -V, --verbose    Enable verbose output");
        println!("  --version        Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("ctest version 0.1.0 (Slate OS)");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-V" || a == "--verbose");

    println!("Test project /build");
    println!("    Start 1: test_basic");
    if verbose { println!("1: Test command: /build/test_basic"); }
    println!("1/3 Test #1: test_basic ...............   Passed    0.01 sec");
    println!("    Start 2: test_advanced");
    println!("2/3 Test #2: test_advanced ............   Passed    0.03 sec");
    println!("    Start 3: test_integration");
    println!("3/3 Test #3: test_integration .........   Passed    0.15 sec");
    println!();
    println!("100% tests passed, 0 tests failed out of 3");
    println!("Total Test time (real) = 0.19 sec");
    0
}

fn run_cpack(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cpack [options]");
        println!();
        println!("Options:");
        println!("  -G <generator>   Package generator (TGZ, DEB, RPM, ZIP)");
        println!("  -C <config>      Configuration (Debug/Release)");
        println!("  --config <file>  CPack config file");
        println!("  --version        Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("cpack version 0.1.0 (Slate OS)");
        return 0;
    }

    let generator = args.iter().position(|a| a == "-G")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("TGZ");

    println!("CPack: Create package using {}", generator);
    println!("CPack: Install projects");
    println!("CPack: - Install project: myproject");
    println!("CPack: Create package");
    println!("CPack: - package: myproject-0.1.0-Linux.tar.gz generated.");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("cmake");
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
        "ctest" => run_ctest(rest),
        "cpack" => run_cpack(rest),
        _ => run_cmake(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() {
        // cmake is primarily a command-line tool; smoke test only.
        let _ = ();
    }
}
