# `for` and `select` expand a word list of their own before anything else the
# loop does, and bash never returns from a failure in one. Both go through
# `expand_words_no_vars` (execute_cmd.c), which `jump_to_top_level`s straight
# out of the loop — so the `list_len == 0` test just after it is never even
# reached, no menu is printed, no body runs, and the rest of the parse unit
# goes with it.
#
# The two abort depths unwind differently, as they do everywhere else:
#
#   * a *discard* — a bad array subscript in the list, or a `$( … )` whose
#     re-print will not parse back — drops one parse unit, leaves `$?` at 1,
#     and the shell reads on with the *next* unit;
#   * a nounset reference under `set -u`, or a `${x:?word}`, ends the shell
#     with the status it carried.
#
# Whether a loop that failed to answer for this showed depends on what the
# list expanded *to*. A list with anything left in it runs a body, and the
# body's own driver consumes the pending flag, so the two agree by accident.
# A list that came out empty runs no body at all — and then the flag outlives
# the loop, and the parse unit, and is spent on a command in the next one.
# That is the shape below that has a line after it to lose.
#
# `select` needs the answer sooner than `for` does, because it has side
# effects before any body runs: it prints the numbered menu and the `PS3`
# prompt, and then blocks reading a reply.

echo "=== a for list with a live word left still ends at the abort"
( for i in a "${nope?bad}" c; do echo "body $i"; done; echo "not reached" )
echo "for rc=$?"

echo "=== and a select list does too, without printing its menu"
( select i in a "${nope?bad}" c; do echo "body $i"; done; echo "not reached" )
echo "select rc=$?"

echo "=== a list that aborts to nothing has no body to catch it for it"
for i in ${b[0=1]}; do echo "body $i"; done
echo "for after rc=$?"
select i in ${c[0=1]}; do echo "body $i"; done
echo "select after rc=$?"

echo "=== the discard still only costs the unit it was raised in"
for i in ${d[0=1]}; do echo "body $i"; done; echo "not reached"
echo "after rc=$?"

echo "=== set -u reaches both lists the same way"
( set -u; for i in a $nope c; do echo "body $i"; done; echo "not reached" )
echo "for rc=$?"
( set -u; select i in a $nope c; do echo "body $i"; done; echo "not reached" )
echo "select rc=$?"

echo "=== an empty list is not an aborted one: it is a silent success"
for i in $unset_but_fine; do echo "body $i"; done
echo "for rc=$?"
select i in $unset_but_fine; do echo "body $i"; done
echo "select rc=$?"

echo "=== and a list that expands cleanly still runs"
for i in a b; do echo "body $i"; done
echo "for rc=$?"
