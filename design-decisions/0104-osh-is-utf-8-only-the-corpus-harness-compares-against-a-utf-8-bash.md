## §104 — osh is UTF-8-only; the corpus harness compares against a UTF-8 bash

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended this option; operator accepted and
asked that the rejected scope stay documented in `known-issues.md`). This was
Q38 "Should osh be locale-aware, or UTF-8-only?" in `open-questions.md`.

**Context.** bash decides *per locale* whether a string is a sequence of bytes
or of characters: every multibyte site sits behind `HANDLE_MULTIBYTE` and calls
`mbrlen`/`mbstate`, so `${#s}` on `a…b` is 5 under `LC_ALL=C` and 3 under
`LC_ALL=C.UTF-8`. osh has no such switch — it always does UTF-8 character
semantics. `scripts/osh-bash-diff.py` pinned `LC_ALL=C` for both shells, so on
any multibyte input osh was being compared against a bash doing byte semantics,
a baseline osh was never built for. No corpus case had exercised it until one
happened to put a `…` inside a `printf '%-46s'` label.

**Decision.** osh's string layer is UTF-8, full stop, and the harness is moved
to a UTF-8 locale so that the reference bash agrees. `LC_ALL` is not modelled as
an observable switch over character semantics.

**Rationale.** The OS this shell ships in is UTF-8 throughout; there is no
non-UTF-8 locale on the SlateOS target for the switch to serve. Making osh
locale-aware would thread a locale notion through `bytes.rs` — today free
functions with no state — and through every character-counting site (`${#v}`,
`${v:off:len}`, `${v^^}`/`${v,,}`, `%q`, `\u`/`\U`, `select`'s display width,
plausibly globbing and `[[ =~ ]]`), for an axis nothing in the OS exercises.

**What this gives up, kept on the record.** A real bash under `LC_ALL=C` is now
not reproducible by osh at all, so that axis of bash's behaviour goes untested.
Scripts that set `LC_ALL=C` for speed or determinism — a common idiom — get
different answers from osh than from bash. The sharpest example is `%q` on a
byte that is no character: bash's `ansic_shouldquote` defers to
`ansic_wshouldquote`, which quotes when `mbstowcs` fails, so under UTF-8
`a\xffb` prints `$'a\377b'` and under C it prints the raw bytes. There is no
edit to osh that is right under both.

**Alternatives considered.**
- *B — make osh locale-aware, as bash is.* Actually matches bash, which is the
  project's stated goal, and would let the corpus test both axes. Rejected on
  cost-to-value: it is the whole string layer, and the C locale is the *easy*
  half — a non-UTF-8 multibyte locale would be far worse, so the honest scope is
  "C vs UTF-8", not "all locales". The scope stays written down in
  `known-issues.md` under
  `TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE` so that a
  future change of mind starts from a survey and not from scratch.

**Where it lives.** `scripts/osh-bash-diff.py` (the environment it pins);
`userspace/oils/src/bytes.rs` (`char_count`, `char_slice`, `char_at`) and its
callers.

**How to reverse.** Re-pin the harness to `LC_ALL=C` and work the survey in the
`known-issues.md` entry. Nothing in osh encodes the choice; the decision is
which reference behaviour the corpus is measured against.
