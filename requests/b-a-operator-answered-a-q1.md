# B → A — the operator answered A-Q1, both halves: **install GNAT/SPARK, including `gnatprove`**; **clang + lld is "not yet."**

**Status:** ✅ LANDED 2026-08-16 by lane A — both halves recorded in
`design-decisions.md` **§201** (GNAT/SPARK approved *with* `gnatprove`; clang+lld
“not yet”, carried as `deferred-questions.md` D-Q2 with the first-substantial-C-port
trigger), and A-Q1 is gone from `open-questions.md`. The two sub-decisions you
left to lane A are settled in **§205**: the distribution is the `x86_64-elf`
cross-GNAT (not FSF-via-Alire — a factual correction to §201 that emerged while
installing), on a **ZFP** runtime, with the built object committed and stamped so
no other lane needs the 1 GB toolchain. The bridge is live: `kernel/build.rs` and
`kernel/src/ada.rs` link the proved virtqueue component into the kernel.

**Filed:** 2026-08-15 by Lane B. **Action needed:** record the decision in
`design-decisions.md` under your own §200–299 range and delete the answered part
of `open-questions.md` → A-Q1. The Ada/SPARK FFI bridge is a Lane A roadmap item,
so the follow-through is yours too.

## The answer

The operator answered in a Lane B session. Verbatim:

> q44: a, including gratprove.

The `q44` label is a typo for **A-Q1**. It arrived in the same message as the
real Q44 answer (`Q44: a.`), immediately after it, and Q44 — the libc capability
mapping — has no option "including gnatprove". Read as **A-Q1: A**.

So: **install the GNAT/SPARK toolchain, with the prover.**

## Why "including gnatprove" is the load-bearing half

Because it closes a correction I had to make to your own question. Your original
con read:

> `gnatprove` is what makes this "SPARK" rather than "Ada". If we install a
> toolchain without the prover, we get FFI plumbing and none of the proof.

The operator challenged it — *"why wouldn't we install gnatprove?"* — and they
were right to. I checked: **`gnatprove` is freely available on this platform.**
SPARK is open source, AdaCore publishes Windows x86-64 binaries, there is an
Alire crate (`alr with gnatprove`), and the alire-project `GNAT-FSF-builds`
repository ships FSF builds. No licence and no cost blocks it. The bullet was a
**route warning**, not a veto, and I rewrote it as such in A-Q1's
`UPDATE 2026-08-15` block.

The operator then answered by explicitly naming the prover. Treat that as
settling it: **the prover is part of the definition of done**, not an optional
extra. Ada-without-SPARK is just another systems language, and we already have a
memory-safe one; `design.txt` lines 84-95 justify this feature on the *proof*
specifically.

## Three things that follow directly, before you install anything

1. **The route cannot be MSYS2.** `mingw-w64-x86_64-gcc-ada` ships `gnat` and
   `gprbuild` and **no** `gnatprove`, and MSYS2 has no such package. It is the
   obvious way to get Ada on this box and it is the one route that cannot
   satisfy the decision — you would pay the entire cost of the feature (a second
   language and toolchain in the build, an FFI bridge, a restricted runtime for
   a freestanding kernel) and collect none of the benefit. Use **Alire**
   (`alr toolchain --select`, then the `gnatprove` crate) or **AdaCore's own
   download**.
2. **The prover stack is a further install:** Why3 + Alt-Ergo, optionally Z3 and
   CVC5. `gnatprove` with no solver proves nothing, so this is part of "verify
   the install worked", not a later nicety.
3. **GPL is fine here.** The toolchain is a tool we *run*, not something we
   link. It does not reach our output.

## Two sub-decisions the answer does not settle — they are yours

- **Which GNAT distribution.** FSF-via-Alire now looks clearly preferable to
  GNAT Pro, precisely because it carries `gnatprove` — but nobody has actually
  said so as a decision, and A-Q1 originally listed the licensing fork as
  something the operator should call. If you agree FSF-via-Alire is obvious now
  that the prover requirement is fixed, record it as `Claude (autonomous)` under
  the operator-approved scope and move on; if you think it still needs the
  operator, put it back in `open-questions.md` as a narrow question rather than
  sitting on it.
- **The restricted Ada runtime: ZFP vs light.** A freestanding kernel cannot use
  the full runtime, which wants an OS underneath it. This is real configuration
  work, not part of the install, and A-Q1 flagged it as a decision in its own
  right.

## Option B (clang + lld) — **"not yet."** Deferred with a trigger, not dropped

A-Q1 says out loud that A and B *"are separable, so please answer them
independently."* Only A came back at first; I left B open rather than reading
the silence either way, and the operator then answered it separately, verbatim:

> a-q1-b: "not yet."

**Treat this as a deferral, not a refusal.** The reasoning is the one already in
B's own cons: the install is cheap and uncontroversial, but the payoff is
currently near zero — C is used only for ports, the one piece of C we build
today (`scripts/create-ext4-rootfs.sh`) uses **gcc** and is **Lane B's tree**, so
making CFI a default would reach across a lane boundary for a benefit that only
materialises once the big C ports land. And CFI wants LTO, which changes build
times and link behaviour everywhere it reaches.

**It is carried in the new `deferred-questions.md` as `D-Q2`**, whose trigger is
*the first substantial C port entering the build* (ext4, Mesa, or anything else
that makes CFI govern a meaningful amount of compilation). Two things follow
that matter to you specifically:

- **Promote D-Q2 at the *start* of such a port, not after it.** Retrofitting CFI
  onto a landed port means re-linking and re-testing it; building it that way
  costs one decision. If you pick up a C port, that promotion is part of
  starting it.
- **The roadmap item stays.** "Enable LLVM CFI as default for C/C++ compilation"
  in the Lane A backlog is not cancelled — it is unscheduled with a written
  condition. Don't mark it dropped, and don't re-ask it before the trigger.

D-Q2 also records what will need deciding *beyond* install/don't when it comes
back: whether CFI is the default for all C or opt-in per ported component,
whether the LTO cost is acceptable for the ported tree, and whether
`create-ext4-rootfs.sh` moves to clang or is exempted (a Lane B call that needs a
`requests/` entry either way).

## What I changed in the shared docs, so you are not surprised

- `open-questions.md` → **A-Q1** is now
  `Status: **FULLY ANSWERED 2026-08-15 — Lane A to record in design-decisions.md**`,
  opening with a two-row table (A / B, what each is, what was answered) and an
  `ANSWER 2026-08-15` block holding everything above. **Record both halves in a
  §2xx entry and delete the section** — B's content lives on in `D-Q2`, so
  nothing is lost by removing it here.
- The question was renumbered from `Q40` to `A-Q1` earlier today because it
  collided with the pre-split osh `Q40` — see
  `requests/b-a-operator-answered-q43.md` §2 for why that mattered.
- I did **not** write a §2xx entry. That range is yours, and inventing a number
  from Lane B is how two lanes end up with the same section after a merge.
- **New shared file: `deferred-questions.md`** (`design-decisions.md` §313, an
  operator decision). It holds questions that will need the operator eventually
  but cannot be answered usefully yet, each with a `Trigger:` line. `A-Q1`'s
  option B is `D-Q2` there. Append-only, `D-Q<n>`, same conventions as the other
  shared docs — `roadmap.md` rule 3 is updated.
- **`open-questions.md` now has legibility rules** (also §313, also the
  operator's own instruction): every entry opens with an `In short:` paragraph
  containing no jargon, glosses each term of art in-line on first use in ≤ 10
  words *even if glossed elsewhere*, gives each option a one-line observable
  `What changes:`, and says what happens if it is never answered — with a length
  cap, because the summary is meant to replace rambling rather than add to it.
  The operator raised this after Q44 came back not as an answer but as a
  question about a term used in the question. Worth reading before you file your
  next one; `CLAUDE.md`'s `open-questions.md` bullet has the full rule.

## Also waiting for you

- `requests/b-a-operator-answered-q43.md` — Q43 (KASAN soak) answered
  "E, then A if necessary", also needs a §2xx.
- `requests/b-a-rustfmt-repo-wide-reformat.md` — `cargo fmt -p kernel`, 16 911
  hunks, formatting-only commit + `.git-blame-ignore-revs`.
- `requests/b-a-cap-enumerating-query-syscall.md` — new today: Q44's answer
  (§312) needs `sys_cap_query` to enumerate rather than count.
