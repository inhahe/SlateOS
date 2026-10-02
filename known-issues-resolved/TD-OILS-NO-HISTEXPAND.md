### TD-OILS-NO-HISTEXPAND. `!`-style history expansion — 2026-07-29 — RESOLVED 2026-07-29

**Was.** osh listed `histexpand` in `set -o` but never acted on it: with both
history options on, `!!` reached the parser literally and osh tried to run a
command called `!!`.

**Now.** Implemented in full and pinned byte-for-byte against bash by
`tests/corpus/histexpand.sh` (the expansion) and
`tests/corpus/histexpand-option.sh` (the `set -H` switch itself). Three pieces:

- `src/histexpand.rs` — the expansion engine, deliberately pure: a line plus a
  view of the history in, the rewritten line out. Unit-tested on its own
  (16 tests) so its rules can be checked without a parser or a shell.
- `IncrementalParser::peek_raw_line` / `commit_raw_line` / `drop_raw_line`
  (`src/parser.rs`) — hand out the next *physical* line before it is lexed, and
  splice the expanded text back into `src`, re-lexing the tail via the
  generalised `relex_from`. This is the same mechanism a mid-script
  `shopt -s extglob` already used, plus a source edit.
- `Shell::expand_history_lines` (`src/interp.rs`) — the driver, called from
  `run_source_flow_out` before each `next_unit`, and only for the top-level
  reader (`record`), since bash expands nothing an `eval`/`source`/`$( … )` body
  reads.

**Measured bash model (dev-host MSYS bash 5.2, 2026-07-29)** — everything below
is implemented and covered by the corpus case:

- Reachable non-interactively once **both** `set -o history` and `set -H` are
  on; both default off. `set -H` alone does nothing at all.
- Event designators: `!!`, `!n`, `!-n`, `!string` (most recent entry *starting
  with*), `!?string?` (most recent *containing*), `^old^new^`.
- Word designators `!$`, `!^`, `!*`, `!!:n`, `!!:n-m`, `!!:n*`; modifiers
  `:h :t :r :e :s :gs :& :p :q :x`.
- Quoting: **single quotes suppress expansion, double quotes do NOT** — the rule
  most likely to be got wrong, being the opposite of every other expansion. A
  backslash suppresses it too and is *left in place*: history expansion does not
  eat it, the parser does, so `echo \!!` is reported unchanged (no echo of a
  rewritten line) yet still prints `!!`.
- A bare `!` before space, tab, newline, `=` or `(`, or at end of line, is
  literal.
- A rewritten line is echoed **to stderr** before being run, and it is the
  *expanded* form that goes into the history.
- `:p` echoes and records the line but does not run it.
- Failures come in three wordings, each quoting the modifier back verbatim:
  `!x: event not found`, `:s/a/b/: substitution failed`,
  `:&: no previous substitution`. The line is then dropped whole — not run, not
  recorded, `$?` untouched.
- The last `s/old/new/` is **shell-global**: a `:&` (or an empty `:s//new/`) on
  one line repeats the substitution from a line expanded much earlier. Held in
  `Shell::hist_last_subst`, cloned into subshells.

**Two behaviours worth remembering, because they look like bugs.**

1. *Expansion is per physical line, not per parse unit.* Within one unit the
   two would agree — history is recorded per unit (`record_history`, once per
   `next_unit`), so `!!` on line 2 of an `if` names the command *before* the
   `if` — but the unit's extent is only knowable by lexing it, and expansion
   changes what lexing sees. So `expand_history_lines` loops a line at a time
   and stops on the first *complete* command, judged by `needs_more_lines`.
   Blank and comment lines do not stop it (the parser folds them into the unit
   that follows, so the line after a comment would otherwise never be offered).
2. *A discarded line does not advance the source line counter.* bash counts a
   line only when its newline reaches the lexer, so after a failed expansion or
   a `:p`, every later diagnostic — and `$LINENO` — is one lower. That is
   reproduced exactly, and deliberately: `drop_raw_line` cuts the line *and its
   newline* out of `src`, and osh's line numbers are likewise counted from the
   newlines in `src`. Without it the corpus case diverges from line 102 on.

**Known remaining gap.** `!#` (the current line so far) is approximated as the
text before the designator on the current physical line, which is exact for a
single-line command and the only case bash's own docs describe usefully. Not
covered by the corpus case.
