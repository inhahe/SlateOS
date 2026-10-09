//! The one question util-linux asks of terminfo: how many colours a terminal
//! has -- `setupterm (NULL, STDOUT_FILENO, &ret) == 0 && ret == 1`, then
//! `tigetnum ("colors")`, from ncurses on the reference.
//!
//! Answered by `userspace/terminfo`, which finds and reads the entry as
//! ncurses does: `$TERMINFO`, `$HOME/.terminfo`, `$TERMINFO_DIRS`, then
//! `/etc/terminfo`, `/lib/terminfo` and `/usr/share/terminfo`; a name that
//! is empty, `.`, `..` or holds `/` or `:` is no terminal; and `setupterm`
//! fails for a generic or hard-copy terminal too, which therefore has no
//! colours here, as it has none upstream.

/// The colour count `term`'s entry gives -- `tigetnum ("colors")`, -1 when
/// it gives none -- or `None` when `setupterm` fails: no entry, or one it
/// will not set up.
#[must_use]
pub fn colors(
    term: &[u8],
    terminfo: Option<&[u8]>,
    home: Option<&[u8]>,
    terminfo_dirs: Option<&[u8]>,
) -> Option<i32> {
    let env = terminfo::Env {
        terminfo: terminfo.map(<[u8]>::to_vec),
        home: home.map(<[u8]>::to_vec),
        terminfo_dirs: terminfo_dirs.map(<[u8]>::to_vec),
        trusted: terminfo::env_access(),
        // `$CC` rewrites strings only; `colors` is a number.
        cc: None,
    };
    let setup = terminfo::setupterm(None, Some(term), &env);
    if setup.complaint.is_some() || setup.status != terminfo::TGETENT_YES {
        return None;
    }
    let n = setup.entry?.number(terminfo::number::MAX_COLORS);
    // `tigetnum`: an absent or cancelled number is -1.
    Some(if n >= 0 { n } else { -1 })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// A compiled entry, 16-bit or 32-bit, whose `colors` is `c`, and
    /// whose booleans are `bools`.
    #[cfg(unix)]
    fn entry(wide: bool, names: &[u8], bools: &[u8], c: i32) -> Vec<u8> {
        let magic: u16 = if wide { 0o1036 } else { 0o432 };
        let mut names = names.to_vec();
        names.push(0);
        let mut t = Vec::new();
        for w in [magic, names.len() as u16, bools.len() as u16, 14, 0, 0] {
            t.extend_from_slice(&w.to_le_bytes());
        }
        t.extend_from_slice(&names);
        t.extend_from_slice(bools);
        if t.len() % 2 == 1 {
            t.push(0);
        }
        for k in 0..14 {
            let v: i32 = if k == 13 { c } else { -1 };
            if wide {
                t.extend_from_slice(&v.to_le_bytes());
            } else {
                t.extend_from_slice(&(v as i16).to_le_bytes());
            }
        }
        t
    }

    /// A `$TERMINFO` holding the given entries, removed when dropped.
    #[cfg(unix)]
    struct Dir(std::path::PathBuf);

    #[cfg(unix)]
    impl Dir {
        fn new(tag: &str, entries: &[(&str, Vec<u8>)]) -> Self {
            let d =
                std::env::temp_dir().join(format!("ulcolors-tinfo-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            for (name, data) in entries {
                let sub = d.join(&name[..1]);
                std::fs::create_dir_all(&sub).unwrap();
                std::fs::write(sub.join(name), data).unwrap();
            }
            Self(d)
        }
        fn bytes(&self) -> Vec<u8> {
            self.0.to_string_lossy().into_owned().into_bytes()
        }
    }

    #[cfg(unix)]
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // A Windows path's drive colon is a search-list separator.
    #[cfg(unix)]
    #[test]
    fn both_formats_and_absent_colours() {
        let mut generic = vec![0u8; 8];
        generic[6] = 1;
        let dir = Dir::new(
            "formats",
            &[
                ("zz-256", entry(true, b"zz-256|x", &[0; 38], 256)),
                ("zz-mono", entry(false, b"zz-mono", &[0; 3], -1)),
                ("zz-ansi", entry(false, b"zz-ansi", &[0; 4], 8)),
                ("zz-gn", entry(false, b"zz-gn", &generic, 8)),
            ],
        );
        let at = dir.bytes();
        assert_eq!(colors(b"zz-256", Some(&at), None, None), Some(256));
        assert_eq!(colors(b"zz-mono", Some(&at), None, None), Some(-1));
        assert_eq!(colors(b"zz-ansi", Some(&at), None, None), Some(8));
        // Generic: `setupterm` wants something more specific.
        assert_eq!(colors(b"zz-gn", Some(&at), None, None), None);
        assert_eq!(colors(b"zz-none", Some(&at), None, None), None);
    }

    #[test]
    fn names_ncurses_refuses() {
        assert_eq!(colors(b"", None, None, None), None);
        assert_eq!(colors(b"../x", None, None, None), None);
        assert_eq!(colors(b"..", None, None, None), None);
        assert_eq!(colors(b"xterm:x", None, None, None), None);
    }
}
