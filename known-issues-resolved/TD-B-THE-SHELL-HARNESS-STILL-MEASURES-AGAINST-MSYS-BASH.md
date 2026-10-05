### TD-B-THE-SHELL-HARNESS-STILL-MEASURES-AGAINST-MSYS-BASH. `scripts/osh-bash-diff.py` compares a Windows `osh.exe` against Git-for-Windows bash, so every osh behaviour it certifies was learned from a Cygwin port — 2026-08-25 — **FIXED 2026-08-25** (harness migrated; triage of what it exposed is in progress, see below)

**Where:** `scripts/osh-bash-diff.py`. Two lists decide what is compared:

```python
OSH_CANDIDATES = [
    REPO / "target" / "x86_64-pc-windows-gnu" / "release" / "osh.exe",
    ...
]
BASH_CANDIDATES = [
    Path("C:/Program Files/Git/usr/bin/bash.exe"),
    Path("/usr/bin/bash"),
    Path("/bin/bash"),
]
```

On this host the first entry of each wins, so the subject is a Windows binary
and the reference is MSYS bash. `/usr/bin/bash` is reached only on a machine
that has no Git for Windows.

**What:** this is the same structural fault that
`TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE` recorded for `sort`, one
subsystem over, and it is still live. MSYS is a Cygwin derivative: it links
`msys-2.0.dll`, not glibc. Its getopt wording, its signal table, its locale
handling and its process model are all Cygwin's. A differential harness whose
reference is MSYS certifies behaviour that no GNU/Linux system has — and
SlateOS is a GNU/Linux-shaped target, so the wrong reference is wrong in the
direction that matters.

**This is not news to the tree, and that is the point.** `TOOL-OSH-BASH-DIFF`
(above) says so plainly and resolves it by waiving: "Cases hitting those need an
`# EXPECT-DIFF` waiver." That was the right call when there was no way to reach
a glibc bash from a harness. There is now — every one of the twenty-five
`scripts/*-diff.sh` coreutils harnesses reaches glibc through
`scripts/diff-wsl.sh`, which re-execs into WSL, builds the subject for
`x86_64-unknown-linux-gnu`, and refuses to run against a stale build. The shell
harness was not part of that migration only because it is Python and does not
match the `*-diff.sh` glob.

**Four entries in this file exist because of it**, and each is a case the
corpus cannot currently ask:

| entry | what the MSYS reference costs |
|---|---|
| `TD-OILS-SIGNAL-NUMBERS-ARE-LINUXS-AND-CANNOT-BE-CHECKED-AGAINST-MSYS` | the signal table is untestable; the corpus tests only behaviour, never a number |
| `TD-OILS-MSYS-CHMOD-TYPE-A` | `type -a` cannot be used on a file the case created, because MSYS `chmod +x` does not make `test -x` true |
| `TD-OILS-HOST-ARGV0` | `argv[0]` for an external command is the resolved path, not the word typed |
| `TD-OILS-A-NON-UTF-8-ARGUMENT-IS-REPLACED-ON-THE-WINDOWS-HOST` | a child sees `\xef\xbf\xbd` where bash's child sees `\xff` |

The last two are *subject*-side, not reference-side: they are artifacts of osh
being built for Windows, and would go away with the subject rather than with the
reference. That is worth stating because it means the migration fixes both
halves, not one.

**What the fix looks like.** The Python equivalent of `diff-wsl.sh`, which is
mostly the same six steps:

1. Re-exec into WSL if not already there, carrying the harness's arguments.
2. Build `osh` for `x86_64-unknown-linux-gnu` into
   `$HOME/.cache/slateos-diff-target` — the same shared target directory the
   shell harnesses use (design-decisions.md §374).
3. Take the reference from `/usr/bin/bash`, and skip rather than fall back.
4. Pin `LC_ALL=C.UTF-8`.
5. Keep the newest-binary rule and the `write_bytes` rule, both of which are
   still load-bearing (see `TOOL-OSH-BASH-DIFF`).
6. Add the staleness guard: `osh` is one binary out of a package, and a cargo
   cache that replays a library unit while relinking the binary is exactly the
   failure `diff_assert_fresh` was written for.

**The first measurement, before any of that**, is whether `userspace/oils`
builds for `x86_64-unknown-linux-gnu` at all. It has never been asked to; it is
built for `x86_64-pc-windows-gnu` on the host and for `x86_64-slateos` for the
image. If it does not build, that is the finding and it comes first.

**Expect a wave of differences, and treat each as a finding.** Roughly 150
corpus cases have been written, run and waived against MSYS. Some fraction were
tuned to MSYS behaviour and will differ against glibc — every such case is a
place where osh was shaped by a Cygwin port, which is the whole reason to do
this. Budget the triage, not just the migration. The four entries above should
be re-examined at the same time: two should become testable and two should
disappear.

**Risk of leaving it.** It compounds. Every corpus case added between now and
the migration is another case written against the wrong reference, and every
`# EXPECT-DIFF` waiver added is a divergence pinned into the corpus as though
it were behaviour. `sort` is the precedent: eleven option cases sat green for
weeks while the implementation faithfully copied a Windows porting artifact.

### FIXED 2026-08-25 — and what the migration found

**The harness is migrated.** `scripts/osh-diff.sh` (commit `45fe3300c`) runs the
corpus with a `x86_64-unknown-linux-gnu` osh against `/usr/bin/bash` inside WSL.

**The fix was not the one predicted above.** The six-step plan was to port
`diff-wsl.sh` into Python. None of that was needed: `osh-bash-diff.py` already
took `--osh` and `--bash`, so a ten-line `sh` wrapper that *does* match the
`*-diff.sh` glob inherits all six steps — the WSL re-exec, the shared target
dir, the build-every-run, the freshness assert, the locale pin — from the same
copy the twenty-five coreutils harnesses source. The diagnosis above ("not part
of that migration only because it is Python and does not match the `*-diff.sh`
glob") named the cause correctly and then reached for the expensive remedy: the
thing that did not match the glob was a *file name*, and the cheap fix is to add
a file that matches it. Worth remembering the next time a harness needs the same
plumbing.

**The first measurement, which the entry rightly demanded first: it builds.**
`userspace/oils` compiled for `x86_64-unknown-linux-gnu` on the first attempt
with no source change. Its tests pass there too — 1460 lib tests on
`x86_64-unknown-linux-gnu` against 1486 on `x86_64-pc-windows-gnu` (the
difference is `cfg`-gated cases, not failures).

**First full sweep against glibc: 641 matched, 0 waived, 21 failed**, plus 2
cases that exceeded their budget on a busy host and so were never measured.

**The `0 waived` is the finding, not the good news.** `git grep EXPECT-DIFF`
over every corpus case, at every commit in this repo's history, matches
`README.md` and nothing else: **the waiver mechanism was never once used.** So
`TOOL-OSH-BASH-DIFF`'s stated resolution — "cases hitting those need an
`# EXPECT-DIFF` waiver" — describes a policy that was never exercised, and the
picture in the entry above (MSYS-isms parked in visible waivers) is wrong in the
worse direction. Nothing was parked. Every Cygwin behaviour the corpus met was
*implemented in osh and certified green*, because that is what a differential
harness does with a wrong reference: it does not fail, it agrees. The 21
failures are the first time any of it became visible, and each one is a place
where osh was shaped by a port nobody runs.

**The four dependent entries came out as predicted** — all four re-examined the
same day:

| entry | outcome |
|---|---|
| `TD-OILS-SIGNAL-NUMBERS-…-AGAINST-MSYS` | now checkable, and checked: **VERIFIED CORRECT**; the "unverifiable on this host" premise is retired |
| `TD-OILS-MSYS-CHMOD-TYPE-A` | **WONTFIX** — confirmed a dev-host measurement artifact, not an osh bug |
| `TD-OILS-HOST-ARGV0` | subject-side; the `cfg(unix)` path is now **measured** rather than argued |
| `TD-OILS-A-NON-UTF-8-ARGUMENT-…` | subject-side; **not reproducible off Windows**, now measured |

**Triage of the 21, in progress.** Closed so far: the GNU-`time` page-fault
leak (2 cases); the Cygwin-isms osh had transcribed — `pwd -W`, `shopt igncr`,
the `paste-from-clipboard` bind name — plus the two `help test` paragraphs bash
5.2 has and osh did not; `compgen` ordering; the POSIX `time` path; child CPU
accounting on unix (`TD-OILS10`); and `-O`/`-G`, which asked an invented root
identity and therefore answered *backwards* on every file (commit `60a63c49f`);
and the job-death group described just below (3 cases, commit `eb23cc57e`).
Remaining groups: shebang handling
(`shebang-interpreter-line`, `exec-shebangless-script`,
`a-command-path-that-will-not-run…`); and ten one-off defects (`enable -f`, an
arithmetic subscript expanded in place, a debug trap's `PIPESTATUS`, `nounset`
array length, `printf`'s `strtod` float parsing, the status of a fatal expansion
abort, `TIMEFORMAT`, a std fd bound to a read-only source, `compgen` actions,
and pipeline `xtrace` ordering).

**One guard added so this cannot recur silently.** The harness now asks the
reference shell for its own `$MACHTYPE` — baked in at bash's configure time, so
a copy, a symlink or an explicit `--bash` cannot fool it the way a path
heuristic would — and prints a warning above the first case when the answer is
not a Linux triple. A wrong reference announces itself as a green run, so it now
gets announced somewhere the reader will still be paying attention.

### The job-death group: one wrong belief, three cases, and a doc comment that stated the inverse — 2026-08-25 — **FIXED** (`eb23cc57e`)

**In short:** when a background job is killed by a signal, osh kept the job in
the table so a later `jobs` could report `Terminated`. bash does not: it drops
the job the moment it hears of the death, whether or not it prints anything
about it, so a script's `jobs` listing can never name a signal at all. Three
corpus cases had been written around osh's version and certified green against
MSYS bash.

**The belief, quoted from the code it was written into:**

> Those three, and only those, are the signal names a `jobs` listing can ever
> show.

— `signal_announced_when_reaped`'s doc comment, of `INT`, `TERM` and `PIPE`.
This is the exact inverse of what bash does. Those three are the signals bash
prints *no message* for (`DONT_REPORT_SIGTERM`, `DONT_REPORT_SIGPIPE`, and an
interrupt being the user's own doing), and the exemption is from the **message
and nothing else**. The shell still notices the death, and noticing is what
takes the row.

**Measured, bash 5.2.21, non-interactive:**

| line | result |
|---|---|
| `sleep 5 & kill -TERM %1; sleep 0.02; jobs` | prints **nothing**, 8 runs out of 8 |
| `sleep 5 & kill -TERM %1; jobs` | prints `Running`, 8 runs out of 8 |
| `… ; wait %1` after the first | `no such job`; `%1` is free for the next job |

So there is no listing that words the death: the sweep either has not happened
yet (`Running`) or has (the row is gone). A non-interactive listing has exactly
two states, `Done` and `Exit N`. `Interrupt` is doubly impossible — an
asynchronous job is handed `SIGINT` already ignored.

**Interactive bash is a different shell here** and *does* print `[1]+
Terminated sleep 5`. That is presumably where the belief came from, since it is
what one sees by hand at a prompt. The corpus runs non-interactively, so the
non-interactive answer is the one osh owes.

**The fix** splits the two questions that had been one.
`announce_signalled_job` → `notice_signalled_job`: it prints only when bash
prints, and returns `true` either way, so the caller drops the row
unconditionally. `signal_announced_when_reaped` keeps its body and loses its
claim about listings. What genuinely *does* suppress the notice is unrelated and
unchanged: a command substitution, whose announcement would land on a stderr
that has nothing to do with the value being collected, so bash holds the death
for the substitution's own `jobs`.

**Three unit tests changed with it**, and one of them is worth recording
because it nearly produced a second wrong fix.
`jobs_marks_the_current_and_previous_job` used `kill %1` to make a finished job
to hang the `-` marker on; with the fix there is no row left to mark, so it now
gates on a file instead:

```sh
until [ -e gate ]; do sleep 0.01; done & sleep 30 &
: > gate
```

The listing then read `until [ -e gate ]; do` / `    sleep 0.01;` / `done` —
which looked like a second osh defect, since the source text is one line. It is
not: **both shells re-print a compound command in the `jobs` command column from
the parse tree**, four-space indented and broken over lines. Verified against
bash 5.2.21 for `until`, `for`, `while` and `if`. The expectation was wrong and
osh was right; the note is in the test.

**The general lesson, which is the same one the migration keeps teaching.** A
differential harness with a wrong reference does not fail — it agrees. This
belief was not parked in a waiver or flagged in `todo.txt`; it was implemented,
documented in prose, asserted by three unit tests and three corpus cases, and
green throughout. The only thing that found it was changing the reference.
