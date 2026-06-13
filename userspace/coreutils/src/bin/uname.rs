//! uname -- print system information.
//!
//! Usage: uname [-a] [-s] [-r] [-m]
//!   -s  print the operating system name (default if no flags)
//!   -r  print the OS release version
//!   -m  print the machine hardware name
//!   -a  print all information

use std::env;
use std::process;

const SYSNAME: &str = "Slate OS";
const RELEASE: &str = "0.1.0";
const MACHINE: &str = "x86_64";

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct UnameFlags {
    sys: bool,
    rel: bool,
    mach: bool,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let flags = match parse_args(&args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("uname: {e}");
            process::exit(1);
        }
    };

    println!("{}", format_output(&flags, SYSNAME, RELEASE, MACHINE));
}

/// Parse uname's argv into `UnameFlags`. Returns an error string for unknown
/// options or unexpected non-flag arguments.
fn parse_args(args: &[String]) -> Result<UnameFlags, String> {
    let mut flags = UnameFlags::default();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg.chars().skip(1) {
                match c {
                    'a' => {
                        flags.sys = true;
                        flags.rel = true;
                        flags.mach = true;
                    }
                    's' => flags.sys = true,
                    'r' => flags.rel = true,
                    'm' => flags.mach = true,
                    _ => return Err(format!("unknown option: -{c}")),
                }
            }
        } else {
            return Err(format!("unexpected argument: {arg}"));
        }
    }

    Ok(flags)
}

/// Format the output line. With no flags set, defaults to just SYSNAME.
fn format_output(flags: &UnameFlags, sys: &str, rel: &str, mach: &str) -> String {
    let mut effective = *flags;
    if !effective.sys && !effective.rel && !effective.mach {
        effective.sys = true;
    }
    let mut parts: Vec<&str> = Vec::new();
    if effective.sys {
        parts.push(sys);
    }
    if effective.rel {
        parts.push(rel);
    }
    if effective.mach {
        parts.push(mach);
    }
    parts.join(" ")
}

impl Clone for UnameFlags {
    fn clone(&self) -> Self {
        *self
    }
}
impl Copy for UnameFlags {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_no_args_returns_default() {
        let f = parse_args(&s(&[])).unwrap();
        assert!(!f.sys && !f.rel && !f.mach);
    }

    #[test]
    fn parse_dash_s() {
        let f = parse_args(&s(&["-s"])).unwrap();
        assert!(f.sys && !f.rel && !f.mach);
    }

    #[test]
    fn parse_dash_r() {
        let f = parse_args(&s(&["-r"])).unwrap();
        assert!(!f.sys && f.rel && !f.mach);
    }

    #[test]
    fn parse_dash_m() {
        let f = parse_args(&s(&["-m"])).unwrap();
        assert!(!f.sys && !f.rel && f.mach);
    }

    #[test]
    fn parse_dash_a_sets_all() {
        let f = parse_args(&s(&["-a"])).unwrap();
        assert!(f.sys && f.rel && f.mach);
    }

    #[test]
    fn parse_combined_short_options() {
        let f = parse_args(&s(&["-srm"])).unwrap();
        assert!(f.sys && f.rel && f.mach);
    }

    #[test]
    fn parse_multiple_args() {
        let f = parse_args(&s(&["-s", "-r"])).unwrap();
        assert!(f.sys && f.rel && !f.mach);
    }

    #[test]
    fn parse_unknown_option() {
        let err = parse_args(&s(&["-x"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_unexpected_positional() {
        let err = parse_args(&s(&["foo"])).unwrap_err();
        assert!(err.contains("unexpected"));
    }

    #[test]
    fn format_default_prints_sysname_only() {
        let f = UnameFlags::default();
        assert_eq!(format_output(&f, "OS", "1.0", "x64"), "OS");
    }

    #[test]
    fn format_all_flags() {
        let f = UnameFlags { sys: true, rel: true, mach: true };
        assert_eq!(format_output(&f, "OS", "1.0", "x64"), "OS 1.0 x64");
    }

    #[test]
    fn format_only_release() {
        let f = UnameFlags { sys: false, rel: true, mach: false };
        assert_eq!(format_output(&f, "OS", "1.0", "x64"), "1.0");
    }

    #[test]
    fn format_only_machine() {
        let f = UnameFlags { sys: false, rel: false, mach: true };
        assert_eq!(format_output(&f, "OS", "1.0", "x64"), "x64");
    }

    #[test]
    fn format_sys_and_mach_skips_release() {
        let f = UnameFlags { sys: true, rel: false, mach: true };
        assert_eq!(format_output(&f, "OS", "1.0", "x64"), "OS x64");
    }

    #[test]
    fn parse_combined_with_unknown_fails() {
        let err = parse_args(&s(&["-sx"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }
}
