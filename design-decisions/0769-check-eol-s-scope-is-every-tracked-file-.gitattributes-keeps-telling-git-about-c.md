## 769. `check-eol`'s scope is every tracked file; `.gitattributes` keeps telling git about checkout and stops doubling as the gate's reading list

**Date:** 2026-09-04
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** a checker exists to notice when a tool has written Windows line
endings (a carriage return before every newline) into a source file, because
that silently breaks shell scripts and no `git` command will show it to you. It
was only ever *looking at* the file types listed in `.gitattributes` — `.sh`,
`.py`, `.yaml`, `.yml`, `.md`, `.txt`. That list does not include `.rs` or
`.toml`, which is most of the code. So it read a worktree holding 49 corrupted
files and reported it clean, truthfully, for as long as it had existed. The
choice was whether to add the missing types to that list or to stop using the
list as the scope. I stopped using the list.

**The measurement that forced it**, taken on `os-lane-b` while chasing an
unrelated `git` warning about one `Cargo.toml`:

| population | files holding a carriage return |
|---|---|
| declared `text eol=lf` — what the gate read | **0** |
| tracked `*.rs` and `*.toml` — declared by nothing | 27 |
| every tracked file (22 further hits are genuine binaries) | **49** |

**Why the original scope was chosen, and why that reason does not survive.** The
gate's docstring argued the scope should be "the promise": read the set out of
git rather than duplicating it here as a suffix list, since a second copy of the
policy is free to drift from the first. That is a good argument and it is about
the wrong policy. `.gitattributes` answers *what should git normalise at
checkout*; the gate asks *where might a tool have written the wrong bytes*.
Those two questions have different right answers, and tying the second to the
first bought automatic coverage of newly-declared types at the price of
permanent blindness to never-declared ones. The docstring noticed only the first
half of that trade.

**Option A — add `*.rs text eol=lf` (and `*.toml`, …) to `.gitattributes`.**
*What changes:* the gate reports Rust files; git also rewrites them to LF at
checkout. This is the fix lane A proposed in `known-issues.md` →
`A-27-KERNEL-SOURCES-ARE-CRLF-…` on 2026-08-18, deferred there on the objection
that a shared root file "needs to be coordinated, not dropped in by one lane".

- *For:* it is the only option that also **prevents** — an attribute makes git
  write LF on checkout, where a gate can only report after the fact. It is one
  line, and A-27's stated blocker has expired: a root `.gitattributes` now
  exists and already carries six rules three lanes depend on.
- *Against:* it is still a suffix list, so it fixes today's gap and not the
  shape of it — the next text type nobody thinks to declare (`.c`, `.json`, an
  `.ld` script) is invisible exactly as `.rs` was, and nothing will report that
  it is. It also makes a file governing three lanes' checkout behaviour
  load-bearing for one gate's coverage, which is how the coupling arose in the
  first place. And the prevention it buys is weaker than it sounds: the
  attribute acts at checkout, while every occurrence in this tree came from a
  tool rewriting a file *after* checkout, which no attribute can intercept.

**Option B — scope the gate to every tracked file. (chosen)**
*What changes:* the gate reads all 13 908 tracked paths instead of ~1 440;
`.gitattributes` is untouched and keeps its own job.

- *For:* there is no list, so there is nothing to drift. It cannot under-cover
  again by construction. It found 27 files the previous scope could not see, in
  the first run. And it *deletes* the gate's most fragile part as the scope: the
  `git check-attr` stream walk, which the docstring itself called "the half that
  can silently empty out", now costs one heuristic rather than the whole gate if
  it breaks.
- *Against:* it costs more. Measured, not guessed: 13 908 files / 195 MB in 44 s
  at 48 threads, against 73 s at 16 and 43.7 s at 96 — flat past 48, so the
  thread count sits at the knee. The real run went ~20 s → 56 s. It also means
  reading binaries, which needs the exclusion below.

**Why not both.** Nothing stops a later lane doing A as well, and if lane A
wants the checkout-time normalisation it should. But A was not worth doing *for
this purpose*: once B is in, declaring `*.rs` changes nothing about what is
reported, so the only remaining argument for A is prevention-at-checkout — which
is A's to make, in A's tree, on A's evidence, and is not blocked by anything
here. Doing it silently as part of this change would have been one lane altering
three lanes' checkout behaviour to buy a coverage property it had just obtained
another way.

**The binary exclusion, and the one heuristic that cannot be used naively.**
Reading everything means reading PNGs, where a `\r` is data. The test for binary
is git's own — a NUL byte anywhere — with git's own bug fixed: a file declared
`text` is *asserted* to be text and is scanned whatever it holds. Skipping on
NUL alone would reproduce, exactly, the failure recorded in `.gitattributes`
itself, where two NUL bytes 4.5 MB into `known-issues.md` (inside backticks
quoting `grep -Z` output) made git call a 4.5 MB text document binary and stop
normalising it. So the attribute query survives, doing this one narrow job and
not the scope. That is also what keeps the property honest rather than
convenient: the self-test grades the override as a *pair* — identical bytes,
differing only in whether the path is asserted text, one skipped and one
reported.

**Severity is untouched.** §764's split still decides refusal: a carriage return
stops the build only in a file some machine executes from disk (a `.sh`, or
anything opening `#!`). All 27 findings in the first wide run were reported and
none was fatal, so widening the scope did not newly block any lane — which was a
precondition for making the change autonomously rather than asking. Note the one
real consequence to watch: an *undeclared* file that is run from disk (a
shebanged script with no suffix) was invisible before and is fatal now, which is
correct but is a new way for a build to stop.

**That precondition was then checked against the other two lanes rather than
assumed,** by running both the new gate and the pre-change one in their
worktrees and comparing the *fatal* sets:

| Worktree | New: reported / fatal | Old: fatal | New stoppages |
|---|---|---|---|
| `os-lane-a` | 0 / 0 | 0 | 0 |
| `os-lane-b` | 0 / 0 | 0 | 0 |
| `os-lane-c` | 65 / 4 | **4 — the same four** | 0 |

Lane C's four are `.py` files with real shebangs, already declared `text eol=lf`
and so already in the old gate's scope: that build is refused today and this
change does not alter it. What the change does for lane C is raise *reported*
from 31 to 65. No lane gains a build stoppage.

**The check paid for itself, because the first wide run across another lane's
tree found a bug in this gate rather than in that tree.** Lane C's
`gui/toolkit/src/colorpicker.rs` came back *fatal* — a Rust source file
supposedly executed from disk. The severity test read `data.startswith(b"#!")`,
and `#![deny(clippy::all)]` opens with those two bytes. The rule had been
correct for as long as it was never shown a `.rs`, which is to say it was
untested rather than right. Fixed in the same breath by requiring what a real
shebang requires — an interpreter *path*, since `execve` does no `PATH` search,
so the token after `#!` must begin with `/`.

The general lesson is worth more than the fix: **widening what a checker looks
at also widens what its heuristics are wrong about.** A scope change should be
expected to surface false positives in the rules that were only ever exercised
on the narrow set, and the cross-lane dry run is what converts that from an
outage into an edit.

**If this is wrong,** the revert is one function's argument: `check` takes its
path list as a parameter, so restoring the old behaviour is passing
`declared_lf(paths)` where `paths` now goes.
