### [A] Three ring-3 self-tests have red-flagged every boot for days, and the guard that exists to prevent exactly this is green -- 2026-09-21
**Status:** ALL THREE ROOT-CAUSED 2026-09-24, in the kernel and in lane A's own rungs: `ctest-coreutils-runs` (11) and `ctest-python-repl` (8) were spawned without (File, METADATA), which libc's exec needs to stat the binary (`A-TWO-RUNGS-COULD-NOT-EXEC-FOR-WANT-OF-METADATA`); `ctest-pty` (45) was the line discipline running only inside a reader (`A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER`). Fixes committed; the boot that shows them green is pending. The text below, including its correction, predates this.
Correction at the end of this entry, written within the hour. The binary is
on the image. The failure is a kernel-side exec, which is lane A's

**In short:** every boot fails three tests. The reason is that one small
program is missing from the disk image the tests run against. There is a
check whose whole job is to catch a stale or wrong image, and it passes --
because the list it checks against does not include that program.

**The failure, from the serial log rather than from inference:**

```
[cu] COULD NOT EXEC /mnt/bin/true -- it is not on the image, or is not executable.
[cu] This is NOT a finding about the Rust userland. Check that
[cu] create-ext4-rootfs.sh actually staged the manifest binaries.
[spawn]   FAIL: ctest-coreutils-runs (ring 3) exit code was Some(11), expected 42
```

Three tests fail together and the first is the one that matters: *our own
userland runs at all*, then *pty ^C signal delivery*, then *CPython REPL
over a pty*. The harness comment beside the first says why they are one
finding and not three -- if `/bin/true` cannot exec, the rungs below it are
exercising the same broken path from further away, and passing would be
worse than failing because it would look like evidence.

**Why nothing caught it, which is the part worth keeping.**
`boot-test.sh` has a guard built for precisely this, and its own comment
states the failure mode:

> The result is not a missing warning, it is a FALSE GREEN: the Path-Z
> rungs run, they pass, and what they exercised was last week's binary.

It ran and said: `ok rootfs.ext4 (85 staged artifacts match the tree, every
fixture recipe has one)`. Both halves true. `rootfs.ext4.manifest` has **93
entries and zero mention of `bin/true`** -- its header says it covers "the
ctest/fastpy ELFs, the ported binaries, and CPython's stdlib zip". The
coreutils binaries the rungs *exec* are not in that population.

So the guard answers **"do the staged fixtures match the tree?"** and the
failing test needs **"is `/bin/true` on the image?"**. A guard written to
prevent a false green has one of its own, one level down, and for the same
reason: a verdict is only as good as its corpus, and the corpus is
invisible in the output (dd-942).

**It is not simple staleness, which is what makes it interesting.** The
image is *newer* than the binary: `rootfs.ext4` was packed 2026-09-18
03:44, `target/x86_64-slateos/release/true` was built 2026-09-16 04:11.
The binary existed, the image was made afterwards, and it still is not
there. So the staging step did not silently lag -- it did not stage it.

**Why it has survived days.** `boot-test.sh` does not build the image; it
is a separate manual `wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`.
So the only feedback is a boot that goes red 600 seconds in with
`InternalError`, and the diagnostic that names the real cause is 6 lines
above it in a 2.7 MB serial log. Everything upstream is green.

**Fix, in two halves that belong to different lanes:**

| half | owner | what |
|---|---|---|
| stage the binaries | lane B | `create-ext4-rootfs.sh` / `ctest-fixtures.py` -- either stage them or say why they are absent |
| widen the corpus | lane A | the guard should assert the image contains what the rungs will **exec**, not only what the fixture recipes **produce**. Those are different lists and only one of them is checked |

The second half is the durable one: even after the image is rebuilt, the
same gap lets the next missing binary through to a 600-second failure.

#### CORRECTION, same day: the binary is there, and this is mine

I verified the image with `debugfs` instead of believing the test's own
error message, and the first half of the entry above is false:

```
    109  100755 (1)  1000  1000  796064  18-Sep-2026 03:44 true
```

`/bin/true` is present in `rootfs.ext4`, mode 0755, **796064 bytes -- the
same size as `target/x86_64-slateos/release/true`**, in a `/bin` holding
114 entries. The image is correct and lane B staged it correctly.

**How I got it wrong.** The fixture prints:

> `COULD NOT EXEC /mnt/bin/true -- it is not on the image, or is not
> executable.`

That is a **disjunction, and a guess** -- the fixture cannot see why exec
failed, so it names the two likely causes. I took the first branch and
wrote it up as a finding, including a confident story about manifest
corpora. The manifest observation stands on its own (it really does not
cover `/bin/true`), but it was not the cause of anything, and presenting it
as the explanation made a true fact into a false diagnosis.

**What the serial log actually shows**, six lines the entry above quoted
without reading:

```
[ext4] Mounted vdb at /mnt                      <- the mount worked
[cu] true (exec, run, exit 0 -- ...)
[cow] Cloned address space: parent=... -> child=...   <- the fork worked
[sched] Spawned task 153 (priority 16, cpu 0)
[thread] Process 184 has no threads left - now zombie <- died immediately
[cu] COULD NOT EXEC /mnt/bin/true ...
```

Mount succeeded, fork succeeded, the child was spawned and exited without
running the program. So this is an **exec failure in the kernel** on a
present, executable, correctly sized ELF -- `kernel/**`, lane A, mine. Not
a staging gap and not lane B's to fix.

**The lesson is the one I have been writing all day, pointed at me.** The
fixture's message was true about what it could observe (exec returned an
error) and a guess about why. I read the guess as the finding. dd-953 calls
this a check reporting something true about the wrong thing; here the check
was honest and explicitly offered two branches, and I collapsed them.

**Three ring-3 tests fail together and they are one finding**, exactly as
the harness comment beside them says: if `/bin/true` cannot exec, the pty
and CPython rungs below are exercising the same broken path from further
away. Fixing the exec should clear all three.

#### SECOND rediscovery: lane A filed this too, on 2026-09-16, with better evidence

`requests/a-b-ctest-pty-races-its-own-child-the-pty-is-fine.md`. Same day
as the execl one, same lane, same dropbox. Everything below this heading I
derived today was already there, and the prior note has a fact I never
reached:

> The parent writes `\003` **immediately after `forkpty` returns, before the
> child has run at all**. `cur=173` is the parent. The scheduler boosts the
> starved child twice and the parent still burns its 2,000,000-iteration
> `waitpid` spin first, returns 45, and exits.

It was measured with kernel probes on **both master-write paths and all
three slave-read paths**, and its conclusion is stronger than mine: not
"the budgets are equal so it is a photo finish" but *"nothing is wrong
with the pty -- the `^C` reaches the input ring and the child is never
scheduled to read it"*.

That also sharpens the open question. The fixture's own comment says the
readiness byte exists so *"the parent's 0x03 can never arrive before there
is something to catch it -- this is the whole synchronisation of the
test"*. If the parent is writing before the child has run, that
synchronisation is not holding, which is a more specific defect than equal
spin budgets and is still lane B's file.

**Two rediscoveries in one session, both from `requests/a-*.md`.** 149
outgoing requests filed by lane A, and I have never searched them. The
rule I am writing down, because "remember to look" is not one: **before
investigating any failing rung, grep `requests/a-*.md` for its name.** It
costs one command and it would have saved two investigations today.

What I add to the prior note rather than repeat: the two fixtures that
fail by `execl` are the only two `execl` callers in the tree, and
`ctest-pty` -- the third failing rung -- does not exec at all, so the
harness's "these three are one finding" grouping is wrong in a way that
will make an `execl` fix look incomplete.

#### `ctest-pty`: exit 45 is masking exit 47, and the cause is equal spin budgets

No probe needed -- it is arithmetic. `SPIN` is 2,000,000 and **both sides
use it**:

```c
child:   for (long i = 0; i < SPIN; i++) { if (got_sigint) _exit(77); sched_yield(); }
         _exit(78);                      /* handler never ran */
parent:  for (long i = 0; i < SPIN && w <= 0; i++) { waitpid(WNOHANG); sched_yield(); }
         if (w != kid) return 45;
```

If the `SIGINT` never arrives, the child must burn all 2,000,000
iterations before it can report 78. The parent is yielding in lockstep, so
it burns its own 2,000,000 over the same wall-clock and gives up at
essentially the same instant. **The parent cannot outlast the child by
construction**, so the child's verdict is unreachable whenever the signal
does not arrive.

That is why the scheduler's anti-starvation boost appears: it is not the
cause, it is the scheduler doing its job on a pair of tasks that are
yielding at each other two million times.

**So 45 is standing in for 47, and 47 is the one that matters.** The
fixture's own table calls it *"THE INTERESTING ONE -- the line discipline
did not turn 0x03 into a SIGINT that crossed into the child"*, and it is
kernel-owned. Every run so far has reported the race instead of the
verdict.

**There is an irony worth recording**, because it is the same defect one
turn of the screw along. That exit table was rewritten precisely to stop
47 meaning four things at once -- the comment says so: *"47 meant 'the ^C
did not become a SIGINT' and also 'isatty said no' and also 'signal()
refused' ... and no run could tell which"*. The ambiguity was fixed and
the reachability was not, so the disambiguated code cannot be reached.
**A code that cannot fire is as uninformative as one that means four
things.**

**The ask is one constant**, and it is lane B's file: give the parent a
larger budget than the child (`SPIN * 2`, or make the child's loop shorter).
Then a missing SIGINT reports 47 and lands in lane A's court with a real
diagnosis instead of a race.

#### `ctest-pty`: the yield chain is correct end to end, so the starvation is elsewhere

Walked it rather than assuming, because "yield does not really yield" is
the obvious first theory and it is wrong here:

| step | what it does |
|---|---|
| `posix::sched_yield()` | `syscall1(SYS_SLEEP, 0)` -- note: **not** `SYS_YIELD` |
| `sys_sleep(0)` | *"Zero sleep -> just yield"*, calls `sched::yield_now()` |
| `yield_now()` | counts a voluntary switch, reports an RCU quiescent state, `schedule_inner(true, Voluntary)` |
| `schedule_inner(requeue=true)` | `PER_CPU_SCHED.enqueue(current_id, prio, cpu)` -- back of its priority level |

The `SYS_SLEEP(0)` spelling is surprising when `SYS_YIELD` exists two
constants away, but it is not a defect: it lands on the same `yield_now()`.

**So the remaining question is a real scheduler one.** Both sides spin with
`sched_yield()` -- the parent on `waitpid(WNOHANG)`, the child waiting for
its `SIGINT` handler to fire -- and the parent exhausted `SPIN` first. If
yielding rotates correctly, why did the scheduler have to rescue the child
with `Anti-starvation: ... boosted 1 task to priority 0: [174(p16)]`?

The exit order says the child never finished on its own:

```
[thread] Process 204 has no threads left - now zombie   <- the PARENT gave up
[pty] master closed: SIGHUP+SIGCONT to group 205
[signal] Process 205 continued
[thread] Process 205 has no threads left - now zombie   <- the child, killed by the HUP
```

Two readings remain and they belong to different lanes. Either the fixture
races itself -- both sides spin the same `SPIN` and the parent must outlast
the child plus signal latency, which is lane B's to size -- or a task that
yields every iteration is still starving a same-priority peer, which is
lane A's. **Not guessing between them**: the next probe is a count of
voluntary switches per task across the rung, which `VOLUNTARY_SWITCHES`
already maintains per CPU and nothing currently reports.

#### The three failures are at least TWO findings, not one

`main.rs` frames them as one: *if `/bin/true` cannot exec, none of the
rungs below is testing what its name says.* That is sound reasoning about
`ctest-coreutils-runs` being a precondition, and it is wrong about the
third rung, which never execs anything.

| rung | exit | mechanism |
|---|---|---|
| `ctest-coreutils-runs` | 11 | `execl` of `/mnt/bin/true` failed |
| `ctest-python-repl` | 8 | `execl` of `/bin/python3` failed |
| `ctest-pty` | **45** | `waitpid(kid, &status, WNOHANG)` never returned the child's pid within `SPIN` |

Exactly two C fixtures in the tree call `execl`, and they are the first
two. **None calls `execv`.** `ctest-pty` does not exec at all: its child
`forkpty`s, checks `isatty`, installs a `SIGINT` handler and spins.

**And its failure is a starvation, not a fault.** The kernel log shows the
child DID finish -- `Process 205 has no threads left - now zombie` -- and
three lines earlier:

```
[sched] Anti-starvation: cur=173 boosted 1 task to priority 0: [174(p16)]
```

The scheduler had to boost the child out of starvation, and the parent's
`WNOHANG` + `sched_yield()` spin ran out of `SPIN` before the child got
far enough. The fixture's own comment predicts this shape: *"WNOHANG means
the child must run to exit, and it cannot while this loop owns the
quantum."* `sched_yield()` is the mitigation and it was not sufficient.

**Why this matters more than a third bug.** Someone fixing `execl` will
expect all three rungs to clear, because the harness says they are one
finding. Two will clear. The third will not, and the natural reading of
that is *"the `execl` fix is incomplete"* -- sending the next round back
into the exec path it just left. A wrong grouping costs more than a
missing one.

`ctest-pty`'s half is **lane A's**: `sched_yield`, the anti-starvation
boost and the quantum are kernel, not fixture. The `execl` half is lane
B's. They are unrelated and should be worked separately.

#### ROUND 7: lane A diagnosed this on 2026-09-16, and that diagnosis is ALSO misattributed

Lane C found `requests/a-b-libc-execl-passes-a-null-path-to-execve.md` --
filed by **lane A**, on `origin/main`, in my own tree since 2026-09-16. It
contains the discriminator I rebuilt today, the named mechanism
(`va_trampoline!`, `posix/src/spawn.rs:2204`) I never reached, and the
conclusion: *no C program on this system can exec by the list form*.

**Six rounds today rediscovered my own five-day-old work, more slowly, with
a wrong published diagnosis on the way.** The artefact holding the answer
was my own dropbox.

**And then the older diagnosis turned out to rest on a misattribution.**
Lane C flagged a tension they could not resolve: `execl_body` has opened
with `if path.is_null() { set_errno(EFAULT); return -1; }` since
2026-08-21, three weeks before the probe saw a NULL reach `linux_execve`.
If the trampoline lost `%rdi`, that guard would have caught it.

It resolves by reading, and cost one command. The probe line

```
[exec] linux_execve ENTERED and failed early: filename_ptr=0x0 errno=14
```

sits at line **557** of today's log, between `[syscall/linux] ... : OK`
lines, 2,500 lines before the fixture runs. It is a **kernel self-test**
deliberately passing NULL to check EFAULT -- `linux.rs:54128` is "execve
user-marshalling NULL handling", `:86053` does exactly this for
`execveat`. And **no `[exec]` probe fires anywhere near the fixture's
failure at all.**

So the 2026-09-16 note took a self-test's intentional NULL as evidence
about `ctest-coreutils-runs`. The observation was true; the subject was
wrong. That is dd-953's first costume -- a check reporting something true
about the wrong thing -- and it has now cost two investigations.

**What survives and what does not:**

| claim | status |
|---|---|
| vector form execs, list form does not | **survives** -- independently confirmed today by fastpy `forkexec` |
| `execl` is `execv` plus a `va_list` walk | **survives** -- read directly in `execl_body` |
| the NULL arrives at the syscall | **withdrawn** -- that probe hit belongs to a self-test |
| `va_trampoline` loses `%rdi` | **unsupported**, not disproved. It may still be right; the evidence cited for it was not evidence |
| the `execl_body` null guard should have caught it | **resolved** -- there was nothing to catch |

**Round 7's direction changes.** `ctest-coreutils-runs` is native-ABI, so
it never enters `linux_execve`; it goes through `SYS_PROCESS_EXEC`, which
had **no failure logging at all** until `391232edb` today. Both prior
diagnoses were reading the Linux-ABI door while the fixture used the
native one. The boot running now is the first that can say what the native
path actually returns.

**The lesson, and it is not "search harder".** I did search: I read the
fixture, the kernel exec paths, `posix::execve`, `load_elf`, and the
serial log. I did not read `requests/`, because I had classified it as
*incoming asks to triage* rather than *what lane A already knows*. A
dropbox is a record of findings as much as a queue of work, and mine
contained the answer under a filename that states it outright.

#### NARROWED by lane C, and one correction to how I framed the correlation

**`execl` has no exec path of its own.** `posix/src/spawn.rs` `execl_body`
ends:

```rust
let ret = match mode {
    ExecLMode::Direct => execv(path, argv),
    ExecLMode::SearchPath => execvp(path, argv),
    ExecLMode::WithEnv => execve(path, argv, envp),
```

So `execl` **is** `execv` plus a `va_list` walk, and everything below that
call is shared with the arm that works. That kills the second of my two
remaining candidates outright: it cannot be anything about `true` or
`python3` that `cat` lacks, because that would all be below `execv` and
identical. **What is left is the walk, or what the walk produces.**

**And a correction to my own framing.** I wrote that two of two `execl`
callers fail while the `execv` caller succeeds. True, but it implies a
control I did not have: there is **no C `execv`, `execvp` or `execlp`
anywhere in `services/`** -- three `execl` call sites in two files, and
nothing else. The only `execv` in evidence is fastpy's, a different
language and runtime. So the sample contained no C-side control, and
"every C `execl` fails, no C `execv` is tried" is not a correlation, it is
a description of a sample with one arm.

The delegation is what rescues the conclusion, and it is stronger than the
correlation was: since `execl` calls `execv`, and an `execv` demonstrably
works in the same boot, the difference is provably above that call.

**The specific thing to read first**, from lane C: pass 1 walks a *copy* of
the `VaList` and the comment carries the load-bearing assumption -- "the
cursors are per-copy, while the register save and overflow areas they index
are only ever read." If any cursor lives behind a pointer the copy shares,
pass 1 consumes what pass 2 re-reads and `argv` comes out wrong: an exec
that fails with the exec path blameless. `EXECL_STACK_ARGV` is 64 and both
failing sites pass one or two arguments, so the heap branch is not
involved.

#### The `execl` correlation, with every confounder checked off

`debugfs` on the image, so this is the bytes and not the manifest:

| binary | inode | mode | size | call path | result |
|---|---|---|---|---|---|
| `cat` | 19 | 0755 | 2,710,648 | fastpy `os.execv` | **OK** |
| `true` | 109 | 0755 | 796,064 | C `execl` | fails (exit 11) |
| `python3` | 81 | 0755 | 10,468,016 | C `execl` | fails (exit 8) |

Same directory, same mode, all staged, sizes spanning 13x. Both fixtures
`#define BIN "/mnt/bin/"`, so the `/bin/python3` in exit 8's description is
stale prose from before the path fix, not the path used -- checked, because
if it had still been `/bin` the second data point would have collapsed and
the correlation with it.

Two of two `execl` callers fail; the one `execv` caller succeeds; no C
fixture in the tree uses `execv`. That is as clean as this gets without
changing lane B's code.

#### ROUND 6: a passing control in the SAME boot kills every structural theory

The evidence was in the serial log the whole time, 557 lines below the
failure. From the same boot:

> `[spawn]   fastpy-on-SlateOS 'forkexec' (ring 3: os.fork cloned the
> process, the child os.execv'd 'cat' over PATH ["/mnt/bin"], and the
> parent os.waitpid'd + os.WEXITSTATUS-decoded the child's exit): OK`

A ring-3 process forked, and **the child exec'd a binary out of
`/mnt/bin`** -- the same directory as `/mnt/bin/true` -- and it passed.
That one line kills, simultaneously:

| theory | why it is dead |
|---|---|
| the binary is not staged | `cat` is staged and runs, from the same dir |
| the path is wrong | the same `/mnt/bin` prefix works |
| a forked child cannot exec | this child did |
| a forked child cannot read ext4 | it read `cat` to exec it |
| capabilities do not survive fork | and `Spawned child inherits parent capabilities: OK` is its own rung |
| the native exec syscall is broken | fastpy is native-ABI and uses it |

**What differs between the arm that works and the arm that fails** is not
structural at all. It is the call form:

| | working | failing |
|---|---|---|
| caller | fastpy `os.execv` | C fixture `execl(path, path, (char *)0)` |
| posix entry | `execv` -- argv already an array | `execl_body` -- variadic, walks a `VaList` |
| target | `/mnt/bin/cat` | `/mnt/bin/true` |

So the remaining hypotheses are two, both in userspace: the **variadic**
`execl` path specifically, or something about the `true` binary that `cat`
does not have. Both live in lane B's tree (`posix/src/spawn.rs`, the
fixture), which is why this entry stops at the localisation.

**The method note, because five rounds is a lot.** Rounds 1-5 each
proposed a structural cause, and each cost a boot to disprove. The control
that disproves all six at once was already sitting in the first serial log
any of them produced -- it just was not looked for, because nobody asked
*"does anything in this boot already do the thing I think is broken?"*
That question is free and should come before any theory that costs 90
minutes. dd-954 says a control licenses only the axis it varies; the
corollary is that **a passing control you did not write is still a
control**, and a long log usually contains one.

#### The control that cleared the capability theory varied four axes at once

Round 4 dropped the capability theory on a measured comparison:
`ctest-keylayout`, holding `(File, READ)`, opened and read `/proc/keylayout`
from ring 3 while this fixture, holding nothing, could not open
`/mnt/bin/true`. `(File, READ | EXECUTE)` was then granted here and the
rung still fails.

Reading `run_one` in the fixture shows why that comparison proves less than
it appears to. It `fork()`s, and **only the child** calls `execl`. The
parent never touches `/mnt/bin/true` at all. So the two arms differ in four
ways and share one:

| | failing case | the control |
|---|---|---|
| process | `ctest-coreutils-runs` | `ctest-keylayout` |
| file | `/mnt/bin/true` | `/proc/keylayout` |
| filesystem | ext4, a mounted image | procfs, synthesised in kernel |
| who reads it | a **forked child** | the parent itself |
| capability | `(File, READ)` | `(File, READ)` |

Only the last row is held constant, and it is the one the comparison was
used to reason about. That is dd-954's asymmetry in a debugging session
rather than a scan: **a control licenses only the axis it varies**, and
this one varied four while being read as evidence about the fifth.

It does not make the capability conclusion wrong. It makes it unsupported,
which is a different and more useful thing to know after four rounds.

**The hypothesis it suggests, stated so it can be killed cheaply:** a
forked child may not be able to read an **ext4** file at all -- because of
the fork, or because of ext4, or both -- and nothing in the four rounds so
far distinguishes those from a capability problem.

**The test that settles it is one I own.** A kernel self-test in
`spawn.rs` that forks and has the child read a known ext4 file, with no
exec anywhere in it, separates fork-vs-parent and ext4-vs-procfs in one
boot. `posix/src/spawn.rs` and the fixture are lane B's; that self-test is
not, so it can be written without a handoff.

**What `load_elf` actually does in the child**, from reading
`posix/src/spawn.rs` rather than assuming: `SYS_FS_STAT` on the path, then
`mmap` of the file size (796 KB here), then open and read. Any of those
three failing returns -1 with errno set and **never issues
`SYS_PROCESS_EXEC`** -- which is exactly the silence round 5 observed.
Worth noting separately: `execve`'s frame declares `[0u8; PATH_MAX]` plus
two `[0u8; 128 * 1024]` packing buffers, so it needs ~260 KiB of stack on
entry. That is a fifth candidate and it is not capability-shaped either.

#### Round 5's discriminator answered, and it eliminates the whole exec theory

`linux_exec_common` already wraps the exec path and prints
`[exec] execve(...) FAILED -> errno N` on every negative return. It was
added in `fec0ab339` precisely because *"the fixture can only report THAT
exec failed; the kernel knows the errno and had never been asked"*.

**It did not fire.** Establishing that took checking the instrument first:

| check | result |
|---|---|
| wrapper present at booted commit `37ff910f2` | yes |
| early probe present too | yes |
| do they work? | yes -- line 557 logs `linux_execve ENTERED and failed early: filename_ptr=0x0 errno=14`, and lines 3563+ log successful execs |
| any `[exec]` line at the failure (~3126) | **none** |

So the child process **never entered `execve`**. Every theory about exec --
wrong path, missing file, missing capability, ELF rejection, the 16 MiB
ceiling -- is eliminated at once, because none of them can be reached
without the syscall being made.

What the log establishes, and nothing more:

```
[cu] true (exec, run, exit 0 -- ...)          fixture announces the attempt
[cow] Cloned address space: parent -> child   fork() SUCCEEDED
[sched] Spawned task 153 (priority 16, cpu 0)
[thread] Spawned thread (task 153) in process 184
[thread] Process 184 has no threads left - now zombie
[sched] Task 153 exiting
```

No fault, no exception, no exec. The child was created and exited. **The
fault is between `fork()` returning in the child and the child issuing
`execve`** -- which is a handful of instructions in the fixture's libc.

**The process lesson, which cost me this whole session.** That wrapper's
doc comment lists the three theories already tried and disproved: *not
staged; wrong path; no capability*. I spent this session re-running the
first one -- rebuilding the argument that the binary was missing, writing
it up, pushing it, and having to retract it. The comment naming my theory
as already-dead was inside the function I eventually read, and I read it
**last**. Reading the code that owns the failure before theorising about
it is not a refinement of method, it is the method.

**Next probe belongs at the fork boundary, not the exec one.** A diagnostic
on the child's first return from `fork_process_clone` would say whether the
child ever ran userspace instructions at all. Do not spend another boot on
an exec theory: the instrument has already ruled that family out.

**Still true from the original entry:** `rootfs.ext4.manifest` has 93
entries and none for `/bin/true`, so the image guard's corpus genuinely
excludes the binaries the rungs exec. That is worth widening on its own
merits -- it just is not what broke this boot.
