## TD-A-A-WIRED-GATE-CAN-GRADE-ONE-LINE-AND-LOOK-LIKE-IT-GRADES-A-SUBSYSTEM (lane A, 2026-09-03)

**In short:** the two checkers wired by the entry above were switched on because
they read a file in `kernel/`. They do — and then check one line of it. I
deliberately broke the code each one is named after, and both reported a clean
tree. Both are now fixed. What is *not* fixed is the general problem they are an
instance of: a gate's `--self-test` proves its *logic* works on strings the
author made up, and proves nothing about whether the gate is still attached to
the real file it claims to grade. 24 of 37 gates have a self-test; after this
work, 2 of 37 have a case that mutates their actual subject and demands a
refusal.

`scripts/check-shellquote-vs-bash.py`, `scripts/check-kshell-rungs-vs-bash.py`,
wired into `scripts/boot-test.sh` in `cb29ea5dc`; fixed in `a6551a3af` and
`d280f66b1`.

**How this surfaced.** Not from a failure. The entry above closes with lane B's
warning that "a later edit to `shellquote.rs` that changes behaviour is caught
by nothing", offered as the cost of leaving the gates unwired. After wiring them
I went to confirm that cost had been paid, by planting the edit lane B
described. It was not caught. The warning survived the fix that was supposed to
answer it.

**Reproductions** (against the tree as of `cb29ea5dc`; both are one-line edits
to a clean tree, and both exit 0):

1. In `kernel/src/shellquote.rs`, change the `Ctx::Single` arm to
   `let structural = false;` — a scanner in which a single-quoted string can
   never close. `python scripts/check-shellquote-vs-bash.py` printed
   `0 failure(s)` and exited 0. Its only read of the Rust was a regex for
   `const DQ_ESCAPABLE: [u8; N] = [...]`; everything else it compared was a
   Python re-implementation of the scanner, measured against bash.
2. In `kernel/src/kshell.rs:22167`, change `alloc::vec!["a", "b", "c"]` to
   `alloc::vec!["a", "b", "", "c"]` — a blank word bash never produces.
   `python scripts/check-kshell-rungs-vs-bash.py` printed `0 rung assertion(s)
   disagree with the reference tool` and exited 0. It pinned the rung *inputs*
   verbatim and never read a single rung's *expected value*.

**Status: both fixed.** `scripts/rustrungs.py` is a new shared reader that pulls
a rung's own `assert_eq!` expectation out of the Rust; both oracles now require
three-way agreement between the rung, the transcription in the gate's own table,
and real bash, and both now enumerate every asserted call in their subject file
so a rung nobody transcribed raises instead of passing silently. Coverage went
from 13 rungs to 16 in kshell, because reading the Rust found three graded by
nothing. Each of the seven mutants above and below was replanted after the fix
and each is now caught. The honest scope of `check-shellquote-vs-bash.py` is
written into its docstring: of `shellquote.rs`'s thirteen rungs, five
`strip_quotes` rungs are gradeable against bash, one (`strip_quotes(b"a\\")`) is
a deliberate divergence pinned three ways so that *agreeing* with bash reds the
gate, the five `find_bare` and two `bare_positions` rungs assert byte offsets
that bash does not expose and are graded against the Python port and labelled as
such, and two are excused by name. Reproduction (1) is still not caught by the
gate — a Python program cannot execute Rust, so a scanner defect is caught by
`self_test()` at boot and by nothing in `scripts/`. The docstring now says that
in as many words rather than letting the file's name imply otherwise.

**The general debt, which is the part that remains.** Stated precisely, because
the imprecise version of this sentence is what let the defect through: 24 of the
tree's 37 gates *do* ship a `--self-test`, and those self-tests *do* contain
true-positive cases — a fixture the gate is expected to refuse. What none of
them had, before the two written here, is a true-positive case built from the
gate's **real subject as it stands in the tree**, mutated. That distinction is
not a nicety; it is the whole failure. `check-shellquote-vs-bash.py`'s
`--self-test` was green at 25/25 while the gate graded a single line of
`shellquote.rs`, because every one of those 25 cases was a synthetic string the
gate was handed rather than the file the gate is supposed to be reading. A
synthetic fixture proves the comparison logic works on input shaped the way the
author imagined; only a mutated real subject proves the gate is still *attached*
to the thing it claims to grade.

`check-gates-can-refuse.py` covers the neighbouring question — is *any* non-zero
exit reachable on a bare run — and its docstring is explicit that it cannot
cover this one, because doing so means "planting a defect each gate would
notice: 30 bespoke fixtures against 30 unrelated subjects". (That "30" is
historical: the ratchet counted 30 gates when the line was written and counts
**37** today, which is itself the point — the number only grows.) Two of the
thirty-seven now have a real-subject mutation fixture, written by hand during
this investigation. The other **thirty-five** do not, and a gate in that state
is indistinguishable from a working one from outside: same directory, same exit
0, same green self-test, same silence.

The check that failed here is worth naming, because it is cheap and I used it:
asking *does this gate open a file in `kernel/`?* cannot tell a gate that grades
a subsystem from one that grades a single constant inside it. Both answer yes.
The question that separates them is **how much of that file can change without
this noticing**, and it can only be answered by changing something and watching.

**Proper fix for the residue.** A per-gate mutation fixture, in the shape the
two written here already have: a `--self-test` case that takes the gate's real
subject text, plants a defect in it in memory, feeds it to the gate through an
injectable `src` parameter, and asserts a non-zero verdict. The injectable
parameter is the load-bearing part — it is what makes the fixture free of the
working tree, so a gate can prove it refuses without a lane ever writing a
broken file to disk. `rustrungs.py` and both oracles now demonstrate the
pattern; the work is applying it **thirty-five** more times, one gate at a time,
in whichever lane owns each gate's subject. Where a gate already has a
`--self-test`, this is an addition to it rather than a replacement: the synthetic
cases still guard the comparison logic, and the mutated-real-subject case guards
the tether. Both are needed, and only the second was missing.

**If it is never fixed:** the two oracles are now genuinely tethered, so the
specific hole is closed. But the same defect can be reintroduced anywhere else
in `scripts/` and will look exactly like a passing gate, which is how this one
survived being written into a decision record as a "yes". Every gate added from
here is a coin flip on whether it grades anything, resolved only if someone
happens to break its subject and look.

### 2026-09-11 — one gate moved from 2 to 3, and the sweep to do the rest was abandoned on a false premise

**`scripts/check-selftest-reach.py` now has a case of the second kind**: it reads the
real `kernel/src/sockact.rs`, requires the gate to be quiet on it, injects one
destructive reach into its self-test body in memory, and requires exactly that
finding back. Verified it can refuse — a copy with its scope test short-circuited
fails its own suite, exit 1, with the mutation case among the failures. Resolved
script-relative rather than CWD-relative, and checked by running it from `/tmp`,
because a case that *fails* when it cannot find its subject must not hang on an
ambient assumption.

**Then I tried to do the remaining ones in a sweep, and the measurement that was
going to drive it was wrong.** Recording it because the wrong number was plausible
and the work it implied was several hours:

| pass | instrument | answer |
|---|---|---|
| 1 | gate mentions a real tree path anywhere | 26 of 30 "have" one |
| 2 | Python `ast`: does a function named `self_test*` read a file? | only **2** of 28 |
| 3 | read one gate the entry says it fixed | **pass 2 is wrong** |

`check-shellquote-vs-bash.py` is one of the two gates this entry reports fixing, and
pass 2 calls it synthetic-only. It is not. Line 66 is

    RUST = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "shellquote.rs"

— a module-level tether to the real file — and its assertion lives in
`assert_port_matches_rust(src=None)`, which takes an optional override *so that it
can also be fed synthetic source*. A scan for file reads inside functions named
`self_test*` sees neither.

**And the deeper reason the sweep was the wrong idea.** That gate's own docstring
says where its tether actually is:

> `shellquote.rs::self_test()` runs those rungs at boot, and the mutation above
> fails the rung at `shellquote.rs:568`.

For a gate that cross-checks a kernel module, **the kernel's own boot self-test is
the tether** — not a Python case inside the gate. So "add a mutation case to each of
the 26" was not a backlog; it was a plan built on a number produced by an instrument
that could not see either of the two shapes the tree actually uses.

The figure in this entry — *2 of 37 have a case that mutates their actual subject* —
was produced by reading the gates. That is still what it takes, and it is why no
number here should be re-derived by grep. Whoever picks this up should read each
gate and ask **where is this gate's tether to the thing it grades?**, accepting three
legitimate answers: a case of its own, a module-level read of the real file, or a
rung in the kernel that fails when the code is broken. Only a gate with none of the
three is a finding.
