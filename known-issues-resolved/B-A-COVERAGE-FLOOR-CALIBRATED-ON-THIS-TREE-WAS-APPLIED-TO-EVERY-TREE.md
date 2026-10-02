## B-A-COVERAGE-FLOOR-CALIBRATED-ON-THIS-TREE-WAS-APPLIED-TO-EVERY-TREE — FIXED 2026-09-03 (lane B)

**In short:** a gate learned to refuse a verdict when it had inspected
implausibly little of the project. The numbers it compares against were
measured from *this* repository. Nothing stopped it applying them to a
different repository, and another test suite runs that gate against tiny
synthetic repositories on purpose — so fourteen of its cases went red with a
message accusing the scan of collapsing when the tree really was that small.

### How it got in

`d50a4b97e check-doc-links: refuse a verdict on a scan that inspected too
little` added five absolute floors (`MIN_TREE_CRATES` and friends) and routed
them with `whole_tree = not paths`. That expression is a property of the
*invocation* — "the caller did not narrow the scope". The floors are a property
of the *corpus* — "this scan saw less than this project irreducibly holds".
The two were treated as one, and they are not: a whole-tree run of somebody
else's tree satisfies the first and says nothing about the second.

`test-checkers-honour-head.py` builds one-crate repositories in a temp dir,
copies the gate into their `scripts/`, and runs it. The gate then computes
`ROOT` as its own parent's parent, concludes it is looking at the whole tree,
and measures a two-file fixture against a floor of 200 files.

### Why the gate's own self-test did not catch it

It nearly did, and the near miss is the instructive part. There *is* an
end-to-end case that points `ROOT` at a fixture tree — but its fixture was
built for a different question (does `whole_tree = not paths` route both ways),
so it wanted the refusal and got it. Every other floor case called
`coverage_breach` directly as a pure function, where the corpus never comes up.
A premise that only exists in `main()` cannot be found by cases that never run
`main()`.

### The remedy

`TREE_MARKERS = ("design.txt", "roadmap.md", "CLAUDE.md")` and
`is_calibrated_corpus(tree)`. The absolute floors apply only when the tree the
gate is installed in carries all three; the *structural* floors (a crate yields
a unit, a unit yields a file) still apply everywhere, because those hold for
any real crate at any size and are what actually catch a collapsed scan.

Three markers and `all`, not one and `any`: a scratch repository with a stray
`CLAUDE.md` in it is not unusual, and inheriting floors measured against a tree
five hundred times its size is the failure being fixed, not a smaller version
of it.

The skip is **printed to stderr every time it happens**. A floor that silently
stops applying is exactly the decoration `scripts/mutate-gate.py` exists to
find, so the one path that legitimately skips it announces itself. In this tree
that line never prints; if it ever does, that is the finding.

Five mutation needles were added for the new branch (both directions of the
corpus check, both directions of the `main()` derivation, and `all` → `any`),
plus self-test cases that drive `main()` with the predicate stubbed both ways —
because a premise decided in `main()` is invisible to a suite that only unit-
tests the function obeying it.

### The general lesson

**A floor derived from a measurement carries the corpus it was measured on as
an unwritten premise, and that premise has to be written down and checked.**
The calibration comment named the tree, the date and the counts; what it did
not do was make the code ask whether it was still looking at that tree. Any
constant justified by "measured here" has the same hole.

---

### Lesson 110: a refusal justified by a guess about a syscall's errno invents the errno (lane B, 2026-09-03)

**In short:** `coreutils`'s shared `dirfd` module refused to create a symlink
whose target was the empty string, and the comment defending that refusal said
the kernel "would answer `EINVAL` anyway", so answering early only improved the
message. The kernel answers `ENOENT`. Nobody had asked it. The refusal did not
save a syscall — it replaced the syscall's answer with a different one, and the
only reason that surfaced is that `tar` has a harness which compares its errors
against GNU's case by case.

**What happened.** Converting `tar` to the shared `dirfd` module routed its
symlink creation through `dirfd::c_target`, which had this:

```rust
if target.is_empty() || target.contains(&0) {
    return Err(embedded_nul());          // io::ErrorKind::InvalidInput
}
```

`tar`'s own code, which the conversion deleted, had passed the empty target
straight to `symlinkat`. So the conversion was a behavioural no-op everywhere
except one forged-header case, where:

```
ours (rc=2): tar: sl: Cannot create symlink to '': Invalid argument
gnu  (rc=2): tar: sl: Cannot create symlink to '': No such file or directory
```

Linux resolves a symlink target through `getname()`, which rejects the empty
string with `ENOENT`, not `EINVAL`. The comment had reasoned from the shape of
the argument ("empty is invalid") rather than from the kernel, and reasoning
from shape is how you arrive at a plausible errno that is not the real one.

**Why the guess was invisible until a differential harness ran.** Every other
check on that code agrees with the guess by construction. Clippy cannot know an
errno. A unit test written by the same author asserts the same `InvalidInput`
the code raises — this is Lesson 65 (a check that reuses the assumption it is
checking will agree with itself), in its errno form. The only thing in the tree
that holds an independent opinion about what `symlinkat("")` does is the kernel,
and the only thing that consults it is a test that *calls* it, or a harness that
compares against a program that calls it. `scripts/tar-diff.sh` was the latter,
and its `emptysym` case is the entire reason this is a fixed defect rather than
a shipped one.

**The fix, and the shape of the test that keeps it.** `c_target` now refuses
only the NUL — which is a real refusal, because a C call handed a NUL stores the
prefix and the link would resolve somewhere the caller never named — and the
empty target goes to the kernel. The test that pins it does *not* assert that
empty is rejected; that was never in doubt and asserting it would have passed
against the bug too. It asserts **who** rejected it:

```rust
let e = dir.symlink(b"", b"link").unwrap_err();
assert_eq!(e.kind(), io::ErrorKind::NotFound);   // only the syscall says this
let e = dir.symlink(b"a\0b", b"link").unwrap_err();
assert_eq!(e.kind(), io::ErrorKind::InvalidInput); // only this module says this
```

The second assertion is what makes the first discriminating: the two errors are
distinguishable, so `NotFound` cannot have come from the early return.

**Where else to look.** Any validation that rejects an argument *before* the
syscall, on the grounds that the syscall would reject it too. The grep is for an
early `return Err(...)` in a thin wrapper over an `extern "C"` call, and the
question to ask at each one is: has anyone run the syscall to find out, or is
the errno in the comment a plausible-sounding one? Within `dirfd` the audit is
now clean — `c_name`'s refusals (empty, `/`, NUL) are not errno-equivalence
claims at all; they are the module's own contract that a name is one lookup, and
the comment says so rather than appealing to what `openat` would have done.

---

### Lesson 111: a fixture chosen so both sides fail cannot tell which side tried (lane B, 2026-09-03)

**In short:** `rm -r` refused to delete an empty directory it could not read,
where GNU deletes it. Two separate checks were supposed to catch that and both
were pointed at the same blind spot: nine differential cases and one unit test,
every one of them built on a directory that had a file in it. With a file in it
GNU fails too, and prints the same error for a different reason — so a program
that never attempted the removal at all looked identical to one that attempted
it and was refused. The missing fixture was not an exotic one. It was the
*simpler* one: the same directory, empty.

**The rule that was missed.** Reading a directory needs `r`. Removing an empty
one needs `w`+`x` on its **parent** and nothing at all on the directory itself.
So `chmod 300 d` on an empty `d` is a directory nobody can list and anybody can
delete, and GNU deletes it — `fts` hands the entry over as `FTS_DNR` and
`remove.c:571` calls `excise` on it anyway. Our walk reported the read failure
and stopped:

```text
$ mkdir d && chmod 300 d && rm -rv d
GNU : rc=0  removed directory 'd'
OURS: rc=1  rm: cannot remove 'd': Permission denied
```

**Why nine cases and a unit test all missed it.** `rm-diff.sh` section 15 had
nine cases for "directories that cannot be read or emptied", and all nine used
`tree/sub`, which holds `b.txt`. There GNU's `rmdir` *also* fails, with
`ENOTEMPTY` — which upstream then throws away in favour of the held read error,
printing `Permission denied`. That is character-for-character what we printed by
never calling `rmdir` at all. **The non-empty fixture makes the two behaviours
converge on the same output**, so nine cases pinned an output that two different
programs produce, and could not distinguish them however many of them there
were. Adding a tenth of the same shape would not have helped; the count was
never the problem.

The unit test was worse, because it was not merely blind but confirming: it
built an *empty* 0000 directory, ran `rm -r`, and asserted `!r.ok`. That is the
correct fixture with the wrong expectation — written from what the code did
rather than from what GNU does, so it converted the bug into a *requirement*.
Anyone who later fixed the walk would have been told by the suite that they had
broken it. This is Lesson 65's shape (a check that reuses the assumption it is
checking will agree with itself) in its most direct form: the assertion's source
was the program under test.

**The tell.** In both cases the fixture had a property that was not needed for
the thing being tested, and that property was what hid the bug. Section 15's
comment says it is about "directories that cannot be read"; the file inside
`tree/sub` is not part of that description, it is an incidental of the shared
`mktree` helper being reused. Whenever a fixture is inherited rather than
chosen, ask which of its properties the test actually needs — and then build the
one with only those properties, because that is the case the suite does not
have.

**How it was fixed, and how the fix was checked.** Fifteen cases added for the
empty-unreadable directory across `-r`, `-d`, `-f`, `-i`, `-I` and
`---presume-input-tty`, and the unit test split in two: the non-empty one keeps
the ancestor-silence rule it was named for (with a child added so the `rmdir`
genuinely fails, plus an assertion that `not empty` does *not* appear, which is
what proves the substitution ran), and a new one asserts the empty one is
removed **silently and successfully** — success being the part a walk that
skipped the directory cannot fake.

Then the whole section was run against the *pre-fix* binary, which is the step
that distinguishes a test suite from a description of current behaviour:
**eleven of the fifteen differ**, and the four that do not are modes
0100/0000/0500 where erroring is correct, so they pin the boundary from the
other side and would catch an over-eager fix. A new case that passes before and
after the change it was written for is not a regression test; it is a comment.

**And the wider point about the conversion that found it.** A shared module
absorbing a caller inherits every one of that caller's edge cases, including the
ones the caller handled by *not* handling them. `tar` passed the empty target
through because it had no opinion; the shared module had an opinion, and the
opinion was wrong. Extraction is not only a diff about call sites — it is a diff
about which layer is allowed to have opinions, and every opinion the new layer
adds is a behaviour change that has to be certified, not assumed.

### Lesson 112: cancelling a background task kills the shell, not the build — and the orphans come back as a compiler error (lane B, 2026-09-03)

**In short:** eight coreutils binaries failed to build with
`STATUS_DLL_INIT_FAILED` (`0xc0000142`), which reads exactly like a broken
linker invocation or a corrupt toolchain. It was neither. It was that the
machine had **611 processes** on it, because every long `cargo` run I had
stopped over the preceding hour was still running. `TaskStop` had killed the
wrapper shell each time and nothing else; `cargo`, `rustc` and `clippy-driver`
are grandchildren, and they were orphaned, not killed. Windows fails
`DLL_PROCESS_ATTACH` when a process cannot get the resources to initialise, so
the symptom surfaced in the newest process rather than in the ones causing it.

**The two failure modes, and why the first one hides the second.** An orphaned
`cargo` still holds the flock on the build directory, so the *visible* symptom
is a new run sitting at `Blocking waiting for file lock on build directory`
forever. That one is at least self-describing. The invisible symptom is the
resource ceiling: nothing in the 0xc0000142 message mentions process count, so
the natural reading is that the *code* or the *toolchain* is broken, and the
natural response is to start bisecting a source tree that is fine. I lost time
to exactly that reading before running `wmic` and seeing the process table.

**What actually diagnoses it.** `wmic process get
ProcessId,ParentProcessId,CreationDate,CommandLine` — the creation dates are the
tell, because orphans from a run you stopped forty minutes ago have timestamps
that no longer correspond to anything you are running now. Kill only your own,
**by PID**: `taskkill //PID <n> //T //F`. (From MSYS the slashes must be
doubled, or the shell rewrites `/PID` into `C:/Program Files/Git/PID` and
`taskkill` rejects it as a filename.) Never by image name — another lane's
`cargo test -p mindmap` was in that same list, and a `taskkill /IM cargo.exe`
would have destroyed their run to fix mine.

**The fix is not discipline, it is the job object.** `scripts/run-timeout.py`
exists precisely for this: it puts the child in a Windows Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so a timeout, a Ctrl-C, or the runner
itself dying tears down the **entire tree**, grandchildren included. Every long
run in this session went through it afterwards and the orphan problem did not
recur. `CLAUDE.md` already said to use it for anything that might hang; what
this incident adds is that it is equally required for anything you might
*cancel*, which is a much larger set — a cancellable run is any run long enough
that you would want to stop it, i.e. every run worth backgrounding.

**The generalisable shape.** A cancel that only reaches the process you have a
handle on is not a cancel; it is a detach. Whenever a tool offers to stop
something, ask what it has a handle on, because that — not the thing you asked
it to stop — is the extent of what dies. And when a build fails with an error
that names no file of yours, check the machine before you check the code: the
first hypothesis should not be "my program is wrong" when the evidence is
equally consistent with "there is no room to run it."

**Postscript, 2026-09-04 — I then made the inverse error, twice in one hour,
and the remedy above is what caught it.** Checking whether a `git push` was
still alive, I ran the process query through `| head -14`. The `python.exe`
rows filled all fourteen lines, the `git.exe` rows were cut off below them, and
I read the truncated output as "no git process" and concluded the push had died
silently. It had not. On that reading I started a **second** `git push` to the
same ref, and for several minutes two pushes were racing to run the same eleven
pre-push gates. Re-querying without `head` — and with `CreationDate`, which
distinguished the 04:36 original from my 04:44 duplicate — showed both, and the
duplicate was killed by PID with `taskkill //PID … //T //F`.

So the sharper rule, which this entry did not previously state: **never pipe a
process query through `head`, `tail`, or any other truncation, because the
thing you are inferring from is the *absence* of a row.** Truncation and
absence are indistinguishable in the output, and the reading you will reach for
is the one that says the process is gone — which is also the reading that makes
you take action. Every other kind of command can be sampled; this one cannot.
It is the same defect as Lesson 111 (lane B)'s fixture, in a different costume: an
observation whose two possible causes produce identical output, mistaken for
evidence of one of them.

### Lesson 113: a log written by two processes at once is not a log, and `wsl.exe` is always the second one (lane B, 2026-09-04)

**In short:** when a program running inside WSL writes to both stdout and
stderr, and the caller merges the two into one file — `cmd > log 2>&1`, which
is how every backgrounded run in this tree is recorded — the quieter of the two
streams is **silently overwritten**. Not truncated, not interleaved: gone, with
no error anywhere. `wsl.exe` does not share a file offset with any other
writer, so each stream starts at byte zero of the same file and the louder one
paves over the other. Two of this project's checks were affected, and the
symptom in both is a log that reads as if the check passed.

**How it surfaced.** I ran `scripts/coreutils-check.sh --only linux` to measure
what the Linux half costs, and grepped the captured output for its section
headers to confirm which half had run. The grep matched only `=== summary ===`.
The natural reading is "clippy did not run" — and I nearly acted on it. Running
the script again with the streams captured *separately* showed the header
present and correct on stdout. Nothing had failed to run; the bytes had been
destroyed in transit. The merged capture said `result: clean` with nothing left
in it to say which half had produced that verdict, which is precisely the
defect `coreutils-check.sh` was written to close, one level up: a check that
did not visibly happen, reading as a check that passed.

**The minimal reproduce**, which is worth keeping because the behaviour is not
documented anywhere and is easy to disbelieve:

```bash
cat > probe.sh <<'EOF'
wsl -e bash -c 'echo "VERDICT"; for i in $(seq 1 40000); do echo "noise" >&2; done'
EOF
bash probe.sh > f.txt 2>&1
grep -c VERDICT f.txt        # 0 -- the line is gone
bash probe.sh > o.txt 2> e.txt
grep -c VERDICT o.txt        # 1 -- it was always being written
```

Volume decides the winner, which is what makes this so dangerous: a *warm*
build is quiet, so the verdict survives and the arrangement looks fine. It is
the failing run — the one that floods stderr, the one whose output you actually
need — that loses it.

**Two plausible fixes that do not work, and why.** Both are worth recording
because both look obviously correct.

| Attempt | Why it fails |
|---|---|
| Redirect on the near side (`wsl … 2>&1`) | This is already what `> log 2>&1` does: it makes fd2 a dup of fd1. `wsl.exe` still writes the two with offsets of its own. |
| Relay WSL's stdout through a pipe (`wsl … \| cat`) | The relay is simply one more writer with an offset of its own, racing `wsl.exe`'s stderr. Same bytes lost. |

**What works is the rule that exactly one writer may own the file**, and the
collapse must happen on the *far* side, where both streams are still ordinary
pipes back to `wsl.exe`. One `exec` redirect does it. Which direction to
collapse is a real choice and it went differently in the two places, for
reasons in design-decisions.md §762: `coreutils-check.sh` does `exec 1>&2`,
freeing stdout to carry its verdict alone where no WSL handle can reach it;
`diff-wsl.sh` does `exec 2>&1`, because the 50 harnesses' report is *already*
on stdout and every caller looks for it there.

**Scope, once you know to look for it.** 34 files here invoke WSL, but command
substitution — `$(wsl …)` — is immune, because a pipe has no offset to collide
over. Only invocations that let `wsl.exe` inherit the caller's file handles can
lose anything, and there were exactly two: `coreutils-check.sh`, and
`diff-wsl.sh`'s `exec wsl -e env "$@"`, which is the shared preamble every one
of the 50 `*-diff.sh` differential harnesses re-execs itself through. That
second one means the primary correctness evidence for coreutils has been
written this way for as long as the harnesses have existed.

**The generalisable shape.** A redirection is a *contract about a file offset*,
and it silently stops holding whenever the writer is on the far side of a
process boundary that reopens the handle — WSL here, but the same is true of
anything that marshals handles across a VM or container edge. So: **decide
which stream carries the verdict, make it the only thing on that stream, and
make the collapse happen where the streams originate.** A verdict sharing a
file with unbounded tool output is a verdict you have merely been lucky to keep
reading.

And the reason this was caught at all is Lesson 112 (lane B)'s postscript, applied
without meaning to: I was reasoning from the **absence** of a line. That entry
says never to infer from an absence in truncated output; the wider rule this
adds is that a missing line has *three* causes, not two — it was never written,
it was cut off, or it was written and then destroyed — and the third is the one
nobody checks. When output disagrees with what you believe ran, re-capture with
the streams separated before you conclude anything about the program.
