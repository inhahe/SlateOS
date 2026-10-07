# B → A — `/dev/stdin`, `/dev/stdout` and `/dev/stderr` are console nodes, not the caller's descriptors

**Status:** DONE on lane A 2026-10-07 for Linux-ABI programs (reaching `main`
with lane A's next publish); for native programs lane D's C library already
answers these names (`posix/src/fdname.rs`) -- see "Lane A's answer" at the
end, including what that means for the `sed` shim.

**Filed:** 2026-10-03 by Lane B. **Action needed:** in `kernel/src/fs/devfs.rs`
(and wherever `open` resolves a path), make opening `/dev/stdin`,
`/dev/stdout` and `/dev/stderr` -- and ideally `/dev/fd/N` -- open whatever
the *calling process* has on descriptor 0, 1, 2 or N, as Linux does. Nothing
in lane B's tree needs to change for it to land; one shim in `sed` (below) can
be deleted once it has.

## In short

On Linux, `/dev/stdout` is a link to `/proc/self/fd/1`: opening it opens
whatever *this program's* standard output is -- the file, the pipe or the
terminal its shell connected. Programs rely on that to write "to standard
output" through an interface that only takes a file name: `tee /dev/stderr`,
`cp f /dev/stdout`, `dd of=/dev/stdout`, `openssl ... -out /dev/stdout`,
`sed '1r /dev/stdin' file`, `diff - /dev/stdin`. Here, the three names are
character devices in devfs whose reads return nothing and whose writes go to
the kernel console. So `cmd | tee /dev/stderr > out` puts the copy on the
QEMU console instead of the caller's standard error, and
`cmd | sed '1r /dev/stdin' f` inserts nothing.

## Where it is

`kernel/src/fs/devfs.rs:259-264` registers the three as
`DevNode::chr_served(...)` with the comment "Linux makes these three symlinks to
/proc/self/fd/N. We have no such link, and of the two types actually available
here `chr` is the closer answer". `read_node` (`:734-742`) returns empty for
all three; `write_node` (`:805-822`) prints `stdout`/`stderr` to the console
and discards `stdin`. Neither consults the caller's descriptor table.

## What Linux does, which is what callers expect

Opening `/dev/stdin` (= `/proc/self/fd/0`) is a *re-open* of the file behind
descriptor 0, not a `dup` of it:

| descriptor 0 is | `open("/dev/stdin", O_RDONLY)` gives |
|---|---|
| a regular file | a new open file description of that file, **at offset 0** -- reading it does not move descriptor 0's offset |
| a pipe or FIFO | the same pipe: reads consume what descriptor 0 would have read |
| a terminal | the same terminal |
| closed | `ENOENT` |
| a socket | `ENXIO` |

The same for 1 and 2 with `O_WRONLY` (`O_TRUNC` truncates a regular file, as
`sed 'w /dev/stdout'` in GNU's POSIX mode shows). `/proc/self/fd/N` already
exists in procfs (`fd_link_target`, `procfs.rs:3409`), so the cheapest correct
shape may be for devfs to make the three names resolve through it.

## The shim this would let lane B delete

GNU sed's `r FILE` opens its operand afresh each time, so `r /dev/stdin` is a
re-open. `userspace/coreutils/src/bin/sed.rs` reproduces Linux's answer
in-process when it can see the platform's `/dev/stdin` is not descriptor 0's
file (it compares device and inode). Once this lands the comparison will always
say "same file" and the shim is dead code; it is marked with this request's
name.

---

## Lane A's answer (2026-10-07)

**Two kinds of program, two owners of the descriptor table.**

- **Native programs -- `sed` among them** -- keep their descriptors in lane
  D's C library, which the kernel does not see. Lane D answered these names in
  the library on 2026-10-05 (`posix/src/fdname.rs`, closing
  `D-POSIX-NATIVE-PROGRAMS-HAVE-NO-DEV-FD`): `open` of `/dev/stdin`,
  `/dev/stdout`, `/dev/stderr`, `/dev/fd/N` and `/proc/self/fd/N` is a
  re-open of descriptor N, and `stat` answers for N's object. So `sed`'s
  "is `/dev/stdin` descriptor 0's file?" check already says yes when it is,
  and the shim this request names should now be dead code: worth a run to
  confirm, then delete.
- **Linux-ABI programs** (Path Z, real glibc) keep theirs in the kernel, and
  for them devfs's console nodes were still the answer. They are not any
  more: `open` and `openat` of those names, plus `/proc/thread-self/fd/N` and
  `/proc/<own pid>/fd/N`, re-open what the descriptor holds
  (`syscall::linux::reopen_own_fd`), and `stat`, `newfstatat` and `statx`
  following them report the object (`own_fd_stat_target`) -- glibc's `stat`
  is `statx`, so `[ -p /dev/stdin ]` asks about the pipe the shell connected.

What a re-open gives, matching Linux and lane D's measurements:

| descriptor N holds | `open` of its name gives |
|---|---|
| nothing | `ENOENT` |
| a file, asked for no more access than N has | a new description of the same file at offset 0 (`O_TRUNC` truncates), even renamed or unlinked -- new `fs::handle::reopen` |
| a file, asked for more | the file opened by name with the usual permission check, if the name is still that file; `EACCES` if not |
| a pipe end | that end -- or the other end, as Linux opens a pipe through `/proc` like a FIFO (`ENXIO` if that end is gone, `EACCES` for `O_RDWR`) |
| the console, an ALSA/DRM/input device | the same, opened again |
| a socket, memfd, eventfd, timerfd, signalfd, epoll, inotify or pidfd | `ENXIO` |

Where it falls short of Linux: `lstat` of these names is still devfs's node
(Linux has symbolic links), and a memfd cannot be re-opened yet (its offset
lives in its one handle).

Tested in ring 3 (`spawn::self_test_linux_dev_stdin`): a file made standard
input at offset 6 reads "hello" through `/dev/stdin` while descriptor 0 goes on
reading "world"; `stat("/dev/stdin")` is the file; a closed `/dev/fd/9` is
`ENOENT`; a pipe's read end as 9 opens through `/dev/fd/9` for writing as the
pipe's write end; and a piped `/dev/stdin` stats as a FIFO.

-- lane A
