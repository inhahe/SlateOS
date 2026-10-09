//! `tic`: ncurses 6.4's (`progs/tic.c`, 20240113), ported -- with its
//! aliases `captoinfo` (`-I`) and `infotocap` (`-C`).
//!
//! Compiles terminfo (or termcap) source into the terminfo database: each
//! entry's `use=` resolved against the source and then the database, the
//! result written under its first name with its aliases linked to it. Or,
//! with `-I`, `-L` or `-C`, translates the source back out -- terminfo,
//! terminfo with long names, or termcap -- its comments carried over. `-c`
//! only checks; `-v` adds `tic`'s own checks of each entry (`check.rs`).
//!
//! The compiler is `terminfo::compile`'s; the writing is `writer.rs`.
//!
//! Deliberately different:
//!
//! - `-V` names SlateOS's coreutils rather than the ncurses version
//!   (design-decisions §370).
//! - Standard input, and a named source that is not a regular file, are read
//!   into memory rather than copied to a file in `/tmp` first -- upstream
//!   copies so that it can seek back for the comments it prints with `-I`
//!   and `-C`. The result is the same, without depending on `/tmp`.
//! - Where upstream never finishes or crashes, this does not: a `use=`
//!   cycle, which upstream resolves forever, is reported as the `use=` that
//!   could not be resolved, and fails (`terminfo::compile::comp_parse`); and
//!   the four places `-v`'s checks crash or loop are listed in `check.rs`.

mod check;
mod writer;

use std::ffi::OsString;
use std::io::Read;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program};
use coreutils::ncurses as nc;
use coreutils::quote::os_bytes;
use coreutils::stdfd;
use terminfo::compile::comp_parse::Source;
use terminfo::compile::dump::{
    Dumper, F_LITERAL, F_TERMCAP, F_TERMINFO, F_VARIABLE, S_TERMCAP, S_TERMINFO, S_VARIABLE,
    nametrans, repair_acsc,
};
use terminfo::compile::parse::Compiler;
use terminfo::compile::scan::cstr;
use terminfo::compile::tables::{first_name, name_match};
use terminfo::compile::{Abort, Diagnostics};
use terminfo::termtype::Str;

use writer::Writer;

coreutils::guard_std_fds!();

/// The parser. Its complaints name `argv[0]`, as glibc's `getopt` does, and
/// `usage` follows them.
const TIC: Program = Program::new("tic", 1);

/// `STDIN_NAME`.
const STDIN_NAME: &[u8] = b"<stdin>";
/// `MAX_TERMINFO_LENGTH`.
const MAX_TERMINFO_LENGTH: i32 = 4096;
/// `MAX_TERMCAP_LENGTH`.
const MAX_TERMCAP_LENGTH: i32 = 1023;
/// `BUFSIZ`: `-e` reads its file's lines in pieces of one less than this.
const BUFSIZ: usize = 8192;

/// `usage_string`.
const USAGE_STRING: &[u8] =
    b"[-e names] [-o dir] [-R name] [-v[n]] [-V] [-w[n]] [-1aCDcfGgIKLNrsTtUx] source-file\n";

/// `usage`'s options.
const OPTIONS_STRING: &[u8] = b"Options:\n\
\x20 -0         format translation output all capabilities on one line\n\
\x20 -1         format translation output one capability per line\n\
\x20 -a         retain commented-out capabilities (sets -x also)\n\
\x20 -C         translate entries to termcap source form\n\
\x20 -D         print list of tic's database locations (first must be writable)\n\
\x20 -c         check only, validate input without compiling or translating\n\
\x20 -e<names>  translate/compile only entries named by comma-separated list\n\
\x20 -f         format complex strings for readability\n\
\x20 -G         format %{number} to %'char'\n\
\x20 -g         format %'char' to %{number}\n\
\x20 -I         translate entries to terminfo source form\n\
\x20 -K         translate entries to termcap source form with BSD syntax\n\
\x20 -L         translate entries to full terminfo source form\n\
\x20 -N         disable smart defaults for source translation\n\
\x20 -o<dir>    set output directory for compiled entry writes\n\
\x20 -Q[n]      dump compiled description\n\
\x20 -q    brief listing, removes headers\n\
\x20 -R<name>   restrict translation to given terminfo/termcap version\n\
\x20 -r         force resolution of all use entries in source translation\n\
\x20 -s         print summary statistics\n\
\x20 -T         remove size-restrictions on compiled description\n\
\x20 -t         suppress commented-out capabilities\n\
\x20 -U         suppress post-processing of entries\n\
\x20 -V         print version\n\
\x20 -W         wrap long strings according to -w[n] option\n\
\x20 -v[n]      set verbosity level\n\
\x20 -w[n]      set format width for translation output\n\
\x20 -x         treat unknown capabilities as user-defined\n\
\n\
Parameters:\n\
\x20 <file>     file to translate or compile\n";

/// Standard error, unbuffered, as upstream's is.
struct Stderr;

impl Diagnostics for Stderr {
    fn emit(&mut self, bytes: &[u8]) {
        ulclosestream::stderr_write(bytes);
    }
}

/// What ends the program early, its message written: the status.
struct Exit(u8);

impl From<Abort> for Exit {
    fn from(_: Abort) -> Self {
        Exit(1)
    }
}

/// `usage ()`.
fn usage(progname: &[u8]) -> Exit {
    // `"Usage: %s %s\n"`: `usage_string` ends a line already, so this
    // leaves an empty one after it.
    let mut m = b"Usage: ".to_vec();
    m.extend_from_slice(progname);
    m.push(b' ');
    m.extend_from_slice(USAGE_STRING);
    m.push(b'\n');
    m.extend_from_slice(OPTIONS_STRING);
    ulclosestream::stderr_write(&m);
    Exit(1)
}

/// `stripped (src)`: without the white space around it; `None` if that is
/// all it was.
fn stripped(src: &[u8]) -> Option<Vec<u8>> {
    let src = cstr(src);
    let start = src
        .iter()
        .position(|&c| !terminfo::compile::scan::isspace(c))?;
    let mut v = src.get(start..).unwrap_or_default().to_vec();
    // `while (--len != 0 && isspace (dst[len]))`: never the first byte.
    while v.len() > 1
        && v.last()
            .is_some_and(|&c| terminfo::compile::scan::isspace(c))
    {
        v.pop();
    }
    Some(v)
}

/// `make_namelist (src)`: `-e`'s names -- from the file it names, if it has
/// a `/`, one a line; else comma-separated.
fn make_namelist(src: &[u8], progname: &[u8], showsummary: bool) -> Result<Vec<Vec<u8>>, Exit> {
    let mut dst = Vec::new();
    if src.contains(&b'/') {
        let (data, _) = open_input(src, progname, false)?;
        // `fgets (buffer, BUFSIZ, fp)`: a line, or as much of it as fits.
        let mut rest = data.as_slice();
        while !rest.is_empty() {
            let take = rest
                .iter()
                .position(|&c| c == b'\n')
                .map_or(rest.len(), |p| p.wrapping_add(1))
                .min(BUFSIZ - 1);
            let (line, after) = rest.split_at(take);
            if let Some(s) = stripped(line) {
                dst.push(s);
            }
            rest = after;
        }
    } else {
        for field in src.split(|&c| c == b',') {
            if let Some(s) = stripped(field) {
                dst.push(s);
            }
        }
    }
    if showsummary {
        let mut m = b"Entries that will be compiled:\n".to_vec();
        for (n, name) in dst.iter().enumerate() {
            m.extend_from_slice(format!("{}:", n.wrapping_add(1)).as_bytes());
            m.extend_from_slice(name);
            m.push(b'\n');
        }
        ulclosestream::stderr_write(&m);
    }
    Ok(dst)
}

/// `matches (needle, haystack)`: whether one of the names (all, without
/// `-e`) is one of the entry's.
fn matches(needle: Option<&[Vec<u8>]>, haystack: &[u8]) -> bool {
    needle.is_none_or(|n| n.iter().any(|name| name_match(haystack, name, b"|")))
}

/// `open_input (filename, alt_file)`: the source's bytes -- standard input
/// for `-` -- and whether it was standard input. A source that is not a
/// regular file is accepted only where upstream would copy it (`copy`).
fn open_input(filename: &[u8], progname: &[u8], copy: bool) -> Result<(Vec<u8>, bool), Exit> {
    let cannot = |reason: &str| {
        let mut m = progname.to_vec();
        m.extend_from_slice(b": cannot open '");
        m.extend_from_slice(filename);
        m.extend_from_slice(reason.as_bytes());
        m.push(b'\n');
        ulclosestream::stderr_write(&m);
        Exit(1)
    };
    if filename == b"-" {
        let mut data = Vec::new();
        if let Err(e) = std::io::stdin().lock().read_to_end(&mut data) {
            let mut m = b"copy_input (source)".to_vec();
            m.extend_from_slice(format!(": {}\n", coreutils::errmsg::strerror(&e)).as_bytes());
            ulclosestream::stderr_write(&m);
            return Err(Exit(1));
        }
        return copied(data, STDIN_NAME, progname).map(|d| (d, true));
    }
    let path = writer::os(filename);
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return Err(cannot(&format!("': {}", coreutils::errmsg::strerror(&e)))),
    };
    let ft = meta.file_type();
    let regular = ft.is_file();
    #[cfg(unix)]
    let special = {
        use std::os::unix::fs::FileTypeExt;
        ft.is_char_device() || ft.is_fifo()
    };
    #[cfg(not(unix))]
    let special = false;
    if ft.is_dir() || !(regular || special) {
        return Err(cannot("'; it is not a file"));
    }
    let mut f = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => return Err(cannot(&format!("': {}", coreutils::errmsg::strerror(&e)))),
    };
    if !regular && !copy {
        return Err(cannot("'; it is not a file"));
    }
    let mut data = Vec::new();
    if let Err(e) = f.read_to_end(&mut data) {
        let mut m = filename.to_vec();
        m.extend_from_slice(format!(": {}\n", coreutils::errmsg::strerror(&e)).as_bytes());
        ulclosestream::stderr_write(&m);
        return Err(Exit(1));
    }
    if regular {
        Ok((data, false))
    } else {
        copied(data, filename, progname).map(|d| (d, false))
    }
}

/// `copy_input`'s refusal of a NUL -- "don't loop in case someone wants
/// to convert /dev/zero".
fn copied(data: Vec<u8>, filename: &[u8], progname: &[u8]) -> Result<Vec<u8>, Exit> {
    if data.contains(&0) {
        let mut m = progname.to_vec();
        m.extend_from_slice(b": ");
        m.extend_from_slice(filename);
        m.extend_from_slice(b" is not a text-file\n");
        ulclosestream::stderr_write(&m);
        return Err(Exit(1));
    }
    Ok(data)
}

/// `put_translate (c)`: a comment character, a `<name>` in it translated
/// to its termcap name.
struct Translate {
    in_name: bool,
    namebuf: Vec<u8>,
}

impl Translate {
    fn put(&mut self, out: &mut Vec<u8>, c: u8) {
        if self.in_name {
            if c == b'\n' || c == b'@' {
                out.push(b'<');
                out.extend_from_slice(&self.namebuf);
                out.push(c);
                self.in_name = false;
            } else if c != b'>' {
                self.namebuf.push(c);
            } else {
                // "ah! candidate name!"
                self.in_name = false;
                let mut name = std::mem::take(&mut self.namebuf);
                let mut suffix = Vec::new();
                let at = name
                    .iter()
                    .position(|&c| c == b'#')
                    .or_else(|| name.iter().position(|&c| c == b'='))
                    .or_else(|| {
                        name.iter()
                            .position(|&c| c == b'@')
                            .filter(|&p| name.get(p.wrapping_add(1)) == Some(&b'>'))
                    });
                if let Some(at) = at {
                    suffix = name.split_off(at);
                }
                match nametrans(&name) {
                    Some(tp) => {
                        out.push(b':');
                        out.extend_from_slice(tp.as_bytes());
                        out.extend_from_slice(&suffix);
                        out.push(b':');
                    }
                    None => {
                        out.push(b'<');
                        out.extend_from_slice(&name);
                        out.extend_from_slice(&suffix);
                        out.push(b'>');
                    }
                }
            }
        } else {
            self.namebuf.clear();
            if c == b'<' {
                self.in_name = true;
            } else {
                out.push(c);
            }
        }
    }
}

/// `write_it (ep)`: `%{number}` made `%'char'` where that is shorter, then
/// the entry written.
fn write_it(
    w: &mut Writer,
    c: &mut Compiler<'_>,
    ep: &mut terminfo::compile::entry::Entry,
) -> Result<(), Abort> {
    for s in ep.tterm.strings.iter_mut().take(terminfo::STRCOUNT) {
        let Str::Value(v) = s else {
            continue;
        };
        let v_c = cstr(v).to_vec();
        if !v_c.contains(&b'{') {
            continue;
        }
        let mut result = Vec::with_capacity(v_c.len());
        let mut t = 0usize;
        while let Some(&ch) = v_c.get(t) {
            t = t.wrapping_add(1);
            result.push(ch);
            if ch == b'\\' {
                match v_c.get(t) {
                    None => break,
                    Some(&n) => {
                        result.push(n);
                        t = t.wrapping_add(1);
                    }
                }
            } else if ch == b'%' && v_c.get(t) == Some(&b'{') {
                let rest = v_c.get(t.wrapping_add(1)..).unwrap_or_default();
                let (value, used) = cstrtol::strtol(rest, 0);
                let after = t.wrapping_add(1).wrapping_add(used);
                if v_c.get(after) == Some(&b'}')
                    && value > 0
                    && value != i64::from(b'\\')
                    && value < 127
                    && let Ok(b) = u8::try_from(value)
                    && (0x20..0x7f).contains(&b)
                {
                    result.extend_from_slice(&[b'\'', b, b'\'']);
                    t = after.wrapping_add(1);
                }
            }
        }
        if result.len() < v_c.len() {
            *v = result;
        }
    }
    let first = first_name(&ep.tterm.term_names);
    c.scan.set_type(&first);
    c.scan.curr_line = i32::try_from(ep.startline).unwrap_or(i32::MAX);
    w.write_entry(&mut c.scan, &ep.tterm)
}

/// `show_databases (outdir)`: where `tic` could write, the first being
/// where it would; an error if neither.
fn show_databases(
    w: &mut Writer,
    outdir: Option<&[u8]>,
    out: &mut ulclosestream::Stdout,
    progname: &[u8],
) -> Result<(), Exit> {
    let specific = outdir.is_some() || w.env.terminfo.is_some();
    let outdir = match outdir {
        Some(o) => o.to_vec(),
        None => w.current_dir(),
    };
    let mut tried: Option<Vec<u8>> = None;
    match writer::valid_db_path(&outdir) {
        Some(r) => {
            out.write(&r);
            out.write(b"\n");
        }
        None => tried = Some(outdir),
    }
    if let Some(home) = w.home_terminfo() {
        match writer::valid_db_path(&home) {
            Some(r) => {
                out.write(&r);
                out.write(b"\n");
            }
            None if !specific => tried = Some(home),
            None => {}
        }
    }
    if let Some(t) = tried {
        out.flush();
        let mut m = progname.to_vec();
        m.extend_from_slice(b": ");
        m.extend_from_slice(&t);
        m.extend_from_slice(b" (no permission)\n");
        ulclosestream::stderr_write(&m);
        return Err(Exit(1));
    }
    Ok(())
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = ulclosestream::Stdout::new(1);
    let mut diag = Stderr;
    let mut to_remove: Option<Vec<u8>> = None;
    let status = match run(&argv, &mut out, &mut diag, &mut to_remove) {
        Ok(code) | Err(Exit(code)) => code,
    };
    // `cleanup`, the `atexit` handler, runs before `exit` flushes stdout.
    if let Some(name) = to_remove
        && let Err(e) = std::fs::remove_file(writer::os(&name))
    {
        let mut m = name;
        m.extend_from_slice(format!(": {}\n", coreutils::errmsg::strerror(&e)).as_bytes());
        ulclosestream::stderr_write(&m);
    }
    out.flush_at_exit();
    ExitCode::from(status)
}

/// What the dumper has printed, onto standard output.
fn drain(d: &mut Dumper, out: &mut ulclosestream::Stdout) {
    let text = std::mem::take(&mut d.stdout);
    out.write(&text);
}

/// Upstream's `main`, up to its `ExitProgram`.
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "upstream's main"
)]
fn run(
    argv: &[OsString],
    out: &mut ulclosestream::Stdout,
    diag: &mut Stderr,
    to_remove: &mut Option<Vec<u8>>,
) -> Result<u8, Exit> {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let progname = nc::rootname(&argv0).to_vec();

    let mut v_opt: i32 = -1;
    let mut smart_defaults = true;
    let mut outform = F_TERMINFO;
    let mut sortmode = S_TERMINFO;
    let mut width: i32 = 60;
    let mut height: i32 = 65535;
    let mut formatted = false;
    let mut literal = false;
    let mut numbers = 0;
    let mut forceresolve = false;
    let mut limited = true;
    let mut tversion: Option<Vec<u8>> = None;
    let mut outdir: Option<Vec<u8>> = None;
    let mut check_only = false;
    let mut suppress_untranslatable = false;
    let mut quickdump = 0;
    let mut quiet = false;
    let mut wrap_strings = false;
    let mut showsummary = false;
    let mut namelst: Option<Vec<Vec<u8>>> = None;
    let mut using_extensions = false;
    let mut disable_period = false;
    let mut strict_bsd = false;

    let mut infodump = nc::same_program(&progname, b"captoinfo");
    if infodump {
        outform = F_TERMINFO;
        sortmode = S_TERMINFO;
    }
    let mut capdump = nc::same_program(&progname, b"infotocap");
    if capdump {
        outform = F_TERMCAP;
        sortmode = S_TERMCAP;
    }
    let env = terminfo::Env::from_process();
    let mut w = Writer::new(env.clone(), false);

    let words = argv.get(1..).unwrap_or_default();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut last_opt = b'?';
    for item in TIC
        .parse(words, "0123456789CDIKLNQR:TUVWace:fGgo:qrstvwx", &[])
        .short_only(true)
    {
        let opt = match item {
            Err(e) => {
                nc::getopt_complaint(&argv0, &e.sentence);
                return Err(usage(&progname));
            }
            Ok(o) => o,
        };
        let arg = |v: &Option<OsString>| {
            v.as_ref()
                .map(|v| os_bytes(v).into_owned())
                .unwrap_or_default()
        };
        let this_opt = match &opt {
            Opt::Short(c, _) => *c,
            Opt::Operand(o) => {
                operands.push(os_bytes(o).into_owned());
                continue;
            }
            Opt::Long(..) => return Err(usage(&progname)),
        };
        if this_opt.is_ascii_digit() {
            let add = |t: &mut i32| {
                *t = t
                    .wrapping_mul(10)
                    .wrapping_add(i32::from(this_opt.wrapping_sub(b'0')));
            };
            match last_opt {
                b'Q' => add(&mut quickdump),
                b'v' => add(&mut v_opt),
                b'w' => add(&mut width),
                _ => match this_opt {
                    b'0' => {
                        last_opt = this_opt;
                        width = 65535;
                        height = 1;
                    }
                    b'1' => {
                        last_opt = this_opt;
                        width = 0;
                    }
                    _ => return Err(usage(&progname)),
                },
            }
            continue;
        }
        let Opt::Short(_, value) = opt else {
            continue;
        };
        match this_opt {
            b'K' => strict_bsd = true,
            b'C' => {
                capdump = true;
                outform = F_TERMCAP;
                sortmode = S_TERMCAP;
            }
            b'D' => {
                let level = if v_opt > 0 {
                    v_opt
                } else {
                    i32::from(v_opt == 0)
                };
                let _ = level;
                show_databases(&mut w, outdir.as_deref(), out, &progname)?;
                return Ok(0);
            }
            b'I' => {
                infodump = true;
                outform = F_TERMINFO;
                sortmode = S_TERMINFO;
            }
            b'L' => {
                infodump = true;
                outform = F_VARIABLE;
                sortmode = S_VARIABLE;
            }
            b'N' => {
                smart_defaults = false;
                literal = true;
            }
            b'Q' => quickdump = 0,
            b'R' => tversion = Some(arg(&value)),
            b'T' => limited = false,
            b'U' => literal = true,
            b'V' => {
                out.write(&nc::version_line(&progname));
                return Ok(0);
            }
            b'W' => wrap_strings = true,
            b'c' => check_only = true,
            b'e' => namelst = Some(make_namelist(&arg(&value), &progname, showsummary)?),
            b'f' => formatted = true,
            b'G' => numbers = 1,
            b'g' => numbers = -1,
            b'o' => outdir = Some(arg(&value)),
            b'q' => quiet = true,
            b'r' => forceresolve = true,
            b's' => showsummary = true,
            b'v' => v_opt = 0,
            b'w' => width = 0,
            b't' => {
                disable_period = false;
                suppress_untranslatable = true;
            }
            b'a' => {
                disable_period = true;
                using_extensions = true;
            }
            b'x' => using_extensions = true,
            _ => return Err(usage(&progname)),
        }
        last_opt = this_opt;
    }

    // "If the -v option is set, it may override the $NCURSES_TRACE
    // environment variable"
    let debug_level: u32 = if v_opt > 0 {
        u32::try_from(v_opt).unwrap_or(0)
    } else {
        u32::from(v_opt == 0)
    };
    let tracing = debug_level.wrapping_shl(13);
    let user_definable = using_extensions;
    w.user_definable = user_definable;

    let mut c = Compiler::new(diag);
    c.user_definable = user_definable;
    c.env = env;
    c.tracing = tracing;
    c.scan.strict_bsd = strict_bsd;
    c.scan.disable_period = disable_period;
    if tracing != 0 {
        c.check_termtype = Some(Box::new(check::Checker::new(
            debug_level,
            capdump,
            using_extensions,
            user_definable,
        )));
    }

    // The source. (Upstream starts it as "terminfo", which every path
    // replaces.)
    let mut source_file: Vec<u8>;
    let mut data: Option<Vec<u8>> = None;
    if let Some((first, rest)) = operands.split_first() {
        source_file = first.clone();
        if !rest.is_empty() {
            let mut m = progname.clone();
            m.extend_from_slice(b": Too many file names.  Usage:\n\t");
            m.extend_from_slice(&progname);
            m.push(b' ');
            m.extend_from_slice(USAGE_STRING);
            ulclosestream::stderr_write(&m);
            return Err(Exit(1));
        }
    } else if infodump {
        // "captoinfo's no-argument case"
        source_file = b"/etc/termcap".to_vec();
        let termcap = std::env::var_os("TERMCAP").map(|t| os_bytes(&t).into_owned());
        let term = std::env::var_os("TERM").map(|t| os_bytes(&t).into_owned());
        if let (Some(termcap), Some(term)) = (termcap, term) {
            namelst = Some(make_namelist(&term, &progname, showsummary)?);
            if writer::sys_access(&termcap, 0).is_ok() {
                source_file = termcap;
            } else {
                // The entry itself: into a file of its own, whose name the
                // warnings then give, removed at the end.
                let mut text = termcap;
                text.push(b'\n');
                match open_tempfile(&text) {
                    Some(name) => {
                        *to_remove = Some(name.clone());
                        source_file = name;
                        data = Some(text);
                    }
                    None => {
                        ulclosestream::stderr_write(b"tmpnam: No such file or directory\n");
                        return Err(Exit(1));
                    }
                }
            }
        }
    } else {
        let mut m = progname.clone();
        m.extend_from_slice(b": File name needed.  Usage:\n\t");
        m.extend_from_slice(&progname);
        m.push(b' ');
        m.extend_from_slice(USAGE_STRING);
        ulclosestream::stderr_write(&m);
        return Err(Exit(1));
    }
    let data = match data {
        Some(d) => d,
        None => {
            let (d, was_stdin) = open_input(&source_file, &progname, true)?;
            if was_stdin {
                source_file = STDIN_NAME.to_vec();
            }
            d
        }
    };

    let mut d = Dumper::new(&progname, user_definable);
    if infodump || check_only {
        d.init(
            &mut c.scan,
            tversion.as_deref(),
            if smart_defaults { outform } else { F_LITERAL },
            sortmode,
            wrap_strings,
            width,
            height,
            debug_level,
            formatted || check_only,
            check_only,
            quickdump,
        );
    } else if capdump {
        d.init(
            &mut c.scan,
            tversion.as_deref(),
            outform,
            sortmode,
            wrap_strings,
            width,
            height,
            debug_level,
            false,
            false,
            0,
        );
    }

    // "parse entries out of the source file"
    c.scan.set_source(Some(&source_file));
    c.read_entry_source(
        Source::File(Box::new(std::io::Cursor::new(data.clone())), Some(0)),
        !smart_defaults || literal,
        false,
    )?;

    // "do use resolution"
    if (check_only || (!infodump && !capdump) || forceresolve)
        && !c.resolve_uses2(true, literal)?
        && !check_only
    {
        return Err(Exit(1));
    }

    // "length check"
    if check_only && limited && (capdump || infodump) {
        for qi in 0..c.entries.len() {
            let Some(qp) = c.entries.get_mut(qi) else {
                break;
            };
            if matches(namelst.as_deref(), &qp.tterm.term_names) {
                let mut t = qp.tterm.clone();
                let len = d.fmt_entry(&mut c.scan, &mut t, None, false, true, infodump, numbers);
                let limit = if infodump {
                    MAX_TERMINFO_LENGTH
                } else {
                    MAX_TERMCAP_LENGTH
                };
                if len > limit {
                    let mut m = progname.clone();
                    m.extend_from_slice(b": resolved ");
                    m.extend_from_slice(&first_name(&t.term_names));
                    m.extend_from_slice(format!(" entry is {len} bytes long\n").as_bytes());
                    ulclosestream::stderr_write(&m);
                }
            }
        }
    }

    // "write or dump all entries"
    if check_only {
        // "this is in case infotocap() generates warnings"
        c.scan.curr_col = -1;
        c.scan.curr_line = -1;
        for qi in 0..c.entries.len() {
            let names = c
                .entries
                .get(qi)
                .map(|e| e.tterm.term_names.clone())
                .unwrap_or_default();
            if !matches(namelst.as_deref(), &names) {
                continue;
            }
            c.scan.set_type(&first_name(&names));
            let startline = c.entries.get(qi).map_or(0, |e| e.startline);
            c.scan.curr_line = i32::try_from(startline).unwrap_or(i32::MAX);
            let Some(qp) = c.entries.get_mut(qi) else {
                break;
            };
            repair_acsc(&mut qp.tterm);
            let mut t = std::mem::take(&mut qp.tterm);
            d.dump_entry(
                &mut c.scan,
                &mut t,
                suppress_untranslatable,
                limited,
                numbers,
                None,
            );
            if let Some(qp) = c.entries.get_mut(qi) {
                qp.tterm = t;
            }
            drain(&mut d, out);
        }
    } else if !infodump && !capdump {
        w.set_writedir(&mut c.scan, outdir.as_deref())?;
        for qi in 0..c.entries.len() {
            let names = c
                .entries
                .get(qi)
                .map(|e| e.tterm.term_names.clone())
                .unwrap_or_default();
            if !matches(namelst.as_deref(), &names) {
                continue;
            }
            let Some(slot) = c.entries.get_mut(qi) else {
                break;
            };
            let mut ep = std::mem::take(slot);
            let r = write_it(&mut w, &mut c, &mut ep);
            if let Some(slot) = c.entries.get_mut(qi) {
                *slot = ep;
            }
            r?;
        }
    } else {
        // "this is in case infotocap() generates warnings"
        c.scan.curr_col = -1;
        c.scan.curr_line = -1;
        let mut tr = Translate {
            in_name: false,
            namebuf: Vec::new(),
        };
        for qi in 0..c.entries.len() {
            let names = c
                .entries
                .get(qi)
                .map(|e| e.tterm.term_names.clone())
                .unwrap_or_default();
            if !matches(namelst.as_deref(), &names) {
                continue;
            }
            let (cstart, cend) = c.entries.get(qi).map_or((0, 0), |e| (e.cstart, e.cend));
            c.scan.set_type(&first_name(&names));
            if !quiet {
                let from = usize::try_from(cstart).unwrap_or(0);
                let j = usize::try_from(cend.wrapping_sub(cstart)).unwrap_or(0);
                let mut text = Vec::new();
                for &ch in data.get(from..).unwrap_or_default().iter().take(j) {
                    if infodump {
                        text.push(ch);
                    } else {
                        tr.put(&mut text, ch);
                    }
                }
                out.write(&text);
            }
            let Some(qp) = c.entries.get_mut(qi) else {
                break;
            };
            repair_acsc(&mut qp.tterm);
            let mut t = std::mem::take(&mut qp.tterm);
            d.dump_entry(
                &mut c.scan,
                &mut t,
                suppress_untranslatable,
                limited,
                numbers,
                None,
            );
            let uses: Vec<Option<Vec<u8>>> = c
                .entries
                .get(qi)
                .map(|e| {
                    e.uses
                        .iter()
                        .take(e.nuses)
                        .map(|u| u.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            for u in &uses {
                d.dump_uses(&mut c.scan, u.as_deref(), !capdump);
            }
            if let Some(qp) = c.entries.get_mut(qi) {
                qp.tterm = t;
            }
            drain(&mut d, out);
            let len = d.show_entry();
            if debug_level != 0 && !limited {
                d.stdout
                    .extend_from_slice(format!("# length={len}\n").as_bytes());
            }
            drain(&mut d, out);
        }
        if namelst.is_none()
            && !quiet
            && let Some(tail) = c.entries.last()
        {
            let from = usize::try_from(tail.cend).unwrap_or(0);
            let mut oldc = 0u8;
            let mut in_comment = false;
            let mut trailing_comment = false;
            let mut text = Vec::new();
            for &ch in data.get(from..).unwrap_or_default() {
                if oldc == b'\n' {
                    if ch == b'#' {
                        trailing_comment = true;
                        in_comment = true;
                    } else {
                        in_comment = false;
                    }
                }
                if trailing_comment && (in_comment || (oldc == b'\n' && ch == b'\n')) {
                    text.push(ch);
                }
                oldc = ch;
            }
            out.write(&text);
        }
    }

    // "Show the directory into which entries were written, and the total
    // number of entries"
    if showsummary && !(check_only || infodump || capdump) {
        let total = w.total_written;
        let m = if total != 0 {
            let mut m = format!("{total} entries written to ").into_bytes();
            m.extend_from_slice(&w.current_dir());
            m.push(b'\n');
            m
        } else {
            b"No entries written\n".to_vec()
        };
        ulclosestream::stderr_write(&m);
    }
    Ok(0)
}

/// `open_tempfile`, written: a new file in `/tmp`, made only for this user,
/// holding `text`; its name.
fn open_tempfile(text: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        ^ u128::from(std::process::id());
    for attempt in 0u128..100 {
        let mut v = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(attempt);
        let mut name = b"/tmp/".to_vec();
        for _ in 0..6 {
            let at = usize::try_from(v % 62).unwrap_or(0);
            name.push(CHARS.get(at).copied().unwrap_or(b'X'));
            v /= 62;
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        if let Ok(mut f) = opts.open(writer::os(&name)) {
            return f.write_all(text).ok().map(|()| name);
        }
    }
    None
}
