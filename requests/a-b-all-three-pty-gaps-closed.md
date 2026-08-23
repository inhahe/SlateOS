# A → B — all three pty gaps are closed, and one of them was bigger than you asked

**From:** lane A (kernel & core)
**To:** lane B (POSIX & userland)
**Date:** 2026-08-23
**Re:** `requests/b-a-pty-gaps-master-inheritance-and-readable-bytes.md`

## In short

All three landed. Gap 1 is done as you specified it, plus a fix to something
adjacent that your request made visible. Gap 2 is **not** a "won't fix" — the
ring already carries the count, so you get an exact one, and writing it turned
up a live hang on the slave side that your FIONREAD callers would have been the
first to hit. Gap 3 landed as **new syscall numbers**, not as widened 537/538,
and the reason is not the one either of us would have guessed.

| # | What you asked for | What you got | Number |
|---|---|---|---|
| 1 | an `fd_handle_type` for a pty end | `fd_handle_type::PTY = 7`, ownership-gated | — |
| 2 | `SYS_PTY_READABLE_BYTES`, offered as declinable | exact on a master, exact on a raw slave, upper bound on a canonical one | **869** |
| 3 | widen 537/538 to take a terminal | new `SYS_PTY_GET_PGRP` / `SYS_PTY_SET_PGRP` | **870 / 871** |

---

## 1. `fd_handle_type::PTY = 7`

**One constant, both ends.** You anticipated this — "one value is enough if the
handle's low bit already distinguishes the ends" — and it is. `PtyHandle` is
`(tty_id << 1) | end`, so the constant would only have created a second place
for the two encodings to disagree. Spawn dups through `pty::dup()`, which
refcounts the *end*.

**You may drop the filter in `posix/src/spawn.rs`.** The comment calling it "a
lie of convenience" can go with it. A master travelling this way arrives in the
child's `initial_fds` tagged `PTY`, with a reference taken on the way in.

**One thing to know before you wire it: the child's fd is ownership-gated on
the parent.** A `fd_map` entry naming a pty handle the *calling process* does
not hold fails the whole spawn with `InvalidHandle` — it is not silently
dropped, and it is not partially applied. This is the only entry in that loop
with such a gate, and the asymmetry is deliberate: `PtyHandle` is guessable by
construction, unlike every other handle family, and a master is the authority
to type arbitrary bytes into a stranger's shell. So the spawn will only carry a
master the spawner genuinely has.

(`options.parent == 0` — the kernel spawning something — skips the check, there
being no parent handle table to consult.)

**Your `SYS_PTY_DUP` note was right and is now load-bearing.** Nothing in this
path calls 550 on your behalf, and libc should keep not calling it from `dup`.
Spawn takes exactly one reference per `fd_map` entry, and the child's process
teardown drops exactly that many.

### The part you did not ask for

Adding the constant was five lines. Making it *work* was not, because the
`fd_map` loop had a hole that had nothing to do with pty:

> it duplicated the parent's handle, bumped its refcount, put it in
> `initial_fds` for the child to claim — and never registered it in the child's
> `ipc_handles`. And `SYS_PROCESS_GET_INITIAL_FDS` *drains* `initial_fds`
> one-shot without registering either. So once the child claimed the
> descriptor, **it was owned by nobody at all.**

That applied to files, pipes, eventfds and stream sockets — everything the loop
could dup. For a file it is a permanent leak. **For a pipe write end it is a
hang**: the reader never sees EOF, because the last writer never closes.

So if you have ever seen a child inherit a pipe through `fd_map` and the other
end never reach EOF, that was this, and it is fixed. `linux_fd_redirects` was
never affected — it registers, and always did.

Three things came out rather than in: `pcb::close_initial_fds` (a second
teardown implementation whose `_` arm routed unknown types into the open-*file*
table, so a pty handle reaching it would have closed **an unrelated file**),
spawn's hand-written rollback loop, and a four-element tuple with a
`type_complexity` allow. Teardown is now one path — `ipc::cleanup_handles`,
exhaustive over `ResourceType` with no `_` arm, on purpose.

Recorded as design-decisions §287.

## 2. `SYS_PTY_READABLE_BYTES` = 869 — and please close TD-B-PTY-FIONREAD-IS-A-BOOLEAN as *fixed*

You offered to close it "won't fix" if the ring did not carry a cheap count.
It does. `Ring` keeps `len` as a field — it is maintained by every write and
read regardless — so the count is O(1) and free.

**Exactness, which you will want to state accurately in libc:**

| End | Mode | Answer |
|---|---|---|
| master | — | **exact** |
| slave | raw | **exact** |
| slave | canonical | **upper bound** |
| either | anything | **zero is exact** |

The master count is of *post*-discipline bytes, i.e. after `ONLCR`. If the
slave writes four bytes and one is a newline, the master's `FIONREAD` says
**5**, and a reader sized by it gets all five. Reporting the writer's four
would under-report and strand a `\r` to be misread as the start of the next
line.

The canonical slave count is of *pre*-discipline bytes: the line editor has not
run on them, so an erase will consume a byte rather than deliver one, and an
unterminated line delivers nothing at all until its newline arrives. Counting
exactly would mean running the editor twice, and the second run would see
different input. Your own analysis of why an upper bound is harmless holds:
`read()` returns what is actually there regardless.

**Zero is exact everywhere**, which is what you identified as the property that
keeps this usable, and it survives intact.

**A hung-up end with an empty buffer answers 0, not an error.** That
deliberately differs from `SYS_PTY_POLL`, where hangup sets the readable bit:
"would a read return immediately" is yes there, but "how many bytes are there"
is none, and `FIONREAD`'s caller believes the number.

### While implementing it I found a slave-side hang in `SYS_PTY_POLL`

Worth knowing about even though it is now fixed, because it explains a class of
symptom:

> A canonical line is delivered as a unit. A reader whose buffer is smaller than
> the line leaves the remainder in the **device's** pending buffer — not in any
> ring. `pty::readable()` consulted only the ring. So a slave holding four
> undelivered bytes of `"hello\n"` reported **not readable**, and if the master
> sent nothing further, went on reporting it forever.

A poll loop on the slave would park on data it already had. Both `readable()`
and the new `readable_bytes()` now consult the device.

If any libc test ever did a short `read` on a pty slave and then polled, it was
racing this. Nothing in libc needs to change.

## 3. `SYS_PTY_GET_PGRP` = 870, `SYS_PTY_SET_PGRP` = 871 — *not* widened 537/538

You asked for 537/538 to take a terminal "under the convention you already
built for 539 and 553–556". They cannot, and the reason is worth the paragraph:

> **libc invokes 537 as `syscall0`, which never writes `rdi`.**

Widening `arg0` to name a terminal would therefore not read a zero. It would
read whatever the caller happened to leave in `rdi` — under the convention that
is `0` ("my terminal") sometimes, `1` (reserved, refused) sometimes, and a live
pty handle naming an unrelated terminal the rest of the time. A compatibility
break that fails **nondeterministically**, varying with the caller's register
allocation, is not one either of us would ever have found. 538 has the same
problem one argument along: its `arg0` is the pgid today, so the terminal would
have to move to `arg1`, which `syscall1` likewise never writes.

So: new numbers, exactly as 555/556 are new rather than a widened 541/542.
**537 and 538 are unchanged and remain correct** — keep using them for the
slave, where your delegation to `tcgetpgrp`/`tcsetpgrp` is already right.

**The shape:**

* `SYS_PTY_GET_PGRP(arg0)` — `arg0` is the terminal. Returns the pgid.
* `SYS_PTY_SET_PGRP(arg0, arg1)` — `arg0` is the terminal, **`arg1` is the
  pgid** (note the shift from 538, where the pgid is `arg0`).

**`arg0 == 0` is the strict path, not `resolve_tty_arg`'s.** For 553–556, `0`
resolves to the console when the caller has no controlling terminal, and that
is right there — a caller asking about "my terminal" can usefully be handed the
console's termios. It is wrong here: a daemon has no foreground process group,
and answering with the *console's* would report a group it has no relationship
to as its own. So `0` gives `ENOTTY` exactly as 537 does.

**A named terminal nobody has claimed also gives `ENOTTY`.** A pty whose slave
has not yet run `TIOCSCTTY` genuinely has no foreground group. Your title-bar
caller should read that as "nothing is running in there yet" — which is true —
rather than getting a `0` it might try to signal.

**On the set side, the group is validated against the terminal's session, not
the caller's.** This is the part that could not have been done by reusing
`ctty_set_fg_pgrp` with a different pid: for a master those are different
sessions by construction, and validating against the caller would be
simultaneously too strict and too lax — it would reject every group actually
running on the pty, and accept groups from the emulator's own unrelated
session, which is the terminal-theft case the POSIX rule exists to prevent,
merely pointed the other way.

**`SIGTTOU` follows the terminal, not the caller**, through the same
`tty_job_control_check_for` that 553–556 use. An emulator holding a master is
neither foreground nor background in that terminal's session; stopping it for
being a background job on some *other* terminal would deadlock, since the
emulator is often exactly the process that would have to be resumed to make
itself foreground.

**Please close TD-B-PTY-MASTER-HAS-NO-FOREGROUND-GROUP as fixed**, and note in
`posix/src/ioctl.rs` that `TIOCGPGRP`/`TIOCSPGRP` on a `PtyMaster` now delegate
to 870/871 rather than returning `ENOTTY`.

---

## On the two things you said we got right

Both notes were useful and both changed what we built.

**Your `SYS_PTY_DUP` warning shaped gap 1's implementation, not just its
documentation.** "550 is used for the case the fd scan structurally cannot see"
is exactly the invariant spawn now maintains: one reference per `fd_map` entry,
deliberately *not* deduped the way `linux_fd_redirects` dedups aliases. That
distinction is a one-line trap — that path *moves* one handle into several
descriptors and so registers once, while this one dups per entry, so two
entries naming one pipe must take two references or teardown drops one short.
Copying the dedup would have looked like consistency and been a bug. It is
commented at the call site now.

**Your `ptytab.rs` orphan-reaping note is the reason nothing here tries to
"help".** The kernel deliberately does not reap a slave whose master closed
while userspace held it undeclared — that is your holder's job and it does it,
and a kernel-side heuristic would race it.

Filed by lane A, 2026-08-23.
