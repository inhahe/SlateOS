# Entering posix mode turns on two `shopt` options — `inherit_errexit`, which
# makes a command substitution's subshell inherit `set -e`, and `shift_verbose`,
# which makes `shift` complain when the count is larger than `$#`.
#
# It is a **transition**, not a value derived from the mode. Three things follow
# from that, and each is checked below:
#
#   * Leaving posix mode clears only `shift_verbose`. `inherit_errexit` is
#     sticky — once posix mode has turned it on, `set +o posix` leaves it on.
#   * A `shift_verbose` the script had set by hand is cleared all the same when
#     the mode goes off, because nothing remembers who set it.
#   * While the mode stays on, turning either option back off sticks. Only
#     leaving and re-entering turns `shift_verbose` on again.
#
# And the mode is entered by every route, not just `set -o posix`: assigning
# `POSIXLY_CORRECT` enters it, unsetting the variable leaves it, and a `local`
# copy going out of scope leaves it too.

echo "=== off by default, on with the mode"
shopt -p inherit_errexit shift_verbose
( set -o posix; shopt -p inherit_errexit shift_verbose )

echo "=== leaving clears only shift_verbose"
( set -o posix; set +o posix; shopt -p inherit_errexit shift_verbose )

echo "=== …even one the script had set by hand"
( shopt -s shift_verbose; set -o posix; set +o posix; shopt -p shift_verbose )
( shopt -s inherit_errexit; set -o posix; set +o posix; shopt -p inherit_errexit )

echo "=== turning them back off while the mode is on sticks"
( set -o posix; shopt -u inherit_errexit; shopt -p inherit_errexit )
( set -o posix; shopt -u shift_verbose; shopt -p shift_verbose )

echo "=== but leaving and re-entering turns shift_verbose on again"
( set -o posix; shopt -u shift_verbose; set +o posix; set -o posix; shopt -p shift_verbose )

echo "=== the variable spellings of the mode"
( POSIXLY_CORRECT=1; shopt -p inherit_errexit shift_verbose )
( export POSIXLY_CORRECT=1; shopt -p inherit_errexit shift_verbose )
( POSIXLY_CORRECT=1; unset POSIXLY_CORRECT; shopt -p inherit_errexit shift_verbose )
f() { local POSIXLY_CORRECT=1; shopt -p shift_verbose; }
f; shopt -p shift_verbose

# …and that `f` just left a mark on *this* shell: the posix mode its `local`
# turned on lasted only until the function returned, but the `inherit_errexit`
# that mode set is sticky and is still on out here.
echo "--- and f left inherit_errexit behind"; shopt -p inherit_errexit
shopt -u inherit_errexit

echo "=== what shift_verbose actually does"
( shift 5; echo "  quiet rc=$?" )
( shopt -s shift_verbose; shift 5; echo "  rc=$?" )
( shopt -s shift_verbose; shift; echo "  bare rc=$?" )
( shopt -s shift_verbose; set -- a b c; shift 2; echo "  in range rc=$?" )
( set -o posix; shift 5; echo "  via the mode rc=$?" )
( set -o posix; shopt -u shift_verbose; shift 5; echo "  opted back out rc=$?" )

echo "=== what inherit_errexit actually does"
( set -e; v=$(false; echo reached); echo "  v=$v rc=$?" )
( set -o posix; set -e; v=$(false; echo reached); echo "  v=$v rc=$?" )

echo "=== and both show up in the listings"
( set -o posix; shopt | grep -E 'inherit_errexit|shift_verbose' )
( set -o posix; echo "$BASHOPTS" | tr : '\n' | grep -E 'inherit_errexit|shift_verbose' )
