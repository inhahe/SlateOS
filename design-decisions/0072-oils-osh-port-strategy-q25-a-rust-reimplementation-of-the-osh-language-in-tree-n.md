## 72. Oils (OSH) port strategy (Q25-A) — **Rust reimplementation of the OSH language in-tree**, not a C++ `oils-for-unix` cross-compile

> ## ⛔ SUPERSEDED IN PART — 2026-08-14, by **§305**. Read §305 before doing any osh parity work.
>
> **Do not read this entry's rationale as current, and do not treat its "How to
> reverse" clause as live — it already fired, on 2026-07-22, and went unchecked
> for 25 days.** The operator settled the resulting question (Q41) on
> 2026-08-14 with the **hybrid**: osh remains the shell, the cross-compiled GNU
> bash ships beside it as the escape hatch and future on-device differential
> oracle, and **osh's bash-fidelity scope is frozen** behind a written stopping
> criterion. §305 carries that criterion and is binding on all `TD-OILS-*` work
> and on `userspace/oils/tests/corpus/`.
>
> What survives here: the *choice* of a Rust reimplementation, which stands.
> What does not: the "no C/C++ → `x86_64-slateos` cross-toolchain" premise
> (false since 2026-07-22 — bash now boots and runs on SlateOS) and the
> open-ended byte-for-byte parity goal that grew out of it.

**Decided by:** Claude (operator-approved scope) — the operator committed to "port
Oils (OSH), a bash-*superset* shell (NOT bash itself)" as the first large
initiative (§69, Q25→A). *How* to port it (faithful C++ cross-compile vs. Rust
reimplementation) is the sub-decision recorded here. Flagged to the operator as
open-question **Q26** because it is large and costly-to-reverse; proceeding on the
prerequisite-forced default while the operator is away.

**Decision.** Build `userspace/oils` as a **real Rust reimplementation** of the
OSH language (a bash/POSIX superset shell that actually forks/execs external
programs on SlateOS), matching the pattern already used for **coreutils** (85
real Rust tools) and the existing 1194-line `userspace/coreutils/src/bin/sh.rs`
minimal POSIX shell. **Not** a cross-compile of upstream Oils' C++
(`oils-for-unix`) tarball.

**Why (the decisive prerequisite fact).** There is **no C/C++ → `x86_64-slateos`
cross-toolchain in this repo** — verified: no crate/build.rs/script references a
C++ cross-compile to slateos, and every "port" to date is either a Rust
reimplementation (coreutils) or a Rust personality binary (the in-tree
`userspace/nushell` is a *stub* that simulates output; the real `nu.exe` was only
verified building against the **Windows host** target, never slateos).
Cross-compiling `oils-for-unix` would first require standing up an entire C++
cross-toolchain **and** a slateos libc/CRT sufficient for Oils' POSIX use — a
separate, massive, unlisted prerequisite initiative. A Rust reimplementation is
the only path that yields a **running** shell on the OS now, and it is the honest
match to the operator's intent (Q24 was spent specifically de-risking the
fork/exec teardown deadlock so this shell can fork/exec for real — a stub would
not exercise that at all).

**Alternatives considered.**
- **C++ `oils-for-unix` cross-compile (faithful port).** Pro: bit-for-bit OSH
  semantics, no reimplementation risk. Con: blocked on a non-existent C++/slateos
  toolchain + libc — not buildable today; would deliver nothing runnable for a
  long time. Rejected as prerequisite-blocked.
- **Extend the existing coreutils `sh.rs` in place.** Pro: least new code. Con:
  that binary is deliberately a *minimal POSIX sh*; growing it to a bash-superset
  OSH would bloat the coreutils crate and blur the "one crate = one deliverable"
  layout. A dedicated `userspace/oils` crate keeps the OSH shell reviewable and
  independently buildable/testable, and lets `sh.rs` stay a small POSIX baseline.
- **Rust stub personality (like the checked-in nushell).** Rejected — a shell
  that only prints simulated output is not a "port," does not run programs, and
  wastes the Q24 fork/exec de-risking.

**How to reverse.** If a C++/slateos toolchain is later built (e.g. as part of the
Mesa/Chromium/WINE initiatives, which need C/C++ anyway), the faithful
`oils-for-unix` cross-compile can replace `userspace/oils` — the crate is an
isolated userspace binary with no other code depending on its internals, so the
swap is local. Until then the Rust OSH shell is the deliverable.

**Where it lives.** `userspace/oils/` (new crate; auto-registered via the
`userspace/*` workspace glob). Roadmap: §2.7 "Port Oils (OSH)" (roadmap.md:1494).
Tracking: open-questions.md Q26.

> **⚠ Audit note — 2026-08-12: the prerequisite fact above is STALE.** The
> "decisive prerequisite fact" — no C/C++ → `x86_64-slateos` cross-toolchain —
> was true on 2026-07-18 when this was written, and **stopped being true on
> 2026-07-21/22**, when the `x86_64-slateos` C cross-target and `zig cc` landed
> (fastpy initiative F) together with `toolchain/sysroot/lib/libc.a`. The "How to
> reverse" clause above therefore fired within four days and was never
> re-examined; ~1,100 of the 1,181 `userspace/oils` commits postdate it. Note
> that bash is **C**, not the C++ this decision was actually arguing against, so
> it needs strictly less. This does *not* retroactively invalidate the decision
> (it was correctly reasoned on the facts of the day), and one real blocker
> remains — `posix/src/signal.rs:572` has no kernel suspend mechanism, so bash's
> job control cannot work yet — but the rationale must no longer be read as
> current. Raised by the operator; now **open-questions.md Q41**.
>
> **Follow-up, same day — measured, not argued.** The operator authorised a
> spike (`scripts/bash-spike/`). **GNU bash 5.2 now boots and runs on SlateOS**:
> cross-compiled with `zig cc`, linked statically against
> `toolchain/sysroot/lib/libc.a` with **zero undefined symbols and no shims**,
> and exercised by `self_test_bash_on_slateos_libc` in
> `kernel/src/proc/spawn.rs` on a script using arrays, `${v,,}`, `$(( ** ))`
> and brace expansion — constructs dash lacks, so no `/bin/sh` fallback can
> explain the result. Closing the gap took exactly three small additions to
> `posix/src`: `killpg`, `eaccess`/`euidaccess`, `__fpurge`. The prerequisite
> objection is therefore not merely stale but comprehensively so:
> **feasibility is settled and is no longer an input to Q41**, which is now
> purely a scope/ownership question — keep osh, switch to bash, or keep both
> and use bash as a differential oracle running on SlateOS itself.
>
> **Answered 2026-08-14 — the third option, and osh's scope is now frozen.
> See §305.**
