## D-POSIX-NATIVE-PROGRAMS-HAVE-NO-DEV-FD — a native program cannot open `/dev/fd/N`, and its `/dev/stdin`, `/dev/stdout` and `/dev/stderr` are the console rather than its own descriptors (lane D, 2026-10-05)

**Status:** OPEN — the C library's to fix; bash is built to cope meanwhile.

**In short:** on Linux, `/dev/fd/3` names "my file descriptor 3", and
`/dev/stdin` "my standard input", whatever that is -- a pipe, a file, a
terminal. Programs and shell scripts lean on that: `diff <(sort a) <(sort b)`
hands `diff` two such names, and `echo oops > /dev/stderr` means "to my error
output". On SlateOS a native program -- one linked against our C library,
which is every program on the image but the Linux-ABI ones -- gets neither:
there is no `/dev/fd`, and `/dev/stdin`, `/dev/stdout` and `/dev/stderr` are
the console, so output a script sent to its own redirected stderr through
`/dev/stderr` lands on the screen instead.

**Why:** a native program's descriptors are the C library's, a table over
capability handles that the kernel does not see. The kernel serves
`/proc/<pid>/fd/N` only for Linux-ABI processes (TD21; `kernel/src/fs/procfs.rs`,
"for others ... each `<n>` resolves to NotFound"), and devfs makes `stdin`,
`stdout` and `stderr` console nodes (`kernel/src/fs/devfs.rs`: "We have no such
link"). FIFOs, the other way to hand a program a pipe by name, cannot be made
either: `mknodat` answers `ENOSYS` for one, the filesystem holding no FIFO
(`posix/src/stat.rs`).

**Who it bites:** bash's process substitution (`<(...)` and `>(...)`), which
needs `/dev/fd` or FIFOs and has neither, so it fails: "cannot make pipe for
process substitution: Function not implemented". bash's own redirections to
these names work since 2026-10-05, because its configure is told they are
absent and bash then opens them itself, as descriptors
(`scripts/bash-spike/cross2.sh`). Any other program that opens one of these
names gets the console, or `ENOENT`.

**The proper fix:** the C library resolves these names itself, since only it
knows the descriptors. `open` and `openat` -- and `fopen`, `freopen` through
them -- of `/dev/fd/N`, `/dev/stdin`, `/dev/stdout`, `/dev/stderr`,
`/proc/self/fd/N` and `/proc/<own pid>/fd/N` duplicate descriptor N (or 0, 1,
2): the semantics of BSD's `fdescfs`, which agree with Linux's for pipes,
sockets and terminals and differ only in that a regular file's offset is
shared rather than reopened. `stat` and `access` of them answer for the
descriptor, so `[ -p /dev/stdin ]` asks the right question, and `/dev/fd`
lists the open descriptors. Then bash's `bash_cv_dev_fd` and
`bash_cv_dev_stdin` go back to `standard` and `present`, and process
substitution works without FIFOs.
