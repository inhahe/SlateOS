### TD-OILS-A-READONLY-LOCAL-IS-NOT-WIDENED-BY-THE-DECLARATION-THAT-IS-ABOUT-TO-BE-REFUSED. `f() { local -r g=5; declare g[1]=9; }` leaves `declare -r g="5"` where bash leaves `declare -ar g=()` — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped` —
`drop_local_scalar` (the local array widening) sits *below* the readonly
refusals, which `continue` before it is ever reached.

bash puts the two in the other order. The local branch widens at
declare.def:614, long before the assign-to-readonly refusal at :849:

```c
  if (variable_context && mkglobal == 0)		/* :601 */
    {
      …
      var = making_array_special || (flags_on & (att_array|att_assoc))
	      ? make_local_array_variable (newname, …)		/* :614 */
	      : make_local_variable (…);
```

`making_array_special` is set by a subscripted operand, so a subscript asks for
the widening just as plainly as `-a` does. The refusal that follows then
`NEXT_VARIABLE ()`s — with the array already made, and empty, the readonly
scalar it replaced gone.

**Reproduce.** Six spellings, all measured, all differing the same way:

```sh
f() { local -r g=5; declare g[1]=9;  declare -p g; }; f  # bash declare -ar g=()
f() { local -r g=5; local   g[1]=9;  declare -p g; }; f  # bash declare -ar g=()
f() { local -r g=5; local -a g=5;    declare -p g; }; f  # bash declare -ar g=()
f() { local -r g=5; local -A g=5;    declare -p g; }; f  # bash declare -Ar g=()
f() { local -r g=5; declare -i g[1]=9; declare -p g; }; f  # bash declare -ar g=()
f() { local -r g=5; declare g[1]=9 h=2; declare -p g h; }; f
#   bash declare -ar g=()  /  declare -- h="2"
```

osh answers `declare -r g="5"` for every one. The status (1) and the
diagnostic (`local: g: readonly variable` / `declare: g: …`) already match; only
the shape of the variable left behind is wrong.

**Not this, and already right:** a *valueless* subscripted operand
(`declare g[1]`), a readonly local that is already an array
(`local -r g=(1)`) or already associative (`local -Ar g=([k]=1)`), a readonly
**global** with no local of the name (`declare -r g=5; f() { declare g[1]=9; }`
— there is no local branch to widen), and the plain `local g=9`.

**Fixed** in `f10dae6d8`: the widening now runs immediately above the three
readonly refusals, gated on the declaration being the one that *found* the
frame's binding (`held_here`) and on that refusal actually being about to
fire. It is deliberately not the whole of `drop_local_scalar` hoisted: osh's
widening below the refusals is where every operand that gets that far still
meets it, and moving it wholesale would drag the `was_assoc` and `name_existed`
reads — both of which mean "when the command arrived" — past it.

Three things the entry above did not anticipate, all measured:

* **only the kind survives.** `VSETATTR (var, flags_on)` is declare.def:944,
  past the refusal, so the declaration's other letters are lost along with the
  value: `declare -ax g=9` leaves an *unexported* `declare -ar g=()` and
  `declare -al g=AB` leaves it unfolded.
* **`-A` outranks `-a` here too**, because :610 tests `att_assoc` first —
  `local -r g=5; declare -aA g=5` leaves `declare -Ar g=()`, and the `-a`'s own
  self-conflict refusal never happens, the readonly one having come first.
* the widened name counts as **valued**, which is what prints the empty `=()`
  rather than a bare `declare -ar g`; a name that was already an array keeps
  its elements (`local -r g=(1); declare g[1]=9` → `declare -ar g=([0]="1")`),
  since there was nothing to widen.

Corpus: `tests/corpus/a-readonly-local-is-widened-by-the-declaration-that-is-about-to-be-refused.sh`.

**How it was found:** the readonly probes taken while landing
TD-OILS-A-GLOBAL-SUBSCRIPTED-DECLARATION-IN-A-FUNCTION-ASSIGNS-TO-WHICHEVER-BINDING-IS-LIVE,
which needed to know which of the two bindings a readonly refusal was about.
