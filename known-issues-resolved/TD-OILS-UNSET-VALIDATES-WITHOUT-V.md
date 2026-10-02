### TD-OILS-UNSET-VALIDATES-WITHOUT-V. `unset` never checked its operand was a name, so `unset -v 1x` succeeded silently — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_unset`.

**What:** bash validates the operand as a variable name **only when a `-v` says
the operand names one**. Plain `unset X` falls back to the function namespace
and `unset -f X` names a function, and a function may be called anything, so
neither checks. Add a `-v` anywhere in the options and a word that is neither an
identifier nor an element reference is
`` unset: `WORD': not a valid identifier `` with status 1:

```sh
unset -v 1x      # bash: unset: `1x': not a valid identifier, rc 1; osh: rc 0
unset -v ""      # same
unset -v -       # same
unset -v 'n[]'   # same
unset 1x         # no complaint from either — the function namespace has no rule
```

**Fixed 2026-08-04.** The gate accepts exactly what bash accepts — a plain
identifier or a `name[sub]` element reference, since that is how one element is
unset — reports the operand quoted whole, and does not stop the operands after
it. Pinned by the corpus case `unset-v-validates-the-name.sh`.
