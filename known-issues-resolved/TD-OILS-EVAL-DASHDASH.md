### TD-OILS-EVAL-DASHDASH. `eval` does not honour `--` as end-of-options — 2026-07-28 — RESOLVED same day

**Where:** `userspace/oils/src/interp.rs`, the `eval` builtin.

**What:** bash's `eval` accepts `--` and drops it; osh joined it into the
command text, so `eval -- 'echo hi'` ran `-- echo hi` and reported
`--: command not found` (status 127) instead of printing `hi`. Found while
writing `tests/corpus/cond-multiline.sh`, where the probes were originally
written defensively as `eval -- '…'`.

**It was never only `eval`.** Auditing the rest turned up four more builtins
with the same gap — `let`, `shift`, `return`, `exit` — against eleven that
already handled the marker (`command`, `source`, `unset`, `export`, `declare`,
`set`, `printf`, `trap`, `type`, `break`, `continue`).

**Fix:** one `strip_end_of_options` helper beside `parse_exit_status`, applied
at each of the five sites, with `loop_count_arg`'s open-coded copy folded into
it. It drops *exactly one* leading `--`, which is bash's rule — a second is
ordinary data, so `exit -- -- 3` is "numeric argument required" and
`eval -- -- cmd` runs `-- cmd`. Builtins that take no options at all (`eval`,
`let`) honour the marker anyway, because bash's shim strips it before the
builtin ever sees its words. Covered by `tests/corpus/builtin-dashdash.sh`.

**Found along the way:** `let -- --2` reaches the arithmetic parser as `--2`,
which osh read as a decrement of a literal. That is a separate bug in
`arith.rs`, fixed in its own commit — bash forms `++`/`--` only where an
lvalue makes an increment possible.

**Reproduce (now matching):**

```sh
eval -- 'echo hi'
# bash: hi        osh (before the fix): --: command not found (rc=127)
```
