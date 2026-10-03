## `TD-C-THE-TERMINAL-ECHOES-AND-RUNS-NOTHING` -- **FIXED 2026-09-18** (lane C)
**Status:** FIXED again 2026-09-24 by lane E -- the 2026-09-18 fix ran the shell on pipes, which is not a terminal (no prompt, no `^C`, no size, and a dropped stderr pipe that killed it); it now runs on a kernel pseudo-terminal. See `[E] The terminal's shell ran on pipes` at the end of this file.

**In short:** `apps/terminal` has no shell and starts no process. Typing works
and the characters appear -- the PTY's cooked-mode line discipline echoes them
locally -- and pressing Enter queues the line to a slave end that **nothing
reads**. Nothing in the window says so, and the module doc says the opposite.

**Verified.**

| | |
|---|---|
| processes started | **none.** `Command::new`, `spawn(` and `exec(` appear zero times in the crate |
| why typing still shows | `PtyInner` defaults to `PtyTerminalMode::Cooked`, whose line discipline "buffers input, echoes characters, and translates control keys" |
| where a line goes | `queue_to_slave`, into a `ByteChannel` with no reader |
| what the doc claims | *"keystrokes go to a child through `pty::PtyMaster` and its output comes back"* |
| what the window admits | nothing -- the only "cannot"/"nothing" strings in the crate are test assertion messages |

**The echo is what makes this worth filing rather than shrugging at.** An empty
window that does nothing reads as unfinished. A window that *responds to
typing* reads as working, so the first thing a user does is type a command and
press Enter -- and the silence that follows is indistinguishable from a command
that produced no output. `ls` returning nothing looks exactly like `ls` in an
empty directory.

**`pty.rs` is not the problem and should not be touched.** It is a careful,
complete implementation -- master/slave channels, line discipline, cooked and
raw modes, signal translation, queueing rather than dropping on a full channel
-- and its own doc is honest about the boundary: the emulator "can then deliver
these to the child process via the OS's ..." That sentence describes work not
done. `main.rs`'s doc is where it became a claim that it was.

**Same shape as `apps/editor`'s `use_spaces` comment**, filed hours earlier: a
doc sentence describing a mechanism that does not exist, written by somebody
wiring up the half that does. There it was "set when a file is read" over a
crate with no indent detection; here it is "keystrokes go to a child" over a
crate that starts no child. **Both were written truthfully about the
*intention* and read as claims about the *program*.**

**Worse than filed: it drew a shell prompt.** `main` fed
`"Welcome to Slate OS Terminal\r\n$ "`, so the window opened with a `$ `
waiting. **A prompt is not decoration; it is a claim that a shell is waiting
for a command** -- the single most direct way this app could assert the thing
it cannot do. Found only by opening `main` to place the fix.

**Fixed (2026-09-18):** the prompt is gone and the greeting says why -- "There
is no shell here. Nothing in this program starts a process, so what you type is
echoed and then goes nowhere. Silence after Enter is not a command that
produced no output." The module doc's "keystrokes go to a child" claim and the
absence of a real process remain; those are the larger half.

**The remaining half landed (2026-09-18).** `main` now starts the shell from
`$SHELL` (or `/bin/sh`) and bridges its pipes to the slave end of the PTY the
emulator already drains. Two threads copy bytes in each direction, because
`std` has no portable non-blocking read of a child's stdout -- a read must
block somewhere, and a thread is the only place it can block without stopping
the frame.

**The tested seam did not change**, which is why this adds no flake risk on a
night spent removing them: `drain_child` still reads the master and the
existing tests still write to the slave directly. The threads are a thin I/O
bridge with no new assertions hanging off them.

**Three outcomes, three things said**, because they are three different
situations for whoever is looking at the window:

| | |
|---|---|
| the shell started | nothing -- it will greet them itself |
| the shell failed | "No shell." plus the program and the error, then the echo warning |
| there is no pty | "No terminal device." -- nothing can be connected at all |

**What is verified and what is not, plainly.** The seam, the greeting logic and
both targets' clippy are checked; 126 tests pass. **That a real shell actually
appears in the window is not covered by a test** -- it needs a process, a
window and a shell on the machine running it, which is an integration test this
lane does not have. The failure path is the one that matters for honesty and it
is the one exercised on a host with no `/bin/sh`: the window says why.

**The feasibility check that preceded it, kept because it was the useful part.**
This entry first said spawning a process is "a much larger piece of work"
without establishing whether it was possible at all. It is:

| | |
|---|---|
| a shell to run | `userspace/shell` exists |
| spawning works here | 4 real sites -- `apps/launcher`, `apps/explorer`, `gui/desktop`, `apps/installer` |
| the pattern | `launcher::spawn_program` is `Command::new(path).spawn()`, whose own comment notes that waiting on the child "would make the launcher behave like a terminal" |

So the work is not "can a process be started" but **connecting a child's stdio
to the PTY that already exists**: piped stdin and stdout, a reader feeding
`feed()`, and a decision about process lifetime. `pty.rs` is already the right
shape for it -- master and slave ends, a line discipline, queueing rather than
dropping. It is a real piece of work with concurrency in it, not a small one,
and it is not blocked on anything.

**A hazard this entry created, worth naming.** The sentence above recording
that "`Command::new`, `spawn(` and `exec(` appear zero times" put those three
names *into the file*, so `apps/terminal/src/main.rs` now matches a grep for
the very thing it does not do. Checking which apps spawn processes returned
terminal as a hit, from this comment. **Documenting an absence makes the file
match searches for the thing that is absent** -- and the fix is the one this
file already prescribes: run such questions through
`rustlex.strip_noise(keep_literals=True)`, which blanks comments and left four
real sites out of six candidate files.

**What the repair wants, in order.** The window should say it has no shell --
one line, on the §862 pattern, since a terminal that silently swallows commands
is the most convincing wrong answer this app can give. The module doc should
describe the emulator it is rather than the one it will be. Spawning a real
process is a much larger piece of work and is not a prerequisite for either.
