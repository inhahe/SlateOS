### TD-OILS-THE-DECLARATION-BUILTINS-GLOBAL-FLAG-COULD-BE-TAKEN-BACK-AND-HAD-NO-TWIN. `declare -g +g`, `declare -G` and `local -g` all landed in the wrong scope — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_global_flag`,
`Shell::enter_global_scope`, `Shell::builtin_declare`,
`Shell::in_declare_global_scope`, and the `make_local` of both
`Shell::builtin_declare_scoped` and `Shell::declare_compounds_scoped`.

**What.** Found while probing the flag gate for
TD-OILS-A-WHOLE-ARRAY-REFERENCE-UNDER-A-DECLARATION-BUILTIN-IS-MISSING-ITS-BAD-SUBSCRIPT-LINE.
Three separate divergences, all about which binding a declaration writes:

```text
                                             bash              osh (before)
f(){ declare -g +g w=1; }; f
  declare -p w                               w="1"             w: not found
declare -G x=1                               declare -- x="1"  -G: invalid option
y=glob; f(){ local -g y=in; }; f
  declare -p y                               y="in"            y="glob"
```

1. **`+g` does not cancel `-g`.** bash's option loop (`declare.def`) sets
   `mkglobal` in the `-` direction only — `case 'g': if (flags == &flags_on)
   mkglobal = 1;` — and never clears it. osh had `b'g' => global = enable`,
   last-one-wins.
2. **`-G` was unimplemented.** It is undocumented (the usage synopsis omits it,
   which is why osh's usage line was already right) but real:
   `case 'G': if (flags == &flags_on) chklocal = 1; /*FALLTHROUGH*/ case 'g':`.
   So `-G` is `-g` plus bash's `chklocal`, which `declare_find_variable` reads
   as "write a local of *this very frame* if there is one, otherwise the
   global". A local of an *enclosing* frame is stepped over exactly as `-g`
   steps over it, so the two letters differ only where the declaration and the
   local are in the same call.
3. **`local` honours both.** bash's `local` is `declare_internal(list, 1)` — the
   same option loop behind one extra "am I in a function?" check — so
   `local -g y=1` writes the global and leaves any local alone. osh treated
   `local` as never-global.

A compound literal is not moved by `-G`, only by `-g`: `fix_assignment_words`
decides where a literal binds from `strpbrk(word+1, "Aag")`, and `G` is not in
that set, so `f(){ declare -G n=(1 2); }` leaves a local `n` behind while
`declare -g n=(1 2)` leaves a global one. The builtin's attribute pass that
follows still goes global either way.

**Fixed 2026-08-06.** `declare_global_flag` returns `(global, chklocal)`,
ignores the `+` direction entirely, and reads `G` as well as `g`;
`enter_global_scope` takes `chklocal` and skips names the innermost frame
already holds; the two `make_local` expressions lost their `is_local` override.
`declare_compounds_scoped` deliberately does *not* read `G`. Corpus:
`the-declaration-builtins-global-flag-is-never-taken-back-and-has-an-undocumented-twin.sh`.
