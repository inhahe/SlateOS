### TD-OILS-A-GLOBAL-SUBSCRIPTED-DECLARATION-IN-A-FUNCTION-ASSIGNS-TO-WHICHEVER-BINDING-IS-LIVE. `declare -g g[1]=9` over a local `g` declared the global an array *and* stored in it, where bash splits the two — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs` — a new `Shell::declare_global_swap`
field, a new `Shell::with_live_binding`, and the subscripted store in
`Shell::builtin_declare_scoped`.

**What it was.** osh wrapped the *whole* operand in the `-g` scope swap — the
array conversion and the element store together — so both landed on the global.
bash reaches **two variables** with the one command.

**The measurement that explains it.** `-g` takes the operand off the local
branch (`variable_context && mkglobal == 0` is false, declare.def:601), so the
variable the declaration is *about* is `find_global_variable (name)`. The
element store is not about that variable at all:

```c
      else if (simple_array_assign && subscript_start)
	{
	  …
	  local_aflags = aflags & ASS_APPEND;
	  …
	  var = assign_array_element (name, value, local_aflags, …);   /* :960 */
```

and `assign_array_element` begins `entry = find_variable (vname)`
(arrayfunc.c:363) — an ordinary lookup, which finds the frame's local.

```sh
f() { local g=5; declare -g g[1]=9; declare -p g; }; f; echo AFTER; declare -p g
#  bash: declare -a g=([0]="5" [1]="9")  /  declare -a g=()
#  osh : declare -- g="5"                /  declare -a g=([1]="9")
```

Four things were measured about the split, and all four now hold:

* **The global keeps the declaration and stays empty**, carrying every letter:
  `declare -g -i g[1]=4+4` leaves `declare -ai g=()` global and the *raw* `4+4`
  local, since only `ASS_APPEND` is carried over (declare.def:957) and the fold
  is by the live variable's own attributes.
* **It is left visible.** declare.def:802 re-hides a variable it just made only
  `if (offset == 0)`, and the operand has a value — hence `=()` and not the bare
  form. A global that already existed keeps what it had (`declare -A g` stays
  bare) and one that was a scalar is converted but never stored into
  (`g=7` → `declare -a g=([0]="7")`).
* **The two halves need not agree on the kind.** `declare -g -A g[k]=9` over a
  local scalar makes the *global* associative and widens the *local* to an
  **indexed** array, where `k` arith-evaluates to 0. Conversely `local -A g;
  declare -g g[k]=9` stores the key `k` locally and leaves an indexed global.
* **The store follows a live nameref**, which the declaration does not:
  `local -n g=w; local w=1; declare -g g[1]=9` fills `w` and still makes a
  global `g`.

**Found with it — a readonly store fails nothing.** `bind_array_variable`
(arrayfunc.c:280) and `bind_assoc_variable` (:311) `err_readonly` and then
`return (entry)` — **non-NULL** — so declare.def:962's `if (var == 0)` never
bumps `assign_error`. Only a bad subscript returns NULL. So
`f() { local -r g=5; declare -g g[1]=9; }` prints `g: readonly variable` and
exits **0**, where the unsplit `f() { local -r g=5; declare g[1]=9; }` exits 1 —
that one is refused by declare.def:849, against the very variable it is about,
and never reaches the store.

**The fix.** `enter_global_scope`'s displaced bindings are now parked on the
shell as `declare_global_swap` for the length of the builtin, and
`with_live_binding` undoes the swap for one name around the element store —
osh's one-table equivalent of bash simply looking the name up again. The
visibility insert moved out of the store's failure branch, since the global is
visible whether the store failed *or* went somewhere else entirely; and the
store's readonly refusal no longer sets the status.

**Not this, and unchanged.** An operand with no subscript (`declare -g g=9`),
one whose name nothing shadows, and `-G`/`export`/`readonly`, which skip the
swap when this very frame holds the name and so reach one variable either way.

**Corpus:** `a-global-subscripted-declaration-reaches-two-variables-at-once.sh`.
Unit test: `a_global_subscripted_declaration_reaches_two_variables_at_once`.
