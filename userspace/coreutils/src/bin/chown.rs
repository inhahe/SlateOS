//! chown — change file owner and group.
//!
//! Usage: chown [-R] OWNER[:GROUP] FILE...
//!   -R  operate recursively on directories.
//!
//! OWNER and GROUP are numeric UIDs/GIDs (name lookup not yet supported).
//!
//! Built only on unix-family targets (our x86_64-slateos presents as
//! linux-musl, so `cfg(unix)` matches).  On non-unix hosts (e.g.
//! Windows when running `cargo test --workspace`), a stub `main` keeps
//! the workspace compile-clean.
//!
//! The pure helpers (`parse_args`, `parse_owner_group`) are exposed on
//! all platforms so they can be unit-tested anywhere.

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

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ChownArgs {
    recursive: bool,
    owner: String,
    paths: Vec<String>,
}

/// Parse chown's argv.  The first non-flag positional argument is the
/// owner spec (`UID`, `UID:GID`, `:GID`, or `UID:`); the rest are paths.
/// Returns an error if there are fewer than two positionals.
fn parse_args(args: &[String]) -> Result<ChownArgs, String> {
    let mut out = ChownArgs::default();
    let mut positional: Vec<&str> = Vec::new();

    for arg in args {
        if arg == "-R" || arg == "-r" {
            out.recursive = true;
        } else {
            positional.push(arg);
        }
    }

    if positional.len() < 2 {
        return Err("missing operand".to_string());
    }

    out.owner = positional.first().copied().unwrap_or_default().to_string();
    out.paths = positional
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    Ok(out)
}

/// Parse an `OWNER[:GROUP]` spec into (uid, gid).  `None` for a field
/// means "leave unchanged".  Empty owner with `:GROUP` is allowed (gid-
/// only change); empty group after `:` is allowed (uid-only).
fn parse_owner_group(spec: &str) -> Result<(Option<u32>, Option<u32>), String> {
    if let Some((owner_str, group_str)) = spec.split_once(':') {
        let uid = if owner_str.is_empty() {
            None
        } else {
            Some(
                owner_str
                    .parse::<u32>()
                    .map_err(|_| format!("invalid user: '{owner_str}' (only numeric UIDs supported)"))?,
            )
        };
        let gid = if group_str.is_empty() {
            None
        } else {
            Some(
                group_str
                    .parse::<u32>()
                    .map_err(|_| format!("invalid group: '{group_str}' (only numeric GIDs supported)"))?,
            )
        };
        Ok((uid, gid))
    } else {
        let uid = spec
            .parse::<u32>()
            .map_err(|_| format!("invalid user: '{spec}' (only numeric UIDs supported)"))?;
        Ok((Some(uid), None))
    }
}

#[cfg(unix)]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("chown: {e}");
            eprintln!("Usage: chown [-R] OWNER[:GROUP] FILE...");
            process::exit(1);
        }
    };

    let (uid, gid) = match parse_owner_group(&parsed.owner) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("chown: {e}");
            process::exit(1);
        }
    };

    let mut exit_code = 0;
    for path_str in &parsed.paths {
        let path = Path::new(path_str);
        if parsed.recursive && path.is_dir() {
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
        return Err("chown failed (errno)".to_string());
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_no_args_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_owner_only_errors() {
        let err = parse_args(&s(&["1000"])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_owner_and_file() {
        let a = parse_args(&s(&["1000", "f"])).unwrap();
        assert!(!a.recursive);
        assert_eq!(a.owner, "1000");
        assert_eq!(a.paths, vec!["f"]);
    }

    #[test]
    fn parse_recursive_dash_r_uppercase() {
        let a = parse_args(&s(&["-R", "1000:100", "dir"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.owner, "1000:100");
        assert_eq!(a.paths, vec!["dir"]);
    }

    #[test]
    fn parse_recursive_lowercase_r_accepted() {
        let a = parse_args(&s(&["-r", "1000", "f"])).unwrap();
        assert!(a.recursive);
    }

    #[test]
    fn parse_multiple_files() {
        let a = parse_args(&s(&["0:0", "a", "b", "c"])).unwrap();
        assert_eq!(a.paths, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_recursive_flag_position_independent() {
        let a = parse_args(&s(&["1000", "-R", "dir"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.owner, "1000");
        assert_eq!(a.paths, vec!["dir"]);
    }

    // ---------------- parse_owner_group ----------------

    #[test]
    fn pog_owner_only() {
        assert_eq!(parse_owner_group("1000").unwrap(), (Some(1000), None));
    }

    #[test]
    fn pog_owner_and_group() {
        assert_eq!(parse_owner_group("1000:100").unwrap(), (Some(1000), Some(100)));
    }

    #[test]
    fn pog_owner_with_empty_group() {
        // "1000:" — uid set, gid unchanged.
        assert_eq!(parse_owner_group("1000:").unwrap(), (Some(1000), None));
    }

    #[test]
    fn pog_empty_owner_with_group() {
        // ":100" — uid unchanged, gid set.
        assert_eq!(parse_owner_group(":100").unwrap(), (None, Some(100)));
    }

    #[test]
    fn pog_both_empty() {
        // ":" — both unchanged (no-op).
        assert_eq!(parse_owner_group(":").unwrap(), (None, None));
    }

    #[test]
    fn pog_root() {
        assert_eq!(parse_owner_group("0").unwrap(), (Some(0), None));
        assert_eq!(parse_owner_group("0:0").unwrap(), (Some(0), Some(0)));
    }

    #[test]
    fn pog_non_numeric_owner_errors() {
        let err = parse_owner_group("alice").unwrap_err();
        assert!(err.contains("invalid user"));
        assert!(err.contains("alice"));
    }

    #[test]
    fn pog_non_numeric_group_errors() {
        let err = parse_owner_group("1000:wheel").unwrap_err();
        assert!(err.contains("invalid group"));
        assert!(err.contains("wheel"));
    }

    #[test]
    fn pog_non_numeric_owner_with_group_errors() {
        let err = parse_owner_group("alice:100").unwrap_err();
        assert!(err.contains("invalid user"));
    }

    #[test]
    fn pog_max_u32() {
        assert_eq!(
            parse_owner_group("4294967295:4294967295").unwrap(),
            (Some(u32::MAX), Some(u32::MAX)),
        );
    }

    #[test]
    fn pog_negative_errors() {
        let err = parse_owner_group("-1").unwrap_err();
        assert!(err.contains("invalid user"));
    }
}
