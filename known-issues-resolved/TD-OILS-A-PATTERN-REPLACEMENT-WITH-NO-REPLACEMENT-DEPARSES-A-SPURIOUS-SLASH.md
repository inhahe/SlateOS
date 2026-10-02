### TD-OILS-A-PATTERN-REPLACEMENT-WITH-NO-REPLACEMENT-DEPARSES-A-SPURIOUS-SLASH. `${q/ab}` prints back as `${q/ab/}` — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/unparse.rs` — the `WordPart::ParamReplace` arm
(line ~1477) and the `BulkOp::Replace` arm (line ~1612), both of which print
`b"/"` unconditionally between pattern and replacement.

**What.** bash prints a word back from its saved *source text*, so "no
replacement given" and "empty replacement" print differently even though the two
expand alike. osh reconstructs the text and cannot tell them apart, because
`parse_replace_pieces` (`parser.rs`:6440–6507) folds a missing replacement into
an empty `Word` — its `in_repl` flag, which is exactly the distinction, is
dropped at the return. All five shapes are affected:

```text
f() { echo "${a[@]/ab}" "${q/#ab}" "${q/%ab}" "${q//ab}" "${a[@]//ab}"; }
declare -f f
    bash:  echo "${a[@]/ab}" "${q/#ab}" "${q/%ab}" "${q//ab}" "${a[@]//ab}"
    osh :  echo "${a[@]/ab/}" "${q/#ab/}" "${q/%ab/}" "${q//ab/}" "${a[@]//ab/}"
```

`declare -f` is byte-compared by the corpus harness, so this is a real
divergence wherever such a function is printed back.

bash's own expansion cannot tell the two apart either — `parameter_brace_patsub`
(subst.c:9163) sets `rep = NULL` when no delimiter is found *and* again when
`*rep == '\0'` — which confirms the distinction is the printback's alone.

**Fixed by** making the replacement `Option<Box<Word>>` on both
`WordPart::ParamReplace` and `BulkOp::Replace` — "absent" is what bash's text
records, and an `Option` says so where a `had_slash: bool` beside an empty
`Word` would only hint at it. `parse_replace_pieces` now returns its `in_repl`
flag as that `Option` instead of dropping it; the two `interp.rs` consumers map
`None` to the empty replacement, which is what bash's expansion does; `ast.rs`'s
`first_scanned_arith` walkers switch from `in_word` to `in_opt`; and
`unparse.rs` gained `repl_src`, which keeps the separator **with** the
replacement so no caller can print one the source never had. Found incidentally
while measuring the ANSI-C splice work.

Corpus case:
`userspace/oils/tests/corpus/a-replacement-with-no-separator-is-printed-without-one.sh`
— both spellings of all four operator shapes, the `${a[@]…}` bulk forms, an
empty pattern, a `/` that belongs to a nested construct, and the `declare -f`
printback of each.
