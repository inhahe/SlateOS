//! The parts of init that are functions of their input alone, kept apart
//! from `main.rs`'s system calls so that the host can test them:
//!
//! ```sh
//! cd services/init
//! cargo test --lib --target x86_64-pc-windows-gnu
//! ```
//!
//! On the target this is `no_std`, like the binary that uses it.

#![cfg_attr(not(test), no_std)]

/// Linux's `HOST_NAME_MAX`, and the most `SYS_HOSTNAME_SET` takes.
pub const HOST_NAME_MAX: usize = 64;

/// What `/etc/hostname` says, read the way systemd's `read_etc_hostname()`
/// reads it: the first line that is neither empty nor a comment, with the
/// white space around it removed.
#[derive(Debug, PartialEq, Eq)]
pub enum EtcHostname<'a> {
    /// A valid host name, with one trailing dot dropped.
    Name(&'a [u8]),
    /// No name at all: an empty file, or only blank lines and comments.
    /// The kernel's name stands, as systemd falls back to its default.
    Empty,
    /// A line that is not a valid host name ([`hostname_is_valid`]),
    /// whole and trimmed, for the boot message that says so.
    Invalid(&'a [u8]),
}

/// The name `/etc/hostname` holds; see [`EtcHostname`].
///
/// systemd also "cleans up" the line before it checks it -- drops the
/// characters a host name cannot hold, collapses repeated dots -- and sets
/// whatever is left. This does not: a line that is not a host name is
/// reported and the kernel's name kept, so that a typo in the file is seen
/// at boot rather than turned silently into some other name. That is the
/// rule `requests/b-d-init-should-set-the-host-name-from-etc-hostname.md`
/// asks for.
#[must_use]
pub fn read_etc_hostname(file: &[u8]) -> EtcHostname<'_> {
    for line in file.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        let name = line.strip_suffix(b".").unwrap_or(line);
        return if hostname_is_valid(name) {
            EtcHostname::Name(name)
        } else {
            EtcHostname::Invalid(line)
        };
    }
    EtcHostname::Empty
}

/// Whether `name` is a host name the way systemd's `hostname_is_valid()`
/// judges one (its trailing dot already dropped): at most [`HOST_NAME_MAX`]
/// bytes of ASCII letters, digits, hyphens and dots, in labels that are not
/// empty and neither begin nor end with a hyphen.
#[must_use]
pub fn hostname_is_valid(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > HOST_NAME_MAX {
        return false;
    }
    name.split(|&b| b == b'.').all(|label| {
        !label.is_empty()
            && label.first() != Some(&b'-')
            && label.last() != Some(&b'-')
            && label
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// `s` without the ASCII white space around it -- spaces, tabs, and the
/// carriage return of a file written on Windows.
fn trim(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i.saturating_add(1));
    s.get(start..end).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_is_the_name() {
        assert_eq!(
            read_etc_hostname(b"slate-test\n"),
            EtcHostname::Name(b"slate-test")
        );
        assert_eq!(
            read_etc_hostname(b"slate-test"),
            EtcHostname::Name(b"slate-test")
        );
    }

    #[test]
    fn blank_lines_and_comments_come_first() {
        let file = b"\n   \n# the machine's name\n\t# another\n  box.example.org  \nignored\n";
        assert_eq!(
            read_etc_hostname(file),
            EtcHostname::Name(b"box.example.org")
        );
    }

    #[test]
    fn a_file_with_no_name_is_empty() {
        assert_eq!(read_etc_hostname(b""), EtcHostname::Empty);
        assert_eq!(read_etc_hostname(b"\n\n  \n"), EtcHostname::Empty);
        assert_eq!(read_etc_hostname(b"# only a comment\n"), EtcHostname::Empty);
    }

    #[test]
    fn white_space_and_a_windows_line_end_are_trimmed() {
        assert_eq!(
            read_etc_hostname(b"  slate \r\n"),
            EtcHostname::Name(b"slate")
        );
    }

    #[test]
    fn one_trailing_dot_is_dropped() {
        assert_eq!(
            read_etc_hostname(b"box.example.org.\n"),
            EtcHostname::Name(b"box.example.org")
        );
        assert_eq!(
            read_etc_hostname(b"box..\n"),
            EtcHostname::Invalid(b"box..")
        );
    }

    #[test]
    fn what_is_not_a_host_name_is_reported_whole() {
        for bad in [
            &b"my_host"[..],
            b"two words",
            b"-leading",
            b"trailing-",
            b"a..b",
            b".lead",
            b"caf\xc3\xa9",
            b".",
        ] {
            let mut file = bad.to_vec();
            file.push(b'\n');
            assert_eq!(
                read_etc_hostname(&file),
                EtcHostname::Invalid(bad),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn sixty_four_bytes_is_the_limit() {
        let ok = [b'a'; HOST_NAME_MAX];
        assert!(hostname_is_valid(&ok));
        let too_long = [b'a'; HOST_NAME_MAX + 1];
        assert!(!hostname_is_valid(&too_long));
        // A trailing dot does not count against the limit: it is dropped first.
        let mut with_dot = ok.to_vec();
        with_dot.push(b'.');
        assert_eq!(read_etc_hostname(&with_dot), EtcHostname::Name(&ok));
    }

    #[test]
    fn digits_and_hyphens_inside_labels_are_fine() {
        assert!(hostname_is_valid(b"node-01.rack-7.example"));
        assert!(hostname_is_valid(b"3com"));
        assert!(!hostname_is_valid(b""));
    }
}
