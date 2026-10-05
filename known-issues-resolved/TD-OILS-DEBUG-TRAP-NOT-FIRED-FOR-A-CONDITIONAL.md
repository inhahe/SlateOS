### TD-OILS-DEBUG-TRAP-NOT-FIRED-FOR-A-CONDITIONAL. `[[ … ]]` is never announced — ✅ **RESOLVED 2026-08-01** — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `exec_cond`, which unlike
`exec_arith`/`exec_for`/`exec_case` does not call `Shell::announce_debug`.

**Reproduce:**

```sh
b() { echo "B:[$BASH_COMMAND]"; }
trap b DEBUG
[[ -n x ]]
```

bash prints `B:[[[ -n x ]]]`; osh prints nothing. (A `[ -n x ]` *is* announced —
that one is an ordinary simple command.)

**Fixed.** The expected blocker — that osh had no unparser for a `CondExpr` —
turned out not to exist: `unparse::cond_src` was already there, written for the
`declare -f` printers, and already normalising exactly as bash's
`print_cond_command` does (whitespace collapsed, a bare word spelled out as the
`-n` test it means, everything else kept as written — the operator's own
spelling, the quotes round a word, an expansion left unexpanded, and a
redundant `( … )` group, which has to survive because dropping it would regroup
the expression). It was private and both printers wrapped it in `[[ `/` ]]`
themselves; that wrapping is now a public `unparse::cond_command_src`, which
the two printers and `exec_cond` share.

`exec_cond` announces it exactly as `exec_arith` does, with `Skip` leaving
status 0 — and a refused conditional is not evaluated at all, so an expansion
inside it does not run either.

Measured against bash across the operator set and pinned by
`tests/corpus/debug-trap-announces-a-conditional.sh`, which also reads the
announcement side by side with what `set -x` makes of the same conditionals
(per-sub-test, expanded, requoted — a completely different printer), covers
where a conditional gets announced *from* (an `if`/`while` header, either side
of `&&`/`||`), and records that a `[[ … ]]` **pipeline stage** is announced by
nobody: the forking shell announces only simple-command stages, and the fork
itself has no DEBUG trap without `functrace`.

**Found while fixing it:** the announcement lands *before* the trace, and osh
had `exec_arith` the other way round — so a `(( … ))` traced before it was
announced, and a *refused* one was traced despite never running. Both are
fixed by the same reordering, and the corpus case pins them.
