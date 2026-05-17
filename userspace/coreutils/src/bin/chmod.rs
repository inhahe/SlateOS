//! chmod — change file mode bits.
//!
//! Usage: chmod [-R] MODE FILE...
//!   MODE is an octal number (e.g. 755) or symbolic (e.g. u+x,g-w).
//!   -R  operate recursively on directories.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut recursive = false;
    let mut positional: Vec<&str> = Vec::new();

    for arg in &args {
        if arg == "-R" || arg == "-r" {
            recursive = true;
        } else {
            positional.push(arg);
        }
    }

    if positional.len() < 2 {
        eprintln!("chmod: missing operand");
        eprintln!("Usage: chmod [-R] MODE FILE...");
        process::exit(1);
    }

    let mode_str = positional[0];
    let mut exit_code = 0;

    for path_str in &positional[1..] {
        let path = Path::new(path_str);
        if recursive && path.is_dir() {
            if let Err(e) = chmod_recursive(path, mode_str) {
                eprintln!("chmod: {path_str}: {e}");
                exit_code = 1;
            }
        } else if let Err(e) = apply_chmod(path, mode_str) {
            eprintln!("chmod: {path_str}: {e}");
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}

fn chmod_recursive(dir: &Path, mode_str: &str) -> Result<(), String> {
    apply_chmod(dir, mode_str)?;

    let entries = fs::read_dir(dir).map_err(|e| format!("{e}"))?;
    for entry_result in entries {
        let entry = entry_result.map_err(|e| format!("{e}"))?;
        let path = entry.path();
        if path.is_dir() {
            chmod_recursive(&path, mode_str)?;
        } else {
            apply_chmod(&path, mode_str)?;
        }
    }
    Ok(())
}

fn apply_chmod(path: &Path, mode_str: &str) -> Result<(), String> {
    // Try octal first
    if let Ok(mode) = u32::from_str_radix(mode_str, 8) {
        let perms = fs::Permissions::from_mode(mode);
        fs::set_permissions(path, perms).map_err(|e| format!("{e}"))?;
        return Ok(());
    }

    // Symbolic mode: parse and apply relative to current permissions
    let meta = fs::metadata(path).map_err(|e| format!("{e}"))?;
    let current = meta.permissions().mode();
    let new_mode = parse_symbolic(current, mode_str)?;
    let perms = fs::Permissions::from_mode(new_mode);
    fs::set_permissions(path, perms).map_err(|e| format!("{e}"))
}

/// Parse symbolic mode like "u+x,g-w,o=r" and apply to current mode.
fn parse_symbolic(current: u32, spec: &str) -> Result<u32, String> {
    let mut mode = current & 0o7777; // keep only permission bits

    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Parse who: u, g, o, a (default = a)
        let mut who_mask: u32 = 0;
        let mut i = 0;
        let bytes = part.as_bytes();

        while i < bytes.len() {
            match bytes[i] {
                b'u' => who_mask |= 0o700,
                b'g' => who_mask |= 0o070,
                b'o' => who_mask |= 0o007,
                b'a' => who_mask |= 0o777,
                _ => break,
            }
            i += 1;
        }

        if who_mask == 0 {
            who_mask = 0o777; // default = 'a'
        }

        if i >= bytes.len() {
            return Err(format!("invalid mode: {part}"));
        }

        let op = bytes[i];
        i += 1;

        // Parse permission chars
        let mut perm_bits: u32 = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'r' => perm_bits |= 0o444,
                b'w' => perm_bits |= 0o222,
                b'x' => perm_bits |= 0o111,
                b's' => perm_bits |= 0o4000 | 0o2000, // setuid + setgid
                b't' => perm_bits |= 0o1000,           // sticky
                _ => break,
            }
            i += 1;
        }

        let effective = perm_bits & who_mask;

        match op {
            b'+' => mode |= effective,
            b'-' => mode &= !effective,
            b'=' => {
                mode &= !who_mask;
                mode |= effective;
            }
            _ => return Err(format!("invalid operator in mode: {part}")),
        }
    }

    Ok(mode)
}
