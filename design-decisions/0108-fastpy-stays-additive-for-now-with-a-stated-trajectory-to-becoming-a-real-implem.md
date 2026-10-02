## §108 — fastpy stays additive for now, with a stated trajectory to becoming a real implementation

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended "A for now"; the operator accepted
it and set the direction beyond it). This was Q35 in `open-questions.md`.

**Context.** The fastpy `/bin`-promotion (§87 follow-on) installs `cat`, `wc`,
`head`, `tail` at `/bin/<cmd>` in the **test** rootfs. These are minimal
proof-of-pipeline implementations — `cat` is ~5 lines of Python — while SlateOS
already ships 85 mature Rust coreutils (roadmap §2.7). At some point one
shipping `/bin` has to decide which `cat` is *the* `cat`.

**Decision — three parts.**

1. **For now: additive only.** fastpy commands keep being promoted into the
   *test* rootfs `/bin` to exercise the pipeline. No Rust coreutil is touched,
   shadowed or retired. A silent swap is a user-visible policy change and is not
   Claude's to make.
2. **The trajectory is toward fastpy being a real implementation**, per command,
   gated on two bars: a real parity test suite for that command, and a
   performance bar — fastpy's version must be **faster, equal, or not
   significantly slower** than the Rust one. Whether the user gets the fastpy
   version is then an **opt-in switch**, not a silent substitution.
3. **fastpy's scope is not coreutils.** The operator's original intent was any
   non-CPU-intensive OS function — driving a file explorer window, a settings
   dialog — and, because fastpy compiles to native code, CPU-intensive ones too.
   The coreutils promotion is a *pipeline test*, not the point. Roadmap items
   that reach for fastpy should be picked with that in mind rather than treating
   `/bin` as the target surface.

**Still open, deliberately.** Which way the *shipping default* points — whether
a stock install prefers fastpy implementations where they exist, or prefers the
canonical ones and makes fastpy the opt-in — is not settled. It is carried
forward in `open-questions.md` as its own narrower question, to be answered when
there is at least one fastpy utility that has actually cleared both bars, since
answering it earlier would be answering it without evidence.

**Alternatives considered.**
- *B — swap per command as each reaches parity, retiring the Rust one.* This is
  the trajectory, but not the current state: adopting it now would throw away
  the maturity and measured performance of the Rust tools before any fastpy
  utility has a parity suite to justify it.
- *C — coexist under distinct names* (`/bin/pycat`). Rejected: it clutters
  `/bin` and leaves no answer to "which is canonical", which is the actual
  question.

**Where it lives.** `scripts/create-ext4-rootfs.sh` (the `PROMOTED` map —
currently the *test* rootfs `/bin`), `kernel/src/proc/spawn.rs`
(`resolve_command` / `COMMAND_PATH`), the `services/fastpy-*` sources, and
whatever eventually assembles the production rootfs `/bin`.

**How to reverse.** Part 1 is the status quo and needs no unwinding. Parts 2–3
are direction, not code; the first per-command swap is where the decision
becomes concrete, and it is gated on the two bars above.
