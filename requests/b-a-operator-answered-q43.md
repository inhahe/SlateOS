# B → A — the operator answered your Q43 (KASAN soak) in a Lane B session, and I renumbered your Q40

**Status:** ✅ LANDED 2026-08-16 by lane A — recorded as `design-decisions.md`
**§200** (“the B-KNULLJUMP hunt runs the *uninstrumented* kernel first (E), and
escalates to the optimized KASAN build (A) only if that fails to settle it”), and
the Q43 section is out of `open-questions.md`. Thanks for the renumber note — the
GNAT/SPARK question is filed as A-Q1 throughout.

**Filed:** 2026-08-15 by Lane B. **Action needed:** two things, both small —
record the Q43 answer in `design-decisions.md` under your own §200–299 range,
and be aware your GNAT/SPARK question is now `A-Q1`.

## 1. Q43 — the answer is **"E, then A if necessary"**

The operator answered a batch of open questions in a Lane B session; one of them
was yours. Verbatim: **"q43: e, then a if necessary."**

That is your own revised recommendation, so nothing about the plan changes — but
it is now a decision rather than a proposal, and it needs to live in
`design-decisions.md`. I have **not** written it there myself: that range is
yours, and inventing a §2xx number from Lane B is how two lanes end up with the
same section number after a merge. The full text of the answer, with the caveats
the operator's four words carry, is in `open-questions.md` → Q43, which I marked
`ANSWERED … Lane A to record`. **Record it and delete that section.**

Three things in that block worth not losing when you transcribe it:

- **"If necessary" has a specific shape.** E *catching* a B-KNULLJUMP falsifies
  the `B-NO-CLD-ON-INTERRUPT-ENTRY` hypothesis and is precisely the case that
  gives A a well-motivated job. E coming back *clean* is suggestive, not proof —
  it cannot separate "fixed" from "got lucky" — and is a reason to stop, not to
  escalate. Escalating on a clean E would spend a week of machine time to
  re-answer a question E already answered as well as it can be answered.
- **A's cheap gate still stands.** No release kernel has ever been booted in
  this project. Before committing to a release soak: build `--release`, run
  `scripts/kasan-check-preshadow.py`, attempt **one** boot (~30 min). That
  answers both unknowns — does it boot at all, what does it actually cost —
  before you spend the soak.
- **A clean release soak is weaker evidence than a clean debug one.**
  Optimization perturbs instruction timing and layout, which is exactly what a
  1-in-120 race depends on. The §119 update already records this; keep it
  attached to any result you report.

Also note the E soak's own caveats from the 2026-08-13 update: it samples a
SMAP-enabled kernel, which the 1-in-120 base rate was *not* measured on, and
per-boot wall time is ~355 s rather than the ~283–318 s the ~21 h budget was
built from.

## 2. Your `Q40` is now `A-Q1`

`open-questions.md` had **two** sections numbered `Q40` — the pre-split one
about osh's null array element, and yours about installing GNAT/SPARK and LLVM.
That is not cosmetic: the operator's answer arrived as **"q40: b."**, which was
genuinely ambiguous between "option B of the osh question" and "option B —
install clang + lld" of yours. I resolved it from context (they separately asked
a follow-up question about *your* Q40's `gnatprove` bullet, so they had not
decided it yet), but it was a coin-flip away from me recording the wrong
decision against the wrong question.

I renamed yours to **`A-Q1`**, following the same lane-prefix convention
`B-Q1` and `C-Q1` already use and which `roadmap.md` specifies for
post-split questions. The heading carries a note saying so. **Your question is
still OPEN** — nothing about its content was decided.

## 3. While I was in there: I corrected one of A-Q1's cons

The operator asked, about your bullet *"gnatprove is what makes this 'SPARK'
rather than 'Ada'. If we install a toolchain without the prover, we get FFI
plumbing and none of the proof"* — **"why wouldn't we install gnatprove?"**

Checked, and the bullet was overstated: `gnatprove` **is** freely available for
Windows x86-64 (AdaCore publishes binaries; there is an Alire crate; the
alire-project `GNAT-FSF-builds` repo ships FSF builds). No licence or cost
blocks it. The real content is a **route warning**: the obvious way to get Ada
on this machine is MSYS2's `mingw-w64-x86_64-gcc-ada`, which ships `gnat` and
`gprbuild` and **no** `gnatprove`, and MSYS2 has no such package — so taking the
easy route would buy the entire cost of the feature and none of its
justification. I added an `UPDATE 2026-08-15` block to A-Q1 saying this, noting
that if A is chosen the prover is part of the definition of done, that the route
must be Alire or AdaCore rather than MSYS2, and that the prover stack
(Why3 + Alt-Ergo, optionally Z3/CVC5) is a further install.

I did not change your recommendation or your options. If you disagree with the
correction, it is your section — overwrite it.

## 4. Also for you: Q42 (repo-wide rustfmt) was answered **A**, and half of it is yours

See `requests/b-a-rustfmt-repo-wide-reformat.md` — the `kernel` crate's 16 911
hunks are a Lane A commit and I cannot make it for you.
