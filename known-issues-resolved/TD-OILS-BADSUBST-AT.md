### TD-OILS-BADSUBST-AT. Invalid `@` transform operator — set-vs-unset "bad substitution" — 2026-07-19 — ✅ RESOLVED 2026-07-20

**What (original bug):** the true bash rule for an *invalid* `${name@OP}`
transform operator — an EMPTY (`@`), UNKNOWN (`@Z`), or MULTI-CHAR (`@QU`) one —
is a **set-vs-unset** split, not a quoted-vs-unquoted one as this entry
previously claimed: bash yields an empty field (status 0) when `name` is
**unset**, but a fatal `${…}: bad substitution` (status 1) when it is **set**.
The same rule generalises to every reference form — scalar (`${x@Z}`), single
element (`${a[0]@Z}`), whole-array (`${a[@]@Z}`), and positional (`${@@Z}`) —
where "set" for the bulk forms means the array/positional list has ≥1 element
(an empty array/`set --` yields empty with no error). osh previously always
errored (matching only bash's *set* case) and, worse, silently returned the
value unchanged for an *unknown* single-char op like `@Z`.

**Also fixed:** osh had wrongly accepted `@l` as a valid transform. Bash has no
lowercase-first operator; its valid set (5.2) is exactly `Q E P A a K k U u L`.
`${x@l}` on a set variable is a "bad substitution" — now matched.

**Fix:** added `WordPart::BadTransform { name, index, raw }` and
`BulkOp::BadTransform { raw }` (ast.rs). The parser routes any invalid `@`
operator to these, carrying the raw inner source. At expansion the scalar/
element arm checks `param_elem_value(name, index).is_some()` (set → error, unset
→ empty); the bulk arm in `bulk_elements` checks the element/positional count
(≥1 → error, else empty). Both call `Shell::bad_substitution(raw)`. Valid ops
gated by `is_valid_transform_op` in parser.rs (no `l`). Verified against
`bash 5.2.37` across scalar/element/bulk/positional forms; regression test
`param_transform_invalid_op_bad_substitution` in interp.rs.

**Remaining (separate, cosmetic, out of scope):** when the bad expansion appears
inside a larger word (e.g. `echo "[${x@Z}]"`), bash's diagnostic quotes the
whole word — `[${x@Z}]: bad substitution` — whereas osh quotes only the failing
`${…}` — `${x@Z}: bad substitution`. osh's form is arguably cleaner and it
affects *all* osh bad-substitution diagnostics uniformly (e.g. `${x!}` shows the
same), so it is a shared cosmetic divergence tracked separately rather than
here.
