## TD-B-SWEPT-FOR-OTHER-DOCUMENTED-BUT-UNPARSED-FLAGS (lane B, 2026-09-10) — closed, `who` was isolated

**In short:** `coreutils/src/bin/who.rs` documented `Usage: who [-a]` and read
no arguments at all. Swept the rest of coreutils for the same shape — a flag a
program advertises and never parses. **There are none.** Recorded because the
sweep took three attempts and each failure is the same kind.

**Attempt 1 — flags anywhere in the `//!` block.** Four hits, all prose: `ls -l`
in a comment about column widths (`chown`), `cat -A` in an example pipeline
(`more`), `diff -u` describing an input format (`patch`), and `--workspace`
from a `cargo` command someone quoted. A doc block mentions *other commands*,
and their flags look exactly like the subject's.

**Attempt 2 — only the `Usage:` line and the option list beneath it.** Precise,
and nearly empty: **6 of ~100 binaries have such a block**, so a clean negative
over them says almost nothing. `who` itself would have been caught, which is
the only reason the attempt was worth making.

**Attempt 3 — the runtime `--help` text, which is what a user actually sees.**
31 binaries print an option list. Three appeared to advertise an unparsed long
option: `free --giga/--pebi/--peta`, `kill --list/--table`,
`stat --quoting-style`. All three are parsed. **Option tables store names
without the leading dashes** — `("giga", Takes::Nothing)`, matched as
`Opt::Long("giga", _)` — so a comparison keyed on the `--` spelling finds the
help text and misses the parser.

**The transferable part:** for this question the *help text* is the right
corpus and the *parser table* is the wrong one to match textually, because the
two spell the same flag differently on purpose. `who` was findable only because
it had no parser at all.
