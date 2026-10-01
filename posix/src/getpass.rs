//! `getpass` (`<unistd.h>`; in SUSv2, withdrawn since, still called by
//! `login`-like programs): prompt for a password and read it with echo off,
//! as glibc 2.39's `getpass.c` does.
//!
//! It talks to the terminal -- `/dev/tty`, for both the prompt and the
//! answer -- so that a password is typed at the person, not read from a
//! pipe the program happens to have on stdin; with no terminal to open it
//! falls back to stdin and stderr. Echo and signal keys (`ECHO`, `ISIG`) are
//! turned off while the line is read, and the newline the typist cannot see
//! echoed is written after it. The answer is the line without its newline,
//! in this process's own buffer, overwritten by the next call; at end of
//! input it is the empty string, not NULL (glibc's).

use crate::perprocess::process_global;

process_global! {
    /// The line buffer, a `getline` pair, kept from call to call.
    fn line() -> (*mut u8, usize) = (core::ptr::null_mut(), 0);
}

/// Prompt with `prompt` and read a password: see the module docs. NULL only
/// when the buffer cannot be allocated.
///
/// # Safety
///
/// `prompt` is NULL -- written as `(null)`, as glibc's `%s` writes it -- or
/// a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpass(prompt: *const u8) -> *mut u8 {
    // SAFETY: both C strings.
    let tty = unsafe { crate::stdio::fopen(c"/dev/tty".as_ptr().cast(), c"w+ce".as_ptr().cast()) };
    let (input, output) = if tty.is_null() {
        (crate::stdio::stdin_stream(), crate::stdio::stderr_stream())
    } else {
        (tty, tty)
    };
    // SAFETY: open streams; `prompt` as the caller gave it.
    let answer = unsafe { getpass_on(prompt, input, output) };
    if !tty.is_null() {
        // Read-only use of our own stream: nothing is lost if closing it
        // reports an error.
        let _ = crate::stdio::fclose(tty);
    }
    answer
}

/// [`getpass`] on streams the caller opened: the tests' way in.
///
/// # Safety
///
/// `input` and `output` are open streams; `prompt` is NULL or a C string.
pub(crate) unsafe fn getpass_on(prompt: *const u8, input: *mut u8, output: *mut u8) -> *mut u8 {
    crate::stdio::flockfile(output.cast());
    let fd = crate::stdio::fileno(input);
    // Echo and the signal keys off, if `input` is a terminal that lets us.
    let mut t = crate::ioctl::Termios {
        c_iflag: 0,
        c_oflag: 0,
        c_cflag: 0,
        c_lflag: 0,
        c_line: 0,
        c_cc: [0; crate::ioctl::NCCS],
        c_ispeed: 0,
        c_ospeed: 0,
    };
    let saved = if crate::ioctl::tcgetattr(fd, &raw mut t) == 0 {
        let old = t;
        t.c_lflag &= !(crate::ioctl::ECHO | crate::ioctl::ISIG);
        (crate::ioctl::tcsetattr(fd, crate::ioctl::TCSAFLUSH, &raw const t) == 0).then_some(old)
    } else {
        None
    };
    let text: &[u8] = if prompt.is_null() {
        b"(null)"
    } else {
        // SAFETY: the caller's C string.
        unsafe { crate::nss_files::c_bytes(prompt) }
    };
    // A prompt that cannot be written still leaves a password to read, as
    // glibc's, which does not look at its `fprintf`'s answer either.
    let _ = crate::stdio::write_stream(output, text.as_ptr(), text.len());
    let _ = crate::stdio::fflush(output);
    // SAFETY: this process's buffer pair, used by one call at a time.
    let (buf, cap) = unsafe { &mut *line() };
    // SAFETY: a `getline` pair and an open stream.
    let n = unsafe { crate::stdio::getline(&raw mut *buf, &raw mut *cap, input) };
    let p = *buf;
    if !p.is_null() {
        match usize::try_from(n).ok().and_then(|len| len.checked_sub(1)) {
            // `getline` wrote the line, then a NUL: a newline ending it
            // goes.
            Some(last) => {
                // SAFETY: `last` is the index of the line's last byte.
                let end = unsafe { p.add(last) };
                // SAFETY: as above.
                if unsafe { end.read() } == b'\n' {
                    // SAFETY: as above.
                    unsafe { end.write(0) };
                    if saved.is_some() {
                        // The newline the typist did not see echoed.
                        let _ = crate::stdio::write_stream(output, b"\n".as_ptr(), 1);
                    }
                }
            }
            // End of input, or an error: the empty string.
            None => {
                // SAFETY: a `getline` buffer holds at least one byte.
                unsafe { p.write(0) };
            }
        }
    }
    if let Some(old) = saved {
        // Nothing to be done if the terminal refuses its settings back.
        let _ = crate::ioctl::tcsetattr(fd, crate::ioctl::TCSAFLUSH, &raw const old);
    }
    crate::stdio::funlockfile(output.cast());
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// glibc's answers, from `posix/tools/oracle/accounts_harness.py`: three
    /// calls with input `secret\nline2` and no terminal, the prompts
    /// `P0:`..`P2:` on stderr, which are asserted in `accounts_oracle`.
    #[test]
    fn reads_lines_without_their_newline_and_the_end_as_empty() {
        let mut input = *b"secret\nline2";
        // SAFETY: a buffer of the length given, read-only.
        let inp = unsafe {
            crate::stdio_mem::fmemopen(input.as_mut_ptr().cast(), input.len(), c"r".as_ptr().cast())
        };
        let mut out: *mut u8 = core::ptr::null_mut();
        let mut out_len = 0usize;
        // SAFETY: a pair of out-pointers.
        let outp = unsafe { crate::stdio_mem::open_memstream(&raw mut out, &raw mut out_len) };
        assert!(!inp.is_null() && !outp.is_null());
        let mut got = std::vec::Vec::new();
        for prompt in [c"P0:", c"P1:", c"P2:"] {
            // SAFETY: open streams, a C string.
            let r = unsafe { getpass_on(prompt.as_ptr().cast(), inp, outp) };
            assert!(!r.is_null());
            // SAFETY: getpass's buffer, NUL-terminated.
            got.push(unsafe { crate::nss_files::c_bytes(r) }.to_vec());
        }
        assert_eq!(got, [&b"secret"[..], b"line2", b""]);
        assert_eq!(crate::stdio::fclose(outp), 0);
        // SAFETY: open_memstream's buffer, `out_len` bytes long.
        let written = unsafe { core::slice::from_raw_parts(out, out_len) };
        assert_eq!(written, b"P0:P1:P2:", "no newline: echo was never off");
        // SAFETY: the memstream's block.
        unsafe { crate::malloc::free(out) };
        let _ = crate::stdio::fclose(inp);
    }

    #[test]
    fn a_null_prompt_is_written_as_glibcs_percent_s_writes_it() {
        let mut input = *b"x\n";
        // SAFETY: as above.
        let inp = unsafe {
            crate::stdio_mem::fmemopen(input.as_mut_ptr().cast(), input.len(), c"r".as_ptr().cast())
        };
        let mut out: *mut u8 = core::ptr::null_mut();
        let mut out_len = 0usize;
        // SAFETY: as above.
        let outp = unsafe { crate::stdio_mem::open_memstream(&raw mut out, &raw mut out_len) };
        // SAFETY: open streams; a NULL prompt.
        let r = unsafe { getpass_on(core::ptr::null(), inp, outp) };
        // SAFETY: getpass's buffer.
        assert_eq!(unsafe { crate::nss_files::c_bytes(r) }, b"x");
        assert_eq!(crate::stdio::fclose(outp), 0);
        // SAFETY: as above.
        assert_eq!(
            unsafe { core::slice::from_raw_parts(out, out_len) },
            b"(null)"
        );
        // SAFETY: the memstream's block.
        unsafe { crate::malloc::free(out) };
        let _ = crate::stdio::fclose(inp);
    }
}
