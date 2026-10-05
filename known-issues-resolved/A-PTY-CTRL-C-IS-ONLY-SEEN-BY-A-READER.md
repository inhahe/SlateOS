### [A] `A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER` — `^C` typed into a pty could not interrupt a program that was not reading -- 2026-09-24
**Status:** FIXED 2026-09-24 in the kernel (lane A); awaiting the boot that shows `ctest-pty` pass.

**In short:** pressing Ctrl-C in a terminal window is how you stop the program
running in it. On SlateOS that only worked if the program happened to be
*reading from the terminal* at the time — and a program you want to stop is
almost never doing that; it is busy. The kernel only looked for the Ctrl-C when
the program next read its input. It now acts on it the moment it is typed, as
every Unix does.

**The mechanism.** The line discipline — the code that turns `0x03` into
`SIGINT`, echoes what you type and assembles lines — ran inside `tty::read`,
i.e. inside the *slave's reader*. `pty::master_write` only put the byte in a
ring. So a `^C` sat in that ring until somebody read the slave, and nothing
else ever looked at it.

`ctest-pty`'s child installs its handler, writes its readiness byte, and then
spins on `got_sigint` with `sched_yield()` — it never reads again, which is
precisely the situation `^C` exists for. The byte was never classified, no
signal was raised, and both processes spun their 2,000,000 iterations: the
parent's `waitpid` budget ran out first (exit **45**), and under a debug kernel
the same two spins outlast the 2400 s boot budget (the **hang** of 2026-09-22).

The module's own documentation already stated the requirement —
*"`^C` must be acted on when it is typed, not when somebody next calls `read`.
A line discipline running inside a reader only runs while a reader is in it, so
a program in a compute loop would be uninterruptible"* — and the code did not
implement it.

**Why eleven rounds of probes did not find it.** Every probe sat on a path the
`^C` takes *through a reader*, so the question they could answer was "which read
path consumes it". When none fired, the 2026-09-16 resolution read the absence
of a read as a scheduling fault ("the child is never scheduled; nothing is wrong
with the pty") and sent the fix to lane B's fixture. The "positive control" it
cited — the kernel's pty self-test driving `master_write` → `slave_read` →
`decided signal 2` — was no control for this: it performs the read the fixture
never performs. **A control that shares the subject's hidden assumption cannot
test it.** The question that finds the bug is not "which reader consumes the
byte" but "why does a reader have to exist at all".

**The fix — the discipline runs on arrival** (`kernel/src/tty/mod.rs`,
`kernel/src/tty/pty.rs`, `kernel/src/syscall/handlers.rs`):

- `tty::receive` processes one byte as it arrives: input translation, then
  `ISIG`, then canonical editing (`feed`) or the raw queue, plus echo. There is
  now **one** `ISIG` classifier; there were three (one in the canonical editor,
  two in the raw read paths), which is why the investigation had to instrument
  "all three sites".
- Finished bytes go into a per-device `InputQueue` (Linux's `read_buf` +
  `read_flags`): complete lines marked with their ends, `^D` as an end-of-file
  mark that is never delivered. Reads take from it through one waiting
  primitive (`wait_for`) and one policy per mode.
- `pty::master_write` runs `receive` for every byte it is given and returns
  `MasterWrite { written, signals }`; `SYS_PTY_MASTER_WRITE`/`_TRY_WRITE`
  deliver the signals to the foreground group before returning.

**Behaviour that changes with it — all of it Linux's behaviour:**

| | before | now |
|---|---|---|
| `^C` to a busy program | nothing, ever | `SIGINT` at once |
| echo of typed-ahead text | when the program next reads | as it is typed |
| `^C` flush | the line being edited | that line **and** complete lines not yet read |
| `FIONREAD` / poll on a canonical slave | an upper bound (a half-typed line counted) | exact (complete lines only) |
| a line typed to `MAX_CANON` | its `\n` could be lost | the last slot is kept for the terminator |
| `VEOL` / `VEOL2` | not recognised | end a line |
| a control character set to 0 | matched the NUL byte (`stty intr undef` made NUL a `^C`) | disabled |
| `ICRNL`/`INLCR`/`IGNCR` in raw mode | not applied | applied (they are input flags) |
| `TCSETSF` (Linux ABI) | same as `TCSETS` | also flushes unread input |
| `ICANON` switched with input unread | undefined | carried across, as `n_tty_set_termios` does |

**Not changed:** the console. See the next entry.

**Still owed:** the native ABI has no `tcflush`/`TCSAFLUSH` (todo.txt, "native
tcflush"); `VWERASE`, `VREPRINT` and `VLNEXT` are still unimplemented, as
before.
