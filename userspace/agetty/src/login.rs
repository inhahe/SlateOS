//! The command line `login` is run with: by default `login [-h host | -H]
//! [-f] -- NAME`, or, under `--login-options`, the words given with every
//! `\u` replaced by the name -- and the check the name passes first.

use crate::options::{F_NOHOSTNAME, F_REMOTE, Options};
use crate::{Diag, Exit};

/// `LOGIN_ARGV_MAX`: the most words a login command line has, the program's
/// own included.
const LOGIN_ARGV_MAX: usize = 16;

/// C's `isspace` in the "C" locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `replace_u (str, username)`: `word` with each `\u` in it replaced by the
/// name, left to right, the name's own text not searched again. A word that
/// comes to be exactly `\u` with nothing yet put before it is the name
/// itself -- `None` when there is none, which ends upstream's argument
/// vector there.
#[must_use]
pub fn replace_u(word: &[u8], name: Option<&[u8]>) -> Option<Vec<u8>> {
    let user = name.unwrap_or_default();
    let mut s = word.to_vec();
    let mut p = 0usize;
    while p < s.len() {
        if s.get(p..p.saturating_add(2)) != Some(b"\\u") {
            p = p.saturating_add(1);
            continue;
        }
        if p == 0 && s.len() == 2 {
            return name.map(<[u8]>::to_vec);
        }
        let mut next = s.get(..p).unwrap_or_default().to_vec();
        next.extend_from_slice(user);
        let resume = next.len();
        next.extend_from_slice(s.get(p.saturating_add(2)..).unwrap_or_default());
        s = next;
        p = resume;
    }
    Some(s)
}

/// `login_options_to_argv`: `--login-options`'s words, split at runs of
/// blanks, each through [`replace_u`], after the program's name -- at most
/// [`LOGIN_ARGV_MAX`] in all.
#[must_use]
pub fn options_argv(login: &[u8], logopt: &[u8], name: Option<&[u8]>) -> Vec<Option<Vec<u8>>> {
    let mut argv = vec![Some(login.to_vec())];
    let words = logopt
        .split(|&b| is_space(b))
        .filter(|w| !w.is_empty())
        .take(LOGIN_ARGV_MAX.saturating_sub(1));
    argv.extend(words.map(|w| replace_u(w, name)));
    argv
}

/// The command line `login` is run with: `--login-options`' words, or by
/// default the host (`-h`, or `-H` for none) under `--remote`, `-f` for an
/// automatic login, and `-- NAME`. Upstream's argument vector ends at its
/// first null: so does this.
#[must_use]
pub fn login_argv(op: &Options, name: Option<&[u8]>) -> Vec<Vec<u8>> {
    let argv = match &op.logopt {
        Some(logopt) => options_argv(&op.login, logopt, name),
        None => {
            let mut argv = vec![Some(op.login.clone())];
            if op.flags & F_REMOTE != 0 {
                if let Some(host) = &op.fakehost {
                    argv.push(Some(b"-h".to_vec()));
                    argv.push(Some(host.clone()));
                } else if op.flags & F_NOHOSTNAME != 0 {
                    argv.push(Some(b"-H".to_vec()));
                }
            }
            if let Some(name) = name {
                if op.autolog.is_some() {
                    argv.push(Some(b"-f".to_vec()));
                }
                argv.push(Some(b"--".to_vec()));
                argv.push(Some(name.to_vec()));
            }
            argv
        }
    };
    argv.into_iter().map_while(|w| w).collect()
}

/// `check_username (nm)`: refuse a name of more than 42 bytes, or one that
/// begins -- after blanks -- with `-`, which `login` would read as an
/// option.
///
/// # Errors
///
/// The refusal: logged, status 1.
pub fn check_username(name: &[u8], diag: &mut dyn Diag) -> Result<(), Exit> {
    let first = name.iter().copied().find(|&b| !is_space(b));
    if name.len() > 42 || first == Some(b'-') {
        return Err(diag.log_err(b"checkname failed: Operation not permitted"));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::tests::Said;

    fn words(argv: &[Vec<u8>]) -> Vec<&str> {
        argv.iter()
            .map(|w| std::str::from_utf8(w).unwrap())
            .collect()
    }

    #[test]
    fn the_default_command_line() {
        let op = Options::default();
        assert_eq!(
            words(&login_argv(&op, Some(b"ann"))),
            ["/bin/login", "--", "ann"]
        );
        assert_eq!(words(&login_argv(&op, None)), ["/bin/login"]);
        let auto = Options {
            autolog: Some(b"root".to_vec()),
            flags: F_REMOTE,
            fakehost: Some(b"far.example".to_vec()),
            ..Options::default()
        };
        assert_eq!(
            words(&login_argv(&auto, Some(b"root"))),
            ["/bin/login", "-h", "far.example", "-f", "--", "root"]
        );
        let remote = Options {
            flags: F_REMOTE | F_NOHOSTNAME,
            ..Options::default()
        };
        assert_eq!(
            words(&login_argv(&remote, Some(b"x"))),
            ["/bin/login", "-H", "--", "x"]
        );
        // Without --remote, neither.
        let local = Options {
            flags: F_NOHOSTNAME,
            fakehost: Some(b"h".to_vec()),
            ..Options::default()
        };
        assert_eq!(
            words(&login_argv(&local, Some(b"x"))),
            ["/bin/login", "--", "x"]
        );
    }

    #[test]
    fn login_options_replace_the_whole_command_line() {
        let op = Options {
            logopt: Some(b"  -p   -- \\u  ".to_vec()),
            login: b"/sbin/mylogin".to_vec(),
            ..Options::default()
        };
        assert_eq!(
            words(&login_argv(&op, Some(b"ann"))),
            ["/sbin/mylogin", "-p", "--", "ann"]
        );
        // No name: the vector ends where `\u` stood.
        assert_eq!(words(&login_argv(&op, None)), ["/sbin/mylogin", "-p", "--"]);
        let many = Options {
            logopt: Some(b"1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17".to_vec()),
            ..Options::default()
        };
        assert_eq!(login_argv(&many, None).len(), 16);
    }

    #[test]
    fn each_backslash_u_is_the_name() {
        assert_eq!(replace_u(b"x\\uy\\u", Some(b"ann")).unwrap(), b"xannyann");
        assert_eq!(replace_u(b"\\u", Some(b"ann")).unwrap(), b"ann");
        assert_eq!(replace_u(b"plain", None).unwrap(), b"plain");
        // The name's own `\u` is not searched again.
        assert_eq!(replace_u(b"<\\u>", Some(b"\\u")).unwrap(), b"<\\u>");
        // No name: removed -- unless the word comes to be just `\u`.
        assert_eq!(replace_u(b"a\\ub", None).unwrap(), b"ab");
        assert_eq!(replace_u(b"\\u", None), None);
        assert_eq!(replace_u(b"\\u\\u", None), None);
        assert_eq!(replace_u(b"\\", Some(b"x")).unwrap(), b"\\");
    }

    #[test]
    fn a_name_that_looks_like_an_option_is_refused() {
        assert_eq!(check_username(b"ann", &mut Said::default()), Ok(()));
        for bad in [&b"-froot"[..], b" \t-x", &[b'a'; 43]] {
            let mut said = Said::default();
            assert_eq!(check_username(bad, &mut said), Err(Exit(1)));
            assert_eq!(
                said.syslog,
                [b"checkname failed: Operation not permitted".to_vec()]
            );
        }
        assert_eq!(check_username(&[b'a'; 42], &mut Said::default()), Ok(()));
    }
}
