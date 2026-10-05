## `A-KSHELL-THE-OPTION-GATE-COUNTS-ONE-LINE-AND-RUSTFMT-USES-FOUR` (lane A, 2026-08-25) — **FIXED** 2026-08-25 (the gate); the 706-site backlog it revealed is now carried honestly

> **Resolution.** The blind spot is closed. `check-recursive-locks.py` — the
> directory's one self-tested Rust scanner — gained `statements(code, struct)`,
> which yields each statement-sized span with its newlines collapsed, so a
> regex spanning method calls matches the wrapped and unwrapped spellings
> alike. `check-option-refusal.py` now runs D1 and D2 over that instead of over
> `lines`.
>
> The predicted figure held: **240 → 706**, i.e. 466 sites that had been in the
> shell all along and were invisible because rustfmt had wrapped their chain.
> The estimate above said "about 712"; it was made by gluing a statement
> starting at *every* line, which double-counts a wrapped chain once per line
> it occupies. 706 is the measured number.
>
> **No function's count went down**, which is the check that this is purely
> additive — the gate is seeing more, not re-attributing what it already saw.
> `scripts/option-refusal-ledger.txt` was regenerated wholesale: 110 functions
> → 240, and its header now records why its known-issues key names a figure a
> third of the truth (renaming the key would break every reference to it).
>
> Eight self-test cases guard the span walk, each one answered wrongly by a
> line-granular splitter: the wrapped chain; the unwrapped chain, which must
> produce the *same* span or the blind spot has only moved; a `(` inside a
> string literal (counting it makes the depth drift up and never return — that
> mistake, made in the first draft, collapsed 100698 spans into 1068 and 706
> findings into 2); a `;` inside a literal; sibling match arms, which must not
> glue, or a `.parse()` in one and an `.unwrap_or()` in the next would match a
> chain that is not in the source; an argument comma, which must not split; a
> closure body, which must stay with its chain; and the crediting of a span to
> its first line.
>
> **The burn-down itself is not done** — 706 sites across 240 functions are
> carried in the ledger, where they can only shrink. That work continues under
> `A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.
> What is fixed here is the measurement: the number in the ledger is now the
> number in the shell.
>
> **The lesson, which is the same one as the entry above it, one granularity
> up.** That one counted braces by the line and hid 195 findings; this one
> counted statements by the line and hid 466. Both were *silent*, because a
> gate that undercounts prints a smaller number and reads as progress. Three
> counters in `scripts/` remain line-granular and are correct — the rule that
> separates them is now written into `walk_block`'s and `statements`'
> docstrings: a walk over a *balanced* region is safe by the line; a walk that
> must see depth go *negative*, or a regex that spans a construct **rustfmt is
> free to wrap**, is not.

**In short:** the shell has a known, deliberately-counted backlog of places
where it invents a value for a word it could not read — `bright set 1 abc`
reading `abc` as 0 and turning the backlight off, that family. A gate counts
that backlog so it can only shrink. The gate reads the source **one line at a
time**, and the code formatter splits exactly these expressions across four
lines. So the count it publishes — 240 — is not the backlog. The backlog is
**about 712**. Two thirds of a debt that exists specifically to be honest
about its size are missing from it.

**Where.** `scripts/check-option-refusal.py`, the `D1` scan in `main`:

```python
for i, ln in enumerate(lines):
    ...
    if D1.search(code) and not allowed(fn, shown):
```

`D1` is `\.parse(?:::<[^>]*>)?\(\)[^;]*?\.unwrap_or(?:_default|_else)?\b` — a
single-line regex, and `[^;]` cannot cross a newline anyway. It sees

```rust
let level = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
```

and misses the *identical expression* after `cargo fmt` decides the chain is
too long:

```rust
let level = parts
    .get(2)
    .and_then(|s| s.parse::<u32>().ok())
    .unwrap_or(0);
```

Which form a site is in has nothing to do with whether it is a defect. It is
decided by the length of the variable name and the depth of the indentation.

**The measurement.** Walking statements instead of lines — glue each line to
its continuations up to the `;`, then match once per statement, so a wrapped
chain is one unit and cannot be counted once per line it spans:

| | statements matching `D1` |
|---|---|
| seen by the gate as shipped (per line) | 236 |
| **hidden by line granularity** | **472** in 148 functions |
| total | 708 |

(236 rather than the ledger's 240 because a statement holding two `D1` chains
counts once here and twice there. The ledger is per line.)

Worst affected: `cmd_partmgr` 21, `cmd_vdesktop` 18, `cmd_colorpicker` 16,
`cmd_winsnap` 12, `cmd_wintiling` 11, `cmd_screenshot` 10, `cmd_a11y` 10,
`cmd_filepicker` 10, `cmd_focusassist` 9, `cmd_peninput` 9, `cmd_aiostat` 8,
`cmd_cgiostat` 8.

A random sample of eight hidden sites was read by hand; all eight are genuine,
and one is the defect twice in one statement:

```rust
let pid = parts
    .get(1)
    .copied()
    .unwrap_or("0")      // no pid given  -> "0"
    .parse::<u32>()
    .unwrap_or(0);       // pid unreadable -> 0, the same value
```

**Why this is the same family as the `} else {` bug and not the same bug.**
Both are a scanner reading Rust a line at a time when Rust is not written a
line at a time. But the rule that cleared the other three counters — *a walk
over a balanced block is safe at line granularity; only one that must see
depth go negative is not* — is about **brace counting**, and this is not brace
counting. It is **pattern matching across a statement**, and there the rule is
simpler and admits no exceptions: a regex spanning method calls must be
matched against the statement, because the formatter, not the author, chooses
where the newlines go. `check-usage-status.py` was fixed by sharing a walk;
this needs a shared *statement view* — a list of `(start_line, end_line, text)`
built once from `strip_noise` output — which `check-variant-lists.py` and
`check-tick-wiring.py` would also be able to use.

**The fix.**

1. Add a statement view to the shared scanner in `check-recursive-locks.py`,
   with self-test cases covering the wrapped-chain form specifically (it is
   the form that hid 472 sites, so it is the form a regression must fail on).
2. Match `D1` and `D2` against statements rather than lines.
3. Regenerate `scripts/option-refusal-ledger.txt`. The published debt goes
   from 240 across 110 functions to roughly 712 across ~200. **That number
   going up is the point** — a counted ledger (§296) is worth having only if
   the count is the real one, and this one has been understating it since the
   day it was written.
4. Continue the burn-down against the true figure.

**What it costs to leave.** Nothing gets worse on its own, but every burn-down
report so far has quoted a denominator that is wrong by 3×, and the ledger's
one job is to be believed. The sites themselves are ordinary D1: a mistyped
number silently becomes a default, and the command reports success.

**Not a regression.** True since the checker was written. Found while fixing
the `} else {` bug, from a single site — `cmd_brightness` had an obvious
`.unwrap_or(1)` on an unreadable operand and was not in the ledger.
