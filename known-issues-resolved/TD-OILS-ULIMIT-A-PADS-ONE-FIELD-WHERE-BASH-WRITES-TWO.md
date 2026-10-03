### TD-OILS-ULIMIT-A-PADS-ONE-FIELD-WHERE-BASH-WRITES-TWO. The `(unit, -x)` token was right-aligned against a single width-36 field, so the closing paren sat a column short of bash's and a long description would have been truncated — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `fn ulimit_line`.

**What:** osh computed `36 - paren.len()` and padded the description to that,
which makes the paren close in column 39. bash writes two independent printf
fields, `%-20s` for the description and `%21s` for a `(unit, -x) ` token that
carries its own trailing space:

```text
core file size              (blocks, -c) 0
open files                          (-n) 3200
pipe size                (512 bytes, -p) 8
```

**Measured rule** (bash 5.2.37, checked on all nine limits MSYS exposes —
description widths 25, 28 and 36 among them): the `)` lands in **column 40**
and the value starts in **42**, for every unit width. That is the whole point
of the layout: it is what makes a column of values line up.

The two-field form is not a stylistic restatement of one padded width — it is
what decides the overflow case. A description longer than 20 is *not*
truncated; the token is still right-aligned behind it, so the columns give way
rather than the text. No label in `RLIMIT_SPECS` is that long today (`POSIX
message queues` is exactly 20), but the rule is bash's, not a coincidence of
the current table, so it is spelled as bash spells it.

**Fixed** by `format!("{:<20}{:>20} {}\n", …)`. Pinned by the unit test
`ulimit_dash_a_puts_the_closing_paren_in_column_forty` (which walks every line
asserting the paren column and the gap byte, then spot-checks the three unit
widths literally) and by the corpus case `ulimit-a-aligns-the-unit-token.sh`.

**Standing lesson:** when a layout is described by a single derived width, ask
what the source computes it *from*. A width that happens to agree on today's
data is not the rule; two fields and one field differ only in the case the
current table does not reach, which is exactly the case a port gets wrong.
