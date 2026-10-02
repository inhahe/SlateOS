## 601. The option-refusal gate is keyed on "can this loop say no?", and its backlog is pinned rather than exempted

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** §600 said a command must refuse a word it does not understand,
and left open whether to build a checker. This is that checker, plus the two
choices that decide whether it is worth having. First: it does not look for
the *spellings* the known bugs used — it looks for a loop that dispatches on
option names and contains no way to reject one. That is what let it find
fourteen commands the original survey missed, including two that erase disks.
Second: it found 334 pre-existing sites of a *second*, related defect that it
cannot fix today, so those are frozen at their current count in a file that can
only shrink — rather than being switched off, which is what an ordinary
allowlist would have done.

**The trigger, and why not the obvious one.** The nine commands fixed in §600
shared two literal shapes: `.filter(|w| !w.starts_with('-'))` and
`parse().unwrap_or(default)`. A checker built from those is trivial to write
and passes the tenth command, because the tenth will misspell the defect
differently. §299 makes this its whole point: *a gate's trigger is part of its
rule, and a trigger derived from the last bug's spelling is a syntactic sweep
wearing a semantic hat.*

So the primary detector (D3) asks a structural question instead: **does this
`while`/`for` body compare a word against two or more option spellings, and
contain nothing that refuses?** No enumeration of flags, no knowledge of any
particular command. Measured outcome — the literal-shape detectors would have
caught 2 of the 14 new findings; the structural one caught all 14, plus two
(`container ls`, `oci build`) that do not resemble the original nine at all,
because they *print a warning and continue* — §600's explicitly-rejected middle
option, which reads as diligence in code review.

The literal detectors were kept anyway. They are exact, they cost nothing, and
they see a value guessed *inside* an arm the loop does handle — which D3
structurally cannot.

| Trigger | Caught | Missed | Verdict |
|---|---|---|---|
| The nine bugs' literal shapes | 2/14 | 12, incl. `mkfs.fat` | passes the tenth command; rejected as the primary |
| "Loop dispatches on options, cannot refuse" | 14/14 | — | primary (D3) |
| Both | 14/14 + in-arm guesses | — | **chosen** |

**What "refuses" means, and why it was widened rather than allowlisted.** Two
parsers tripped D3 and were right: `parse_tr_args` and `classify_sed_args`
refuse via `return Err(…)`, because their contract is a `Result`. The choice
was between two entries in `ALLOWED` and one more alternative in the refusal
pattern. The pattern won: an `ALLOWED` entry exempts the *function*, forever,
including a genuinely mute loop added inside it next year. Teaching the
detector a real refusal shape costs one line and generalises to every future
`Result`-returning parser. **Prefer widening the definition of correct over
exempting a site that meets it.**

**The 334 pinned sites.** The second half of §600's rule — never invent a value
for a word you could read but could not parse — has a backlog the size of the
shell's history: 334 sites across 119 functions, of the form
`s.parse().unwrap_or(D)`. Three options:

| Option | *What changes* | Cost |
|---|---|---|
| Fix all 334 first, then wire the gate | nothing until it all lands | the gate is unwired for the duration, so new instances land ungated — the exact failure the gate exists to prevent |
| Turn D1 off until later | the checker never mentions them | "later" never arrives, and the backlog grows silently |
| **Pin the counts, burn down separately** | a 335th site fails the build today; the 334 are visible and enumerated | one sidecar file to maintain |

Pinning was chosen, following §296's ledger pattern (its second use). The
properties that make it a ledger rather than an allowlist are load-bearing:
counts are **per function, never per line number** (line numbers drift on every
edit, and an allowlist that rots is a rubber stamp), and an entry that matches
*fewer* sites than it claims is **itself reported** — so a fix that forgets to
lower its count fails the build, and a renamed function cannot leave behind an
entry silently exempting something it was never meant to.

**What this does not settle.** D3 sees `while`/`for` loops. A parser written as
a recursive descent, or as an iterator chain with no loop keyword, is invisible
to it. No such parser exists in `kshell.rs` today; if one is added, the honest
response is another detector, not a wider regex — the checker's docstring says
plainly that the real property needs dataflow it does not have, and that
admission is worth more than a regex that pretends otherwise.

**How to reverse.** Delete the `check_option_refusal` call from
`scripts/boot-test.sh`. The script and ledger are inert without it.

**Where it came from.** §600's own "What this does not settle" section, written
minutes before: *"a rule violated in nine places at once without anyone
noticing is a rule that wants a checker, and §299 says in advance what that
checker must key on if it is to catch the tenth."* It caught fourteen.
Write-ups: `known-issues.md` →
`A-KSHELL-FOURTEEN-MORE-COMMANDS-DISCARD-AN-OPTION-THEY-DO-NOT-RECOGNISE` and
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.
