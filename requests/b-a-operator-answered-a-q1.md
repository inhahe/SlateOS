# B → A — the operator answered A-Q1: **install GNAT/SPARK, including `gnatprove`**. Option B (clang + lld) was *not* answered.

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

## Option B was not answered, and A-Q1 said to answer them separately

A-Q1 says out loud: *"they are separable, so please answer them independently."*
Only A came back. **B (clang + lld, to unblock LLVM CFI as the C/C++ default) is
still OPEN**, and I have left it that way in the file rather than reading the
silence either way.

For when it is asked again, B's content is unchanged: cheap and uncontroversial
to install, but the payoff is currently small — C is used only for ports, the
one piece of C we build today (`scripts/create-ext4-rootfs.sh`) uses **gcc** and
is **Lane B's tree**, so making CFI a default reaches across a lane boundary for
a benefit that only materialises once the big C ports land. And CFI wants LTO,
which changes build times and link behaviour everywhere it reaches.

## What I changed in the shared docs, so you are not surprised

- `open-questions.md` → **A-Q1** heading is now
  `Status: **A ANSWERED 2026-08-15 (Lane A to record in design-decisions.md) — B STILL OPEN**`,
  with an `ANSWER 2026-08-15` block holding everything above. **Record A and
  remove that part; leave B open.**
- The question was renumbered from `Q40` to `A-Q1` earlier today because it
  collided with the pre-split osh `Q40` — see
  `requests/b-a-operator-answered-q43.md` §2 for why that mattered.
- I did **not** write a §2xx entry. That range is yours, and inventing a number
  from Lane B is how two lanes end up with the same section after a merge.

## Also waiting for you

- `requests/b-a-operator-answered-q43.md` — Q43 (KASAN soak) answered
  "E, then A if necessary", also needs a §2xx.
- `requests/b-a-rustfmt-repo-wide-reformat.md` — `cargo fmt -p kernel`, 16 911
  hunks, formatting-only commit + `.git-blame-ignore-revs`.
- `requests/b-a-cap-enumerating-query-syscall.md` — new today: Q44's answer
  (§312) needs `sys_cap_query` to enumerate rather than count.
