### TD-OILS-AN-INDIRECT-SUBSTRINGS-ARITHMETIC-ERROR-NAMES-THE-TARGET-NOT-THE-REFERENCE. `${!p:b*}` says `sv: …` where bash says `!p: …` — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, the `WordPart::ParamSubstr` arm —
the arithmetic evaluation of the offset/length reports under the *renamed*
target rather than under `Shell::ref_label`, which every other complaint reached
through an indirection uses.

**Reproduce.**

```sh
p=sv
printf '<%s>' ${!p:b*}
# bash: !p: b*: syntax error: operand expected (error token is "*")
# osh : sv: b*: syntax error: operand expected (error token is "*")
```

A referent that is a **whole array** diverged the same way through a second
site: `pn='n[@]'; ${!pn:b*}` is a *slice*, built by `Shell::slice_elements`,
which made its own `name[@]` label — so osh said `n[@]` where bash says `!pn`.

**Fixed** by routing the arithmetic diagnostic's name through the same
`ref_label` the rest of the indirect path already installs, in both places:
the `WordPart::ParamSubstr` arm of `expand_dynamic_with` and
`Shell::slice_elements`. `ref_label` is installed for exactly the length of a
modifier's expansion, so its absence is what keeps the ordinary `${sv:b*}` and
`${n[@]:b*}` naming themselves. Corpus:
`an-indirect-substrings-bounds-are-reported-against-the-reference.sh`; unit test
`an_indirect_substrings_bounds_are_reported_against_the_reference`.
