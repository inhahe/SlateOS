### TD-OILS-EXPORT-READONLY-ATTR. A refused `export NAME=value` dropped the export attribute along with the value — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the readonly branch of
`builtin_export`, the readonly branch of `builtin_readonly`, and the new
`Shell::attr_tag`.

`export` and `readonly` are attribute-setters *and* assignments, and
against a readonly target the two halves fail independently: bash marks
the name first and only then tries the store, so the value is refused
(status 1, `NAME: readonly variable`) while the attribute lands anyway.
osh skipped the operand whole.

```sh
readonly q=1; export q=2; declare -p q
# bash: q: readonly variable / declare -rx q="1"
# osh (before): q: readonly variable / declare -r q="1"

q=1; export q; readonly q; export -n q=2; declare -p q
# bash: declare -r q="1"   (the -n took it off, refusal or no)
# osh (before): declare -rx q="1"
```

A second, smaller divergence in the same two lines: `-a`/`-A` sends bash
through `declare_internal`, which tags its diagnostics with the calling
builtin, so `export -a q=2` says `export: q: readonly variable` where a
bare `export q=2` says `q: readonly variable`. `readonly -a` splits the
same way. No other flag does it — `-n`, `-p` and `--` all keep the bare
form — and osh used the bare form for every one.

`declare -x q=2` is deliberately *not* the same thing: it names itself in
the diagnostic and applies nothing, which osh already had right. So does
the `noassign` refusal (`export GROUPS=5` applies `x` and reports **0** —
see TD-OILS-DECL-NOASSIGN-SCOPE), which is a different code path from
this one.

**Fixed 2026-07-31.** The readonly branch of `builtin_export` now applies
or removes the export attribute before it `continue`s, and both builtins
build their diagnostic through `Shell::attr_tag`, which supplies the
`export: `/`readonly: ` prefix exactly when `-a`/`-A` is present. Corpus
case `export-readonly-attr.sh`; unit test `a_refused_export_still_exports`.
