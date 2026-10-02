### TD-OILS-DECL-P-IGNORED-ON-THE-FUNCTION-SIDE. `-p` alongside `-f`/`-F` was read only on the variable side, so it neither suppressed the marking routes nor changed what a named listing printed — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_functions` and the
`"declare" | "typeset"` dispatch arm that routes to it.

**What:** osh looked for the listing letter only when it had already decided the
command was about variables. On the function side the `p` was dropped, so three
separate behaviours were missed at once:

```sh
g() { :; }; h() { echo h; }; declare -ft h; export -f h
declare -Fp g      # bash: declare -f g          osh: g
declare -fp h      # bash: body, then declare -ftx h   osh: body only
declare -Fp nope   # bash: declare: nope: not found rc 1   osh: silent rc 1
declare -ftp g     # bash: prints `g`, marks nothing   osh: marked g traced
```

**Measured rule** (bash 5.2.37), all now pinned by the corpus case
`p-makes-a-function-listing-print-the-attribute-line.sh` and the unit test
`p_makes_a_function_listing_print_the_attribute_line`:

- `-p` runs **ahead of the routing**: `declare -frp g`, `-fxp g`, `-ftp g`,
  `-Frp g`, `-Ftp g`, `-Fxp g` are all listings that mark nothing. Without the
  `p`, `declare -Ft g` / `-Fx g` do mark.
- `declare -F g` is the bare name; `declare -Fp g` is instead the attribute line
  the *nameless* `-F` listing prints — `declare -f g` — whether or not there is
  an attribute to report.
- `declare -f g` is the body; `declare -fp g` is the body followed by that same
  line, but **only** when the function carries an attribute (a plain function's
  body has already said everything).
- A name that is no function is silent without the `-p` and
  `TAG: NAME: not found` with it, under the name the command was written by
  (`typeset -Fp nope` says `typeset:`). `declare -Fp v` for a *variable* `v`
  likewise says `not found`.
- extdebug decorates the bare `-F g` alone (`g LINE FILE`); `-Fp g` has nowhere
  to put it.
- Every spelling selects it: `-Fp`, `-pF`, `-F -p`, `-p -F`, `-F +p`, `-Fpg`,
  `-Ffp`, `-Fp -- g`.
- `+f` does **not** enter function mode — only a minus-signed `-f`/`-F` does —
  so `declare +ftp q` is a *variable* listing and says `declare: q: not found`.

**Fixed 2026-08-04.** `declare_functions` took a `print` parameter, its mutation
branch gained `&& !print`, and its named loop was rewritten from
accumulate-then-write to write-per-name so each operand is answered where it is
reached: with the two streams merged, `declare -fp g nope h` reads `g`'s body,
then the complaint, then `h`'s, which is the order bash's single loop gives.
The `-t` trace route in the dispatch arm gained `&& !Self::declare_wants_print(args)`.

**Standing lesson:** when a flag is handled on one route, ask whether the
sibling routes see it too. The `p` here was not a variable-side detail — it ran
before the route was even chosen.
