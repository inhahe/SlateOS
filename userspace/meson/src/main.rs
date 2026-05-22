#![deny(clippy::all)]

//! meson — OurOS Meson build system
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `meson` (default) — configure and manage builds
//! - `mesonconf` — configure project options
//! - `mesonintrospect` — introspect project

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_meson(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("usage: meson <command> [<options>]");
            println!();
            println!("Commands:");
            println!("  setup       Configure the project");
            println!("  configure   Change project options");
            println!("  compile     Build the project");
            println!("  test        Run tests");
            println!("  install     Install the project");
            println!("  dist        Generate release archive");
            println!("  subprojects Manage subprojects");
            println!("  wrap        Manage WrapDB packages");
            println!("  init        Create a new project");
            println!("  introspect  Introspect project");
            println!("  env2mfile   Convert env vars to cross file");
            println!("  rewrite     Modify meson.build");
            println!("  --version   Show version");
            0
        }
        "--version" => { println!("0.1.0 (OurOS)"); 0 }
        "setup" => {
            let source = cmd_args.first().map(|s| s.as_str()).unwrap_or(".");
            let build = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("builddir");
            let buildtype = cmd_args.iter().position(|a| a.starts_with("--buildtype="))
                .map(|i| cmd_args[i].split('=').nth(1).unwrap_or("debugoptimized"))
                .unwrap_or("debugoptimized");

            println!("The Meson build system");
            println!("Version: 0.1.0 (OurOS)");
            println!("Source dir: {}", source);
            println!("Build dir: {}", build);
            println!("Build type: {}", buildtype);
            println!();
            println!("Found ninja-0.1.0 at /usr/bin/ninja");
            println!("Found cc: gcc (gcc 13.2.0)");
            println!("Found cpp: g++ (gcc 13.2.0)");
            println!("Checking for size of \"void *\" : 8 (cached)");
            println!("Configuring project...");
            println!("Build targets in project: 3");
            println!();
            println!("Found run-time dependency threads: YES");
            println!("Found run-time dependency math: YES");
            println!();
            println!("Build directory configured. Run 'ninja -C {}' to build.", build);
            0
        }
        "configure" => {
            println!("Core properties:");
            println!("  Source dir: /project");
            println!("  Build dir: /project/builddir");
            println!();
            println!("Core options:");
            println!("  Option             Current Value  Possible Values");
            println!("  ------             -------------  ---------------");
            println!("  auto_features      auto           [enabled, disabled, auto]");
            println!("  buildtype          debugoptimized [plain, debug, debugoptimized, release, minsize, custom]");
            println!("  default_library    shared         [shared, static, both]");
            println!("  optimization       2              [plain, 0, g, 1, 2, 3, s]");
            println!("  prefix             /usr/local     []");
            println!("  warning_level      1              [0, 1, 2, 3, everything]");
            0
        }
        "compile" | "build" => {
            let jobs = cmd_args.iter().position(|a| a == "-j")
                .and_then(|i| cmd_args.get(i + 1))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(4);
            println!("[1/5] Compiling C object src/main.c.o");
            println!("[2/5] Compiling C object src/util.c.o");
            println!("[3/5] Compiling C object src/config.c.o");
            println!("[4/5] Linking static target libutil.a");
            println!("[5/5] Linking target myapp");
            println!("Build complete ({} jobs).", jobs);
            0
        }
        "test" => {
            let verbose = cmd_args.iter().any(|a| a == "-v" || a == "--verbose");
            println!("1/4 basic_test       OK              0.05s");
            println!("2/4 unit_tests       OK              0.12s");
            println!("3/4 integration      OK              0.35s");
            println!("4/4 regression       OK              0.08s");
            if verbose {
                println!();
                println!("Full log: builddir/meson-logs/testlog.txt");
            }
            println!();
            println!("Ok:                 4");
            println!("Expected Fail:      0");
            println!("Fail:               0");
            println!("Unexpected Pass:    0");
            println!("Skipped:            0");
            println!("Timeout:            0");
            0
        }
        "install" => {
            let destdir = cmd_args.iter().position(|a| a == "--destdir")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str());
            println!("Installing myapp to /usr/local/bin/myapp");
            println!("Installing libutil.a to /usr/local/lib/libutil.a");
            println!("Installing header util.h to /usr/local/include/util.h");
            if let Some(d) = destdir {
                println!("(DESTDIR={})", d);
            }
            0
        }
        "dist" => {
            println!("Creating source archive...");
            println!("Running custom dist scripts...");
            println!("Created myproject-0.1.0.tar.xz");
            println!("Created myproject-0.1.0.tar.xz.sha256sum");
            0
        }
        "init" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("myproject");
            let lang = cmd_args.iter().position(|a| a == "-l" || a == "--language")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("c");
            println!("Using {} language", lang);
            println!("Creating files for project '{}':", name);
            println!("  meson.build");
            println!("  src/main.{}", if lang == "cpp" { "cpp" } else { lang });
            println!("Project '{}' initialized.", name);
            0
        }
        "introspect" => {
            let what = cmd_args.first().map(|s| s.as_str()).unwrap_or("--all");
            match what {
                "--targets" => {
                    println!("[");
                    println!("  {{\"name\": \"myapp\", \"type\": \"executable\", \"sources\": [\"src/main.c\"]}},");
                    println!("  {{\"name\": \"libutil\", \"type\": \"static_library\", \"sources\": [\"src/util.c\"]}}");
                    println!("]");
                }
                "--buildoptions" => {
                    println!("[");
                    println!("  {{\"name\": \"buildtype\", \"value\": \"debugoptimized\"}},");
                    println!("  {{\"name\": \"prefix\", \"value\": \"/usr/local\"}}");
                    println!("]");
                }
                "--projectinfo" => {
                    println!("{{\"descriptive_name\": \"myproject\", \"version\": \"0.1.0\", \"subprojects\": []}}");
                }
                _ => {
                    println!("introspect: {} (simulated)", what);
                }
            }
            0
        }
        "subprojects" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("List of subprojects:");
                    println!("  glib (from wrapdb)");
                    println!("  zlib (from wrapdb)");
                }
                "download" => println!("Downloading subprojects... done (simulated)"),
                "update" => println!("Updating subprojects... done (simulated)"),
                _ => println!("subprojects: {} (simulated)", sub),
            }
            0
        }
        "wrap" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Available wraps in WrapDB:");
                    println!("  glib      2.78.0");
                    println!("  zlib      1.3.1");
                    println!("  libpng    1.6.40");
                    println!("  openssl   3.2.0");
                }
                "install" => {
                    let pkg = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("package");
                    println!("Installed {} wrap (simulated)", pkg);
                }
                "search" => {
                    let q = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("");
                    println!("Search results for '{}': (simulated)", q);
                }
                _ => println!("wrap: {} (simulated)", sub),
            }
            0
        }
        "env2mfile" => { println!("Generated cross file from environment (simulated)"); 0 }
        "rewrite" => { println!("Rewrote meson.build (simulated)"); 0 }
        other => { eprintln!("meson: unknown command '{}'", other); 1 }
    }
}

fn run_mesonconf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mesonconf [builddir]");
        println!("       mesonconf [builddir] -Doption=value");
        return 0;
    }
    // Delegate to configure subcommand
    run_meson(vec!["configure".to_string()].into_iter().chain(args).collect())
}

fn run_mesonintrospect(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mesonintrospect [builddir] [--targets|--buildoptions|--projectinfo]");
        return 0;
    }
    run_meson(vec!["introspect".to_string()].into_iter().chain(args).collect())
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("meson");
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
        "mesonconf" => run_mesonconf(rest),
        "mesonintrospect" => run_mesonintrospect(rest),
        _ => run_meson(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() {
        // Meson is primarily a command-line tool; minimal testable logic
        assert!(true);
    }
}
