### TD-OILS23. `osh` unquoted word splitting ignores a custom `$IFS` (always splits on whitespace) — RESOLVED 2026-07-19

**RESOLVED 2026-07-19.** Replaced the free function `split_ifs` (which split
only on whitespace via `str::split_whitespace`) with `split_field_ifs(s, ifs)`
implementing full POSIX word-splitting: IFS whitespace collapses/trims, each
non-whitespace IFS char is a single delimiter with adjacent whitespace absorbed,
adjacent non-whitespace delimiters produce empty fields, a trailing delimiter
produces no trailing empty field, empty IFS disables splitting, and empty input
yields no fields. The `other =>` arm of `expand_word_annotated` now reads `$IFS`
from `self.vars` (default `" \t\n"`) and routes through it. Regression test:
`unquoted_word_split_honors_ifs`. Now `IFS=:; x="a:b:c"; for w in $x` yields
three fields; `a::c` yields `a`, ``, `c`; leading/trailing custom delimiters and
mixed whitespace+non-whitespace IFS behave per bash.

**Where:** `userspace/oils/src/interp.rs` — `split_ifs` (splits only on
`char::is_whitespace` via `str::split_whitespace`) and its caller, the `other =>`
arm of `expand_word_annotated`.

**What:** an unquoted expansion is field-split on the *default* whitespace IFS
regardless of the current `$IFS`. So `IFS=:; x="a:b:c"; for w in $x; do …` yields
a single field `a:b:c` (bash: three fields `a`, `b`, `c`), and `IFS=,; set -- a,b;
echo $1` likewise does not split on `,`. The quoted joins now honor IFS correctly
(`"$*"` joins with `$IFS[0]` — see `star_sep`), and the `read` builtin already
splits on the live `$IFS` (`read_split`, which distinguishes whitespace vs
non-whitespace IFS runs) — only the *word-splitting* pass after parameter/command/
arithmetic expansion is stuck on whitespace.

**Proper fix:** replace `split_ifs` (and the `other =>` split in
`expand_word_annotated`) with an IFS-aware splitter modelled on `read_split`:
read `$IFS` from `self.vars` (unset ⇒ `" \t\n"`, empty ⇒ no splitting), collapse
runs of IFS-whitespace, and treat each non-whitespace IFS character as a single
delimiter (with the usual "adjacent whitespace+delimiter counts once" rule). Route
the annotated-expansion unquoted arm through it. This needs `&self` access to read
IFS, so `split_ifs` must become a method (it is currently a free function).
