### TD-OILS-THE-FRAMES-OWN-BINDING-WAS-ASKED-ABOUT-BEFORE-THE-RESTART-RATHER-THAN-AFTER-IT. `readonly g=(1 2)` over a frame-local `g` left nothing at the global the chain died on — 2026-08-12 — FIXED 2026-08-12

**Where:** `userspace/oils/src/interp.rs`, [`Shell::enter_global_scope`] and the
new [`Shell::make_empty_global`]. The `chklocal` question — is there a binding
of this name in *this* frame? — was put once, up front, about the name as
written, and a `true` answer cancelled the whole swap. bash puts it in two
different places with two different names.

The builtin half puts it **last** (declare.def:774):

```c
if (var == 0)
  var = declare_find_variable (name, mkglobal, chklocal);
```

By then `name` may not be the name written: three lines above, the global-only
walk `find_global_variable_last_nameref` has already run, and where it answered
a reference to a name no global answers to, the command restarts on that name
(`goto restart_new_var_name`, declare.def:768) — so the frame's binding is asked
about the name the restart *arrived at*, and the restart itself never saw it.
The compound literal asks about the name as written
(`do_compound_assignment`, subst.c:3494).

Where the two answers part, the declaration leaves **two** variables behind.

**Reproduce.**

```sh
( declare -n g=z                                 # a global chain dying on `z`
  f() { local g=5; readonly -a g=(1 2); }; f
  declare -p g z )
# bash: declare -n g="z"  /  declare -a z        — empty, made by step 1 alone
# osh (before): z unset
```

**The fix.** `enter_global_scope` now maps every operand *before* asking
`chklocal`, and where the frame's binding stops the swap it checks whether the
map moved the name anyway. If it did — and nothing in the frame answers to the
name it moved to — step 1's creation is performed on its own:
`make_empty_global` binds the name globally and unset, in whatever shape the
kind letter chose. Only the shape, since step 1's option string is spelled out
of the word flags and those name a scope and a shape and nothing else
(subst.c:12662-12745) — the `-r` of a `readonly` stays with the builtin proper,
so the array left behind is writable. `DeclScopeFlags` gained `assoc` to carry
which of the two letters won (`att_assoc` is tested first, declare.def:786).

**Tests.** Unit test
`the_restart_happens_before_chklocal_so_step_one_outlives_the_frames_own_binding`
and the corpus case of the same name (8 sections). The 240-shape probe matrix
(`/tmp/kind_matrix.sh`) went 214 → 224; the remainder is the two Active entries
above plus the `declare -Ga` family.
