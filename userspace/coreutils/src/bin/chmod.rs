//! chmod — change file mode bits.
//!
//! Usage: chmod [-R] MODE FILE...
//!   MODE is an octal number (e.g. 755) or symbolic (e.g. u+x,g-w).
//!   -R  operate recursively on directories.
//!
//! Built only on unix-family targets (our x86_64-slateos presents as
//! linux-musl, so `cfg(unix)` matches).  On non-unix hosts (e.g.
//! Windows when running `cargo test --workspace`), a stub `main` keeps
//! the workspace compile-clean.
//!
//! The pure helpers (`parse_args`, `parse_symbolic`, `apply_mode_string`)
//! are exposed on all platforms so they can be unit-tested anywhere.

#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
fn main() {
    eprintln!("chmod: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ChmodArgs {
    recursive: bool,
    mode: String,
    paths: Vec<String>,
}

/// Parse chmod's argv.  The first non-flag positional argument is the
/// mode string; the rest are file paths.  Returns an error if there are
/// fewer than two positionals (i.e. nothing to chmod).
fn parse_args(args: &[String]) -> Result<ChmodArgs, String> {
    let mut out = ChmodArgs::default();
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

    out.mode = positional.first().copied().unwrap_or_default().to_string();
    out.paths = positional
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    Ok(out)
}

/// Resolve a mode spec (octal or symbolic) against an optional current
/// permission word, returning the new u32 mode.  Octal specs ignore the
/// current value entirely; symbolic specs mask `current` to its permission
/// bits and apply each clause in order.
fn apply_mode_string(spec: &str, current: u32) -> Result<u32, String> {
    if let Ok(mode) = u32::from_str_radix(spec, 8) {
        return Ok(mode);
    }
    parse_symbolic(current, spec)
}

/// Parse symbolic mode like "u+x,g-w,o=r" and apply to `current`.
fn parse_symbolic(current: u32, spec: &str) -> Result<u32, String> {
    let mut mode = current & 0o7777; // keep only permission bits

    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Parse who: u, g, o, a (default = a)
        let mut who_mask: u32 = 0;
        let mut i: usize = 0;
        let bytes = part.as_bytes();

        while i < bytes.len() {
            let Some(b) = bytes.get(i) else { break };
            match *b {
                b'u' => who_mask |= 0o700,
                b'g' => who_mask |= 0o070,
                b'o' => who_mask |= 0o007,
                b'a' => who_mask |= 0o777,
                _ => break,
            }
            i = i.saturating_add(1);
        }

        if who_mask == 0 {
            who_mask = 0o777; // default = 'a'
        }

        if i >= bytes.len() {
            return Err(format!("invalid mode: {part}"));
        }

        let op = *bytes.get(i).unwrap_or(&0);
        i = i.saturating_add(1);

        // Parse permission chars.  `perm_bits` collects the rwx bits
        // (masked by `who_mask`); `high_bits` collects setuid/setgid/
        // sticky, which live outside the 0o777 range and would be
        // erased if naively masked by who_mask.
        let mut perm_bits: u32 = 0;
        let mut high_bits: u32 = 0;
        while i < bytes.len() {
            let Some(b) = bytes.get(i) else { break };
            match *b {
                b'r' => perm_bits |= 0o444,
                b'w' => perm_bits |= 0o222,
                b'x' => perm_bits |= 0o111,
                b's' => {
                    // `s` on `u` → setuid (0o4000); on `g` → setgid
                    // (0o2000).  POSIX leaves "o+s" implementation-
                    // defined; we treat it as a no-op.
                    if who_mask & 0o700 != 0 {
                        high_bits |= 0o4000;
                    }
                    if who_mask & 0o070 != 0 {
                        high_bits |= 0o2000;
                    }
                }
                b't' => high_bits |= 0o1000, // sticky (not who-scoped)
                _ => break,
            }
            i = i.saturating_add(1);
        }

        let effective = (perm_bits & who_mask) | high_bits;

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

#[cfg(unix)]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("chmod: {e}");
            eprintln!("Usage: chmod [-R] MODE FILE...");
            process::exit(1);
        }
    };

    let mut exit_code = 0;
    for path_str in &parsed.paths {
        let path = Path::new(path_str);
        if parsed.recursive && path.is_dir() {
            if let Err(e) = chmod_recursive(path, &parsed.mode) {
                eprintln!("chmod: {path_str}: {e}");
                exit_code = 1;
            }
        } else if let Err(e) = apply_chmod(path, &parsed.mode) {
            eprintln!("chmod: {path_str}: {e}");
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}

#[cfg(unix)]
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

#[cfg(unix)]
fn apply_chmod(path: &Path, mode_str: &str) -> Result<(), String> {
    // Look up the current mode lazily — only needed for symbolic specs,
    // but the helper always accepts it.
    let current = fs::metadata(path)
        .map(|m| m.permissions().mode())
        .unwrap_or(0);
    let new_mode = apply_mode_string(mode_str, current)?;
    let perms = fs::Permissions::from_mode(new_mode);
    fs::set_permissions(path, perms).map_err(|e| format!("{e}"))
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
    fn parse_mode_only_errors() {
        let err = parse_args(&s(&["755"])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_mode_and_one_file() {
        let a = parse_args(&s(&["755", "a.txt"])).unwrap();
        assert!(!a.recursive);
        assert_eq!(a.mode, "755");
        assert_eq!(a.paths, vec!["a.txt"]);
    }

    #[test]
    fn parse_recursive_dash_r_uppercase() {
        let a = parse_args(&s(&["-R", "755", "dir"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.mode, "755");
        assert_eq!(a.paths, vec!["dir"]);
    }

    #[test]
    fn parse_recursive_lowercase_r_accepted() {
        let a = parse_args(&s(&["-r", "644", "f"])).unwrap();
        assert!(a.recursive);
    }

    #[test]
    fn parse_multiple_files() {
        let a = parse_args(&s(&["644", "a", "b", "c"])).unwrap();
        assert_eq!(a.paths, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_recursive_flag_anywhere() {
        let a = parse_args(&s(&["755", "-R", "dir"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.mode, "755");
        assert_eq!(a.paths, vec!["dir"]);
    }

    // ---------------- apply_mode_string ----------------

    #[test]
    fn apply_octal_ignores_current() {
        assert_eq!(apply_mode_string("755", 0o000).unwrap(), 0o755);
        assert_eq!(apply_mode_string("755", 0o777).unwrap(), 0o755);
        assert_eq!(apply_mode_string("0", 0o777).unwrap(), 0);
    }

    #[test]
    fn apply_octal_four_digit() {
        assert_eq!(apply_mode_string("4755", 0).unwrap(), 0o4755);
    }

    #[test]
    fn apply_symbolic_falls_through_to_parse_symbolic() {
        // "u+x" against 0o644 → 0o744
        assert_eq!(apply_mode_string("u+x", 0o644).unwrap(), 0o744);
    }

    // ---------------- parse_symbolic ----------------

    #[test]
    fn sym_u_plus_x() {
        assert_eq!(parse_symbolic(0o644, "u+x").unwrap(), 0o744);
    }

    #[test]
    fn sym_g_minus_w() {
        assert_eq!(parse_symbolic(0o664, "g-w").unwrap(), 0o644);
    }

    #[test]
    fn sym_o_equals_r() {
        // "o=r": clear other bits, then set r.
        assert_eq!(parse_symbolic(0o777, "o=r").unwrap(), 0o774);
    }

    #[test]
    fn sym_a_plus_x() {
        // "a+x" = "ugo+x".
        assert_eq!(parse_symbolic(0o644, "a+x").unwrap(), 0o755);
    }

    #[test]
    fn sym_default_who_is_a() {
        // "+x" with no who-mask defaults to 'a'.
        assert_eq!(parse_symbolic(0o644, "+x").unwrap(), 0o755);
    }

    #[test]
    fn sym_multiple_clauses_comma_separated() {
        // 0o644 → u+x → 0o744 → g-r → 0o704
        assert_eq!(parse_symbolic(0o644, "u+x,g-r").unwrap(), 0o704);
    }

    #[test]
    fn sym_setuid_with_s() {
        // "u+s" sets setuid (0o4000), masked by who=u → 0o4000.
        assert_eq!(parse_symbolic(0o755, "u+s").unwrap(), 0o4755);
    }

    #[test]
    fn sym_sticky_with_t() {
        // "+t" sets sticky bit (0o1000); default who=a but 0o1000 isn't
        // affected by who-mask (only the 0o777 bits are).
        let m = parse_symbolic(0o777, "+t").unwrap();
        assert_eq!(m & 0o1000, 0o1000);
    }

    #[test]
    fn sym_high_bits_in_current_are_preserved_until_explicit_change() {
        // 0o7755 & 0o7777 → kept; u+x adds nothing new.
        let m = parse_symbolic(0o7755, "u+x").unwrap();
        assert_eq!(m, 0o7755);
    }

    #[test]
    fn sym_empty_string_keeps_current_perms() {
        // No clauses → mode = current & 0o7777, unchanged.
        assert_eq!(parse_symbolic(0o644, "").unwrap(), 0o644);
    }

    #[test]
    fn sym_only_who_no_op_errors() {
        let err = parse_symbolic(0o644, "u").unwrap_err();
        assert!(err.contains("invalid mode"));
    }

    #[test]
    fn sym_invalid_operator_errors() {
        // "u@x" — '@' isn't a valid op.
        let err = parse_symbolic(0o644, "u@x").unwrap_err();
        assert!(err.contains("invalid operator"));
    }

    #[test]
    fn sym_equals_clears_who_bits_then_sets() {
        // u=r against 0o755: clear 0o700, set 0o400 → 0o455.
        assert_eq!(parse_symbolic(0o755, "u=r").unwrap(), 0o455);
    }

    #[test]
    fn sym_minus_does_nothing_if_bits_absent() {
        assert_eq!(parse_symbolic(0o444, "u-x").unwrap(), 0o444);
    }
}
