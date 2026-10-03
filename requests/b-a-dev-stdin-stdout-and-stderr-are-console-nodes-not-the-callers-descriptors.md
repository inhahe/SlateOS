# B → A — `/dev/stdin`, `/dev/stdout` and `/dev/stderr` are console nodes, not the caller's descriptors

**Status:** OPEN

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
