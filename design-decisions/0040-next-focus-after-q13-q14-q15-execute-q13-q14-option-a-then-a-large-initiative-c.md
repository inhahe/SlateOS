## 40. Next focus after Q13/Q14 (Q15) — execute Q13 + Q14 (option A), then a large initiative; C (GPU accel) or D (Docker port) in operator-indifferent order

**Date:** 2026-06-30

**Decided by:** Operator (this was `open-questions.md` Q15; the operator chose
option **A**, then C-or-D). The operator's words: *"Q15: A, then do C or D. I'm
not sure which is better to do first between C and D. I guess it doesn't matter
because it all has to be done anyway and nobody can use the OS yet."* Claude
recommended A as the immediate next step (and had recommended B as the next large
initiative; the operator instead directed C or D).

**The decision.** Immediate next step: **(A)** — execute the now-resolved Q13
(page-cache-primary, §38) and Q14 (connect the cgroup subsystems, §39). After
that, proceed to a large initiative: either **(C) GPU acceleration** or **(D)
Docker / container-runtime port**, in whichever order — the operator is
explicitly indifferent ("it all has to be done anyway"). **This is the explicit
operator go-ahead the standing rule required for the Docker/container-runtime
port (a giant external port).** Option (B) TCP/IP→userspace, which Claude had
recommended as the next *large* initiative, was not selected as the immediate
follow-on; it remains valid future work but C and D come first.

**Alternatives considered (from Q15).** (B) TCP/IP stack → userspace (Claude's
recommended next large initiative — internal, stack already feature-complete, on
the microkernel roadmap); the operator chose C/D instead. C and D are the two
selected; the operator left their relative order open.

**Where it bites.** (A) §38 (Q13) + §39 (Q14). (C) `gui/gpu/`,
`gui/compositor/`. (D) `kernel/src/container.rs`, `pkg/`, plus a large external
dependency — and Q14's cgroup-enforcement gap was a stated prerequisite, now
being closed by §39.
