## A-CTEST-COREUTILS-RUNS-EXIT-3-WAS-A-MISSING-BINARY-NOT-A-BROKEN-ONE (lane A, 2026-09-16) — **Status: NOT A KERNEL DEFECT**

**`ctest-coreutils-runs` exited 3 on its first run and it is not a finding about the kernel.** `/bin/true` is not on the image. `create-ext4-rootfs.sh` reported this during the rebuild, in plain words, and I did not read its output:

```
NOTE: none of the binaries named in scripts/rootfs-bin-manifest.txt have been
      built, so /bin gets none of this project's own utilities. They
      build for the HOST by default; the slateos target is separate
NOTE: 72 name(s) in the manifest have no built binary and were skipped: ...
```

I ran the rebuild, checked `ROOTFS_RC=0` and that the *fixtures* staged, then booted a test whose entire subject is `/bin`. The information was present, timestamped, and addressed to me.

**Two real defects sit underneath the mistake, though.**

**The fixture cannot say which it is.** Its check is `rc != 0`, and a failed exec makes the child `_exit(127)`, so 127 arrives as 3: *could not exec* and *ran and failed* are the same code. That is the collapse lane B themselves fixed in `ctest-pty`, where 47 had stood for four causes -- their note there reads "ONE CODE PER CHILD STATUS". Reported to them; the fixture is theirs.

**My diagnostic asserted the wrong subsystem.** It read *"our own ELFs do not exec, run or exit cleanly, which would explain every other ring-3 rung"* -- confident, specific, and pointing at the loader. The serial showed the fork succeeding and **no `ELF validated` line at all**, i.e. nothing was loaded. Rewritten to say it cannot distinguish the two cases and to send the reader to the rootfs log first: *a missing `/bin/true` looks exactly like a broken one.*

**RESOLVED 2026-09-16: the path was wrong. The rootfs mounts at `/mnt`.**

With all five producing crates built and 72 utilities staged with zero
skipped names, the rung still failed -- as lane B's new exit **11**, with
the serial saying `COULD NOT EXEC /bin/true -- it is not on the image, or
is not executable.` That claim is checkable, so I stopped trusting the
staging log and inspected the image:

```
$ debugfs -R "stat /bin/true" rootfs.ext4
Inode: 108   Type: regular    Mode: 0755   Size: 796064
```

Present, executable, byte-size-identical to the built binary -- which left
the path. And the serial says `[vfs] Mounted ext4 filesystem at '/mnt'`.
`create-ext4-rootfs.sh` stages into `$STAGE/bin`, i.e. `/bin` *inside the
image*, and the image mounts at `/mnt`, so the runtime path is
`/mnt/bin/true`. Five fixtures shared the fault; lane B fixed all five
behind a `BIN "/mnt/bin/"` macro and made the staging log name both forms.

**`/bin` means two different things depending on which side of the mount
you are on.** `rootfs-bin-manifest`, "staged into /bin", and
`execl("/bin/true")` are all true sentences about different sides of it.
The day's recurring shape, with the ambiguity in a **name** rather than in
an instrument.

**Exit 11 found this in four minutes where exit 3 would have sent me to
the loader**, and lane B's generalisation is the keeper: *diagnostic
precision is about falsifiability, not correctness.* Exit 11's claim was
**wrong**, and being wrong in a checkable way is what made it the fastest
route to the truth. A diagnosis narrow enough to disprove in one command
beats a broad one that happens to be true.

**Two retractions from resolving it.** I twice offered the absence of an
`ELF validated` line as evidence nothing was loaded; that line comes from
kernel-side `spawn_process`, not the `execl` syscall path, so it was never
evidence about exec -- and lane B repeated it back as settled, which is two
witnesses agreeing because one was quoting the other. And I chased a
capability theory first, because this rung passes `capabilities: &[]`
exactly as the keylayout one did: exec has no `(File, EXECUTE)` gate and
both matches are inside capability self-tests. A recent real cause distorts
the search order.

### 2026-09-16, second run: the path was fixed and it returned 11 again

With `/mnt/bin/true` asked for correctly, the rung still returned **11**,
now naming the right path. **Two independent faults had to be fixed before
this rung could answer its question**, which is why it took two rounds:
a wrong path, and a missing capability.

The second was settled by a control in the same boot rather than by
theory. `ctest-keylayout` holds `(File, READ)` and opened and read
`/proc/keylayout` from ring 3 successfully. `ctest-coreutils-runs` holds
`capabilities: &[]` and could not open `/mnt/bin/true` to exec it. Two
arms, one boot, one difference.

**This is the capability theory I reached for and dropped, and dropping it
was still correct.** What I checked for -- a `(File, EXECUTE)` gate on
exec -- genuinely does not exist; both matches are inside capability
self-tests. What I missed is that **exec must OPEN the file to read its
ELF**, and the open is what a process holding nothing cannot do. The theory
was right, the reason for rejecting it was right, and they were about
different steps. Granted `READ | EXECUTE` now, with the comparison written
at the site rather than the inference.

**A third case exists that the fixture's wording does not cover.** Exit 11
says "missing from the image or not executable". Both were false here: the
file is present at mode 0755 and the caller simply could not reach it. The
honest third arm is *present, executable, and unreachable by this caller* --
filed for the owning lane.

### 2026-09-16 RESOLVED: libc's `execl` passes a NULL path

Six rounds, and the answer came from the first thing I should have done:
asking the kernel what errno it returned.

```
[exec] linux_execve ENTERED and failed early: filename_ptr=0x0 errno=14
```

`frame.arg0` is `execve`'s filename pointer and it is **0x0**; errno 14 is
`EFAULT`. The kernel is correct -- `read_user_cstr(0)` must fail -- and it
fails *before* `linux_exec_common`, which is why round 5's probe wrapped
around that function never fired at all. That silence was the clue: it
meant the failure was upstream, not that nothing failed.

**The discriminator is in the same boot.** `fastpy-run` (pid 212) and pid
214 exec'd successfully -- `ELF validated for exec`, entry `0x29c1cc` --
using the vector form. `ctest-coreutils-runs` uses `execl`, the list form,
and lost its first argument. Same kernel, same boot, same image. `cat` (pid
211) is created fine through the kernel spawn path, so the binaries are
sound.

So **"staged is not run" was never a question about the Rust userland.** The
image is right, the binaries are right at mode 0755, the kernel execs
correctly. No C program on this system can exec by the list form --
`execl("/bin/sh", "sh", "-c", cmd, NULL)`, which is what most real code
writes. Filed for lane B, whose `posix/` owns it.

**The three theories that were wrong, and why they were expensive.**

| theory | disproved by | cost |
|---|---|---|
| not staged in the image | `debugfs`: inode 108, mode 0755 | one boot |
| wrong path | real, and fixed to `/mnt/bin` -- still failed | one boot |
| caller holds no capability | granted `(File, READ\|EXECUTE)` -- still failed | one boot |

Each was plausible and each was a guess wearing a diagnosis. The fixture
could only ever report *that* exec failed; **the errno existed in the kernel
the whole time and nothing printed it.** The lesson is not that the theories
were bad -- the second was even true, and had to be fixed -- but that a
measurement available from round one was deferred behind three rounds of
inference.

**Two probe-design points worth keeping.** Wrapping a function beats
instrumenting its error sites: `linux_exec_common` has eight or more
`return -i64::from(..)` paths, and a probe per site is eight chances to miss
the one that fires -- the same way `sig_for` covered two of four
classification sites and produced a zero I nearly called decisive. And a
probe that reports *entry* as well as failure is what turned round 5's
silence from ambiguous into informative.

### 2026-09-16 round 7: the trampoline IS set up — and I over-read it

The probe fired **56 times**, including once for pid 205, which is
`ctest-pty`'s forkpty child. I read that as *the child got the signal, ran
its handler, and exited*, and said so. **That was wrong, and two more lines
of context showed it:**

```
3328  [thread] Process 204 has no threads left — now zombie   <- THE PARENT, FIRST
3329  [pty] master closed: SIGHUP+SIGCONT to group 205
3330  [signal] Process 205 continued
3332  [sig] trampoline SET UP for pid Some(205)
3333  [thread] Process 205 has no threads left — now zombie
```

The **parent gives up and exits first**. The master fd then closes *because*
the parent died, which sends `SIGHUP+SIGCONT` to group 205 — so the
trampoline is **SIGHUP's, post-mortem**. The child never received the `^C`.

The mistake is the one this whole investigation is about: I had a line that
fitted the hypothesis and stopped, instead of asking what *else* produces
that line. The answer was three lines above it. A probe that fires for any
signal cannot tell you *which* signal fired it, and I did not make it say.

**What rounds 3 and 4 actually established, restated.** Neither was wrong;
both were weaker than I read them.

| round | what it proved | what I read it as |
|---|---|---|
| 3 | a `SIGINT` was **decided** by the discipline | (correct) |
| 4 | **somebody** received a delivery | the child received it |
| 7 | a trampoline was set up for pid 205 | the SIGINT handler ran |

`delivered > 0` says a send succeeded, not that the **right group** got it.
The pty child is its own group leader (`login_tty`/`setsid`), so its group is
205 — and a terminal signal delivered to a stale or otherwise wrong
foreground pgid succeeds, counts as success, and reaches nobody who cares.
**A count told me the send worked and could not tell me who it reached.**
That is the corpus problem in 942 applied to a destination rather than to a
population.

**Round 8 prints the target instead of counting it**: the foreground pgid,
the member count, and the member pids, on every terminal-signal delivery.
Terminal signals are rare, so it stays quiet. If the pgid is not 205, that is
the bug and it has been hiding behind three successful-looking measurements.

### 2026-09-16 round 8: it never fires — and that retracts rounds 1, 3 and 4

Round 8 printed the target group unconditionally on entry and fired **zero**
times. `signal_foreground_group` is never called for `ctest-pty` at all.

Rounds 1 and 4 both lived inside that function, so their silence never meant
what I read it as. And round 3's three VINTR sightings are at serial lines
**46834-46842** while the pty rung runs at **3316-3337** — they belong to
`[tty] Running self-test...` and `[pty] Running self-test...`, the kernel's
own line-discipline suites, 43,000 lines later.

| round | what I claimed | what was true |
|---|---|---|
| 1 | `pgid != 0`, so delivery proceeded | the function is never called |
| 3 | the discipline saw the `^C` | a *different* terminal did, much later |
| 4 | deliveries succeeded | the function is never called |
| 7 | the SIGINT handler ran | it was SIGHUP, after the parent died |
| 8 | — | confirms: never called |

**Every positive in this investigation was the same error: I correlated by
existence instead of by identity.** The probes answered *did this fire* and I
read them as answering *did this fire for the subject under test*. A probe
with no subject in its output cannot distinguish the two, and three of them
did not carry one.

That is worse than the silence problems recorded above it, and differently
shaped. A silent probe at least announces nothing; a probe that fires for the
wrong subject **manufactures a positive**, and a positive ends an
investigation where a silence only stalls it. Rounds 1, 4 and 8 cost me
boots. Round 3 cost me a conclusion.

**The rule, which is cheap and I was not following it:** a probe must print
the identity of the thing it is about — tty id, pid, handle, path — and a
reading must be correlated by position in the log, not by count. `grep -c`
answers *how many*, never *whose*. Both checks are one command.

**Where the trail actually stands.** For `ctest-pty`, nothing downstream of
the input ring has been observed at all: the discipline never classifies the
byte, so no signal is decided, so no group is looked up. The master write
succeeds (the fixture returns 45, not 44), so the byte reaches the ring.
Between those two facts is the whole remaining search space, and the most
likely occupant is that the child never performs the read that would drain
it.

**Round 9 instruments all three steps at once, each carrying an identity:**
`master_write` (which pty received the `0x03`), the slave read (did that
pty's child consume it), and the discipline's decision (on which tty). All
three gate on VINTR, so ordinary traffic stays silent, and together they
cannot produce an ambiguous quiet — whichever speaks last names the step that
failed.

The identity probe sits at `canonical_try_read`'s `LineStep::Signal` arm
rather than inside `step()`, and the compiler is why: `step()` takes
`(line, raw, t)` and has no tty id in scope. **That is the mechanical reason
round 3 could not name a terminal** — the information was never there. The
caller already holds `id`, so the probe moved rather than a signature
changing to carry instrumentation.

### 2026-09-16 RESOLVED: the child is never scheduled; nothing is wrong with the pty

With both write paths and all three read paths instrumented, the whole rung
window reads:

```
3323  [cow] Cloned address space: parent=0x3ad000 -> child=0x7e226000
3325  [thread] Spawned thread (task 174) in process 205
3326  [pty] master_TRY_write handle=PtyHandle(14): VINTR (0x03) entering the input ring
3327  [sched] Anti-starvation: cur=173 boosted 1 task to priority 0: [174(p16)]
3328  [sched] Anti-starvation: cur=173 boosted 1 task to priority 0: [174(p16)]
3329  [thread] Process 204 has no threads left — now zombie
3330  [pty] master closed: SIGHUP+SIGCONT to group 205
```

**The parent writes the `^C` immediately after forking, before the child has
ever been scheduled.** `cur=173` is the parent; the scheduler boosts the
starved child (task 174) twice, and the parent still exhausts its 2,000,000
iteration `waitpid` spin first, returns 45 and exits. The child then dies of
the `SIGHUP` the master's close sends — which is the trampoline round 7 saw
and I misread as the SIGINT handler.

**The byte sits in the ring the whole time and is never read, because the
child never reaches its read.** Nothing is wrong with the pty: the kernel's
own self-test drives `master_write` → `slave_read` → `discipline decided
signal 2` end to end in the same boot, on tty 9.

`main.rs` already carried the prediction, from an earlier investigation of
the mirror image: *"the child waits in a pure userspace spin … so it never
yields, while the parent needs three syscalls to reach its write; QEMU boots
single-CPU under TCG, so that busy-wait starves the one process that could
end it."* Same mechanism, roles reversed.

#### What it took

Eleven rounds, and every wrong turn was one of two errors. They are worth
separating because the fixes differ:

| error | instances |
|---|---|
| **subset coverage** — probed some paths, read the silence as absence | `sig_for` 2 of 4 sites; `linux_exec_common` wrapped below the failure; `master_write` 1 of 2; `slave_read` 1 of 3 |
| **no identity** — fired, but for another subject | round 3's VINTR hits were the kernel's own self-tests, 43,000 lines away |

Identity and coverage are independent and each was paid for separately. A
probe that names its subject can still miss the path the subject takes; a
probe on every path can still be attributed to the wrong subject.

**What finally worked was enumeration, not reasoning.** `grep -n
"input.read_byte()"` found three callers in one command, after three rounds
of deciding which path *should* be used. The same command earlier would have
found two `master_*write` paths and four `ConsoleRead::Signal` sites.

**And the answer was written down twice already.** `main.rs` carried both the
`O_NONBLOCK` → 1065 routing round 10 needed and the starvation mechanism that
resolves it, left by whoever investigated this before. Second time today an
unread artifact in this tree held what I was rediscovering; the first was
lane B's notice naming `/mnt`.

#### Where the fix belongs

Not in the kernel. The fixture writes its `^C` before the child can possibly
be ready, then bounds its wait by iteration count on a single-CPU TCG guest.
Filed for lane B, whose `services/ctest-pty` it is: the child should signal
readiness before the parent writes, rather than the two racing.
