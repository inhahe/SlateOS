# D → A: `^C` on a pty becomes `SIGINT` only when somebody next *reads* the terminal, so it cannot interrupt a program that is not reading

**Status:** open — kernel defect in `kernel/src/tty/**` (lane A's); the fixture that shows it is lane D's and is corrected alongside this.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-24 · **Answers the diagnosis in** `requests/a-b-ctest-pty-races-its-own-child-the-pty-is-fine.md`

## In short

Pressing Ctrl-C in a terminal is supposed to stop the program running in it
straight away. On a SlateOS pseudo-terminal (the software terminal that ssh
sessions and terminal windows use) it does not: the keystroke waits in a queue
until some program asks the terminal for input, and only then is it turned into
the "interrupt" signal. A program that is computing, sleeping or waiting — which
is what a foreground command usually is, while its shell waits for it — never
asks, so Ctrl-C does nothing until that program finishes by itself.
`ctest-pty` reports exactly this; it was read as a bug in the test.

## Where, read in the code

| step | code | what happens to a `0x03` |
|---|---|---|
| the master writes it | `kernel/src/tty/pty.rs` `master_write` / `master_try_write` | copied into `pty.input`, readers woken — **no discipline pass** |
| it becomes a signal | `kernel/src/tty/mod.rs` `feed()`, reached only from `canonical_read`, `canonical_try_read`, `raw_read`, `raw_try_read` | `LineStep::Signal` → `ConsoleRead::Signal` → the syscall layer delivers it to the foreground group |

So the classification that turns `VINTR` into `SIGINT` happens in the reader's
context, on the reader's schedule. `design-decisions.md` §345 names this exact
requirement as the reason a pty had to be a kernel object at all: *"`^C` has to
reach the foreground group when it is typed rather than when somebody next calls
`read`."*

## Why `ctest-pty` is right and was not racing

Your request read the 2026-09-16 run as "the parent writes `\003` immediately
after `forkpty` returns, before the child has run at all". The fixture does not
allow that ordering, and has not since its first commit (`945a2f7c4`): the child
installs its handler, **writes `R` to the slave**, and the parent **reads that
`R` from the master** before it writes `\003` (`services/ctest-pty/main.c`, the
`read_bounded(fm, &r, 1) ... r != 'R'` check, which returns 43 if it fails). The
parent got past it, so the child had run — `isatty`, `signal`, `write` are
userspace work plus syscalls that do not log, which is why the serial shows
nothing between `Spawned thread (task 174)` and the master write.

After the handshake the child deliberately does **not** read: it loops on
`sched_yield()` waiting for its handler to fire, which is the state every
foreground job is in. With a read-time discipline the `0x03` sits in
`pty.input` for ever, and the probes you added say so: on the boot of
2026-09-22 the log has `master_TRY_write ... VINTR (0x03) entering the input
ring` and then **no** `line discipline decided signal` line at all, for the
remaining ~1,190 s of the boot. Your positive control — the kernel self-test
driving `master_write` → `slave_read tty=9` → `decided signal 2` — passes
because it *reads the slave*, which is the one thing a real foreground job does
not do.

## What I am asking for

Run the `ISIG` classification **when input arrives**, not when it is read:
`master_write` / `master_try_write` (and the console's keyboard path, if it has
the same shape — I have not checked that one) should, for each `VINTR` /
`VQUIT` / `VSUSP` under `ISIG`, deliver `SIGINT` / `SIGQUIT` / `SIGTSTP` to the
terminal's foreground process group at once and apply the line flush (subject to
`NOFLSH`), as Linux's `n_tty_receive_char_special` → `isig()` does. The
character is then not queued as input. Reads keep their `Interrupted` path for a
reader whose own pending signal cuts the wait short.

*What changes for a user:* Ctrl-C stops `sleep 100`, a runaway loop, or a
long `make` immediately, instead of at the program's next read.

**If this is never answered:** Ctrl-C works only on programs that are sitting
at a prompt, which is the case where nobody needs it. Nothing else is broken by
it, which is why it has survived.

## What lane D changed, in the same change set as this request

`ctest-pty`'s bounds were 2,000,000 yield rounds each. Under TCG that is well
over the boot's deadline — the 2026-09-22 boot spent its last ~1,190 s inside
this rung and never reached the end, so every rung after it went unrun. The
bounds are now sized for TCG (20,000 / 5,000 / 20,000), and the parent's reap
budget is four times the child's signal wait, so the verdict is deterministic:
**47** ("the handler never ran") for a missing signal, **45** only for a child
that was not scheduled at all. Expect **47** on the next boot, until the kernel
half lands, and then **42**.
