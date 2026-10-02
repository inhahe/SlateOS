//! libmagic's `magic.c`: the library's entry points -- open, load, identify a
//! named file or standard input -- as `file.c` calls them.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::apprentice::{Action, file_apprentice, os_path};
use crate::buffer::Stat;
use crate::funcs::{EVENT_HAD_ERR, Ms, file_buffer};
use crate::magic::*;

/// `PIPE_BUF`: a read from a pipe this short is taken as all there is.
const PIPE_BUF: usize = 4096;

/// `O_NONBLOCK`: `file` opens what it reads so that a FIFO with no writer
/// does not hang it. The value is Linux's, which SlateOS's C library shares.
#[cfg(unix)]
const O_NONBLOCK: i32 = 0o4000;

/// `magic_open`.
#[must_use]
pub fn magic_open(flags: u32) -> Ms {
    Ms::new(flags)
}

/// `magic_load`.
pub fn magic_load(ms: &mut Ms, magicfile: Option<&[u8]>) -> i32 {
    file_apprentice(ms, magicfile, Action::Load)
}

/// `magic_compile`.
pub fn magic_compile(ms: &mut Ms, magicfile: Option<&[u8]>) -> i32 {
    file_apprentice(ms, magicfile, Action::Compile)
}

/// `magic_check`.
pub fn magic_check(ms: &mut Ms, magicfile: Option<&[u8]>) -> i32 {
    file_apprentice(ms, magicfile, Action::Check)
}

/// `magic_list`.
pub fn magic_list(ms: &mut Ms, magicfile: Option<&[u8]>) -> i32 {
    file_apprentice(ms, magicfile, Action::List)
}

/// `magic_error`: the first error's message, if one was recorded.
#[must_use]
pub fn magic_error(ms: &Ms) -> Option<Vec<u8>> {
    if ms.event_flags & EVENT_HAD_ERR == 0 {
        return None;
    }
    Some(ms.o_buf.as_deref().map(crate::cstd::cstr).unwrap_or_default().to_vec())
}

/// `MAGIC_PARAM_*_MAX`, as `-P` names them: the limit on each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Indir,
    Name,
    ElfPhnum,
    ElfShnum,
    ElfNotes,
    Regex,
    Bytes,
    Encoding,
    ElfShsize,
}

/// `magic_setparam`: the 16-bit limits keep their low 16 bits, as C's
/// conversion does.
pub fn magic_setparam(ms: &mut Ms, p: Param, v: usize) {
    #[allow(clippy::cast_possible_truncation)]
    let v16 = v as u16;
    match p {
        Param::Indir => ms.indir_max = v16,
        Param::Name => ms.name_max = v16,
        Param::ElfPhnum => ms.elf_phnum_max = v16,
        Param::ElfShnum => ms.elf_shnum_max = v16,
        Param::ElfNotes => ms.elf_notes_max = v16,
        Param::Regex => ms.regex_max = v16,
        Param::Bytes => ms.bytes_max = v,
        Param::Encoding => ms.encoding_max = v,
        Param::ElfShsize => ms.elf_shsize_max = v,
    }
}

/// `access(2)`: whether the real user may `mode` the file (2 write, 1
/// execute).
fn access(path: &[u8], mode: i32) -> bool {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        unsafe extern "C" {
            fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
        }
        let Ok(c) = CString::new(path) else {
            return false;
        };
        // SAFETY: `c` is a NUL-terminated string that lives across the call,
        // and access(2) only reads it.
        unsafe { access(c.as_ptr(), mode) == 0 }
    }
    #[cfg(not(unix))]
    {
        // No permission bits to ask about on the host: writable unless
        // read-only, and nothing is executable by its mode.
        mode == 2 && std::fs::metadata(os_path(path)).is_ok_and(|m| !m.permissions().readonly())
    }
}

/// `unreadable_info`: what can be said of a file that cannot be read.
fn unreadable_info(ms: &mut Ms, mode: u32, file: Option<&[u8]>) -> i32 {
    if let Some(f) = file {
        // We cannot open it, but we were able to stat it.
        if access(f, 2) && ms.print_str(b"writable, ") == -1 {
            return -1;
        }
        if access(f, 1) && ms.print_str(b"executable, ") == -1 {
            return -1;
        }
    }
    if mode & crate::buffer::S_IFMT == crate::buffer::S_IFREG && ms.print_str(b"regular file, ") == -1 {
        return -1;
    }
    if ms.print_str(b"no read permission") == -1 {
        return -1;
    }
    0
}

/// Standard input as a `File`, not to be closed.
fn stdin_file() -> std::mem::ManuallyDrop<File> {
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        // SAFETY: descriptor 0 is open for the life of the process (the
        // standard-descriptor guard sees to that), and `ManuallyDrop` keeps
        // this `File` from closing it.
        std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(0) })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        let h = std::io::stdin().as_raw_handle();
        // SAFETY: the process's standard-input handle stays open, and
        // `ManuallyDrop` keeps this `File` from closing it.
        std::mem::ManuallyDrop::new(unsafe { File::from_raw_handle(h) })
    }
}

/// `sread`: a pipe read to its end or to `n` bytes. (Upstream polls a pipe
/// that is not standard input with `FIONREAD` and `select` for half a second;
/// `file` reaches such a pipe only with `-s`, and reads it until it ends.)
fn sread(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0usize;
    while got < buf.len() {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(r) => got += r,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

/// `magic_file`: identify the named file, or standard input for `None`.
/// `None` back is an error, whose message [`magic_error`] has.
pub fn magic_file(ms: &mut Ms, inname: Option<&[u8]>) -> Option<Vec<u8>> {
    if file_or_fd(ms, inname) == 0 {
        ms.getbuffer()
    } else {
        None
    }
}

/// `file_or_fd`.
#[allow(clippy::too_many_lines)]
fn file_or_fd(ms: &mut Ms, inname: Option<&[u8]>) -> i32 {
    if ms.reset(true) == -1 {
        return -1;
    }
    let mut sb = Stat::default();
    match crate::fsmagic::file_fsmagic(ms, inname, &mut sb) {
        -1 => return -1,
        0 => {}
        _ => return 0,
    }

    let mut owned: Option<File> = None;
    let mut stdin = None;
    if let Some(name) = inname {
        // `errno = 0`: what the readers below may find in it is theirs.
        ms.errno = None;
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(O_NONBLOCK);
        }
        match opts.open(os_path(name)) {
            Ok(f) => owned = Some(f),
            Err(e) => {
                ms.errno = Some(crate::funcs::Errno::of(&e));
                if let Ok(md) = std::fs::metadata(os_path(name)) {
                    let st = Stat::from_metadata(&md);
                    if unreadable_info(ms, st.mode, Some(name)) == -1 {
                        return -1;
                    }
                }
                return 0;
            }
        }
    } else {
        stdin = Some(stdin_file());
    }
    let file: &mut File = match (owned.as_mut(), stdin.as_mut()) {
        (Some(f), _) => f,
        (None, Some(s)) => s,
        (None, None) => return -1,
    };

    let mut okstat = false;
    let mut ispipe = false;
    if let Ok(md) = file.metadata() {
        okstat = true;
        sb = Stat::from_metadata(&md);
        ispipe = sb.is_fifo();
    }
    let pos = if inname.is_none() { file.stream_position().ok() } else { None };

    let mut buf = vec![0u8; ms.bytes_max];
    let mut nbytes = 0usize;
    if ispipe {
        loop {
            match sread(file, &mut buf[nbytes..]) {
                Ok(r) if r > 0 => {
                    nbytes += r;
                    if r < PIPE_BUF {
                        break;
                    }
                }
                _ => break,
            }
        }
        if nbytes == 0 && inname.is_some() {
            // We cannot read it, but we were able to stat it.
            if unreadable_info(ms, sb.mode, inname) == -1 {
                return finish(ms, file, pos, inname, &sb, -1);
            }
            return finish(ms, file, pos, inname, &sb, 0);
        }
    } else {
        match file.read(&mut buf) {
            Ok(n) => nbytes = n,
            Err(e) => {
                let mut msg = b"cannot read `".to_vec();
                msg.extend_from_slice(inname.unwrap_or(b"/dev/stdin"));
                msg.push(b'\'');
                ms.error(Some(&e), &msg);
                return finish(ms, file, pos, inname, &sb, -1);
            }
        }
    }
    buf.truncate(nbytes);
    let st = if okstat { Some(sb) } else { None };
    let rv = if file_buffer(ms, Some(&*file), st, inname, &buf) == -1 { -1 } else { 0 };
    finish(ms, file, pos, inname, &sb, rv)
}

/// `done:` -- put standard input back where it was, and with `-p` the file's
/// access and modification times.
fn finish(ms: &Ms, file: &mut File, pos: Option<u64>, inname: Option<&[u8]>, sb: &Stat, rv: i32) -> i32 {
    if let Some(p) = pos {
        // A pipe cannot seek back; nothing upstream does about it either.
        let _back = file.seek(SeekFrom::Start(p));
    }
    if inname.is_some() && ms.flags & MAGIC_PRESERVE_ATIME != 0 {
        // `utimes` with whole seconds: what upstream sets back.
        let at = epoch_plus(sb.atime);
        let mt = epoch_plus(sb.mtime);
        let times = std::fs::FileTimes::new().set_accessed(at).set_modified(mt);
        // "don't care if loses".
        let _set = file.set_times(times);
    }
    rv
}

/// The epoch plus `secs`, which may be negative.
fn epoch_plus(secs: i64) -> std::time::SystemTime {
    let d = std::time::Duration::from_secs(secs.unsigned_abs());
    if secs >= 0 {
        std::time::UNIX_EPOCH + d
    } else {
        std::time::UNIX_EPOCH - d
    }
}
