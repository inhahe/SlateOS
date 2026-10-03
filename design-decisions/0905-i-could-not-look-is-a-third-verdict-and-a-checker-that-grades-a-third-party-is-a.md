## §905 — "I could not look" is a third verdict, and a checker that grades a third party is an instrument rather than a gate

**Date:** 2026-09-03. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** The boot test runs about thirty small checking programs before it
builds anything. Each one answers yes or no, and `run_checker` — the wrapper
they all go through — treats any other answer as a bug and stops the build, on
the principle that a checker which did not reach a verdict must not be mistaken
for one that reached "fine". That was right, but it left several perfectly good
checkers permanently switched off, because they legitimately cannot answer on
some machines: one needs a Linux shell that not every developer has installed,
another grades a file produced by a build step that a fresh checkout has not
run. Three decisions here. Checkers may now declare, per call site, one exit
code that means *"I could not look"*, and the build says so loudly and
continues. A checker that could not reach its instrument must use that code and
never exit 0 or 1. And two of the four checkers waiting on this turned out not
to belong in the boot test at all, for a reason that has nothing to do with
availability: they do not read our code.

The three parts landed as `e7d9573b9` (the channel), `0662772f8` (the exit
split), `cb29ea5dc` and `5c3a57267` (the classification and the wiring).
Requested by lane B in
`requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md`,
which lists it as the one change five pinned gates were all waiting on.

### The channel is opt-in per call site, not a property of the checker

`run_checker --may-skip=2 <label> <command>` says: *at this call site*, exit 2
from this checker means it could not look. Any other unexpected code still
aborts the build.

> **Spelling superseded, same day — the argument below is unchanged.** Lane B
> had written the same channel independently, and the merge (`a29a07d68`) kept
> lane B's spelling: the flag is bare **`--may-skip`**, with 2 hard-coded as the
> tree's one code for "I did not reach a verdict", rather than
> `--may-skip=<rc>` naming the code per site. Everything this section argues —
> opt-in per call site, permission living in the grader not the graded — holds
> identically for the bare form, which is why the merge was not contested. What
> is lost is the ability to allow a *different* code per site, and that turned
> out to be a freedom nobody wanted: a second no-verdict code would be a second
> convention to remember. The reason string is also lane B's: the checker's
> **first** line of output, not its last, and a skip is refused outright if that
> line is a `usage:` banner, a traceback, or empty. Read `--may-skip=2` below as
> `--may-skip`.

The alternative was to let the checker declare it — a convention that exit 2
always means "could not look", enforced nowhere. That is the cheaper design and
it is wrong here, for a reason this repo has already been bitten by: the
declaration would live in the file being graded rather than in the file doing
the grading. A checker that acquires a new exit-2 path — a bare `return 2` from
some later error branch, added by someone who never read this rule — would
silently convert an abort into a skip, and a skip reads as *nothing was wrong*.
Putting the permission at the call site means widening what may be skipped is
an edit to `boot-test.sh`, where the ratchet and the label-distinctness suite
can both see it.

The cost is real: five call sites now repeat `--may-skip=2`, and a sixth that
forgets it will abort the build on a host that should have skipped. That is the
right failure direction — it stops the build and names the gate, rather than
passing.

Against the whole idea: a skip is a hole, and holes accumulate. The mitigation
is that the skip is *loud* — it prints the checker's own last line as the
reason and appends a row to `CHECKER_SKIPLOG`, so a machine that never builds a
sysroot is told every single boot which checks are not being made. A silent
skip would have been strictly worse than the pin it replaced.

### Why the exit code has to be split at the checker too

`bashprobe.py` left via `raise SystemExit("no WSL")` when the Linux shell was
absent, which Python maps to **exit 1** — the code that means *"I looked and
found a problem."* On a machine without WSL, four checkers therefore reported
that our shell quoting disagreed with bash, about a bash they never reached.
Lane B found this while writing the pin and flagged it as the first thing to
fix, correctly.

It now has three endings that cannot be confused: 0 or 1 after actually
comparing, **2** for "I could not look", and an uncaught exception — a
traceback, not a tidy exit — for the case where the comparison machinery itself
is broken. The third deserves the ugliest ending on purpose: if the harness is
wrong then no result it produces means anything, including a clean one, and
that must not be expressible as a number the caller might handle.

### The classification, which is the part that generalises

Four checkers were pinned as "needs WSL". Once WSL-dependence became wireable,
the obvious move was to wire all four. Reading their inputs says otherwise, and
the distinction is not about availability at all:

| | reads | can a change to our tree red it? |
|---|---|---|
| `check-shellquote-vs-bash.py` | `kernel/src/shellquote.rs` + bash | yes — **wired** |
| `check-kshell-rungs-vs-bash.py` | `kernel/src/kshell.rs` + bash | yes — **wired** |
| `check-kshell-pipeline-vs-bash.py` | a Python table + bash | no — **pinned** |
| `check-ansic-quoting-vs-bash.py` | a Python table + bash | no — **pinned** |

The bottom two open no `.rs` file. They compare a written-down model of bash
against real bash, and their own docstring says a disagreement means the model
is wrong, not bash. Wiring them would add about 23 seconds to every boot to
guard nothing, and — worse — would be *read* as coverage of our quoting code by
anyone scanning the gate list.

That is the general rule this entry exists to record: **a program that measures
a third party is an instrument, and only a program that grades this repository
is a gate.** They look identical from outside — both live in `scripts/`, both
exit 0, both print little — which is exactly why the difference has to be
written down where the list of gates is kept, rather than left to be
rediscovered. The two instruments stay valuable and stay run by hand: they are
how bash's answers are learned before those answers are written into kshell's
own self-test rungs, and the rungs *are* gated.

Against this split: it is a judgement call per checker, and a wrong call is
invisible — a misfiled instrument is a gate nobody runs. The pins therefore
carry the unpinning condition explicitly ("wire it if it ever grows an
assertion against `kernel/src/kshell.rs`"), so the question is re-asked by the
file itself rather than by memory.

### What is still missing, and is not fixed by any of this

None of the four has a `--self-test`, so nothing proves either of the two now
wired can still *find* anything. They scan Rust source by regex for literals,
which means a rename makes them match nothing and report a clean tree — the
failure this whole family of gates exists to prevent, in the two gates just
added to the family. Lane B predicted this in the reply cited above and listed
it as step 3 of four; it is tracked in `known-issues.md` →
`TD-B-THE-FOUR-BASH-ORACLES-ARE-PINNED-NOT-WIRED` and is the immediate next
piece of work, not a deferral.

### Correction, same day — "yes" in that table was true but far too narrow

**In short:** the table above answers "can a change to our tree red it?" with
"yes" for the two gates I wired. I established that by reading which file each
one opens, which is the wrong question. Both do open a kernel source file — and
then read one line of it. Afterwards I mutated each gate's subject to see what
it would catch, and both reported a clean tree while looking straight at a
defect. The gates were worth having and stayed wired; the claim that a kernel
change reds them needed narrowing to what was actually true, which was a much
smaller thing than the table implies. Both are now fixed.

What each one actually read, before the fix:

| gate | its whole tether to our tree | what could change without it noticing |
|---|---|---|
| `check-shellquote-vs-bash.py` | one regex for `const DQ_ESCAPABLE: [u8; N] = [...]` | every other line of `shellquote.rs` — the entire scanner |
| `check-kshell-rungs-vs-bash.py` | the rung *inputs* occur verbatim (`assert_rust_src_is_verbatim`) | every rung's *expected value* |

Both were established by mutation, not by reading:

- Changing `shellquote.rs`'s `Ctx::Single` arm to `let structural = false;` — a
  scanner in which a single-quoted string can never close — gave
  `0 failure(s)`, exit 0. The gate did catch a renamed or re-shaped
  `DQ_ESCAPABLE` and a drifted alphabet, which was the whole of what it caught.
- Corrupting a kshell rung's expectation from `alloc::vec!["a", "b", "c"]` to
  `alloc::vec!["a", "b", "", "c"]` — a blank word bash never produces — gave
  "0 rung assertion(s) disagree with the reference tool", exit 0.

**The sentence in "What is still missing" was also wrong, and wrong in the
direction that matters.** It said these gates "scan Rust source by regex for
literals, which means a rename makes them match nothing and report a clean
tree". For `check-kshell-rungs-vs-bash.py` a renamed *input* literal does not
pass quietly — `assert_rust_src_is_verbatim()` fails the run, and that check was
added precisely to be the discovery floor. The real hole was not a rename going
unnoticed; it was that the expectation side of every rung was never read at all.
I had described a weaker version of the defect than the one present, which is
worse than describing none, because it reads as though the floor were the gap.

### What generalises: a tether's width is not visible from its name

The check I ran when classifying these — *does it open a file in `kernel/`?* —
cannot distinguish a gate that grades a subsystem from one that grades a single
constant inside it. Both answer yes. The question that discriminates is **how
much of that file can change without this noticing**, and the only way to answer
it is to change something and watch. That is a mutation test, and its absence is
why a wrong answer survived being written into a decision record.

This also sharpens this entry's own framing. The split above between
*instruments* (measure a third party) and *gates* (grade this repository) was
treated as binary. It is not: a gate can grade this repository through a tether
one line wide, and from outside — same directory, same exit 0, same silence — it
is indistinguishable from one that grades the subsystem its name claims.
`check-gates-can-refuse.py` already documents the neighbouring version of this
("a gate that passes on a clean tree is indistinguishable from a gate that
passes on everything") and explains why it cannot test for it: doing so means
"planting a defect each gate would notice — 30 bespoke fixtures against 30
unrelated subjects". Two of those thirty fixtures now exist, written by hand
during this work; the other twenty-eight do not, and that residue is tracked in
`known-issues.md` →
`TD-A-A-WIRED-GATE-CAN-GRADE-ONE-LINE-AND-LOOK-LIKE-IT-GRADES-A-SUBSYSTEM`.

Worth recording that the false-clean had two channels, not one. `pre-boot.py`
globs `scripts/check-*.py` and runs every gate bare, so both oracles had been
reporting clean there as well for as long as they had existed — not only from
`boot-test.sh` since that morning. That does not change the wiring argument
(pre-boot is a ~40-minute local pre-flight nobody is obliged to run, which is
exactly why being named in `boot-test.sh` is what "wired" means), but it does
mean the reassurance was being printed twice.

### The fix, and the part of it that is a refusal to pretend

Landed as `a6551a3af` (kshell) and `d280f66b1` (shellquote). Rung expectations
are now read out of the Rust by `scripts/rustrungs.py`, a shared reader — both
`kshell.rs` and `shellquote.rs::self_test()` assert in the same
`assert_eq!(call(literal), expected)` shape, so one parser serves both. Three
choices inside that work had a case on either side:

- **Three witnesses, not two.** The transcription in each gate's own table is
  kept rather than dropped as redundant, so a gate now requires the rung's own
  expectation, its transcription, and real bash to agree. Dropping the
  transcription is tempting — it is duplicated data that must be maintained —
  but then a silently broken *reader* passes by comparing bash against bash.
  Same argument that put `assert_rust_src_is_verbatim()` there in the first
  place.
- **Coverage is a separate failure from correctness, and is reported
  separately.** A table-driven oracle cannot see a rung nobody transcribed:
  `expectations()` answers "does the tree still assert what my table says",
  which is silent about a rung the table omits. So `assert_eq_calls()`
  enumerates every asserted call in the Rust and the gate raises on any it does
  not account for, naming the rung and the table to add it to. It *raises*
  rather than incrementing the failure count, because an ungraded rung is an
  unasked question, not a wrong answer. Reading the Rust this way found three
  kshell rungs graded by nothing (13 → 16) — which is the whole defect class,
  found a second time by the machinery built to prevent it.
- **What cannot be graded is named, not quietly dropped.** Of `shellquote.rs`'s
  thirteen rungs: five `strip_quotes` rungs are gradeable against real bash; one
  (`strip_quotes(b"a\\")`) is a deliberate divergence from bash and is pinned
  three ways so that *agreeing* with bash reds the gate; the five `find_bare`
  and two `bare_positions` rungs assert byte offsets, which bash does not
  expose, so they are graded against the Python port and labelled as such; two
  are excused by name, being inside a round-trip loop with no fixed literal.
  Likewise `remove_quotes("a 'b,c'")` is excluded from the kshell oracle with
  its reason in the code — its input has an unquoted space, so bash splits it
  into two words while `remove_quotes` splits nothing, and any `want` that
  greens one leg reds the other. Covering it would be the same false coverage
  this correction is about.

The one thing not fixed, and deliberately: `check-shellquote-vs-bash.py` still
does not grade the scanner. A Python gate cannot execute Rust, so the
`Ctx::Single` mutation is caught by `self_test()` at boot and by nothing in
`scripts/`. The gate's docstring now says so in as many words, with that
measurement in it, rather than letting its name imply otherwise.

### Second correction — the instrument/gate split was wrong too, and lane B's tree already says so

**In short:** the table above pins two of the four oracles on the grounds that
they "read no `.rs` file" and so guard nothing of ours. Lane B had independently
wired all four, that is what the merge (`a29a07d68`) brought in, and it is what
`boot-test.sh` does today — `check_bash_oracles` runs all four, gates carrying
`--may-skip`, self-tests not. Only `check-evdev-elf-asm.py` is still pinned.
Lane B is right and I was wrong, so the wiring stands and this section is the
record of why my reasoning failed rather than an argument to revisit it.

**Lane B's reason, which I did not have.** It is in the comment above
`check_bash_oracles` in `boot-test.sh`:

> Every other shell gate here reads kshell's source and checks it against a rule
> written down in this repository. These four check the *rule* — they hand the
> same bytes to real bash through WSL and compare. A disagreement means our
> model of the shell is wrong, which no amount of internal consistency would
> ever reveal: the rest of the gates would go on agreeing with each other about
> the wrong answer.

That is the argument I was missing. I asked what a checker *reads* and concluded
that one reading only a Python table protects nothing here. What it protects is
the **rule** that a dozen other gates enforce against our source. If the rule is
wrong, every gate that agrees with it is confidently wrong *together*, and their
agreement is the thing that makes the error invisible. A checker that reads none
of our files can still be the only witness that our files are being measured
against the right standard.

**Both of this entry's errors have one cause.** I classified checkers by *which
file each one opens* rather than by *what would go undetected without it*, and
that question got the answer wrong in both directions on the same day:

| | I asked | I concluded | what was true |
|---|---|---|---|
| the two wired gates | opens `kernel/src/*.rs`? yes | grades that subsystem | graded one constant and one set of inputs |
| the two pinned instruments | opens `kernel/src/*.rs`? no | guards nothing of ours | guards the rule every other shell gate enforces |

Over-credit and under-credit, from one bad question. The correction above says
the discriminating question is "how much of that file can change without this
noticing"; this one extends it, because that phrasing still presumes the subject
is a file in our tree. The general form is **what becomes undetectable if this
program stops running**, which is answerable for an instrument as well as a
gate — and for the instruments the answer is "that our whole model of bash is
wrong", which is not nothing and is not smaller than what a gate protects.

**What survives of the split.** The distinction between measuring a third party
and grading this repository is still real and still worth naming — it is why the
two instruments genuinely cannot fail on a change to `kernel/`, and why reading
them as coverage of our quoting code would be a mistake. What does not survive is
the *conclusion* drawn from it, that only the second kind belongs in the boot
test. The cost that made me draw it — about 23 seconds per boot — is paid only on
hosts that have WSL, and `--may-skip` means a host without WSL skips them loudly
rather than failing. Twenty-three seconds to know that the standard every other
shell gate is measured against is the real one is obviously worth paying, and I
priced it against the wrong benefit.

Worth noting that lane B's argument is the same one I used, one level down, to
justify keeping each gate's own transcription of a rung as a third witness: with
only two witnesses a silently broken reader passes by comparing bash against
bash. Wiring the instruments is that argument applied to the family as a whole.
I made it about a table and missed it about the tree.
