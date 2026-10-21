//! `/etc/login.defs`, as util-linux's `lib/logindefs.c` reads it when built
//! without libeconf -- Ubuntu's build: `NAME value` (or `NAME=value`), a
//! `#` starting a comment anywhere, names matched in any case, the last
//! setting of a name winning. agetty asks it one thing,
//! `LOGIN_PLAIN_PROMPT`.

use std::path::Path;

use crate::is_space;

/// `BUFSIZ`: `fgets`'s buffer, which cuts a longer line into pieces read as
/// lines of their own.
const LINE_MAX: usize = 8192;

/// The settings a file made, in the order it made them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoginDefs {
    items: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

impl LoginDefs {
    /// `logindefs_load_file (filename)`: the file's settings, none when it
    /// does not open.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).map_or_else(|_| Self::default(), |data| Self::parse(&data))
    }

    /// The settings in a file's `data`.
    #[must_use]
    pub fn parse(data: &[u8]) -> Self {
        let mut items = Vec::new();
        let mut rest = data;
        while !rest.is_empty() {
            // `fgets`: through the new line, or as much as the buffer holds.
            let len = rest
                .iter()
                .take(LINE_MAX - 1)
                .position(|&b| b == b'\n')
                .map_or(rest.len().min(LINE_MAX - 1), |nl| nl.saturating_add(1));
            let (chunk, after) = rest.split_at(len);
            rest = after;
            if let Some(item) = Self::line(chunk) {
                items.push(item);
            }
        }
        Self { items }
    }

    /// One line's setting, as `logindefs_load_file` reads it.
    fn line(chunk: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
        // The buffer is a C string: a NUL in the line ends it.
        let buf = chunk.split(|&b| b == 0).next().unwrap_or_default();
        if matches!(buf.first(), Some(b'#' | b'\n')) {
            return None;
        }
        let buf = match buf.iter().position(|&b| b == b'#') {
            Some(hash) => buf.get(..hash).unwrap_or_default(),
            None => buf.strip_suffix(b"\n").unwrap_or(buf),
        };
        if buf.is_empty() {
            return None;
        }
        let start = buf.iter().position(|&b| !is_space(b)).unwrap_or(buf.len());
        let name_and_rest = buf.get(start..).unwrap_or_default();
        let name_len = name_and_rest
            .iter()
            .position(|&b| is_space(b) || b == b'=')
            .unwrap_or(name_and_rest.len());
        if name_len == 0 {
            return None;
        }
        let name = name_and_rest.get(..name_len).unwrap_or_default();
        // Past the name's end, which the NUL written over it took.
        let mut data = name_and_rest
            .get(name_len.saturating_add(1)..)
            .unwrap_or_default();
        while let Some((&b, more)) = data.split_first() {
            if is_space(b) || b == b'=' || b == b'"' {
                data = more;
            } else {
                break;
            }
        }
        // Blanks and quotes off the end -- never the first byte.
        while data.len() > 1 && data.last().is_some_and(|&b| is_space(b) || b == b'"') {
            data = data.get(..data.len().saturating_sub(1)).unwrap_or_default();
        }
        let value = (!data.is_empty()).then(|| data.to_vec());
        Some((name.to_vec(), value))
    }

    /// `getlogindefs_bool (name, dflt)`: whether the setting is `yes`, in
    /// any case -- `dflt` when the name is not set, or set to nothing.
    #[must_use]
    pub fn get_bool(&self, name: &[u8], dflt: bool) -> bool {
        self.items
            .iter()
            .rev()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.as_deref())
            .map_or(dflt, |v| v.eq_ignore_ascii_case(b"yes"))
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_is_a_name_and_a_value() {
        let defs = LoginDefs::parse(
            b"# a comment\n\nLOGIN_PLAIN_PROMPT yes\nMAIL_DIR=/var/mail # where\n  QUOTED = \"no\"  \n",
        );
        assert!(defs.get_bool(b"LOGIN_PLAIN_PROMPT", false));
        assert!(defs.get_bool(b"login_plain_prompt", false));
        assert!(!defs.get_bool(b"QUOTED", true));
        assert_eq!(
            defs.items[1],
            (b"MAIL_DIR".to_vec(), Some(b"/var/mail".to_vec()))
        );
        assert!(defs.get_bool(b"ABSENT", true));
    }

    #[test]
    fn the_last_setting_wins_and_an_empty_one_is_the_default() {
        let defs = LoginDefs::parse(b"X yes\nX no\nY\nZ yes\nZ \"\"\n");
        assert!(!defs.get_bool(b"X", true));
        assert!(defs.get_bool(b"Y", true));
        assert!(!defs.get_bool(b"Y", false));
        // Quotes are skipped before a value: `""` is none, and the default.
        assert!(defs.get_bool(b"Z", true));
        assert!(!defs.get_bool(b"Z", false));
    }

    #[test]
    fn odd_lines_are_read_as_upstream_reads_them() {
        // A name alone, with no new line at the end.
        assert_eq!(LoginDefs::line(b"NAME"), Some((b"NAME".to_vec(), None)));
        // Nothing but blanks, or a name made of nothing.
        assert_eq!(LoginDefs::line(b"   \n"), None);
        assert_eq!(LoginDefs::line(b"=yes\n"), None);
        // A NUL ends the line.
        assert_eq!(
            LoginDefs::line(b"A yes\0no\n"),
            Some((b"A".to_vec(), Some(b"yes".to_vec())))
        );
        // A line longer than the buffer is read as two.
        let mut long = vec![b'x'; LINE_MAX + 10];
        long.extend_from_slice(b" yes\n");
        let defs = LoginDefs::parse(&long);
        assert_eq!(defs.items.len(), 2);
    }

    #[test]
    fn a_file_that_does_not_open_says_nothing() {
        assert_eq!(
            LoginDefs::load(Path::new("/nonexistent/login.defs")),
            LoginDefs::default()
        );
    }
}
