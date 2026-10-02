### TD-OILS-INDIRECT-ARITH. `osh` indirect-expansion error inside a `(( ))`/arith command is not fatal — 2026-07-19 — ✅ FIXED 2026-07-20 (stale; now fatal, matching bash)

**Status:** FIXED. Verified byte-for-byte against bash 5.2:
`(( ${!nonexist} )); echo after` — osh prints `line 1: nonexist: invalid
indirect expansion`, aborts the arith command, does not run `after`, and exits
1, exactly as bash does. The `${!ptr}` bad-pointer path now sets the fatal
word-expansion error inside arithmetic-command expansion. Regression-guarded by
an arith-command assertion added to `indirect_expansion_bad_pointer_is_fatal`.

**Where (original):** `userspace/oils/src/interp.rs` — `eval_arith_raw` (~line 1198) and

**Where:** `userspace/oils/src/interp.rs` — `eval_arith_raw` (~line 1198) and
its callers (`exec_arith_command`, `builtin_let`, `exec_for_arith`, array
subscript evaluation). The arith-command path expands `$…`/`${…}` via
`expand_arith_params`, which does not route `${!ptr}` through `expand_indirect`,
so a bad pointer inside `(( ${!nonexist} ))` does not set `unbound_error`.

**What:** bash makes an invalid indirect expansion fatal even inside an
arithmetic *command*: `(( ${!nonexist} )); echo after` exits with status 1 and
never prints `after`. osh evaluates the arith command as if the expansion were
empty (0) and continues, printing `after`. (The related `let x=${!nonexist}`
case *is* already fatal because `let`'s argument is word-expanded through the
normal path that reaches `expand_indirect`.) This is a narrow edge — indirect
expansion inside `(( ))` is rare.

**Proper fix:** have `expand_arith_params` resolve `${!ptr}` through the same
indirect-expansion logic as the word expander (so it sets `unbound_error` on a
bad pointer), and — since `unbound_error` is not save/restored around
`expand_arith_params` — the following simple command's driver check will then
abort as bash does. Add test: `(( ${!nonexist} )); echo after` -> `("", 1)`.
