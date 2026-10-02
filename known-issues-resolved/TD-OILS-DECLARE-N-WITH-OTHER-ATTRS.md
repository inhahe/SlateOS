### TD-OILS-DECLARE-N-WITH-OTHER-ATTRS. `declare -n` combined with another attribute ignores the other attribute — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — the `builtin_declare_scoped` operand
loop, and the new `Shell::nameref_array_error`.

**What:** bash resolves the combination before the nameref is made; osh recorded
both flags and applied neither. Measured against bash 5.2.37 (`declare -nX v=t`
followed by `declare -p v`):

| flags | bash | osh (before) |
|---|---|---|
| `-ni` | rc **1**, `v` never created | rc 0, `declare -in v="t"` |
| `-nu` | `declare -nu v="T"` (the *target name* is folded) | `declare -nu v="t"` |
| `-nc` | `declare -nc v="Tq"` | `declare -nc v="tq"` |
| `-na` | `declare -a v=([0]="t")` — an ordinary array, `-n` dropped | `declare -an v` |
| `-nA` | `declare -A v=([0]="t" )` | `declare -An v` |
| `declare -i q; declare -n q=t` | `declare -n q="t"` — `-n` clears the value attributes | `declare -in q="t"` |
| `declare -a q; declare -n q=t` | `q: reference variable cannot be an array`, rc 1 | accepted |
| `-nl`, `-nr`, `-nx`, `-nt` | match | ✅ |

The `-nu`/`-nc` case mattered most: `declare -nu v=t` in bash points `v` at `T`,
so a write through `v` landed somewhere else entirely than it did in osh.

**Fixed in `7ad30befd`.** `-n` clears the name's prior `-i`/`-u`/`-l`/`-c` (but
not `-x`/`-t`/`-r`); the stored reference name is folded by this command's case
attributes; `-i` beside `-n` keeps the attributes it can, declines to make the
reference and refuses the assignment silently (still running the arithmetic, so
a malformed target name is the ordinary bad-`-i` failure); `-a`/`-A` beside `-n`
takes the name for its array and drops the reference attribute; and a name that
is already an array refuses to become a reference, ahead of the readonly
refusal. Covered by the lib test
`a_nameref_declaration_settles_the_attributes_named_beside_it` and the corpus
case `a-a-nameref-declaration-settles-the-attributes-named-beside-it.sh`.
