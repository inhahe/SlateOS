## ~~B-THREE-COPIES-OF-THE-KILL-CONVENTION-AND-ONE-HAD-DIVERGED~~ (lane B, 2026-09-10) — CLOSED

**Closed by extraction.** `killconv` holds the convention:
`exit_code_for_signal(SIGKILL) == 137`, with named constants and the inverse.
All nine call sites across `kill`, `pgrep` and `htop` now name a signal instead
of writing a number, and no bare `128 + N` literal remains in the lane.

**The syscall stub deliberately stayed put.** Moving inline assembly between
crates buys nothing: the three copies are identical and mechanical, and the
defect was never there. What was worth centralising is the arithmetic **nobody
can check by looking** — the difference between `9` and `137` is invisible at
every point a test can reach, because the value is only observable from the
parent of the killed process.

`killconv` carries five tests including the two that would have caught the
original bug: that `exit_code_for_signal(SIGKILL) != SIGKILL`, and that no
status in `0..=128` is ever read back as a signal death. It also saturates a
signal above 127 rather than wrapping into the range a program uses for its own
exit status — the same confusion in the other direction.

### Original entry

**In short:** `SYS_PROCESS_KILL` (506) takes a PID and an **exit code**, not a
signal number. The tree's convention for "killed by signal N" is the shell's
128+N. Three crates carry their own copy of that call site and their own
inline-asm syscall stub, and one of them had the wrong number.

| Crate | Passes | Correct? |
|---|---|---|
| `userspace/kill` | 143 / 137 / 129 / 130 (TERM/KILL/HUP/INT) | yes |
| `userspace/pgrep` | 143 / 137 / 129 / 130 | yes |
| `userspace/htop` | **9** | no — fixed to 137 on 2026-09-10 |

`htop`'s comment is where the confusion is visible: *"We pass exit code 9
(SIGKILL equivalent)"*. 9 is SIGKILL's **signal** number; as an exit code it is
indistinguishable from a program that exited normally with status 9. A process
killed from htop's process list reported the wrong thing to whatever was
waiting on it, and no test could see it because the value is only observable
from the parent of the killed process.

### Why this is filed rather than closed

The wrong number is fixed. **The reason it could be wrong is not.** Three
crates encode the same convention as bare literals at three call sites, each
with its own `core::arch::asm!("syscall", ...)` stub beside it. That is the
shape `sha1`, `hmac` and `civildate` were extracted to remove, and the
argument those extractions rest on — "every copy is correct today, which is N
pieces of luck rather than one" — is now demonstrably false here: the copies
were **not** all correct.

### What the fix looks like, and why it is not simply "another crate"

`monoclock`'s rule is the relevant one: the boundary belongs at *what the
number means*, not at *where it came from*. So the thing worth sharing is the
**convention** — named constants for the signal-death exit codes — rather than
the syscall stub, which is four lines and genuinely local. A crate holding
four `const`s is thin; a fifth divergent copy is worse. Extract when a fourth
caller appears, and name this entry when doing it.

**`userspace/fuser` deliberately did NOT become the fourth caller.** It is a
Linux-compatible tool whose `-s` argument is a signal, so it goes through
`kill(2)` — exported by `posix/src/signal.rs` as a C symbol and routed to
`SYS_SIGNAL_SEND` — which takes the signal number the user actually named. Two
mechanisms, two meanings; picking by what the caller said rather than by which
one was nearest is the point.
