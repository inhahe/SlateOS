### TD-OILS-INDIRECT-WHOLE-SET-NOUNSET. A quoted `"${!r}"` whose target names an empty `*`-style whole set is an unbound error in bash, not an empty string — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `indirect_ref_part` /
`indirect_target_value`, the whole-array referent path, and the nounset check
in `expand_indirect`.

**What.** Found while fixing TD-OILS-INDIRECT-BANG-UNWIND. Under `set -u`, a
*quoted* indirect reference whose target names a whole set spelled with `*` —
`*` itself, or `name[*]` — and whose set has **no elements** is an unbound
error in bash. osh expands it to the empty string.

```text
$ set -u; set --;            r='*';      echo "[${!r}]"   bash: !r: unbound variable   osh: []
$ set -u; declare -a a;      r='a[*]';   echo "[${!r}]"   bash: !r: unbound variable   osh: []
$ set -u; declare -A m;      r='m[*]';   echo "[${!r}]"   bash: !r: unbound variable   osh: []
$ set -u; unset a;           r='a[*]';   echo "[${!r}]"   bash: !r: unbound variable   osh: []
$ set -u; IFS=; set --;      r='*';      echo "[${!r}]"   bash: !r: unbound variable   osh: []
```

Everything around it agrees, and the boundaries are sharp:

* **unquoted** `${!r}` is exempt in both, for every one of the cases above;
* an `@`-spelled target (`@`, `name[@]`) is exempt in both, quoted or not;
* it is the *element count* that decides, not the joined text — `a=("")` and
  `set -- ""` are "set" in both even though they join to nothing, while
  `declare -A m` with no keys is unset;
* a **direct** whole-array reference is exempt in both however it is quoted
  (`"${a[*]}"` on an empty `a` is silent), so this is specific to reaching one
  through an indirection.

**Why bash does it.** The outer nounset check
(`subst.c:9928`) fires when the target expanded to nothing and
`all_element_arrayref == 0`. For a *direct* array reference bash sets
`all_element_arrayref = 1` unconditionally for both `[@]` and `[*]`
(`subst.c:9883`). For an *indirect* one it instead takes the flag from
`chk_atstar`'s `contains_dollar_at` (`subst.c:9840`), and `chk_atstar` only
sets that for a `*`-spelled set **when the reference is unquoted**
(`subst.c` `chk_atstar`, the `name[0] == '*' && quoted == 0` arm) — because a
quoted `"$*"` is one word and so does not "contain `$@`". The `@`-spelled arms
have no such condition, which is why they are exempt either way.

**Two further halves, measured after the entry was first written.**

*The operators that raise it.* The gate at `subst.c:9926-9928` runs only for
`want_substring || want_patsub || want_casemod || c == '@' || c == '#' ||
c == '%' || c == RBRACE`. So `${!r:1:2}`, `${!r/x/y}`, `${!r^^}`, `${!r@Q}`,
`${!r#x}`, `${!r%x}` and the bare `${!r}` all raise, while the whole
default-value family — `${!r-D}`, `${!r:-D}`, `${!r=D}`, `${!r+S}`, `${!r:+S}`,
`${!r?}` — is simply absent from that list and is therefore exempt.

*A context that never splits counts as quoted — but only for the bare star.*
`chk_atstar`'s `*` arm requires `quoted == 0 && expand_no_split_dollar_star == 0`,
whereas its `name[*]` arm consults `quoted` alone. So in a context that takes
the word whole without quoting it — an assignment's value `v=${!r}`, an array
element store `a[0]=${!r}`, a `[[ ]]` word, a `case` pattern, a here-string, the
operand of a `${x:-…}` — a bare `r='*'` raises and a `r='a[*]'` stays exempt.
That is bash being inconsistent between its two `*` spellings, and it is
observable, so osh reproduces it deliberately. A here-**document** body and an
arithmetic string are *quoted* rather than merely unsplit, so both spellings
raise in them; a redirection target and the assignment-shaped arguments of
`declare`/`export`/`local`/`readonly` are ordinary split words and stay exempt.

**Fixed** by two predicates in `interp.rs`. `Shell::whole_star_set_unbound`
holds the rule: given the referent's base name (`None` for the bare `*`), it
asks whether the set is empty *by element count* and whether the context is one
that cannot come apart — `self.dquote` for a `name[*]`, and additionally
`self.no_split_star || self.cond_word` for a bare `*` — and if so raises
`!<ref>`, named after the reference rather than the target.
`Shell::indirect_whole_set_unbound` is the thin wrapper for callers that hold a
resolved target *string*: it recognises `*` and `name[*]` and delegates. Four
call sites use them — `expand_indirect`, `indirect_ref_part`,
`indirect_op_part`, and the `WordPart::IndirectOp` arm — the last two passing an
`op_exempt` flag so the default-value family is skipped.

`Shell::no_split_star` is a new field mirroring bash's
`expand_no_split_dollar_star`, with exactly one reader (the predicate above) so
it cannot regress anything else. It is set in `expand_assignment_value` and
`expand_operand_fields`, and in the new `Shell::expand_here_string` — which also
folded together the two duplicated `RedirectOp::HereStr` bodies (the
`exec`/persistent-redirect path and the command redirection-plan path) that had
drifted apart. `command_sub_body` clears it alongside `dquote` and `cond_word`.

**Corpus:** `an-empty-star-set-reached-through-a-reference-is-unset-where-it-cannot-come-apart.sh`,
107 lines, byte-identical to bash 5.2.37.
