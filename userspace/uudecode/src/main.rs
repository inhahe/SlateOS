//! `uudecode` -- GNU sharutils 4.15.2's, checked against Ubuntu 24.04's build
//! of it by `scripts/uu-diff.sh`.
//!
//! ```text
//! uudecode [-o OUTPUT-FILE] [-c] [FILE...]
//! ```
//!
//! Finds the `begin` line in each FILE (or standard input), creates the file
//! it names with the mode it gives -- or writes to standard output for
//! `/dev/stdout` and `-` -- and decodes into it, traditional uuencoding up to
//! the zero-length line and `end`, or base64 (`begin-base64`) up to `====`.
//! `begin-encoded` names are themselves base64; `~/` and `~user/` names are
//! expanded. `-o` overrides the name, and is acted on the moment the option
//! is read: it reopens standard output there and then, before any input is
//! looked at.
//!
//! The options, `~/.sharrc`, `--help`, `--version` and the rest are GNU
//! AutoGen's libopts, ported as the `autoopts` crate; this file is
//! `uudecode.c` and the table `uudecode-opts.c` was generated from.
//!
//! # Upstream's behaviour, kept
//!
//! Measured against the real program, and ported as found:
//!
//! * A traditional line's length character is trusted: a line shorter than
//!   it claims decodes whatever an earlier, longer line left in the 16 KiB
//!   line buffer. (Bytes no line ever wrote are zero here; upstream's are
//!   whatever its stack held.)
//! * In base64, a line that decodes to nothing -- a blank line, or a group
//!   still incomplete at its end -- is `fwrite` of zero bytes, which fails,
//!   and is reported as a write error. A `\r` before the newline fails the
//!   next line as invalid input.
//! * Only the line right after the zero-length line may be `end`.
//! * `fserr`'s report of a base64 file cut short prints `errno` as it stood
//!   -- usually the `ENOENT` of looking for an output file that did not yet
//!   exist -- since nothing failed.
//! * Several messages end without a newline (`Short file`, `No `end' line`,
//!   `invalid input`), and error(3)'s name the program by its argv\[0\].
//! * Failures OR together across files, except the ones that `die`, which
//!   end the run on the spot with 2.
//!
//! # Where this port departs
//!
//! * A name in a message -- an input file, an output file, a `~user` -- is
//!   [`autoopts::shown`]: upstream's bytes where they are printable, `\012`
//!   and its like where not, so a `begin` line naming `x\nuudecode: ...`
//!   cannot forge a line of stderr. Upstream prints the raw bytes
//!   (design-decisions.md §1033).

use std::env;
use std::fs::{self, File};
use std::io;
use std::process::ExitCode;

use autoopts::{
    Action, Callbacks, Desc, Exit, Options, Program, Role, SHARUTILS_FLAGS, STANDARD_DESCS,
    bytes_os, os_bytes, st,
};
use ulclosestream::Stdout;

mod lines;

use lines::{Lines, Source};

stdfdguard::guard_std_fds!();

/// `INDEX_OPT_OUTPUT_FILE`.
const OUTPUT_FILE: usize = 0;
/// `INDEX_OPT_IGNORE_CHMOD`.
const IGNORE_CHMOD: usize = 1;

/// `UUDECODE_EXIT_SUCCESS`.
const EXIT_SUCCESS: i32 = 0;
/// `UUDECODE_EXIT_INVALID`: the input is not what it should be.
const EXIT_INVALID: i32 = 2;
/// `UUDECODE_EXIT_NO_INPUT`: an input file would not open.
const EXIT_NO_INPUT: i32 = 4;
/// `UUDECODE_EXIT_NO_OUTPUT`: the output could not be written.
const EXIT_NO_OUTPUT: i32 = 8;
/// `UUDECODE_EXIT_USAGE_ERROR`.
const EXIT_USAGE_ERROR: i32 = 64;

/// `2 * BUFSIZ`: the line buffers.
const LINE_BUF: usize = 16384;

/// The option table (`optDesc` in `uudecode-opts.c`).
static DESCS: [Desc; 7] = {
    let [version, help, more_help, save_opts, load_opts] = STANDARD_DESCS;
    [
        Desc {
            value: b'o',
            name: "output-file",
            disable_name: None,
            text: "direct output to file",
            flags: st::DISABLED | st::ARG_STRING,
            min: 0,
            max: 1,
            role: Role::User,
            action: Action::User,
        },
        Desc {
            value: b'c',
            name: "ignore-chmod",
            disable_name: None,
            text: "ignore fchmod(3P) errors",
            flags: st::DISABLED,
            min: 0,
            max: 1,
            role: Role::User,
            action: Action::None,
        },
        version,
        help,
        more_help,
        save_opts,
        load_opts,
    ]
};

/// `uudecodeOptions`, with its texts as `uudecode_opt_strs` holds them.
static PROGRAM: Program = Program {
    name: "uudecode",
    upper: "UUDECODE",
    rc_name: ".sharrc",
    copyright: "uudecode (GNU sharutils) 4.15.2\n\
Copyright (C) 1994-2015 Free Software Foundation, Inc., all rights reserved.\n\
This is free software. It is licensed for use, modification and\n\
redistribution under the terms of the GNU General Public License,\n\
version 3 or later <http://gnu.org/licenses/gpl.html>\n",
    copy_notice: "uudecode is free software: you can redistribute it and/or modify it under\n\
the terms of the GNU General Public License as published by the Free\n\
Software Foundation, either version 3 of the License, or (at your option)\n\
any later version.\n\n\
uudecode is distributed in the hope that it will be useful, but WITHOUT ANY\n\
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS\n\
FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more\n\
details.\n\n\
You should have received a copy of the GNU General Public License along\n\
with this program.  If not, see <http://www.gnu.org/licenses/>.\n",
    full_version: "uudecode (GNU sharutils) 4.15.2",
    home_list: &["$HOME"],
    usage_title: "uudecode (GNU sharutils) - decode an encoded file\n\
Usage:  %s [ -<flag> [<val>] | --<name>[{=| }<val>] ]... [<file>...]\n",
    explain: Some("If no 'file'(s) are provided, then standard input is decoded.\n"),
    detail: Some(DETAIL),
    bug_addr: "bug-gnu-utils@gnu.org",
    descs: &DESCS,
    preset_ct: 2,
    save_opts: 5,
    full_usage: FULL_USAGE,
    short_usage: "uudecode (GNU sharutils) - decode an encoded file\n\
Usage:  uudecode [ -<flag> [<val>] | --<name>[{=| }<val>] ]... [<file>...]\n\
Try 'uudecode --help' for more information.\n",
    proc_flags: SHARUTILS_FLAGS,
    usage_error: EXIT_USAGE_ERROR,
};

/// `zDetail`.
const DETAIL: &str = "'uudecode' transforms uuencoded files into their original form.\n\n\
The encoded file(s) may be specified on the command line, or one may be\n\
read from standard input.  The output file name is specified in the encoded\n\
file, but may be overridden with the '-o' option.  It will have the mode of\n\
the original file, except that setuid and execute bits are not retained.  If\n\
the output file is specified to be '/dev/stdout' or '-', the result will be\n\
written to standard output.  If there are multiple input files and the\n\
second or subsquent file specifies standard output, the decoded data will\n\
be written to the same file as the previous output.  Don't do that.\n\n\
'uudecode' ignores any leading and trailing lines.  It looks for a line\n\
that starts with \"'begin'\" and proceeds until the end-of-encoding marker is\n\
found.  The program determines from the header line of the encoded file\n\
which of the two supported encoding schemes was used and whether or not the\n\
output file name has been encoded with base64 encoding.  See 'uuencode(5)'.\n";

/// `uudecode_full_usage`: what `--help` prints.
const FULL_USAGE: &str = "uudecode (GNU sharutils) - decode an encoded file\n\
Usage:  uudecode [ -<flag> [<val>] | --<name>[{=| }<val>] ]... [<file>...]\n\n\
\x20  -o, --output-file=str      direct output to file\n\
\x20  -c, --ignore-chmod         ignore fchmod(3P) errors\n\
\x20  -v, --version[=MODE]       output version information and exit\n\
\x20  -h, --help                 display extended usage information and exit\n\
\x20  -!, --more-help            extended usage information passed thru pager\n\
\x20  -R, --save-opts[=FILE]     save the option state to a config file FILE\n\
\x20  -r, --load-opts=FILE       load options from the config file FILE\n\
\x20                               - disabled with '--no-load-opts'\n\
\x20                               - may appear multiple times\n\n\
Options are specified by doubled hyphens and their name or by a single\n\
hyphen and the flag character.\n\
If no 'file'(s) are provided, then standard input is decoded.\n\n\
The following option preset mechanisms are supported:\n\
\x20- reading file $HOME/.sharrc\n\n\
'uudecode' transforms uuencoded files into their original form.\n\n\
The encoded file(s) may be specified on the command line, or one may be\n\
read from standard input.  The output file name is specified in the encoded\n\
file, but may be overridden with the '-o' option.  It will have the mode of\n\
the original file, except that setuid and execute bits are not retained.  If\n\
the output file is specified to be '/dev/stdout' or '-', the result will be\n\
written to standard output.  If there are multiple input files and the\n\
second or subsquent file specifies standard output, the decoded data will\n\
be written to the same file as the previous output.  Don't do that.\n\n\
'uudecode' ignores any leading and trailing lines.  It looks for a line\n\
that starts with \"'begin'\" and proceeds until the end-of-encoding marker is\n\
found.  The program determines from the header line of the encoded file\n\
which of the two supported encoding schemes was used and whether or not the\n\
output file name has been encoded with base64 encoding.  See 'uuencode(5)'.\n\n\
Please send bug reports to:  <bug-gnu-utils@gnu.org>\n";

/// `DEC`: one character's six bits.
fn dec(c: u8) -> u32 {
    u32::from(c.wrapping_sub(b' ') & 0o77)
}

/// The low byte of `x`, as `putchar` takes it.
fn low(x: u32) -> u8 {
    x.to_le_bytes()[0]
}

/// `isspace` in the C locale.
fn isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `bytes` and a NUL over the start of `buf`, which is always big enough
/// (they were read from it).
fn store(buf: &mut [u8], bytes: &[u8]) {
    for (slot, &b) in buf.iter_mut().zip(bytes.iter().chain(std::iter::once(&0))) {
        *slot = b;
    }
}

/// The C string at the start of `buf`.
fn cstr(buf: &[u8]) -> &[u8] {
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf.get(..n).unwrap_or_default()
}

/// The `errno` a call that did not fail leaves: whatever the last one that
/// did fail set.
fn errno_now() -> io::Error {
    io::Error::last_os_error()
}

/// `sscanf(scan, " %o %[^\n]", &mode, name)`: the mode and name of a
/// `begin` line, or `None` when fewer than both convert.
fn scan_header(s: &[u8]) -> Option<(u32, Vec<u8>)> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut p = 0usize;
    while isspace(at(p)) {
        p = p.saturating_add(1);
    }
    let negative = at(p) == b'-';
    if matches!(at(p), b'+' | b'-') {
        p = p.saturating_add(1);
    }
    let start = p;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let c @ b'0'..=b'7' = at(p) {
        match value
            .checked_mul(8)
            .and_then(|v| v.checked_add(u64::from(c.wrapping_sub(b'0'))))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
        p = p.saturating_add(1);
    }
    if p == start {
        return None;
    }
    // `strtoul`'s answer, stored through an `unsigned int *`.
    let value = if overflow {
        u64::MAX
    } else if negative {
        value.wrapping_neg()
    } else {
        value
    };
    let mode = u32::try_from(value & 0xffff_ffff).unwrap_or(u32::MAX);
    while isspace(at(p)) {
        p = p.saturating_add(1);
    }
    let name_start = p;
    while !matches!(at(p), 0 | b'\n') {
        p = p.saturating_add(1);
    }
    if p == name_start {
        return None;
    }
    Some((mode, s.get(name_start..p).unwrap_or_default().to_vec()))
}

/// Standard output: descriptor 1 through glibc-shaped buffering, and
/// `freopen`able onto a file.
struct Out {
    s: Stdout,
}

impl Out {
    /// `freopen(name, "w", stdout)`: the old stream flushed (a failure there
    /// is lost, as `freopen` loses it), the file created or truncated, and a
    /// fresh stream on descriptor 1 -- error flag clear, buffer sized for
    /// the new file at its first write.
    fn reopen(&mut self, name: &[u8]) -> io::Result<()> {
        self.s.flush();
        let file = File::create(bytes_os(name))?;
        onto_stdout(file)?;
        self.s = Stdout::new(1);
        Ok(())
    }
}

/// Make `file` descriptor 1.
#[cfg(unix)]
fn onto_stdout(file: File) -> io::Result<()> {
    use std::os::fd::IntoRawFd;
    unsafe extern "C" {
        fn dup2(oldfd: i32, newfd: i32) -> i32;
        fn close(fd: i32) -> i32;
    }
    let fd = file.into_raw_fd();
    if fd == 1 {
        // Descriptor 1 was closed, so the open took its place.
        return Ok(());
    }
    // SAFETY: `fd` is the descriptor just opened and owned by nobody else;
    // `dup2` onto 1 only rearranges the descriptor table, replacing whatever
    // 1 was (stdout, whose buffer was flushed and is dropped by the caller).
    let rc = unsafe { dup2(fd, 1) };
    let err = (rc < 0).then(io::Error::last_os_error);
    // SAFETY: `fd` is ours and closed exactly once; descriptor 1 now holds
    // its own reference to the file.
    unsafe { close(fd) };
    err.map_or(Ok(()), Err)
}

#[cfg(not(unix))]
fn onto_stdout(_file: File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no descriptor 1 to reopen on this host",
    ))
}

/// `fchmod(STDOUT_FILENO, mode)`.
#[cfg_attr(
    not(unix),
    allow(
        clippy::unnecessary_wraps,
        reason = "the signature of the unix function this stands in for"
    )
)]
fn fchmod_stdout(mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::mem::ManuallyDrop;
        use std::os::fd::FromRawFd;
        use std::os::unix::fs::PermissionsExt;
        // SAFETY: descriptor 1 is borrowed for this one call and not closed
        // (`ManuallyDrop`); `set_permissions` on a `File` is `fchmod`.
        let f = ManuallyDrop::new(unsafe { File::from_raw_fd(1) });
        f.set_permissions(fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        Ok(())
    }
}

/// `access(name, F_OK) == 0`.
fn exists(name: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        unsafe extern "C" {
            fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
        }
        let Ok(c) = CString::new(name.to_vec()) else {
            return false;
        };
        // SAFETY: `c` is a valid NUL-terminated string that outlives the
        // call; `access` only reads it.
        unsafe { access(c.as_ptr(), 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        fs::metadata(bytes_os(name)).is_ok()
    }
}

/// uudecode's own procedure for `-o` (`doOptOutput_File`): unless the name
/// is `-` or `/dev/stdout`, standard output is reopened on it now.
struct OutputOption<'a> {
    out: &'a mut Out,
}

impl Callbacks for OutputOption<'_> {
    fn option(&mut self, opts: &Options, index: usize) -> Result<(), Exit> {
        if index != OUTPUT_FILE {
            return Ok(());
        }
        let name = opts.arg(OUTPUT_FILE).unwrap_or_default().to_vec();
        if name == b"-" || name == b"/dev/stdout" {
            return Ok(());
        }
        self.out
            .reopen(&name)
            .map_err(|e| opts.fserr(EXIT_NO_OUTPUT, "freopen-ing for stdout", &name, &e))
    }
}

/// One run: the options, the output stream, and how the program names
/// itself in error(3)'s messages.
struct Run<'a> {
    opts: &'a Options,
    out: &'a mut Out,
    /// `program_invocation_name`: argv\[0\] as given.
    argv0: Vec<u8>,
}

impl Run<'_> {
    /// error(3): stdout flushed first, then `ARGV0: MESSAGE[: REASON]`.
    fn error(&mut self, err: Option<&io::Error>, msg: &[u8]) {
        self.out.s.flush();
        let mut line = self.argv0.clone();
        line.extend_from_slice(b": ");
        line.extend_from_slice(msg);
        if let Some(e) = err {
            line.extend_from_slice(b": ");
            line.extend_from_slice(errmsg::strerror(e).as_bytes());
        }
        line.push(b'\n');
        ulclosestream::stderr_write(&line);
    }

    /// `die(UUDECODE_EXIT_INVALID, ...)`.
    fn invalid(&self, parts: &[&[u8]]) -> Exit {
        self.opts.die(EXIT_INVALID, &parts.concat())
    }

    /// `decode`: one input, from its `begin` line to its end.
    fn decode(&mut self, inname: &[u8], input: &mut Lines) -> Result<i32, Exit> {
        let mut buf = vec![0u8; LINE_BUF];
        let bad_beginning =
            |r: &Run<'_>| r.invalid(&[&autoopts::shown(inname), b": Invalid or missing 'begin' line\n"]);
        let (mode, do_base64, encoded) = loop {
            if input.fgets(&mut buf).is_none() {
                return Err(bad_beginning(self));
            }
            let line = cstr(&buf);
            if !line.contains(&b'\n') {
                return Err(bad_beginning(self));
            }
            if !line.starts_with(b"begin") {
                continue;
            }
            let at = |i: usize| line.get(i).copied().unwrap_or(0);
            let mut scan = 5usize;
            let mut do_base64 = false;
            let mut encoded = false;
            loop {
                match at(scan) {
                    b' ' => break,
                    b'-' => {
                        scan = scan.saturating_add(1);
                        if at(scan) == b'b' {
                            let rest = line.get(scan.saturating_add(1)..).unwrap_or_default();
                            if !rest.starts_with(b"ase64") || do_base64 {
                                return Err(bad_beginning(self));
                            }
                            do_base64 = true;
                            scan = scan.saturating_add(6);
                        } else {
                            let rest = line.get(scan..).unwrap_or_default();
                            if !rest.starts_with(b"encoded") || encoded {
                                return Err(bad_beginning(self));
                            }
                            encoded = true;
                            scan = scan.saturating_add(7);
                        }
                    }
                    _ => return Err(bad_beginning(self)),
                }
            }
            match scan_header(line.get(scan..).unwrap_or_default()) {
                Some((mode, name)) => {
                    // `%[^\n]` stores the name, and its NUL, over the start
                    // of the line it was read from; the rest of the buffer
                    // keeps what the header search left there.
                    store(&mut buf, &name);
                    break (mode, do_base64, encoded);
                }
                None => return Err(bad_beginning(self)),
            }
        };

        let outname = if self.opts.have(OUTPUT_FILE) {
            self.opts.arg(OUTPUT_FILE).unwrap_or_default().to_vec()
        } else {
            if encoded {
                self.decode_fname(&mut buf)?;
            }
            let mut outname = if buf.first() == Some(&b'~') {
                match self.expand_tilde(&mut buf)? {
                    Some(n) => n,
                    None => return Ok(EXIT_NO_OUTPUT),
                }
            } else {
                cstr(&buf).to_vec()
            };
            while outname.last().copied().is_some_and(isspace) {
                outname.pop();
            }
            outname
        };

        if outname != b"/dev/stdout" && outname != b"-" {
            let rval = self.reopen_output(&outname, mode)?;
            if rval != EXIT_SUCCESS {
                return Ok(rval);
            }
        }
        let rval = if do_base64 {
            self.read_base64(inname, &outname, input)?
        } else {
            self.read_stduu(inname, &outname, input)?
        };
        if rval == EXIT_SUCCESS {
            let failed = self.out.s.error().is_some() || {
                self.out.s.flush();
                self.out.s.error().is_some()
            };
            if failed {
                let mut msg = autoopts::shown(&outname);
                msg.extend_from_slice(b": Write error");
                self.error(None, &msg);
                return Ok(EXIT_NO_OUTPUT);
            }
        }
        Ok(rval)
    }

    /// `decode_fname`: the `-encoded` name at the start of `buf`,
    /// base64-decoded in place -- the decoded bytes and a NUL over the start,
    /// the rest of the buffer as it was.
    fn decode_fname(&self, buf: &mut [u8]) -> Result<(), Exit> {
        let name = cstr(buf).to_vec();
        if name.is_empty() {
            return Err(self.invalid(&[b"output name is empty"]));
        }
        let mut decoded = Vec::new();
        let ok = gnubase64::decode_ctx(
            None,
            &name,
            &mut gnubase64::Out {
                buf: &mut decoded,
                left: name.len(),
            },
        );
        if !ok {
            return Err(self.invalid(&[b"invalid base64 encoded name: ", &autoopts::shown(&name)]));
        }
        store(buf, &decoded);
        Ok(())
    }

    /// `expand_tilde`: `~/rest` under `$HOME`, `~user/rest` under that
    /// user's home. `None` (having said why) for a user who does not exist.
    ///
    /// Upstream looks for the `/` after `~user` with no stop at the name's
    /// end: the scan runs on through the header buffer, over whatever the
    /// lines before the `begin` line left there, and a `/` found there ends
    /// the user name (with a NUL written over it) and starts the rest. So a
    /// `~user` with no slash is `No user 'user'` when there is no such user,
    /// however far the scan went -- and its "Illegal file name" message can
    /// never print. With no `/` anywhere in the buffer upstream scans on into
    /// its stack, and for a user who does exist the rest of the name is
    /// whatever it finds; here it is empty.
    fn expand_tilde(&mut self, buf: &mut [u8]) -> Result<Option<Vec<u8>>, Exit> {
        let (home, rest) = if buf.get(1) == Some(&b'/') {
            let Some(home) = env::var_os("HOME") else {
                return Err(self.invalid(&[b"cannot expand $HOME"]));
            };
            (
                os_bytes(&home),
                cstr(buf.get(2..).unwrap_or_default()).to_vec(),
            )
        } else {
            let slash = buf
                .iter()
                .skip(1)
                .position(|&b| b == b'/')
                .map(|p| p.saturating_add(1));
            if let Some(b) = slash.and_then(|k| buf.get_mut(k)) {
                *b = 0;
            }
            let user = cstr(buf.get(1..).unwrap_or_default()).to_vec();
            let db = pwdb::Db::load();
            let Some(pw) = db.user_by_name(&user) else {
                let mut msg = b"No user '".to_vec();
                msg.extend_from_slice(&autoopts::shown(&user));
                msg.push(b'\'');
                self.error(None, &msg);
                return Ok(None);
            };
            let rest = slash
                .and_then(|k| buf.get(k.saturating_add(1)..))
                .map(|r| cstr(r).to_vec())
                .unwrap_or_default();
            (pw.dir.clone(), rest)
        };
        let mut out = home;
        out.push(b'/');
        out.extend_from_slice(&rest);
        Ok(Some(out))
    }

    /// `reopen_output`: create the output file and give it the header's
    /// mode (less set-id bits). A failed chmod is reported, and fatal to
    /// this file unless `-c` or `POSIXLY_CORRECT` says it is not.
    fn reopen_output(&mut self, outname: &[u8], mode: u32) -> Result<i32, Exit> {
        if exists(outname)
            && let Err(e) = fs::symlink_metadata(bytes_os(outname))
        {
            let mut msg = b"cannot access ".to_vec();
            msg.extend_from_slice(&autoopts::shown(outname));
            self.error(Some(&e), &msg);
            return Ok(EXIT_NO_OUTPUT);
        }
        if let Err(e) = self.out.reopen(outname) {
            return Err(self.opts.fserr(EXIT_NO_OUTPUT, "freopen", outname, &e));
        }
        if let Err(e) = fchmod_stdout(mode & 0o777) {
            let mut msg = b"chmod of ".to_vec();
            msg.extend_from_slice(&autoopts::shown(outname));
            self.error(Some(&e), &msg);
            if !self.opts.have(IGNORE_CHMOD) && env::var_os("POSIXLY_CORRECT").is_none() {
                return Ok(EXIT_NO_OUTPUT);
            }
        }
        Ok(EXIT_SUCCESS)
    }

    /// Write decoded bytes; the first failure is `fserr` naming `op`.
    fn put(&mut self, data: &[u8], op: &str, outname: &[u8]) -> Result<(), Exit> {
        let failed_before = self.out.s.error().is_some();
        self.out.s.write(data);
        match self.out.s.error() {
            Some(e) if !failed_before => Err(self.opts.fserr(EXIT_NO_OUTPUT, op, outname, e)),
            _ => Ok(()),
        }
    }

    /// `read_stduu`: traditional lines, each decoded as long as its first
    /// character says, until one says zero; then the `end` line.
    fn read_stduu(
        &mut self,
        inname: &[u8],
        outname: &[u8],
        input: &mut Lines,
    ) -> Result<i32, Exit> {
        let mut buf = vec![0u8; LINE_BUF];
        let at = |buf: &[u8], i: usize| dec(buf.get(i).copied().unwrap_or(0));
        loop {
            if input.fgets(&mut buf).is_none() {
                return Err(self.invalid(&[&autoopts::shown(inname), b": Short file"]));
            }
            let mut n = at(&buf, 0);
            if n == 0 {
                break;
            }
            let mut p = 1usize;
            let mut bytes = Vec::with_capacity(64);
            while n >= 3 {
                let (a, b, c, d) = (
                    at(&buf, p),
                    at(&buf, p.saturating_add(1)),
                    at(&buf, p.saturating_add(2)),
                    at(&buf, p.saturating_add(3)),
                );
                bytes.push(low((a << 2) | (b >> 4)));
                bytes.push(low((b << 4) | (c >> 2)));
                bytes.push(low((c << 6) | d));
                p = p.saturating_add(4);
                n = n.saturating_sub(3);
            }
            let (a, b, c) = (
                at(&buf, p),
                at(&buf, p.saturating_add(1)),
                at(&buf, p.saturating_add(2)),
            );
            if n >= 1 {
                bytes.push(low((a << 2) | (b >> 4)));
            }
            if n == 2 {
                bytes.push(low((b << 4) | (c >> 2)));
            }
            // One `putchar` per byte upstream; batched, the failure still
            // falls on this line and is still `putchar`'s.
            self.put(&bytes, "putchar", outname)?;
        }
        if input.fgets(&mut buf).is_some()
            && buf.starts_with(b"end")
            && (buf.get(3) == Some(&b'\n')
                || (buf.get(3) == Some(&b'\r') && buf.get(4) == Some(&b'\n')))
        {
            return Ok(EXIT_SUCCESS);
        }
        Err(self.invalid(&[&autoopts::shown(inname), b": No `end' line"]))
    }

    /// `read_base64`: lines through gnulib's decoder until one starts with
    /// `====`.
    fn read_base64(
        &mut self,
        inname: &[u8],
        outname: &[u8],
        input: &mut Lines,
    ) -> Result<i32, Exit> {
        let mut buf = vec![0u8; LINE_BUF];
        let mut ctx = gnubase64::Ctx::default();
        loop {
            if input.fgets(&mut buf).is_none() {
                // `fserr` with the op "%s: Short file", `%s` and all.
                return Err(self
                    .opts
                    .fserr(EXIT_INVALID, "%s: Short file", inname, &errno_now()));
            }
            if buf.starts_with(b"====") {
                break;
            }
            let mut decoded = Vec::new();
            let ok = gnubase64::decode_ctx(
                Some(&mut ctx),
                cstr(&buf),
                &mut gnubase64::Out {
                    buf: &mut decoded,
                    left: LINE_BUF,
                },
            );
            if !ok {
                return Err(self.invalid(&[&autoopts::shown(inname), b": invalid input"]));
            }
            if decoded.is_empty() {
                // `fwrite` of zero bytes returns 0, which is not 1.
                return Err(self
                    .opts
                    .fserr(EXIT_NO_OUTPUT, "fwrite", outname, &errno_now()));
            }
            self.put(&decoded, "fwrite", outname)?;
        }
        Ok(EXIT_SUCCESS)
    }
}

/// `main` less the exit: the status.
fn run(args: Vec<Vec<u8>>, out: &mut Out) -> Result<i32, Exit> {
    let argv0 = args.first().cloned().unwrap_or_default();
    let mut opts = Options::new(&PROGRAM, args);
    let first = opts.process(&mut OutputOption { out })?;
    let files: Vec<Vec<u8>> = opts.args().get(first..).unwrap_or_default().to_vec();
    if files.len() > 1 && opts.have(OUTPUT_FILE) {
        return Err(opts.usage_message(
            b"You cannot specify an output file when processing\nmultiple input files.\n",
        ));
    }
    let mut run = Run {
        opts: &opts,
        out,
        argv0,
    };
    if files.is_empty() {
        let mut input = Lines::new(Source::Stdin);
        return run.decode(b"standard input", &mut input);
    }
    let mut status = EXIT_SUCCESS;
    for f in &files {
        match File::open(bytes_os(f)) {
            Ok(file) => {
                let mut input = Lines::new(Source::File(file));
                status |= run.decode(f, &mut input)?;
            }
            Err(e) => {
                run.error(Some(&e), &autoopts::shown(f));
                status |= EXIT_NO_INPUT;
            }
        }
    }
    Ok(status)
}

fn main() -> ExitCode {
    stdfdguard::restore();
    let args: Vec<Vec<u8>> = env::args_os().map(|a| os_bytes(&a)).collect();
    let mut out = Out { s: Stdout::new(1) };
    let status = match run(args, &mut out) {
        Ok(s) => Exit(s).status(),
        Err(e) => e.status(),
    };
    // `exit`: glibc flushes what is still held, and reports nothing.
    out.s.flush_at_exit();
    ExitCode::from(status)
}

#[cfg(test)]
mod tests;
