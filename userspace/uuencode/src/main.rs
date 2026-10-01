//! `uuencode` -- GNU sharutils 4.15.2's, checked against Ubuntu 24.04's build
//! of it by `scripts/uu-diff.sh`.
//!
//! ```text
//! uuencode [-m] [-e] [IN-FILE] OUTPUT-NAME
//! ```
//!
//! Writes IN-FILE (or standard input) as lines of printable text: a
//! `begin MODE NAME` header, then 45 input bytes per line -- in the
//! traditional encoding, a length character and four characters per three
//! bytes from the alphabet that starts with a backquote; with `-m`, 60
//! characters of base64 -- then `` ` `` and `end` (or `====`). MODE is the
//! input file's permission bits, or `0666` less the umask for standard input.
//! `-e` writes the name itself in base64, marked by `-encoded` in the header.
//!
//! The options, `~/.sharrc`, `--help`, `--version` and the rest are GNU
//! AutoGen's libopts, ported as the `autoopts` crate; this file is
//! `uuencode.c` and the table `uuencode-opts.c` was generated from.
//!
//! Every failure is one `uuencode fatal error:` message naming the C library
//! call that failed -- `fread` on the input, and on standard output whichever
//! of `putchar`, `fwrite`, `puts` or `fclose` was the call that met the error.
//! Which one that is depends on where glibc's buffer filled, so stdout is
//! buffered exactly as glibc buffers it (`ulclosestream`).

use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;

use autoopts::{
    Action, Desc, Exit, NoCallbacks, Options, Program, Role, SHARUTILS_FLAGS, STANDARD_DESCS,
    bytes_os, os_bytes, st,
};
use ulclosestream::Stdout;

stdfdguard::guard_std_fds!();

/// `INDEX_OPT_BASE64`.
const BASE64: usize = 0;
/// `INDEX_OPT_ENCODE_FILE_NAME`.
const ENCODE_FILE_NAME: usize = 1;

/// `UUENCODE_EXIT_FAILURE`.
const EXIT_FAILURE: i32 = 1;
/// `UUENCODE_EXIT_USAGE_ERROR`.
const EXIT_USAGE_ERROR: i32 = 64;

/// The option table (`optDesc` in `uuencode-opts.c`).
static DESCS: [Desc; 7] = {
    let [version, help, more_help, save_opts, load_opts] = STANDARD_DESCS;
    [
        Desc {
            value: b'm',
            name: "base64",
            disable_name: None,
            text: "convert using base64",
            flags: st::DISABLED,
            min: 0,
            max: 1,
            role: Role::User,
            action: Action::None,
        },
        Desc {
            value: b'e',
            name: "encode-file-name",
            disable_name: None,
            text: "encode the output file name",
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

/// `uuencodeOptions`, with its texts as `uuencode_opt_strs` holds them.
static PROGRAM: Program = Program {
    name: "uuencode",
    upper: "UUENCODE",
    rc_name: ".sharrc",
    copyright: "uuencode (GNU sharutils) 4.15.2\n\
Copyright (C) 1994-2015 Free Software Foundation, Inc., all rights reserved.\n\
This is free software. It is licensed for use, modification and\n\
redistribution under the terms of the GNU General Public License,\n\
version 3 or later <http://gnu.org/licenses/gpl.html>\n",
    copy_notice: "uuencode is free software: you can redistribute it and/or modify it under\n\
the terms of the GNU General Public License as published by the Free\n\
Software Foundation, either version 3 of the License, or (at your option)\n\
any later version.\n\n\
uuencode is distributed in the hope that it will be useful, but WITHOUT ANY\n\
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS\n\
FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more\n\
details.\n\n\
You should have received a copy of the GNU General Public License along\n\
with this program.  If not, see <http://www.gnu.org/licenses/>.\n",
    full_version: "uuencode (GNU sharutils) 4.15.2",
    home_list: &["$HOME"],
    usage_title: "uuencode (GNU sharutils) - encode a file into email friendly text\n\
Usage:  %s [ -<flag> | --<name> ]... [<in-file>] <output-name>\n",
    explain: None,
    detail: Some(DETAIL),
    bug_addr: "bug-gnu-utils@gnu.org",
    descs: &DESCS,
    preset_ct: 2,
    save_opts: 5,
    full_usage: FULL_USAGE,
    short_usage: "uuencode (GNU sharutils) - encode a file into email friendly text\n\
Usage:  uuencode [ -<flag> | --<name> ]... [<in-file>] <output-name>\n\
Try 'uuencode --help' for more information.\n",
    proc_flags: SHARUTILS_FLAGS,
    usage_error: EXIT_USAGE_ERROR,
};

/// `zDetail`.
const DETAIL: &str = "'uuencode' is used to create an ASCII representation of a file that can be\n\
sent over channels that may otherwise corrupt the data.  Specifically,\n\
email cannot handle binary data and will often even insert a character when\n\
the six character sequence \"\\nFrom \" is seen.\n\n\
'uuencode' will read 'in-file' if provided and otherwise read data from\n\
standard in and write the encoded form to standard out.  The output will\n\
begin with a header line for use by 'uudecode' giving it the resulting\n\
suggested file 'output-name' and access mode.  If the 'output-name' is\n\
specifically '/dev/stdout', then 'uudecode' will emit the decoded file to\n\
standard out.\n\n\
'Note': 'uuencode' uses buffered input and assumes that it is not hand\n\
typed from a tty.  The consequence is that at a tty, you may need to hit\n\
Ctl-D several times to terminate input.\n";

/// `uuencode_full_usage`: what `--help` prints.
const FULL_USAGE: &str = "uuencode (GNU sharutils) - encode a file into email friendly text\n\
Usage:  uuencode [ -<flag> | --<name> ]... [<in-file>] <output-name>\n\n\
\x20  -m, --base64               convert using base64\n\
\x20  -e, --encode-file-name     encode the output file name\n\
\x20  -v, --version[=MODE]       output version information and exit\n\
\x20  -h, --help                 display extended usage information and exit\n\
\x20  -!, --more-help            extended usage information passed thru pager\n\
\x20  -R, --save-opts[=FILE]     save the option state to a config file FILE\n\
\x20  -r, --load-opts=FILE       load options from the config file FILE\n\
\x20                               - disabled with '--no-load-opts'\n\
\x20                               - may appear multiple times\n\n\
Options are specified by doubled hyphens and their name or by a single\n\
hyphen and the flag character.\n\n\
The following option preset mechanisms are supported:\n\
\x20- reading file $HOME/.sharrc\n\n\
'uuencode' is used to create an ASCII representation of a file that can be\n\
sent over channels that may otherwise corrupt the data.  Specifically,\n\
email cannot handle binary data and will often even insert a character when\n\
the six character sequence \"\\nFrom \" is seen.\n\n\
'uuencode' will read 'in-file' if provided and otherwise read data from\n\
standard in and write the encoded form to standard out.  The output will\n\
begin with a header line for use by 'uudecode' giving it the resulting\n\
suggested file 'output-name' and access mode.  If the 'output-name' is\n\
specifically '/dev/stdout', then 'uudecode' will emit the decoded file to\n\
standard out.\n\n\
'Note': 'uuencode' uses buffered input and assumes that it is not hand\n\
typed from a tty.  The consequence is that at a tty, you may need to hit\n\
Ctl-D several times to terminate input.\n\n\
Please send bug reports to:  <bug-gnu-utils@gnu.org>\n";

/// `uu_std`: the traditional alphabet. Zero is a backquote rather than a
/// space, so that a line's trailing zero groups survive mail transports that
/// strip blanks at line ends.
const UU_STD: &[u8; 64] = b"`!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_";

/// `ENC`: one six-bit group as a character.
fn enc(c: u8) -> u8 {
    UU_STD.get(usize::from(c & 0o77)).copied().unwrap_or(b'`')
}

/// `encode_block`: `input` uuencoded onto `out`, a short final group padded
/// with zero bits.
fn encode_block(out: &mut Vec<u8>, input: &[u8]) {
    for g in input.chunks(3) {
        let a = g.first().copied().unwrap_or(0);
        // `lc`: the second byte of a short group, or zero.
        let b = g.get(1).copied().unwrap_or(0);
        out.push(enc(a >> 2));
        out.push(enc(((a & 0x03) << 4) | ((b >> 4) & 0x0f)));
        if let Some(&c) = g.get(2) {
            out.push(enc(((b & 0x0f) << 2) | ((c >> 6) & 0x03)));
            out.push(enc(c & 0x3f));
        } else {
            out.push(enc((b & 0x0f) << 2));
            out.push(enc(0));
        }
    }
}

/// `umask(IRWALL_MODE)`, as upstream calls it: sets the mask to 0666 and
/// returns the old one. Nothing is created afterwards, so the new mask is
/// never seen.
fn umask_0666() -> u32 {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn umask(mask: u32) -> u32;
        }
        // SAFETY: `umask` takes and returns a plain integer and cannot fail.
        unsafe { umask(0o666) }
    }
    #[cfg(not(unix))]
    {
        0o022
    }
}

/// Where the data comes from: the named file, or descriptor 0 read
/// directly (Rust's `Stdin` reports a closed descriptor as end of file,
/// which upstream's `fread` does not).
enum Input {
    /// `freopen(IN-FILE, "r", stdin)`.
    File(File),
    /// Standard input as it was.
    Stdin,
}

impl Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Input::File(f) => f.read(buf),
            Input::Stdin => read_fd0(buf),
        }
    }
}

#[cfg(unix)]
fn read_fd0(buf: &mut [u8]) -> io::Result<usize> {
    use std::mem::ManuallyDrop;
    use std::os::fd::FromRawFd;
    // SAFETY: descriptor 0 is borrowed for this one read and never closed
    // here -- `ManuallyDrop` keeps the `File` from closing it. A closed
    // descriptor 0 is a defined `EBADF` from `read`, which is the point.
    let mut f = ManuallyDrop::new(unsafe { File::from_raw_fd(0) });
    f.read(buf)
}

#[cfg(not(unix))]
fn read_fd0(buf: &mut [u8]) -> io::Result<usize> {
    io::stdin().read(buf)
}

/// `fread(buf, 1, 45, stdin)`, with stdio's sticky end-of-file and error
/// flags.
struct Reader {
    input: Input,
    eof: bool,
    error: Option<io::Error>,
}

impl Reader {
    fn fill(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0usize;
        while n < buf.len() && !self.eof && self.error.is_none() {
            match self.input.read(buf.get_mut(n..).unwrap_or_default()) {
                Ok(0) => self.eof = true,
                Ok(k) => n = n.saturating_add(k),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => self.error = Some(e),
            }
        }
        n
    }
}

/// `fserr(UUENCODE_EXIT_FAILURE, op, name)`.
fn fserr(opts: &Options, op: &str, name: &[u8], err: &io::Error) -> Exit {
    opts.fserr(EXIT_FAILURE, op, name, err)
}

/// One stdio call's output: into the buffer, and if this call is the one
/// whose flush fails, `fserr` naming it.
fn emit(opts: &Options, out: &mut Stdout, data: &[u8], op: &str) -> Result<(), Exit> {
    let failed_before = out.error().is_some();
    out.write(data);
    match out.error() {
        Some(e) if !failed_before => Err(fserr(opts, op, b"standard output", e)),
        _ => Ok(()),
    }
}

/// `main` less the exit: the status.
fn run(args: Vec<Vec<u8>>, out: &mut Stdout) -> Result<(), Exit> {
    let mut opts = Options::new(&PROGRAM, args);
    let first = opts.process(&mut NoCallbacks)?;
    let operands: Vec<Vec<u8>> = opts.args().get(first..).unwrap_or_default().to_vec();
    let (input, input_name, mode, mut output_name) = match operands.as_slice() {
        [file, name] => {
            let f = File::open(bytes_os(file))
                .map_err(|e| fserr(&opts, "freopen of stdin", file, &e))?;
            let meta = f.metadata().map_err(|e| fserr(&opts, "fstat", file, &e))?;
            (Input::File(f), file.clone(), file_mode(&meta), name.clone())
        }
        [name] => {
            let old = umask_0666();
            (
                Input::Stdin,
                b"standard input".to_vec(),
                0o666 & !old,
                name.clone(),
            )
        }
        _ => return Err(opts.usage(EXIT_USAGE_ERROR)),
    };
    let b64 = opts.have(BASE64);
    let encoded = opts.have(ENCODE_FILE_NAME);
    if encoded {
        output_name = gnubase64::encode(&output_name);
    }

    let mut header = format!(
        "begin{}{} {mode:o} ",
        if b64 { "-base64" } else { "" },
        if encoded { "-encoded" } else { "" }
    )
    .into_bytes();
    header.extend_from_slice(&output_name);
    header.push(b'\n');
    emit(&opts, out, &header, "printf")?;

    let mut reader = Reader {
        input,
        eof: false,
        error: None,
    };
    loop {
        let mut buf = [0u8; 45];
        let rdct = reader.fill(&mut buf);
        if rdct == 0 {
            if let Some(e) = &reader.error {
                return Err(fserr(&opts, "fread", &input_name, e));
            }
            break;
        }
        let finishing = rdct < buf.len();
        if finishing && !reader.eof {
            let e = reader
                .error
                .take()
                .unwrap_or_else(|| io::Error::from_raw_os_error(0));
            return Err(fserr(&opts, "fread", &input_name, &e));
        }
        let data = buf.get(..rdct).unwrap_or_default();
        let mut line = Vec::with_capacity(64);
        if b64 {
            line = gnubase64::encode(data);
        } else {
            emit(
                &opts,
                out,
                &[enc(u8::try_from(rdct).unwrap_or(0))],
                "putchar",
            )?;
            encode_block(&mut line, data);
        }
        line.push(b'\n');
        emit(&opts, out, &line, "fwrite")?;
        if finishing {
            break;
        }
    }
    if !b64 {
        emit(&opts, out, &[enc(0)], "putchar")?;
        emit(&opts, out, b"\n", "putchar")?;
    }
    if let Some(e) = out.error() {
        return Err(fserr(&opts, "ferror", b"standard output", e));
    }
    emit(&opts, out, if b64 { b"====\n" } else { b"end\n" }, "puts")?;
    out.flush();
    if let Some(e) = out.error() {
        return Err(fserr(&opts, "fclose", b"standard output", e));
    }
    Ok(())
}

/// `st_mode & 0777`.
fn file_mode(meta: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        if meta.permissions().readonly() {
            0o444
        } else {
            0o644
        }
    }
}

fn main() -> ExitCode {
    stdfdguard::restore();
    let args: Vec<Vec<u8>> = env::args_os().map(|a| os_bytes(&a)).collect();
    let mut out = Stdout::new(1);
    let status = match run(args, &mut out) {
        Ok(()) => 0,
        Err(e) => e.status(),
    };
    // `exit`: glibc flushes what is still held, and reports nothing.
    out.flush_at_exit();
    ExitCode::from(status)
}

#[cfg(test)]
mod tests;
