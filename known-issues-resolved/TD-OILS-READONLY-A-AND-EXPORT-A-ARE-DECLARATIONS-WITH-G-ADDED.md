### TD-OILS-READONLY-A-AND-EXPORT-A-ARE-DECLARATIONS-WITH-G-ADDED. `readonly -a g=9` in a function makes a **local** where bash makes a global — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_readonly` and
`Shell::builtin_export`. Both are complete implementations of their own,
reached without ever passing through `Shell::builtin_declare` — which is right
for the ordinary spellings, and wrong for the two that bash hands *to* it.

An `-a`/`-A` operand **with a value** is not run by `set_or_show_attributes` at
all. It is rewritten into a `declare` command, and the rewrite adds `-g`
itself (setattr.def:234):

```c
	      /* Let's try something here.  Turn readonly -a xxx=yyy into
		 declare -ra xxx=yyy and see what that gets us. */
	      if (arrays_only || assoc_only)
		{
		  …
		  /* Add -g to avoid readonly/export creating local variables:
		     only local/declare/typeset create local variables */
		  opti = 0;
		  optw[opti++] = '-';
		  optw[opti++] = 'g';
		  if (attribute & att_readonly)
		    optw[opti++] = 'r';
		  if (attribute & att_exported)
		    optw[opti++] = 'x';
		  if (arrays_only)
		    optw[opti++] = 'a';
		  else
		    optw[opti++] = 'A';
		  …
		  opt = declare_builtin (nlist);
```

So `readonly -a g=9` **is** `declare -gra g=9`, and everything that spelling
does follows — including the whole-array store split fixed in
TD-OILS-A-GLOBAL-ARRAY-DECLARATION-IN-A-FUNCTION-STORES-INTO-WHICHEVER-BINDING-IS-LIVE:
the global is declared an array and left empty, and the element goes to the
live binding.

**Reproduce.**

```sh
f() { local g=5; readonly -a g=9; declare -p g; }; f; echo AFTER; declare -p g
```

| | bash 5.2.37 | osh |
|---|---|---|
| `local g=5; readonly -a g=9` after | `declare -ar g=()` | `declare: g: not found` |
| `local g=5; export -a g=9` after | `declare -ax g=()` | `declare: g: not found` |
| `local g=5; readonly -A g=9` inside | `declare -r g="5"` | `declare -Ar g=([0]="9" )` |
| `local -r g=5; readonly -a g=9` | `st=0`, `declare -ar g=([0]="9")` | `readonly: g: readonly variable`, `st=1` |

The last row is the `ASS_FORCE` of the store the rewrite reaches: a readonly
live binding takes the element anyway and raises nothing, where osh's own
`readonly` refuses it.

Three conditions gate the rewrite, and osh must gate on the same three:

* **a value.** `if (assign)` encloses it — `readonly -a g` with no `=` is an
  ordinary attribute marking and stays local, which osh already matches.
* **`-a` or `-A`.** `readonly g=9` is not rewritten either, and also already
  matches.
* the letters carried over are `attribute`'s, not the command's, and `-n`
  edits `attribute` **asymmetrically**: `if (undo && (attribute &
  att_readonly)) attribute &= ~att_readonly;` (setattr.def:174) drops the `r`
  but leaves the `x`. Measured: `readonly -an g=9` leaves the global `declare
  -a g=()`, while `export -an g=9` leaves it `declare -ax g=()`.

**Fixed** in `272101af3`, exactly as the entry proposed: two new helpers,
`Shell::setattr_declared_name` (answers the two gates and returns the name the
marking should still use) and `Shell::setattr_declare` (builds the `-g[r|x][a|A]`
flag word and calls `Shell::builtin_declare`), wired into the operand loop of
both builtins — per operand, since bash rewrites each one on its own.

Three things the entry did not anticipate, all measured:

* **`-a` beats `-A` here**, the opposite of `declare`. The rewrite tests
  `arrays_only` first, so `readonly -aA g=9` is `declare -gra g=9` — an
  *indexed* array — where `declare -aA g=9` makes an associative one and then
  refuses the `-a` against it. The flag the helper takes is therefore
  `indexed`, not `assoc`.
* **the diagnostics keep the calling builtin's tag.** `declare_builtin` is a
  plain C call, so `this_command_name` is never changed for it: a kind
  conflict raised by the rewritten declaration says `readonly: q: cannot
  convert indexed to associative array`. And the kind it collides with is the
  **global**'s — `-g` looks straight past a local of the other kind, which is
  not in its way at all.
* **a failed rewrite is an assignment error.** `if (opt != EXECUTION_SUCCESS)
  assign_error++;`, and `assign_error` is what returns `EX_BADASSIGN` rather
  than a plain failure — which in posix mode ends a non-interactive shell,
  both builtins being special. So `set -o posix; declare -a q=(1 2);
  readonly -A q=9` exits at the kind conflict. The fix sets
  `BuiltinFailure::Assignment` for it.

Corpus: `tests/corpus/readonly-a-and-export-a-are-declarations-with-g-added.sh`.

One row that belongs to this entry had to stay out of the corpus:
`declare -a arr=(x y); declare -n r=arr[1]; readonly -a r=9` still diverges,
but for a *separate* reason — see
TD-OILS-A-READONLY-DECLARATION-MARKS-BEFORE-IT-STORES-SO-IT-REFUSES-ITS-OWN-ELEMENT.

**How it was found:** measuring
`a-global-array-declaration-stores-into-whichever-binding-is-live.sh`, whose
`readonly -a`/`export -a` rows had to be dropped from the corpus because this
is a second, separate mechanism.
