### TD-OILS-DYNAMIC-SPECIALS-ONLY-ANSWER-A-PLAIN-ASSIGNMENT. `SECONDS`, `RANDOM` and `BASH_SUBSHELL` act on a write only when it is spelled `NAME=value`; every other write path stores a string that nothing reads — 2026-08-04 — ✅ RESOLVED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the dynamic-special branch of
`Shell::apply_assignment_inner` (~11640–11706), which is where
`seconds_base`/`seconds_anchor`, the `rng` seed and `subshell_base` are moved.
Every other write reaches `Shell::scalar_write_store` instead, which knows
nothing about them.

**What.** bash attaches an *assign function* to these names, so it runs
whatever reaches the variable. osh runs it for `NAME=value` (and for a nameref
or an environment prefix, which funnel into the same path) and for nothing
else:

| write            | `SECONDS=5` then `$SECONDS` | bash | osh |
|------------------|------------------------------|------|-----|
| `SECONDS=5`      |                              | 5    | 5   |
| `((SECONDS=5))`  |                              | 5    | 0   |
| `let 'SECONDS=5'`|                              | 5    | 0   |
| `read SECONDS`   |                              | 5    | 0   |
| `printf -v SECONDS 5` |                         | 5    | 0   |
| `for SECONDS in 5; do :; done` |                | 5    | 0   |

`RANDOM` is the same (a seed set by `((RANDOM=42))` or `printf -v RANDOM 42`
does not reproduce its sequence in osh, and does in bash), and so is
`BASH_SUBSHELL` (`((BASH_SUBSHELL=7))` → 7 in bash, 1 in osh). The store *does*
land in the value cell, so `declare -p` can disagree with the reading.

**Proper fix.** Move the dynamic-special handling out of
`apply_assignment_inner` and into the shared scalar store
(`Shell::scalar_write_store`), which every checked scalar write already passes
through — the same place `Shell::after_var_write` sits, and for the same
reason. The `NAME=value` path keeps only what is genuinely its own: the
`declare -i`-evaluated value (`DynamicSpecial::assign_evals`) and the append
form, both of which are decided from the assignment's syntax.

**Impact.** A write to one of the dynamic specials through `read`, `printf -v`,
a `for` control variable or any arithmetic assignment is silently inert
(measured); every other caller of the shared scalar store — `select`, `getopts`,
a `read` element target — is presumably the same and was not probed. Found
while probing TD-OILS-READONLY-REFUSAL-NAMES-TARGET.

**Fixed** exactly as prescribed: the whole effect — the shadowing guard, the
`BASH_ARGV0` interception, the number parse, the three counters and the value
cell — moved into `Shell::dyn_special_write`, called from
`Shell::scalar_write_store`'s whole-variable arm as well as from
`Shell::apply_assignment_inner`. The assignment path keeps only `a.append`,
which is the one thing its syntax decides. Covered by
`tests/corpus/every-write-to-a-dynamic-special-runs-the-names-own-assign-function.sh`,
which runs every path against `SECONDS`, `RANDOM`, `BASH_SUBSHELL`,
`BASH_ARGV0`, `LINENO`, `EPOCHSECONDS`, `BASHPID`, `PPID` and the call-stack
arrays.

The probe that confirmed it also settled three things the entry had not:

* `getopts` and `select` do run the assign function — `getopts a BASH_SUBSHELL`
  leaves the counter at 0, not at `"a"` — so the guess in **Impact** was right.
* A bad `-i` value is a diagnostic and *not* a failure whichever path carries
  it: `read SECONDS <<< 1/0` prints `read: 1/0: division by 0` and still
  reports 0, exactly as `RANDOM=1/0` does. `dyn_special_write` therefore
  disarms the abort `eval_int_assign` armed and answers success either way.
* The integer attribute reaches the value on every path, not just the
  assignment: with `SECONDS` touched, `read SECONDS <<< 3+4` stores 7 and an
  untouched one stores 0 — which is `DynamicSpecial::assign_evals` doing the
  same work for `read` that it did for `NAME=value`.
