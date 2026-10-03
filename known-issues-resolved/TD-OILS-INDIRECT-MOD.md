### TD-OILS-INDIRECT-MOD. `osh` rejects indirect expansion combined with a modifier (`${!ptr:-def}`, `${!ptr#pat}`, …) — 2026-07-19 — ✅ FIXED 2026-07-19 (parser emits `WordPart::IndirectOp{refname,target}`; `expand_dynamic` resolves the pointer then applies the modifier to the target)

**Where:** `userspace/oils/src/parser.rs` (parameter-expansion parser) and
`userspace/oils/src/interp.rs` `expand_indirect`. The parser only recognises a
bare `${!name}` (indirect) and the name-listing/keys forms (`${!pre@}`,
`${!pre*}`, `${!arr[@]}`); combining `!` indirection with a value operator is
reported as `syntax error: unsupported parameter expansion '${!ptr:-def}'`.

**What:** bash allows the full operator set to apply to the *indirect target*:
`${!ptr:-default}`, `${!ptr:offset:len}`, `${!ptr#pat}`, `${!ptr/a/b}`,
`${!ptr^^}`, etc. — the `!ptr` first resolves to the target name, then the
operator applies to that target's value. osh does not parse these.

**Proper fix:** in the parameter-expansion parser, when a `${` body starts with
`!` followed by a *name* (not a prefix-`@`/`*` or `[@]`/`[*]` listing form),
parse the remainder as an ordinary modifier suffix and record an "indirect"
flag on the resulting `WordPart`. At expansion time, resolve the indirect
target name first (via the same logic as `expand_indirect`, including the
fatal unset-pointer / invalid-name errors), then apply the modifier to the
target's value. Add tests: `ptr=missing; echo ${!ptr:-fb}` -> `fb`;
`x=hello; p=x; echo ${!p:2:3}` -> `llo`; `x=FOO; p=x; echo ${!p,,}` -> `foo`.
