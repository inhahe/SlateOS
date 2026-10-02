## §201 — Install the GNAT/SPARK toolchain **with `gnatprove`**; clang + lld (and therefore CFI) is deferred, not refused

**Date:** 2026-08-15
**Decided by:** Operator (both halves; on the prover, the operator challenged
Claude's original framing and was right — see below)
**Lane:** A

**In short:** two unrelated compiler installs were bundled into one question.
**Ada/SPARK** is a second programming language whose toolchain can
mathematically *prove* that driver code has no buffer overflows and no illegal
state transitions; `design.txt` (lines 84–95) wants it for safety-critical
drivers. That one is **approved, including the prover program `gnatprove`**.
**clang + lld** is an alternative C compiler and linker, whose point would be
enabling **CFI** (Control-Flow Integrity — a compiler feature that stops an
attacker redirecting a function call to code of their choosing). That one is
**"not yet"**: deferred with a trigger, not rejected.

**On the prover — why "including gnatprove" is the load-bearing half.** The
original question carried a con reading, in effect, *"if we install a toolchain
without the prover we get FFI plumbing and none of the proof."* The operator
challenged it — *"why wouldn't we install gnatprove?"* — and that challenge was
correct on the facts: `gnatprove` is **freely available on this platform**.
SPARK is open source, AdaCore publishes Windows x86-64 binaries, there is an
Alire crate (`alr with gnatprove`), and `GNAT-FSF-builds` ships FSF builds. No
licence and no cost blocks it. The bullet was a **route warning**, not a veto.
The operator then answered by explicitly naming the prover, which settles it:
**the prover is part of the definition of done.** Ada-without-SPARK is just
another systems language, and we already have a memory-safe one — the feature
is justified in `design.txt` on the *proof* specifically.

**Three consequences that follow directly:**

1. **The install route cannot be MSYS2.** `mingw-w64-x86_64-gcc-ada` ships
   `gnat` and `gprbuild` and **no** `gnatprove`, and MSYS2 has no such package.
   Taking the easy route would buy the entire cost of the feature and none of
   its justification. The route must be **Alire** (`alr toolchain --select`,
   then the `gnatprove` crate) or **AdaCore's own download**.
2. **The prover stack is a further install:** Why3 + Alt-Ergo, optionally Z3 and
   CVC5. `gnatprove` without a solver proves nothing.
3. **GPL is not a problem here.** The toolchain is a tool we *run*, not
   something we link; it does not reach our output.

**Two sub-decisions this does *not* settle — they are Lane A's to make:**

- **Which GNAT distribution.** FSF-via-Alire now looks clearly preferable to
  GNAT Pro precisely because it carries `gnatprove`, but nobody has recorded
  that as a decision.
- **The restricted runtime: ZFP vs light.** A freestanding kernel cannot use
  the full Ada runtime, which wants an OS underneath it. Configuration work
  with real content, not part of the install.

**On clang + lld — "not yet" is a deferral with a trigger.** The install is
small and uncontroversial; what is missing is a *reason*. We use C only for
ported code, and the one piece of C compiled today
(`scripts/create-ext4-rootfs.sh`) is built with gcc — so enabling CFI now would
change Lane B's build for a benefit that only materialises when the large C
ports land, and would pull in LTO (whole-program optimization at link time),
which slows every build it touches. Nothing is blocked by waiting. It moves to
`deferred-questions.md` as **D-Q2**, with the trigger being **the first
substantial C port entering the build**, so it returns when the payoff is real
instead of being quietly dropped.

**Where it lives:** the Ada/SPARK FFI bridge is a Lane A roadmap item, so the
follow-through is Lane A's; `deferred-questions.md` → D-Q2 for the clang half;
`design.txt` lines 84–95 for the original justification.

**Provenance:** the operator answered in a Lane B session, verbatim *"q44: a,
including gratprove."* The `q44` label is a typo for **A-Q1** — it arrived
immediately after the real Q44 answer (`Q44: a.`), and Q44 (the libc capability
mapping) has no option "including gnatprove". Relayed as
`requests/b-a-operator-answered-a-q1.md`.
