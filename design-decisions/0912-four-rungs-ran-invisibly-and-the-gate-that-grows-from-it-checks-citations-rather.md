## §912 — Four rungs ran invisibly, and the gate that grows from it checks citations rather than banners

**Date:** 2026-09-05. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** the kshell self-test is a numbered ladder of checks; each one
prints a line to the boot log saying "rung 57: here is what I pin", and those
numbers are how anyone refers to them afterwards. Four of them — 116 to 119 —
had been running and passing for days while printing nothing at all, because
whoever wrote them announced each one with an ordinary code comment instead of
a print statement. The count gate said "115 rungs, no gaps", cleanly, while the
source ran 119. Two places in `known-issues.md` offered these invisible rungs
as the proof that a bug was fixed, so following that proof led to an empty
search. The decision is what new automatic check to add so it cannot recur.

**Where:** `kernel/src/kshell.rs` (the four rungs at what are now lines 22430,
22509, 22558, 22617) and `scripts/check-selftest-rung-numbers.py`.

**What the file's conventions actually were,** counted rather than assumed,
because the count is what decided this:

| syntax | count | what it is |
|---|---|---|
| `serial_println!("  kshell::self_test N: …")` | 115 | the banner — the only one that reaches the log |
| `// Rung N -- …` inside the block | 13 | the house in-block comment, rungs 103–115 |
| `// Rung N <verb>` / `// Rung N's …` | 7 | **cross-references** to another rung |
| `// --- N: …` above the block | 4 | rungs 116–119 only, and *instead of* a banner |

Note where the in-block comment convention starts: **rung 103**, the very rung
whose missing banner prompted this gate's creation in the first place. The
comment was added then as a belt-and-braces marker; four rungs later someone
kept the marker and dropped the belt.

**Option A — bless `// --- N:` as a second convention and gate "a marker
implies a banner". (rejected)**
*What changes:* the four existing markers become load-bearing; nothing else.

- *For:* it is the smallest possible rule and it fires on exactly the bug found.
- *Against:* it protects only the four rungs that already carry a marker,
  because nothing makes anyone write a marker for the *next* rung. It leaves
  two naming conventions in the file permanently, when one of them exists only
  because someone invented it by accident. And it is a rule about a comment
  syntax, which is the weakest kind of thing to hang a build gate on.

This was my first pass, and the census above is what killed it. Writing it down
because "gate the marker" is the obvious idea and the next reader will have it
too.

**Option B — add the four banners, normalise the marker away, and gate
dangling rung *citations*. (chosen)**
*What changes:* the log gains four lines; `// --- N:` stops existing; and the
build fails on any `// Rung N` or kshell-section prose citing a rung that no
banner announces.

- *For:* it catches the bug **from the direction it actually hurt**. Nobody was
  harmed by rung 119 not printing in the abstract; they were harmed by
  `known-issues.md` offering rung 119 as evidence and the log containing no
  such thing. It needs no new convention — it reads numbers humans already
  write down. And it generalises where Option A did not: it fires on a rung
  deleted while still cited, a rung renumbered by a merge, and a typo'd
  citation.
- *Against:* it is a wider rule with a corpus in three files, so it has more
  surface to produce a false positive on. Answered by measurement below, and by
  landing at zero.

**Why this is not the check the gate's own docstring rules out.** That section
says "does every block have a banner" is undetectable, and it is right: such a
rule must *guess* which brace-blocks are rungs, and cannot separate one from the
two legitimate scoping blocks in `self_test`. The citation rule guesses nothing.
It reads a number a human wrote down deliberately and asks whether it resolves.
The docstring now draws that line explicitly, because otherwise the next reader
takes the section as having already ruled this out and moves on.

**The scoping rule was measured, not guessed.** The shared documents are worked
by three lanes, so a bare `rung 12` in `known-issues.md` might be lane B's
ladder. Two candidate qualifiers, run against the real corpus before the rule
was written:

| rule | citations qualified | dangling found |
|---|---|---|
| same line contains `kshell::self_test` | 45 / 160 (28%) | **1** of 4 |
| enclosing `###` section mentions kshell | 150 / 160 (94%) | **4** of 4 |

Line-scope is the tighter rule and it is the wrong one: it would have missed
three of the four instances of the exact bug it was written for, because the
citations that dangle are ordinary prose ("*Rung 116 still passes unchanged*")
and prose does not repeat the fully-qualified name of the function it discusses.
Section-scope's extra breadth cost nothing measurable — 146 of its 150 resolve,
and `design-decisions.md` qualified 18 with zero dangling.

A second measurement changed the implementation after the fixtures were
written: the section test is **case-insensitive**, because the issue headings
are upper-case (`### A-KSHELL-…`) while the prose is lower-case, so a
case-sensitive test silently drops any kshell section whose body never happens
to spell the name in lower case. Measured rather than assumed: 169 of 180
citations qualify against 168, both at zero dangling. The fixture that exposed
this was written to describe the *intended* behaviour and failed against the
first implementation — which is the fixtures earning their place rather than
decorating the file.

**Why it lands clean.** Adding the four banners takes the finding list from
nine to zero, so the gate starts at zero rather than with a backlog — the DD 635
condition, satisfied by construction rather than by hoping. Confirmed both ways:
simulating the pre-fix state (only 1..115 announced) reports nine findings
covering exactly rungs 116–119; the fixed tree reports none.

**Independent corroboration worth recording.** The prose corpus and the
`// --- N:` markers were derived from completely different evidence — one is
comments in Rust, the other is English in two markdown files — and they name
the same four rungs. That agreement is why this was treated as a real defect
rather than a documentation nit.

**Reversal:** delete `check_citations`, `SRC_CITE`, `MD_CITE`, `MD_DOCS` and
`kshell_sections` from the checker and drop the citation cases from its
self-test. The four banners should stay regardless — they are the fix, and the
gate is only what stops it recurring.
