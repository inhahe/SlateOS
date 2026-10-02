## Four limitations left behind by wiring libc's pty family to syscalls 544-556 (lane B, 2026-08-23) -- three FIXED 2026-08-24

All four were found while landing
`requests/a-b-pty-the-tty-layer-is-now-n-devices-and-a-pty-object-exists.md`
on the libc side. None of them is a bug in what lane A built; each is a
place where the Linux ABI has an operation our syscall set does not, and
libc had to pick the least-wrong answer. Three are filed to lane A as
`requests/b-a-pty-gaps-master-inheritance-and-readable-bytes.md`; the
fourth is ours and is a non-issue by design.

**All three filed gaps were closed by lane A on 2026-08-24**
(`requests/a-b-all-three-pty-gaps-closed.md`) and wired up in libc the same
day. The entries below are kept in full rather than deleted: each one records
*why* the least-wrong answer was chosen, and the fix notes appended to them
record why the obvious fix was in two of the three cases not the one that
landed. The fourth entry remains open and is expected to stay that way.
