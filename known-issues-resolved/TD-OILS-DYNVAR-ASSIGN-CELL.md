### TD-OILS-DYNVAR-ASSIGN-CELL. Assigning to a dynamic special variable does not fill its value cell — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::dyn_touched` covers
the lookup half of bash's value cell; the assignment half is missing.
Split out of TD-OILS-DYNVAR-INVISIBLE.

Each of these names has a slot whose value cell is filled in by a
lookup — that is TD-OILS-DYNVAR-INVISIBLE, now fixed. An **assignment**
fills it in too, but differently: it stores the string it was given and
leaves the attributes alone. So `SECONDS=7` makes the name valued
without making it an integer, which is exactly the shape bash reports:

```
                declare -p             declare -i             set
SECONDS=7       declare -- SECONDS=7   —                      SECONDS=7
RANDOM=7        declare -i RANDOM=7    declare -i RANDOM=7    RANDOM=7
HISTCMD=7       declare -i HISTCMD=7   declare -i HISTCMD=7   HISTCMD=7
SRANDOM=7       declare -i SRANDOM=7   declare -i SRANDOM=7   SRANDOM=7
```

osh reports the pristine, unvalued form for all four. (`RANDOM` and the
rest already carry `-i` in their slot, so only the value is missing
there; `SECONDS` needs the value *without* the `-i`, which is the case
`dyn_touched` alone cannot express — a second bit, or a real
`Option<Str>` cell per row.)

The mirror image is the three names bash gives **no assignment hook** —
`BASH_SUBSHELL`, `EPOCHSECONDS`, `EPOCHREALTIME`. bash discards the
assignment outright: after `EPOCHSECONDS=7` the slot is still empty
(`declare -- EPOCHSECONDS`, absent from `set`) and `$EPOCHSECONDS` still
computes. osh stores the 7 and shadows the value function with it. That
half needs a `stores` column in `DYNAMIC_SPECIALS` and a check in the
assignment path.

The same cell explains `+=` on these names, which is why that is filed
here rather than on its own: bash appends to *the cell*, not to the
computed value, and the concatenation is what the assignment hook then
receives. `SECONDS=100; SECONDS+=5` gives **1005** (string append to the
`"100"` the assignment stored, with no `-i` in the way), while
`SECONDS=100; : $SECONDS; SECONDS+=5` gives **105** — the read refilled
the cell *and* set `-i`, so the append became arithmetic. osh gives 100
for both. `SECONDS=7; SECONDS+=abc` gives 0 in bash (the hook cannot
parse `7abc`) against `abc` in osh.

**Fixed.** The cell is now real: `Shell::dyn_cell`, a
`RefCell<HashMap<&'static str, Str>>` beside the `dyn_touched` bitmask
that holds the attribute half. A lookup fills both (`dynamic_special_value`
stores what it computed); an assignment fills only the value. Every listing
that walks the variable table — bare `declare -p`, the flag-filtered forms,
bare `set` — now prints *the cell* through `dynamic_special_listed_value`
rather than taking a fresh reading, so it goes stale exactly where bash's
does:

```sh
SECONDS=0; sleep 2; declare -p          # declare -- SECONDS="0"   (the cell)
SECONDS=0; sleep 2; declare -p SECONDS  # declare -i SECONDS="2"   (a lookup)
```

…and the lookup refills the cell as it goes, so the two agree again
afterwards. `unset` empties it along with the rest of the binding.

`DynamicSpecial` gained an `assign: DynAssign` column recording what an
assignment does with the string — `Cell` for `SECONDS`, `RANDOM`,
`SRANDOM`, `LINENO` and `HISTCMD`, `Discard` for `BASHPID`, `BASH_ARGV0`,
`BASH_SUBSHELL`, `EPOCHSECONDS`, `EPOCHREALTIME` and the two call-stack
arrays. Either way the string never becomes a `Shell::vars` entry now,
which is what used to shadow the value function: after `LINENO=7`,
`$LINENO` reports the real line again, and `EPOCHSECONDS=7` leaves the
clock alone.

What is stored is the **number**, never the text as typed, and which
parse it is depends on the slot's `-i` (`Shell::dyn_integer_attr`) — the
same attribute that decides what `+=` means. `Shell::dyn_assigned_number`
does both: an arithmetic expression with the `-i` and numeric addition for
`+=`, a plain decimal without it (bash's `legal_number`: blanks around a
whole number and nothing else, everything else 0) and string append for
`+=`, the concatenation then read the same way. So `SECONDS=3+4` stores 0
but `: $SECONDS; SECONDS=3+4` stores 7; `SECONDS=100; SECONDS+=5` gives
1005 and `: $SECONDS; SECONDS=100; SECONDS+=5` gives 105; and
`SECONDS=7; SECONDS+=abc` gives 0. `Shell::seconds_base` became `i64` on
the way, because bash counts on from a negative number as readily as a
positive one (`SECONDS=-3` reads `-3`, then `-2`).

One measured oddity is preserved: a bad expression assigned to one of
these names prints its diagnostic, leaves the slot untouched and then
*carries on* with a status of 0 — `RANDOM=1/0; echo $?` prints the error
and `0` — where the same expression assigned to an ordinary `-i`
variable abandons the script.

Covered by `tests/corpus/dynamic-var-assign.sh` and the unit tests
`assigning_a_dynamic_variable_fills_its_value_cell_only` /
`assigning_a_dynamic_variable_stores_the_number_not_the_text`.
