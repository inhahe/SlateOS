//! id — print real and effective user and group IDs.
//!
//! Usage: id [-u] [-g] [-n]
//!   (no flags)  print full uid/gid info
//!   -u          print only effective UID
//!   -g          print only effective GID
//!   -n          print name instead of number (not yet supported — prints number)

use std::env;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn getgid() -> u32;
    fn getegid() -> u32;
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct IdArgs {
    show_uid_only: bool,
    show_gid_only: bool,
    /// Unknown short flags collected so the caller can warn about them
    /// without aborting; matches the existing behaviour.
    unknown: Vec<char>,
}

/// Parse id's argv.  Treats every argument starting with `-` as a cluster
/// of short flags; bare arguments are ignored (the real `id` accepts a
/// USER operand, but our build doesn't have a name db yet).
fn parse_args(args: &[String]) -> IdArgs {
    let mut out = IdArgs::default();
    for arg in args {
        if let Some(flags) = arg.strip_prefix('-') {
            for c in flags.chars() {
                match c {
                    'u' => out.show_uid_only = true,
                    'g' => out.show_gid_only = true,
                    'n' => {} // name mode — accepted but ignored
                    other => out.unknown.push(other),
                }
            }
        }
    }
    out
}

/// Format the long "uid=N gid=N [euid=N] [egid=N]" output line.  euid and
/// egid are only included when they differ from their non-effective
/// counterparts.
fn format_full(uid: u32, euid: u32, gid: u32, egid: u32) -> String {
    let mut out = format!("uid={uid}");
    if euid != uid {
        out.push_str(&format!(" euid={euid}"));
    }
    out.push_str(&format!(" gid={gid}"));
    if egid != gid {
        out.push_str(&format!(" egid={egid}"));
    }
    out
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = parse_args(&args);

    for c in &parsed.unknown {
        eprintln!("id: unknown option: -{c}");
    }

    #[cfg(target_os = "linux")]
    let (uid, euid, gid, egid) = {
        // SAFETY: these are simple POSIX getters with no pointer arguments.
        let uid = unsafe { getuid() };
        let euid = unsafe { geteuid() };
        let gid = unsafe { getgid() };
        let egid = unsafe { getegid() };
        (uid, euid, gid, egid)
    };
    #[cfg(not(target_os = "linux"))]
    let (uid, euid, gid, egid): (u32, u32, u32, u32) = (0, 0, 0, 0);

    if parsed.show_uid_only {
        println!("{euid}");
    } else if parsed.show_gid_only {
        println!("{egid}");
    } else {
        println!("{}", format_full(uid, euid, gid, egid));
    }
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
    fn parse_no_args_is_default() {
        let a = parse_args(&s(&[]));
        assert!(!a.show_uid_only);
        assert!(!a.show_gid_only);
        assert!(a.unknown.is_empty());
    }

    #[test]
    fn parse_u_flag() {
        let a = parse_args(&s(&["-u"]));
        assert!(a.show_uid_only);
        assert!(!a.show_gid_only);
    }

    #[test]
    fn parse_g_flag() {
        let a = parse_args(&s(&["-g"]));
        assert!(a.show_gid_only);
        assert!(!a.show_uid_only);
    }

    #[test]
    fn parse_n_flag_accepted_silently() {
        let a = parse_args(&s(&["-n"]));
        assert!(a.unknown.is_empty());
    }

    #[test]
    fn parse_clustered_flags() {
        let a = parse_args(&s(&["-un"]));
        assert!(a.show_uid_only);
        assert!(a.unknown.is_empty());
    }

    #[test]
    fn parse_unknown_flag_recorded() {
        let a = parse_args(&s(&["-X"]));
        assert_eq!(a.unknown, vec!['X']);
    }

    #[test]
    fn parse_mixed_known_and_unknown() {
        let a = parse_args(&s(&["-uXg"]));
        assert!(a.show_uid_only);
        assert!(a.show_gid_only);
        assert_eq!(a.unknown, vec!['X']);
    }

    #[test]
    fn parse_bare_arg_ignored() {
        let a = parse_args(&s(&["someuser"]));
        assert!(!a.show_uid_only);
        assert!(!a.show_gid_only);
        assert!(a.unknown.is_empty());
    }

    #[test]
    fn parse_multiple_flag_groups() {
        let a = parse_args(&s(&["-u", "-g"]));
        assert!(a.show_uid_only);
        assert!(a.show_gid_only);
    }

    // ---------------- format_full ----------------

    #[test]
    fn format_full_equal_uid_euid_gid_egid() {
        // No euid/egid shown when real == effective.
        assert_eq!(format_full(1000, 1000, 1000, 1000), "uid=1000 gid=1000");
    }

    #[test]
    fn format_full_distinct_euid() {
        assert_eq!(
            format_full(1000, 0, 1000, 1000),
            "uid=1000 euid=0 gid=1000",
        );
    }

    #[test]
    fn format_full_distinct_egid() {
        assert_eq!(
            format_full(1000, 1000, 1000, 4),
            "uid=1000 gid=1000 egid=4",
        );
    }

    #[test]
    fn format_full_all_distinct() {
        assert_eq!(
            format_full(1000, 0, 1000, 0),
            "uid=1000 euid=0 gid=1000 egid=0",
        );
    }

    #[test]
    fn format_full_root() {
        assert_eq!(format_full(0, 0, 0, 0), "uid=0 gid=0");
    }
}
