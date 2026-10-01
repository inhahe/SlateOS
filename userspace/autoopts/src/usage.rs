//! Usage, version and paged help (`usage.c`, `version.c`, `pgusage.c`).
//!
//! sharutils ships its usage texts precomputed, and libopts prints those
//! verbatim -- unless `AUTOOPTS_USAGE` (or an rc file's `<?auto-options>`)
//! says `compute`, in which case the text is built from the descriptors, in
//! the GNU layout or (`autoopts`) libopts' own table. Both are here, since a
//! user can select either.

use std::io;

use crate::charmap::{END_LIST_ENTRY, GRAPHIC, WHITESPACE, at, is, spn, strneqvcmp};
use crate::config::make_path;
use crate::out::Out;
use crate::{EXIT_REQ_USAGE, Exit, NOLIMIT, Options, Role, bytes_os, getenv, pr, st};

/// `AUTOOPTS_USAGE` words and the bits they set or (for the negated masks)
/// clear, in `AOFLAG_TABLE` order.
const USAGE_FLAGS: [(&[u8], u32, bool); 5] = [
    (b"gnu", pr::GNUUSAGE, true),
    (b"autoopts", pr::GNUUSAGE, false),
    (b"no_misuse_usage", pr::MISUSE, true),
    (b"misuse_usage", pr::MISUSE, false),
    (b"compute", pr::COMPUTE, true),
];

/// "Please send bug reports to:" (`zPlsSendBugs`).
fn bug_line(addr: &str) -> Vec<u8> {
    format!("\nPlease send bug reports to:  <{addr}>\n").into_bytes()
}

/// The option-line formats one layout uses (`arg_types_t`).
struct ArgTypes {
    /// A string argument.
    string: &'static str,
    /// A required option's mark.
    req: &'static str,
    /// An optional argument.
    opt: &'static str,
    /// No argument.
    none: &'static str,
    /// Before an option with no flag character.
    no_flag: &'static str,
    /// Before every option, when there are no flag characters at all.
    spc: &'static str,
    /// How the argument type and the name are laid out.
    fmt: OptFmt,
}

/// `pzOptFmt`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OptFmt {
    /// `--%2$s%1$s`: the GNU layout.
    Gnu,
    /// `%2$s%1$s`: GNU, with neither long nor flag options.
    GnuBare,
    /// `%s`: GNU, flag options only.
    ShortGnu,
    /// ` %3s %s`: libopts' own, no option required.
    Normal,
    /// ` %3s %-14s %s`: libopts' own, with a required-option column.
    Required,
}

/// `%-Ns`: `s` padded with spaces to `n` bytes.
fn pad(s: &[u8], n: usize) -> Vec<u8> {
    let mut v = s.to_vec();
    while v.len() < n {
        v.push(b' ');
    }
    v
}

/// `%Ns`: `s` right-aligned in `n` bytes.
fn rpad(s: &[u8], n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    while v.len().saturating_add(s.len()) < n {
        v.push(b' ');
    }
    v.extend_from_slice(s);
    v
}

impl Options {
    /// `set_usage_flags`: `txt`, or `AUTOOPTS_USAGE` when `None`. Anything
    /// not fully understood -- or asking for both of a pair -- changes
    /// nothing.
    pub(crate) fn set_usage_flags(&mut self, txt: Option<&[u8]>) {
        let env;
        let txt = match txt {
            Some(t) => t,
            None => {
                env = getenv(b"AUTOOPTS_USAGE");
                match &env {
                    Some(v) => v.as_slice(),
                    None => return,
                }
            }
        };
        let flg = parse_usage_flags(txt);
        if flg == 0 {
            return;
        }
        if flg & 0b11 == 0b11 || flg & 0b1100 == 0b1100 {
            return;
        }
        for (i, &(_, mask, set)) in USAGE_FLAGS.iter().enumerate() {
            if flg & (1 << i) != 0 {
                if set {
                    self.set |= mask;
                } else {
                    self.set &= !mask;
                }
            }
        }
    }

    /// `optionUsage`: usage to stdout (exit 0) or stderr (exit `code`).
    pub(crate) fn option_usage(&mut self, code: i32) -> Exit {
        let exit_code = if code == EXIT_REQ_USAGE { 0 } else { code };
        let mut out = if exit_code == 0 {
            Out::stdout()
        } else {
            Out::Stderr
        };
        self.usage_into(&mut out, code)
    }

    /// `optionUsage` with `option_usage_fp` already chosen.
    fn usage_into(&mut self, out: &mut Out, usage_exit_code: i32) -> Exit {
        let exit_code = if usage_exit_code == EXIT_REQ_USAGE {
            0
        } else {
            usage_exit_code
        };
        self.set_usage_flags(None);
        if self.set & pr::COMPUTE == 0 {
            let text = if exit_code == 0 {
                self.prog.full_usage
            } else {
                self.prog.short_usage
            };
            out.put(text.as_bytes());
        } else {
            out.put(&self.title());
            if exit_code == 0 || self.set & pr::MISUSE == 0 {
                if let Some(e) = self.print_usage_details(out, usage_exit_code) {
                    return e;
                }
            } else {
                self.print_offer_usage(out);
            }
        }
        out.flush();
        if let Some(e) = out.error() {
            let name: &[u8] = if out.is_stdout() {
                b"standard output"
            } else {
                b"standard error"
            };
            return self.fserr_exit(b"write", name, &e);
        }
        Exit(exit_code)
    }

    /// The usage title with the program's name in it.
    fn title(&self) -> Vec<u8> {
        let title = self.prog.usage_title.as_bytes();
        match title.windows(2).position(|w| w == b"%s") {
            Some(p) => {
                let mut v = title.get(..p).unwrap_or_default().to_vec();
                v.extend_from_slice(&self.prog_name);
                v.extend_from_slice(title.get(p.saturating_add(2)..).unwrap_or_default());
                v
            }
            None => title.to_vec(),
        }
    }

    /// `print_offer_usage`: "Try 'PROG --help' for more information."
    fn print_offer_usage(&self, out: &mut Out) {
        let help = self
            .prog
            .descs
            .iter()
            .skip(self.prog.preset_ct)
            .find(|d| d.role == Role::Help);
        let word: Vec<u8> = match (help, self.set & (pr::LONGOPT | pr::SHORTOPT)) {
            (Some(d), pr::SHORTOPT) => vec![b'-', d.value],
            (Some(d), 0) => d.name.as_bytes().iter().take(20).copied().collect(),
            (Some(d), _) => {
                let mut v = b"--".to_vec();
                v.extend(d.name.as_bytes().iter().take(20));
                v
            }
            (None, pr::SHORTOPT) => b"-h".to_vec(),
            (None, 0) => b"help".to_vec(),
            (None, _) => b"--help".to_vec(),
        };
        let mut msg = b"Try '".to_vec();
        msg.extend_from_slice(&self.prog_name);
        msg.push(b' ');
        msg.extend_from_slice(&word);
        msg.extend_from_slice(b"' for more information.\n");
        out.put(&msg);
    }

    /// `setGnuOptFmts` / `setStdOptFmts`: the layout, its column width and
    /// its table heading.
    fn formats(&self) -> (ArgTypes, usize, &'static str) {
        if self.set & pr::GNUUSAGE != 0 {
            let short_only = self.set & (pr::LONGOPT | pr::SHORTOPT) == pr::SHORTOPT;
            let fmt = match self.set & (pr::LONGOPT | pr::SHORTOPT) {
                0 => OptFmt::GnuBare,
                pr::SHORTOPT => OptFmt::ShortGnu,
                _ => OptFmt::Gnu,
            };
            let types = ArgTypes {
                string: if short_only { " str" } else { "=str" },
                req: " ",
                opt: if short_only { " [arg]" } else { "[=arg]" },
                none: " ",
                no_flag: "      ",
                spc: "   ",
                fmt,
            };
            let flen = if short_only { 8 } else { 22 };
            return (types, flen, "  Flg Arg Option-Name    Description\n");
        }
        let (title, fmt, flen) = match self.set & (pr::NO_REQ_OPT | pr::SHORTOPT) {
            x if x == pr::NO_REQ_OPT | pr::SHORTOPT => {
                ("  Flg Arg Option-Name    Description\n", OptFmt::Normal, 19)
            }
            pr::NO_REQ_OPT => ("   Arg Option-Name    Description\n", OptFmt::Normal, 19),
            pr::SHORTOPT => (
                "  Flg Arg Option-Name   Req?  Description\n",
                OptFmt::Required,
                24,
            ),
            _ => (
                "   Arg Option-Name   Req?  Description\n",
                OptFmt::Required,
                24,
            ),
        };
        let types = ArgTypes {
            string: "Str",
            req: "YES",
            opt: "opt",
            none: "no ",
            no_flag: "     ",
            spc: "  ",
            fmt,
        };
        (types, flen, title)
    }

    /// `print_usage_details`: the computed option table and what follows
    /// it. `Some` when the stream failed.
    fn print_usage_details(&mut self, out: &mut Out, exit_code: i32) -> Option<Exit> {
        let (types, flen, title) = self.formats();
        if self.set & pr::GNUUSAGE != 0 {
            out.put(b"\n");
        } else if exit_code != 0
            || self
                .state
                .first()
                .is_none_or(|s| s.flags & st::DOCUMENT == 0)
        {
            out.put(title.as_bytes());
        }
        let skip = 4usize.saturating_sub(flen.saturating_add(15) / 8);
        self.prt_opt_usage(out, exit_code, &types, flen, skip);
        match self.set & (pr::LONGOPT | pr::SHORTOPT) {
            x if x == pr::LONGOPT | pr::SHORTOPT => out.put(
                b"Options are specified by doubled hyphens and their name or by a single\nhyphen and the flag character.\n",
            ),
            pr::SHORTOPT => {}
            pr::LONGOPT => out.put(b"Options are specified by single or double hyphens and their name.\n"),
            _ => out.put(b"All arguments are named options.\n"),
        }
        if self.set & pr::NUM_OPT != 0 {
            out.put(b"The '-#<number>' option may omit the hash char\n");
        }
        if self.set & pr::REORDER != 0 {
            out.put(b"Operands and options may be intermixed.  They will be reordered.\n");
        }
        if let Some(explain) = self.prog.explain {
            out.put(explain.as_bytes());
        }
        if exit_code == 0 {
            self.prt_prog_detail(out);
        }
        out.put(&bug_line(self.prog.bug_addr));
        out.flush();
        let e = out.error()?;
        let name: &[u8] = if out.is_stderr() {
            b"standard error"
        } else {
            b"standard output"
        };
        Some(self.fserr_exit(b"write", name, &e))
    }

    /// `prt_opt_usage`: one line per option, and for full usage the
    /// details under it.
    fn prt_opt_usage(
        &self,
        out: &mut Out,
        exit_code: i32,
        types: &ArgTypes,
        flen: usize,
        skip: usize,
    ) {
        let tab = |s: &str| -> Vec<u8> { s.as_bytes().get(skip..).unwrap_or_default().to_vec() };
        for (i, d) in self.prog.descs.iter().enumerate() {
            let flags = self.state.get(i).map_or(d.flags, |s| s.flags);
            if flags & st::NO_USAGE_MASK != 0 {
                if flags == (st::OMITTED | st::NO_INIT) && exit_code == 0 {
                    self.prt_preamble(out, d.value, types);
                    let why = if d.text.is_empty() {
                        "This option has been disabled"
                    } else {
                        d.text
                    };
                    let mut line = b" --- ".to_vec();
                    line.extend_from_slice(&pad(d.name.as_bytes(), 14));
                    line.push(b' ');
                    line.extend_from_slice(why.as_bytes());
                    line.push(b'\n');
                    out.put(&line);
                }
                continue;
            }
            if flags & st::DOCUMENT != 0 {
                continue;
            }
            if self.set & pr::VENDOR_OPT != 0 && !is(d.value, GRAPHIC) {
                continue;
            }
            self.prt_preamble(out, d.value, types);
            let atyp = if flags & st::ARG_OPTIONAL != 0 {
                types.opt
            } else if flags & st::ARG_TYPE_MASK == 0 {
                types.none
            } else {
                types.string
            };
            let name = d.name.as_bytes();
            let mut z = match types.fmt {
                OptFmt::Gnu => {
                    let mut v = b"--".to_vec();
                    v.extend_from_slice(name);
                    v.extend_from_slice(atyp.as_bytes());
                    v
                }
                OptFmt::GnuBare => {
                    let mut v = name.to_vec();
                    v.extend_from_slice(atyp.as_bytes());
                    v
                }
                OptFmt::ShortGnu => atyp.as_bytes().to_vec(),
                OptFmt::Normal => {
                    let mut v = b" ".to_vec();
                    v.extend_from_slice(&rpad(atyp.as_bytes(), 3));
                    v.push(b' ');
                    v.extend_from_slice(name);
                    v
                }
                OptFmt::Required => {
                    let mut v = b" ".to_vec();
                    v.extend_from_slice(&rpad(atyp.as_bytes(), 3));
                    v.push(b' ');
                    v.extend_from_slice(&pad(name, 14));
                    v.push(b' ');
                    v.extend_from_slice(if d.min != 0 {
                        types.req.as_bytes()
                    } else {
                        types.opt.as_bytes()
                    });
                    v
                }
            };
            // `snprintf(z, 80, ...)`.
            z.truncate(79);
            let mut line = pad(&z, flen);
            line.push(b' ');
            line.extend_from_slice(d.text.as_bytes());
            line.push(b'\n');
            out.put(&line);
            if exit_code != 0 {
                continue;
            }
            // `prt_extd_usage`.
            if let Some(dn) = d.disable_name {
                let mut l = tab("\t\t\t\t- disabled as '--");
                l.extend_from_slice(dn.as_bytes());
                l.extend_from_slice(b"'\n");
                out.put(&l);
            }
            if flags & 0x800 != 0 {
                out.put(&tab("\t\t\t\t- enabled by default\n"));
            }
            if flags & st::NO_INIT != 0 && i < self.prog.preset_ct {
                out.put(&tab("\t\t\t\t- may not be preset\n"));
            }
            if d.min <= 1 {
                match d.max {
                    0 => out.put(&tab("\t\t\t\t- may NOT appear - preset only\n")),
                    NOLIMIT => out.put(&tab("\t\t\t\t- may appear multiple times\n")),
                    1 => {}
                    n => {
                        let mut l = tab("\t\t\t\t- may appear up to ");
                        l.extend_from_slice(format!("{n} times\n").as_bytes());
                        out.put(&l);
                    }
                }
            } else {
                let mut l = tab("\t\t\t\t- must appear between ");
                l.extend_from_slice(format!("{} and {} times\n", d.min, d.max).as_bytes());
                out.put(&l);
            }
        }
        out.put(b"\n");
    }

    /// `prt_preamble`: the flag column.
    fn prt_preamble(&self, out: &mut Out, value: u8, types: &ArgTypes) {
        let gnu_long = self.set & (pr::GNUUSAGE | pr::LONGOPT) == (pr::GNUUSAGE | pr::LONGOPT);
        if self.set & pr::SHORTOPT == 0 {
            out.put(types.spc.as_bytes());
        } else if !is(value, GRAPHIC) {
            if gnu_long {
                out.put(b" ");
            }
            out.put(types.no_flag.as_bytes());
        } else {
            out.put(&[b' ', b' ', b' ', b'-', value]);
            if gnu_long {
                out.put(b", ");
            }
        }
    }

    /// `prt_prog_detail`: the rc files, then the detail text.
    fn prt_prog_detail(&self, out: &mut Out) {
        if !self.prog.home_list.is_empty() {
            out.put(b"\nThe following option preset mechanisms are supported:\n");
            for &path in self.prog.home_list {
                let made = make_path(4097, path.as_bytes(), &self.prog_path);
                let path_b = path.as_bytes();
                let nm: Vec<u8> = made.clone().unwrap_or_else(|| path_b.to_vec());
                let shown: Vec<u8> = match &made {
                    Some(m)
                        if path_b.first() == Some(&b'$')
                            && matches!(path_b.get(1), Some(b'$' | b'@')) =>
                    {
                        m.clone()
                    }
                    _ => path_b.to_vec(),
                };
                let mut line = b" - reading file ".to_vec();
                line.extend_from_slice(&shown);
                out.put(&line);
                if !self.prog.rc_name.is_empty()
                    && std::fs::metadata(bytes_os(&nm)).is_ok_and(|m| m.is_dir())
                {
                    out.put(b"/");
                    out.put(self.prog.rc_name.as_bytes());
                }
                out.put(b"\n");
            }
        }
        if let Some(detail) = self.prog.detail {
            out.put(detail.as_bytes());
        }
    }

    /// `optionPrintVersion`: `--version[=MODE]`, to stdout.
    pub(crate) fn print_version(&mut self, od: usize) -> Exit {
        let state = self.state.get(od).cloned();
        let explicit = state.as_ref().and_then(|s| {
            (s.flags & st::ARG_OPTIONAL != 0)
                .then_some(s.arg.as_deref())
                .flatten()
                .and_then(|a| a.first().copied())
        });
        let ch = match explicit {
            Some(c) => c,
            None => {
                self.set_usage_flags(None);
                if self.set & pr::GNUUSAGE != 0 {
                    b'c'
                } else {
                    b'v'
                }
            }
        };
        let mut out = Out::stdout();
        let prog = self.prog;
        match ch {
            b'v' | b'V' => {
                let full = prog.full_version.as_bytes();
                let first = full.split(|&b| b == b'\n').next().unwrap_or_default();
                out.put(first);
                out.put(b"\n");
            }
            b'c' | b'C' => {
                out.put(prog.copyright.as_bytes());
                out.put(b"\n");
                out.put(&bug_line(prog.bug_addr));
            }
            b'n' | b'N' => {
                out.put(prog.copyright.as_bytes());
                out.put(prog.copy_notice.as_bytes());
                out.put(b"\n");
                out.put(
                    b"Automated Options version 41.1\n\tCopyright (C) 1999-2014 by Bruce Korb - all rights reserved\n",
                );
                out.put(b"\n");
                out.put(&bug_line(prog.bug_addr));
            }
            _ => {
                let mut msg = b"error: version option argument ".to_vec();
                msg.extend_from_slice(&crate::shown_in_quotes(&[ch]));
                msg.extend_from_slice(
                    b" invalid.  Use:\n\t'v' - version only\n\t'c' - version and copyright\n\t'n' - version and full copyright notice\n",
                );
                ulclosestream::stderr_write(&msg);
                return Exit(1);
            }
        }
        out.flush();
        if let Some(e) = out.error() {
            return self.fserr_exit(b"write", b"standard output", &e);
        }
        Exit(0)
    }

    /// `optionPagedUsage`: the full usage into a temporary file, then
    /// `$PAGER` (or `more`) on it -- run, as upstream's `atexit` handler runs
    /// it, after the usage has decided the exit status.
    pub(crate) fn paged_usage(&mut self) -> Exit {
        let tmpdir = getenv(b"TMPDIR").unwrap_or_else(|| b"/tmp".to_vec());
        let mut prefix = tmpdir;
        prefix.extend_from_slice(format!("/use-{}.", std::process::id()).as_bytes());
        let Some((file, name)) = mkstemp(&prefix) else {
            return self.option_usage(0);
        };
        let mut out = Out::Temp {
            file,
            held: Vec::new(),
            failed: None,
        };
        let exit = self.usage_into(&mut out, 0);
        drop(out);
        let pager = getenv(b"PAGER").unwrap_or_else(|| b"more".to_vec());
        let mut cmd = pager;
        cmd.push(b' ');
        cmd.extend_from_slice(&name);
        cmd.extend_from_slice(b" ; rm -f ");
        cmd.extend_from_slice(&name);
        run_pager(&cmd);
        exit
    }
}

/// `parse_usage_flags`: the bit for each word of `txt`, or 0 if any word is
/// not one.
fn parse_usage_flags(txt: &[u8]) -> u32 {
    let mut p = spn(txt, 0, WHITESPACE);
    if at(txt, p) == 0 {
        return 0;
    }
    let mut res = 0u32;
    loop {
        let rest = txt.get(p..).unwrap_or_default();
        let Some((ix, (word, _, _))) = USAGE_FLAGS
            .iter()
            .enumerate()
            .find(|(_, (w, _, _))| strneqvcmp(rest, w, w.len()) == 0)
        else {
            return 0;
        };
        if !is(at(txt, p.saturating_add(word.len())), END_LIST_ENTRY) {
            return 0;
        }
        res |= 1 << ix;
        p = spn(txt, p.saturating_add(word.len()), WHITESPACE);
        match at(txt, p) {
            0 => return res,
            b',' => p = spn(txt, p.saturating_add(1), WHITESPACE),
            _ => {}
        }
    }
}

/// `mkstemp`: a new file named `prefix` and six random characters.
fn mkstemp(prefix: &[u8]) -> Option<(std::fs::File, Vec<u8>)> {
    use std::hash::{BuildHasher, Hasher};
    const LETTERS: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    // glibc tries 62^3 names before giving up.
    for attempt in 0u32..238_328 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u32(attempt);
        let mut v = h.finish();
        let mut name = prefix.to_vec();
        for _ in 0..6 {
            let i = usize::try_from(v % 62).unwrap_or(0);
            name.push(LETTERS.get(i).copied().unwrap_or(b'X'));
            v /= 62;
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(bytes_os(&name)) {
            Ok(f) => return Some((f, name)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
    }
    None
}

/// `system(cmd)` after `fclose(stderr); dup2(1, 2)`: the shell's
/// diagnostics go to standard output, as the pager's do. Its status is
/// ignored, as upstream ignores it.
fn run_pager(cmd: &[u8]) {
    let mut command = shellcmd::shell_bytes(cmd);
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        if let Ok(fd) = io::stdout().as_fd().try_clone_to_owned() {
            command.stderr(std::process::Stdio::from(fd));
        }
    }
    // The pager's own failure is upstream's to ignore too (`ignore_val`).
    let _ = command.status();
}
