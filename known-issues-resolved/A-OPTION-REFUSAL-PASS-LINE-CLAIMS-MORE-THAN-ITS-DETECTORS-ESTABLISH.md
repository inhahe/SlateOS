### A-OPTION-REFUSAL-PASS-LINE-CLAIMS-MORE-THAN-ITS-DETECTORS-ESTABLISH — 2026-09-12 — FIXED by lane B (lane A)

**FIXED by lane B, 2026-09-12, and better than what I proposed.** The pass line now reads:

```
[option-refusal] kshell.rs: 102155 production statement(s) and 1483 loop(s) inspected;
no site matches D1 (parse fallback), D2 (dash-filtered word), D3 (option …)
```

I suggested naming the detectors. They also added **the counts** — 102,155 statements and
1,483 loops inspected — which is the half that matters more: naming the detectors stops the
line overclaiming, but naming the population is what makes a regex that has quietly stopped
matching visible. "No site matches D1/D2/D3" out of 102,155 and out of 0 are different
statements; the old line and my proposed line would both have printed the same word.

**In short:** a check that runs on every boot prints "no word is silently dropped"
when it passes. It does look for three specific ways a word can be dropped, and it
finds none of them — but there is a fourth way, it happens in the very file the
check reads, and the printed line does not leave room for it. Someone reading the
boot log comes away believing something stronger than what was tested.

**Where:** `scripts/check-option-refusal.py`, the pass line (~line 615):

    [option-refusal] kshell.rs: no word is silently dropped and no new value is
    guessed (1 allowed, 78 guessed-value site(s) carried as known debt across 78
    function(s))

**Be fair to the file first, because the obvious criticism is the wrong one.** Its
module docstring is scrupulous — it enumerates its three detectors and says in as
many words that it reports what it checks *"rather than claiming completeness"*:

| | shape detected |
|---|---|
| `D1 guessed-value` | `parse().unwrap_or(D)`, `unwrap_or_default()` — a default standing in for an unreadable word |
| `D2 dropped-word` | `.filter(\|w\| !w.starts_with('-'))` over an operand list |
| `D3 mute-parser` | an option-dispatch loop with no way to say no |

So the *file* does not overclaim. The **pass line** does, and the pass line is the
only part of it that anybody reads: it goes into every boot log, while the
docstring is seen only by someone who opens the source. The line even discloses
the D1 backlog in its own parenthetical — "78 guessed-value site(s) carried as
known debt" — which makes the unqualified first clause read as deliberate by
contrast. One half of the sentence is careful and the other is not.

**The live counterexample, in the same file the gate reads.**
`kshell::split_words` (`kernel/src/kshell.rs`) ends:

    String::from_utf8(word).ok()

`filter_map` therefore **drops the word** when it is not valid UTF-8 — silently,
changing the command's arity so the command runs and does something *else* rather
than doing nothing. It matches none of D1/D2/D3: it is not a parse fallback, not a
dash filter, and not an option loop.

**It was unreachable until 2026-09-12 and is not any more**, which is why this is
worth filing rather than shrugging at. The comment above it says the failure
cannot happen because "only ASCII bytes are removed, and word boundaries are ASCII
blanks" — true of every scanner that had ever run. Adding `$'…'` decoding
(`A-KSHELL-DOLLAR-SINGLE-QUOTE-LEAVES-A-STRAY-DOLLAR`) made `strip_quotes` able to
*produce* a non-UTF-8 byte, which is the entire purpose of the construct. So
`touch $'re\xffport.txt'` now runs `touch` with no argument at all.

The sibling `remove_quotes` degrades more gently — `unwrap_or_else(|_| s.to_string())`
returns the line undecoded rather than dropping anything — and its author said why:
"the one fallback that cannot turn a command into a *different* command if that
reasoning is ever wrong." That instinct was right and it is the difference between
the two sites.

**The fix is not to widen the detectors.** A fourth regex is another proxy, and the
lesson from `check-shellquote-vs-bash` this same night is that a table of shapes
stops standing for the property it was chosen to represent. Two things that are
worth doing:

1. **Make the pass line say what was checked.** "no D1/D2/D3 site outside the
   ledger" is a status a reader can act on; "no word is silently dropped" is a
   claim the detectors do not support. A gate that overstates on success trains
   people to believe the next one.
2. **Remove the counterexample rather than describe it** — the refusal drafted as
   `args_are_representable` turns both silent degradations into one visible
   diagnostic at the two dispatch sites, following `path_arg_as_str`'s precedent
   ("the refusal is the point... visible, not data loss, and disappears on its own
   as each module is converted").

**Ownership:** `scripts/` belongs to no lane per `which-lane.py`, which is A-Q11.
The counterexample and its fix are lane A's and are being done here; the pass-line
wording is a one-line change in an unowned file and is left for whoever answers
A-Q11, recorded so it is not lost in the meantime.
