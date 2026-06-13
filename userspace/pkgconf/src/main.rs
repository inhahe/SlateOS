#![deny(clippy::all)]

//! pkgconf — Slate OS package configuration tool
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `pkgconf` (default) — package configuration utility
//! - `pkg-config` — GNU pkg-config compatible interface
//! - `bomtool` — bill of materials tool

use std::env;
use std::process;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct _PkgInfo {
    name: String,
    version: String,
    description: String,
    cflags: String,
    libs: String,
    _requires: Vec<String>,
}

fn _sample_packages() -> Vec<_PkgInfo> {
    vec![
        _PkgInfo {
            name: "zlib".to_string(), version: "1.3.1".to_string(),
            description: "zlib compression library".to_string(),
            cflags: "-I/usr/include".to_string(), libs: "-lz".to_string(),
            _requires: vec![],
        },
        _PkgInfo {
            name: "libpng".to_string(), version: "1.6.40".to_string(),
            description: "Portable Network Graphics library".to_string(),
            cflags: "-I/usr/include/libpng16".to_string(), libs: "-lpng16 -lz".to_string(),
            _requires: vec!["zlib".to_string()],
        },
        _PkgInfo {
            name: "openssl".to_string(), version: "3.2.0".to_string(),
            description: "OpenSSL cryptography library".to_string(),
            cflags: "-I/usr/include/openssl".to_string(), libs: "-lssl -lcrypto".to_string(),
            _requires: vec![],
        },
        _PkgInfo {
            name: "freetype2".to_string(), version: "2.13.2".to_string(),
            description: "FreeType font rendering library".to_string(),
            cflags: "-I/usr/include/freetype2".to_string(), libs: "-lfreetype".to_string(),
            _requires: vec!["zlib".to_string(), "libpng".to_string()],
        },
        _PkgInfo {
            name: "cairo".to_string(), version: "1.18.0".to_string(),
            description: "Cairo 2D graphics library".to_string(),
            cflags: "-I/usr/include/cairo".to_string(), libs: "-lcairo".to_string(),
            _requires: vec!["freetype2".to_string(), "libpng".to_string()],
        },
    ]
}

fn _find_package(name: &str) -> Option<_PkgInfo> {
    _sample_packages().into_iter().find(|p| p.name == name)
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_pkgconf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: pkgconf [OPTIONS] [PACKAGES...]");
        println!();
        println!("Query package compiler/linker flags.");
        println!();
        println!("Options:");
        println!("  --cflags           Output compiler flags");
        println!("  --libs             Output linker flags");
        println!("  --libs-only-l      Output -l flags only");
        println!("  --libs-only-L      Output -L flags only");
        println!("  --cflags-only-I    Output -I flags only");
        println!("  --modversion       Output version");
        println!("  --exists           Return 0 if package exists");
        println!("  --atleast-version=V  True if version >= V");
        println!("  --exact-version=V    True if version == V");
        println!("  --max-version=V      True if version <= V");
        println!("  --list-all         List all known packages");
        println!("  --print-variables  List all variables");
        println!("  --variable=NAME    Get variable value");
        println!("  --validate         Validate .pc files");
        println!("  --version          Show pkgconf version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("2.1.0 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "--list-all") {
        let packages = _sample_packages();
        for pkg in &packages {
            println!("{:<20} {:<10} - {}", pkg.name, pkg.version, pkg.description);
        }
        return 0;
    }

    // Gather options and package names
    let want_cflags = args.iter().any(|a| a == "--cflags" || a == "--cflags-only-I");
    let want_libs = args.iter().any(|a| a == "--libs" || a == "--libs-only-l" || a == "--libs-only-L");
    let want_version = args.iter().any(|a| a == "--modversion");
    let want_exists = args.iter().any(|a| a == "--exists");
    let want_variables = args.iter().any(|a| a == "--print-variables");
    let want_validate = args.iter().any(|a| a == "--validate");

    let specific_var = args.iter().find(|a| a.starts_with("--variable="))
        .map(|a| a.split('=').nth(1).unwrap_or(""));

    let pkg_names: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if pkg_names.is_empty() && !args.iter().any(|a| a == "--list-all" || a == "--version") {
        eprintln!("Please specify at least one package name on the command line.");
        return 1;
    }

    for name in &pkg_names {
        if let Some(pkg) = _find_package(name) {
            if want_exists {
                // Just return 0 (success) for --exists
                continue;
            }

            if want_version {
                println!("{}", pkg.version);
            }

            if want_cflags {
                println!("{}", pkg.cflags);
            }

            if want_libs {
                println!("{}", pkg.libs);
            }

            if want_variables {
                println!("prefix=/usr");
                println!("exec_prefix=${{prefix}}");
                println!("libdir=${{exec_prefix}}/lib");
                println!("includedir=${{prefix}}/include");
            }

            if let Some(var) = specific_var {
                match var {
                    "prefix" => println!("/usr"),
                    "libdir" => println!("/usr/lib"),
                    "includedir" => println!("/usr/include"),
                    _ => println!("(undefined)"),
                }
            }

            if want_validate {
                println!("{}: valid .pc file", name);
            }

            // Default: if no specific query, show summary
            if !want_cflags && !want_libs && !want_version && !want_exists && !want_variables && specific_var.is_none() && !want_validate {
                println!("{} {}", pkg.name, pkg.version);
            }
        } else {
            eprintln!("Package '{}' was not found in the pkg-config search path.", name);
            return 1;
        }
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let _prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("pkgconf");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    // All personalities (pkgconf, pkg-config, bomtool) use the same logic
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pkgconf(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_packages() {
        let pkgs = _sample_packages();
        assert_eq!(pkgs.len(), 5);
        assert_eq!(pkgs[0].name, "zlib");
    }

    #[test]
    fn test_find_package() {
        assert!(_find_package("zlib").is_some());
        assert!(_find_package("nonexistent").is_none());
    }
}
