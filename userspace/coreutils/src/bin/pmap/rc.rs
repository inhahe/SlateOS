//! `pmap`'s rc file: `config_read`, `config_create` and the default name,
//! `~/.pmaprc` -- `~/.NAMErc` for a `pmap` run by another name, since the
//! name is `program_invocation_short_name`'s.
//!
//! The file says which columns `-c` shows (`[Fields Display]`) and whether
//! the mapping is shown with its path (`[Mapping]`, `ShowPath`). Upstream's
//! reading of it is kept as it is, oddities included: a line of 1024 bytes
//! or more is cut there with a warning; text after a section's name is
//! warned about and then read as an entry of that section; a second word
//! that is not a comment is warned about and the first one still taken; a
//! field name is cut to 31 bytes; and every `ShowPath` *toggles* the path,
//! so two of them turn it off again.

use std::path::Path;

use super::cfile::CFile;

/// `MAX_CNF_LINE_LEN`: a line is read with `fgets` into one byte more.
const MAX_CNF_LINE_LEN: usize = 1024;

/// `cnf_listnode`'s `description`: 32 bytes, one of them the NUL.
const DESCRIPTION_MAX: usize = 31;

/// Which section the lines being read belong to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Unsupported,
    FieldsDisplay,
    Mapping,
}

/// What `config_read` changes: the fields `-c` shows, and the path switch.
pub struct Settings<'a> {
    /// `cnf_listhead`: newest first, as the list is built by prepending.
    pub fields: &'a mut Vec<Vec<u8>>,
    /// `map_desc_showpath`.
    pub showpath: &'a mut bool,
}

/// `strlen`: a line as C sees it ends at its first NUL.
fn c_len(b: &[u8]) -> usize {
    b.iter().position(|&c| c == 0).unwrap_or(b.len())
}

/// `strtok (s, " \t")` and the call after it: the first two words.
fn first_two_words(s: &[u8]) -> (Option<&[u8]>, Option<&[u8]>) {
    let mut words = s
        .split(|&c| c == b' ' || c == b'\t')
        .filter(|w| !w.is_empty());
    (words.next(), words.next())
}

/// `config_read (rc_filename)`: false when the file cannot be opened.
/// `warn` is `xwarnx`, which says each complaint as it is met.
pub fn read(path: &Path, set: &mut Settings<'_>, warn: &mut dyn FnMut(String)) -> bool {
    let Ok(mut f) = CFile::open(path) else {
        return false;
    };
    let mut line_cnt: i64 = 0;
    let mut section = Section::None;
    while let Some(raw) = f.fgets(MAX_CNF_LINE_LEN.saturating_add(1)) {
        line_cnt = line_cnt.saturating_add(1);
        let length = c_len(&raw);
        let mut line = raw.get(..length).unwrap_or_default();
        if let Some(body) = line.strip_suffix(b"\n") {
            line = body;
        } else if length == MAX_CNF_LINE_LEN {
            warn(format!("config line too long - line {line_cnt}"));
            // The rest of the line, whatever its length, goes unread.
            while let Some(tail) = f.fgets(MAX_CNF_LINE_LEN.saturating_add(1)) {
                let n = c_len(&tail);
                if n == 0 || tail.get(n.saturating_sub(1)) == Some(&b'\n') {
                    break;
                }
            }
        }

        let skip = line
            .iter()
            .take_while(|&&c| c == b' ' || c == b'\t')
            .count();
        let mut trimmed = line.get(skip..).unwrap_or_default();
        if matches!(trimmed.first(), None | Some(b'#')) {
            continue;
        }

        if trimmed.first() == Some(&b'[') {
            if let Some(rest) = trimmed.strip_prefix(b"[Fields Display]") {
                trimmed = rest;
                section = Section::FieldsDisplay;
            } else if let Some(rest) = trimmed.strip_prefix(b"[Mapping]") {
                trimmed = rest;
                section = Section::Mapping;
            } else {
                match trimmed.iter().position(|&c| c == b']') {
                    Some(close) => {
                        section = Section::Unsupported;
                        warn(format!(
                            "unsupported section found in the config - line {line_cnt}"
                        ));
                        trimmed = trimmed.get(close.saturating_add(1)..).unwrap_or_default();
                    }
                    None => {
                        warn(format!(
                            "syntax error found in the config - line {line_cnt}"
                        ));
                        trimmed = &[];
                    }
                }
            }
            let skip = trimmed
                .iter()
                .take_while(|&&c| c == b' ' || c == b'\t')
                .count();
            trimmed = trimmed.get(skip..).unwrap_or_default();
            if matches!(trimmed.first(), None | Some(b'#')) {
                continue;
            }
            // Something after the section's name: said, and then read as a
            // line of the section.
            warn(format!(
                "syntax error found in the config - line {line_cnt}"
            ));
        }

        match section {
            Section::FieldsDisplay => {
                let (token, tail) = first_two_words(trimmed);
                if let Some(token) = token {
                    if tail.is_some_and(|t| t.first() != Some(&b'#')) {
                        warn(format!(
                            "syntax error found in the config - line {line_cnt}"
                        ));
                    }
                    let kept = token
                        .get(..token.len().min(DESCRIPTION_MAX))
                        .unwrap_or_default();
                    set.fields.insert(0, kept.to_vec());
                }
            }
            Section::Mapping => {
                let (token, tail) = first_two_words(trimmed);
                if let Some(token) = token {
                    if tail.is_some_and(|t| t.first() != Some(&b'#')) {
                        warn(format!(
                            "syntax error found in the config - line {line_cnt}"
                        ));
                    }
                    if token == b"ShowPath" {
                        *set.showpath = !*set.showpath;
                    }
                }
            }
            Section::Unsupported => {}
            Section::None => {
                warn(format!(
                    "syntax error found in the config - line {line_cnt}"
                ));
            }
        }
    }
    true
}

/// The rc file `config_create` writes, with the column names in it.
const TEMPLATE: &str = concat!(
    "# pmap's Config File\n",
    "\n",
    "# All the entries are case sensitive.\n",
    "# Unsupported entries are ignored!\n",
    "\n",
    "[Fields Display]\n",
    "\n",
    "# To enable a field uncomment its entry\n",
    "\n",
    "#Perm\n",
    "#Offset\n",
    "#Device\n",
    "#Inode\n",
    "#Size\n",
    "#Rss\n",
    "#Pss\n",
    "#Shared_Clean\n",
    "#Shared_Dirty\n",
    "#Private_Clean\n",
    "#Private_Dirty\n",
    "#Referenced\n",
    "#Anonymous\n",
    "#AnonHugePages\n",
    "#Swap\n",
    "#KernelPageSize\n",
    "#MMUPageSize\n",
    "#Locked\n",
    "#VmFlags\n",
    "#Mapping\n",
    "\n",
    "\n",
    "[Mapping]\n",
    "\n",
    "# to show paths in the mapping column uncomment the following line\n",
    "#ShowPath\n",
    "\n",
);

/// `config_create (rc_filename)`: the template written to a file that is
/// not there yet. False when the file is there -- said through `warn` -- or
/// cannot be made; a write that fails after that is not looked at, as
/// upstream's `fprintf`s and `fclose` are not.
pub fn create(path: &Path, warn: &mut dyn FnMut(String)) -> bool {
    if std::fs::File::open(path).is_ok() {
        warn("the file already exists - delete or rename it first".to_owned());
        return false;
    }
    let Ok(mut f) = std::fs::File::create(path) else {
        return false;
    };
    // Not looked at upstream: see above.
    let _ = std::io::Write::write_all(&mut f, TEMPLATE.as_bytes());
    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{Settings, create, read};

    fn run(text: &[u8]) -> (Vec<Vec<u8>>, bool, Vec<String>) {
        let p = std::env::temp_dir().join(format!("pmap-rc-{}-{}", std::process::id(), text.len()));
        std::fs::write(&p, text).unwrap();
        let mut fields = Vec::new();
        let mut showpath = false;
        let mut warnings = Vec::new();
        let mut set = Settings {
            fields: &mut fields,
            showpath: &mut showpath,
        };
        assert!(read(&p, &mut set, &mut |w| warnings.push(w)));
        std::fs::remove_file(&p).unwrap();
        (fields, showpath, warnings)
    }

    #[test]
    fn fields_are_listed_newest_first_and_showpath_toggles() {
        let (fields, showpath, warnings) =
            run(b"# c\n[Fields Display]\n  Rss\nPss #x\n[Mapping]\nShowPath\nShowPath\nShowPath\n");
        assert_eq!(fields, vec![b"Pss".to_vec(), b"Rss".to_vec()]);
        assert!(showpath, "three toggles");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn what_upstream_warns_about() {
        let (fields, _, warnings) = run(b"Rss\n[Other]\nx\n[Broken\n[Fields Display] Size\nA B\n");
        assert_eq!(
            warnings,
            vec![
                "syntax error found in the config - line 1",
                "unsupported section found in the config - line 2",
                "syntax error found in the config - line 4",
                "syntax error found in the config - line 5",
                "syntax error found in the config - line 6",
            ]
        );
        // `Size` after the section's name, and `A` despite its tail.
        assert_eq!(fields, vec![b"A".to_vec(), b"Size".to_vec()]);
    }

    #[test]
    fn a_long_line_is_cut_and_its_tail_dropped() {
        let mut text = b"[Fields Display]\n".to_vec();
        text.extend(std::iter::repeat_n(b'x', 1500));
        text.extend_from_slice(b"\nRss\n");
        let (fields, _, warnings) = run(&text);
        assert_eq!(warnings, vec!["config line too long - line 2"]);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields.first().unwrap(), b"Rss");
        assert_eq!(fields.get(1).unwrap().len(), 31, "cut to the field's size");
    }

    #[test]
    fn create_refuses_a_file_that_is_there() {
        let p = std::env::temp_dir().join(format!("pmap-rc-create-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let mut warnings = Vec::new();
        assert!(create(&p, &mut |w| warnings.push(w)));
        assert!(warnings.is_empty());
        let text = std::fs::read(&p).unwrap();
        assert!(text.starts_with(b"# pmap's Config File\n"));
        assert!(!create(&p, &mut |w| warnings.push(w)));
        assert_eq!(
            warnings,
            vec!["the file already exists - delete or rename it first"]
        );
        std::fs::remove_file(&p).unwrap();
    }
}
