//! libmagic's `compress.c`: looking inside compressed files (`-z`, `-Z`), and
//! the careful reads and the pipe-to-file copy the other readers share.
//!
//! The decompressors are upstream's, as a build of file 5.45 with zlib and
//! no other compression library has them: gzip and zlib streams are inflated
//! here ([`crate::zlib`], zlib's semantics), and everything else -- compress,
//! bzip2, xz, lzip, zstd and the rest -- is handed to the program upstream
//! names (`bzip2 -cd`, ...), found on `PATH`, run with an empty environment
//! as `posix_spawnp` runs it, its output read back. A program that is not
//! there gives upstream's answer for that case, `Wait failed, No child
//! processes`, which is what glibc's `posix_spawnp` failure turns into
//! there.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::buffer::Buffer;
use crate::cstd::{cstr, isspace};
use crate::funcs::{Ms, file_buffer};
use crate::magic::{MAGIC_COMPRESS, MAGIC_COMPRESS_TRANSP, MAGIC_MIME, MAGIC_NO_COMPRESS_FORK};
use crate::printf::Arg;

/// How a compression method is recognised: by leading bytes, or by a test of
/// at least this many bytes.
enum Sig {
    Bytes(&'static [u8]),
    Test(fn(&[u8]) -> bool, usize),
}

/// One entry of `compr[]`: how the method is recognised, and the program
/// that undoes it.
struct Compr {
    sig: Sig,
    argv: &'static [&'static str],
}

const GZIP_ARGS: &[&str] = &["gzip", "-cd"];
const UNCOMPRESS_ARGS: &[&str] = &["uncompress", "-c"];
const BZIP2_ARGS: &[&str] = &["bzip2", "-cd"];
const LZIP_ARGS: &[&str] = &["lzip", "-cd"];
const XZ_ARGS: &[&str] = &["xz", "-cd"];
const LRZIP_ARGS: &[&str] = &["lrzip", "-qdf", "-"];
const LZ4_ARGS: &[&str] = &["lz4", "-cd"];
const ZSTD_ARGS: &[&str] = &["zstd", "-cd"];
/// Never run: zlib streams are inflated here.
const ZLIB_ARGS: &[&str] = &[
    "python",
    "-c",
    "import sys, zlib; sys.stdout.write(zlib.decompress(sys.stdin.read()))",
];

/// `compr[]`, in upstream's order: the first two both match `compress`'d
/// data, and both are tried.
const COMPR: [Compr; 15] = [
    Compr { sig: Sig::Bytes(b"\x1f\x9d"), argv: GZIP_ARGS },
    // "Uncompress can get stuck; so use gzip first if we have it."
    Compr { sig: Sig::Bytes(b"\x1f\x9d"), argv: UNCOMPRESS_ARGS },
    Compr { sig: Sig::Bytes(b"\x1f\x8b"), argv: GZIP_ARGS },
    Compr { sig: Sig::Bytes(b"\x1f\x9e"), argv: GZIP_ARGS },
    Compr { sig: Sig::Bytes(b"\x1f\xa0"), argv: GZIP_ARGS },
    Compr { sig: Sig::Bytes(b"\x1f\x1e"), argv: GZIP_ARGS },
    Compr { sig: Sig::Bytes(b"PK\x03\x04"), argv: GZIP_ARGS },
    Compr { sig: Sig::Bytes(b"BZh"), argv: BZIP2_ARGS },
    Compr { sig: Sig::Bytes(b"LZIP"), argv: LZIP_ARGS },
    Compr { sig: Sig::Bytes(b"\xfd7zXZ\x00"), argv: XZ_ARGS },
    Compr { sig: Sig::Bytes(b"LRZI"), argv: LRZIP_ARGS },
    Compr { sig: Sig::Bytes(b"\x04\x22\x4d\x18"), argv: LZ4_ARGS },
    Compr { sig: Sig::Bytes(b"\x28\xb5\x2f\xfd"), argv: ZSTD_ARGS },
    Compr { sig: Sig::Test(lzmacmp, 13), argv: XZ_ARGS },
    Compr { sig: Sig::Test(zlibcmp, 2), argv: ZLIB_ARGS },
];

/// `METH_FROZEN` (the gzip entry, despite the name) and `METH_ZLIB`: the
/// two decompressed here.
const METH_FROZEN: usize = 2;
const METH_ZLIB: usize = 14;

/// `zlibcmp`: a zlib header -- deflate, a window of at most 32K, and the
/// check that makes the first two bytes a multiple of 31.
fn zlibcmp(buf: &[u8]) -> bool {
    let b0 = buf.first().copied().unwrap_or(0);
    let b1 = buf.get(1).copied().unwrap_or(0);
    if b0 & 0xf != 8 || b0 & 0x80 != 0 {
        return false;
    }
    (u16::from(b0) << 8 | u16::from(b1)) % 31 == 0
}

/// `lzmacmp`: an LZMA-alone header.
fn lzmacmp(buf: &[u8]) -> bool {
    let at = |i: usize| buf.get(i).copied().unwrap_or(0);
    if at(0) != 0x5d || at(1) != 0 || at(2) != 0 {
        return false;
    }
    !(at(12) != 0 && at(12) != 0xff)
}

/// What `uncompressbuf` came to: `OKDATA` and the bytes, `ERRDATA` and a
/// message, or `NODATA`.
enum Data {
    Ok(Vec<u8>),
    Err(Vec<u8>),
}

/// `makeerror`.
fn makeerror(msg: impl AsRef<[u8]>) -> Data {
    Data::Err(msg.as_ref().to_vec())
}

/// `methodname`: zlib for the two inflated here, the program otherwise.
fn methodname(method: usize) -> &'static str {
    match method {
        METH_FROZEN | METH_ZLIB => "zlib",
        _ => COMPR.get(method).and_then(|c| c.argv.first()).copied().unwrap_or(""),
    }
}

/// `format_decompression_error`.
fn format_decompression_error(ms: &mut Ms, method: usize, msg: &[u8]) -> i32 {
    let msg = cstr(msg);
    let name = methodname(method).as_bytes();
    if ms.flags & MAGIC_MIME == 0 {
        return ms.printf(b"ERROR:[%s: %s]", &[Arg::Str(name), Arg::Str(msg)]);
    }
    let dashed: Vec<u8> = msg.iter().map(|&c| if c.is_ascii_alphanumeric() { c } else { b'-' }).collect();
    ms.printf(
        b"application/x-decompression-error-%s-%s",
        &[Arg::Str(name), Arg::Str(&dashed)],
    )
}

/// `file_zmagic`: when the file is compressed in a way `compr[]` knows,
/// describe what it decompresses to, then (unless `-Z`, or with only one of
/// the MIME parts) the compressed file itself in parentheses.
pub fn file_zmagic(ms: &mut Ms, b: &Buffer<'_>, name: Option<&[u8]>) -> i32 {
    if ms.flags & MAGIC_COMPRESS == 0 {
        return 0;
    }
    let mime = ms.flags & MAGIC_MIME;
    let buf = b.fbuf;
    let nbytes = buf.len();
    let mut rv = 0;
    // SIGPIPE: a Rust program ignores it already, which is what upstream
    // arranges for the length of this function.
    'methods: for (i, c) in COMPR.iter().enumerate() {
        let zm = match c.sig {
            Sig::Bytes(m) => nbytes >= m.len() && buf.starts_with(m),
            Sig::Test(f, len) => nbytes >= len && f(buf),
        };
        if !zm {
            continue;
        }
        let urv = uncompressbuf(b.fd, ms.bytes_max, i, ms.flags & MAGIC_NO_COMPRESS_FORK != 0, buf);
        ms.flags &= !MAGIC_COMPRESS;
        let prv = match &urv {
            Data::Err(msg) => format_decompression_error(ms, i, msg),
            Data::Ok(newbuf) => file_buffer(ms, None, None, name, newbuf),
        };
        if prv == -1 {
            rv = -1;
            continue;
        }
        rv = 1;
        if ms.flags & MAGIC_COMPRESS_TRANSP != 0 {
            break 'methods;
        }
        if mime != MAGIC_MIME && mime != 0 {
            break 'methods;
        }
        let open: &[u8] = if mime != 0 { b" compressed-encoding=" } else { b" (" };
        if ms.print_str(open) == -1 {
            rv = -1;
            continue;
        }
        let Some(pb) = ms.push_buffer() else {
            rv = -1;
            continue;
        };
        // "XXX: If file_buffer fails here, we overwrite the compressed
        // text. FIXME."
        if file_buffer(ms, None, None, None, buf) == -1 {
            if ms.pop_buffer(pb).is_some() {
                // Upstream aborts: a failure that left no error behind.
                std::process::abort();
            }
            rv = -1;
            continue;
        }
        if let Some(rbuf) = ms.pop_buffer(pb) {
            if ms.print_str(cstr(&rbuf)) == -1 {
                rv = -1;
                continue;
            }
        }
        if mime == 0 && ms.print_str(b")") == -1 {
            rv = -1;
        }
    }
    ms.flags |= MAGIC_COMPRESS;
    rv
}

/// The gzip header's flags.
const FHCRC: u8 = 1 << 1;
const FEXTRA: u8 = 1 << 2;
const FNAME: u8 = 1 << 3;
const FCOMMENT: u8 = 1 << 4;

/// `uncompressgzipped`: past the gzip header, then raw DEFLATE.
fn uncompressgzipped(old: &[u8], bytes_max: usize) -> Data {
    let n = old.len();
    let at = |i: usize| usize::from(old.get(i).copied().unwrap_or(0));
    if n < 4 {
        return makeerror("File too short");
    }
    let flg = old.get(3).copied().unwrap_or(0);
    let mut data_start = 10usize;
    if flg & FEXTRA != 0 {
        if data_start + 1 >= n {
            return makeerror("File too short");
        }
        data_start += 2 + at(data_start) + at(data_start + 1) * 256;
    }
    if flg & FNAME != 0 {
        while data_start < n && at(data_start) != 0 {
            data_start += 1;
        }
        data_start += 1;
    }
    if flg & FCOMMENT != 0 {
        while data_start < n && at(data_start) != 0 {
            data_start += 1;
        }
        data_start += 1;
    }
    if flg & FHCRC != 0 {
        data_start += 2;
    }
    if data_start >= n {
        return makeerror("File too short");
    }
    uncompresszlib(old.get(data_start..).unwrap_or_default(), bytes_max, false)
}

/// `uncompresszlib`.
fn uncompresszlib(old: &[u8], bytes_max: usize, zlib: bool) -> Data {
    match crate::zlib::inflate(old, bytes_max, zlib) {
        Ok(v) => Data::Ok(v),
        Err(msg) => makeerror(msg),
    }
}

/// `uncompressbuf`: decompress `old` (or, given the file, the file) with
/// method `method`, at most `bytes_max` bytes of it.
fn uncompressbuf(fd: Option<&File>, bytes_max: usize, method: usize, nofork: bool, old: &[u8]) -> Data {
    if method == METH_FROZEN || method == METH_ZLIB {
        // Upstream refuses the built-in decompressors, which need no fork,
        // when forking is forbidden -- the test is on the wrong branch, and
        // kept.
        if nofork {
            return makeerror("Fork is required to uncompress, but disabled");
        }
        return if method == METH_ZLIB {
            uncompresszlib(old, bytes_max, true)
        } else {
            uncompressgzipped(old, bytes_max)
        };
    }
    uncompress_external(fd, bytes_max, method, old)
}

/// `strerror` of an I/O error.
fn strerror(e: &std::io::Error) -> String {
    errmsg::strerror(e)
}

/// The external half of `uncompressbuf`: the program reads the file (or, with
/// no file, `old` through a pipe) and its standard output is the result --
/// unless it writes to standard error, which is then the message.
#[allow(clippy::too_many_lines)]
fn uncompress_external(fd: Option<&File>, bytes_max: usize, method: usize, old: &[u8]) -> Data {
    let Some(c) = COMPR.get(method) else {
        return makeerror("");
    };
    // `fflush(stdout)`: what is written so far goes before the child's turn.
    let _flushed = crate::out::flush();

    let stdin_pipe = if fd.is_none() {
        match std::io::pipe() {
            Ok(p) => Some(p),
            Err(e) => return makeerror(format!("Cannot create pipe, {}", strerror(&e))),
        }
    } else {
        None
    };
    let (mut out_r, out_w) = match std::io::pipe() {
        Ok(p) => p,
        Err(e) => return makeerror(format!("Cannot create pipe, {}", strerror(&e))),
    };
    let (mut err_r, err_w) = match std::io::pipe() {
        Ok(p) => p,
        Err(e) => return makeerror(format!("Cannot create pipe, {}", strerror(&e))),
    };

    // The child's standard input: the file, from its start, or the pipe.
    let (child_in, stdin_w): (Option<std::process::Stdio>, _) = match (fd, stdin_pipe) {
        (Some(f), _) => {
            // `lseek` back to the start; a pipe cannot, and is passed on as
            // it is.
            let _rewound = (&*f).seek(SeekFrom::Start(0));
            (f.try_clone().ok().map(std::process::Stdio::from), None)
        }
        (None, Some((r, w))) => (Some(std::process::Stdio::from(r)), Some(w)),
        (None, None) => (None, None),
    };
    let child = child_in.and_then(|stdin| spawn(c.argv, stdin, out_w, err_w));

    std::thread::scope(|scope| {
        // `writechild`: the data goes in from apart, so that neither side
        // waits on a full pipe.
        let writer = stdin_w.map(|mut w| {
            scope.spawn(move || {
                // A failed write is the writer's exit status, which nothing
                // reads.
                let _written = w.write_all(old);
            })
        });

        let mut rv = read_back(&mut out_r, &mut err_r, bytes_max);
        drop(out_r);
        drop(err_r);

        // `waitpid`. A spawn that failed leaves upstream waiting on a pid
        // it never got: ECHILD.
        match child {
            Some(mut ch) => {
                if let Err(e) = ch.wait() {
                    rv = makeerror(format!("Wait failed, {}", strerror(&e)));
                }
            }
            None => rv = makeerror("Wait failed, No child processes"),
        }
        if let Some(h) = writer {
            // The writer cannot panic: it only writes.
            let _joined = h.join();
        }
        rv
    })
}

/// Start `argv` as `posix_spawnp(..., envp = NULL)` does: found on this
/// process's `PATH`, run with an empty environment.
fn spawn(argv: &[&str], stdin: std::process::Stdio, out: std::io::PipeWriter, err: std::io::PipeWriter) -> Option<std::process::Child> {
    let (prog, args) = argv.split_first()?;
    #[cfg(unix)]
    let mut cmd = {
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(find_program(prog)?);
        cmd.arg0(prog);
        cmd
    };
    // Elsewhere -- Windows, where this is built only to be tested -- `Command`
    // searches this process's `PATH` itself, empty environment or not, and a
    // program that is not there fails to spawn.
    #[cfg(not(unix))]
    let mut cmd = std::process::Command::new(prog);
    cmd.args(args).env_clear().stdin(stdin).stdout(out).stderr(err);
    cmd.spawn().ok()
}

/// Where `posix_spawnp` finds `prog`: itself when it has a slash, otherwise
/// the first executable file of that name on this process's `PATH` (the
/// child's environment is empty, so the search cannot be left to it).
#[cfg(unix)]
fn find_program(prog: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    if prog.contains('/') {
        return Some(prog.into());
    }
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/bin:/usr/bin".into());
    std::env::split_paths(&path).find_map(|dir| {
        // An empty element is the current directory.
        let dir = if dir.as_os_str().is_empty() { std::path::PathBuf::from(".") } else { dir };
        let full = dir.join(prog);
        let md = std::fs::metadata(&full).ok()?;
        (md.is_file() && md.permissions().mode() & 0o111 != 0).then_some(full)
    })
}

/// The parent's side of `uncompressbuf`: standard output, up to
/// `bytes_max`; then, if that was not all of it, standard error.
fn read_back(out: &mut std::io::PipeReader, err: &mut std::io::PipeReader, bytes_max: usize) -> Data {
    let stdout = match read_upto(out, bytes_max) {
        Ok(v) => v,
        // Upstream's buffer then holds what it holds; the message is
        // whatever the read left in it.
        Err((partial, _)) => return Data::Err(partial),
    };
    if stdout.len() == bytes_max {
        // Close it, so that the child dies of SIGPIPE rather than blocking.
        return Data::Ok(stdout);
    }
    match read_upto(err, bytes_max) {
        Ok(e) if e.is_empty() => Data::Ok(stdout),
        Ok(e) => Data::Err(filter_error(overlay(&stdout, &e))),
        Err((_, e)) => makeerror(format!("Read stderr failed, {}", strerror(&e))),
    }
}

/// Standard error read into the buffer standard output was read into, then
/// cut where standard output ended: `filter_error(*newch, r)`.
fn overlay(stdout: &[u8], stderr: &[u8]) -> Vec<u8> {
    let r = stdout.len();
    let mut v = stderr.to_vec();
    if v.len() < r {
        v.extend_from_slice(stdout.get(v.len()..).unwrap_or_default());
    }
    v.truncate(r);
    v
}

/// `sread(fd, buf, n, 0)` into a new buffer: to `n` bytes or the end. On an
/// error, what was read and the error.
fn read_upto(r: &mut impl Read, n: usize) -> Result<Vec<u8>, (Vec<u8>, std::io::Error)> {
    let mut v = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    while v.len() < n {
        let want = (n - v.len()).min(chunk.len());
        match r.read(&mut chunk[..want]) {
            Ok(0) => break,
            Ok(k) => v.extend_from_slice(&chunk[..k]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err((v, e)),
        }
    }
    Ok(v)
}

/// `filter_error`: the useful part of a decompressor's complaint -- its first
/// line, before any `;`, after the last `:`, capitalised.
fn filter_error(ubuf: Vec<u8>) -> Vec<u8> {
    let s = cstr(&ubuf);
    let start = s.iter().position(|&c| !isspace(c)).unwrap_or(s.len());
    let mut t = s.get(start..).unwrap_or_default();
    if let Some(p) = t.iter().position(|&c| c == b'\n') {
        t = t.get(..p).unwrap_or_default();
    }
    if let Some(p) = t.iter().position(|&c| c == b';') {
        t = t.get(..p).unwrap_or_default();
    }
    let mut out = match t.iter().rposition(|&c| c == b':') {
        Some(p) => {
            let q = t.get(p + 1..).unwrap_or_default();
            let skip = q.iter().position(|&c| !isspace(c)).unwrap_or(q.len());
            q.get(skip..).unwrap_or_default().to_vec()
        }
        // Without a colon nothing moves: the leading space stays.
        None => s.get(..start + t.len()).unwrap_or_default().to_vec(),
    };
    if let Some(c) = out.first_mut() {
        c.make_ascii_uppercase();
    }
    out
}


#[cfg(unix)]
mod sys {
    /// `struct pollfd`.
    #[repr(C)]
    pub struct PollFd {
        pub fd: i32,
        pub events: i16,
        pub revents: i16,
    }

    unsafe extern "C" {
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
        pub fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        pub fn dup2(oldfd: i32, newfd: i32) -> i32;
    }

    /// `FIONREAD`, Linux's number, which SlateOS's C library shares.
    pub const FIONREAD: u64 = 0x541B;
    pub const POLLIN: i16 = 0x001;

    /// `ioctl(fd, FIONREAD, &t)`: the bytes waiting, or `None` for -1.
    pub fn fionread(fd: i32) -> Option<i32> {
        let mut t: i32 = 0;
        // SAFETY: FIONREAD writes one `int` through the pointer, which points
        // at `t`, live for the call.
        let r = unsafe { ioctl(fd, FIONREAD, &raw mut t) };
        if r == -1 { None } else { Some(t) }
    }

    /// `select` on one descriptor for reading, for 100ms: 1, 0 or -1.
    pub fn wait_readable(fd: i32) -> i32 {
        let mut p = PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        // SAFETY: one `pollfd`, pointing at `p`, live for the call.
        let r = unsafe { poll(&raw mut p, 1, 100) };
        if r > 0 { 1 } else { r }
    }
}

/// `sread`: read `buf.len()` bytes, or to the end. With `canbepipe`, a
/// descriptor other than standard input is asked first how much is waiting --
/// and given a tenth of a second for something to arrive -- and only that
/// much is read.
pub fn sread(f: &File, buf: &mut [u8], canbepipe: bool) -> std::io::Result<usize> {
    #[cfg(unix)]
    let n = if canbepipe {
        match waiting(f, buf.len()) {
            Some(n) => n,
            None => return Ok(0),
        }
    } else {
        buf.len()
    };
    // Elsewhere there is no `FIONREAD` to ask.
    #[cfg(not(unix))]
    let n = {
        let _ = canbepipe;
        buf.len()
    };
    let mut got = 0usize;
    while got < n {
        match (&*f).read(&mut buf[got..n]) {
            Ok(0) => break,
            Ok(r) => got += r,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

/// The part of `sread` that looks first: how much of `n` to read from a
/// descriptor that may be a pipe -- what `FIONREAD` says is waiting, when that
/// is less, after a tenth of a second's wait if nothing is -- or `None` when
/// the wait gives up. Standard input is read without looking.
#[cfg(unix)]
fn waiting(f: &File, n: usize) -> Option<usize> {
    use std::os::unix::io::AsRawFd;
    let fd = f.as_raw_fd();
    if fd == 0 {
        return Some(n);
    }
    // `t` starts at 0, and a failed `ioctl` leaves it there.
    let mut t = sys::fionread(fd).unwrap_or(0);
    if t == 0 {
        // Upstream's loop: an error tries again; a timeout gives up at once
        // on the first round, and on any later one after five.
        let mut cnt = 0;
        loop {
            let selrv = sys::wait_readable(fd);
            if selrv == -1 {
                cnt += 1;
                continue;
            }
            if selrv == 0 && cnt >= 5 {
                return None;
            }
            break;
        }
        t = sys::fionread(fd).unwrap_or(t);
    }
    Some(match usize::try_from(t) {
        Ok(tu) if tu > 0 && tu < n => tu,
        _ => n,
    })
}

/// `lseek(fd, 0, SEEK_SET)` failing with `ESPIPE`: a pipe, socket or FIFO,
/// which the readers that seek around cannot read in place. A failure leaves
/// its `errno` behind, as upstream's does.
pub fn is_pipe(ms: &mut Ms, f: &File) -> bool {
    match (&*f).seek(SeekFrom::Start(0)) {
        Ok(_) => false,
        Err(e) => {
            ms.errno = Some(crate::funcs::Errno::of(&e));
            e.kind() == std::io::ErrorKind::NotSeekable
        }
    }
}

/// A file only this process can see: `mkstemp("/tmp/file.XXXXXX")`, unlinked
/// at once.
fn tempfile() -> std::io::Result<File> {
    use std::time::{SystemTime, UNIX_EPOCH};
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ u64::from(std::process::id()).rotate_left(32);
    let mut last = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    for _ in 0..100 {
        let mut name = b"/tmp/file.".to_vec();
        for _ in 0..6 {
            // xorshift: names, not secrets -- `create_new` is what makes it
            // safe.
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            name.push(CHARS[(seed % CHARS.len() as u64) as usize]);
        }
        let path = crate::apprentice::os_path(&name);
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&path) {
            Ok(f) => {
                // Unlinked, it goes when the last descriptor does.
                let _gone = std::fs::remove_file(&path);
                return Ok(f);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Where [`file_pipe2file`] left the copy.
pub enum PipeCopy {
    /// In the pipe's own descriptor, which now names it.
    InPlace,
    /// In a file of its own: hosts without `dup2`.
    #[cfg(not(unix))]
    Apart(File),
}

/// `file_pipe2file`: copy what was read and the rest of the pipe to a
/// temporary file, which then takes the pipe's descriptor (`dup2`) -- so the
/// readers can seek, and so does anything that reads that descriptor later.
/// `None` is -1, the error recorded.
pub fn file_pipe2file(ms: &mut Ms, fd: &File, startbuf: &[u8]) -> Option<PipeCopy> {
    let mut tf = match tempfile() {
        Ok(f) => f,
        Err(e) => {
            ms.error(Some(&e), b"cannot create temporary file for pipe copy");
            return None;
        }
    };
    if let Err(e) = tf.write_all(startbuf) {
        ms.error(Some(&e), b"error while writing to temp file");
        return None;
    }
    let mut buf = vec![0u8; 4096];
    loop {
        match sread(fd, &mut buf, true) {
            Ok(0) => break,
            Ok(r) => {
                if let Err(e) = tf.write_all(&buf[..r]) {
                    ms.error(Some(&e), b"error while writing to temp file");
                    return None;
                }
            }
            Err(e) => {
                ms.error(Some(&e), b"error copying from pipe to temp file");
                return None;
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // SAFETY: both are open descriptors this process owns; `dup2` makes
        // `fd`'s number refer to the copy, and the `File` that owns that
        // number keeps owning it.
        if unsafe { sys::dup2(tf.as_raw_fd(), fd.as_raw_fd()) } == -1 {
            let e = std::io::Error::last_os_error();
            ms.error(Some(&e), b"could not dup descriptor for temp file");
            return None;
        }
        drop(tf);
        if let Err(e) = (&*fd).seek(SeekFrom::Start(0)) {
            ms.error(Some(&e), b"error seeking");
            return None;
        }
        Some(PipeCopy::InPlace)
    }
    #[cfg(not(unix))]
    {
        if let Err(e) = tf.seek(SeekFrom::Start(0)) {
            ms.error(Some(&e), b"error seeking");
            return None;
        }
        Some(PipeCopy::Apart(tf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_error_keeps_the_useful_part() {
        assert_eq!(filter_error(b"gzip: stdin: not in gzip format\n".to_vec()), b"Not in gzip format");
        assert_eq!(filter_error(b"  plain words\nmore".to_vec()), b"  plain words");
        assert_eq!(filter_error(b"x: a; b\n".to_vec()), b"A");
        assert_eq!(filter_error(Vec::new()), b"");
    }

    #[test]
    fn standard_error_is_cut_where_standard_output_ended() {
        assert_eq!(overlay(b"", b"bzip2: oops\n"), b"");
        assert_eq!(overlay(b"abcdefgh", b"xy"), b"xycdefgh");
        assert_eq!(overlay(b"abc", b"wxyz"), b"wxy");
    }

    #[test]
    fn the_headers_are_recognised() {
        assert!(zlibcmp(&[0x78, 0x9c]));
        assert!(!zlibcmp(&[0x78, 0x9d]));
        assert!(!zlibcmp(&[0x88, 0x9c]));
        let mut lzma = [0u8; 13];
        lzma[0] = 0x5d;
        assert!(lzmacmp(&lzma));
        lzma[12] = 1;
        assert!(!lzmacmp(&lzma));
    }

    #[test]
    fn a_gzip_header_is_skipped() {
        // gzip -n of "hello\n", without its trailer.
        let gz = [
            0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x02, 0x03, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0xe7, 0x02, 0x00,
        ];
        assert!(matches!(uncompressgzipped(&gz, 100), Data::Ok(v) if v == b"hello\n"));
        assert!(matches!(uncompressgzipped(&gz[..3], 100), Data::Err(m) if m == b"File too short"));
        assert!(matches!(uncompressgzipped(&gz[..10], 100), Data::Err(m) if m == b"File too short"));
    }
}
