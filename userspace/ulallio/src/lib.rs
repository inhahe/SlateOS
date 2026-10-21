//! util-linux's `include/all-io.h`: a whole buffer written, or read as far
//! as the file goes, through the short transfers and interruptions a single
//! `write` or `read` may meet -- with util-linux's habit of waiting a
//! quarter of a second (`xusleep (250000)`) before trying again a descriptor
//! that said it would block.
//!
//! The wait is part of what a caller can see: a non-blocking descriptor that
//! stays full makes `write_all` retry four times a second, for ever, where a
//! plain loop would spin. `rfkill` writes its requests with it and
//! `hardlink`'s file comparison reads with it.

/// `xusleep (250000)`: a quarter of a second, cut short by a signal a
/// handler catches -- whose `EINTR` upstream does not look at either.
fn nap() {
    // The wait only paces the retries: ending early changes nothing.
    let _ = libcall::clock::nanosleep(0, 250_000_000);
}

/// `write_all (fd, buf, count)`: all of `buf` written to `fd` -- again after
/// a short write, at once after `EINTR`, and a quarter of a second after
/// `EAGAIN`, as often as it takes.
///
/// # Errors
///
/// The `errno` of the write that failed; 0 for a write that wrote nothing,
/// as upstream sets `errno` to 0 before each write and then finds it neither
/// `EINTR` nor `EAGAIN`.
pub fn write_all(fd: i32, buf: &[u8]) -> Result<(), i32> {
    let mut rest = buf;
    while !rest.is_empty() {
        match libcall::fd::write(fd, rest) {
            Ok(0) => return Err(0),
            Ok(n) => rest = rest.get(n..).unwrap_or_default(),
            Err(libcall::EINTR) => {}
            Err(libcall::EAGAIN) => nap(),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// `read_all (fd, buf, count)`: `buf` zeroed, then filled from `fd` as far
/// as the file goes -- again after a short read, and after `EINTR` or
/// `EAGAIN` a quarter of a second later, five times in a row at most. How
/// many bytes came: fewer than `buf` holds at the end of the file, or after
/// an error once something had come.
///
/// # Errors
///
/// The `errno` of a failed read before anything came.
pub fn read_all(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
    buf.fill(0);
    let mut got = 0usize;
    let mut tries = 0u32;
    while got < buf.len() {
        match libcall::fd::read(fd, buf.get_mut(got..).unwrap_or_default()) {
            Ok(0) => return Ok(got),
            Ok(n) => {
                tries = 0;
                got = got.saturating_add(n);
            }
            Err(e) if (e == libcall::EAGAIN || e == libcall::EINTR) && tries < 5 => {
                tries = tries.saturating_add(1);
                nap();
            }
            Err(e) => return if got > 0 { Ok(got) } else { Err(e) },
        }
    }
    Ok(got)
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, reason = "a test fails loudly on a broken pipe")]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_goes_through_a_pipe_whole_and_comes_back_zero_padded() {
        let (r, w) = libcall::fd::pipe().unwrap();
        write_all(w, b"hello").unwrap();
        libcall::fd::close(w).unwrap();
        let mut buf = [0xffu8; 8];
        assert_eq!(read_all(r, &mut buf), Ok(5), "the end of the file");
        assert_eq!(&buf, b"hello\0\0\0", "the rest zeroed");
        let mut again = [0xffu8; 2];
        assert_eq!(read_all(r, &mut again), Ok(0));
        assert_eq!(again, [0, 0]);
        libcall::fd::close(r).unwrap();
    }

    #[test]
    fn a_bad_descriptor_fails_with_its_errno() {
        // -1, never a descriptor: one just closed could be reused at once by
        // a test running beside this one.
        use libcall::EBADF;
        assert_eq!(read_all(-1, &mut [0u8; 4]), Err(EBADF));
        assert_eq!(write_all(-1, b"x"), Err(EBADF));
        assert_eq!(write_all(-1, b""), Ok(()), "nothing to write is no write");
    }
}
