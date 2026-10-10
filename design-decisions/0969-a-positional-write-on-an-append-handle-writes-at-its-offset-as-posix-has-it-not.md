## 969. A positional write on an append handle writes at its offset, as POSIX has it -- not at the end, as Linux does

**Date:** 2026-09-26. **Lane:** A. **Decided by:** Claude (autonomous) -- lane D's
request left the choice to lane A explicitly.

**In short:** a program can open a file "for appending" -- every ordinary write
then goes to the end -- and it can also write at a position it names (`pwrite`).
When a program does both, one of them has to give way. POSIX says the named
position wins. Linux lets the append win, and documents that as a bug it keeps
for compatibility. SlateOS gives the named position the win, on both the native
call and the Linux one.

**The question.** `SYS_FS_PWRITE` (1080/1081, lane D's
`d-a-positional-file-read-and-write`) writes at an explicit offset without
moving the handle's position. On a handle opened `APPEND`, should it write at
that offset, or append?

**Options.**

| option | pro | con |
|---|---|---|
| **Offset wins (POSIX)** -- chosen | Matches the standard; `pwrite` means what it says; matches this kernel's Linux `pwrite64` since it was written, so the native and Linux calls agree | A program ported from Linux that relied on Linux's append-anyway behaviour writes at its offset instead |
| Append wins (Linux) | Bug-compatible with Linux, which glibc programs run against | Contradicts POSIX; would make the native call disagree with this kernel's own Linux call unless that changed too; the behaviour Linux itself lists under BUGS |

**Why the offset.** Two answers from one kernel would be the worst outcome, and
this kernel's Linux `pwrite64` already answered: its helper writes at the
offset (`fs::handle::write_at`), and a comment says so. Changing it to append
would change behaviour for every Linux-ABI program already running on SlateOS to
match a documented Linux bug. The number of programs that open a file
append-only and then deliberately `pwrite` to it expecting an append is small,
and they are relying on something Linux's own manual calls wrong.

**Revisit if** a real ported program is found depending on Linux's behaviour.
The change is one line in `fs::handle::write_at`'s append handling, plus this
entry.
