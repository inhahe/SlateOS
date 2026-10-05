### TD-OILS-DECLARE-BADID. `osh` `declare NAME[a b]=v` silently no-ops; bash errors "not a valid identifier" — RESOLVED 2026-07-20

**Resolved 2026-07-20** — and the fix uncovered/closed a larger adjacent gap:
`declare "NAME[sub]=value"` (a *quoted*, single-arg subscripted target) was not
handled at all — osh stored a scalar literally named `NAME[sub]` and `declare -p
NAME` reported "not found". `builtin_declare` now:
- splits each target into a base name + optional `[subscript]`;
- validates the **base** as an identifier, emitting `{tag}: \`ARG': not a valid
  identifier` (status 1, quoting the original arg) for `bad@name=v`, `1x=v`, or
  an unbalanced `h[a` — this also fixes the original non-subscript case;
- auto-creates an **indexed** array for a subscripted name (never clobbering an
  existing associative array), matching bash's `declare "x[5]"` → `declare -a x`;
- routes the element assignment through the normal array machinery via a directly
  constructed `Assignment` AST (NOT by re-parsing a `base[sub]=value` string,
  which would word-split an unquoted-space value like `x[0]="2 x"`), so the
  subscript is arith-evaluated for indexed arrays / literal for associative, and
  a bad `-i` **value** stays fatal.

Faithful error tagging: a bad **subscript** is reported untagged (`a b: syntax
error in expression`, like a command-position `a[x y]=v`, even under `-i`) by
clearing `arith_cmd` only while resolving the subscript, whereas a bad `-i`
**value** keeps the `declare:` tag. Verified against bash across `x[5]=v`,
`x[2+3]=v`, `-i x[0]=2+3`, `-i a[0]="2 x"` (fatal, tagged), `x[a b]=v` (fatal,
untagged), `-A m[k]=v`, `x[5]` (empty array), `x[3]+=b`, `bad@name=v`, `1x=v`.
Regression test `declare_subscripted_target_and_bad_identifier`; 684 tests,
clippy clean, host + slateos build green. `is_valid_name` was made
`pub(crate)` in `parser.rs`.

<details><summary>Original entry (for history)</summary>

**Where:** `userspace/oils/src/interp.rs` `builtin_declare` argument
parsing (the per-arg assignment/attribute handling).

**What:** In *argument* position (after the `declare` command word), bash's
tokenizer splits `declare h[a b]=v` into `h[a` and `b]=v` and `declare`
then rejects each: `declare: `h[a': not a valid identifier` (status 1). osh
splits the same way but its `declare` builtin silently ignores the
malformed args (no error, exit 0). Reproduce: `declare -A h; declare
h[a b]=v; echo $?` → bash prints two errors + status 1; osh prints nothing
+ status 0.

**Why deferred:** niche (unquoted spaces in a `declare` argument subscript);
the correct incantation `declare "h[a b]=v"` works in both shells.

**Proper fix:** in `builtin_declare`, when an argument is neither a valid
attribute flag nor a well-formed `name[sub]?=value` assignment, emit
`osh: declare: \`ARG': not a valid identifier` to stderr and set the exit
status to 1 (accumulating the worst status across args).
</details>
