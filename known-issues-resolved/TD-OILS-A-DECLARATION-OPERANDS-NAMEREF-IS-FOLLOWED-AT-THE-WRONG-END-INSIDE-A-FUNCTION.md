### TD-OILS-A-DECLARATION-OPERANDS-NAMEREF-IS-FOLLOWED-AT-THE-WRONG-END-INSIDE-A-FUNCTION. `declare -r 'r[1]=9'` blames `g` where bash blames `r`, and the other way round — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped` and
`Shell::apply_assignment_inner`. bash follows the operand's nameref at *both*
ends of a declaration, and reports each end under a different name; osh
reported one name for both.

```sh
f() { local -n r=g; declare 'r[1]=9'; }; ( g=(1 2); readonly g; f )
bash: declare: g: readonly variable     # tagged → the resolved target
osh : declare: r: readonly variable

f() { local -n r=g; declare -r 'r[1]=9'; }; ( g=(1 2); f )
bash: r: readonly variable              # untagged → the written reference
osh : g: readonly variable
```

**Fixed** in this commit. The filed root cause — that `makes_array` stops the
nameref follow — did not survive measurement: `declare -r r=5` with *no*
subscript at all differs the same way, so the follow was never the gate. The
real split is which of bash's two refusals each row reaches.

* The **tagged** one is not declare.def:849 at all. Inside a function the
  operand goes through `declare_transform_name` (declare.def:205) and the
  builtin declares the **resolved** name, so the refusal comes from
  `make_local_variable` (variables.c:2683) three hundred lines earlier —
  which is handed `newname`, the resolved target, and which only refuses a
  readonly **global**. A readonly *local* of an outer frame is one bash
  shadows happily, and a name the frame already holds returns early
  (`local_p (old_var) && old_var->context == variable_context`); both of
  those fall back to :849 and the operand as written. That is the
  `shadow_new` condition the fix now tests, not `makes_array`.
* The **untagged** one is the element store's, and bash hands it the raw
  operand: `assign_array_element_internal`'s `char *name;` carries the
  comment `/* only used for error messages */` (arrayfunc.c:394). So the
  store's readonly refusal — and all four spellings of `bad array
  subscript` — quote the reference as written whatever variable the element
  belonged to. `builtin_declare_scoped` now passes that spelling down beside
  the resolved name.

Which of the two subscript sources a plain element write came from decides
the same question, so the spelling is picked by the new [`Shell::elem_blame`]:
a subscript the *writer* wrote arrives inside the word it was written in
(`declare -n r=g; g=(1 2); r[-9]=9` says `r[-9]`), while one the *reference*
supplied was built into a new name first (`t=v; declare -n r=t[-2]; r=w` says
`t[-2]`). `spelled.index.is_some()` is exactly that distinction.

Corpus:
`a-declaration-operands-nameref-is-followed-at-two-ends-at-once.sh`.
Unit test:
`a_declaration_operands_nameref_is_followed_at_two_ends_at_once`.

**How it was found:** the first of the two rows only became reachable once
TD-OILS-A-READONLY-DECLARATION-MARKS-BEFORE-IT-STORES-SO-IT-REFUSES-ITS-OWN-ELEMENT
was fixed; the second was already wrong before it.
