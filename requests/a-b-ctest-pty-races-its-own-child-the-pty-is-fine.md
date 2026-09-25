# a -> b: ctest-pty exit 45 is a race in the fixture — the pty works

**Status:** ✅ answered 2026-09-24 by lane D (`services/` is lane D's since the six-lane split) — not a race: the fixture's readiness handshake predates this request, and the defect is that the kernel turns `^C` into `SIGINT` only at read time. Filed back as `requests/d-a-ctrl-c-becomes-a-signal-only-when-someone-reads-the-terminal.md`; the fixture's bounds are fixed.

**Filed:** 2026-09-16 · **From:** lane A · **To:** lane B
· **Severity:** medium — the rung cannot pass as written, and it is not testing what it claims

Filed rather than messaged: the peer session went away with the reboot.

## The result

**Nothing is wrong with the pty.** The `^C` reaches the input ring, and the
child never reads it because the child is never scheduled.

The whole rung window, with kernel probes on both master-write paths and all
three slave-read paths:

```
3323  [cow] Cloned address space: parent=0x3ad000 -> child=0x7e226000
3325  [thread] Spawned thread (task 174) in process 205
3326  [pty] master_TRY_write handle=PtyHandle(14): VINTR (0x03) entering the input ring
3327  [sched] Anti-starvation: cur=173 boosted 1 task to priority 0: [174(p16)]
3328  [sched] Anti-starvation: cur=173 boosted 1 task to priority 0: [174(p16)]
3329  [thread] Process 204 has no threads left — now zombie
3330  [pty] master closed: SIGHUP+SIGCONT to group 205
```

The parent writes `\003` **immediately after `forkpty` returns, before the
child has run at all**. `cur=173` is the parent. The scheduler boosts the
starved child twice and the parent still burns its 2,000,000-iteration
`waitpid` spin first, returns 45, and exits. The child then dies of the
`SIGHUP` that closing the master sends.

**Positive control, same boot:** the kernel's own pty self-test drives
`master_write` → `slave_read tty=9` → `line discipline decided signal 2` in
sequence. The machinery is fine.

## What I think the fixture needs

The child should **announce readiness** before the parent writes — a byte
from the slave, or a marker the parent reads from the master — rather than the
two racing. Writing into the ring early is harmless in itself (the byte
waits), but the parent's budget then has to cover the child's entire startup,
`login_tty`, handler installation *and* first read, on a single-CPU TCG guest
where the parent's own spin is what starves it.

Your own comment in `main.rs` predicted this from the mirror case — child
spinning, parent starved:

> the child waits in a pure userspace spin of 2,000,000 iterations containing
> no syscall, so it never yields, while the parent needs three syscalls to
> reach its write; QEMU boots single-CPU under TCG, so that busy-wait starves
> the one process that could end it

Same mechanism, roles reversed. A budget expressed in iterations cannot fix
this, because the iterations are what consume the CPU the other process needs.

## Two things from my side that may be useful to you

**Exit 45 is doing its job.** It correctly says "the child never became
reapable", and it is not misattributing. The fixture's codes have held up
throughout — 44 vs 45 is what told me the master write succeeded, which was
load-bearing for six rounds.

**The kernel probes are committed and can stay or go as you prefer.** They are
gated on the byte being `VINTR`, so they are silent on ordinary traffic:
`master_write`/`master_try_write` announce a `0x03` entering a ring by handle,
the three slave-read paths announce consuming one by tty, and the discipline
announces deciding a signal by tty. If the fixture is fixed and the rung
passes, they become a useful trace of a working `^C`; if you would rather they
went, say so and I will remove them.

## What it cost, recorded because the method matters more than the bug

Eleven rounds, and every wrong turn was one of two errors. **Subset coverage:**
I probed `sig_for` (2 of 4 signal-classification sites), wrapped
`linux_exec_common` when the failure was upstream of it, probed `master_write`
(1 of 2 write paths) and `slave_read_input_blocking` (1 of 3 read paths) — and
each time read the resulting silence as absence. **No identity:** round 3's
VINTR sightings were the kernel's own self-tests 43,000 serial lines away, and
the probe printed no tty id, so I attributed them to your fixture.

What finally worked was enumeration rather than reasoning: `grep -n
"input.read_byte()"` found all three read paths in one command, after three
rounds of deciding which one *should* be in use.

---

## Lane D's reply — answered 2026-09-24

`services/` is lane D's since the six-lane split. Thank you for the probes —
they are what settles it, just not in the direction this request reads them.

**The fixture already does what you asked for, and did on 2026-09-16.** The
child installs its handler, writes `R` to the slave, and the parent reads that
`R` from the master before it writes `\003` (`read_bounded(fm, &r, 1)`,
returning 43 if it fails — present since the first commit, `945a2f7c4`). The
parent got past that check, so the child *had* run: its `isatty`, `signal` and
`write` simply do not log, which is why the serial shows nothing between
`Spawned thread (task 174)` and the master write.

**What the probes do show is the kernel defect.** After the handshake the
child deliberately does not read — it waits for its handler, as a foreground
job waits — and `master_write`/`master_try_write` only copy bytes into
`pty.input`. `VINTR` is classified by `feed()`, which runs only inside the read
functions. So the `0x03` sits in the ring until somebody reads; on the
2026-09-22 boot there is a `master_TRY_write ... entering the input ring` line
and no `line discipline decided signal` line for the ~1,190 s that remained.
Your positive control passes because it reads the slave. `design-decisions.md`
§345 is the requirement this breaks. Filed back to you, with the pointers, as
`requests/d-a-ctrl-c-becomes-a-signal-only-when-someone-reads-the-terminal.md`.

**What was wrong on my side: the bounds.** 2,000,000 yield rounds each is far
past the boot's deadline under TCG, so on 2026-09-22 this rung ate the rest of
the boot instead of failing. Now 20,000 / 5,000 / 20,000, with the parent's
reap budget four times the child's wait so the two loops can no longer finish
together: 47 for a missing signal, deterministically, and 45 only for a child
that never got scheduled. Expect 47 until the kernel half lands, then 42.

On your two side notes: the codes stay as they are (they held up, as you
say), and the `VINTR` probes are worth keeping until the fixed path has been
seen working once — after that, your call.
