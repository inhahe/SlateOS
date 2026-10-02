### TD-OILS-DECLARE-P-BULK-DYNAMICS. bulk `declare -p` (no names) omits the dynamic special variables — 2026-07-19 — ✅ RESOLVED 2026-07-31

**What:** `declare -p NAME` for a scalar dynamic special variable now
matches bash (e.g. `declare -i BASHPID="12345"`, `declare -- LINENO="1"`
— implemented via `format_scalar_dynamic_declare` in `interp.rs`). But the
*bulk* listing `declare -p` with no name operands still only enumerates
`self.vars` / `self.arrays` / `self.assoc`, so it does not print the
special variables at all. bash lists them there in an attribute-only form
(no value), e.g. `declare -i BASHPID`, `declare -- LINENO`,
`declare -a BASH_LINENO=()`, `declare -i SRANDOM`.

**Reproduce:** `bash -c 'declare -p' | grep BASHPID` prints
`declare -i BASHPID`; `osh -c 'declare -p' | grep BASHPID` prints nothing.

**Why deferred (not a band-aid):** matching the bulk listing byte-for-byte
means enumerating bash's full set of always-present special variables
(including ones osh does not model at all, e.g. `SRANDOM`, `SHLVL`
attribute quirks, `PIPESTATUS`, `COMP_*`) and reproducing the
attribute-only "invisible variable" form (name with flags but no `=`).
That is a large surface for a listing that scripts almost never parse
(callers use `declare -p NAME`, which is now correct). Emitting a partial
subset would diverge from bash in a *different* way than omitting them.

**Proper fix:** add a curated table of always-present special variables
with their fixed attribute flags and "has a live value vs. attribute-only"
status, and have the no-names branch of `declare_print` merge that table
into the enumeration (skipping any that are shadowed by a real `self.vars`
entry). Verify the union and ordering against `bash -c 'declare -p'`.

**Fixed (2026-07-31):** as proposed, with a wider blast radius than the
entry expected — the omission was not specific to `declare -p`. *Every*
listing that walks the variable tables missed these names, so `declare -i`
did not list `BASHPID`/`RANDOM`, `declare -a` did not list `BASH_SOURCE`,
and `readonly -p` did not list `PPID`.

The curated table is now the single source of truth for the whole family:
`Shell::DYNAMIC_SPECIALS: &[DynamicSpecial]` in `interp.rs`, one row per
variable carrying `named_flags` (the letters `declare -p NAME` prints),
`listed_flags` (the letters a *listing* prints) and a `DynListing`
(`Bare` / `EmptyArray` / `Live`) saying how the listed line ends. It
replaces the old `DYNAMIC_SPECIAL_NAMES` name-only list, which fed
`${!prefix*}` alone — the names and the attribute letters used to live
apart, which is exactly why only one of them was ever wired in.

Wiring: `declare_p_names` chains the table in (`dedup` folds away any name
a real binding has since shadowed); `format_declare_def` split into
`format_declare_def_stored` plus a named-form (`format_dynamic_special_declare`,
live value) and a listed-form (`format_dynamic_special_listing`) tail;
`listing_names` gained `listed_has_attr`, which reads the letters from the
table for a dynamic name; `listing_kind_admits` counts a table row with
`a` as an indexed array; `readonly -p` chains in
`dynamic_special_names_with_attr('r')`.

Every column is measured against bash 5.2, including two forms that look
like bugs and are not: the listing prints *no* value (bash lists a
variable's stored value, and a computed one has none), and `SECONDS` loses
its `i` in a listing while keeping it for `declare -p SECONDS`. `PPID` is
the one row that lists with a value, because in bash it is an ordinary
binding made once at startup rather than a computed variable.

Result: the `declare -p` name set went from **21 names missing** vs. bash
to **6**, none of them modelled by osh at all (`BASH_ARGC`/`BASH_ARGV` —
extdebug-only here; `BASH_LOADABLES_PATH`, `COMP_WORDBREAKS`; `GROUPS`,
tracked under TD-OILS-MISSING-SPECIAL-ARRAYS; and `FUNCNAME`, which bash
lists bare at the top level and osh lists correctly inside a function).
Zero names are osh-only. Deliberately not faked: a name that
lists but does not expand would be a worse lie than an absent one. Test:
`listings_report_dynamic_special_variables`.

Three of the names the diff turned up were not listing bugs at all but
missing features, fixed separately:

* `OPTIND`/`OPTERR` — bash has both bound from startup; osh created them
  only inside `getopts`, so `$OPTIND` was empty and the standard
  `shift $((OPTIND - 1))` preamble shifted `-1` in a script that reached
  it without calling `getopts`. Seeded in `seed_shell_vars`. Test:
  `optind_and_opterr_are_bound_from_startup`.
* `SRANDOM` (bash 5.1+) — 32 bits from the system entropy source, a
  separate generator from `$RANDOM` with no seed, so assignments to it are
  swallowed. Implemented via `/dev/urandom` (the kernel CSPRNG on
  SlateOS), with a SplitMix64 fallback for hosts without the device; see
  design-decisions.md. Test: `srandom_is_a_seedless_32_bit_random`.

**Not replicated (a bash implementation artifact, deliberately):** bash's
listing prints a *stale cache*. Reading or assigning a dynamic variable
writes the value back into the variable struct, so `x=$SECONDS; declare -p`
prints `declare -i SECONDS="0"` — the value from the read, frozen, and
with an `i` the untouched form does not show — and `RANDOM=7; echo $RANDOM;
declare -p` lists the last number generated, not the seed. Reproducing
that would mean caching every computed value purely so a listing can print
a stale one. osh always lists the untouched (startup) form, which is what
bash shows for a script that has not touched the variable.
