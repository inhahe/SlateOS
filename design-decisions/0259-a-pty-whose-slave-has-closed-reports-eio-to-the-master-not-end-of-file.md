## §259 — A pty whose slave has closed reports `EIO` to the master, not end-of-file

**Date:** 2026-08-21
**Decided by:** Claude (autonomous) — answering the semantics question lane B
delegated in `requests/b-a-pty-devices-need-the-line-discipline-that-the-console-already-has.md`
**Lane:** A

**In short:** A pseudo-terminal ("pty") is a fake terminal: a pair of connected
endpoints where one side — the *master* — is held by a terminal emulator or a
terminal-multiplexer window, and the other — the *slave* — is what the program
inside believes is its keyboard and screen. When the program inside exits, its
slave end closes, and the emulator holding the master is left reading from a
pipe with nobody on the far end. There are two plausible things to tell it:
"zero bytes, end of file" or "the error `EIO`". They lead to visibly different
programs, and the two cannot both be right, so this picks `EIO` — which is what
Linux does — and records why the choice is not merely "copy Linux".

### The situation

`master_read` in `kernel/src/tty/pty.rs` is the emulator's read. Three states
are possible when it is called:

| State | What it means | Answer |
|---|---|---|
| Output ring has bytes | The program printed something | those bytes |
| Ring empty, slave still open | The program hasn't printed *yet* | block / would-block |
| Ring empty, slave all closed | The program is gone | **this decision** |

The third row is the only one in question. Note that whatever we choose, any
bytes still in the ring must be delivered *first* — a program's last line of
output must not be swallowed by the fact that it exited immediately after
printing it. That part is not a tradeoff and the self-test pins it.

### The options

**Option A — return 0 (end of file).** *What changes:* an emulator sees the same
"stream finished" answer it would get from a closed pipe, and its ordinary
end-of-stream path closes the window.

**Option B — return `EIO`.** *What changes:* the emulator gets an error return,
and must treat that specific error as "the child is gone" rather than as a
failure worth reporting.

### The decision: B, `EIO`

Linux does this, but the reason to follow it here is that **the two failure
modes are not symmetric.**

- If we return `EIO` to a program that only understands `0`: it sees an
  unexpected error, most likely prints a diagnostic, and stops. Noisy, but it
  terminates.
- If we return `0` to a program that only understands `EIO`: many such programs
  treat a zero-length read as "nothing right now, try again" — which is exactly
  what a zero-length read means on a non-blocking fd — and spin forever at 100%
  CPU on a window whose child is already dead.

A spurious error message is a cosmetic bug. A hot spin on a dead window is a
resource leak that the user must kill by hand. When one option's worst case is
strictly less bad than the other's, that asymmetry decides it, and the fact
that Linux landed in the same place means every ported terminal emulator
already has the code path we are choosing.

### The secondary consequence

The opposite direction — *master* closed, slave reading — is **not** symmetric
with this, and is deliberately different: the slave gets `Data(0)`, a genuine
end of file. That is right because a program reading its own terminal and
finding it gone should see the same thing as a program reading a closed stdin,
and because the shell inside is additionally hung up: `close` collects the
foreground process groups on the device and the caller signals them. A program
that reads EOF from its terminal exits; that is the behaviour we want, and an
`EIO` there would instead make well-behaved programs report a fault on the
entirely ordinary event of a terminal window being closed.

### If this is ever revisited

The place to change is `master_read`'s `slave_refs == 0` arm and the assertion
in `pty::self_test` that pins it (`"master read after slave close"`). Lane B's
libc will grow the userspace half — `read(2)` on `/dev/ptmx` — and must map this
to `errno == EIO` rather than inventing its own convention, or a terminal
emulator will see one answer from the kernel and a different one from libc.
