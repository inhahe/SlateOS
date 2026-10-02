## TD-B-THE-2285-DELETIONS-PREDATE-THEIR-INSTRUMENT'S-FIX (lane B, 2026-09-10) — checked, no impact

**In short:** `ccefac978` at 05:51 deleted 2,285 commands on the strength of
`scripts/audit-cli-fabrication.py`. `2db20437e` at 09:22 fixed a bug in that
script which made it **discard every file after the first test module** — whose
own docstring names the consequence as a *false positive*, "a crate that
genuinely reads the world could be reported as doing no I/O". The deletions
predate the fix by three and a half hours. Checked: **no crate was wrongly
deleted.**

**Why the blast radius is one crate.** The bug only bites a crate whose sources
are CONCATENATED — `crate_sources` joins all of `src/**/*.rs`, so the first test
module in the alphabetically-first file discarded the rest. A single-file crate
with its tests at the bottom is judged correctly, because the truncation lands
exactly where the tests begin.

Of the 2,285 deleted crates, **2,284 had one `.rs` file each.** Exactly one,
`userspace/cryptsetup-cli`, had two.

**And that one judges identically either way.** Restored from `ccefac978~1` and
re-run: the buggy truncation saw 141 lines, the fixed `strip_tests` sees 143,
and both find **zero** I/O markers. The deletion was correct.

**Why this was worth an hour.** The docstring warns that the instrument's output
"was used to delete 2,285 crates" and stops there, so it reads as an open
liability that nobody had bounded. It is now bounded, by the cheapest possible
question — how many of the affected crates could the bug even reach — rather
than by re-auditing 2,285 of them.

**Still open, and smaller.** `strip_tests` matches braces over the RAW text, so
a `{` or `}` inside a string or comment within a test module unbalances it.
`check-read-defaults.py`'s `live_code` does the same job over a `strip_noise`
mask, which is why it cannot. Sharing that lexer rather than copying it is the
right fix — it has been wrong twice already, and a second copy is the defect
this file spent the day removing — but it means extracting `strip_noise` into a
module both import, which is a change to make deliberately rather than in
passing.
