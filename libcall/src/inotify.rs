//! Watching a directory for what is written into it: `inotify`, through the
//! linked C library.
//!
//! The settings watcher (`gui/settingswatch`) is the first user: it watches
//! the settings folder and announces each file rewritten in it. What reaches
//! the kernel is our `posix` library's `inotify_*`, backed by the kernel's
//! own filesystem-watch API (`SYS_FS_WATCH_*`, `fs::notify`), which queues an
//! event at the source for every create, delete, write and rename.
//!
//! # What SlateOS does not send
//!
//! **`IN_CLOSE_WRITE`, `IN_OPEN`, `IN_CLOSE_NOWRITE`**: the kernel has no
//! open or close hooks, so a watch asking for them is accepted, for Linux's
//! sake, and never fires. A caller that wants "this file is finished" has
//! `IN_MOVED_TO` for a file renamed into place -- how a careful writer saves
//! -- and otherwise has to wait for `IN_MODIFY` to go quiet.
//!
//! # Shape
//!
//! [`Inotify`] owns the descriptor and closes it when dropped. It is
//! non-blocking: [`Inotify::wait`] is the wait, with a timeout, so a caller
//! that needs to act when things go quiet can; [`Inotify::read`] never
//! blocks. [`Events`] reads the records one `read` returned without copying
//! or allocating, and is plain byte arithmetic, so it is tested on every host.
//!
//! On a host that is not unix every call answers [`ENOSYS`](crate::ENOSYS).

use core::ffi::CStr;

/// A file was written.
pub const IN_MODIFY: u32 = 0x0000_0002;
/// A file's metadata changed.
pub const IN_ATTRIB: u32 = 0x0000_0004;
/// A file opened for writing was closed. **SlateOS never sends this**; see
/// the module documentation.
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
/// A file was renamed away from the watched directory, or within it (the
/// first half of the pair).
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
/// A file was renamed into the watched directory, or within it: how a file
/// saved by writing a copy and renaming it over the original appears.
pub const IN_MOVED_TO: u32 = 0x0000_0080;
/// A file was created in the watched directory.
pub const IN_CREATE: u32 = 0x0000_0100;
/// A file was deleted from the watched directory.
pub const IN_DELETE: u32 = 0x0000_0200;
/// The watched directory itself was deleted.
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
/// The watched directory itself was renamed.
pub const IN_MOVE_SELF: u32 = 0x0000_0800;
/// Events were lost: the queue filled before they were read. Everything the
/// caller cares about must be looked at again.
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;
/// The watch is gone -- removed, or its directory deleted.
pub const IN_IGNORED: u32 = 0x0000_8000;
/// Only watch `path` if it is a directory.
pub const IN_ONLYDIR: u32 = 0x0100_0000;
/// The event's subject is a directory.
pub const IN_ISDIR: u32 = 0x4000_0000;

/// `inotify_init1`: close on `exec`.
pub const IN_CLOEXEC: i32 = 0o2_000_000;
/// `inotify_init1`: reads do not block.
pub const IN_NONBLOCK: i32 = 0o4_000;

/// The size of `struct inotify_event` before its name: `wd`, `mask`,
/// `cookie`, `len`, four bytes each.
pub const HEADER: usize = 16;

/// One event, borrowed from the buffer it was read into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event<'a> {
    /// The watch it came from, as [`Inotify::watch`] returned it; `-1` for
    /// [`IN_Q_OVERFLOW`].
    pub wd: i32,
    /// What happened: the `IN_*` bits.
    pub mask: u32,
    /// Pairs an [`IN_MOVED_FROM`] with its [`IN_MOVED_TO`].
    pub cookie: u32,
    /// The name, within the watched directory, of the file it happened to --
    /// empty for an event about the directory itself. Bytes, not text: a
    /// file name is whatever bytes it is.
    pub name: &'a [u8],
}

/// The records one `read` of an inotify descriptor returned, in order.
///
/// A record the buffer holds only part of ends the iteration with
/// [`Malformed`]: the kernel never splits one across reads, so a short
/// record means the buffer was not what `read` returned.
#[derive(Clone, Debug)]
pub struct Events<'a> {
    rest: &'a [u8],
}

/// A record that runs past the end of the bytes read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Malformed;

impl<'a> Events<'a> {
    /// The records in `bytes`, which must be what one `read` returned.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }
}

impl<'a> Iterator for Events<'a> {
    type Item = Result<Event<'a>, Malformed>;

    fn next(&mut self) -> Option<Self::Item> {
        // A copy of the slice, so what is read out of it borrows the buffer
        // (`'a`) and not this iterator.
        let rest: &'a [u8] = self.rest;
        if rest.is_empty() {
            return None;
        }
        let word =
            |at: usize| -> Option<[u8; 4]> { rest.get(at..at.checked_add(4)?)?.try_into().ok() };
        let parsed = (|| {
            let wd = i32::from_ne_bytes(word(0)?);
            let mask = u32::from_ne_bytes(word(4)?);
            let cookie = u32::from_ne_bytes(word(8)?);
            let len = usize::try_from(u32::from_ne_bytes(word(12)?)).ok()?;
            let end = HEADER.checked_add(len)?;
            let padded = rest.get(HEADER..end)?;
            // The name is NUL-padded to an alignment boundary; what comes
            // before the first NUL is the name.
            let name_len = padded.iter().position(|&b| b == 0).unwrap_or(padded.len());
            let name = padded.get(..name_len)?;
            Some((
                Event {
                    wd,
                    mask,
                    cookie,
                    name,
                },
                end,
            ))
        })();
        match parsed {
            Some((event, end)) => {
                self.rest = rest.get(end..).unwrap_or_default();
                Some(Ok(event))
            }
            None => {
                self.rest = &[];
                Some(Err(Malformed))
            }
        }
    }
}

/// An inotify instance: a descriptor its watches report to.
#[derive(Debug)]
pub struct Inotify {
    // Read only by the unix half; a host build never makes one.
    #[cfg_attr(not(unix), allow(dead_code))]
    fd: i32,
}

#[cfg(unix)]
mod sys {
    /// C's `struct pollfd`.
    #[repr(C)]
    pub struct PollFd {
        pub fd: i32,
        pub events: i16,
        pub revents: i16,
    }

    unsafe extern "C" {
        pub fn inotify_init1(flags: i32) -> i32;
        pub fn inotify_add_watch(fd: i32, pathname: *const u8, mask: u32) -> i32;
        pub fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
        pub fn close(fd: i32) -> i32;
    }

    /// `poll`: there is data to read.
    pub const POLLIN: i16 = 0x0001;
    /// `EAGAIN`: a non-blocking read with nothing to return.
    pub const EAGAIN: i32 = 11;
    /// `EINTR`: interrupted before anything happened.
    pub const EINTR: i32 = 4;
}

impl Inotify {
    /// A new instance, close-on-exec and non-blocking.
    ///
    /// # Errors
    ///
    /// The `errno` set by `inotify_init1`: `EMFILE` when this process has
    /// all the instances it may, `ENOSYS` on a host that is not unix.
    #[cfg(unix)]
    pub fn new() -> Result<Self, i32> {
        // SAFETY: flags only; no memory is passed.
        let fd = unsafe { sys::inotify_init1(IN_CLOEXEC | IN_NONBLOCK) };
        if fd < 0 {
            Err(crate::last_errno())
        } else {
            Ok(Self { fd })
        }
    }

    /// A new instance.
    ///
    /// # Errors
    ///
    /// Always [`ENOSYS`](crate::ENOSYS): the host has no Slate kernel to ask.
    #[cfg(not(unix))]
    pub fn new() -> Result<Self, i32> {
        Err(crate::ENOSYS)
    }

    /// Watch `path` for the events in `mask`, answering the watch's
    /// descriptor, which each of its events carries as [`Event::wd`].
    ///
    /// # Errors
    ///
    /// The `errno` set by `inotify_add_watch`: `ENOENT` when `path` does
    /// not exist, `ENOTDIR` with [`IN_ONLYDIR`] and a path that is not a
    /// directory, `ENOSPC` when the instance has all the watches it may.
    #[cfg(unix)]
    pub fn watch(&self, path: &CStr, mask: u32) -> Result<i32, i32> {
        // SAFETY: `CStr` guarantees a NUL terminator, and `path` outlives the
        // call, so the library reads a valid C string and nothing past it.
        let wd = unsafe { sys::inotify_add_watch(self.fd, path.as_ptr().cast::<u8>(), mask) };
        if wd < 0 {
            Err(crate::last_errno())
        } else {
            Ok(wd)
        }
    }

    /// Watch `path`.
    ///
    /// # Errors
    ///
    /// Always [`ENOSYS`](crate::ENOSYS).
    #[cfg(not(unix))]
    pub fn watch(&self, _path: &CStr, _mask: u32) -> Result<i32, i32> {
        Err(crate::ENOSYS)
    }

    /// Wait up to `timeout_ms` for something to read -- `-1` waits for ever
    /// -- answering whether there is. An interruption counts as nothing yet.
    ///
    /// # Errors
    ///
    /// The `errno` set by `poll`.
    #[cfg(unix)]
    pub fn wait(&self, timeout_ms: i32) -> Result<bool, i32> {
        let mut fd = sys::PollFd {
            fd: self.fd,
            events: sys::POLLIN,
            revents: 0,
        };
        // SAFETY: one `pollfd`, owned by this frame and alive for the call,
        // and the count says one.
        let n = unsafe { sys::poll(&raw mut fd, 1, timeout_ms) };
        if n < 0 {
            let errno = crate::last_errno();
            return if errno == sys::EINTR {
                Ok(false)
            } else {
                Err(errno)
            };
        }
        Ok(n > 0 && fd.revents & sys::POLLIN != 0)
    }

    /// Wait for something to read.
    ///
    /// # Errors
    ///
    /// Always [`ENOSYS`](crate::ENOSYS).
    #[cfg(not(unix))]
    pub fn wait(&self, _timeout_ms: i32) -> Result<bool, i32> {
        Err(crate::ENOSYS)
    }

    /// Read what has happened into `buf`, without blocking: nothing yet is an
    /// empty [`Events`]. `buf` should hold at least one record with a full
    /// name -- [`HEADER`] plus 256 bytes -- or a long name cannot be read.
    ///
    /// # Errors
    ///
    /// The `errno` set by `read`, `EAGAIN` and `EINTR` excepted.
    #[cfg(unix)]
    pub fn read<'b>(&self, buf: &'b mut [u8]) -> Result<Events<'b>, i32> {
        // SAFETY: `buf` is a live, writable slice and the count is its
        // length, so the library writes only within it.
        let n = unsafe { sys::read(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            let errno = crate::last_errno();
            return if errno == sys::EAGAIN || errno == sys::EINTR {
                Ok(Events::new(&[]))
            } else {
                Err(errno)
            };
        }
        let n = usize::try_from(n).unwrap_or(0).min(buf.len());
        Ok(Events::new(buf.get(..n).unwrap_or_default()))
    }

    /// Read what has happened.
    ///
    /// # Errors
    ///
    /// Always [`ENOSYS`](crate::ENOSYS).
    #[cfg(not(unix))]
    pub fn read<'b>(&self, _buf: &'b mut [u8]) -> Result<Events<'b>, i32> {
        Err(crate::ENOSYS)
    }
}

impl Drop for Inotify {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // SAFETY: `fd` is this instance's own descriptor, opened by
            // `new` and closed only here, once. A failed close leaves nothing
            // to do: the descriptor is released either way.
            let _ = unsafe { sys::close(self.fd) };
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test indexes and sizes what it just built, in small constants"
)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    /// One record as the kernel lays it out: the header, then the name
    /// NUL-padded to `padded` bytes.
    fn record(wd: i32, mask: u32, cookie: u32, name: &[u8], padded: usize) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&wd.to_ne_bytes());
        out.extend_from_slice(&mask.to_ne_bytes());
        out.extend_from_slice(&cookie.to_ne_bytes());
        out.extend_from_slice(&u32::try_from(padded).unwrap_or(0).to_ne_bytes());
        out.extend_from_slice(name);
        out.resize(HEADER + padded, 0);
        out
    }

    #[test]
    fn the_records_of_one_read_are_read_in_order_with_their_names() {
        let mut bytes = record(1, IN_MOVED_TO, 7, b"notes.yaml", 16);
        bytes.extend(record(1, IN_MODIFY, 0, b"calendar.yaml", 16));
        bytes.extend(record(-1, IN_Q_OVERFLOW, 0, b"", 0));
        let events: Vec<_> = Events::new(&bytes).collect();
        assert_eq!(
            events,
            [
                Ok(Event {
                    wd: 1,
                    mask: IN_MOVED_TO,
                    cookie: 7,
                    name: b"notes.yaml"
                }),
                Ok(Event {
                    wd: 1,
                    mask: IN_MODIFY,
                    cookie: 0,
                    name: b"calendar.yaml"
                }),
                Ok(Event {
                    wd: -1,
                    mask: IN_Q_OVERFLOW,
                    cookie: 0,
                    name: b""
                }),
            ]
        );
    }

    /// A name exactly as long as its padding has no NUL at all, and a name
    /// is whatever bytes the file system holds -- not text.
    #[test]
    fn a_name_filling_its_padding_or_not_text_is_read_whole() {
        let bytes = record(3, IN_CREATE, 0, b"exactly-16-bytes", 16);
        assert_eq!(
            Events::new(&bytes).next(),
            Some(Ok(Event {
                wd: 3,
                mask: IN_CREATE,
                cookie: 0,
                name: b"exactly-16-bytes",
            }))
        );
        let odd = record(3, IN_CREATE, 0, b"caf\xe9", 8);
        assert_eq!(
            Events::new(&odd).next().map(|e| e.map(|e| e.name.to_vec())),
            Some(Ok(b"caf\xe9".to_vec()))
        );
    }

    /// A record cut short is reported once and ends the reading: the rest of
    /// the buffer cannot be trusted to start where a record starts.
    #[test]
    fn a_record_cut_short_is_malformed_and_ends_the_reading() {
        let mut bytes = record(1, IN_MODIFY, 0, b"a.yaml", 16);
        let whole = bytes.len();
        bytes.extend(record(1, IN_MODIFY, 0, b"b.yaml", 16));
        bytes.truncate(whole + HEADER + 3);
        let events: Vec<_> = Events::new(&bytes).collect();
        assert_eq!(events.len(), 2);
        assert!(events[0].is_ok());
        assert_eq!(events[1], Err(Malformed));
        // A header alone, cut short.
        assert_eq!(Events::new(&[0u8; 5]).collect::<Vec<_>>(), [Err(Malformed)]);
        assert_eq!(Events::new(&[]).next(), None);
    }

    /// On this host there is no Slate kernel, and the answer says so rather
    /// than pretending to watch.
    #[cfg(not(unix))]
    #[test]
    fn without_a_slate_kernel_nothing_is_watched() {
        assert_eq!(Inotify::new().map(|_| ()), Err(crate::ENOSYS));
    }
}
