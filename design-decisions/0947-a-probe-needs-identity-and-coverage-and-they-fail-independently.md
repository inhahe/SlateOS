## 947. A probe needs identity and coverage, and they fail independently

**Date:** 2026-09-16 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; paid for over eleven rounds of one investigation

**In short:** finding out why a `^C` never reached a test program took eleven
boot cycles. The bug was small and the kernel was innocent. Every wrong turn
was one of exactly two mistakes in how the *instruments* were built, and they
are independent -- fixing one does not protect against the other. This
records both, because the next long investigation will offer the same two
opportunities.

### The two errors

**Subset coverage.** A probe placed on some of the paths a thing can take,
whose silence is then read as absence. Four instances in one investigation:

| probe | coverage | what the silence seemed to mean |
|---|---|---|
| `sig_for` | 2 of 4 `ConsoleRead::Signal` sites | the discipline never saw the byte |
| `linux_exec_common` | wrapped, but the failure was upstream in `linux_execve` | exec never failed |
| `master_write` | 1 of 2 master-write paths | the byte never reached the ring |
| `slave_read_input_blocking` | 1 of 3 readers of `input.read_byte()` | the child never read |

**No identity.** A probe that fires, but for a different subject, and is read
as being about the one under test. One instance, and it was the expensive
one: a VINTR sighting with no tty id in its output. The three hits were the
kernel's own `tty`/`pty` self-tests at serial lines 46834-46842; the fixture
under test ran at 3316-3337. **43,000 lines apart, and indistinguishable by
`grep -c`.**

### Why they need separating

They are orthogonal, and each was paid for on its own:

* a probe that names its subject can still miss the path that subject takes;
* a probe on every path can still be attributed to the wrong subject.

Round 9 added identity to three probes and the very next round still lost a
day to coverage -- `master_write` was one of two. Fixing identity felt like
fixing the class, and it was not.

**They also differ in cost, and the cheaper-looking one is worse.** A
subset's silence *stalls* an investigation: it produces no claim, you notice
nothing happened, and you look again. A misattributed hit *ends* one: it
produces a positive, the positive is consistent with the hypothesis, and the
search moves downstream of a step that never happened. Rounds 1, 4 and 8 cost
boots. Round 3 cost a conclusion, and two further rounds built on it.

### The rules, which are cheap

1. **Print the identity of the subject** -- tty id, pid, handle, path,
   inode. A probe whose output cannot name what it is about cannot be
   correlated, only counted.
2. **Correlate by position, not by count.** `grep -c` answers *how many*,
   never *whose*. Get the subject's line range first, then read hits inside
   it. This is one extra command and it is what finally made the trail
   legible.
3. **Enumerate the paths mechanically before choosing where to probe.**
   `grep -n "input.read_byte()"` found three callers in one command, after
   three rounds of *reasoning* about which one the fixture "should" use. The
   same command earlier would have found two `master_*write` paths and four
   `ConsoleRead::Signal` sites. **Reasoning about which path is taken is how
   all four subset errors happened; enumerating is how none of them
   survived.**
4. **Prefer a wrapper to per-site probes** where one exists. A wrapper cannot
   miss a path, including one added later. But check what the wrapper
   actually encloses -- `linux_exec_common` was wrapped correctly and the
   failure was upstream of it, so the wrapper was honest and useless.
5. **Ask what else could produce this line.** Round 7's trampoline fired for
   the right pid and was `SIGHUP`, post-mortem, not the `SIGINT` under test.
   The disproof was three lines above it in the same log.

### What actually resolved it

Not any probe. `main.rs` already contained both the `O_NONBLOCK` routing that
round 10 needed and the starvation mechanism that explained the whole thing,
written by whoever investigated the mirror case earlier. **The answer was in
the tree before the investigation started.** That is the second time in one
day -- lane B's notice naming `/mnt` was three days old and addressed to this
lane.

So a sixth rule, and on this evidence the highest-yield one: **before
instrumenting, grep the tree for prose about the thing.** Comments, notices
and known-issues entries are cheaper to read than a boot is to run, and on
this project they are frequently already correct.
