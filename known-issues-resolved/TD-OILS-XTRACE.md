### TD-OILS-XTRACE. `set -x` tracing: no `[[ … ]]` conditional trace, no nested command-substitution trace, no `PS4` parameter expansion — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — the `set -x` trace block in
`exec_simple` (after the readonly-prefix guard), `apply_assignment` (bare-
assignment tracing), the compound-command exec sites (`exec_for`,
`exec_for_arith`, `exec_case`, `exec_select`, `exec_arith`), `xtrace_prefix`,
`xtrace_emit`, and `xtrace_quote`.

**Status (2026-07-19, updated):** simple-command, bare-assignment, and most
compound-command tracing now match bash byte-for-byte:

- Plain scalars trace their *expanded* value (minimally single-quoted, empty
  shown unquoted); indexed-element/array assignments trace in *source* form;
  temporary prefix assignments (`FOO=bar cmd`) each trace on their own line;
  command arguments are minimally quoted; `PS4` overrides the `+ ` prefix.
- `for NAME in WORDS` prints a *source-form* header before **each** iteration
  (`for i; do` → `for i in "$@"`).
- C-style `for ((init;cond;update))` traces `(( init ))` once, `(( cond ))`
  before each test, `(( update ))` after each body; an **empty** section is
  traced as always-true `(( 1 ))` (so `for ((;;))` traces `(( 1 ))` for the
  init and cond slots), matching bash.
- `select NAME in WORDS` prints a source-form header once (bash does not
  re-emit it per iteration).
- `case WORD in` prints `case <source-word> in` (unexpanded) before matching.
- `(( … ))` commands trace `(( <raw> ))` (raw text preserved, so interior
  spacing like `((  2 > 1  ))` matches). This also covers `while`/`until`
  arithmetic *conditions*, which self-trace via the `(( ))` command path (bash
  emits no separate `while`/`until` header, and neither does `osh`).

**Resolution (2026-07-30).** The three gaps this entry was left open for are all
closed — in one commit, since they interlock: a `[[ … ]]` inside a `$( … )`
needs the indirection level, and the level is only observable through an
expanded `PS4`. Each was measured against bash 5.2.37 and is pinned by a corpus
case (`tests/corpus/xtrace-cond.sh`, `xtrace-ps4.sh`, `xtrace-quoting.sh`) plus
unit tests. What each turned out to be:

- **`[[ … ]]` conditional tracing** — `Shell::cond_eval_inverted`
  and the `cond_trace_*` helpers. bash traces one line **per primary**, emitted
  after both operands are expanded and before they are tested, so a
  short-circuited `&&` never prints its right half and an `||` can print two
  lines for one command. Groups print without their parentheses; a bare word
  prints as the `-n` it was rewritten to; a `!` is a flag on the node it binds
  to (`[[ ! ( x == y ) ]]` prints no `!` at all, and `! !` cancels); operands
  print with **no** quoting except an empty one as `''`; and a pattern RHS
  backslash-escapes whatever a quote made literal (every such character for
  `==`/`!=`/`=`, only the regex-special ones for `=~`). Prerequisite: the AST
  had to start carrying the operator's *spelling* (commit `770b9d755`), since
  bash echoes back `-h` vs `-L` and `=` vs `==` as written — which also fixed a
  pre-existing `declare -f`/`type` bug.
- **Nested command-substitution trace** — new `Shell::xtrace_level`, bash's
  `indirection_level`. A command substitution (and a process substitution) adds
  one level for the whole of its body, nesting included, and each level repeats
  the *first character* of the expanded prefix once more: `v=$(echo a)` traces
  `++ echo a` then `+ v=a`, `PS4='XY '` gives `XXY `. A `( … )` subshell, a
  function call and a pipeline are not expansions and add nothing.
- **`PS4` parameter/arithmetic expansion** — `Shell::prompt_expand` now decodes
  the prompt escapes and then expands the *result* as if inside double quotes
  (bash's `Q_DOUBLE_QUOTES`), so `PS4='+ $LINENO '` works and `${x@P}` gained
  the same behaviour. Nothing is word-split or globbed and a `'`/`"` stays
  literal; tracing is suppressed during the expansion, so a `$( … )` in `PS4`
  cannot trace itself. Two related findings came out of the same measurement:
  `PS4` is a *variable* bash seeds with `+ ` (so `unset PS4` leaves **no**
  prefix, and `declare -p PS4` prints it) whose inherited value wins — hence
  `SOFT_DEFAULT_VARS`; and `\u` now also falls back to `$USERNAME`.

- **`if` header.** bash (like `osh`) emits no `if`/`then`/`else` header — the
  guard and body commands self-trace. Always was correct; noted for completeness.

One `set -x` divergence remained, tracked separately because its cause was the
pipeline executor rather than the tracer: TD-OILS-XTRACE-PIPE-ORDER (stage start
order), resolved 2026-07-31.
