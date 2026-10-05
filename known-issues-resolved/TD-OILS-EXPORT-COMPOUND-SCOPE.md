### TD-OILS-EXPORT-COMPOUND-SCOPE. `export`/`readonly` of an array literal inside a function made a local — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` —
`exec_declare_with_arrays_scoped`, the `make_local` computation.

**What:** found while measuring the declaration builtins for
TD-OILS-NOASSIGN-VARS. `export` and `readonly` always bind at the
*global* scope, whichever scope they are invoked from; `declare`/`local`
bind locally inside a function unless `-g` is given. The scalar path got
this right by routing through `builtin_export`/`builtin_readonly`, which
never make locals at all, but the compound-literal path bound the array
itself and decided `make_local` from `!self.local_frames.is_empty()`
alone — so an array literal exported from a function vanished with the
frame:

```sh
f() { export q=(1 2); }; f; declare -p q
# bash: declare -ax q=([0]="1" [1]="2")
# osh (before): osh: line 1: declare: q: not found
```

Same for `readonly q=(1 2)`.

**Fixed 2026-07-31:** `make_local` now excludes the two globally-scoped
builtins by *name* (`let global_builtin = cmd == "export" || cmd ==
"readonly"`). It has to be the name and not the attribute, which was
measured too: `declare -x q=(1 2)`, `declare -r q=(1 2)` and `local -x
q=(1 2)` all still make locals in bash, because `export`/`readonly` are
separate entry points there rather than `declare` under a flag. Corpus
case `noassign-vars.sh` extended; unit test
`an_exported_array_literal_declared_in_a_function_is_global`.
