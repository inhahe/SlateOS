### TD-OILS-NAMEREF-WARNING-COUNT. bash reports a circular nameref once per walk of the chain — 2026-07-28 — FIXED 2026-08-04

**The rule, measured.** bash prints `warning: NAME: circular name reference`
**once for every time it walks the chain**, and each syntactic shape walks it a
fixed number of times. Counts add for a read-modify-write. This was originally
logged as an unprincipled implementation artifact; it is not — it is a
consistent, decomposable rule, and osh now implements it everywhere:

| shape | walks | why |
|---|---|---|
| `$c1`, `${c1}`, and every modifier on one (`:-`, `:+`, `#`, `^^`, `@Q`, `/a/b`, `:0:1`) | 1 | one read |
| `${c1[0]}`, and `[@]`/`[*]` however spelled — quoted, split, sliced, transformed, through a pointer | 2 | once to find the array, once to read out of it |
| `${#c1[@]}`, `${!c1[@]}` | 1 | these ask about the array, not its contents |
| `${#c1}` | **2** | the exact inversion: whole-parameter length asks after the parameter *then* reads the value to measure |
| `${#c1[0]}` | **1** | …and a subscripted length is answered in one go |
| `${c1@a}`, `${c1@A}` | 3 | the read's 1, plus 2 for asking after the *variable* |
| `${c1[0]@a}`, `${c1[@]@a}` | 4 | the read's 2, plus the same 2 |
| `${c1:=v}` | 2 | read 1, store 1 — stores nothing, expansion abandoned |
| `${c1[0]:=v}` | 4 | read 2, store 2 — and it **lands**, breaking the cycle |
| `${c1[@]:=v}` | 3 | read 2, store 1, then `bad array subscript` |
| `(( c1 ))` / `(( c1[0] ))` | 1 / 2 | the same read split |

**And on the declaration side**, where the count is decided by what the operand
*does* rather than by how it is spelled. The dividing predicate is whether the
operand **makes an array** — brings one into being rather than only saying
something about a binding that is already there:

| shape | walks | why |
|---|---|---|
| `declare c1`, `declare -i c1`, `declare -x c1`, `typeset c1`, `local c1`, `declare +n c1`, and a valued `declare c1=5` | 2 | once to learn what is being declared, once to declare it — and the operand is then dropped with the status left alone |
| `declare -a c1`, `declare -A c1`, `declare -ax c1`, `declare -i 'c1[0]'`, `declare -a c1=(x)` | 1 | an array-making operand stops at the first walk: it has learnt the chain leads nowhere and turns to the operand's *own* name, dropping the nameref attribute and breaking the cycle |
| `export c1`, `readonly c1` | 2 | find, then mark |
| `export c1=5`, `readonly c1=5` | 3 | find, mark, store |
| `export -n c1` / `export -n c1=5` | 1 / 2 | `-n` never reaches the marking lookup |
| `export -a c1`, `readonly -a c1` | 2 | the letter alone creates nothing for these two — `export -a fresh` is a plain `declare -x fresh` — so it keeps the ordinary rule |
| `export -a c1=5` | 1 | *with* a value it is an array write, which falls back on the reference's own name: `declare -ax c1=([0]="5")` |
| `unset c1`, `unset 'c1[0]'` | 2 | once to learn what is being unset, once to unset it |
| `unset -n c1`, `declare -p c1`, `declare -n c1` | 0 | these ask nothing of the chain |

**Fixed** in commits `1799ee4b6`, `d95c40900` (arithmetic), `90157f573`
(parameter reads), `36804129f` (`@a`/`@A`), `0463535a0` (assign-default),
`22a752847` (the `declare` family, count *and* the array-making behaviour),
`67b8d6118` (`export`/`readonly`), `ba6490b58` (`unset`). Corpus:
`arithmetic-through-a-circular-nameref-warns-once-per-walk.sh`,
`a-parameter-read-through-a-circular-nameref-warns-once-per-walk.sh`,
`an-element-assign-default-through-a-circular-nameref-lands-and-breaks-the-cycle.sh`,
`a-declaration-that-makes-an-array-makes-the-operands-own-when-the-chain-is-circular.sh`,
`export-and-readonly-walk-a-nameref-chain-once-for-the-store-and-once-for-the-mark.sh`,
`unset-through-a-circular-nameref-gives-up-the-subscript-and-removes-the-reference.sh`.
`nameref-declare.sh` counts the warning as of `e5f937b7e`, having deduplicated
it with `sort -u` while the counts disagreed.

**The behaviour differences found while measuring are fixed too.**
`declare -a c1` on a cycle drops the nameref attribute and makes the array;
`declare -aA c1` reports `cannot convert associative to indexed array`;
`export -a c1=5` makes `declare -ax c1=([0]="5")`; `unset` removes the
*reference*, leaving the rest of the cycle, and a subscripted operand through a
cycle gives up on the element and removes the reference whole without ever
reading the subscript.

**Where.** `userspace\oils\src\interp.rs` — `Shell::resolve_ref_use_walks` is
the one place that counts, and its callers pass what their shape costs:
`Shell::builtin_declare_scoped`'s `makes_array`, `AttrRefRule::use_walks` for
`export`/`readonly`, and the two sites in `Shell::builtin_unset`.

**Not copied: the local-frame corner is a bash bug.** See
TD-OILS-NAMEREF-LOCAL-FRAME-DECLARATION-IS-A-BASH-BUG.
