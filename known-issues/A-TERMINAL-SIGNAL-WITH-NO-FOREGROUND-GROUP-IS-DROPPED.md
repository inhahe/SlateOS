## A-TERMINAL-SIGNAL-WITH-NO-FOREGROUND-GROUP-IS-DROPPED (lane A, 2026-09-16) — **Status: OPEN**, instrumented, hypothesis not yet confirmed
**Status:** SUPERSEDED 2026-09-24 (lane A). The section below headed "RESOLVED: the child is never scheduled; nothing is wrong with the pty" is **wrong**: a `^C` written to a pty master was only ever classified by a *reader*, and `ctest-pty`'s child never reads after announcing readiness, so no signal could be raised. Root cause and fix: `A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER` at the end of this file. The drop-with-no-foreground-group branch this entry is named for is real, still prints, and is not a bug (Linux drops the signal too).

**In short:** a `^C` typed at a terminal that has no registered foreground process group is thrown away, and the reader that consumed it is then told to *restart*. The byte is gone, no signal was sent, and nothing will ever arrive -- so the read spins forever. Until 2026-09-16 that left no trace anywhere: the drop was a bare `return`.

**Where.** `syscall/handlers.rs::signal_foreground_group`:

```rust
let pgid = crate::tty::foreground_pgid(tty);
if pgid == 0 {
    return;          // <- no diagnostic, and the caller restarts
}
```

The caller is `deliver_console_signal`, whose very next statement is `restart_result(ERESTARTSYS)`. So the sequence is: line discipline reads `0x03`, sees `ISIG` and `VINTR == 3`, correctly decides a `SIGINT` is due, returns `ConsoleRead::Signal(2)` -- and the delivery step finds nobody to deliver to, says nothing, and restarts the reader.

`foreground_pgid(id)` is `pcb::ctty_fg_pgrp(id).unwrap_or(0)`. That `unwrap_or(0)` is where the information is lost: *no session holds this terminal* becomes the same value a caller would read as *nothing to do*.

**How it surfaced.** `ctest-pty`'s first real run, 2026-09-16, after the rung had been switched off across four disable cycles (see design-decisions 944). It exited **45**, not the historical blanket 44 -- 45 means the parent's `waitpid(WNOHANG)` spin of 2,000,000 iterations never saw the child become reapable. The serial then shows `[pty] master closed: SIGHUP+SIGCONT to group 203`, i.e. the parent gave up and exited, which closed the master. **The child never returned from its read.** An unbounded restart loop explains that exactly.

**This is a hypothesis and is labelled as one.** Everything above is a reading of the code plus one exit code; nothing has yet observed the `pgid == 0` branch being taken. So the change committed today is *only instrumentation*: the branch now prints which signal and which tty it dropped. That print is the discriminator, and the two outcomes say different things:

| next boot shows | means |
|---|---|
| the line, naming the pty's id | the fault is in **acquiring the terminal** -- `login_tty`'s TIOCSCTTY/tcsetpgrp did not take effect -- and the line discipline is exonerated, having decided correctly |
| nothing | the hypothesis is **wrong**; the child is blocked somewhere else and this branch is not on the path |

**Why behaviour was not changed in the same commit.** Two candidate fixes exist and picking between them needs the measurement above. Returning an error instead of a restart stops the spin but changes semantics for a console that legitimately has no session during early boot; fixing `login_tty` instead leaves the restart loop in place for the next caller to fall into. Changing behaviour now would also destroy the evidence -- a boot that no longer loops cannot tell me whether it was looping for this reason.

**The shape, for the record.** A silent early return that makes "nobody is listening" indistinguishable from "delivered", feeding a caller that retries on the assumption something happened. Same family as 942's rows: the verdict -- here, the restart -- survived the disappearance of its own evidence.

### 2026-09-16 round 1: THE HYPOTHESIS ABOVE IS REFUTED

The print fired **zero times**. `ctest-pty` returned 45 again, reproducibly, and `signal_foreground_group`'s `pgid == 0` branch was never taken. The table above committed in advance to what that means, so it is settled rather than argued: **the terminal signal is not being dropped for want of a foreground group.**

Recording it here rather than deleting the section, because the reasoning was sound and the conclusion was wrong, and those are different things. It is also the reason only a print was committed: a fix would have changed behaviour on a guess and destroyed the evidence that the guess was wrong.

**What round 1 narrowed, which is the useful part.** Two facts now bracket the fault:

1. **The byte reaches the input ring.** `ctest-pty` returns 44 when the master write fails, and it returned 45 -- so `write(fm, "\003", 1)` returned 1.
2. **Nothing ever decided a signal was due.** `signal_foreground_group` was not reached at all, which is upstream of delivery, not inside it.

So the question is whether the line discipline SEES the byte, and with `ISIG` on when it does.

**Round 2 instruments `sig_for`**, which already computes `isig` and `vintr` and is therefore the cheapest place to ask. It exists **twice** -- in `raw_try_read` (non-blocking) and `raw_read` (blocking) -- which an assertion caught before anything was written, so both are instrumented and labelled. Three outcomes, the third being the absence of any line:

| next boot shows | means |
|---|---|
| `saw VINTR ... isig=true` | a signal WAS decided; the loss is downstream of `sig_for` |
| `saw VINTR ... isig=false` | the slave's termios has `ISIG` off |
| nothing | the byte never reached the discipline; the fault is the master-to-slave input path |

A fourth outcome is possible and worth naming: the line appearing under
`raw_read` rather than `raw_try_read` would mean the fixture is not
reading the way its own header says it does.

### 2026-09-16 round 2: ZERO HITS, AND THE ZERO PROVED NOTHING

The probes reported **no VINTR sightings**, and by the table above that
reads as *the byte never reached the discipline*. **It does not, and the
table was wrong to offer it.** Verified the instrument first -- the probe
string is present twice in the staged kernel `build/esp/boot/kernel`,
matching the built binary, with a positive control -- so the probes did
run. The problem is that they do not cover the subject.

`ConsoleRead::Signal` is returned from **four** sites. `sig_for`, which
round 2 instrumented, serves `raw_try_read` and `raw_read` only. The other
two come via `LineStep::Signal`, produced by `step()` -- a separate
classifier with its own `ISIG`/`vintr` logic. **A pty slave in canonical
mode (the `sane_default`) reads through `step()` and never touches
`sig_for` at all**, so silence from the raw probes is exactly what a
correctly working canonical path looks like.

Round 3 instruments `step()` as well, labelled `canonical`. With all three
sites covered, silence from all three finally means what round 2's table
claimed silence from two meant.

**Third instance in one investigation of reading a subset's silence as
absence** -- after `nm` returning 0 from a binary that was never opened,
and five true procfs checks about the wrong subject. The common step is
not carelessness about the result; it is not asking what the instrument's
coverage was before interpreting its quiet.

### 2026-09-16 round 3: THE BYTE ARRIVES AND A SIGNAL IS DECIDED

The canonical probe fired **three times**:

```
[tty] canonical: line discipline saw VINTR (0x03) isig=true
```

Instrument verified before the result was read -- three probe strings in
the staged kernel `build/esp/boot/kernel`, one canonical-specific, with a
positive control. So this is a real positive, and the first in the
investigation.

It also confirms round 2's zero was a **coverage gap and not a null
result**: every hit is on the canonical path, the site `sig_for` does not
serve. Without round 3, that zero would have been recorded as "the byte
never reached the discipline" and sent the search to the master-to-slave
path, which is empty.

**Five links are now settled, and the child still never returns from its
read:**

| link | settled by |
|---|---|
| the master write reaches the input ring | ctest-pty returning 45, not 44 |
| the discipline sees the byte | round 3 |
| `ISIG` is on | round 3 |
| a `SIGINT` is decided | round 3 |
| the foreground group is non-empty | round 1's silence at `pgid == 0` |

**Round 4 probes the next link: do the sends to the group members actually
succeed?** Nothing could tell, because the loop discarded every result:

```rust
// Best-effort: a member that exited between the membership snapshot
// and delivery just fails its own send; the rest still receive it.
let _ = sys_signal_send_with_info(&send_args, SI_KERNEL, 0);
```

That justification is correct **per member** and the wrong shape for the
whole. One member exiting is benign; *not one* send succeeding is the bug,
and a discarded `Result` reports both as silence -- a tolerated per-item
failure hiding a total failure.

Round 4 counts, and prints only when a non-empty group received nothing:
quiet on a healthy system, quiet on the benign case the comment describes,
loud on exactly the case that would explain exit 45. Printing every failure
would have buried the distinction in noise and told a reader nothing the
discarded result did not.

(`SyscallResult` is not a `Result` and has no `is_err`; it carries an `i64`
`value` whose negative range is the error code. Assuming the API from the
name cost one compile, which is the cheapest place to be wrong.)
