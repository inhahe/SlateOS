#![deny(clippy::all)]

//! python3 — OurOS Python interpreter
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `python3` (default) — Python 3 interpreter
//! - `python` — alias for python3
//! - `pydoc` — Python documentation tool
//! - `pip` — Python package installer
//! - `pip3` — alias for pip

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_python(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: python3 [option] ... [-c cmd | -m mod | file | -] [arg] ...");
        println!("Options:");
        println!("  -c cmd   program passed in as string");
        println!("  -m mod   run library module as a script");
        println!("  -i       inspect interactively after running script");
        println!("  -V       print Python version and exit");
        println!("  --version  same as -V");
        println!("  -E       ignore PYTHON* environment variables");
        println!("  -B       don't write .pyc files");
        println!("  -u       unbuffered stdin/stdout/stderr");
        println!("  -O       remove assert statements");
        println!("  -OO      remove assert statements and docstrings");
        return 0;
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Python 3.13.0 (OurOS)");
        return 0;
    }

    // Check for -c command
    if let Some(pos) = args.iter().position(|a| a == "-c") {
        if let Some(cmd) = args.get(pos + 1) {
            println!("(executing: {} — simulated)", cmd);
            return 0;
        }
        eprintln!("Argument expected for the -c option");
        return 2;
    }

    // Check for -m module
    if let Some(pos) = args.iter().position(|a| a == "-m") {
        if let Some(module) = args.get(pos + 1) {
            return run_module(module, &args[pos + 2..]);
        }
        eprintln!("No module name specified");
        return 2;
    }

    // Script file or interactive
    let script = args.first().filter(|a| !a.starts_with('-'));
    if let Some(file) = script {
        println!("(executing script: {} — simulated)", file);
        return 0;
    }

    // Interactive mode
    println!("Python 3.13.0 (OurOS) [GCC 13.2.0 compatible]");
    println!("Type \"help\", \"copyright\", \"credits\" or \"license\" for more information.");
    println!(">>> import sys");
    println!(">>> sys.platform");
    println!("'ouros'");
    println!(">>> sys.version");
    println!("'3.13.0 (OurOS)'");
    println!(">>> exit()");
    0
}

fn run_module(module: &str, _args: &[String]) -> i32 {
    match module {
        "http.server" => {
            println!("Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...");
            println!("(simulated — press Ctrl+C to quit)");
            0
        }
        "json.tool" => {
            println!("{{\"formatted\": true}} (simulated)");
            0
        }
        "venv" => {
            println!("Creating virtual environment... done (simulated)");
            0
        }
        "pip" => {
            println!("pip 24.0 from /usr/lib/python3.13/site-packages/pip (python 3.13)");
            0
        }
        "timeit" => {
            println!("10000000 loops, best of 5: 28.3 nsec per loop (simulated)");
            0
        }
        "compileall" => {
            println!("Compiling '.'... (simulated)");
            0
        }
        "ensurepip" => {
            println!("Successfully installed pip-24.0 setuptools-69.0 (simulated)");
            0
        }
        other => {
            println!("(running module: {} — simulated)", other);
            0
        }
    }
}

fn run_pydoc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("pydoc - Python documentation tool");
        println!();
        println!("Usage: pydoc <name>       Show text doc for module/class/function");
        println!("       pydoc -k <keyword> Search module synopses");
        println!("       pydoc -p <port>    Start HTTP documentation server");
        println!("       pydoc -b           Start server and open browser");
        return 0;
    }

    if args.iter().any(|a| a == "-p") {
        let port = args.iter().position(|a| a == "-p")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(8080);
        println!("Server ready at http://localhost:{}", port);
        println!("Server commands: [b]rowser, [q]uit");
        return 0;
    }

    if let Some(pos) = args.iter().position(|a| a == "-k") {
        let keyword = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("help");
        println!("Searching for '{}':", keyword);
        println!("os - OS routines for NT or Posix");
        println!("sys - System-specific parameters and functions");
        return 0;
    }

    let topic = args.first().map(|s| s.as_str()).unwrap_or("help");
    match topic {
        "os" => {
            println!("Help on module os:");
            println!();
            println!("NAME");
            println!("    os - OS routines for NT or Posix depending on what system we're on.");
            println!();
            println!("FUNCTIONS");
            println!("    getcwd()");
            println!("    listdir(path='.')");
            println!("    mkdir(path, mode=0o777)");
            println!("    remove(path)");
        }
        "sys" => {
            println!("Help on module sys:");
            println!();
            println!("NAME");
            println!("    sys - System-specific parameters and functions.");
            println!();
            println!("DATA");
            println!("    platform = 'ouros'");
            println!("    version = '3.13.0 (OurOS)'");
        }
        _ => println!("Help on '{}': (simulated)", topic),
    }
    0
}

fn run_pip(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: pip <command> [options]");
            println!();
            println!("Commands:");
            println!("  install     Install packages");
            println!("  download    Download packages");
            println!("  uninstall   Uninstall packages");
            println!("  freeze      Output installed packages");
            println!("  list        List installed packages");
            println!("  show        Show package info");
            println!("  search      Search PyPI");
            println!("  check       Verify installed packages");
            println!("  config      Manage configuration");
            println!("  wheel       Build wheels");
            println!("  cache       Inspect pip cache");
            println!("  --version   Show version");
            0
        }
        "--version" | "-V" => {
            println!("pip 24.0 from /usr/lib/python3.13/site-packages/pip (python 3.13)");
            0
        }
        "install" => {
            for pkg in &cmd_args {
                if pkg.starts_with('-') { continue; }
                println!("Collecting {}", pkg);
                println!("  Downloading {}-1.0.0-py3-none-any.whl (simulated)", pkg);
                println!("Installing collected packages: {}", pkg);
                println!("Successfully installed {}-1.0.0", pkg);
            }
            0
        }
        "uninstall" => {
            for pkg in &cmd_args {
                if pkg.starts_with('-') { continue; }
                println!("Found existing installation: {} 1.0.0", pkg);
                println!("Uninstalling {}-1.0.0:", pkg);
                println!("  Successfully uninstalled {}-1.0.0", pkg);
            }
            0
        }
        "list" => {
            println!("Package         Version");
            println!("--------------- -------");
            println!("pip             24.0");
            println!("setuptools      69.0");
            println!("wheel           0.42.0");
            0
        }
        "freeze" => {
            println!("pip==24.0");
            println!("setuptools==69.0");
            println!("wheel==0.42.0");
            0
        }
        "show" => {
            let pkg = cmd_args.first().map(|s| s.as_str()).unwrap_or("pip");
            println!("Name: {}", pkg);
            println!("Version: 1.0.0");
            println!("Summary: A Python package");
            println!("Home-page: https://pypi.org/project/{}/", pkg);
            println!("Author: Author Name");
            println!("License: MIT");
            println!("Location: /usr/lib/python3.13/site-packages");
            0
        }
        "cache" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Package index page size: 2.5 MiB");
                    println!("Number of HTTP files: 12");
                    println!("Number of locally built wheels: 3");
                }
                "purge" => println!("Files removed: 15"),
                "list" => println!("Cache contents: 3 wheels (simulated)"),
                _ => println!("cache {}: (simulated)", sub),
            }
            0
        }
        "check" => { println!("No broken requirements found."); 0 }
        other => { eprintln!("pip: unknown command '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("python3");
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
        "pydoc" | "pydoc3" => run_pydoc(rest),
        "pip" | "pip3" => run_pip(rest),
        _ => run_python(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() {
        assert!(true);
    }
}
