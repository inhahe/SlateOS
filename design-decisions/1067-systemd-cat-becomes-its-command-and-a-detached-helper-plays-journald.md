## 1067. `systemd-cat` becomes its command, and a detached helper plays journald

**Date:** 2026-10-09
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `systemd-cat` runs a command with its output going into the
system journal. On Linux it does that by connecting the command's output to
journald, the journal's own service, and then turning *itself into* the
command, so the command keeps `systemd-cat`'s process number, signals and exit
status. SlateOS has no journald -- its journal is a file -- so something has to
stand in for it. This decides that our `systemd-cat` still turns itself into
the command, as upstream does, and starts a small background process per
output stream that does journald's job: reading what the command writes and
appending a record per line to the journal file.

### What it is

`systemd-cat` makes a `socketpair` for standard output (and one more when
`--stderr-priority` asks for a second stream), writes the header
`sd_journal_stream_fd` writes, puts the sockets on descriptors 1 and 2 as
`rearrange_stdio` does, and `execvp`s the command -- `/bin/cat` when none is
named. Before that, for each stream, it forks twice: the grandchild takes the
other end of the socket, starts a session of its own, puts its standard
descriptors on `/dev/null`, closes everything else it inherited, and reads the
stream to its end as journald's `stdout_stream_process` reads one -- the header
protocol, the line cuts, the trimming, the `<N>` prefixes, and each line filed
under the process that wrote it, from the credentials the kernel attaches to
what it reads (`SO_PASSCRED`, `SCM_CREDENTIALS`). The middle process exits at
once, so the helper is nobody's child but init's.

### Alternatives

**`systemd-cat` stays, runs the command as its child, and reads the pipes
itself.** For: simpler; records are all in the journal by the time
`systemd-cat` exits, so a script that reads the journal straight afterwards
never races. Against: everything the caller sees of the process changes. `$!`
names `systemd-cat` and not the command; a signal sent to it does not reach
the command unless forwarded, and a Ctrl-C reaches both; the exit status has
to be rebuilt, a death by signal re-raised; the command's parent is not the
shell. Upstream has none of that, because there is only one process. It also
makes every record the child's, where journald names the writer.

**One helper reading both streams.** For: one process instead of two when
`--stderr-priority` splits them. Against: it needs threads or `poll`, and
SlateOS's C library keeps its descriptor table for a single thread
(`posix/src/fdtable.rs`); journald itself treats each stream as its own
connection, so two helpers are the more faithful shape as well as the safer.

**A helper that is `systemd-cat`'s child, not detached.** For: one fork fewer.
Against: after the `exec` the helper is the *command's* child, and a command
that waits for all its children -- a shell's `wait`, any `while (wait(NULL) >
0)` -- would wait for the helper, which ends only when the command does.

### What it costs

Records may land a moment after `systemd-cat` exits, as they may on Linux,
where journald also files them asynchronously. `scripts/systemd-cat-diff.sh`
waits for the helpers by running each case in a PID namespace and waiting for
it to empty.
