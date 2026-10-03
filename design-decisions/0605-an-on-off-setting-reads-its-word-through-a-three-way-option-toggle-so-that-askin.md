## 605. An on/off setting reads its word through a three-way `Option<Toggle>`, so that "asking" and "mistyping" cannot collapse into each other

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Dozens of shell settings are switches — `a11y captions on`,
`a11y motion off`. Typing the word with the switch sets it; leaving the word
off asks what it currently is. Until now a mistyped word (`a11y captions
onn`) was treated as *no word at all*, so the shell answered a request to
change the setting by printing the setting's present value and exiting
successfully. It now says `` a11y: captions: `onn' is not on or off `` and
exits 1. The reading is done once, in a helper called `toggle_arg`, rather
than by a `matches!` in each of ~30 commands.

**Why this shape is worse than the guessed number §600 is about.** §600's
usual defect substitutes a made-up value for a word it could not read, which
at least prints something visibly wrong — a brightness of 0 when you asked
for 5o. This one prints something *right*. `Captions: false` is a true
statement about the system, delivered in reply to a request to change it,
with exit status 0. Nothing downstream — not the user, not a script checking
`$?` — has any signal that the command declined to act. It was found by
reading the arms, not by the ledger's regex, which is keyed on
`unwrap_or`-shaped substitution and structurally cannot see a catch-all that
prints a getter.

**The decision that carries the weight: absence, and only absence, means
query.** The helper returns `Option<Toggle>` where `Toggle` is `Query` or
`Set(bool)`, giving three outcomes from one call:

| Input | Return | Meaning |
|---|---|---|
| no word at index | `Some(Toggle::Query)` | the user is asking |
| `on`/`true`/`yes`/`1`/`enable`/`enabled` | `Some(Toggle::Set(true))` | understood |
| `off`/`false`/`no`/`0`/`disable`/`disabled` | `Some(Toggle::Set(false))` | understood |
| anything else | `None`, after printing and setting status 1 | refused |

The alternative — returning `Option<bool>` and letting the caller treat
`None` as "query" — is what the code already did informally, and is exactly
the bug. Two distinguishable situations must not share a return value; the
extra variant exists to make the collapse unrepresentable rather than merely
discouraged.

**Why a helper rather than fixing the arms in place.** The `matches!(word,
"on" | "true" | "yes" | "1")` idiom had been copied into roughly fifty
places, and the copies had drifted: some accepted `enable`, some did not;
some treated an unrecognised word as `false`, which is the dangerous
direction — `datausage limit block <name> 1` and `kernelbuild auto <id> 1`
both *disable* a protection when handed a word they cannot read. One helper
means one vocabulary, and means the next setting added gets the refusal for
free instead of getting it if its author remembers.

**What was given up.** The helper fixes the *shape* of the diagnostic (`is
not on or off`), so a command wanting to advertise a third state — a
tri-state `auto`, say — cannot use it and must read the word itself. That is
correct: such a command's vocabulary is not this one, and borrowing the
helper would make it lie about what it accepts. Judged a feature, not a
limitation.

**Where it came from.** The `cmd_a11y` batch of the
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`
burn-down; six toggles in that one command had the defect. Rung 78 of
`kshell::self_test` holds it down, and pairs every refusal with a control
proving the query form still answers.
