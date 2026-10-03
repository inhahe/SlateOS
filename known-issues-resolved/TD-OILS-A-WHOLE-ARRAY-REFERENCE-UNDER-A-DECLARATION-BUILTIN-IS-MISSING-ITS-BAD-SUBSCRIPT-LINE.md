### TD-OILS-A-WHOLE-ARRAY-REFERENCE-UNDER-A-DECLARATION-BUILTIN-IS-MISSING-ITS-BAD-SUBSCRIPT-LINE. `declare -n r='n[@]'; declare r=(x y)` gives one line where bash gives two — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_compounds_scoped`,
the `RefTarget { sub: Some(…) }` arm.

**What.** bash validates the reference's subscript on the way to deciding the
operand names no array, and complains about both:

```text
                                       bash                     osh
n=(a b c); declare -n r='n[@]'
  declare r=(x y)                      `n[@]: bad array         only the second line
                                       subscript`, then
                                       `` `n[@]': not a valid
                                       identifier ``
n=(a b c); declare -n r='n[1]'
  declare r=(x y)                      one line only            the same — agreed
n=(a b c); declare -n r='n[@]'
  r=(x y)                              one line only            the same — agreed
```

Only the *declaration builtin* path gives two — the ordinary assignment gives
one, which is what
TD-OILS-A-COMPOUND-LITERAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-IS-STORED-INSTEAD-OF-REFUSED
made osh match. So the extra line belongs to whatever the builtin does before it
refuses, not to the refusal itself.

Re-measured 2026-08-05, which narrows it further. The extra line comes only from
the operand that has no array flag — `declare -a r=(x y)` and `declare -A r=(…)`
through the same `n[@]` give one line each (the `-A` one being the kind check of
the entry below) — and only where the operand is *refused* at all: inside a
function the same command binds a local named `n[@]` and says nothing. The base
makes no difference: `m[@]` on an associative array and `nosuch[@]` on no array
at all both give it, and `*` reads the same as `@`.

That matches bash's own shape, described in `declare_compounds_scoped` for
allexport: `declare NAME=(…)` without an array flag reaches the variable through
the ordinary scalar bind first, and that is the read which evaluates the
subscript and complains.

**Fixed 2026-08-05,** both halves — the compound one described above and a
*scalar* one this entry had not noticed: the valueless `declare r` through the
same reference gives the line alone, declares nothing, and succeeds (`s=0`),
where osh was silent and created `n`/`nosuch` as an empty array.

The mechanism, read out of bash 5.2.37 (`D:\refsrc\bash-5.2`, see
TOOLING-BASH-5.2.37-SOURCE under Reference Material at the top of this file).
The line is `err_badarraysub` reached from
an ordinary `bind_variable(name, NULL, ASS_FORCE)` — `declare.def:795`, the
branch taken when `declare_internal` has been given *nothing to do*:

```c
  if (var == 0 && (mkglobal || flags_on || flags_off || offset))
    { …reach for the nameref's base, rebuild the name, restart… }
  if (var == 0)
    var = declare_find_variable (name, mkglobal, chklocal);
  …
  if (var == 0)
    var = … bind_variable (name, (char *)NULL, ASS_FORCE);   /* ← complains */
```

So *any* flag at all takes the operand off that path, because the first gate is
on the mere presence of one — `-r`, `+r`, `-t`, `-n`, `+n` and `-g` are all
silent, and each lands on the base. `-I` (inherit) and `--` ask nothing of a
variable and leave it standing; `-G` falls through to `g` and sets `mkglobal`,
so it silences.

The compound operand draws the line in a *different* place, because it does not
reach the builtin as written. `subst.c:expand_declaration_argument` rebuilds the
option string, keeping only: `A`/`a` (from the word's `W_ASSIGNASSOC`/
`W_ASSIGNARRAY`), `g`/`G` (`W_ASSNGLOBAL`/`W_CHKLOCAL` — which `export` and
`readonly` carry standingly, per `execute_cmd.c:fix_assignment_words`), and the
value-transforming letters `i`, `l`, `u`, `c` scanned out of the command's own
option words **in either direction**. Everything else is dropped, and if nothing
accumulates the string is `--`. It then calls `make_internal_declare`, which
strips the `=value` and calls `declare_builtin` directly — so `offset` is 0 and
the same gate decides. That is why `declare -r r=(x y)`, `declare -t`,
`declare -p`, `declare -I` and even `declare +a` still give the line while
`declare -i`, `declare +l`, `export` and `readonly` do not.

(The same `make_internal_declare` call is also the whole explanation of the
tagging rule in
TD-OILS-A-COMPOUND-KIND-REFUSAL-INSIDE-A-FUNCTION-IS-REPORTED-ONCE-AND-ENDS-THE-COMMAND:
bypassing `execute_builtin` leaves `this_command_name` at whatever it was —
empty at top level, the enclosing function's name inside one.)

Inside a function neither route reaches for anything: `declare.def:601`
(`variable_context && mkglobal == 0`) makes the local first, so `var != 0` and
both gates are skipped. osh already bound the spelling there.

Implemented as two guards. In `builtin_declare_scoped`, just after the
both-subscripts refusal: a valueless, subscript-less operand at non-local scope
with no attribute in either direction, no `-g` and no `-p`, whose resolved
target carries `@`/`*`, prints `warn_whole_array_sub` and skips the operand with
the status untouched. In `declare_compounds_scoped`, inside the
`RefTarget { sub: Some(…) }` arm and ahead of the identifier refusal, gated on
`!kind_or_scope_flag && !global_builtin` and none of `int_named`/`seen_lower`/
`seen_upper`/`seen_capcase`. Corpus:
`a-whole-array-reference-under-a-declaration-builtin-complains-about-its-subscript.sh`.
