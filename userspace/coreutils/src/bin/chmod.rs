//! chmod — change file mode bits.
//!
//! Usage: chmod [-R|--recursive] [--] MODE FILE...
//!   MODE is an octal number (e.g. 755) or symbolic (e.g. u+x,g-w).
//!   -R, --recursive  operate recursively on directories.
//!   --               end of options; everything after is MODE then FILEs.
//!
//! Note the asymmetry between `-R` and `-r`: **`-R` is the recursion flag and
//! `-r` is a mode**, the `-` operator applied to the read bit, exactly like the
//! `-w` in `chmod -w file`. POSIX specifies the mode operand may begin with a
//! `-`, which is why `parse_args` cannot simply treat every leading dash as an
//! option. Getting this wrong is not a cosmetic incompatibility: `chmod -r f`
//! is the ordinary way to make a file unreadable, and reading its `-r` as
//! "recurse" leaves the file readable while reporting a usage error.
//!
//! # Symbolic links, and why `-R` never touches one
//!
//! Under `-R`, every path after the operand was chosen by whoever wrote the
//! files, not by the caller — which under `/tmp`, a download directory, or a
//! user's home is not the same person. A symbolic link met during the walk is
//! therefore an instruction from a stranger, and this program obeys none of
//! them (`known-issues.md` → `B-chmod-FOLLOWS-SYMLINKS-WHILE-RECURSING`):
//!
//! 1. **It is not descended into.** The recursion used to test `path.is_dir()`,
//!    which follows links, so `srv/x -> /etc` turned `chmod -R 777 srv/` into
//!    `chmod -R 777 /etc`. It now tests the link's own type.
//! 2. **It is skipped entirely**, as GNU chmod skips it. A symlink's own mode
//!    bits are never consulted by anything, so the only effect a `chmod` on one
//!    can have is on its target — and `srv/x -> /etc/shadow` would otherwise
//!    make `/etc/shadow` world-writable.
//!
//! A link named directly on the command line *is* followed, also matching GNU
//! (`fts` with `FTS_COMFOLLOW`): the caller typed that name and can see what it
//! is.
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
use coreutils::quote::quotef_os;
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

/// Characters that may follow a leading `-` in a *mode operand* rather than an
/// option: the three operators, the permission letters, the `ugoa` who-letters,
/// the `st` special bits, and the `,` that separates clauses. Deliberately does
/// not contain `R`, which is the recursion flag — that one letter is the whole
/// difference between `chmod -R` and `chmod -r`.
const MODE_OPERAND_CHARS: &str = "rwxXstugoa,+-=";

/// What a recursive walk does with one entry it found.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum WalkAction {
    /// A symbolic link: leave it, and everything it points at, alone.
    Skip,
    /// A real directory: walk into it.
    Descend,
    /// Anything else: chmod it.
    Apply,
}

/// Decide what to do with a walk entry from its `lstat` file type.
///
/// Extracted as a pure function purely so it can be tested: the walk itself is
/// `cfg(unix)`, so on the build host — where `cargo test --workspace` runs —
/// not one line of it is compiled, and a rule nobody can test is a rule that
/// quietly stops holding. `is_symlink` is checked first and wins outright,
/// which is the entire fix: `is_dir` alone had already followed the link by the
/// time it answered.
fn walk_action(is_symlink: bool, is_dir: bool) -> WalkAction {
    if is_symlink {
        WalkAction::Skip
    } else if is_dir {
        WalkAction::Descend
    } else {
        WalkAction::Apply
    }
}

/// Whether `arg` is a mode written with a leading `-`, e.g. `-r`, `-w`, `-rw`.
///
/// POSIX allows the mode operand to start with `-`, so a leading dash cannot be
/// taken as proof of an option. This is the test that separates the two.
fn is_mode_operand(arg: &str) -> bool {
    match arg.strip_prefix('-') {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| MODE_OPERAND_CHARS.contains(c)),
        None => false,
    }
}

/// Parse chmod's argv.  The first positional argument is the mode string; the
/// rest are file paths.  Returns an error if there are fewer than two
/// positionals (i.e. nothing to chmod), or if an argument is an option we do
/// not recognise.
///
/// An unknown leading-dash argument is rejected rather than silently taken as a
/// path, because the alternative is a `chmod -v 644 f` that reports
/// `invalid operator in mode: -v` — an error about the wrong thing, pointing
/// the reader at their mode instead of at the flag we do not implement.
fn parse_args(args: &[String]) -> Result<ChmodArgs, String> {
    let mut out = ChmodArgs::default();
    let mut positional: Vec<&str> = Vec::new();
    let mut end_of_options = false;

    for arg in args {
        if end_of_options {
            positional.push(arg);
            continue;
        }
        // `--` exists so a file genuinely named `-r` can be addressed.
        if arg == "--" {
            end_of_options = true;
            continue;
        }
        if arg == "-R" || arg == "--recursive" {
            out.recursive = true;
            continue;
        }
        // A bare `-` is not an option; let it fall through and fail as a mode,
        // which is what it is being used as.
        if arg.starts_with('-') && arg != "-" && !is_mode_operand(arg) {
            return Err(if arg.starts_with("--") {
                format!("unrecognized option '{arg}'")
            } else {
                format!("invalid option -- '{}'", arg.trim_start_matches('-'))
            });
        }
        positional.push(arg);
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
            eprintln!("Usage: chmod [-R|--recursive] [--] MODE FILE...");
            process::exit(1);
        }
    };

    let mut exit_code = 0;
    for path_str in &parsed.paths {
        let path = Path::new(path_str);
        // `is_dir()` follows, which is correct *here*: a command-line symlink
        // is dereferenced, as it is by GNU chmod. Inside `chmod_recursive` the
        // opposite rule applies — see the module doc.
        if parsed.recursive && path.is_dir() {
            chmod_recursive(path, &parsed.mode, &mut exit_code);
        } else if let Err(e) = apply_chmod(path, &parsed.mode) {
            eprintln!("chmod: {}: {e}", quotef_os(path_str));
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}

/// Apply `mode_str` to `dir` and everything beneath it.
///
/// Errors are reported and recorded in `exit_code` rather than returned,
/// because a walk that stops at the first failure leaves the caller with a tree
/// in an unknown state: `chmod -R 700 ~` hitting one unreadable subdirectory
/// used to abandon every sibling after it, silently leaving them world-
/// readable. GNU chmod reports and keeps going, and so does this.
#[cfg(unix)]
fn chmod_recursive(dir: &Path, mode_str: &str, exit_code: &mut i32) {
    if let Err(e) = apply_chmod(dir, mode_str) {
        eprintln!("chmod: {}: {e}", quotef_os(dir));
        *exit_code = 1;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("chmod: {}: {e}", quotef_os(dir));
            *exit_code = 1;
            return;
        }
    };
    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!("chmod: {}: {e}", quotef_os(dir));
                *exit_code = 1;
                continue;
            }
        };
        let path = entry.path();
        // `file_type()` is lstat-based: a symlink to a directory reports
        // `is_symlink()`, not `is_dir()`, so neither branch below can leave the
        // tree. An entry whose type cannot be read is treated as a link — the
        // conservative guess, costing at worst one file left unchanged.
        let Ok(ft) = entry.file_type() else {
            eprintln!(
                "chmod: {}: cannot determine file type; skipping",
                quotef_os(&path)
            );
            *exit_code = 1;
            continue;
        };
        match walk_action(ft.is_symlink(), ft.is_dir()) {
            // Deliberately silent: GNU chmod skips links without comment, and
            // a tree full of them would otherwise bury the real diagnostics.
            WalkAction::Skip => {}
            WalkAction::Descend => chmod_recursive(&path, mode_str, exit_code),
            WalkAction::Apply => {
                if let Err(e) = apply_chmod(&path, mode_str) {
                    eprintln!("chmod: {}: {e}", quotef_os(&path));
                    *exit_code = 1;
                }
            }
        }
    }
}

#[cfg(unix)]
fn apply_chmod(path: &Path, mode_str: &str) -> Result<(), String> {
    // The current mode is the base a symbolic spec modifies, so a failure to
    // read it is fatal to the operation rather than something to paper over.
    // This used to be `.unwrap_or(0)`, which turned an unreadable mode into
    // "no bits set" and made `chmod u+x f` *clear every other permission on
    // the file* instead of failing.
    let current = fs::metadata(path)
        .map(|m| m.permissions().mode())
        .map_err(|e| format!("{e}"))?;
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

    // ---------------- walk_action (symlink policy) ----------------
    //
    // These guard a security boundary, so they are written as the rule rather
    // than as the code: a red one here is a question about the rule, not an
    // assertion to update. See known-issues.md →
    // `B-chmod-FOLLOWS-SYMLINKS-WHILE-RECURSING`.

    #[test]
    fn walk_skips_a_symlink() {
        assert_eq!(walk_action(true, false), WalkAction::Skip);
    }

    #[test]
    fn walk_skips_a_symlink_to_a_directory_rather_than_descending() {
        // The whole bug in one assertion. `is_dir()` on a link to a directory
        // answers true, and answering "descend" to that is how `chmod -R`
        // walked out of the tree it was given.
        assert_eq!(walk_action(true, true), WalkAction::Skip);
    }

    #[test]
    fn walk_descends_into_a_real_directory() {
        assert_eq!(walk_action(false, true), WalkAction::Descend);
    }

    #[test]
    fn walk_applies_to_a_regular_file() {
        assert_eq!(walk_action(false, false), WalkAction::Apply);
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

    // `-r` is a MODE, not a flag. This previously asserted the opposite, which
    // is how the bug survived: fixing the parser turned an existing test red,
    // which reads like a regression rather than the fix it is.
    #[test]
    fn parse_lowercase_r_is_a_mode_not_recursion() {
        let a = parse_args(&s(&["-r", "f"])).unwrap();
        assert!(!a.recursive);
        assert_eq!(a.mode, "-r");
        assert_eq!(a.paths, vec!["f"]);
    }

    #[test]
    fn parse_lowercase_r_alone_is_missing_operand() {
        let err = parse_args(&s(&["-r"])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    // The symmetry that was broken: `-w` always worked because it fell through
    // to the mode parser; `-r` did not, only because it was special-cased.
    #[test]
    fn parse_dash_w_and_dash_r_behave_alike() {
        for spec in ["-r", "-w", "-x", "-rw", "-rwx"] {
            let a = parse_args(&s(&[spec, "f"])).unwrap();
            assert!(!a.recursive, "{spec} must not imply recursion");
            assert_eq!(a.mode, spec);
        }
    }

    #[test]
    fn parse_long_recursive() {
        let a = parse_args(&s(&["--recursive", "755", "dir"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.mode, "755");
        assert_eq!(a.paths, vec!["dir"]);
    }

    #[test]
    fn parse_double_dash_ends_options() {
        // Without `--` there is no way to name a file `-R`.
        let a = parse_args(&s(&["--", "644", "-R"])).unwrap();
        assert!(!a.recursive);
        assert_eq!(a.mode, "644");
        assert_eq!(a.paths, vec!["-R"]);
    }

    #[test]
    fn parse_unknown_short_option_rejected() {
        let err = parse_args(&s(&["-v", "644", "f"])).unwrap_err();
        assert!(err.contains("invalid option"), "got: {err}");
    }

    #[test]
    fn parse_unknown_long_option_rejected() {
        let err = parse_args(&s(&["--verbose", "644", "f"])).unwrap_err();
        assert!(err.contains("unrecognized option"), "got: {err}");
    }

    #[test]
    fn mode_operand_classification() {
        for yes in ["-r", "-w", "-x", "-rwx", "-u+x", "-a=r", "-rw,-x"] {
            assert!(is_mode_operand(yes), "{yes} should be a mode operand");
        }
        for no in ["-R", "-v", "--recursive", "644", "u+x", "-", ""] {
            assert!(!is_mode_operand(no), "{no} should not be a mode operand");
        }
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
