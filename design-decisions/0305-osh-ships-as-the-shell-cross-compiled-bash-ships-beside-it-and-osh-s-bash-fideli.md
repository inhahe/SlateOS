## §305 — osh ships as the shell, cross-compiled bash ships beside it, and osh's bash-fidelity scope is **frozen**

**Date:** 2026-08-14
**Decided by:** Operator (Claude recommended this option; the operator asked the
question that exposed the problem in the first place — see the history below)

### The question

Should SlateOS's POSIX shell be `userspace/oils` (osh — a 141,899-line Rust
reimplementation, 1,210 commits, 662/662 corpus cases byte-exact against bash
5.2.37), or the genuine GNU bash 5.2 that we can now cross-compile and which
**already boots and runs on this OS**? Tracked as open-questions **Q41**, now
closed by this entry.

### Decision — the hybrid, with a stopping criterion

Three parts, and the third is the one that actually changes day-to-day work:

1. **osh remains the shell.** It works today, it is ours to debug and extend,
   it needs no dynamic linker, and 141k lines of it are already correct. No
   rewrite, no deletion, no migration project.
2. **The cross-compiled bash stays and is a first-class artifact.** It is the
   escape hatch (a script that genuinely needs exact bash can run exact bash)
   and, longer term, the differential oracle *running on SlateOS itself* —
   which is the path to dropping `scripts/osh-bash-diff.py`'s dependency on a
   host Linux/MSYS reference bash. Keep `scripts/bash-spike/` reproducible and
   keep `self_test_bash_on_slateos_libc` green.
3. **osh's fidelity scope is frozen.** Byte-for-byte parity with bash is no
   longer an open-ended goal. The corpus stops growing for its own sake. The
   stopping criterion is written out below, and it is binding.

### The stopping criterion (read this before opening a `TD-OILS-*` entry)

**Fix an osh divergence when at least one of these is true:**

- **Something on SlateOS actually hits it.** A real script, service, init file,
  build step, package recipe, test harness or interactive session that we ship
  or run. "A user might type this" does not qualify; "our own `create-ext4-rootfs.sh`
  types this" does.
- **It is a crash, hang, wrong-exit-status-that-propagates, data-loss or
  security bug.** These are bugs on their own terms, independent of bash.
- **It is a regression** against a corpus case that is already green. The 662
  existing cases stay green; that is a floor, not a ceiling.

**Do not fix — and do not write a corpus case for:**

- **Diagnostic wording, spelling, and the exact substring a message echoes.**
  Two of the last three sessions' fixes were of exactly this kind: which of
  bash's two messages an unterminated `${` takes when its *name* scan runs off
  the text, and the precise `%s` slice in ``bad substitution: no closing "`" in
  …`` when an unmated backquote sits inside a single-quoted run in an array
  subscript. Both are now correct, both were verified byte-exact, and neither
  will ever matter to anything running on this OS.
- **Artifacts of bash being a 40-year-old C program.** The canonical example,
  already in the corpus: `OPTIND=4294967297` wraps to the first argument
  because bash stores it in a C `int`. That is a fact about `int`, not about
  shells.
- **Constructs only reachable by adversarial or nonsense input** whose only
  observable difference is which error text appears.

**When a divergence is real but out of scope**, the answer is now bash: note it
in the `TD-OILS-*` entry as `SCOPE: out of frozen scope (§305)` and move on.
Do not open a fidelity investigation.

### Why this and not the alternatives

- **B — keep osh, close Q41 permanently.** Rejected as stated, because it
  answers the feasibility question that was already settled and leaves the
  *actual* problem — that byte-for-byte fidelity has no stopping criterion —
  exactly where it was. This entry is B plus the missing stopping criterion,
  which is why it supersedes rather than contradicts it.
- **C — switch to cross-compiling bash, retire osh.** Rejected: it discards
  141k lines of working, byte-exact, dependency-free Rust in exchange for
  maintaining a fork of 40-year-old C, and osh's independence from a C
  toolchain has value on an OS whose toolchain story is still moving. Note
  that this rejection is *not* on feasibility grounds — bash demonstrably runs.
- **The hybrid (chosen).** Gets C's benefit (fidelity is available exactly,
  for free, when something actually needs it) without C's cost (no rewrite, no
  fork to maintain), and converts the unbounded chase into a bounded job.

### The history this entry exists to prevent repeating

This is the part a future session must read. **§72 chose the Rust
reimplementation on one decisive fact — "there is no C/C++ → `x86_64-slateos`
cross-toolchain in this repo" — and wrote its own reversal condition. The
condition fired four days later and nobody checked for twenty-five days.**

| Date | Event |
|---|---|
| 2026-07-18 | `userspace/oils` begins; §72 rejects the cross-compile as prerequisite-blocked |
| 2026-07-21 | `x86_64-slateos` C cross-target lands (fastpy, initiative F) |
| 2026-07-22 | `zig cc` wired in; `toolchain/sysroot/lib/libc.a` exists — **§72's premise is now false** |
| 2026-08-12 | The operator asks the question. Spike measures it: **bash 5.2 boots and runs on SlateOS**, 5,349,720-byte static ELF against our own `libc.a`, zero undefined symbols, no shims, three small `posix/src` additions (`killpg`, `eaccess`/`euidaccess`, `__fpurge`) |
| 2026-08-14 | This entry |

Roughly 1,100 of oils' 1,210 commits postdate the moment §72's stated blocker
ceased to exist. The original call was correctly reasoned **on the facts of its
day**; the failure was procedural, not analytical — a decision carrying an
explicit expiry condition was never re-audited, and a large initiative kept
compounding on a premise that had quietly become false.

### The general rule this establishes

**A decision whose rationale rests on a stated prerequisite being absent must
name the condition that reverses it, and that condition must be *checked*, not
merely recorded.** Concretely, for this repo:

- When a `design-decisions.md` entry contains a "How to reverse" / "revisit if
  …" clause, the reversing condition belongs in `todo.txt` as a live item, not
  only in the entry's prose. A clause nobody is scheduled to evaluate is a
  comment, not a control.
- **When you build a capability, grep the design decisions for entries that
  were rejected for lack of it.** The `zig cc` work of 2026-07-21/22 was the
  moment to re-read §72, and the person best placed to notice was whoever
  landed the toolchain. `grep -n "no C/C++\|cross-toolchain\|prerequisite"
  design-decisions.md` would have found it.
- Prefer rationales that rest on *tradeoffs*, which age slowly, over rationales
  that rest on a *missing prerequisite*, which can evaporate overnight.

### Where it bites

`design-decisions.md` §72 (superseded in part — its prerequisite claim and its
"How to reverse" clause are both spent; see the pointer added there),
`open-questions.md` Q41 (closed by this entry), `roadmap.md` §2.7 and the Lane B
backlog summary, `roadmap-detailed.md` §2.7, `todo.txt`, `known-issues.md` (the
whole `TD-OILS-*` family is now scope-gated by the criterion above),
`userspace/oils/` (all of it), `scripts/osh-bash-diff.py` and
`userspace/oils/tests/corpus/` (the corpus no longer grows for its own sake),
`scripts/bash-spike/`, `scripts/create-ext4-rootfs.sh` (stages `/bin/bash`) and
`kernel/src/proc/spawn.rs::self_test_bash_on_slateos_libc`.
