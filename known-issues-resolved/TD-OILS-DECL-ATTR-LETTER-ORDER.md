### TD-OILS-DECL-ATTR-LETTER-ORDER. `declare -p` printed the case-folding letters before `r`/`x` — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `attr_flag_letters` (~8409) and
`declare_attr_flags` (~18994).

**Was.** bash reports a variable's attribute letters in the order of its own
internal attribute table, not the order the flags were written — and the
case-folding trio `l`/`u`/`c` sits *after* `r`/`x`, not before. osh had `c` in the
right place but `l`/`u` in the wrong one, in both of the two hand-written copies of
the order:

```sh
declare -alrx v=A; declare -p v   # bash: declare -arxl   osh: declare -alrx
declare -lx s=q;   declare -p s   # bash: declare -xl     osh: declare -lx
```

`${v@a}` and `${v@A}` share the order in bash and so had the same skew.

**Fix.** One builder — `attr_flag_letters` — in the measured order (kind, `n`, `i`,
`r`, `x`, then `l`/`u`/`c`), with `declare_attr_flags` reduced to wrapping it in
`-`/`--`. Its redundant `kind` parameter went away too: every caller passed exactly
what the builder derives from `self.assoc`/`self.arrays` itself. Having written the
order out twice is what let it drift, and an earlier fix had already had to patch
both copies for `c`. Covered by `param_transform_escape_and_attrs` and the `ord*`
block of `tests/corpus/declare-attrs.sh`.

Not covered, and still open: bash's `t` (trace) attribute. osh accepts `declare -t`
and silently drops it, so it never appears in the letters
(`declare -tirx v=1` reports `-irtx` in bash, `-irx` in osh) and function tracing
is not implemented at all. See TD-OILS-DECL-TRACE-ATTR.
