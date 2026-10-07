//! The process's supplementary groups, as the kernel keeps them.
//!
//! The kernel keeps a list of supplementary groups for each process.
//! [`setgroups`](super::setgroups) and `initgroups` install it through
//! `SYS_PROCESS_SETGROUPS`, and the kernel's file access gate consults it
//! (`check_path_access`, `kernel/src/fs/vfs.rs`). But the native ABI has no
//! call that reports it, as it has none that reports the hostname: its one
//! read path is `/proc/self/status`, whose `Groups:` line lists it.
//!
//! **The last `Groups:` line is the kernel's.** The file's first line is
//! `Name:` and the task's name, written raw, and a name may hold a newline:
//! a program run as `x\nGroups:\t0 27` has a `Groups:` line of its own
//! choosing ahead of the real one. Nothing a process controls follows the
//! real line, so each `Groups:` line restarts the list and the last one
//! stands. (Linux escapes the newline; making the kernel do so is lane A's,
//! `requests/d-a-proc-status-writes-the-name-raw-so-a-newline-in-it-forges-lines.md`.)
//!
//! A list that cannot be read is `EIO`, never an empty list: "no groups" is
//! a claim about the process, and nobody asked the kernel. Opening any file
//! takes a File capability, so a confined process that holds none cannot
//! read it, nor can one in a `chroot` without `/proc`; a native call would
//! need neither (`known-issues/D-POSIX-GETGROUPS-NEEDS-A-FILE-CAPABILITY.md`,
//! `requests/d-a-a-process-cannot-ask-for-its-own-supplementary-groups.md`).
//! A file with no well-formed `Groups:` line last is `EIO` too.
//!
//! The host build has no kernel and no `/proc`, and nothing can install a
//! group there ([`setgroups`](super::setgroups) answers `ENOSYS` once its
//! checks pass), so the host's list is empty.

use crate::errno;
use crate::types::GidT;

/// Where [`GroupsLine`] puts what it reads.
pub(crate) trait GroupSink {
    /// A `Groups:` line begins: what an earlier one listed no longer counts.
    fn restart(&mut self);
    /// One gid of the current line, in the line's order.
    fn gid(&mut self, gid: GidT);
}

/// The key of the line that lists the groups.
const KEY: &[u8] = b"Groups:";

/// Where in the text a [`GroupsLine`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    /// At the start of a line, with this many bytes of [`KEY`] matched.
    LineStart(usize),
    /// In a line that is not a `Groups:` line.
    OtherLine,
    /// In a `Groups:` line's list.
    List,
}

/// Reads the `Groups:` lines of a `/proc/<pid>/status` text, fed in pieces
/// of any size: the kernel's is `Groups:` and a tab, then decimal gids
/// separated by spaces (Linux's ends with a space too).
#[derive(Debug)]
pub(crate) struct GroupsLine {
    at: At,
    /// The gid being read.
    value: GidT,
    /// Whether `value` has a digit yet.
    digits: bool,
    /// Whether the current `Groups:` line is malformed: a byte that is
    /// neither a digit nor a separator, or a gid past `u32::MAX`.
    bad: bool,
    /// Whether a `Groups:` line has begun.
    seen: bool,
}

impl GroupsLine {
    pub(crate) const fn new() -> Self {
        Self {
            at: At::LineStart(0),
            value: 0,
            digits: false,
            bad: false,
            seen: false,
        }
    }

    /// Read `bytes`, the text's next piece.
    pub(crate) fn feed(&mut self, bytes: &[u8], sink: &mut dyn GroupSink) {
        for &b in bytes {
            self.at = match self.at {
                At::LineStart(n) => {
                    if KEY.get(n) == Some(&b) {
                        let n = n.saturating_add(1);
                        if n == KEY.len() {
                            self.begin_list(sink);
                            At::List
                        } else {
                            At::LineStart(n)
                        }
                    } else if b == b'\n' {
                        At::LineStart(0)
                    } else {
                        At::OtherLine
                    }
                }
                At::OtherLine if b == b'\n' => At::LineStart(0),
                At::OtherLine => At::OtherLine,
                At::List => self.list_byte(b, sink),
            };
        }
    }

    /// The text has ended. True if its last `Groups:` line was well formed
    /// -- the sink then holds what it listed -- and false if it was not, or
    /// if there was none.
    pub(crate) fn finish(mut self, sink: &mut dyn GroupSink) -> bool {
        if self.at == At::List {
            self.end_gid(sink);
        }
        self.seen && !self.bad
    }

    fn begin_list(&mut self, sink: &mut dyn GroupSink) {
        sink.restart();
        self.seen = true;
        self.bad = false;
        self.value = 0;
        self.digits = false;
    }

    fn list_byte(&mut self, b: u8, sink: &mut dyn GroupSink) -> At {
        match b {
            b'0'..=b'9' => {
                let next = char::from(b)
                    .to_digit(10)
                    .and_then(|d| self.value.checked_mul(10)?.checked_add(d));
                match next {
                    Some(v) => {
                        self.value = v;
                        self.digits = true;
                    }
                    None => self.bad = true,
                }
                At::List
            }
            b' ' | b'\t' => {
                self.end_gid(sink);
                At::List
            }
            b'\n' => {
                self.end_gid(sink);
                At::LineStart(0)
            }
            _ => {
                self.bad = true;
                At::List
            }
        }
    }

    fn end_gid(&mut self, sink: &mut dyn GroupSink) {
        if self.digits && !self.bad {
            sink.gid(self.value);
        }
        self.value = 0;
        self.digits = false;
    }
}

/// Read a `/proc/<pid>/status` text and feed its groups to `sink`. `next`
/// fills the buffer it is given and says how much it filled -- 0 at the
/// end, `None` for a failed read.
fn read_status(
    mut next: impl FnMut(&mut [u8]) -> Option<usize>,
    sink: &mut dyn GroupSink,
) -> Result<(), i32> {
    let mut text = GroupsLine::new();
    let mut buf = [0u8; 1024];
    loop {
        match next(&mut buf) {
            Some(0) => break,
            // `next` fills at most `buf.len()`; were it to say more, feeding
            // nothing is the safe reading of a broken answer.
            Some(n) => text.feed(buf.get(..n).unwrap_or(&[]), sink),
            None => return Err(errno::EIO),
        }
    }
    if text.finish(sink) {
        Ok(())
    } else {
        Err(errno::EIO)
    }
}

/// Feed the kernel's list of this process's supplementary groups to `sink`.
/// `Err(EIO)` when it cannot be read; the caller's `errno` is kept either
/// way.
#[cfg(target_os = "none")]
pub(crate) fn supplementary_groups(sink: &mut dyn GroupSink) -> Result<(), i32> {
    let saved = errno::get_errno();
    let fd = crate::file::open(
        b"/proc/self/status\0".as_ptr(),
        crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC,
        0,
    );
    if fd < 0 {
        errno::set_errno(saved);
        return Err(errno::EIO);
    }
    let read = read_status(
        |buf| loop {
            let n = crate::file::read(fd, buf.as_mut_ptr(), buf.len());
            match usize::try_from(n) {
                Ok(n) => return Some(n),
                Err(_) if errno::get_errno() == errno::EINTR => {}
                Err(_) => return None,
            }
        },
        sink,
    );
    // Nothing was written through `fd`, so a failed close loses nothing, and
    // the descriptor is gone either way.
    let _ = crate::file::close(fd);
    errno::set_errno(saved);
    read
}

/// The host's stand-in for the kernel's `/proc/self/status`: the host has no
/// kernel and no `/proc`, and nothing can install a group there (see the
/// module docs), so its list is empty.
#[cfg(not(target_os = "none"))]
const HOST_STATUS: &[u8] = b"Name:\thost\nUmask:\t0022\nGroups:\t\nThreads:\t1\n";

/// The host's list: [`HOST_STATUS`]'s, read as the target reads the
/// kernel's file.
#[cfg(not(target_os = "none"))]
pub(crate) fn supplementary_groups(sink: &mut dyn GroupSink) -> Result<(), i32> {
    let mut rest = HOST_STATUS;
    read_status(
        |buf| {
            let (now, later) = rest.split_at_checked(rest.len().min(buf.len()))?;
            buf.get_mut(..now.len())?.copy_from_slice(now);
            rest = later;
            Some(now.len())
        },
        sink,
    )
}

/// Counts the groups.
struct Count(usize);

impl GroupSink for Count {
    fn restart(&mut self) {
        self.0 = 0;
    }
    fn gid(&mut self, _: GidT) {
        self.0 = self.0.saturating_add(1);
    }
}

/// Finds one gid among the groups.
struct Member {
    want: GidT,
    found: bool,
}

impl GroupSink for Member {
    fn restart(&mut self) {
        self.found = false;
    }
    fn gid(&mut self, gid: GidT) {
        if gid == self.want {
            self.found = true;
        }
    }
}

/// Writes the groups to a caller's array of `room` entries, and counts
/// those past its end.
struct Fill {
    list: *mut GidT,
    room: usize,
    n: usize,
}

impl GroupSink for Fill {
    fn restart(&mut self) {
        self.n = 0;
    }
    fn gid(&mut self, gid: GidT) {
        if self.n < self.room {
            // SAFETY: `list` is non-null and holds `room` entries
            // ([`getgroups_with`] makes a `Fill` only then, on its caller's
            // word), and `n < room`.
            unsafe { self.list.add(self.n).write(gid) };
        }
        self.n = self.n.saturating_add(1);
    }
}

/// Whether `gid` is one of this process's supplementary groups. A list that
/// cannot be read holds no group.
pub(crate) fn in_supplementary_groups(gid: GidT) -> bool {
    let mut member = Member {
        want: gid,
        found: false,
    };
    supplementary_groups(&mut member).is_ok() && member.found
}

/// A count of groups as `getgroups` returns it. The kernel caps a list at
/// 65 536 (`NGROUPS_MAX`), far inside an `int`.
fn as_count(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// [`getgroups`](super::getgroups), with the groups from `read`.
///
/// Linux's order (`kernel/groups.c`): a negative `size` is `EINVAL`; a zero
/// `size` asks only how many; a `size` short of that is `EINVAL`; then the
/// copy, `EFAULT` for a NULL `list` -- but only if there is a group to copy.
///
/// Linux sorts a list when `setgroups` installs it (`groups_sort`), so its
/// `getgroups` answers in ascending order. The native kernel keeps the list
/// as given, so the copy is sorted here.
///
/// One difference, on a refusal only: Linux leaves `list` untouched when
/// `size` is too small, and this may have written its first `size` entries
/// by the time it knows -- each `Groups:` line is read once, as it comes.
pub(crate) fn getgroups_with(
    size: i32,
    list: *mut GidT,
    read: impl FnOnce(&mut dyn GroupSink) -> Result<(), i32>,
) -> i32 {
    let Ok(room) = usize::try_from(size) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if room == 0 || list.is_null() {
        let mut count = Count(0);
        if let Err(e) = read(&mut count) {
            errno::set_errno(e);
            return -1;
        }
        let n = count.0;
        if room == 0 || n == 0 {
            return as_count(n);
        }
        errno::set_errno(if n > room {
            errno::EINVAL
        } else {
            errno::EFAULT
        });
        return -1;
    }
    let mut fill = Fill { list, room, n: 0 };
    if let Err(e) = read(&mut fill) {
        errno::set_errno(e);
        return -1;
    }
    if fill.n > room {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: `list` is non-null and holds `room >= fill.n` entries (the
    // caller's word), the first `fill.n` of them written by `fill`.
    let got = unsafe { core::slice::from_raw_parts_mut(list, fill.n) };
    got.sort_unstable();
    as_count(fill.n)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests;
