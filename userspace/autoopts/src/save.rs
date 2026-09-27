//! `--save-opts[=FILE]` (`save.c`): the program's own options, as they stand
//! after all three passes, written as an rc file that `--load-opts` or
//! `$HOME/.sharrc` would read back.
//!
//! Where the file goes: the option's argument if it has one, else the last
//! home-list entry (`$HOME`); a directory there gets the rc file's name
//! appended. Whatever is there already is unlinked first. Any failure is a
//! warning and the program still exits 0.

use std::fs;
use std::io::{self, Write};

use crate::{Options, bytes_os, getenv, st};

/// `zsave_warn`'s first half. Upstream passes one argument to a format with
/// two `%s` in most calls; the second prints whatever a register held --
/// nothing when `HOME` is unset, a stray byte elsewhere. It is left empty
/// here (crate docs, "Where this port departs").
fn save_warn(prog_name: &[u8], what: &[u8]) {
    let mut msg = prog_name.to_vec();
    msg.extend_from_slice(b" warning:  cannot save options - ");
    msg.extend_from_slice(what);
    msg.extend_from_slice(b" not regular file\n");
    ulclosestream::stderr_write(&msg);
}

/// "error ERRNO (REASON) VERB NAME".
fn errno_line(verb: &str, err: &io::Error, name: &[u8]) {
    let mut msg = format!(
        "error {} ({}) {verb} ",
        err.raw_os_error().unwrap_or(0),
        errmsg::strerror(err)
    )
    .into_bytes();
    msg.extend_from_slice(name);
    msg.push(b'\n');
    ulclosestream::stderr_write(&msg);
}

/// `S_IFREG` / `S_IFDIR` as far as `find_file_name` cares.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A regular file, or one about to be created.
    Regular,
    /// A directory.
    Dir,
    /// Anything else.
    Other,
}

fn kind_of(m: &fs::Metadata) -> Kind {
    if m.is_dir() {
        Kind::Dir
    } else if m.is_file() {
        Kind::Regular
    } else {
        Kind::Other
    }
}

impl Options {
    /// `find_dir_name`: the option's argument, or the last home-list entry
    /// with its `$VAR` expanded.
    fn find_dir_name(&self) -> Option<Vec<u8>> {
        if let Some(arg) = self.arg(self.prog.save_opts)
            && !arg.is_empty()
        {
            return Some(arg.to_vec());
        }
        let dir = *self.prog.home_list.last()?;
        let dir = dir.as_bytes();
        if dir.first() != Some(&b'$') {
            return Some(dir.to_vec());
        }
        let var = dir.get(1..).unwrap_or_default();
        match var.iter().position(|&b| b == b'/') {
            Some(slash) => {
                // `AO_NAME_LIMIT`: a longer variable name is not looked up.
                if slash > 127 {
                    return None;
                }
                let name = var.get(..slash).unwrap_or_default();
                let Some(value) = getenv(name) else {
                    save_warn(&self.prog_name, b"");
                    let mut msg = b"'".to_vec();
                    msg.extend_from_slice(var);
                    msg.extend_from_slice(b"' not defined\n");
                    ulclosestream::stderr_write(&msg);
                    return None;
                };
                let mut out = value;
                out.push(b'/');
                out.extend_from_slice(var.get(slash..).unwrap_or_default());
                Some(out)
            }
            None => {
                let Some(value) = getenv(var) else {
                    save_warn(&self.prog_name, b"");
                    let mut msg = b"'".to_vec();
                    msg.extend_from_slice(var);
                    msg.extend_from_slice(b"' not defined\n");
                    ulclosestream::stderr_write(&msg);
                    return None;
                };
                Some(value)
            }
        }
    }

    /// `find_file_name`: where the file will be written, with any old one
    /// removed.
    fn find_file_name(&self) -> Option<Vec<u8>> {
        let mut path = self.find_dir_name()?;
        let bogus = |path: &[u8], e: &io::Error| {
            save_warn(&self.prog_name, b"");
            errno_line("stat-ing", e, path);
        };
        let mut kind = match fs::metadata(bytes_os(&path)) {
            Ok(m) => kind_of(&m),
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    bogus(&path, &e);
                    return None;
                }
                match path.iter().rposition(|&b| b == b'/') {
                    None => Kind::Regular,
                    Some(slash) => {
                        if slash >= 4096 {
                            bogus(&path, &e);
                            return None;
                        }
                        let parent = path.get(..slash).unwrap_or_default();
                        match fs::metadata(bytes_os(parent)) {
                            Ok(m) if m.is_dir() => Kind::Regular,
                            // Not a directory: errno is still the first
                            // stat's.
                            Ok(_) => {
                                bogus(&path, &e);
                                return None;
                            }
                            Err(e2) => {
                                bogus(&path, &e2);
                                return None;
                            }
                        }
                    }
                }
            }
        };
        if kind == Kind::Dir {
            path.push(b'/');
            path.extend_from_slice(self.prog.rc_name.as_bytes());
            kind = match fs::metadata(bytes_os(&path)) {
                Ok(m) => kind_of(&m),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Kind::Regular,
                Err(e) => {
                    save_warn(&self.prog_name, b"");
                    errno_line("stat-ing", &e, &path);
                    return None;
                }
            };
        }
        if kind != Kind::Regular {
            save_warn(&self.prog_name, &path);
            return None;
        }
        // "Get rid of the old file"; a failure shows up at the open.
        let _ = fs::remove_file(bytes_os(&path));
        Some(path)
    }

    /// `optionSaveFile`.
    pub(crate) fn save_file(&self) {
        let Some(path) = self.find_file_name() else {
            return;
        };
        let mut file = match fs::File::create(bytes_os(&path)) {
            Ok(f) => f,
            Err(e) => {
                save_warn(&self.prog_name, b"");
                errno_line("creating", &e, &path);
                return;
            }
        };
        let mut text = b"#  ".to_vec();
        let title = self.prog.usage_title.as_bytes();
        if let Some(nl) = title.iter().position(|&b| b == b'\n') {
            text.extend_from_slice(title.get(..=nl).unwrap_or_default());
        }
        text.extend_from_slice(b"#  preset/initialization file\n#  ");
        text.extend_from_slice(&ctime_now());
        text.extend_from_slice(b"#\n");
        for (i, d) in self.prog.descs.iter().enumerate().take(self.prog.preset_ct) {
            let Some(state) = self.state.get(i) else {
                continue;
            };
            if state.flags & st::SET_MASK == 0 || state.flags & st::DO_NOT_SAVE_MASK != 0 {
                continue;
            }
            let disabled = state.flags & st::DISABLED != 0;
            if state.flags & st::ARG_TYPE_MASK == 0 {
                let name = if disabled {
                    d.disable_name.unwrap_or(d.name)
                } else {
                    d.name
                };
                text.extend_from_slice(name.as_bytes());
                text.push(b'\n');
            } else {
                let name = if disabled {
                    d.disable_name.unwrap_or(d.name)
                } else {
                    d.name
                };
                prt_entry(&mut text, name, state.arg.as_deref());
            }
        }
        // Upstream checks nothing here: a full disk loses the file silently.
        let _ = file.write_all(&text);
    }
}

/// `prt_entry`: `name = value`, the value's newlines as backslash
/// continuations, the `=` aligned at column 18.
fn prt_entry(text: &mut Vec<u8>, name: &str, arg: Option<&[u8]>) {
    text.extend_from_slice(name.as_bytes());
    let Some(arg) = arg else {
        text.push(b'\n');
        return;
    };
    text.extend_from_slice(b" = ");
    for _ in name.len()..17 {
        text.push(b' ');
    }
    let mut parts = arg.split(|&b| b == b'\n').peekable();
    while let Some(part) = parts.next() {
        text.extend_from_slice(part);
        if parts.peek().is_some() {
            text.extend_from_slice(b"\\\n");
        }
    }
    text.push(b'\n');
}

/// `ctime(time(NULL))`: "Sun Sep 27 01:09:57 2026\n", local time.
fn ctime_now() -> Vec<u8> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let zone = localtime::Zone::from_env();
    let tm = zone.localtime(now, 0);
    localtime::strftime(b"%a %b %e %H:%M:%S %Y\n", &tm)
}
