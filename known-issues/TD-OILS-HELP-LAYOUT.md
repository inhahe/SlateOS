### TD-OILS-HELP-LAYOUT. Bare `help` uses an osh-identity header + single-column listing, not bash's "GNU bash" banner + COLUMNS-wide 2-column truncated layout — INTENTIONAL / documented — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` — `builtin_help`, the no-pattern branch.

**What:** `help` (no arguments) in bash prints a `GNU bash, version …` banner,
four guidance lines, a star-disabled note, then all builtin synopses laid out in
**two columns** whose width is derived from `$COLUMNS` (default 80), with long
synopses truncated and marked with a trailing `>`. osh instead prints its **own**
identity header (`osh (Oils for SlateOS) <ver>` + the guidance lines) followed by
one **full, untruncated** synopsis per line.

**Update 2026-08-06 — the star is no longer dropped.** The divergence is about
*layout*, and it must not cost information. bash's `dispcolumn`
(`builtins/help.def`) writes `' '` or `'*'` into the first column of every cell
from `BUILTIN_ENABLED`, and the header line explains it; osh implements
`enable -n` but printed neither, so a disabled builtin was indistinguishable
from an enabled one in the listing. osh now prints bash's note verbatim
(`A star (*) next to a name means that the command is disabled.`) and the same
per-name marker. Reserved-word topics are never starred, because `enable -n if`
is refused as "not a shell builtin" and so no reserved word can reach the
disabled set. What remains divergent is only the column layout and the banner —
the two reasons below.

**Why intentional (two independent reasons):**
1. **Identity.** osh must not claim to be "GNU bash" — the banner deliberately
   reports osh's own version, exactly like `--version` and `$BASH_VERSION`. So a
   byte-for-byte match with bash's first line is impossible *by design*, which
   removes most of the value of replicating the rest of the layout.
2. **Losslessness + width-independence.** bash's 2-column layout truncates long
   synopses (`compgen`/`complete`/`printf` lose their tail behind `>`). osh's
   one-per-line listing shows every synopsis in full and does not depend on a
   terminal width osh's line-oriented REPL doesn't track. This is strictly more
   informative for the reader.

`help NAME` / `help -s` / `help -d` / glob and prefix matching all match bash
exactly (see the `builtin_help` test); only the no-argument *listing shape*
differs. Scripts never parse `help` output, so there is no behavioral impact.
