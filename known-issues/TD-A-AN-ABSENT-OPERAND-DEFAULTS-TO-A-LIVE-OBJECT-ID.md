## TD-A-AN-ABSENT-OPERAND-DEFAULTS-TO-A-LIVE-OBJECT-ID (lane A, 2026-09-10) — **open**, now counted: **34 sites across 11 functions**, 3 allowed (was stated as 38/13; see the correction note below)

**The counter exists as of 2026-09-10.** `scripts/check-absent-operand-default.py`
defines the class in code and `scripts/absent-operand-ledger.txt` holds the count,
one line per enclosing function, failing in **both** directions — a new site in a
function that does not allow one, and an entry claiming more than exist. Wired
into the boot test. Verified by moving a count each way: lowering one reports
`cmd_audioeq: 1 more than the ledger allows`, raising another reports
`cmd_colortemp: 2 fewer than it claims`.

So the number below is reproducible, which none of the earlier ones were. It is
also a **fourth** value — 38, where hand-counting gave 14, 31 and 37 — because it
is the first one attached to a definition rather than to a grep: it counts
`parts.get(N).unwrap_or(&"<numeric literal>")` wherever it appears, without
requiring a `.parse()` on the same line, which is what the earlier patterns all
keyed on.

**In short:** several shell commands, when given no argument at all, act on object
number 0 or 1 instead of asking for one. `filevault unlock` with nothing after it
tries to unlock vault 0 with an empty password. It is a cousin of the §600
guessed-value backlog and is *not* counted by it, so it needs its own record or it
will be filed as already-handled.

### The shape

    let id: u32 = match parts.get(1).unwrap_or(&"0").parse() { … };

The `match` refuses an *unreadable* id correctly — that is why
`check-option-refusal.py` does not count these. What it cannot see is that the
default `"0"` is supplied for an **absent** operand and then parses perfectly, so
a missing id becomes a real one.

**The size of this population is not reliably known, and that is the finding.**
Three attempts at counting it gave three answers, each from a slightly different
pattern:

| pattern searched | sites |
|---|---|
| `parts.get(1).unwrap_or(&"0").parse()` | 14 |
| ...plus the `&"1"` variant | 31 |
| `parts.get(N).unwrap_or(&"<literal>").parse()`, any index, any literal | **37** |

None of those is wrong; they are answers to three different questions, and the
first two were published here as though they were answers to this one. A fourth
pattern would give a fourth number.

**None are fixed.** An earlier draft said two had been "fixed in passing". They
had not: the edit failed on an ambiguous match — the block appears 14 times, so
the replacement refused rather than guessing which was meant — and the draft was
written from the intention instead of from the file.

Two examples of the harm, both still live:

* `filevault unlock` — bare, it attempts vault 0 with the empty password that
  `parts.get(2)` also defaults to;
* `screensaver preview` — bare, it previews saver 1.

**And the obvious way to triage them does not work either.** Whether requiring the
operand is correct depends on whether the command documents it as optional, so the
usage string should decide it. It cannot: of the 37 sites, **34 have no `Usage:`
line within 60 lines**, and of the 3 that do, all three matched a *neighbouring
match arm's* usage string rather than their own. A sweep built on that would
require operands that a command had deliberately made optional.

### Why it is worth its own entry

The §600 ledger is a *count of a specific spelling*, and the checker's error
message is what makes the count trustworthy. This defect passes that checker by
construction: it refuses the unreadable word, which is what the checker looks for.
So it will not appear in the burn-down however far that goes, and anyone reading
"78 of 800 remain" would reasonably assume the class was fully enumerated.

### The fix

Require the operand. `parts.get(1)` returning `None` is *"you did not say"*, and
the usage line is the answer to it — as distinct from `"you said something I could
not read"`, which the existing `match` already handles by name. The two fixed
sites show the shape.

**The prerequisite is a checker, not a sweep, and that is the lesson from §600
next door.** What makes "78 of 800 remain" trustworthy is not diligence — it is
`check-option-refusal.py` plus a ledger that fails in both directions, so a new
site cannot appear unnoticed and a fixed one cannot stay counted. This class has
no such definition, which is why three greps gave three numbers.

The definable version is narrower than the harm and is exactly what a checker can
see: **`parts.get(N).unwrap_or(&"<literal>")` where the literal parses as a
number** — a missing operand silently becoming a numeric value. Whether that
number names a live object is judgment and belongs in the per-site review; the
pattern is mechanical and countable.

So the order is: write the checker, pin the ledger, then sweep against it. Fixing
sites first would repeat what happened above — a population that is 14 or 31 or 37
depending on who asks, and no marker saying how far a partial pass got.

### Correction to the 38/13 count (2026-09-10, same day)

38 was wrong in two ways and both are instructive, which is why this is recorded
rather than quietly edited.

One of the 38 was a **comment**. `cmd_colortemp` carries a note left behind when
that site was fixed, quoting the line it replaced. The checker scored the epitaph
as a body, so a fixed-and-documented site still counted against the ledger, the
ledger could never reach zero while the explanation existed, and the cheapest way
to lower the number was to delete the comment. `mask_noncode()` now blanks
comments; its first draft blanked string bodies too and took the count from 37 to
0 with the gate passing green, because the measured pattern *is* a string literal.

Six more were blessed as "quantities" on the theory that a default naming an
object is 0 or 1 while one naming an amount is larger. True of the data, wrong as
a test: three of the six are defects, and each command says so itself.
`screensaver` prints `timeout <id> <s>`, `sysanimations` prints `speed <percent>`,
`filevault` prints `autolock <id> <seconds>`. Angle brackets. The blessing for
`filevault` was justified in writing with the synopsis `autolock [secs]`, square
brackets, a string that appears nowhere in the tree: the documentation that
licensed the exemption was invented. The criterion is now the printed synopsis and
nothing else, and every `allow` line quotes the help text that licenses it.

### Resolution 2026-09-11 — the counted backlog is ZERO

`scripts/absent-operand-ledger.txt` now holds no counted lines, only the three
`allow` entries whose own help prints square brackets (`sharesheet history
[count]`, `apppermissions log [count]`, `udp6 listen <port> [timeout_ms]`). The
detector reports `3 site(s); 0 counted across 0 function(s), 3 allowed`.

The last four were the ones deliberately left behind when ~105 siblings were
fixed, because their subcommands printed no synopsis and therefore had no
documented arity. **The synopsis was derived, not invented** — which matters,
because the earlier attempt in this area invented a `filevault autolock [secs]`
line that appears nowhere in the tree in order to justify an exemption. Every
other subcommand of the same two commands already requires its id in angle
brackets (`audioeq preamp <id> <cb>`, `audioeq remove <id>`, `kbshortcuts unbind
<id>`, `kbshortcuts trigger <id>`), and four existing lines already settled how to
word an arm that accepts two words, the closest being `notiffilter enable|disable
<id>`. So the help text was read off the tree rather than chosen.

`sharesheet share` was a different case: it already had a `parts.len() < 3` guard,
making its `&"0"` default unreachable. Blanked to `&""` so the dead value fails
closed — if the guard is ever removed, a missing target becomes a parse error
rather than silently becoming target 0. Same treatment as `vmfrag compact`, where
a missing result silently meant success.

**Reaching zero is the point, not the tidiness.** With no counted lines left there
is no backlog for a new site to hide inside, so the next numeric default for an
absent operand is a finding on its first appearance rather than a rounding error
in a total.
