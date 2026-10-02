### TD-OILS-DECL-LOCAL-OUTSIDE-FUNCTION-SKIPS-BINDING. `local q=(1)` outside a function refuses before binding, and the refusal escapes the redirection — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` —
`exec_declare_with_arrays_scoped`, the `is_local && self.local_frames
.is_empty()` guard at the head of the function. It returns before phase 1
has bound the compound literals, and before the stderr push installed for
phases 2 and 3 (see TD-OILS-DECL-DIAGNOSTIC-ESCAPES-REDIRECTION).

bash raises "can only be used in a function" from the *builtin*, which
runs after the words have expanded — so the compound operand binds first,
and the message is redirectable:

```sh
( local q=(1); echo "rc=$?"; declare -p q )
# bash: "local: can only be used in a function" / rc=1 / declare -a q=([0]="1")
# osh:  the same message and rc, but "declare: q: not found"

( local q=(1) 2>/dev/null; echo "rc=$?" )
# bash: rc=1, silent.  osh: prints the message.
```

Reproduce with `target/dvscratch/px82.sh` (the `local at top level`
sections are the whole diff).

**Fix.** The guard moved down to phase 2, immediately under the stderr
push, so phase 1 runs first and the message is redirectable. `make_local`
stopped being unconditionally true for `local` at the same time —
`!self.local_frames.is_empty() && (is_local || (!global &&
!global_builtin))` — because at top level bash takes the plain
`apply_assignment` readonly path (one untagged message, then the parse
unit is discarded) rather than the doubly-reported local-shadow one.
Pinned by `userspace/oils/tests/corpus/local-outside-function.sh`.
