### TD-OILS-FUNCNAME-NOT-LISTED-AT-TOP-LEVEL. `declare -p FUNCNAME` says "not found" outside a function — 2026-08-03 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::DYNAMIC_SPECIALS` (~13156),
which deliberately omits `FUNCNAME`, and `Shell::refresh_funcname` (~14553),
which removes `arrays["FUNCNAME"]` whenever no function frame is active.

**What:** bash keeps `FUNCNAME` present-but-empty at every level, so at a
script's top level:

```sh
declare -p FUNCNAME   # bash: declare -a FUNCNAME        osh: FUNCNAME: not found (rc 1)
declare -p | grep -c FUNCNAME   # bash: 1                osh: 0
declare -a | grep FUNCNAME      # bash: declare -a FUNCNAME
declare -n FUNCNAME=t # bash: FUNCNAME: reference variable cannot be an array
                      #       osh: silent rc 1 (nothing there to refuse)
```

The expansion side is already right — `${FUNCNAME[@]}` is empty at top level and
correct inside a function — so this is purely about the name being *listed* and
being visible to the checks that walk the array tables. The
`DYNAMIC_SPECIALS` doc comment states that bash does not list `FUNCNAME` outside
a function, which is not what bash 5.2.37 does.

**Fixed in `58f03723d`.** `FUNCNAME` has a `DYNAMIC_SPECIALS` row now, but
not the `DynListing::EmptyArray` the note above guessed at: bash builds
`BASH_SOURCE` and `BASH_LINENO` by *assigning* them an empty array, and creates
`FUNCNAME` without ever assigning it, so the first two print `=()` and the third
prints bare. That is the same never-assigned state a `declare -a q` leaves
behind, so the new `DynListing::BareArray` says exactly that, and the four sites
that asked "is this listing an array?" go through `DynListing::is_array()`
instead of comparing against `EmptyArray`.

Three consequences fell out of the name being present at the top level, where it
has no `arrays` entry to read anything off:

* It is *invisible*, in bash's sense — reported by `declare -p`, passed over by
  the value listings. `dynamic_special_visible_names` is what
  `Shell::visible_var_names` chains, and it drops a `BareArray`; see
  TD-OILS-NAME-ENUMERATION-LISTS-INVISIBLE-NAMES for the other half of that rule.
* `declare -A FUNCNAME` is a *conversion*, so `array_kind_conflict` now asks
  `dynamic_special_listed` as well as the `arrays` table (bash: `cannot convert
  indexed to associative array`, rc 1, name unchanged).
* `unset FUNCNAME` drops the binding for good — bash never rebuilds it, so a
  later call finds nothing there — which `refresh_funcname` honours by checking
  `dyn_unset` before writing the array. bash refuses to unset the other two at
  all, so only `FUNCNAME` can reach that state.

`${FUNCNAME+set}` is still empty at the top level as the note asked: the
listing and the expansion genuinely disagree there, and a function call settles
them. Corpus case
`a-a-funcname-is-present-and-empty-outside-a-function.sh`; lib test
`funcname_is_present_and_empty_outside_a_function`.
