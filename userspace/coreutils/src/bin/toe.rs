//! `toe`: ncurses 6.4's (`progs/toe.c`, 20240113), ported -- the table of
//! terminfo entries.
//!
//! Lists each terminal of a database directory -- the first one of the
//! search list, every one with `-a`, or those named -- by its primary name,
//! with its description; or, with `-s`, all of them merged and sorted, a
//! column per directory marking where each name is and whether its entries
//! look the same. `-u` and `-U` instead read a terminfo source and list what
//! each entry `use=`s, or what `use=`s it.
//!
//! As in the reference's build, only directory databases are read: an
//! operand that is not a directory (a hashed database, a termcap file) is
//! passed over in silence. And as upstream does, `toe` changes into each
//! subdirectory to read it, so a relative directory operand is found again
//! only from where the previous one left it.
//!
//! Deliberately different:
//!
//! - `-V` names SlateOS's coreutils rather than the ncurses version
//!   (design-decisions §370).
//! - Two entries of one directory under the same primary name (each in its
//!   own subdirectory) are listed by `-s` in the order they were found;
//!   upstream's `qsort` leaves their order to glibc's.

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program};
use coreutils::ncurses as nc;
use coreutils::quote::os_bytes;
use coreutils::stdfd;
use terminfo::compile::Diagnostics;
use terminfo::compile::comp_parse::Source;
use terminfo::compile::parse::Compiler;
use terminfo::compile::scan::cstr;
use terminfo::compile::tables::{first_name, name_match};
use terminfo::termtype::{Str, TermType};

coreutils::guard_std_fds!();

/// The parser. Its complaints name `argv[0]`, as glibc's `getopt` does, and
/// `usage` follows them.
const TOE: Program = Program::new("toe", 1);

/// Standard error, unbuffered, as upstream's is.
struct Stderr;

impl Diagnostics for Stderr {
    fn emit(&mut self, bytes: &[u8]) {
        ulclosestream::stderr_write(bytes);
    }
}

/// A file name's bytes as the `OsString` the platform opens.
#[cfg(unix)]
fn os(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(cstr(b).to_vec())
}

/// A file name's bytes, as near as a host without byte paths comes.
#[cfg(not(unix))]
fn os(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(cstr(b)).into_owned())
}

/// A directory entry's name as bytes.
#[cfg(unix)]
fn name_bytes(n: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    n.as_bytes().to_vec()
}

/// A directory entry's name as bytes, as near as the host comes.
#[cfg(not(unix))]
fn name_bytes(n: &std::ffi::OsStr) -> Vec<u8> {
    os_bytes(n).into_owned()
}

/// `_nc_is_dir_path (path)`.
fn is_dir_path(path: &[u8]) -> bool {
    std::fs::metadata(os(path)).is_ok_and(|m| m.is_dir())
}

/// `_nc_is_file_path (path)`.
fn is_file_path(path: &[u8]) -> bool {
    std::fs::metadata(os(path)).is_ok_and(|m| m.is_file())
}

/// `quick_prefix (path)`: a search-list element that is an entry spelled
/// out rather than a directory.
fn quick_prefix(path: &[u8]) -> bool {
    path.starts_with(b"hex:") || path.starts_with(b"b64:")
}

/// `term_description (tp)`: what follows the name field's last `|`.
fn term_description(tp: &TermType) -> Vec<u8> {
    let names = cstr(&tp.term_names);
    match names.iter().rposition(|&c| c == b'|') {
        Some(bar) if bar.saturating_add(1) < names.len() => names
            .get(bar.saturating_add(1)..)
            .unwrap_or_default()
            .to_vec(),
        _ => b"(No description)".to_vec(),
    }
}

/// `string_sum (value)`: a string's bytes added up; a cancelled one is all
/// ones, an absent one nothing.
fn string_sum(value: &Str) -> u64 {
    match value {
        Str::Cancelled => !0,
        Str::Absent => 0,
        Str::Value(v) => cstr(v)
            .iter()
            .fold(0u64, |sum, &c| sum.wrapping_add(u64::from(c))),
    }
}

/// `checksum_of (tp)`: every value of the entry added up, as an `unsigned
/// long` -- the signed ones sign-extended, as C converts them.
fn checksum_of(tp: &TermType) -> u64 {
    let names = Str::Value(tp.term_names.clone());
    let mut result = string_sum(&names);
    for &b in &tp.booleans {
        result = result.wrapping_add(u64::from_le_bytes(i64::from(b).to_le_bytes()));
    }
    for &n in &tp.numbers {
        result = result.wrapping_add(u64::from_le_bytes(i64::from(n).to_le_bytes()));
    }
    for s in &tp.strings {
        result = result.wrapping_add(string_sum(s));
    }
    result
}

/// One terminal of `-s`'s table: `TERMDATA`.
struct TermData {
    db_index: usize,
    checksum: u64,
    term_name: Vec<u8>,
    description: Vec<u8>,
}

/// `printf ("%-10s\t%s\n", name, description)`.
fn line(out: &mut Vec<u8>, name: &[u8], description: &[u8]) {
    out.extend_from_slice(name);
    for _ in name.len()..10 {
        out.push(b' ');
    }
    out.push(b'\t');
    out.extend_from_slice(description);
    out.push(b'\n');
}

/// Where `typelist` sends each terminal: `deschook`, which prints it, or
/// `sorthook`, which keeps it for `show_termdata`.
enum Hook {
    Desc,
    Sort(Vec<TermData>),
}

impl Hook {
    fn call(
        &mut self,
        out: &mut ulclosestream::Stdout,
        db_index: usize,
        db_limit: usize,
        term_name: &[u8],
        tp: &TermType,
    ) {
        match self {
            Self::Desc => {
                let mut l = Vec::new();
                line(&mut l, term_name, &term_description(tp));
                out.write(&l);
            }
            Self::Sort(data) => data.push(TermData {
                db_index,
                checksum: if db_limit > 1 { checksum_of(tp) } else { 0 },
                term_name: term_name.to_vec(),
                description: term_description(tp),
            }),
        }
    }
}

/// `show_termdata (eargc, eargv)`: the collected terminals sorted and
/// printed -- with more than one directory, a column each, `*` where a
/// name's entry first appears or differs from the one before it and `+`
/// where it looks the same.
fn show_termdata(out: &mut ulclosestream::Stdout, eargv: &[Vec<u8>], mut data: Vec<TermData>) {
    if data.is_empty() {
        return;
    }
    let eargc = eargv.len();
    let mut text = Vec::new();
    if eargc > 1 {
        for (j, dir) in eargv.iter().enumerate() {
            for _ in 0..=j {
                text.extend_from_slice(b"--");
            }
            text.extend_from_slice(b"> ");
            text.extend_from_slice(dir);
            text.push(b'\n');
        }
    }
    data.sort_by(|p, q| {
        p.term_name
            .cmp(&q.term_name)
            .then(p.db_index.cmp(&q.db_index))
    });
    let mut n = 0usize;
    while let Some(first) = data.get(n) {
        let mut nk: Option<usize> = None;
        if eargc > 1 {
            let mut check: u64 = 0;
            let mut k = 0usize;
            let mut cur = first;
            loop {
                let mark = if check == 0 || check != cur.checksum {
                    b'*'
                } else {
                    b'+'
                };
                while k < cur.db_index {
                    text.extend_from_slice(b"--");
                    k = k.saturating_add(1);
                }
                text.push(mark);
                text.push(b'-');
                check = cur.checksum;
                if mark == b'*' && nk.is_none() {
                    nk = Some(n);
                }
                k = k.saturating_add(1);
                match data.get(n.saturating_add(1)) {
                    Some(next) if next.term_name == cur.term_name => {
                        n = n.saturating_add(1);
                        cur = next;
                    }
                    _ => break,
                }
            }
            while k < eargc {
                text.extend_from_slice(b"--");
                k = k.saturating_add(1);
            }
            text.extend_from_slice(b":\t");
        }
        let nk = nk.unwrap_or(n);
        if let (Some(this), Some(described)) = (data.get(n), data.get(nk)) {
            line(&mut text, &this.term_name, &described.description);
        }
        n = n.saturating_add(1);
    }
    out.write(&text);
}

/// `fflush (stdout)`, then a message on standard error.
fn complain(out: &mut ulclosestream::Stdout, m: &[u8]) {
    out.flush();
    ulclosestream::stderr_write(m);
}

/// `typelist (eargc, eargv, verbosity, hook)`: `hook` applied to each entry
/// of each directory, by its primary name.
fn typelist(
    out: &mut ulclosestream::Stdout,
    progname: &[u8],
    eargv: &[Vec<u8>],
    verbosity: bool,
    mut hook: Hook,
) -> u8 {
    let eargc = eargv.len();
    for (i, dir) in eargv.iter().enumerate() {
        // No hashed database and no termcap file in this build: a path that
        // is not a directory is nothing.
        if !is_dir_path(dir) {
            continue;
        }
        let termdir = match std::fs::read_dir(os(dir)) {
            Ok(d) => d,
            Err(_) => {
                let mut m = progname.to_vec();
                m.extend_from_slice(b": can't open terminfo directory ");
                m.extend_from_slice(dir);
                m.push(b'\n');
                complain(out, &m);
                continue;
            }
        };
        if verbosity {
            let mut m = b"#\n#".to_vec();
            m.extend_from_slice(dir);
            m.extend_from_slice(b":\n#\n");
            out.write(&m);
        }
        // `readdir` ending -- at the end, or on an error -- ends the scan.
        for subdir in termdir.map_while(Result::ok) {
            let name_1 = name_bytes(&subdir.file_name());
            let mut cwd_buf = dir.clone();
            cwd_buf.push(b'/');
            cwd_buf.extend_from_slice(&name_1);
            cwd_buf.push(b'/');
            if std::env::set_current_dir(os(&cwd_buf)).is_err() {
                continue;
            }
            let entrydir = match std::fs::read_dir(".") {
                Ok(d) => d,
                Err(e) => {
                    let mut m = cwd_buf.clone();
                    m.extend_from_slice(b": ");
                    m.extend_from_slice(coreutils::errmsg::strerror(&e).as_bytes());
                    m.push(b'\n');
                    complain(out, &m);
                    continue;
                }
            };
            for entry in entrydir.map_while(Result::ok) {
                let name_2 = name_bytes(&entry.file_name());
                if !is_file_path(&name_2) {
                    continue;
                }
                let Some(lterm) = terminfo::read_file_entry(&name_2, true) else {
                    let mut m = progname.to_vec();
                    m.extend_from_slice(b": couldn't open terminfo file ");
                    m.extend_from_slice(&name_2);
                    m.extend_from_slice(b".\n");
                    complain(out, &m);
                    continue;
                };
                // "only visit things once, by primary name"
                let cn = first_name(&lterm.term_names);
                if cn == name_2 {
                    hook.call(out, i, eargc, &cn, &lterm);
                }
            }
        }
    }
    if let Hook::Sort(data) = hook {
        show_termdata(out, eargv, data);
    }
    0
}

/// `usage ()`.
fn usage(progname: &[u8]) -> u8 {
    let mut m = b"usage: ".to_vec();
    m.extend_from_slice(progname);
    m.extend_from_slice(b" [-ahsuUV] [-v n] [file...]\n");
    ulclosestream::stderr_write(&m);
    1
}

/// Upstream's `main`, up to its `ExitProgram`.
fn run(argv: &[OsString], out: &mut ulclosestream::Stdout) -> u8 {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let progname = nc::rootname(&argv0).to_vec();
    let mut all_dirs = false;
    let mut direct_dependencies = false;
    let mut invert_dependencies = false;
    let mut header = false;
    let mut report_file: Option<Vec<u8>> = None;
    let mut sort = false;
    let mut operands: Vec<Vec<u8>> = Vec::new();

    let words = argv.get(1..).unwrap_or_default();
    for item in TOE
        .parse(words, "0123456789ahsu:vU:V", &[])
        .short_only(true)
    {
        let opt = match item {
            Err(e) => {
                nc::getopt_complaint(&argv0, &e.sentence);
                return usage(&progname);
            }
            Ok(o) => o,
        };
        let (c, value) = match opt {
            Opt::Short(c, value) => (c, value),
            Opt::Operand(o) => {
                operands.push(os_bytes(o).into_owned());
                continue;
            }
            Opt::Long(..) => return usage(&progname),
        };
        let arg = || value.as_ref().map(|v| os_bytes(v).into_owned());
        match c {
            // `-v`'s level, and digits alone: the trace level, which this
            // build has no trace to show.
            b'0'..=b'9' | b'v' => {}
            b'a' => all_dirs = true,
            b'h' => header = true,
            b's' => sort = true,
            b'u' => {
                direct_dependencies = true;
                report_file = arg();
            }
            b'U' => {
                invert_dependencies = true;
                report_file = arg();
            }
            b'V' => {
                out.write(&nc::version_line(&progname));
                return 0;
            }
            _ => return usage(&progname),
        }
    }

    if let Some(report_file) = report_file {
        let file = match std::fs::File::open(os(&report_file)) {
            Ok(f) => f,
            Err(_) => {
                let mut m = progname.clone();
                m.extend_from_slice(b": can't open ");
                m.extend_from_slice(&report_file);
                m.push(b'\n');
                complain(out, &m);
                return 1;
            }
        };
        let mut diag = Stderr;
        let mut c = Compiler::new(&mut diag);
        c.scan.set_source(Some(&report_file));
        // "parse entries out of the source file"
        if c.read_entry_source(Source::File(Box::new(file), Some(0)), false, false)
            .is_err()
        {
            return 1;
        }
        let mut text = Vec::new();
        if direct_dependencies {
            // "maybe we want a direct-dependency listing?"
            for qp in c.entries.iter().filter(|qp| qp.nuses != 0) {
                text.extend_from_slice(&first_name(&qp.tterm.term_names));
                text.push(b':');
                for u in qp.uses.iter().take(qp.nuses) {
                    text.push(b' ');
                    text.extend_from_slice(u.name.as_deref().map_or(b"(null)".as_slice(), cstr));
                }
                text.push(b'\n');
            }
        } else if invert_dependencies {
            // "maybe we want a reverse-dependency listing?"
            for qp in &c.entries {
                let mut matchcount = 0usize;
                for rp in c.entries.iter().filter(|rp| rp.nuses != 0) {
                    for u in rp.uses.iter().take(rp.nuses) {
                        if name_match(
                            &qp.tterm.term_names,
                            u.name.as_deref().unwrap_or_default(),
                            b"|",
                        ) {
                            if matchcount == 0 {
                                text.extend_from_slice(&first_name(&qp.tterm.term_names));
                                text.push(b':');
                            }
                            matchcount = matchcount.saturating_add(1);
                            text.push(b' ');
                            text.extend_from_slice(&first_name(&rp.tterm.term_names));
                        }
                    }
                }
                if matchcount != 0 {
                    text.push(b'\n');
                }
            }
        }
        out.write(&text);
        return 0;
    }

    // "If we get this far, user wants a simple terminal type listing."
    let hook = if sort {
        Hook::Sort(Vec::new())
    } else {
        Hook::Desc
    };
    if !operands.is_empty() {
        typelist(out, &progname, &operands, header, hook)
    } else {
        let env = terminfo::Env::from_process();
        let list = terminfo::search_list(&env);
        let eargv: Vec<Vec<u8>> = if all_dirs {
            list.into_iter().filter(|p| !quick_prefix(p)).collect()
        } else {
            list.into_iter()
                .take(1)
                .filter(|p| !quick_prefix(p))
                .collect()
        };
        typelist(out, &progname, &eargv, header, hook)
    }
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = ulclosestream::Stdout::new(1);
    let status = run(&argv, &mut out);
    out.flush_at_exit();
    ExitCode::from(status)
}
