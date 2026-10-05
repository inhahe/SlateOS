### TD-OILS-NOASSIGN-VARS. bash's six unassignable variables can be clobbered in `osh` — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::noassign` (seeded from
`NOASSIGN_VARS`), checked in `apply_assignment`, `scalar_write_checked`,
`exec_for`, `arith_write_dest`, `arith_elem_writable`, `builtin_printf`,
`builtin_read`, `builtin_mapfile`, `builtin_declare_scoped`,
`builtin_export`, `builtin_readonly` and `exec_declare_with_arrays_scoped`;
`NOUNSET_VARS` in `builtin_unset`; `ArithError::silent` in
`userspace/oils/src/arith.rs`.

**What:** bash gives `FUNCNAME`, `GROUPS`, `BASH_SOURCE`, `BASH_LINENO`,
`BASH_ARGC` and `BASH_ARGV` the `att_noassign` attribute, and the last
four `att_nounset` besides. `DIRSTACK` and `COMP_WORDBREAKS` look like
they belong and do not — both are freely assignable in bash. Measured
against bash 5.2.37 (MSYS):

| form | bash | osh before | osh now |
|---|---|---|---|
| `GROUPS=5`, `GROUPS[0]=5`, `GROUPS+=x` | silently ignored, status 0 | assigns | ✅ |
| `GROUPS=(1 2)`, `GROUPS+=(1)` | silently fails, status 1, **abandons the rest of the parse unit** (`GROUPS=(1); echo after` prints nothing; the same two on separate lines print `after=1` — the abort osh already had for `readonly x=1; x=2`) | assigns | ✅ |
| `read GROUPS`, `for GROUPS in a b` | silently fails, status 1; the loop body never runs, but an *empty* list is still success | assigns | ✅ |
| `set -x; GROUPS=5` | traces `+ GROUPS=5` and then ignores it | — | ✅ |
| `unset FUNCNAME` / `unset GROUPS` | succeeds; the name sheds the attribute and a later `FUNCNAME=(1 2)` makes an ordinary array | unset a *function* by that name (the variable holds no value outside a call, so it never looked like one) | ✅ |
| `unset BASH_SOURCE`/`BASH_LINENO`/`BASH_ARGC`/`BASH_ARGV` | `unset: NAME: cannot unset`, status 1, no abort | unsets | ✅ |
| `printf -v GROUPS x`, `printf -v 'GROUPS[0]' x`, `mapfile GROUPS`, `read -a GROUPS` | silently fails, status 1, value unchanged; the rest of the parse unit still runs | assigns | ✅ |
| `(( GROUPS = 5 ))`, `(( GROUPS[0]=5 ))`, `(( GROUPS++ ))`, `let GROUPS=7` | status 1, value unchanged, silent; the expression is abandoned where it stands, so `(( x=3, GROUPS=5 ))` leaves `x` as 3 and `(( GROUPS=5, x=3 ))` leaves it as 9; in a *word* (`echo $(( GROUPS = 5 ))`) it abandons the parse unit like any fatal arithmetic error | assigns | ✅ |
| `declare`/`typeset GROUPS=…` naming the **global** | status 1, silent, value unchanged — and *no attribute applied either*, the operand being abandoned whole (`declare -x GROUPS=5` leaves a plain `declare -a GROUPS`), while the valueless `declare -u GROUPS` applies `-u` normally | assigns | ✅ |
| `export GROUPS=…`, `readonly GROUPS=…` | status **0**, silent, value unchanged, and the attribute *is* applied (`declare -ax GROUPS=(…)`) | assigns | ✅ |
| `declare`/`local`/`typeset GROUPS` **creating a local** | `<tag>: GROUPS: variable may not be assigned value`, status 1, no abort, no attribute, no local made — even with *no value*, since binding a local of the name is itself the refused assignment | assigns | ✅ |
| `declare GROUPS=(…)` **creating a local** | the same, reported *twice*: once by the compound-assignment machinery (which inside a function tags its diagnostics with the function's name) and once by the builtin | assigns | ✅ |
| `declare -g GROUPS=(…)` / `export GROUPS=(…)` inside a function | silently succeeds, status 0, value unchanged, attributes applied | assigns | ✅ |
| `declare GROUPS=(…)` at top level | not special-cased at all: the same silent parse-unit discard a bare `GROUPS=(1 2)` takes, so a later operand of the same command never binds either | assigns | ✅ |

**Why it matters:** five of the six are osh's own materialised state (the
call-stack views and the extended-debugging argument stack), so a script
that assigns to one corrupts the shell's bookkeeping rather than merely
disagreeing with bash.

**Fixed 2026-07-31:** `noassign: HashSet<String>` on `Shell`, seeded from
`NOASSIGN_VARS`, cloned into subshells and dropped per-name by
`unbind_var`. `apply_assignment` refuses after the `set -x` trace —
returning success for a scalar and failure for an array literal, which
routes the latter into the existing readonly-abort path with no
diagnostic; `scalar_write_checked` refuses silently, which is where
`read` gets its status 1; `exec_for` mirrors its own readonly guard.
`builtin_unset` gained the `cannot unset` refusal and now recognises a
`noassign` name as a *variable* even when it holds no value. Corpus case
`noassign-vars.sh`; unit test
`the_variables_the_shell_maintains_refuse_assignment`.

**Builtin and arithmetic write paths — fixed 2026-07-31 (second pass).**
`arith::ArithError` gained a `silent` flag and an
`ArithError::silently_refused` constructor: the refusal travels as an
ordinary arithmetic error — abandoning the expression where it stands,
failing `(( ))`/`let`, and staying fatal to the command list in expansion
position — while `Shell::emit_arith_error` returns before printing
anything. `arith_write_dest` and `arith_elem_writable` raise it beside
their readonly checks. `builtin_printf`'s `-v` branch, `builtin_read`'s
`-a` branch and `builtin_mapfile` each refuse silently beside their own
readonly guard. Corpus case extended; unit test
`the_builtin_write_paths_refuse_the_variables_the_shell_maintains`.

**The declaration builtins — fixed 2026-07-31 (third pass).** This looked
self-inconsistent from a distance (the same `declare NAME=(…)` discards
the parse unit at top level, prints two diagnostics when it would create a
local, and silently succeeds under `-g`) and the entry above proposed
diverging from it deliberately. Measuring the whole matrix instead showed
one coherent rule, which is what was implemented: **the refusal is by
where the binding would land, not by which builtin asked.** A *local*
target is refused loudly and abandoned whole — no local, no attribute, no
value, status 1 — and the compound form is reported by both the
compound-assignment machinery and the builtin, which is where the doubled
diagnostic comes from. A *global* target is refused silently, with the
builtin deciding only what survives: `declare` applies nothing and reports
1, `export`/`readonly` apply their attribute and report 0. A *global
compound at top level* is not special-cased at all — it is the plain
assignment path, and takes that path's discard. `builtin_declare_scoped`
gates on `value.is_some() || make_local` (which is why the valueless
`declare -u GROUPS` still applies `-u`); `builtin_export` and
`builtin_readonly` drop only the store; `exec_declare_with_arrays_scoped`
carries the compound half. Corpus case extended; unit test
`a_declaration_builtin_refuses_the_variables_the_shell_maintains`.
