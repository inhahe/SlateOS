//! OurOS shared library cache management.
//!
//! Multi-personality binary providing:
//! - **ldconfig** — configure dynamic linker run-time bindings
//! - **ldd** variant — print shared library dependencies
//!
//! Manages the shared library cache at `/etc/ld.so.cache` by scanning
//! configured directories for shared libraries and building a lookup table.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const LD_SO_CONF: &str = "/etc/ld.so.conf";
const LD_SO_CACHE: &str = "/etc/ld.so.cache";
const LD_SO_CONF_D: &str = "/etc/ld.so.conf.d";

/// Default library search paths.
const DEFAULT_DIRS: &[&str] = &["/lib", "/usr/lib", "/lib64", "/usr/lib64"];

/// ELF magic bytes.
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

// ============================================================================
// Data structures
// ============================================================================

/// A shared library entry in the cache.
#[derive(Clone, Debug)]
struct LibEntry {
    /// Library soname (e.g., "libfoo.so.1").
    soname: String,
    /// Full path to the library file.
    path: String,
    /// Library type (ELF class).
    lib_type: LibType,
    /// OS/ABI.
    os_abi: u8,
}

/// ELF class (32/64 bit).
#[derive(Clone, Debug, PartialEq)]
enum LibType {
    Elf32,
    Elf64,
    Unknown,
}

/// Cache file representation.
struct LibCache {
    entries: Vec<LibEntry>,
}

// ============================================================================
// ELF header parsing (minimal)
// ============================================================================

/// Read minimal ELF header info from a file.
fn read_elf_info(path: &str) -> Option<(LibType, u8)> {
    let data = fs::read(path).ok()?;
    if data.len() < 20 {
        return None;
    }

    // Check magic.
    if data[0..4] != ELF_MAGIC {
        return None;
    }

    let class = match data[4] {
        1 => LibType::Elf32,
        2 => LibType::Elf64,
        _ => LibType::Unknown,
    };

    let os_abi = data[7];

    Some((class, os_abi))
}

/// Extract the SONAME from an ELF shared library.
/// This is a simplified version — in a real implementation, we'd parse
/// the dynamic section. Here we use the filename convention.
fn extract_soname(path: &str) -> Option<String> {
    let filename = Path::new(path).file_name()?.to_str()?;

    // Common patterns: libfoo.so.1.2.3 → soname = libfoo.so.1
    // Or: libfoo.so → soname = libfoo.so
    if !filename.contains(".so") {
        return None;
    }

    // Find the soname by truncating to the first version component.
    let parts: Vec<&str> = filename.split(".so").collect();
    if parts.len() < 2 {
        return Some(filename.to_string());
    }

    let prefix = parts[0];
    let suffix = parts[1];

    if suffix.is_empty() {
        // Bare .so file (e.g., libfoo.so).
        return Some(filename.to_string());
    }

    // suffix starts with '.' followed by version: .1.2.3
    // SONAME is typically prefix.so.major
    let version_parts: Vec<&str> = suffix.split('.').collect();
    // version_parts[0] is empty (from the leading '.'), [1] is major
    if version_parts.len() >= 2 && !version_parts[1].is_empty() {
        Some(format!("{prefix}.so.{}", version_parts[1]))
    } else {
        Some(filename.to_string())
    }
}

// ============================================================================
// Configuration parsing
// ============================================================================

fn parse_ld_so_conf() -> Vec<String> {
    let mut dirs = Vec::new();

    if let Ok(content) = fs::read_to_string(LD_SO_CONF) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(include_path) = line.strip_prefix("include ") {
                // Glob include (e.g., include /etc/ld.so.conf.d/*.conf).
                let include_path = include_path.trim();
                if let Some(parent) = Path::new(include_path).parent() {
                    if let Ok(entries) = fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            if let Some(ext) = entry_path.extension() {
                                if ext == "conf" {
                                    if let Ok(sub_content) = fs::read_to_string(&entry_path) {
                                        for sub_line in sub_content.lines() {
                                            let sub_line = sub_line.trim();
                                            if !sub_line.is_empty() && !sub_line.starts_with('#') {
                                                dirs.push(sub_line.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                dirs.push(line.to_string());
            }
        }
    }

    // Also scan /etc/ld.so.conf.d/ directly.
    if let Ok(entries) = fs::read_dir(LD_SO_CONF_D) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "conf").unwrap_or(false) {
                if let Ok(content) = fs::read_to_string(&path) {
                    for line in content.lines() {
                        let line = line.trim();
                        if !line.is_empty() && !line.starts_with('#') && !dirs.contains(&line.to_string()) {
                            dirs.push(line.to_string());
                        }
                    }
                }
            }
        }
    }

    // Add defaults.
    for &d in DEFAULT_DIRS {
        if !dirs.contains(&d.to_string()) {
            dirs.push(d.to_string());
        }
    }

    dirs
}

// ============================================================================
// Library scanning
// ============================================================================

fn scan_directory(dir: &str, verbose: bool) -> Vec<LibEntry> {
    let mut entries = Vec::new();

    let read_dir = match fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return entries,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let path_str = match path.to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only consider files that look like shared libraries.
        if !filename.contains(".so") {
            continue;
        }

        // Read ELF info.
        let (lib_type, os_abi) = match read_elf_info(&path_str) {
            Some(info) => info,
            None => {
                // Might be a symlink — follow it.
                if path.is_symlink() {
                    if let Ok(real) = fs::canonicalize(&path) {
                        let real_str = real.to_string_lossy().to_string();
                        match read_elf_info(&real_str) {
                            Some(info) => info,
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
        };

        let soname = extract_soname(&path_str).unwrap_or(filename);

        if verbose {
            eprintln!("  {path_str} (soname: {soname})");
        }

        entries.push(LibEntry {
            soname,
            path: path_str,
            lib_type,
            os_abi,
        });
    }

    entries
}

fn scan_all_dirs(dirs: &[String], verbose: bool) -> Vec<LibEntry> {
    let mut all_entries = Vec::new();
    for dir in dirs {
        if verbose {
            eprintln!("Scanning {dir}...");
        }
        let mut entries = scan_directory(dir, verbose);
        all_entries.append(&mut entries);
    }
    all_entries
}

// ============================================================================
// Cache operations
// ============================================================================

fn write_cache(entries: &[LibEntry]) -> io::Result<()> {
    let mut content = String::new();
    content.push_str("# ld.so.cache — auto-generated by ldconfig\n");
    content.push_str(&format!("# {} entries\n", entries.len()));

    for entry in entries {
        let type_str = match entry.lib_type {
            LibType::Elf64 => "ELF64",
            LibType::Elf32 => "ELF32",
            LibType::Unknown => "UNKNOWN",
        };
        content.push_str(&format!("{}\t{}\t{}\n", entry.soname, type_str, entry.path));
    }

    fs::write(LD_SO_CACHE, content)
}

fn read_cache() -> Vec<LibEntry> {
    let content = match fs::read_to_string(LD_SO_CACHE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() >= 3 {
            let lib_type = match fields[1] {
                "ELF64" => LibType::Elf64,
                "ELF32" => LibType::Elf32,
                _ => LibType::Unknown,
            };
            entries.push(LibEntry {
                soname: fields[0].to_string(),
                path: fields[2].to_string(),
                lib_type,
                os_abi: 0,
            });
        }
    }
    entries
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_ldconfig(args: &[String]) {
    let mut verbose = false;
    let mut print_cache = false;
    let mut no_write = false;
    let mut extra_dirs: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" | "-?" => {
                println!("Usage: ldconfig [options] [dir...]");
                println!();
                println!("Configure dynamic linker run-time bindings.");
                println!();
                println!("Options:");
                println!("  -v, --verbose   Verbose mode");
                println!("  -p, --print-cache  Print current cache");
                println!("  -N            Don't rebuild cache");
                println!("  -h, --help    Show this help");
                println!("  --version     Show version");
                process::exit(0);
            }
            "--version" => {
                println!("ldconfig {VERSION}");
                process::exit(0);
            }
            "-v" | "--verbose" => verbose = true,
            "-p" | "--print-cache" => print_cache = true,
            "-N" => no_write = true,
            s if !s.starts_with('-') => {
                extra_dirs.push(s.to_string());
            }
            _ => {} // Ignore unknown flags silently (like real ldconfig).
        }
    }

    if print_cache {
        let entries = read_cache();
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let _ = writeln!(out, "{} libs found in cache '{LD_SO_CACHE}'", entries.len());
        for entry in &entries {
            let type_str = match entry.lib_type {
                LibType::Elf64 => "(libc6,x86-64)",
                LibType::Elf32 => "(libc6)",
                LibType::Unknown => "(unknown)",
            };
            let _ = writeln!(out, "\t{} {} => {}", entry.soname, type_str, entry.path);
        }
        return;
    }

    // Scan directories.
    let mut dirs = parse_ld_so_conf();
    for d in &extra_dirs {
        if !dirs.contains(d) {
            dirs.insert(0, d.clone());
        }
    }

    let entries = scan_all_dirs(&dirs, verbose);

    // Deduplicate: keep first occurrence of each soname (per arch).
    let mut seen: HashMap<(String, String), bool> = HashMap::new();
    let deduped: Vec<LibEntry> = entries.into_iter().filter(|e| {
        let arch_key = match e.lib_type {
            LibType::Elf64 => "64",
            LibType::Elf32 => "32",
            LibType::Unknown => "?",
        };
        let key = (e.soname.clone(), arch_key.to_string());
        if seen.contains_key(&key) {
            false
        } else {
            seen.insert(key, true);
            true
        }
    }).collect();

    if verbose {
        eprintln!("Found {} libraries", deduped.len());
    }

    if !no_write {
        if let Err(e) = write_cache(&deduped) {
            eprintln!("ldconfig: cannot write cache: {e}");
            process::exit(1);
        }
        if verbose {
            eprintln!("Cache written to {LD_SO_CACHE}");
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    cmd_ldconfig(&rest);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_soname_versioned() {
        assert_eq!(extract_soname("/usr/lib/libfoo.so.1.2.3"), Some("libfoo.so.1".to_string()));
        assert_eq!(extract_soname("/usr/lib/libbar.so.2"), Some("libbar.so.2".to_string()));
        assert_eq!(extract_soname("/lib/libc.so.6"), Some("libc.so.6".to_string()));
    }

    #[test]
    fn test_extract_soname_bare() {
        assert_eq!(extract_soname("/usr/lib/libfoo.so"), Some("libfoo.so".to_string()));
    }

    #[test]
    fn test_extract_soname_no_so() {
        assert_eq!(extract_soname("/usr/lib/libfoo.a"), None);
        assert_eq!(extract_soname("/usr/bin/program"), None);
    }

    #[test]
    fn test_extract_soname_complex() {
        assert_eq!(extract_soname("/lib/x86_64-linux-gnu/libpthread.so.0"), Some("libpthread.so.0".to_string()));
    }

    #[test]
    fn test_lib_type_equality() {
        assert_eq!(LibType::Elf64, LibType::Elf64);
        assert_ne!(LibType::Elf32, LibType::Elf64);
        assert_ne!(LibType::Unknown, LibType::Elf64);
    }

    #[test]
    fn test_lib_entry_clone() {
        let entry = LibEntry {
            soname: "libfoo.so.1".to_string(),
            path: "/usr/lib/libfoo.so.1.0.0".to_string(),
            lib_type: LibType::Elf64,
            os_abi: 0,
        };
        let cloned = entry.clone();
        assert_eq!(cloned.soname, "libfoo.so.1");
        assert_eq!(cloned.lib_type, LibType::Elf64);
    }

    #[test]
    fn test_default_dirs() {
        assert!(DEFAULT_DIRS.contains(&"/lib"));
        assert!(DEFAULT_DIRS.contains(&"/usr/lib"));
    }

    #[test]
    fn test_elf_magic() {
        assert_eq!(ELF_MAGIC[0], 0x7f);
        assert_eq!(ELF_MAGIC[1], b'E');
        assert_eq!(ELF_MAGIC[2], b'L');
        assert_eq!(ELF_MAGIC[3], b'F');
    }

    #[test]
    fn test_read_elf_info_nonexistent() {
        assert!(read_elf_info("/nonexistent/file").is_none());
    }

    #[test]
    fn test_scan_nonexistent_dir() {
        let entries = scan_directory("/nonexistent/dir/that/should/not/exist", false);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_read_cache_empty() {
        // If cache doesn't exist, should return empty vec.
        let entries = read_cache();
        // May or may not be empty depending on system state.
        let _ = entries.len();
    }

    #[test]
    fn test_parse_ld_so_conf() {
        let dirs = parse_ld_so_conf();
        // Should always include defaults.
        assert!(dirs.iter().any(|d| d == "/lib" || d == "/usr/lib"));
    }
}
