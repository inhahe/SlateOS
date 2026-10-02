### TD-OILS-BASH-SUBSHELL-ASSIGNABLE. `BASH_SUBSHELL` discards an assignment; bash lets it move the counter — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the dynamic-special table row
for `BASH_SUBSHELL`, whose `DynAssign` is `Discard`, and whose value
function returns the current subshell depth outright.

bash's `BASH_SUBSHELL` is not a read-only reporter: it is the counter
itself, so an assignment moves it and every subshell entered afterwards
counts up from the new value. A non-numeric value reads as 0, the way
`atoi` does.

```sh
echo "[$BASH_SUBSHELL]"                        # both [0]
BASH_SUBSHELL=9
echo "[$BASH_SUBSHELL]"                        # bash [9]   osh [0]
( echo "[$BASH_SUBSHELL]"; ( echo "[$BASH_SUBSHELL]" ) )   # bash [10] [11]   osh [1] [2]
( BASH_SUBSHELL=zz; echo "[$BASH_SUBSHELL]" )  # bash [0]   osh [1]
declare -p BASH_SUBSHELL                       # bash declare -- BASH_SUBSHELL="9"   osh "0"
```

`unset BASH_SUBSHELL` already agrees (both leave the name empty), and the
unassigned readings agree, so this is exactly the assignment.

**Fixed 2026-07-31.** A `subshell_base: i64` is added to
[`Shell::subshell_depth`] to give the reported value, and an assignment
sets it to the number given *minus the depth the assignment happened at*
— which is what "the assignment sets the counter" means. The base is
copied into a subshell's clone alongside `seconds_base`, so the number
given is what that level reads and each level further in reads one more,
while the counter a subshell moves stays the subshell's own. The row
keeps `DynAssign::Discard`: bash's assign function for this name does
*not* fill the value cell (only a reading does), which is exactly why a
pristine `BASH_SUBSHELL=7; BASH_SUBSHELL+=5` gives 5 and not 75.

The measurement also turned up a second rule, now modelled by a new
`DynamicSpecial::assign_evals` column: the integer attribute reaches the
value of a *non-appending* assignment only for the names whose own assign
function consults it. bash's `assign_seconds`/`assign_random` (and
`HISTCMD`/`SRANDOM`) do; `BASH_SUBSHELL` and `LINENO` do not, so
`declare -i BASH_SUBSHELL=3+4` stores 0 where the same on `SECONDS`
stores 7. An *appending* assignment is arithmetic either way, because the
appended value is formed before the assign function is reached.

Covered by the unit test `assigning_bash_subshell_moves_the_depth_counter`
and the corpus case `bash-subshell-assign.sh`.
