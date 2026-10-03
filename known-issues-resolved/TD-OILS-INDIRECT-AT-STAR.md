### TD-OILS-INDIRECT-AT-STAR. `${!@}` / `${!*}` list variable names instead of indirecting through `$@`/`$*` — RESOLVED 2026-07-20

**Resolved 2026-07-20** exactly per the proper-fix plan below. (1) In
`parser.rs` `parse_braced_param`, the `${!prefix@}`/`${!prefix*}` name-listing
branches now require `!prefix.is_empty()`, so a bare `${!@}`/`${!*}` falls
through. (2) `is_indirect_referent` now accepts `@`/`*`, so they parse as
`WordPart::Indirect("@"/"*")`. (3) `expand_indirect` short-circuits a `@`/`*`
referent with an **empty** positional list to `String::new()` (status 0) before
the target validator, while a non-empty `$@`/`$*` joins to a single name that
routes through `is_valid_indirect_target` — so `set -- a b c; echo "${!@}"`
yields bash's "a b c: invalid variable name" (exit 1) and `foo=1; echo "${!@}"`
yields empty (exit 0). A single positional naming a set variable indirects one
level (`V=hi; set -- V; echo "${!@}"` → `hi`). Verified against bash across all
four cases plus the still-working prefixed listing form (`${!aa@}`). Regression
test `indirect_at_star_positional`; 683 tests, clippy clean, both targets build.

<details><summary>Original entry (for history)</summary>


**Where:** `userspace/oils/src/parser.rs` — `parse_braced_param`, the `${!…}`
branch (~1308). The empty-prefix cases `${!*}` and `${!@}` are caught by the
`strip_suffix('*')` / `strip_suffix('@')` name-listing logic (`WordPart::VarNames`
with `prefix == ""`).

**Symptom:** bash treats `${!@}` and `${!*}` (empty prefix) as *indirect
expansion through the positional list* `$@` / `$*`, not as the "list all variable
names" form. Only a **non-empty** prefix (`${!PATH@}`, `${!BASH_@}`) triggers
name-listing.

```
set -- a b c; echo "${!@}"    →  bash: "a b c: invalid variable name" (exit 1)
                                  osh:  ALLUSERSPROFILE APPDATA … (every var name)
foo=1; echo "${!@}"           →  bash: (empty, exit 0)   [no positionals → nothing]
                                  osh:  … foo (every var name)
```

So bash resolves `${!@}` = `${!<value-of-$@>}`: with `set -- a b c`, `$@`
expands to `a b c`, which as a single indirect target name is invalid; with no
positionals, `$@` expands to nothing, so the whole thing is empty (no error).
osh instead lists every set variable name.

**Why deferred:** the fix is more than flipping the prefix guard. It needs (1) the
listing branch to require a **non-empty** valid prefix so `${!@}`/`${!*}` fall
through, (2) `@`/`*` added to the accepted indirect referents (they resolve via
`param_value("@"/"*")`), and (3) a special case so an **empty** `$@`/`$*` (no
positionals) yields empty *without* the "invalid variable name" fatal that a
non-empty-but-malformed target produces — bash distinguishes "nothing to
indirect" from "indirect through a bad name". That empty-positionals rule is the
subtle part and is easy to get wrong, so it was split from the `${!#}`/`${!N}`
special/positional-referent fix (which is done). Genuinely obscure form.

**Proper fix:** in `parse_braced_param`, gate the `${!prefix@}`/`${!prefix*}`
listing on `!prefix.is_empty()`; extend `is_indirect_referent` to accept `@`/`*`;
and in `expand_indirect` treat a `@`/`*` referent whose positional list is empty
as an empty (non-fatal) result rather than routing an empty string through
`is_valid_indirect_target`.
</details>
