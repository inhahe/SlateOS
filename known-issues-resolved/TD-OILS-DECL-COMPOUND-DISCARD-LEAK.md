### TD-OILS-DECL-COMPOUND-DISCARD-LEAK. A failed expansion in a declaration builtin's array operand discards the *next* command too — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `exec_declare_with_arrays`, which never
consumes `self.discard_error` after phase 1.

**What.** A word-expansion error inside a compound operand (`declare -a fg=(x
$((1/0)))`) leaves `self.discard_error` set. `exec_declare_with_arrays` returns
`Flow::Next`, so the flag survives to the *following* command's word-expansion
check in `exec_simple_inner`, which then discards a command bash runs normally.
Reproduce:

```sh
declare -A ma; ma=([z]=old)
declare -A ma+=([a]=1 [b]=$((1/0)) [c]=3)
declare -p ma          # bash prints ma; osh prints nothing — discarded
echo "st=$?"           # ... and with two statements the echo is eaten instead
```

It is very visible under `set -x`, where the swallowed command is the `set +x`
that was supposed to turn tracing off.

**Resolution (2026-07-30).** Phase 1 of `exec_declare_with_arrays` now consumes all
three word-expansion flags right after each `apply_assignment`, in the same order
and with the same meanings the bare-assignment path at the top of
`exec_simple_inner` uses: `glob_error` → report `no match:` and `Flow::Discard`
with status 1, `unbound_error` → `Flow::Exit(fatal_abort_status(code))`,
`discard_error` → `Flow::Discard` with the carried status. Pinned by
`a_failed_operand_expansion_discards_only_the_rest_of_the_parse_unit` and an
extension to `tests/corpus/declare-compound-operands.sh`.

Measured while fixing it, and worth recording: the discard genuinely does take out
the rest of a `{ … }` group — bash gives nothing at all for
`{ declare -a a=(1); set -x; declare -a b=(x $((1/0))); set +x; echo done; } 2>&1`
— because the group *is* the parse unit. Only a following top-level line survives.
An operand to the left of the failing one keeps its binding; one to the right never
binds, since bash aborts the command mid-expansion.
