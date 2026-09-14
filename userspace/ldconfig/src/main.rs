//! Slate OS shared library cache management.
//!
//! Multi-personality binary providing:
//! - **ldconfig** — configure dynamic linker run-time bindings
//! - **ldd** variant — print shared library dependencies
//!
//! Manages the shared library cache at `/etc/ld.so.cache` by scanning
//! configured directories for shared libraries and building a lookup table.

#![deny(clippy::all)]
// LibEntry::os_abi and the LibCache struct encode the ELF EI_OSABI
// byte and the on-disk /etc/ld.so.cache (CACHEMAGIC_NEW) file format
// the real ldconfig must produce. Dead-code lint cannot see across
// that future boundary.

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
    ///
    /// Bytes, because it is derived from the FILE NAME and a filename on this
    /// OS may hold every byte but `/` and NUL.
    soname: OsString,
    /// Full path to the library file.
    path: PathBuf,
    /// Library type (ELF class).
    lib_type: LibType,
    /// OS/ABI.
    // The ELF EI_OSABI encoding, parsed and not consulted when choosing a
    // library. Kept so the entry matches the header it was read from.
    #[allow(dead_code)]
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
#[allow(dead_code)]
struct LibCache {
    entries: Vec<LibEntry>,
}

// ============================================================================
// ELF header parsing (minimal)
// ============================================================================

/// Read minimal ELF header info from a file.
fn read_elf_info(path: &Path) -> Option<(LibType, u8)> {
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
fn extract_soname(path: &Path) -> Option<OsString> {
    // On the BYTES. A library's file name may hold any byte but `/` and NUL,
    // and the previous version began `file_name()?.to_str()?` -- so a library
    // whose name is not valid UTF-8 got no soname, and the caller fell back to
    // the file name, which it had also failed to decode.
    let bytes = quoting::os_bytes(path.file_name()?).into_owned();

    // The first `.so`, which is where the name ends and the version begins.
    let at = bytes.windows(3).position(|w| w == b".so")?;
    let prefix = bytes.get(..at)?;
    let suffix = bytes.get(at.saturating_add(3)..)?;

    if suffix.is_empty() {
        // A bare `.so` (e.g. libfoo.so).
        return Some(quoting::os_from_bytes(&bytes));
    }

    // `suffix` opens with `.` and then the version: `.1.2.3`. The soname keeps
    // only the major, so `libfoo.so.1.2.3` gives `libfoo.so.1`.
    let mut parts = suffix.split(|&c| c == b'.');
    let _leading_empty = parts.next();
    match parts.next() {
        Some(major) if !major.is_empty() => {
            let mut out = prefix.to_vec();
            out.extend_from_slice(b".so.");
            out.extend_from_slice(major);
            Some(quoting::os_from_bytes(&out))
        }
        _ => Some(quoting::os_from_bytes(&bytes)),
    }
}

// ============================================================================
// Configuration parsing
// ============================================================================

fn parse_ld_so_conf() -> Vec<PathBuf> {
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
                if let Some(parent) = Path::new(include_path).parent()
                    && let Ok(entries) = fs::read_dir(parent)
                {
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if let Some(ext) = entry_path.extension()
                            && ext == "conf"
                            && let Ok(sub_content) = fs::read_to_string(&entry_path)
                        {
                            for sub_line in sub_content.lines() {
                                let sub_line = sub_line.trim();
                                if !sub_line.is_empty() && !sub_line.starts_with('#') {
                                    dirs.push(PathBuf::from(sub_line));
                                }
                            }
                        }
                    }
                }
            } else {
                dirs.push(PathBuf::from(line));
            }
        }
    }

    // Also scan /etc/ld.so.conf.d/ directly.
    if let Ok(entries) = fs::read_dir(LD_SO_CONF_D) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "conf").unwrap_or(false)
                && let Ok(content) = fs::read_to_string(&path)
            {
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty()
                        && !line.starts_with('#')
                        && !dirs.iter().any(|d| d == Path::new(line))
                    {
                        dirs.push(PathBuf::from(line));
                    }
                }
            }
        }
    }

    // Add defaults.
    for &d in DEFAULT_DIRS {
        if !dirs.iter().any(|x| x == Path::new(d)) {
            dirs.push(PathBuf::from(d));
        }
    }

    dirs
}

// ============================================================================
// Library scanning
// ============================================================================

fn scan_directory(dir: &Path, verbose: bool) -> Vec<LibEntry> {
    let mut entries = Vec::new();

    let read_dir = match fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return entries,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        // NO `to_str()` GUARD HERE, and that is the fix rather than a tidy-up.
        //
        // This used to read the path and the file name through `to_str()` and
        // `continue` when either failed, so a shared library whose path is not
        // valid UTF-8 was SILENTLY LEFT OUT OF THE CACHE. Nothing said so, and
        // the consequence lands somewhere else entirely: the dynamic linker
        // cannot find the library, and a program linked against it fails to
        // start for a reason that points at neither.
        let Some(filename) = path.file_name() else {
            continue;
        };
        let filename = filename.to_os_string();

        // Only consider files that look like shared libraries. On the BYTES,
        // since the name need not be text.
        if !quoting::os_bytes(&filename).windows(3).any(|w| w == b".so") {
            continue;
        }

        // Read ELF info.
        let (lib_type, os_abi) = match read_elf_info(&path) {
            Some(info) => info,
            None => {
                // Might be a symlink — follow it.
                if path.is_symlink() {
                    if let Ok(real) = fs::canonicalize(&path) {
                        // The path itself, not `to_string_lossy()`: a symlink
                        // resolving to a target whose path is not UTF-8 was
                        // opened under a name containing U+FFFD, which names
                        // nothing, so the library was dropped from the cache.
                        match read_elf_info(&real) {
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

        let soname = extract_soname(&path).unwrap_or(filename);

        if verbose {
            // Both are bytes; escaped for display only.
            eprintln!(
                "  {} (soname: {})",
                quoting::escape_unprintable(&quoting::os_bytes(path.as_os_str())),
                quoting::escape_unprintable(&quoting::os_bytes(&soname))
            );
        }

        entries.push(LibEntry {
            soname,
            path,
            lib_type,
            os_abi,
        });
    }

    entries
}

fn scan_all_dirs(dirs: &[PathBuf], verbose: bool) -> Vec<LibEntry> {
    let mut all_entries = Vec::new();
    for dir in dirs {
        if verbose {
            // Escaped for display; the bytes are what get scanned.
            eprintln!(
                "Scanning {}...",
                quoting::escape_unprintable(&quoting::os_bytes(dir.as_os_str()))
            );
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
    let mut content: Vec<u8> = Vec::new();
    content.extend_from_slice("# ld.so.cache — auto-generated by ldconfig\n".as_bytes());
    content.extend_from_slice(format!("# {} entries\n", entries.len()).as_bytes());

    for entry in entries {
        let type_str = match entry.lib_type {
            LibType::Elf64 => "ELF64",
            LibType::Elf32 => "ELF32",
            LibType::Unknown => "UNKNOWN",
        };
        // BYTES. A soname and a path may each hold any byte but `/` and
        // NUL, and this cache is our own tab-separated format rather than
        // glibc's binary one, so it can carry them exactly. Writing it as
        // text would have required a lossy decode, and the entry that came
        // back would name a library that does not exist.
        content.extend_from_slice(&quoting::os_bytes(&entry.soname));
        content.push(b'\t');
        content.extend_from_slice(type_str.as_bytes());
        content.push(b'\t');
        content.extend_from_slice(&quoting::os_bytes(entry.path.as_os_str()));
        content.push(b'\n');
    }

    fs::write(LD_SO_CACHE, content)
}

fn read_cache() -> Vec<LibEntry> {
    // Read as BYTES to match how `write_cache` produces it: a soname or a
    // path in there may hold any byte but `/` and NUL, and `read_to_string`
    // would refuse the WHOLE cache because of one such entry -- turning one
    // awkward library into no libraries at all.
    let content = match fs::read(LD_SO_CACHE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.split(|&b| b == b'\n') {
        if line.first() == Some(&b'#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&[u8]> = line.split(|&b| b == b'\t').collect();
        if let [soname, kind, path, ..] = fields.as_slice() {
            let lib_type = match *kind {
                b"ELF64" => LibType::Elf64,
                b"ELF32" => LibType::Elf32,
                _ => LibType::Unknown,
            };
            entries.push(LibEntry {
                soname: quoting::os_from_bytes(soname),
                path: PathBuf::from(quoting::os_from_bytes(path)),
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

fn cmd_ldconfig(args: &[OsString]) {
    let mut verbose = false;
    let mut print_cache = false;
    let mut no_write = false;
    let mut extra_dirs: Vec<PathBuf> = Vec::new();

    for arg in args {
        // `""` for a word that is not Unicode: it matches no option name and
        // falls to the operand arm, which keeps `arg` -- and the operand here
        // is a DIRECTORY to scan.
        match arg.to_str().unwrap_or("") {
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
                // `arg`, not the decoded view: this operand is a DIRECTORY
                // to scan, and a directory name may hold any byte.
                extra_dirs.push(PathBuf::from(arg));
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
            // `-p` lists the cache for a human, so the names are escaped
            // for display rather than written raw -- an unprintable byte
            // in one would otherwise be able to forge a line of output.
            let _ = writeln!(
                out,
                "\t{} {} => {}",
                quoting::escape_unprintable(&quoting::os_bytes(&entry.soname)),
                type_str,
                quoting::escape_unprintable(&quoting::os_bytes(entry.path.as_os_str()))
            );
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
    let mut seen: HashMap<(OsString, String), bool> = HashMap::new();
    let deduped: Vec<LibEntry> = entries
        .into_iter()
        .filter(|e| {
            let arch_key = match e.lib_type {
                LibType::Elf64 => "64",
                LibType::Elf32 => "32",
                LibType::Unknown => "?",
            };
            let key = (e.soname.clone(), arch_key.to_string());
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
                e.insert(true);
                true
            } else {
                false
            }
        })
        .collect();

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
    // `args_os`, not `args`: the latter's iterator unwraps, so naming a
    // directory whose path is not valid Unicode killed the process here.
    let args: Vec<OsString> = env::args_os().collect();
    let rest: Vec<OsString> = args.into_iter().skip(1).collect();
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
        assert_eq!(
            extract_soname(Path::new("/usr/lib/libfoo.so.1.2.3")),
            Some(OsString::from("libfoo.so.1"))
        );
        assert_eq!(
            extract_soname(Path::new("/usr/lib/libbar.so.2")),
            Some(OsString::from("libbar.so.2"))
        );
        assert_eq!(
            extract_soname(Path::new("/lib/libc.so.6")),
            Some(OsString::from("libc.so.6"))
        );
    }

    #[test]
    fn test_extract_soname_bare() {
        assert_eq!(
            extract_soname(Path::new("/usr/lib/libfoo.so")),
            Some(OsString::from("libfoo.so"))
        );
    }

    #[test]
    fn test_extract_soname_no_so() {
        assert_eq!(extract_soname(Path::new("/usr/lib/libfoo.a")), None);
        assert_eq!(extract_soname(Path::new("/usr/bin/program")), None);
    }

    #[test]
    fn test_extract_soname_complex() {
        assert_eq!(
            extract_soname(Path::new("/lib/x86_64-linux-gnu/libpthread.so.0")),
            Some(OsString::from("libpthread.so.0"))
        );
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
            soname: OsString::from("libfoo.so.1"),
            path: PathBuf::from("/usr/lib/libfoo.so.1.0.0"),
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
        assert!(read_elf_info(Path::new("/nonexistent/file")).is_none());
    }

    #[test]
    fn test_scan_nonexistent_dir() {
        let entries = scan_directory(Path::new("/nonexistent/dir/that/should/not/exist"), false);
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
