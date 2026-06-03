//! chown — change file owner and group.
//!
//! Usage: chown [-R] OWNER[:GROUP] FILE...
//!   -R  operate recursively on directories.
//!
//! OWNER and GROUP are numeric UIDs/GIDs (name lookup not yet supported).
//!
//! Built only on unix-family targets (our x86_64-ouros presents as
//! linux-musl, so `cfg(unix)` matches).  On non-unix hosts (e.g.
//! Windows when running `cargo test --workspace`), a stub `main` keeps
//! the workspace compile-clean.

#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
fn main() {
    eprintln!("chown: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process;

// libc-level chown — our POSIX layer provides this.
#[cfg(unix)]
unsafe extern "C" {
    fn chown(path: *const u8, owner: u32, group: u32) -> i32;
}

#[cfg(unix)]
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
        eprintln!("chown: missing operand");
        eprintln!("Usage: chown [-R] OWNER[:GROUP] FILE...");
        process::exit(1);
    }

    let (uid, gid) = parse_owner_group(positional[0]);
    let mut exit_code = 0;

    for path_str in &positional[1..] {
        let path = Path::new(path_str);
        if recursive && path.is_dir() {
            if let Err(e) = chown_recursive(path, uid, gid) {
                eprintln!("chown: {path_str}: {e}");
                exit_code = 1;
            }
        } else if let Err(e) = apply_chown(path, uid, gid) {
            eprintln!("chown: {path_str}: {e}");
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}

#[cfg(unix)]
fn parse_owner_group(spec: &str) -> (Option<u32>, Option<u32>) {
    if let Some((owner_str, group_str)) = spec.split_once(':') {
        let uid = if owner_str.is_empty() {
            None
        } else {
            Some(owner_str.parse::<u32>().unwrap_or_else(|_| {
                eprintln!("chown: invalid user: '{owner_str}' (only numeric UIDs supported)");
                process::exit(1);
            }))
        };
        let gid = if group_str.is_empty() {
            None
        } else {
            Some(group_str.parse::<u32>().unwrap_or_else(|_| {
                eprintln!("chown: invalid group: '{group_str}' (only numeric GIDs supported)");
                process::exit(1);
            }))
        };
        (uid, gid)
    } else {
        let uid = spec.parse::<u32>().unwrap_or_else(|_| {
            eprintln!("chown: invalid user: '{spec}' (only numeric UIDs supported)");
            process::exit(1);
        });
        (Some(uid), None)
    }
}

#[cfg(unix)]
fn chown_recursive(dir: &Path, uid: Option<u32>, gid: Option<u32>) -> Result<(), String> {
    apply_chown(dir, uid, gid)?;

    let entries = fs::read_dir(dir).map_err(|e| format!("{e}"))?;
    for entry_result in entries {
        let entry = entry_result.map_err(|e| format!("{e}"))?;
        let path = entry.path();
        if path.is_dir() {
            chown_recursive(&path, uid, gid)?;
        } else {
            apply_chown(&path, uid, gid)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn apply_chown(path: &Path, uid: Option<u32>, gid: Option<u32>) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("{e}"))?;
    let actual_uid = uid.unwrap_or_else(|| meta.uid());
    let actual_gid = gid.unwrap_or_else(|| meta.gid());

    let path_bytes = path.to_str().ok_or_else(|| "non-UTF-8 path".to_string())?;
    // Build a null-terminated path for the C call.
    let mut c_path: Vec<u8> = path_bytes.as_bytes().to_vec();
    c_path.push(0);

    // SAFETY: c_path is a valid null-terminated string, and chown is
    // provided by the POSIX layer.
    let ret = unsafe { chown(c_path.as_ptr(), actual_uid, actual_gid) };
    if ret != 0 {
        return Err(format!("chown failed (errno)"));
    }
    Ok(())
}
