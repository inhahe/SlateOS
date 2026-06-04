//! logname — print the login name.
//!
//! Usage: logname
//!   Prints the user's login name from $LOGNAME or $USER.
//!   Exits with 1 if the login name cannot be determined.

use std::env;
use std::process;

fn main() {
    match resolve_login_name(&[("LOGNAME", env::var("LOGNAME").ok()), ("USER", env::var("USER").ok())]) {
        Some(name) => println!("{name}"),
        None => {
            eprintln!("logname: no login name");
            process::exit(1);
        }
    }
}

/// Pick the first non-empty value from a list of (var-name, value)
/// candidates. The var-name is purely for readability at call sites; only
/// the value affects the result.
fn resolve_login_name(candidates: &[(&str, Option<String>)]) -> Option<String> {
    for (_name, value) in candidates {
        if let Some(v) = value
            && !v.is_empty()
        {
            return Some(v.clone());
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn first_candidate_wins() {
        let got = resolve_login_name(&[
            ("LOGNAME", s("alice")),
            ("USER", s("bob")),
        ]);
        assert_eq!(got, Some("alice".to_string()));
    }

    #[test]
    fn falls_back_to_second_when_first_unset() {
        let got = resolve_login_name(&[
            ("LOGNAME", None),
            ("USER", s("bob")),
        ]);
        assert_eq!(got, Some("bob".to_string()));
    }

    #[test]
    fn falls_back_when_first_is_empty_string() {
        // Empty environment value is treated as unset — POSIX-typical.
        let got = resolve_login_name(&[
            ("LOGNAME", s("")),
            ("USER", s("bob")),
        ]);
        assert_eq!(got, Some("bob".to_string()));
    }

    #[test]
    fn returns_none_when_all_unset() {
        let got = resolve_login_name(&[
            ("LOGNAME", None),
            ("USER", None),
        ]);
        assert_eq!(got, None);
    }

    #[test]
    fn returns_none_when_all_empty() {
        let got = resolve_login_name(&[
            ("LOGNAME", s("")),
            ("USER", s("")),
        ]);
        assert_eq!(got, None);
    }

    #[test]
    fn empty_candidate_list_returns_none() {
        assert_eq!(resolve_login_name(&[]), None);
    }

    #[test]
    fn single_candidate() {
        assert_eq!(
            resolve_login_name(&[("LOGNAME", s("solo"))]),
            Some("solo".to_string())
        );
    }
}
