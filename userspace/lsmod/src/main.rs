//! OurOS Kernel Module Management Tools
//!
//! Multi-personality binary providing `lsmod`, `modprobe`, `insmod`, and `rmmod`
//! functionality. The active personality is detected from `argv[0]`.
//!
//! # Usage
//!
//! ```text
//! lsmod                           List loaded kernel modules
//! modprobe <module>               Load module with dependency resolution
//! modprobe -r <module>            Remove a module
//! modprobe --show-depends <mod>   Show module dependencies
//! insmod <path> [params]          Load module from file path (no dep resolution)
//! rmmod <module>                  Remove a loaded module
//! rmmod -f <module>               Force removal
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

/// Syscall: load a kernel module from a memory image.
///
/// `arg0`: pointer to module image data.
/// `arg1`: length of image in bytes.
/// `arg2`: pointer to null-terminated parameter string.
const SYS_INIT_MODULE: u64 = 150;

/// Syscall: remove a loaded kernel module.
///
/// `arg0`: pointer to null-terminated module name.
/// `arg1`: flags (MODULE_FORCE = 1).
const SYS_DELETE_MODULE: u64 = 151;

/// Invoke a raw syscall with up to 3 arguments.
///
/// Uses the standard x86_64 Linux syscall convention:
/// `rax` = syscall number, `rdi` = arg1, `rsi` = arg2, `rdx` = arg3.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees that nr is a valid syscall number and that
    // a1..a3 are valid arguments (pointers to accessible memory or numeric
    // values).  The kernel validates all inputs before acting on them.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// ============================================================================
// Error code mapping
// ============================================================================

/// Map a negative errno-style return code to a human-readable message.
fn errno_message(code: i64) -> &'static str {
    match code {
        -1 => "operation not permitted",
        -2 => "no such file or directory",
        -11 => "resource temporarily unavailable",
        -12 => "out of memory",
        -13 => "permission denied",
        -14 => "bad address",
        -16 => "device or resource busy",
        -17 => "file exists",
        -22 => "invalid argument",
        -38 => "function not implemented",
        _ => "unknown error",
    }
}

// ============================================================================
// /proc/modules parser
// ============================================================================

/// A loaded kernel module entry parsed from /proc/modules.
#[derive(Clone, Debug)]
struct ModuleEntry {
    /// Module name.
    name: String,
    /// Size in bytes.
    size: u64,
    /// Reference count (number of modules depending on this one + direct users).
    refcount: u32,
    /// Comma-separated list of dependent module names (may be empty).
    used_by: Vec<String>,
    /// Module state: Live, Loading, or Unloading.
    #[allow(dead_code)] // Parsed for completeness; used via Debug trait.
    state: String,
    /// Memory offset (hex address) where the module is loaded.
    #[allow(dead_code)] // Parsed for completeness; used via Debug trait.
    offset: String,
}

/// Parse /proc/modules content into a vector of module entries.
///
/// Each line of /proc/modules has the format:
///   name size refcount dep1,dep2,... state offset
///
/// Example:
///   snd_hda_intel 45056 2 snd_hda_codec, Live 0xffffffffc0800000
fn parse_proc_modules(content: &str) -> Vec<ModuleEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let name = parts[0].to_string();
        let size = parts[1].parse::<u64>().unwrap_or(0);
        let refcount = parts[2].parse::<u32>().unwrap_or(0);

        // The used_by field is comma-separated. A trailing comma is common.
        // A lone "-" means no dependents.
        let used_by_str = parts[3];
        let used_by = if used_by_str == "-" {
            Vec::new()
        } else {
            used_by_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };

        let state = parts.get(4).unwrap_or(&"Unknown").to_string();
        let offset = parts.get(5).unwrap_or(&"0x0").to_string();

        entries.push(ModuleEntry {
            name,
            size,
            refcount,
            used_by,
            state,
            offset,
        });
    }

    entries
}

/// Read and parse /proc/modules from the running kernel.
fn read_proc_modules() -> Vec<ModuleEntry> {
    match fs::read_to_string("/proc/modules") {
        Ok(content) => parse_proc_modules(&content),
        Err(_) => Vec::new(),
    }
}

// ============================================================================
// modules.dep parser (dependency resolution for modprobe)
// ============================================================================

/// A single dependency entry from modules.dep.
#[derive(Clone, Debug)]
struct DepEntry {
    /// Module file path (relative to /lib/modules/<version>/).
    path: String,
    /// Module name derived from the file path (without extension).
    name: String,
    /// Paths of modules this one depends on.
    deps: Vec<String>,
}

/// Parse the contents of a modules.dep file.
///
/// Each line has the format:
///   module_path: dep_path1 dep_path2 ...
///
/// Example:
///   kernel/drivers/net/e1000.ko: kernel/drivers/net/mii.ko
fn parse_modules_dep(content: &str) -> Vec<DepEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some(colon_pos) = line.find(':') else {
            continue;
        };

        let path = line[..colon_pos].trim().to_string();
        let deps_str = line[colon_pos + 1..].trim();

        let deps: Vec<String> = if deps_str.is_empty() {
            Vec::new()
        } else {
            deps_str
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        };

        let name = module_name_from_path(&path);

        entries.push(DepEntry { path, name, deps });
    }

    entries
}

/// Extract a module name from its file path.
///
/// Strips directory components and removes .ko / .ko.gz / .ko.xz / .ko.zst
/// extensions. Also replaces hyphens with underscores (Linux convention).
///
/// Example: "kernel/drivers/net/e1000.ko" -> "e1000"
fn module_name_from_path(path: &str) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);

    // Strip known extensions in order from outermost to innermost.
    let base = filename
        .strip_suffix(".ko.zst")
        .or_else(|| filename.strip_suffix(".ko.xz"))
        .or_else(|| filename.strip_suffix(".ko.gz"))
        .or_else(|| filename.strip_suffix(".ko"))
        .unwrap_or(filename);

    base.replace('-', "_")
}

/// Resolve the full ordered dependency list for a given module name.
///
/// Returns a vector of (module_name, module_path) tuples in load order
/// (dependencies first, the requested module last).
fn resolve_dependencies(
    module: &str,
    dep_entries: &[DepEntry],
) -> Result<Vec<(String, String)>, String> {
    // Normalize the name: replace hyphens with underscores.
    let target = module.replace('-', "_");

    // Find the target module in the dep database.
    let entry = dep_entries
        .iter()
        .find(|e| e.name == target)
        .ok_or_else(|| format!("module '{module}' not found in modules.dep"))?;

    let mut result = Vec::new();
    let mut visited = Vec::new();

    // Recursively resolve dependencies.
    resolve_deps_recursive(entry, dep_entries, &mut result, &mut visited)?;

    Ok(result)
}

/// Recursive helper for dependency resolution. Detects circular dependencies.
fn resolve_deps_recursive(
    entry: &DepEntry,
    all_entries: &[DepEntry],
    result: &mut Vec<(String, String)>,
    visited: &mut Vec<String>,
) -> Result<(), String> {
    if visited.contains(&entry.name) {
        return Err(format!("circular dependency detected involving '{}'", entry.name));
    }

    // Already in result means we processed it; skip.
    if result.iter().any(|(name, _)| *name == entry.name) {
        return Ok(());
    }

    visited.push(entry.name.clone());

    // Process each dependency first.
    for dep_path in &entry.deps {
        let dep_name = module_name_from_path(dep_path);
        if let Some(dep_entry) = all_entries.iter().find(|e| e.name == dep_name) {
            resolve_deps_recursive(dep_entry, all_entries, result, visited)?;
        } else {
            return Err(format!(
                "dependency '{}' of '{}' not found in modules.dep",
                dep_name, entry.name
            ));
        }
    }

    visited.retain(|n| *n != entry.name);
    result.push((entry.name.clone(), entry.path.clone()));
    Ok(())
}

// ============================================================================
// Kernel version detection
// ============================================================================

/// Read the kernel version string (equivalent to `uname -r`).
///
/// Tries /proc/version first, falls back to /proc/sys/kernel/osrelease.
fn get_kernel_version() -> String {
    // Try /proc/sys/kernel/osrelease first (single line, just the version).
    if let Ok(ver) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        let ver = ver.trim();
        if !ver.is_empty() {
            return ver.to_string();
        }
    }

    // Try /proc/version (format: "OurOS version X.Y.Z ...").
    if let Ok(ver) = fs::read_to_string("/proc/version") {
        if let Some(third) = ver.split_whitespace().nth(2) {
            return third.to_string();
        }
    }

    // Fallback.
    "0.1.0".to_string()
}

/// Build the path to modules.dep for the current kernel version.
fn modules_dep_path() -> String {
    let version = get_kernel_version();
    format!("/lib/modules/{version}/modules.dep")
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Load a kernel module from a memory image.
///
/// `image`: raw bytes of the module file (.ko).
/// `params`: null-terminated parameter string (e.g., "param1=val1 param2=val2").
fn do_init_module(image: &[u8], params: &str) -> Result<(), String> {
    let params_cstr = format!("{params}\0");

    let ret = unsafe {
        syscall3(
            SYS_INIT_MODULE,
            image.as_ptr() as u64,
            image.len() as u64,
            params_cstr.as_ptr() as u64,
        )
    };

    if ret < 0 {
        Err(format!("init_module failed: {} (error {})", errno_message(ret), ret))
    } else {
        Ok(())
    }
}

/// Remove a loaded kernel module.
///
/// `name`: module name (not file path).
/// `force`: if true, force removal even if the module is in use.
fn do_delete_module(name: &str, force: bool) -> Result<(), String> {
    let name_cstr = format!("{name}\0");
    let flags: u64 = if force { 1 } else { 0 };

    let ret = unsafe {
        syscall3(
            SYS_DELETE_MODULE,
            name_cstr.as_ptr() as u64,
            flags,
            0,
        )
    };

    if ret < 0 {
        Err(format!("delete_module '{}' failed: {} (error {})", name, errno_message(ret), ret))
    } else {
        Ok(())
    }
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format a byte size in a human-friendly way (e.g., "45.0 KiB").
fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        let mib = bytes as f64 / (1024.0 * 1024.0);
        format!("{mib:.1} MiB")
    } else if bytes >= 1024 {
        let kib = bytes as f64 / 1024.0;
        format!("{kib:.1} KiB")
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// Personality: lsmod
// ============================================================================

/// Display loaded kernel modules in tabular format.
fn run_lsmod(args: &[String]) -> i32 {
    // lsmod accepts --help and that's about it.
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" | "help" => {
                println!("Usage: lsmod");
                println!();
                println!("Show the status of loaded kernel modules.");
                println!("Reads from /proc/modules.");
                return 0;
            }
            other => {
                eprintln!("lsmod: unexpected argument '{other}'");
                return 1;
            }
        }
    }

    let entries = read_proc_modules();

    if entries.is_empty() {
        println!("(no modules loaded — /proc/modules not available or empty)");
        return 0;
    }

    // Compute column widths for alignment.
    let name_width = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(6)
        .max(6);

    let size_width = entries
        .iter()
        .map(|e| format_size(e.size).len())
        .max()
        .unwrap_or(4)
        .max(4);

    // Print header.
    println!(
        "{:<nw$}  {:>sw$}  {}  {}",
        "Module",
        "Size",
        "Used",
        "By",
        nw = name_width,
        sw = size_width,
    );

    // Print entries.
    for entry in &entries {
        let used_by_str = if entry.used_by.is_empty() {
            String::from("-")
        } else {
            entry.used_by.join(",")
        };

        println!(
            "{:<nw$}  {:>sw$}  {:<4}  {}",
            entry.name,
            format_size(entry.size),
            entry.refcount,
            used_by_str,
            nw = name_width,
            sw = size_width,
        );
    }

    0
}

// ============================================================================
// Personality: modprobe
// ============================================================================

/// Parsed modprobe command-line options.
struct ModprobeOpts {
    /// Module name to operate on.
    module: String,
    /// Remove instead of load.
    remove: bool,
    /// Dry-run mode (print but don't execute).
    dry_run: bool,
    /// Verbose output.
    verbose: bool,
    /// Quiet mode (suppress error messages).
    quiet: bool,
    /// Show dependencies and exit.
    show_depends: bool,
    /// Extra module parameters (everything after the module name).
    params: Vec<String>,
}

fn parse_modprobe_args(args: &[String]) -> Result<ModprobeOpts, String> {
    let mut opts = ModprobeOpts {
        module: String::new(),
        remove: false,
        dry_run: false,
        verbose: false,
        quiet: false,
        show_depends: false,
        params: Vec::new(),
    };

    let mut found_module = false;
    let mut idx = 0;

    while idx < args.len() {
        let arg = &args[idx];

        if found_module {
            // Everything after the module name is a parameter.
            opts.params.push(arg.clone());
            idx += 1;
            continue;
        }

        match arg.as_str() {
            "-r" | "--remove" => opts.remove = true,
            "-n" | "--dry-run" => opts.dry_run = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-q" | "--quiet" => opts.quiet = true,
            "--show-depends" => opts.show_depends = true,
            "--help" | "-h" => {
                return Err(String::new()); // Signal to print help.
            }
            s if s.starts_with('-') => {
                return Err(format!("unknown option: {s}"));
            }
            _ => {
                opts.module = arg.clone();
                found_module = true;
            }
        }

        idx += 1;
    }

    if opts.module.is_empty() {
        return Err("no module name specified".to_string());
    }

    Ok(opts)
}

fn print_modprobe_help() {
    println!("Usage: modprobe [OPTIONS] <module> [params...]");
    println!();
    println!("Load or remove kernel modules with automatic dependency resolution.");
    println!();
    println!("OPTIONS:");
    println!("  -r, --remove          Remove a module (and unused dependencies)");
    println!("  -n, --dry-run         Show what would be done without doing it");
    println!("  -v, --verbose         Print each action as it happens");
    println!("  -q, --quiet           Suppress error messages");
    println!("  --show-depends        Show module dependencies and exit");
    println!("  -h, --help            Show this help message");
    println!();
    println!("Module dependencies are read from /lib/modules/$(uname -r)/modules.dep");
}

fn run_modprobe(args: &[String]) -> i32 {
    let opts = match parse_modprobe_args(args) {
        Ok(o) => o,
        Err(msg) => {
            if msg.is_empty() {
                print_modprobe_help();
                return 0;
            }
            eprintln!("modprobe: {msg}");
            eprintln!("Try 'modprobe --help' for more information.");
            return 1;
        }
    };

    // Read the dependency database.
    let dep_path = modules_dep_path();
    let dep_content = match fs::read_to_string(&dep_path) {
        Ok(c) => c,
        Err(e) => {
            if !opts.quiet {
                eprintln!("modprobe: cannot read {dep_path}: {e}");
            }
            return 1;
        }
    };

    let dep_entries = parse_modules_dep(&dep_content);

    if opts.remove {
        return modprobe_remove(&opts, &dep_entries);
    }

    if opts.show_depends {
        return modprobe_show_depends(&opts, &dep_entries);
    }

    modprobe_load(&opts, &dep_entries)
}

/// Load a module and all its dependencies.
fn modprobe_load(opts: &ModprobeOpts, dep_entries: &[DepEntry]) -> i32 {
    let chain = match resolve_dependencies(&opts.module, dep_entries) {
        Ok(c) => c,
        Err(e) => {
            if !opts.quiet {
                eprintln!("modprobe: {e}");
            }
            return 1;
        }
    };

    let version = get_kernel_version();
    let base_dir = format!("/lib/modules/{version}");

    // Check which modules are already loaded.
    let loaded = read_proc_modules();

    let params_str = opts.params.join(" ");

    for (name, rel_path) in &chain {
        if loaded.iter().any(|m| m.name == *name) {
            if opts.verbose {
                println!("modprobe: {name} is already loaded, skipping");
            }
            continue;
        }

        let full_path = format!("{base_dir}/{rel_path}");

        // Only the final module in the chain gets parameters.
        let module_params = if *name == chain.last().map(|(n, _)| n.as_str()).unwrap_or("") {
            params_str.as_str()
        } else {
            ""
        };

        if opts.dry_run {
            println!("insmod {full_path} {module_params}");
            continue;
        }

        if opts.verbose {
            println!("insmod {full_path} {module_params}");
        }

        let image = match fs::read(&full_path) {
            Ok(data) => data,
            Err(e) => {
                if !opts.quiet {
                    eprintln!("modprobe: cannot read {full_path}: {e}");
                }
                return 1;
            }
        };

        if let Err(e) = do_init_module(&image, module_params) {
            if !opts.quiet {
                eprintln!("modprobe: {e}");
            }
            return 1;
        }
    }

    0
}

/// Remove a module (reverse dependency order).
fn modprobe_remove(opts: &ModprobeOpts, dep_entries: &[DepEntry]) -> i32 {
    let chain = match resolve_dependencies(&opts.module, dep_entries) {
        Ok(c) => c,
        Err(e) => {
            if !opts.quiet {
                eprintln!("modprobe: {e}");
            }
            return 1;
        }
    };

    // Remove in reverse order (the module itself first, then its dependencies
    // if they are no longer used).
    let loaded = read_proc_modules();

    for (name, _) in chain.iter().rev() {
        // Check if the module is actually loaded.
        let Some(entry) = loaded.iter().find(|m| m.name == *name) else {
            if opts.verbose {
                println!("modprobe -r: {name} is not loaded, skipping");
            }
            continue;
        };

        // Don't remove dependencies that are still in use by other modules.
        if *name != opts.module.replace('-', "_") && entry.refcount > 0 {
            if opts.verbose {
                println!("modprobe -r: {name} still in use (refcount {}), skipping", entry.refcount);
            }
            continue;
        }

        if opts.dry_run {
            println!("rmmod {name}");
            continue;
        }

        if opts.verbose {
            println!("rmmod {name}");
        }

        if let Err(e) = do_delete_module(name, false) {
            if !opts.quiet {
                eprintln!("modprobe: {e}");
            }
            // Don't fail entirely; continue removing what we can.
        }
    }

    0
}

/// Show dependency chain for a module.
fn modprobe_show_depends(opts: &ModprobeOpts, dep_entries: &[DepEntry]) -> i32 {
    let chain = match resolve_dependencies(&opts.module, dep_entries) {
        Ok(c) => c,
        Err(e) => {
            if !opts.quiet {
                eprintln!("modprobe: {e}");
            }
            return 1;
        }
    };

    let version = get_kernel_version();
    let base_dir = format!("/lib/modules/{version}");

    for (_, rel_path) in &chain {
        println!("insmod {base_dir}/{rel_path}");
    }

    0
}

// ============================================================================
// Personality: insmod
// ============================================================================

fn print_insmod_help() {
    println!("Usage: insmod <module_path> [params...]");
    println!();
    println!("Load a kernel module from a file path. No dependency resolution is");
    println!("performed; use modprobe for automatic dependency handling.");
}

fn run_insmod(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("insmod: missing module path");
        eprintln!("Try 'insmod --help' for more information.");
        return 1;
    }

    if args[0] == "--help" || args[0] == "-h" {
        print_insmod_help();
        return 0;
    }

    let module_path = &args[0];
    let params = if args.len() > 1 {
        args[1..].join(" ")
    } else {
        String::new()
    };

    let image = match fs::read(module_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("insmod: cannot read '{module_path}': {e}");
            return 1;
        }
    };

    if image.is_empty() {
        eprintln!("insmod: '{module_path}' is empty");
        return 1;
    }

    match do_init_module(&image, &params) {
        Ok(()) => {
            println!("Loaded module from {module_path}");
            0
        }
        Err(e) => {
            eprintln!("insmod: {e}");
            1
        }
    }
}

// ============================================================================
// Personality: rmmod
// ============================================================================

fn print_rmmod_help() {
    println!("Usage: rmmod [OPTIONS] <module>");
    println!();
    println!("Remove a loaded kernel module.");
    println!();
    println!("OPTIONS:");
    println!("  -f, --force     Force removal even if module is in use");
    println!("  -v, --verbose   Print verbose output");
    println!("  -h, --help      Show this help message");
}

fn run_rmmod(args: &[String]) -> i32 {
    let mut force = false;
    let mut verbose = false;
    let mut module_name: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "-f" | "--force" => force = true,
            "-v" | "--verbose" => verbose = true,
            "--help" | "-h" => {
                print_rmmod_help();
                return 0;
            }
            s if s.starts_with('-') => {
                eprintln!("rmmod: unknown option '{s}'");
                return 1;
            }
            _ => module_name = Some(arg.clone()),
        }
    }

    let module_name = match module_name {
        Some(n) => n,
        None => {
            eprintln!("rmmod: missing module name");
            eprintln!("Try 'rmmod --help' for more information.");
            return 1;
        }
    };

    // Normalize: strip any .ko extension and replace hyphens.
    let clean_name = module_name
        .strip_suffix(".ko")
        .unwrap_or(&module_name)
        .replace('-', "_");

    if verbose {
        let mode = if force { "force " } else { "" };
        println!("rmmod: {mode}removing {clean_name}");
    }

    match do_delete_module(&clean_name, force) {
        Ok(()) => {
            if verbose {
                println!("rmmod: {clean_name} removed");
            }
            0
        }
        Err(e) => {
            eprintln!("rmmod: {e}");
            1
        }
    }
}

// ============================================================================
// Personality detection and main entry point
// ============================================================================

/// Supported tool personalities.
#[derive(Clone, Copy, PartialEq)]
enum Personality {
    Lsmod,
    Modprobe,
    Insmod,
    Rmmod,
}

/// Detect personality from the program name (argv[0]).
///
/// Examines the basename of the program path. Falls back to `Lsmod` if the
/// name is not recognized.
fn detect_personality(argv0: &str) -> Personality {
    // Extract the basename, stripping any directory prefix and extension.
    let basename = argv0
        .rsplit('/')
        .next()
        .unwrap_or(argv0)
        .rsplit('\\')
        .next()
        .unwrap_or(argv0);

    // Strip common extensions (.exe on Windows dev, nothing on target OS).
    let name = basename
        .strip_suffix(".exe")
        .unwrap_or(basename);

    if name.contains("modprobe") {
        Personality::Modprobe
    } else if name.contains("insmod") {
        Personality::Insmod
    } else if name.contains("rmmod") {
        Personality::Rmmod
    } else {
        // Default: lsmod (also covers the bare binary name).
        Personality::Lsmod
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("lsmod");
    let rest = if args.len() > 1 { &args[1..] } else { &[] };

    let personality = detect_personality(argv0);

    let exit_code = match personality {
        Personality::Lsmod => run_lsmod(rest),
        Personality::Modprobe => run_modprobe(rest),
        Personality::Insmod => run_insmod(rest),
        Personality::Rmmod => run_rmmod(rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // === /proc/modules parsing ===

    #[test]
    fn parse_empty_proc_modules() {
        let entries = parse_proc_modules("");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_single_module_no_deps() {
        let content = "e1000 45056 0 - Live 0xffffffffc0800000\n";
        let entries = parse_proc_modules(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "e1000");
        assert_eq!(entries[0].size, 45056);
        assert_eq!(entries[0].refcount, 0);
        assert!(entries[0].used_by.is_empty());
        assert_eq!(entries[0].state, "Live");
    }

    #[test]
    fn parse_module_with_deps() {
        let content = "snd_hda_codec 131072 2 snd_hda_intel,snd_hda_codec_realtek, Live 0xffffffffc0900000\n";
        let entries = parse_proc_modules(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "snd_hda_codec");
        assert_eq!(entries[0].size, 131072);
        assert_eq!(entries[0].refcount, 2);
        assert_eq!(entries[0].used_by.len(), 2);
        assert_eq!(entries[0].used_by[0], "snd_hda_intel");
        assert_eq!(entries[0].used_by[1], "snd_hda_codec_realtek");
    }

    #[test]
    fn parse_multiple_modules() {
        let content = "\
e1000 45056 0 - Live 0xffffffffc0800000
snd 81920 3 snd_hda_codec, Live 0xffffffffc0a00000
usbcore 262144 1 xhci_hcd, Live 0xffffffffc0b00000
";
        let entries = parse_proc_modules(content);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "e1000");
        assert_eq!(entries[1].name, "snd");
        assert_eq!(entries[2].name, "usbcore");
    }

    #[test]
    fn parse_proc_modules_skips_blank_lines() {
        let content = "\n\ne1000 45056 0 - Live 0xffffffffc0800000\n\n";
        let entries = parse_proc_modules(content);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn parse_proc_modules_short_line_skipped() {
        let content = "broken line\ne1000 45056 0 - Live 0xffffffffc0800000\n";
        let entries = parse_proc_modules(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "e1000");
    }

    #[test]
    fn parse_proc_modules_invalid_size_defaults_zero() {
        let content = "badmod notanumber 0 - Live 0x0\n";
        let entries = parse_proc_modules(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, 0);
    }

    #[test]
    fn parse_proc_modules_missing_state_offset() {
        // Only 4 fields: name, size, refcount, deps.
        let content = "minimal 1024 0 -\n";
        let entries = parse_proc_modules(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "minimal");
        assert_eq!(entries[0].state, "Unknown");
        assert_eq!(entries[0].offset, "0x0");
    }

    #[test]
    fn parse_proc_modules_loading_state() {
        let content = "newmod 2048 0 - Loading 0xffffffffc0c00000\n";
        let entries = parse_proc_modules(content);
        assert_eq!(entries[0].state, "Loading");
    }

    // === modules.dep parsing ===

    #[test]
    fn parse_empty_modules_dep() {
        let entries = parse_modules_dep("");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_modules_dep_no_deps() {
        let content = "kernel/drivers/net/e1000.ko:\n";
        let entries = parse_modules_dep(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "e1000");
        assert_eq!(entries[0].path, "kernel/drivers/net/e1000.ko");
        assert!(entries[0].deps.is_empty());
    }

    #[test]
    fn parse_modules_dep_with_deps() {
        let content = "kernel/drivers/net/e1000.ko: kernel/net/mii.ko kernel/net/core.ko\n";
        let entries = parse_modules_dep(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].deps.len(), 2);
        assert_eq!(entries[0].deps[0], "kernel/net/mii.ko");
        assert_eq!(entries[0].deps[1], "kernel/net/core.ko");
    }

    #[test]
    fn parse_modules_dep_skips_comments() {
        let content = "# This is a comment\nkernel/drivers/net/e1000.ko:\n";
        let entries = parse_modules_dep(content);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn parse_modules_dep_skips_blank_lines() {
        let content = "\n\nkernel/drivers/net/e1000.ko:\n\n";
        let entries = parse_modules_dep(content);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn parse_modules_dep_multiple_entries() {
        let content = "\
kernel/drivers/net/e1000.ko: kernel/net/mii.ko
kernel/net/mii.ko:
kernel/sound/snd.ko:
";
        let entries = parse_modules_dep(content);
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn parse_modules_dep_line_without_colon_skipped() {
        let content = "no colon here\nkernel/drivers/net/e1000.ko:\n";
        let entries = parse_modules_dep(content);
        assert_eq!(entries.len(), 1);
    }

    // === module_name_from_path ===

    #[test]
    fn name_from_simple_ko() {
        assert_eq!(module_name_from_path("e1000.ko"), "e1000");
    }

    #[test]
    fn name_from_full_path() {
        assert_eq!(
            module_name_from_path("kernel/drivers/net/e1000.ko"),
            "e1000"
        );
    }

    #[test]
    fn name_from_compressed_ko_gz() {
        assert_eq!(module_name_from_path("snd_hda.ko.gz"), "snd_hda");
    }

    #[test]
    fn name_from_compressed_ko_xz() {
        assert_eq!(module_name_from_path("snd_hda.ko.xz"), "snd_hda");
    }

    #[test]
    fn name_from_compressed_ko_zst() {
        assert_eq!(module_name_from_path("snd_hda.ko.zst"), "snd_hda");
    }

    #[test]
    fn name_replaces_hyphens_with_underscores() {
        assert_eq!(module_name_from_path("snd-hda-intel.ko"), "snd_hda_intel");
    }

    #[test]
    fn name_no_extension_returned_as_is() {
        assert_eq!(module_name_from_path("mymodule"), "mymodule");
    }

    // === Dependency resolution ===

    #[test]
    fn resolve_deps_single_no_deps() {
        let entries = parse_modules_dep("kernel/e1000.ko:\n");
        let chain = resolve_dependencies("e1000", &entries).unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].0, "e1000");
    }

    #[test]
    fn resolve_deps_linear_chain() {
        let content = "\
kernel/a.ko: kernel/b.ko
kernel/b.ko: kernel/c.ko
kernel/c.ko:
";
        let entries = parse_modules_dep(content);
        let chain = resolve_dependencies("a", &entries).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].0, "c");
        assert_eq!(chain[1].0, "b");
        assert_eq!(chain[2].0, "a");
    }

    #[test]
    fn resolve_deps_diamond() {
        // a depends on b and c; both depend on d.
        let content = "\
kernel/a.ko: kernel/b.ko kernel/c.ko
kernel/b.ko: kernel/d.ko
kernel/c.ko: kernel/d.ko
kernel/d.ko:
";
        let entries = parse_modules_dep(content);
        let chain = resolve_dependencies("a", &entries).unwrap();
        assert_eq!(chain.len(), 4);
        // d should appear once, before both b and c.
        let d_pos = chain.iter().position(|(n, _)| n == "d").unwrap();
        let b_pos = chain.iter().position(|(n, _)| n == "b").unwrap();
        let c_pos = chain.iter().position(|(n, _)| n == "c").unwrap();
        let a_pos = chain.iter().position(|(n, _)| n == "a").unwrap();
        assert!(d_pos < b_pos);
        assert!(d_pos < c_pos);
        assert!(b_pos < a_pos);
        assert!(c_pos < a_pos);
    }

    #[test]
    fn resolve_deps_circular_detected() {
        let content = "\
kernel/a.ko: kernel/b.ko
kernel/b.ko: kernel/a.ko
";
        let entries = parse_modules_dep(content);
        let result = resolve_dependencies("a", &entries);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("circular"), "Expected circular dependency error, got: {msg}");
    }

    #[test]
    fn resolve_deps_missing_module() {
        let entries = parse_modules_dep("kernel/a.ko:\n");
        let result = resolve_dependencies("nonexistent", &entries);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_deps_missing_dependency() {
        let content = "kernel/a.ko: kernel/missing.ko\n";
        let entries = parse_modules_dep(content);
        let result = resolve_dependencies("a", &entries);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("missing"), "Expected missing dependency error, got: {msg}");
    }

    #[test]
    fn resolve_deps_hyphen_underscore_normalization() {
        let content = "kernel/snd-hda-intel.ko:\n";
        let entries = parse_modules_dep(content);
        // Query with hyphens should find the underscore-normalized name.
        let chain = resolve_dependencies("snd-hda-intel", &entries).unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].0, "snd_hda_intel");
    }

    // === Personality detection ===

    #[test]
    fn detect_lsmod_from_bare_name() {
        assert_eq!(detect_personality("lsmod"), Personality::Lsmod);
    }

    #[test]
    fn detect_lsmod_from_full_path() {
        assert_eq!(detect_personality("/usr/bin/lsmod"), Personality::Lsmod);
    }

    #[test]
    fn detect_modprobe() {
        assert_eq!(detect_personality("/sbin/modprobe"), Personality::Modprobe);
    }

    #[test]
    fn detect_insmod() {
        assert_eq!(detect_personality("insmod"), Personality::Insmod);
    }

    #[test]
    fn detect_rmmod() {
        assert_eq!(detect_personality("/usr/sbin/rmmod"), Personality::Rmmod);
    }

    #[test]
    fn detect_unknown_defaults_to_lsmod() {
        assert_eq!(detect_personality("mystery"), Personality::Lsmod);
    }

    #[test]
    fn detect_strips_exe_extension() {
        assert_eq!(detect_personality("modprobe.exe"), Personality::Modprobe);
    }

    #[test]
    fn detect_with_backslash_path() {
        assert_eq!(
            detect_personality("C:\\bin\\rmmod.exe"),
            Personality::Rmmod
        );
    }

    // === Argument parsing (modprobe) ===

    #[test]
    fn modprobe_args_basic_load() {
        let args = vec!["e1000".to_string()];
        let opts = parse_modprobe_args(&args).unwrap();
        assert_eq!(opts.module, "e1000");
        assert!(!opts.remove);
        assert!(!opts.dry_run);
        assert!(!opts.verbose);
        assert!(!opts.quiet);
        assert!(!opts.show_depends);
        assert!(opts.params.is_empty());
    }

    #[test]
    fn modprobe_args_remove() {
        let args = vec!["-r".to_string(), "e1000".to_string()];
        let opts = parse_modprobe_args(&args).unwrap();
        assert!(opts.remove);
        assert_eq!(opts.module, "e1000");
    }

    #[test]
    fn modprobe_args_all_flags() {
        let args = vec![
            "-r".to_string(),
            "-n".to_string(),
            "-v".to_string(),
            "-q".to_string(),
            "e1000".to_string(),
        ];
        let opts = parse_modprobe_args(&args).unwrap();
        assert!(opts.remove);
        assert!(opts.dry_run);
        assert!(opts.verbose);
        assert!(opts.quiet);
    }

    #[test]
    fn modprobe_args_with_params() {
        let args = vec![
            "e1000".to_string(),
            "speed=1000".to_string(),
            "duplex=full".to_string(),
        ];
        let opts = parse_modprobe_args(&args).unwrap();
        assert_eq!(opts.module, "e1000");
        assert_eq!(opts.params.len(), 2);
        assert_eq!(opts.params[0], "speed=1000");
        assert_eq!(opts.params[1], "duplex=full");
    }

    #[test]
    fn modprobe_args_no_module_is_error() {
        let args: Vec<String> = vec!["-v".to_string()];
        let result = parse_modprobe_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn modprobe_args_help_returns_empty_error() {
        let args = vec!["--help".to_string()];
        let result = parse_modprobe_args(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_empty());
    }

    #[test]
    fn modprobe_args_unknown_flag_is_error() {
        let args = vec!["--bogus".to_string(), "mod".to_string()];
        let result = parse_modprobe_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn modprobe_args_show_depends() {
        let args = vec!["--show-depends".to_string(), "snd".to_string()];
        let opts = parse_modprobe_args(&args).unwrap();
        assert!(opts.show_depends);
        assert_eq!(opts.module, "snd");
    }

    #[test]
    fn modprobe_args_long_form_flags() {
        let args = vec![
            "--remove".to_string(),
            "--dry-run".to_string(),
            "--verbose".to_string(),
            "--quiet".to_string(),
            "mod".to_string(),
        ];
        let opts = parse_modprobe_args(&args).unwrap();
        assert!(opts.remove);
        assert!(opts.dry_run);
        assert!(opts.verbose);
        assert!(opts.quiet);
    }

    // === format_size ===

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(512), "512 B");
    }

    #[test]
    fn format_size_kib() {
        assert_eq!(format_size(45056), "44.0 KiB");
    }

    #[test]
    fn format_size_mib() {
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MiB");
    }

    // === errno_message ===

    #[test]
    fn errno_known_codes() {
        assert_eq!(errno_message(-1), "operation not permitted");
        assert_eq!(errno_message(-13), "permission denied");
        assert_eq!(errno_message(-16), "device or resource busy");
    }

    #[test]
    fn errno_unknown_code() {
        assert_eq!(errno_message(-9999), "unknown error");
    }

    // === Module info display (lsmod output) ===

    #[test]
    fn lsmod_no_args_returns_zero() {
        // With no /proc/modules on the host, this still returns 0.
        let code = run_lsmod(&[]);
        assert_eq!(code, 0);
    }

    #[test]
    fn lsmod_help_returns_zero() {
        let code = run_lsmod(&["--help".to_string()]);
        assert_eq!(code, 0);
    }

    #[test]
    fn lsmod_unknown_arg_returns_one() {
        let code = run_lsmod(&["--bogus".to_string()]);
        assert_eq!(code, 1);
    }
}
