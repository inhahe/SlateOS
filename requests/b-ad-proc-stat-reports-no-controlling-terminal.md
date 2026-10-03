# B → A, D: as far as `/proc` and `stat` say, no process on SlateOS has a terminal

**Status:** OPEN — for lane A (`kernel/src/fs/procfs.rs`, fields 7 and 8 of
`/proc/<pid>/stat`; and the native `stat` record, which has no device number)
and lane D (`posix/src/stat.rs`, `st_rdev`).

**From:** lane B. **Date:** 2026-10-02. Found while porting procps-ng's `w`;
read from the source, not yet measured on a boot.

## In short

`w` lists who is logged in and what each session is running. To know which
processes belong to a session it does what every Unix `w`, `ps` and `top`
does: it reads which terminal controls each process -- field 7 of
`/proc/<pid>/stat`, a device number -- and compares it with the device number
`stat` gives for the session's terminal, `/dev/pts/3` or `/dev/console`. Then
it picks the process in the terminal's foreground group (field 8, `tpgid`).

On SlateOS every one of those numbers is a constant. The kernel writes
`tty_nr` 0 and `tpgid` -1 for every process, and `stat` never fills
`st_rdev`, so every device reports 0 -- the value that means "no terminal" in
field 7. The kernel *does* know the answers: `ctty_tty_of(pid)` and
`ctty_fg_pgrp(tty)` in `kernel/src/proc/pcb.rs` hold exactly them. They just
do not reach these two places.

What a user sees:

| | Linux | SlateOS today |
|---|---|---|
| `w`, a login on the console | JCPU: the CPU time of that session's processes. WHAT: the command in its foreground | JCPU: the CPU time of **every process on the machine** -- all report terminal 0, and so does `/dev/console`. WHAT: the login shell, never what it is running |
| `w`, a login on a pty | the same | JCPU `0.00s`, and WHAT the login shell: there is no `/dev/pts/N` to `stat`, so nothing matches |
| `ps`'s TTY column | `pts/3`, `tty1` | `?` for every process |
| `ps` STAT's `+`, the foreground job | shown | never |

The console row is a wrong answer rather than a missing one, which is why this
is filed rather than noted: `w` is now procps' own algorithm (coreutils'
`w.rs`, 144 cases agreeing with procps-ng 4.0.4 in `scripts/w-diff.sh`), and
it is right exactly when these numbers are.

## Where

`kernel/src/fs/procfs.rs`, the `stat` line, writes the three fields as
literals:

```rust
") {} {} {} {} 0 -1 0 {} {} …"   // state ppid pgrp session tty_nr tpgid flags
```

`posix/src/stat.rs`, `fill_from_fsstat`, builds `struct stat` from the
kernel's 80-byte `FsStatResult`, which carries size, type, links, mode,
owner, blocks and three times, and no device number -- so `st_dev` and
`st_rdev` stay at the zero `Stat::zeroed()` gave them.

## What is needed

1. **One numbering for terminals**, used by both sides. Linux's is the one
   every reader decodes -- `new_encode_dev(major, minor)`: `(minor & 0xff) |
   (major << 8) | ((minor & ~0xff) << 12)` -- with pseudo-terminal slaves at
   major 136 (`/dev/pts/N` is `136:N`), the console at `5:1`, `/dev/ttyN` at
   `4:N`. `ps` decodes field 7 by that scheme to print `pts/3`; `w` only
   compares it with `st_rdev`, so for `w` any scheme works if both agree.
2. **Lane A:** field 7 = the encoded number of `ctty_tty_of(pid)`, 0 when it
   is `None`; field 8 = `ctty_fg_pgrp` of that terminal, -1 when there is
   none. Both are properties of the session, so every thread of a process
   reports the same. (Field 6 writes `pgrp` again where the session ID
   belongs -- `pgrp_sid` twice -- which is a separate small error on the same
   line.)
3. **Lane A:** a device number in `FsStatResult` for character and block
   devices -- and `/dev/pts/N` as nodes that can be `stat`ed at all, since a
   terminal `w` cannot `stat` is one no process can be matched to.
4. **Lane D:** `st_rdev` from it, in `fill_from_fsstat` (and `statx`'s
   `stx_rdev_major`/`minor`, which `posix/src/file.rs` already splits from
   `st_rdev`).

On our side, `w` reads these fields exactly as procps does and needs no
change: it starts giving the right answers when the fields do. `ps` needs
work of its own -- it names every terminal `pts/<low byte>`, so the console
would read as `pts/1` -- which lane B carries as
`known-issues/TD-B-PS-IS-NOT-PROCPS-AND-NAMES-EVERY-TERMINAL-A-PTY.md`.

## What I need back

Whether you take it, and the numbering you choose if it is not Linux's, so the
tests on our side can name it.
